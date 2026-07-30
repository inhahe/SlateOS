//! `!`-style history expansion — the transformation `set -H` / `set -o
//! histexpand` gates.
//!
//! bash performs this on a complete input line *before* the line is broken into
//! words, which makes it unlike every other expansion the shell does: it is a
//! textual rewrite over raw source, it happens before quoting is interpreted,
//! and consequently **single quotes do not protect a `!` the way they protect
//! everything else** — only a backslash and a *surrounding* single-quoted region
//! that the scanner itself tracks do. See `known-issues.md`
//! TD-OILS-NO-HISTEXPAND for the measured bash model this implements.
//!
//! This module is deliberately pure: it takes a line plus a view of the history
//! and returns the rewritten line. Deciding *when* to call it (per physical
//! line, before lexing) is the caller's job, because expansion has to interleave
//! with reading — see the design notes in `known-issues.md`.

/// The history a line is expanded against.
///
/// `entries` is oldest-first, and `base` is the event number readline would
/// give `entries[0]` — the same pair the `history` builtin lists from, so an
/// absolute `!n` means `entries[n - base]`.
#[derive(Clone, Copy)]
pub struct HistCtx<'a> {
    pub entries: &'a [String],
    pub base: usize,
    /// Whether `shopt -s extglob` is in effect, which makes `!(` the opening of
    /// a negated extended-glob pattern rather than an event designator. See
    /// [`inhibited`].
    pub extglob: bool,
    /// The quote the reader is already *inside* when this line is expanded, for
    /// the continuation lines of a multi-line command — readline's
    /// `history_quoting_state`, which bash sets from its delimiter stack before
    /// each `history_expand` call. `Some('\'')` makes the whole line inert;
    /// `Some('"')` keeps expansion live but suppresses the `#`-comment rule and
    /// leaves `\!` as two characters. `None` for a first line, and for callers
    /// with no reader context (`history -p`, which bash likewise expands with a
    /// cleared state).
    ///
    /// Without this a quote is silently closed at each newline, so `echo 'a`
    /// followed by `!!'` expands a `!!` that bash passes through literally.
    /// See [`crate::lexer::open_quote`], which computes it.
    pub open_quote: Option<char>,
}

impl HistCtx<'_> {
    /// The entry numbered `n`, or `None` if `n` is outside the retained range.
    fn by_number(&self, n: usize) -> Option<&str> {
        n.checked_sub(self.base)
            .and_then(|i| self.entries.get(i))
            .map(String::as_str)
    }

    /// The `n`th entry counting back from the most recent, where 1 is the most
    /// recent (`!-1`, equivalently `!!`).
    fn back(&self, n: usize) -> Option<&str> {
        self.entries
            .len()
            .checked_sub(n)
            .and_then(|i| self.entries.get(i))
            .map(String::as_str)
    }

    /// The most recent entry starting with `prefix`.
    fn by_prefix(&self, prefix: &str) -> Option<&str> {
        self.entries
            .iter()
            .rev()
            .find(|e| e.starts_with(prefix))
            .map(String::as_str)
    }

    /// The most recent entry containing `needle`.
    fn by_substring(&self, needle: &str) -> Option<&str> {
        self.entries
            .iter()
            .rev()
            .find(|e| e.contains(needle))
            .map(String::as_str)
    }
}

/// What [`expand`] made of a line.
#[derive(Debug)]
pub enum Expansion {
    /// The line contained nothing to expand and is to be used as-is.
    Unchanged,
    /// The line was rewritten. bash echoes the result before running it.
    Changed(String),
    /// The line was rewritten but the rewrite is *not* reported. readline
    /// implements `^old^new^` by textually prefixing `!!:s`, and it counts only
    /// the expansion proper as a change — so a line whose `!!` turns out to be
    /// quoted runs (and is recorded) as the literal `!!:s^old^new^` with nothing
    /// echoed to stderr. bash adopts `history_expand`'s output string either way,
    /// which is what this variant carries.
    ChangedQuietly(String),
    /// The line carried a `:p` modifier: bash echoes the rewritten line and
    /// records it in the history exactly as for [`Expansion::Changed`], but does
    /// *not* run it — `:p` exists to preview what an event designator names.
    PrintOnly(String),
    /// The line could not be expanded — an event designator named something not
    /// in the history, or a `:s` modifier whose pattern did not match. bash
    /// reports this on stderr and discards the line without running it. The
    /// payload is the whole message body, since bash words the three cases
    /// differently: `!999: event not found`, `:s/a/b/: substitution failed`,
    /// `:&: no previous substitution`.
    NotFound(String),
}

/// readline's `history_word_delimiters` — what ends a word when nothing is
/// quoting it. Note that it holds the shell's operator characters as well as
/// whitespace, which is why an operator becomes a word of its own.
const WORD_DELIMITERS: &[u8] = b" \t\n;&()|<>";

/// The characters which, immediately followed by `(`, open a span that readline
/// scans through to the matching `)`: command and process substitution and the
/// extended-glob groups.
const SUBST_OPENERS: &[u8] = b"<>$!@?+*";

/// The byte at `i`, or `None` past the end.
///
/// Every character the tokenizer tests for is ASCII, and no byte of a
/// multi-byte UTF-8 sequence is ever ASCII, so scanning bytes can neither match
/// inside a character nor split one: the indices handed back to [`words`] are
/// always character boundaries.
fn byte_at(s: &[u8], i: usize) -> Option<u8> {
    s.get(i).copied()
}

/// Split a command into words the way history expansion counts them — a port of
/// readline's `history_tokenize_word`, which is what `!!:1`, `!$`, `!*` and the
/// rest are numbering.
///
/// This is emphatically *not* a whitespace split. `history_word_delimiters`
/// includes the shell's operator characters, so each operator is a word of its
/// own, and the scanner knows enough shell syntax to keep quoted and
/// substituted spans intact. The rules below were measured against bash 5.2
/// rather than read off the source, because several of them are surprising:
///
/// * `(`, `)` and a newline are always one-character words, wherever they sit.
/// * A doubled operator is one two-character word — so `;;;` is `;;` then `;`,
///   and `&&&&` is `&&` then `&&`. `<<<` and `<<-` are three characters, but
///   `>>-` is `>>` then `-`, since the third-character rule is `<`-only.
/// * `>&`, `<&`, `&>` and `>|` absorb trailing digits and `-`, so `2>&1` and
///   `>&-` are each one word, and `>&12y` is `>&12` then `y`.
/// * Its neighbours are *not* grouped: `<>`, `<|`, `&<`, `&|`, `|&`, `;&` and
///   `;|` all split.
/// * A leading digit run counts as a file descriptor only at the start of a
///   word and only before `<` or `>`: `2>&1` is one word but `a2>b` is `a2`,
///   `>`, `b`, and `12ab;c` is `12ab`, `;`, `c`.
/// * `'…'`, `"…"`, `` `…` ``, `$'…'`, `$"…"` and `$( … )` protect delimiters
///   and keep their quotes; an unterminated one swallows the rest of the line.
///   `${ … }` does *not* protect, so `${a;b}` is `${a`, `;`, `b}`.
/// * A `#` that starts a word ends the line: `echo a # b c` has two words.
fn words(s: &str) -> Vec<&str> {
    word_spans(s).into_iter().filter_map(|(a, b)| s.get(a..b)).collect()
}

/// The byte ranges of the words of `s`, which is [`words`] before the slicing.
///
/// Kept separate because a `%` word designator has to answer "which word is byte
/// *n* in?", and the whitespace between two words is in neither.
fn word_spans(s: &str) -> Vec<(usize, usize)> {
    let bytes = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0usize;
    loop {
        // Only spaces and tabs are skipped between words; a newline is a word.
        while matches!(byte_at(bytes, i), Some(b' ' | b'\t')) {
            i = i.saturating_add(1);
        }
        // `history_comment_char` at the start of a word ends the tokenization
        // outright — the rest of the line is not words at all. One inside a
        // word (`a' #'b`) is just a character, since this test is only reached
        // between words.
        if matches!(byte_at(bytes, i), None | Some(b'#')) {
            return out;
        }
        let start = i;
        i = tokenize_word(bytes, start);
        // Every branch of `tokenize_word` consumes at least one byte, but a
        // zero-width word would spin forever, so refuse to accept one.
        if i <= start {
            return out;
        }
        out.push((start, i));
    }
}

/// The word bash remembers for a later `%`, given the line a `?string?` search
/// matched and the string it searched for.
///
/// readline scans the matched line *backwards* for the search string, so it is
/// the **last** occurrence that counts: `!?a?` against `xa ya za` remembers
/// `za`, not `xa`. The word is then the one holding the first character of that
/// match — which may be an operator word, since [`words`] makes those words of
/// their own (`!?; b?` against `a; b` remembers `;`), and which is nothing at
/// all when the match starts on the whitespace between two words (`!? b?`).
/// Measured against bash 5.2.
fn search_match(line: &str, needle: &str) -> String {
    let Some(at) = line.rfind(needle) else {
        return String::new();
    };
    word_spans(line)
        .into_iter()
        .find(|&(a, b)| at >= a && at < b)
        .and_then(|(a, b)| line.get(a..b))
        .unwrap_or_default()
        .to_string()
}

/// Scan the one word starting at `start`, returning the index just past it.
fn tokenize_word(s: &[u8], start: usize) -> usize {
    let mut i = start;

    // `(`, `)` and a newline stand alone.
    if matches!(byte_at(s, i), Some(b'(' | b')' | b'\n')) {
        return i.saturating_add(1);
    }

    // A leading digit run is a file descriptor when a redirection follows it.
    // Either way the scan resumes at the first non-digit, which is why the rule
    // fires only at the start of a word: in `a2>b` the digit is reached by the
    // ordinary scan below and gets no special treatment.
    if byte_at(s, i).is_some_and(|c| c.is_ascii_digit()) {
        let mut j = i;
        while byte_at(s, j).is_some_and(|c| c.is_ascii_digit()) {
            j = j.saturating_add(1);
        }
        // Digits running to the end of the line are the whole word.
        if byte_at(s, j).is_none() {
            return j;
        }
        i = j;
        if !matches!(byte_at(s, j), Some(b'<' | b'>')) {
            return scan_word(s, i);
        }
    }

    // The operators. Anything not matched here falls through to `scan_word`,
    // which will stop at it as a delimiter if it is one.
    if let Some(c) = byte_at(s, i).filter(|c| b"<>;&|".contains(c)) {
        let peek = byte_at(s, i.saturating_add(1));
        // A lone `<` or `>` before `(` is process substitution, and the ordinary
        // scan takes it through the matching `)`. Only a lone one: `>>(`, `<<(`
        // and `>&(` all keep their operator reading and leave the `(` behind as
        // a word. The other operators are not substitution openers at all, so
        // `&(`, `;(` and `|(` split too.
        if SUBST_OPENERS.contains(&c) && peek == Some(b'(') {
            return scan_word(s, i);
        }
        if peek == Some(c) {
            // A third character joins a doubled `<` only — `<<<` and `<<-` are
            // here-document operators, while `>>-` is `>>` and then a `-`.
            if c == b'<' && matches!(byte_at(s, i.saturating_add(2)), Some(b'-' | b'<')) {
                i = i.saturating_add(1);
            }
            return i.saturating_add(2);
        }
        // The pairs that name a file descriptor, and so absorb one.
        if matches!((c, peek), (b'&', Some(b'>')) | (b'>', Some(b'&' | b'|')) | (b'<', Some(b'&'))) {
            i = i.saturating_add(2);
            while byte_at(s, i).is_some_and(|c| c.is_ascii_digit() || c == b'-') {
                i = i.saturating_add(1);
            }
            return i;
        }
        return i.saturating_add(1);
    }

    scan_word(s, i)
}

/// Scan an ordinary word from `i` — readline's `get_word` tail, which tracks
/// one open span at a time and stops at the first unquoted delimiter.
fn scan_word(s: &[u8], mut i: usize) -> usize {
    // The byte that will close the span we are in, and — for `$( … )` — the
    // matching opener, so that a nested one can be counted.
    let mut delimiter: Option<u8> = None;
    let mut delimopen: Option<u8> = None;
    let mut nestdelim = 0usize;

    // A word that begins with a quote opens it immediately.
    if let Some(q) = byte_at(s, i).filter(|c| matches!(c, b'\'' | b'"' | b'`')) {
        delimiter = Some(q);
        i = i.saturating_add(1);
    }

    while let Some(c) = byte_at(s, i) {
        // A `\<newline>` is a continuation, and both characters belong to the
        // word. (A newline closes no span, so this needs no quote test.)
        if c == b'\\' && byte_at(s, i.saturating_add(1)) == Some(b'\n') {
            i = i.saturating_add(2);
            continue;
        }
        // Elsewhere a backslash escapes whatever follows — except inside `'…'`,
        // where nothing does. (readline also tests `slashify_in_quotes` for the
        // `"…"` case, but it tests the *backslash*, which is always a member,
        // so that arm never fires; and the only character whose escaping is
        // observable inside `"…"` is the closing `"`, which is a member anyway.)
        if c == b'\\' && delimiter != Some(b'\'') {
            i = i.saturating_add(2);
            continue;
        }
        // `$( … )` and friends. Quotes win, so this is inert inside `'…'` and
        // `"…"` — but one *inside* such a span nests, which is what keeps
        // `$(a $(b;c) d)` a single word.
        if (delimiter.is_none() || delimiter == Some(b')'))
            && SUBST_OPENERS.contains(&c)
            && byte_at(s, i.saturating_add(1)) == Some(b'(')
        {
            delimopen = Some(b'(');
            delimiter = Some(b')');
            nestdelim = nestdelim.saturating_add(1);
            // readline steps past the two-character opener and *then* takes its
            // loop's own increment, so the character right after `$(` is never
            // examined. That is not cosmetic: it is exactly why `$((a;b))` ends
            // at the first `)` — the inner `(` is skipped, so it never nests —
            // while in `$(a (b;c) d)` the `(` *is* seen and does.
            i = i.saturating_add(3);
            continue;
        }
        // A second opener inside `$( … )` deepens it.
        if delimiter.is_some() && Some(c) == delimopen {
            nestdelim = nestdelim.saturating_add(1);
            i = i.saturating_add(1);
            continue;
        }
        if Some(c) == delimiter {
            nestdelim = nestdelim.saturating_sub(1);
            if nestdelim == 0 {
                delimiter = None;
                delimopen = None;
            }
            i = i.saturating_add(1);
            continue;
        }
        if delimiter.is_none() {
            if WORD_DELIMITERS.contains(&c) {
                break;
            }
            // A quote opened mid-word protects up to its close, and stays part
            // of the word: `a'b;c'd` is one word, quotes and all.
            if matches!(c, b'\'' | b'"' | b'`') {
                delimiter = Some(c);
                delimopen = None;
            }
        }
        i = i.saturating_add(1);
    }
    // Two branches above step over more than one byte, so a line ending mid-way
    // through a `\x` or a `$(` leaves `i` past the end. bash keeps the truncated
    // word (`echo $(` has two words, the second `$(`), so clamp rather than let
    // the caller's slice fail and drop it.
    i.min(s.len())
}

/// Apply a `:h`/`:t`/`:r`/`:e` pathname modifier.
///
/// These are readline's, which are deliberately naive: each is one `strrchr`
/// over the *whole* selected text, with no notion of a basename and no special
/// case for a leading separator. Two consequences are worth spelling out, both
/// measured against bash 5.2 rather than inferred:
///
/// * When the separator is absent the text comes back **unchanged**, not empty
///   — `echo abc` is `echo abc` under both `:h` and `:e`.
/// * A dot anywhere counts as the extension separator, even one inside a
///   directory name or at the very start: `echo a.b/c` under `:e` is `.b/c`,
///   and the word `.abc` under `:r` is empty.
fn modify_path(text: &str, which: char) -> String {
    match which {
        // head: everything before the last `/` — so a leading slash leaves
        // nothing at all, and `/abc` is empty rather than `/`.
        'h' => match text.rfind('/') {
            Some(i) => text.get(..i).unwrap_or("").to_string(),
            None => text.to_string(),
        },
        // tail: everything after the last `/`.
        't' => match text.rfind('/') {
            Some(i) => text.get(i.saturating_add(1)..).unwrap_or("").to_string(),
            None => text.to_string(),
        },
        // root: everything before the last `.`.
        'r' => match text.rfind('.') {
            Some(i) => text.get(..i).unwrap_or("").to_string(),
            None => text.to_string(),
        },
        // ext: the last `.` and everything after it.
        'e' => match text.rfind('.') {
            Some(i) => text.get(i..).unwrap_or("").to_string(),
            None => text.to_string(),
        },
        _ => text.to_string(),
    }
}

/// Apply an `s/old/new/` substitution to `text`, replacing the first occurrence
/// (or every occurrence when `global`).
fn substitute(text: &str, old: &str, new: &str, global: bool) -> String {
    if old.is_empty() {
        return text.to_string();
    }
    if global {
        text.replace(old, new)
    } else {
        text.replacen(old, new, 1)
    }
}

/// What one history expansion leaves behind for the next one.
///
/// bash keeps all of this for the life of the *shell*, not of one expansion, so
/// the caller owns it and threads it through every [`expand`] call. Two
/// independent pairs live here:
///
/// * the most recent `s/old/new/`, so a `:&` on a later line can repeat it and
///   an empty pattern (`:s//new/`) can reuse its `old`;
/// * the most recent `?string?` event search — both the string, which an empty
///   `!??` searches for again, and the *word* it matched, which is what a `%`
///   word designator expands to.
///
/// Only a **successful** search updates the search pair: after a `!?nope?` that
/// found nothing, `!??` still searches for the string before it and `%` still
/// names the word that one matched. A search that succeeds updates them even if
/// the rest of the expansion then fails, because the record is made when the
/// event is found and nothing rolls it back.
#[derive(Default, Clone)]
pub struct HistState {
    /// The `old` of the most recent `s/old/new/`.
    subst_old: String,
    /// Its `new`.
    subst_new: String,
    /// The string of the most recent successful `?string?` search.
    search_string: String,
    /// The word that search matched — bash's `search_match`. Empty when there
    /// has been no search, and also when the match began on whitespace, which
    /// belongs to no word.
    search_match: String,
}

/// Read a delimited `s`/`&` modifier body starting just past the `s`, e.g.
/// `/old/new/`. The trailing delimiter may be omitted at end of input. Returns
/// the parsed pair and the index just past what was consumed.
fn parse_subst(chars: &[char], mut i: usize) -> Option<(String, String, usize)> {
    let delim = *chars.get(i)?;
    i = i.saturating_add(1);
    let mut old = String::new();
    while let Some(&c) = chars.get(i) {
        i = i.saturating_add(1);
        if c == delim {
            break;
        }
        // A backslash escapes the delimiter inside the pattern.
        if c == '\\' && chars.get(i) == Some(&delim) {
            old.push(delim);
            i = i.saturating_add(1);
        } else {
            old.push(c);
        }
    }
    let mut new = String::new();
    while let Some(&c) = chars.get(i) {
        if c == delim {
            i = i.saturating_add(1);
            break;
        }
        i = i.saturating_add(1);
        if c == '\\' && chars.get(i) == Some(&delim) {
            new.push(delim);
            i = i.saturating_add(1);
        } else {
            new.push(c);
        }
    }
    Some((old, new, i))
}

/// Where a word range ends.
enum RangeEnd {
    /// Written out in the designator — a number, `^`, `$`, or the last word
    /// implied by a `*`. bash range-checks these against the event.
    Given(usize),
    /// Defaulted by a trailing `-` to the second-to-last word. bash lets this
    /// one fall before the start of the range without complaining, which is why
    /// `!!:3-` on a four-word event is empty while `!!:3-1` is an error.
    SecondToLast,
}

/// Select the words of `event` named by a word designator, returning the
/// selected text and the index just past the designator.
///
/// bash lets the `:` be omitted when the designator begins with `^`, `$`, `*`
/// or `-`, which is what makes `!$` and `!!:1` both work. A *number* needs the
/// colon, though: `!!1` is the whole event with a literal `1` stuck on it.
///
/// A designator that names a word the event does not have fails the whole
/// expansion, the way a missing event does — bash does not silently clamp. The
/// `Err` carries the message body, which quotes back the designator alone
/// (`!!:4:h` reports `:4: bad word specifier`, not `:4:h`).
///
/// The grammar was measured against bash 5.2 rather than taken from the manual,
/// which describes only `x-y`. Three corners it does not mention:
///
/// * `$` and `*` end the designator outright — no range is read after either, so
///   the `-` of `!!:$-` and of `!!:*-` is literal text. `^` does not stop the
///   scan (`!!:^-` is a range), and neither does a number (`!!:2-` is a range).
/// * a `^` may stand in for `-^` as the end of a range: `!!:0^` is words 0
///   through 1. A `$` gets no such treatment (`!!:2$` is word 2 and a literal
///   `$`), and neither does a number (`!!:^2` is word 1 and a literal `2`).
/// * `*` never fails, not even the start-of-range check that every other shape
///   is subject to: on a one-word event `!!:*` is empty where `!!:^` is an
///   error, though both reach for word 1.
///
/// `%` is the odd one out: it does not name a word of `event` at all but the
/// word a previous `?string?` search matched, which is what `search_match`
/// carries. Like `*` it never fails, and like `$` it ends the designator.
fn apply_word_designator(
    event: &str,
    chars: &[char],
    start: usize,
    search_match: &str,
) -> Result<(String, usize), String> {
    let ws = words(event);
    let last = ws.len().saturating_sub(1);
    let mut i = start;
    let had_colon = chars.get(i) == Some(&':');
    if had_colon {
        i = i.saturating_add(1);
    }

    // Parse the *end* of a range: a number, `^` (the first argument) or `$`
    // (the last word). Unlike the start of a range, a number here needs no
    // preceding colon — `!!-2` is words 0 through 2.
    let read_end = |chars: &[char], i: &mut usize| -> Option<usize> {
        match chars.get(*i) {
            Some('^') => {
                *i = i.saturating_add(1);
                Some(1)
            }
            Some('$') => {
                *i = i.saturating_add(1);
                Some(last)
            }
            Some(c) if c.is_ascii_digit() => {
                let mut n = 0usize;
                while let Some(d) = chars.get(*i).and_then(|c| c.to_digit(10)) {
                    n = n.saturating_mul(10).saturating_add(d as usize);
                    *i = i.saturating_add(1);
                }
                Some(n)
            }
            _ => None,
        }
    };

    // Join words `from..=to` — an empty range is an empty string, not an error.
    let join = |from: usize, to: Option<usize>| -> String {
        match to {
            Some(to) if from <= to => ws.get(from..=to).unwrap_or(&[]).join(" "),
            _ => String::new(),
        }
    };

    // Range-check and join. A start past the last word is always an error; a
    // written-out end must also be no later than the last word and no earlier
    // than the start.
    let take = |from: usize, to: &RangeEnd, end: usize| -> Result<(String, usize), String> {
        let bad = from > last
            || match *to {
                RangeEnd::Given(to) => to > last || to < from,
                RangeEnd::SecondToLast => false,
            };
        if bad {
            return Err(format!("{}: bad word specifier", spec_text(chars, start, end)));
        }
        let to = match *to {
            RangeEnd::Given(to) => Some(to),
            RangeEnd::SecondToLast => last.checked_sub(1),
        };
        Ok((join(from, to), end))
    };

    // A range start, once one of the leading special forms has been ruled out.
    // `$` is not here because it terminates the designator on its own.
    let read_start = |chars: &[char], i: &mut usize| -> Option<usize> {
        match chars.get(*i) {
            Some('^') => {
                *i = i.saturating_add(1);
                Some(1)
            }
            // A bare number is a word number only after a `:`.
            Some(c) if had_colon && c.is_ascii_digit() => read_end(chars, i),
            _ => None,
        }
    };

    match chars.get(i) {
        // `*` — all arguments, i.e. words 1 through the last. Alone among the
        // designators it never fails: on a one-word event it is simply empty.
        Some('*') => {
            i = i.saturating_add(1);
            Ok((join(1, Some(last)), i))
        }
        // `$` — the last word, and the end of the designator.
        Some('$') => {
            i = i.saturating_add(1);
            take(last, &RangeEnd::Given(last), i)
        }
        // `%` — the word the last `?string?` search matched, from whichever
        // event that was. Nothing about the event in hand is consulted, so no
        // range check applies and there is nothing to fail: with no prior
        // search this is simply empty.
        Some('%') => {
            i = i.saturating_add(1);
            Ok((search_match.to_string(), i))
        }
        // `-m` — words 0 through m.
        Some('-') => {
            i = i.saturating_add(1);
            let end = read_end(chars, &mut i).map_or(RangeEnd::SecondToLast, RangeEnd::Given);
            take(0, &end, i)
        }
        Some(_) => {
            let Some(from) = read_start(chars, &mut i) else {
                // Not a word designator after all; leave the whole event and let
                // the caller reconsider this position as a modifier.
                if had_colon {
                    i = start;
                }
                return Ok((event.to_string(), i));
            };
            match chars.get(i) {
                // `n*` — from n to the last word.
                Some('*') => {
                    i = i.saturating_add(1);
                    take(from, &RangeEnd::Given(last), i)
                }
                // `n^` — from n to the first argument; see the note above.
                Some('^') => {
                    i = i.saturating_add(1);
                    take(from, &RangeEnd::Given(1), i)
                }
                Some('-') => {
                    i = i.saturating_add(1);
                    let end =
                        read_end(chars, &mut i).map_or(RangeEnd::SecondToLast, RangeEnd::Given);
                    take(from, &end, i)
                }
                // A single word.
                _ => take(from, &RangeEnd::Given(from), i),
            }
        }
        // Nothing after the `:` at all. Like the non-designator case above, hand
        // the position back unconsumed so the modifier scan sees the `:` and
        // rejects it (bash: `: unrecognized history modifier`).
        None => Ok((event.to_string(), start)),
    }
}

/// Apply any trailing `:h`/`:t`/`:r`/`:e`/`:s`/`:&`/`:p`/`:q`/`:x` modifiers,
/// returning the modified text, the index just past them, and whether `:p` was
/// seen (which makes bash print the line instead of running it).
///
/// A `:s`/`:&` that cannot be applied fails the whole expansion, exactly as a
/// missing event does: the `Err` carries the message body bash would print,
/// which quotes the modifier back verbatim (`:gs/nope/x/: substitution failed`).
fn apply_modifiers(
    mut text: String,
    chars: &[char],
    mut i: usize,
    state: &mut HistState,
) -> Result<(String, usize, bool), String> {
    let mut print_only = false;
    while chars.get(i) == Some(&':') {
        let mut j = i.saturating_add(1);
        // `g`/`a` prefix makes the following substitution global.
        let mut global = false;
        while matches!(chars.get(j), Some('g' | 'a')) {
            global = true;
            j = j.saturating_add(1);
        }
        match chars.get(j) {
            Some(&c @ ('h' | 't' | 'r' | 'e')) => {
                text = modify_path(&text, c);
                i = j.saturating_add(1);
            }
            Some('p') => {
                print_only = true;
                i = j.saturating_add(1);
            }
            // `:q` quotes the result as one word; `:x` quotes it so that its
            // whitespace still separates words. Either way a later expansion
            // pass leaves the text alone. bash uses single quotes for both.
            Some(&c @ ('q' | 'x')) => {
                text = if c == 'q' { single_quote(&text) } else { quote_breaks(&text) };
                i = j.saturating_add(1);
            }
            Some('s') => {
                // No delimiter at all (`!!:s` at the end of the line) is not an
                // error in bash: the `s` is consumed and the text is left alone.
                let Some((old, new, end)) = parse_subst(chars, j.saturating_add(1)) else {
                    i = j.saturating_add(1);
                    break;
                };
                // An empty pattern reuses the previous one, as in bash.
                let reused = old.is_empty();
                let old = if reused { state.subst_old.clone() } else { old };
                if old.is_empty() {
                    return Err(format!("{}: no previous substitution", spec_text(chars, i, end)));
                }
                if !text.contains(&old) {
                    return Err(format!("{}: substitution failed", spec_text(chars, i, end)));
                }
                text = substitute(&text, &old, &new, global);
                state.subst_old = old;
                state.subst_new = new;
                i = end;
            }
            Some('&') => {
                let end = j.saturating_add(1);
                if state.subst_old.is_empty() {
                    return Err(format!("{}: no previous substitution", spec_text(chars, i, end)));
                }
                if !text.contains(&state.subst_old) {
                    return Err(format!("{}: substitution failed", spec_text(chars, i, end)));
                }
                text = substitute(&text, &state.subst_old, &state.subst_new, global);
                i = end;
            }
            // A `:` commits to a modifier, so anything else after it — an
            // unknown letter, or the end of the line — aborts the whole
            // expansion the way a missing event does. bash names the offending
            // character, which is empty when the line simply ran out (`!!:` and
            // `!!:g` both report `: unrecognized history modifier`).
            //
            // A character that is not a `:` is a different matter: it merely
            // ends the modifier run and stays as literal text, which is why
            // `!!:hz` is the `:h` of the event with a `z` stuck on the end.
            other => {
                let name: String = other.copied().map(String::from).unwrap_or_default();
                return Err(format!("{name}: unrecognized history modifier"));
            }
        }
    }
    Ok((text, i, print_only))
}

/// Wrap `s` in single quotes, escaping any it contains the way bash's `:q` does.
fn single_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len().saturating_add(2));
    out.push('\'');
    for c in s.chars() {
        if c == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(c);
        }
    }
    out.push('\'');
    out
}

/// Quote `s` the way `:x` does — readline's `quote_breaks`.
///
/// This looks like "quote each word" and is not: the *whole* string gets one
/// pair of single quotes, and each whitespace character is individually lifted
/// back out of them. On `a b` the two readings agree (`'a' 'b'`), but they part
/// company as soon as the text is less tidy — `a  b` becomes `'a' '' 'b'`, a
/// tab stays a tab, and `a;b` stays the single item `'a;b'` even though
/// [`words`] would split it in three. Measured against bash 5.2.
fn quote_breaks(s: &str) -> String {
    let mut out = String::with_capacity(s.len().saturating_add(2));
    out.push('\'');
    for c in s.chars() {
        match c {
            '\'' => out.push_str("'\\''"),
            ' ' | '\t' | '\n' => {
                out.push('\'');
                out.push(c);
                out.push('\'');
            }
            _ => out.push(c),
        }
    }
    out.push('\'');
    out
}

/// Whether the `!` at `chars[i]` is a literal `!` rather than the start of an
/// event designator.
///
/// This is bash's `history_no_expand_chars` set plus its
/// `history_inhibit_expansion_function` hook (`bash_history_inhibit_expansion`
/// in `bashhist.c`), which exists precisely because the shell gives `!` four
/// other meanings. Every rule below was **measured** against
/// `bash --norc --noprofile -i` on this machine — reading a line and looking for
/// `bash: …: event not found` — because the obvious guesses are wrong in both
/// directions: `(` is *not* in bash's no-expand set (a bare `echo !(zzz)` with
/// `extglob` off really does try to expand), while `$!` and `x[!a]` are
/// inhibited even though nothing about the `!` itself says so, `[!!]` expands
/// where `[!$]` does not, and double quotes inhibit nothing at all.
fn inhibited(chars: &[char], i: usize, extglob: bool, dquote: bool) -> bool {
    let next = chars.get(i.saturating_add(1)).copied();
    // bash's `history_no_expand_chars`, " \t\n\r=", plus end of line. `\r` is in
    // the set even though a line read by the shell never contains one.
    if matches!(next, None | Some(' ' | '\t' | '\n' | '\r' | '=')) {
        return true;
    }
    // Inside a double-quoted string the closing quote joins that set, so
    // `echo "x !"` is left alone — but only when it comes *immediately* after the
    // `!`. In `echo "x !a"b` the quote merely ends the search string, and `!a` is
    // a real (failing) event reference.
    if dquote && next == Some('"') {
        return true;
    }
    // `!(pat)` is an extended-glob negation — but only once `extglob` is on, only
    // when the group is *closed* on the line, and (bash's own `i > 1`) only when
    // the `!` is at least two characters in: `echo !(zzz)` inhibits, while
    // `echo !(zzz`, `!(zzz)` at the start of a line and `x!(y)` all expand and
    // report `!: event not found`. `!()` counts as closed, so there is no "empty
    // pattern" exception.
    if extglob
        && i >= 2
        && next == Some('(')
        && chars
            .get(i.saturating_add(2)..)
            .unwrap_or_default()
            .contains(&')')
    {
        return true;
    }
    let prev = i.checked_sub(1).and_then(|p| chars.get(p)).copied();
    // `$!` is the last background pid, so a `!` directly after a `$` is never an
    // event — including in `$$!q`, where the `!` follows the second `$`.
    if prev == Some('$') {
        return true;
    }
    // `${!name}` / `${!name[@]}` / `${!pre*}` is indirect expansion, and `[!…]`
    // is a negated glob bracket. Both are inhibited only when the construct is
    // actually *closed* on this line: `echo ${!q` and `echo [!q` (no `}` / `]`)
    // do expand, and so does `${a!b}` or `${ !q}`, where the `!` is not directly
    // after the opener.
    //
    // bash finds the `}` with `skip_to_delim`, which respects nesting and
    // quoting; a plain forward scan differs only for a `}` that appears solely
    // inside quotes or a nested expansion within the same word, which no real
    // input hits.
    let rest = chars.get(i.saturating_add(1)..).unwrap_or_default();
    if prev == Some('{')
        && i.checked_sub(2).and_then(|p| chars.get(p)).copied() == Some('$')
        && rest.contains(&'}')
    {
        return true;
    }
    // The glob-bracket rule has one exception, and it is the `!` itself: `[!!]`
    // expands, while `[!$]`, `[!^]`, `[!:0]`, `[!-1]`, `[!?x?]`, `[!abc]` and
    // `[!*]` are all left alone. So it is not "a designator may not open a
    // bracket expression" — only a second `!` overrides the negation reading,
    // which is bash comparing the next character against its
    // `history_expansion_char`. `[!!!]` therefore expands the `!!` and then
    // *fails* on the `!]` that is left.
    if prev == Some('[') && next != Some('!') && rest.contains(&']') {
        return true;
    }
    false
}

/// Run history expansion over one physical input line.
///
/// Returns [`Expansion::Unchanged`] when the line held no history reference, so
/// the caller can skip the "echo the expanded line" behaviour entirely.
///
/// `state` carries what one expansion leaves for the next — the most recent
/// `:s` and the most recent `?string?` search. See [`HistState`].
pub fn expand(line: &str, ctx: &HistCtx, state: &mut HistState) -> Expansion {
    // `^old^new^` is not a form of its own: readline implements it by *textually*
    // prefixing `!!:s` and running the ordinary expander over the result. Three
    // observable consequences follow, all of which would need special-casing if
    // it were implemented as its own path:
    //
    // * it only works when the `^` is the very first character of the line;
    // * the diagnostics quote back the rewritten spec (`:s^nomatch^x^`,
    //   delimiter and all) rather than the line as typed, and an empty history
    //   fails as `!!: event not found` rather than naming the substitution;
    // * everything after the closing delimiter survives *and is expanded* —
    //   `^one^two^ !!` appends the previous command a second time, `^one^two^^`
    //   leaves a trailing `^`, and `^one^two^:z` reaches the modifier scan.
    let quick = line.starts_with('^').then(|| format!("!!:s{line}"));
    let line = quick.as_deref().unwrap_or(line);
    if !line.contains('!') {
        return Expansion::Unchanged;
    }

    let chars: Vec<char> = line.chars().collect();
    let mut out = String::new();
    let mut i = 0usize;
    let mut changed = false;
    let mut print_only = false;
    // Seeded from the reader's delimiter stack, not from zero: a quote opened on
    // an earlier physical line of the same command is still open here. See
    // `HistCtx::open_quote`.
    let mut in_single = ctx.open_quote == Some('\'');
    // Tracked *only* for the comment rule below — a `!` in a double-quoted string
    // is still expanded, because the rewrite happens before quote removal.
    let mut in_double = ctx.open_quote == Some('"');

    while let Some(&c) = chars.get(i) {
        // A backslash suppresses the history character — and is left in place,
        // because removing it is the *parser's* job, not history expansion's.
        // `echo \!!` is therefore reported unchanged (no echo of a rewritten
        // line) even though the command finally run prints `!!`; and inside
        // double quotes, where `\!` is not a quoting pair, the backslash
        // survives all the way to the output, as it does in bash.
        if c == '\\' && !in_single {
            out.push(c);
            if let Some(&n) = chars.get(i.saturating_add(1)) {
                out.push(n);
                i = i.saturating_add(2);
            } else {
                i = i.saturating_add(1);
            }
            continue;
        }
        if c == '\'' {
            in_single = !in_single;
            out.push(c);
            i = i.saturating_add(1);
            continue;
        }
        if c == '"' && !in_single {
            in_double = !in_double;
            out.push(c);
            i = i.saturating_add(1);
            continue;
        }
        // readline's `history_comment_char`: an unquoted `#` that starts a word
        // makes the rest of the line literal, so a `!` in a trailing comment is
        // never an event. "Starts a word" means at index 0 or right after one of
        // readline's `history_word_delimiters`; measured against bash, `echo x #
        // !q`, `echo x;# !q`, `: $(echo z)#!q` and `(:)#!q` are all inhibited,
        // while `echo a#!q` and `echo "text # !q"` still fail with
        // `!q: event not found`.
        //
        // Unlike the `!` rules above, this one *does* respect double quotes,
        // which is why they are tracked at all. `expand` is handed one raw input
        // line at a time, so "the rest of the line" is the rest of the string.
        if c == '#'
            && !in_single
            && !in_double
            && i.checked_sub(1)
                .and_then(|p| chars.get(p))
                .is_none_or(|&p| is_word_delimiter(p))
        {
            out.extend(chars.get(i..).unwrap_or_default());
            break;
        }
        // Single quotes suppress expansion; double quotes deliberately do not.
        if c != '!' || in_single {
            out.push(c);
            i = i.saturating_add(1);
            continue;
        }

        if inhibited(&chars, i, ctx.extglob, in_double) {
            out.push(c);
            i = i.saturating_add(1);
            continue;
        }

        match expand_one(&chars, i, ctx, state, in_double) {
            Ok((text, end, p)) => {
                out.push_str(&text);
                i = end;
                changed = true;
                print_only |= p;
            }
            Err(msg) => return Expansion::NotFound(msg),
        }
    }

    match (changed, print_only) {
        (_, true) => Expansion::PrintOnly(out),
        (true, false) => Expansion::Changed(out),
        // A `^old^new^` rewrite whose `!!` turned out to be quoted: nothing was
        // expanded, so nothing is echoed, but the line bash runs is still the
        // rewritten one. See [`Expansion::ChangedQuietly`].
        (false, false) if quick.is_some() => Expansion::ChangedQuietly(out),
        (false, false) => Expansion::Unchanged,
    }
}

/// readline's `history_word_delimiters`, `" \t\n;&()|<>"`. It ends an event
/// search string and marks where a `#` may start a comment.
fn is_word_delimiter(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | ';' | '&' | '(' | ')' | '|' | '<' | '>')
}

/// Expand the single designator starting at `chars[start]` (which is the `!`).
/// On success returns the replacement text and the index just past it; on
/// failure the whole message body bash would print (see [`Expansion::NotFound`]).
fn expand_one(
    chars: &[char],
    start: usize,
    ctx: &HistCtx,
    state: &mut HistState,
    dquote: bool,
) -> Result<(String, usize, bool), String> {
    let mut i = start.saturating_add(1);
    // Remember where the spec began so a failure can quote it back.
    let spec_start = start;
    let missing = |spec: &str| format!("{spec}: event not found");

    let event: String = match chars.get(i) {
        // `!!` — the previous command.
        Some('!') => {
            i = i.saturating_add(1);
            ctx.back(1).map(str::to_string).ok_or_else(|| missing("!!"))?
        }
        // `!#` — the current line so far. Handled by the caller having already
        // accumulated it; we approximate with the text before this designator.
        Some('#') => {
            i = i.saturating_add(1);
            chars.get(..start).unwrap_or(&[]).iter().collect()
        }
        // `!-n` — n events back.
        Some('-') => {
            i = i.saturating_add(1);
            let mut n = 0usize;
            let digits_at = i;
            while let Some(d) = chars.get(i).and_then(|c| c.to_digit(10)) {
                n = n.saturating_mul(10).saturating_add(d as usize);
                i = i.saturating_add(1);
            }
            if i == digits_at {
                return Err(missing(&spec_text(chars, spec_start, i)));
            }
            ctx.back(n)
                .map(str::to_string)
                .ok_or_else(|| missing(&spec_text(chars, spec_start, i)))?
        }
        // `!n` — an absolute event number.
        Some(c) if c.is_ascii_digit() => {
            let mut n = 0usize;
            while let Some(d) = chars.get(i).and_then(|c| c.to_digit(10)) {
                n = n.saturating_mul(10).saturating_add(d as usize);
                i = i.saturating_add(1);
            }
            ctx.by_number(n)
                .map(str::to_string)
                .ok_or_else(|| missing(&spec_text(chars, spec_start, i)))?
        }
        // `!?string?` — the most recent event containing string.
        Some('?') => {
            i = i.saturating_add(1);
            let mut written = String::new();
            while let Some(&c) = chars.get(i) {
                if c == '?' {
                    i = i.saturating_add(1);
                    break;
                }
                written.push(c);
                i = i.saturating_add(1);
            }
            // An empty search string means the previous one, exactly as an empty
            // `:s//new/` pattern does — `!??` after a `!?bet?` searches for
            // `bet` again. With no previous search there is nothing to reuse, and
            // readline will not search for nothing even though every event
            // contains the empty string: `!??` and `!?` both fail outright.
            let needle = if written.is_empty() { state.search_string.clone() } else { written };
            // The diagnostic quotes back exactly what was written: `!??` rather
            // than the string it stood for, and `!?zzz` without a `?` that the
            // designator never had.
            let spec = spec_text(chars, spec_start, i);
            if needle.is_empty() {
                return Err(missing(&spec));
            }
            let found =
                ctx.by_substring(&needle).map(str::to_string).ok_or_else(|| missing(&spec))?;
            // bash records the match here, at search time, and a `%` later in
            // *this* line already sees it (`!?gam?:%`). Note that it is a plain
            // string: it survives into lines whose own event has no such word.
            state.search_match = search_match(&found, &needle);
            state.search_string = needle;
            found
        }
        // `!$`, `!^`, `!*` — word designators against the previous command.
        Some('$' | '^' | '*') => ctx
            .back(1)
            .map(str::to_string)
            .ok_or_else(|| missing("!!"))?,
        // `!string` — the most recent event starting with string.
        Some(_) => {
            let mut prefix = String::new();
            let mut hit_delimiter = false;
            while let Some(&c) = chars.get(i) {
                if is_word_delimiter(c) || (dquote && c == '"') {
                    hit_delimiter = true;
                    break;
                }
                // `:^$*%-` introduce a word designator or modifier, so they end
                // the search string without being a hard delimiter: an empty
                // search string before one of them still means "the previous
                // event", which is what makes `!:0` and `!$` work.
                if matches!(c, ':' | '^' | '$' | '*' | '%' | '-') {
                    break;
                }
                prefix.push(c);
                i = i.saturating_add(1);
            }
            // A search string that is empty because a word delimiter came first
            // (`!(zzz)` with `extglob` off, `!;x`, `!|x`) matches nothing at all,
            // and bash quotes back just the `!`.
            if prefix.is_empty() && hit_delimiter {
                return Err(missing("!"));
            }
            ctx.by_prefix(&prefix)
                .map(str::to_string)
                .ok_or_else(|| missing(&format!("!{prefix}")))?
        }
        None => return Err(missing("!")),
    };

    // The `%` arm needs the search match, so this cannot be borrowed for the
    // whole call: clone the one string out first.
    let matched = state.search_match.clone();
    let (selected, after_words) = apply_word_designator(&event, chars, i, &matched)?;
    apply_modifiers(selected, chars, after_words, state)
}

/// The raw text of a designator, for the message of a failed expansion — bash
/// quotes back exactly the part it could not use (`!999: event not found`,
/// `:4: bad word specifier`, `:gs/nope/x/: substitution failed`).
fn spec_text(chars: &[char], start: usize, end: usize) -> String {
    chars.get(start..end).unwrap_or(&[]).iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(entries: &[String]) -> HistCtx<'_> {
        HistCtx { entries, base: 1, extglob: false, open_quote: None }
    }

    fn hist(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_string()).collect()
    }

    /// Every expectation here was measured against bash 5.2 by recording the
    /// line and reading back `!!:0`, `!!:1`, … until the specifier went bad.
    #[test]
    fn words_are_readlines_history_tokens_not_a_whitespace_split() {
        // The base case: operators are words, a quoted span is not split.
        assert_eq!(
            words(": a=(x y z) b<c d|e f&&g h>>i \"s t\" j&k"),
            vec![
                ":", "a=", "(", "x", "y", "z", ")", "b", "<", "c", "d", "|", "e", "f", "&&", "g",
                "h", ">>", "i", "\"s t\"", "j", "&", "k"
            ]
        );

        // Doubled operators group; a third character joins only a doubled `<`.
        assert_eq!(words("a;;;b"), vec!["a", ";;", ";", "b"]);
        assert_eq!(words("a&&&&b"), vec!["a", "&&", "&&", "b"]);
        assert_eq!(words("a<<<b"), vec!["a", "<<<", "b"]);
        assert_eq!(words("a<<-b"), vec!["a", "<<-", "b"]);
        assert_eq!(words("a>>-b"), vec!["a", ">>", "-b"]);

        // The file-descriptor pairs absorb digits and `-`; their neighbours do
        // not group at all.
        assert_eq!(words("cmd 2>&1"), vec!["cmd", "2>&1"]);
        assert_eq!(words("cmd >&12y"), vec!["cmd", ">&12", "y"]);
        assert_eq!(words("cmd >&-"), vec!["cmd", ">&-"]);
        assert_eq!(words("a<>b"), vec!["a", "<", ">", "b"]);
        assert_eq!(words("a|&b"), vec!["a", "|", "&", "b"]);
        assert_eq!(words("a;&b"), vec!["a", ";", "&", "b"]);
        assert_eq!(words("a&<b"), vec!["a", "&", "<", "b"]);

        // A digit run is a file descriptor only at the start of a word.
        assert_eq!(words("a2>b"), vec!["a2", ">", "b"]);
        assert_eq!(words("12ab;c"), vec!["12ab", ";", "c"]);
        assert_eq!(words("12"), vec!["12"]);

        // Quoting protects delimiters and stays in the word.
        assert_eq!(words("x 'a;b' y"), vec!["x", "'a;b'", "y"]);
        assert_eq!(words("x a'b;c'd y"), vec!["x", "a'b;c'd", "y"]);
        assert_eq!(words("x $'a;b' y"), vec!["x", "$'a;b'", "y"]);
        assert_eq!(words("x $\"a;b\" y"), vec!["x", "$\"a;b\"", "y"]);
        // An unterminated quote swallows the rest of the line.
        assert_eq!(words("x 'a b y"), vec!["x", "'a b y"]);
        // …and a backslash does not rescue it, since nothing escapes inside
        // `'…'` — so the second quote here closes and the `;` then splits.
        assert_eq!(words("x 'a\\'b;c' y"), vec!["x", "'a\\'b", ";", "c' y"]);
        // Inside `"…"` a backslash does escape.
        assert_eq!(words("x \"a\\\"b;c\" y"), vec!["x", "\"a\\\"b;c\"", "y"]);
        // An escaped quote never opens one at all.
        assert_eq!(words("x \\'a;b y"), vec!["x", "\\'a", ";", "b", "y"]);
        // A backslash escapes a delimiter directly.
        assert_eq!(words("x a\\;b y"), vec!["x", "a\\;b", "y"]);
        assert_eq!(words("x a\\ b y"), vec!["x", "a\\ b", "y"]);

        // `$( … )` nests on a further `$(`, and on a bare `(` too — but not on
        // the one immediately after the opener, which readline steps over. That
        // asymmetry is the whole reason `$((…))` and `$(… (…) …)` differ.
        assert_eq!(words("x $(a $(b;c) d) y"), vec!["x", "$(a $(b;c) d)", "y"]);
        assert_eq!(words("x $(a (b;c) d) y"), vec!["x", "$(a (b;c) d)", "y"]);
        assert_eq!(words("x $((a;b)) y"), vec!["x", "$((a;b)", ")", "y"]);
        // Every extended-glob opener behaves the same way, and so do the two
        // process-substitution ones — even though `<` and `>` would otherwise
        // have been claimed as operators.
        for open in ["@", "!", "?", "+", "*", "<", ">"] {
            assert_eq!(words(&format!("x {open}(a;b) y")), vec!["x", &format!("{open}(a;b)"), "y"]);
        }
        // Only a *lone* `<`/`>` yields to it: doubled or paired forms keep the
        // operator reading and leave the `(` behind as a word of its own.
        assert_eq!(words("x >>(a;b) y"), vec!["x", ">>", "(", "a", ";", "b", ")", "y"]);
        assert_eq!(words("x <<(a;b) y"), vec!["x", "<<", "(", "a", ";", "b", ")", "y"]);
        assert_eq!(words("x >&(a;b) y"), vec!["x", ">&", "(", "a", ";", "b", ")", "y"]);
        // …and the operators that are not substitution openers never yield.
        for open in ["&", ";", "|"] {
            assert_eq!(
                words(&format!("x {open}(a;b) y")),
                vec!["x", open, "(", "a", ";", "b", ")", "y"]
            );
        }
        // A file descriptor still reaches the substitution through the digits.
        assert_eq!(words("x 2<(a;b) y"), vec!["x", "2<(a;b)", "y"]);
        assert_eq!(words("x 2>(a;b) y"), vec!["x", "2>(a;b)", "y"]);
        // But a digit run before a bare `(` is just a word.
        assert_eq!(words("x 2(a;b) y"), vec!["x", "2", "(", "a", ";", "b", ")", "y"]);
        assert_eq!(words("x a<(b;c) y"), vec!["x", "a<(b;c)", "y"]);
        // A quote outranks it, so `$(` inside one is inert.
        assert_eq!(words("x '$(a;b)' y"), vec!["x", "'$(a;b)'", "y"]);
        assert_eq!(words("x \"$(a;b)\" y"), vec!["x", "\"$(a;b)\"", "y"]);
        // `${ … }` protects nothing.
        assert_eq!(words("x ${a;b} y"), vec!["x", "${a", ";", "b}", "y"]);

        // A `#` starting a word ends the line; one inside a word does not.
        assert_eq!(words("echo a # b c"), vec!["echo", "a"]);
        assert_eq!(words("echo a #b c"), vec!["echo", "a"]);
        assert_eq!(words("echo a' #'b c"), vec!["echo", "a' #'b", "c"]);

        // Tabs separate; a newline is a word of its own.
        assert_eq!(words("x\ta\tb"), vec!["x", "a", "b"]);
        assert_eq!(words("a\nb"), vec!["a", "\n", "b"]);

        // Multi-byte text is never split inside a character.
        assert_eq!(words("echo héllo;wörld"), vec!["echo", "héllo", ";", "wörld"]);

        // A line that stops part-way through a construct keeps the truncated
        // word rather than losing it — the scan must not run off the end.
        assert_eq!(words("echo a\\"), vec!["echo", "a\\"]);
        assert_eq!(words("echo $("), vec!["echo", "$("]);
        assert_eq!(words("echo a$("), vec!["echo", "a$("]);
        assert_eq!(words("echo '"), vec!["echo", "'"]);
        assert_eq!(words("echo \""), vec!["echo", "\""]);
        assert_eq!(words("echo `"), vec!["echo", "`"]);
        assert_eq!(words("echo 2>&"), vec!["echo", "2>&"]);
        assert_eq!(words("echo <"), vec!["echo", "<"]);
    }

    /// `:x` is readline's `quote_breaks`, which is not "quote each word".
    #[test]
    fn quote_breaks_lifts_each_whitespace_character_out_of_the_quotes() {
        assert_eq!(quote_breaks("a b c d"), "'a' 'b' 'c' 'd'");
        // Two spaces leave an empty item between them, which a split would not.
        assert_eq!(quote_breaks("a  b"), "'a' '' 'b'");
        assert_eq!(quote_breaks("a\tb"), "'a'\t'b'");
        // Operators are not whitespace, so they stay inside the quotes even
        // though `words` would split on them.
        assert_eq!(quote_breaks("a;b"), "'a;b'");
        assert_eq!(quote_breaks("a|b&c"), "'a|b&c'");
        assert_eq!(quote_breaks("a'b"), "'a'\\''b'");
        assert_eq!(quote_breaks("a'b c"), "'a'\\''b' 'c'");
    }

    /// [`expand`] with a throwaway last-substitution state, for the cases that
    /// do not exercise a `:&` carried over from an earlier line.
    fn expand1(line: &str, ctx: &HistCtx) -> Expansion {
        expand(line, ctx, &mut HistState::default())
    }

    fn expanded(line: &str, items: &[&str]) -> String {
        let h = hist(items);
        match expand1(line, &ctx(&h)) {
            Expansion::Changed(s) | Expansion::ChangedQuietly(s) => s,
            Expansion::Unchanged => line.to_string(),
            Expansion::PrintOnly(s) => panic!("unexpected print-only: {s}"),
            Expansion::NotFound(e) => panic!("unexpected not-found: {e}"),
        }
    }

    /// `HistCtx::open_quote` — readline's `history_quoting_state`. Every case
    /// was measured by feeding the two lines to `bash --norc --noprofile` as a
    /// script with `set -o history; set -H` and `echo one` recorded before them.
    #[test]
    fn a_carried_quote_state_is_honoured() {
        let h = hist(&["echo one"]);
        let with = |q: Option<char>, line: &str| {
            let c = HistCtx { entries: &h, base: 1, extglob: false, open_quote: q };
            expand1(line, &c)
        };
        // Inside a carried single quote the whole line is inert, so even an
        // event that does not exist is not an error…
        for line in ["!!'", "!! z'", "!zz'"] {
            assert!(matches!(with(Some('\''), line), Expansion::Unchanged), "{line}");
        }
        // …but only up to the quote's close: what follows expands normally.
        match with(Some('\''), "y' !!") {
            Expansion::Changed(s) => assert_eq!(s, "y' echo one"),
            other => panic!("expected an expansion past the close, got {other:?}"),
        }
        // A carried double quote leaves expansion live — including the rule that
        // a `#` inside quotes does not start a comment, so the `!!` after one is
        // still reached.
        match with(Some('"'), "#!!") {
            Expansion::Changed(s) => assert_eq!(s, "#echo one"),
            other => panic!("expected a quoted # not to comment, got {other:?}"),
        }
        // …and a lone `!` before the closing quote is still literal, because `"`
        // is one of the characters a bare `!` may precede.
        assert!(matches!(with(Some('"'), "!\""), Expansion::Unchanged));
        // The `^old^new^` rewrite is still performed inside a carried quote —
        // the `!!` it prefixes is simply quoted, so the literal spec is what
        // runs and nothing is echoed.
        match with(Some('\''), "^one^two^'") {
            Expansion::ChangedQuietly(s) => assert_eq!(s, "!!:s^one^two^'"),
            other => panic!("expected a quiet rewrite, got {other:?}"),
        }
        // With no carried state the same lines expand (or fail) as first lines.
        match with(None, "!!'") {
            Expansion::Changed(s) => assert_eq!(s, "echo one'"),
            other => panic!("expected a first-line expansion, got {other:?}"),
        }
    }

    #[test]
    fn plain_line_is_unchanged() {
        let h = hist(&["echo one"]);
        assert!(matches!(
            expand1("echo hello", &ctx(&h)),
            Expansion::Unchanged
        ));
    }

    #[test]
    fn bang_bang_recalls_previous() {
        assert_eq!(expanded("!!", &["echo one"]), "echo one");
    }

    #[test]
    fn absolute_and_relative_events() {
        let h = ["echo one", "echo two", "echo three"];
        assert_eq!(expanded("!1", &h), "echo one");
        assert_eq!(expanded("!3", &h), "echo three");
        assert_eq!(expanded("!-2", &h), "echo two");
    }

    #[test]
    fn prefix_and_substring_search() {
        let h = ["echo alpha", "print beta", "echo gamma"];
        assert_eq!(expanded("!pr", &h), "print beta");
        assert_eq!(expanded("!?beta?", &h), "print beta");
        // Prefix search finds the most recent match, not the first.
        assert_eq!(expanded("!echo", &h), "echo gamma");
    }

    #[test]
    fn expands_inside_double_quotes_but_not_single() {
        assert_eq!(expanded("echo \"!!\"", &["one"]), "echo \"one\"");
        let h = hist(&["one"]);
        assert!(matches!(
            expand1("echo '!!'", &ctx(&h)),
            Expansion::Unchanged
        ));
    }

    /// A backslash suppresses the history character but is *not* removed here:
    /// bash leaves it for the parser, so the line counts as unexpanded and no
    /// echo of a rewritten line is produced. (`echo \!!` still prints `!!`,
    /// because ordinary shell quoting eats the backslash later.)
    #[test]
    fn backslash_suppresses_and_is_left_in_place() {
        let h = hist(&["one"]);
        for line in ["echo \\!\\!", "echo \"dq \\!! here\"", "echo x\\!y"] {
            assert!(
                matches!(expand1(line, &ctx(&h)), Expansion::Unchanged),
                "{line} should not expand"
            );
        }
    }

    /// `:&` repeats the previous `:s` — and bash's "previous" spans lines, so
    /// the state has to survive between [`expand`] calls.
    #[test]
    fn ampersand_repeats_a_substitution_from_an_earlier_line() {
        let h = hist(&["echo one two"]);
        let mut last = HistState::default();
        match expand("!!:s/one/ONE/", &ctx(&h), &mut last) {
            Expansion::Changed(s) => assert_eq!(s, "echo ONE two"),
            _ => panic!("expected a rewrite"),
        }
        let h2 = hist(&["echo one three"]);
        match expand("!!:&", &ctx(&h2), &mut last) {
            Expansion::Changed(s) => assert_eq!(s, "echo ONE three"),
            _ => panic!("expected a rewrite"),
        }
        // An empty pattern reuses it too.
        let h3 = hist(&["echo one four"]);
        match expand("!!:s//1/", &ctx(&h3), &mut last) {
            Expansion::Changed(s) => assert_eq!(s, "echo 1 four"),
            _ => panic!("expected a rewrite"),
        }
    }

    /// The three failure wordings bash distinguishes, each quoting the modifier
    /// back verbatim. `^old^new^` is reported in its `:s` form.
    #[test]
    fn substitution_failures_are_reported_as_bash_words_them() {
        let h = hist(&["echo one"]);
        let cases = [
            ("!!:s/nope/x/", ":s/nope/x/: substitution failed"),
            ("!!:gs/nope/x/", ":gs/nope/x/: substitution failed"),
            ("!!:s//x/", ":s//x/: no previous substitution"),
            ("!!:&", ":&: no previous substitution"),
            ("^nomatch^x^", ":s^nomatch^x^: substitution failed"),
        ];
        for (line, want) in cases {
            match expand1(line, &ctx(&h)) {
                Expansion::NotFound(msg) => assert_eq!(msg, want, "for {line}"),
                _ => panic!("{line} should have failed"),
            }
        }
    }

    /// `:p` still expands, but the result is for showing, not running.
    #[test]
    fn print_only_modifier() {
        let h = hist(&["echo one"]);
        match expand1("!!:p", &ctx(&h)) {
            Expansion::PrintOnly(s) => assert_eq!(s, "echo one"),
            _ => panic!("expected print-only"),
        }
        // It wins wherever it appears, and the rest of the line comes with it.
        match expand1("echo !!:p tail", &ctx(&h)) {
            Expansion::PrintOnly(s) => assert_eq!(s, "echo echo one tail"),
            _ => panic!("expected print-only"),
        }
    }

    /// `:q` quotes the whole result as one word; `:x` quotes each word.
    #[test]
    fn quoting_modifiers() {
        assert_eq!(expanded("echo !!:q", &["ls a b"]), "echo 'ls a b'");
        assert_eq!(expanded("echo !!:x", &["ls a b"]), "echo 'ls' 'a' 'b'");
    }

    #[test]
    fn bare_bang_is_literal() {
        let h = hist(&["one"]);
        for line in ["echo hi ! there", "echo trailing !", "a != b", "a\t!\tb"] {
            assert!(
                matches!(expand1(line, &ctx(&h)), Expansion::Unchanged),
                "{line} should not expand"
            );
        }
    }

    /// The [`inhibited`] hook — bash's `bash_history_inhibit_expansion`. Every
    /// case here was measured by feeding the line to
    /// `bash --norc --noprofile -i` and watching for `event not found`.
    #[test]
    fn shell_syntax_inhibits_expansion() {
        let h = hist(&["one"]);
        let unchanged = |line: &str, extglob: bool| {
            let c = HistCtx { entries: &h, base: 1, extglob, open_quote: None };
            matches!(expand1(line, &c), Expansion::Unchanged)
        };
        // `$!` is the last background pid — including after a `$$`.
        for line in ["echo $!", "echo a$!b", "echo \"$!\"", "echo $$!q", "echo ${a[$!]-n}"] {
            assert!(unchanged(line, false), "{line} should not expand");
        }
        // `${!name}` indirect expansion, in all its spellings.
        for line in ["echo ${!ref}", "echo ${!pre_*}", "echo ${!pre_@}", "echo ${!a[@]}"] {
            assert!(unchanged(line, false), "{line} should not expand");
        }
        // …but only directly after the `${`, and only when the `}` is on the line.
        for line in ["echo ${!q", "echo ${a!b}", "echo ${ !q}", "echo ${x:-!q}", "echo ${x#!}"] {
            assert!(!unchanged(line, false), "{line} should expand");
        }
        // `[!…]` is a negated glob bracket — closing `]` required, and the `!`
        // must be the first thing in the bracket. Double quotes do not matter,
        // because history expansion runs before quote removal.
        for line in ["echo [!q]", "echo x[!a]y", "echo \"x[!a]\"", "echo ${a[!1]-n}"] {
            assert!(unchanged(line, false), "{line} should not expand");
        }
        for line in ["echo [!q", "echo a[b!c]"] {
            assert!(!unchanged(line, false), "{line} should expand");
        }
        // The one character that overrides the negation reading is the history
        // character itself, so `[!!…]` expands where every other designator in
        // the same position does not.
        for line in ["echo [!!]", "echo [!!:1]", "echo x[!!]y", "echo [[!!]]", "echo [!!$]"] {
            assert!(!unchanged(line, false), "{line} should expand");
        }
        for line in ["echo [!^]", "echo [!$]", "echo [!:0]", "echo [!-1]", "echo [!*]"] {
            assert!(unchanged(line, false), "{line} should not expand");
        }
        // …and only the first `!` of `[!!!]` is exempt: the `!!` expands, leaving
        // a `!]` that is a search for `]` and fails.
        match expand1("echo [!!!]", &ctx(&h)) {
            Expansion::NotFound(e) => assert_eq!(e, "!]: event not found"),
            other => panic!("expected [!!!] to fail, got {other:?}"),
        }
        // `!(pat)` is inhibited only once `extglob` is on: `(` is *not* one of
        // bash's `history_no_expand_chars`, so with extglob off even `[[ x ==
        // !(y) ]]` and `case x in !(y))` are event references that fail. An
        // empty `!()` is inhibited too.
        for line in ["echo !(zzz)", "echo x!(y)", "echo !()", "[[ x == !(y) ]]"] {
            assert!(unchanged(line, true), "{line} should not expand with extglob");
            assert!(!unchanged(line, false), "{line} should expand without extglob");
        }
        // An assignment does not inhibit anything.
        assert!(!unchanged("x=!q", false), "x=!q should expand");
        assert!(!unchanged("x=!(", false), "x=!( should expand");
    }

    /// How much of the line a `!string` event takes as its search string, which
    /// is visible in the "event not found" message. Measured against bash.
    #[test]
    fn event_search_string_ends_at_a_delimiter() {
        let h = hist(&["one"]);
        let failure = |line: &str| match expand1(line, &ctx(&h)) {
            Expansion::NotFound(e) => e,
            _ => panic!("{line} should have failed"),
        };
        // `history_word_delimiters` and the word-designator/modifier introducers
        // end it…
        for (line, spec) in [
            ("echo !zz(q)", "!zz"),
            ("echo !zz)", "!zz"),
            ("echo !zz;b", "!zz"),
            ("echo !zz&b", "!zz"),
            ("echo !zz|b", "!zz"),
            ("echo !zz<b", "!zz"),
            ("echo !zz>b", "!zz"),
            ("echo !zz-b", "!zz"),
            ("echo !zz%b", "!zz"),
            ("echo !zz*b", "!zz"),
            ("echo !zz:0", "!zz"),
            ("echo !zz b", "!zz"),
            // …and nothing else does, not even a `!` or a single quote.
            ("echo !zz=b", "!zz=b"),
            ("echo !zz{b}", "!zz{b}"),
            ("echo !zz[b]", "!zz[b]"),
            ("echo !zz!b", "!zz!b"),
            ("echo !zz'b'", "!zz'b'"),
            ("echo !zz\"b\"", "!zz\"b\""),
            ("echo !zz#b", "!zz#b"),
            ("echo !zz?b", "!zz?b"),
            ("echo !zz.b", "!zz.b"),
            ("echo !zz/b", "!zz/b"),
            ("echo !zz,b", "!zz,b"),
            ("echo !zz+b", "!zz+b"),
            ("echo !zz@b", "!zz@b"),
            ("echo !zz~b", "!zz~b"),
            // A double quote ends it only inside a double-quoted string, where
            // it is the closing delimiter.
            ("echo \"x !zz\"b", "!zz"),
            // An empty search string before a delimiter matches nothing, and the
            // message is a bare `!`.
            ("echo !(zzz)", "!"),
            ("echo !;x", "!"),
            ("echo !|x", "!"),
            ("echo !)", "!"),
        ] {
            assert_eq!(failure(line), format!("{spec}: event not found"), "for {line}");
        }
        // Before a designator, though, an empty search string means the previous
        // event — that is what makes `!:0` and `!$` work.
        assert_eq!(expanded("echo !:0", &["first alpha beta"]), "echo first");
        assert_eq!(expanded("echo !$", &["first alpha beta"]), "echo beta");
    }

    /// Word designators: which words each shape names, and which shapes bash
    /// rejects outright with `bad word specifier` instead of clamping.
    ///
    /// Every expectation was measured against bash 5.2 by recording `a b c d`
    /// (and a one-word event, to reach the "no such word" edges) with
    /// `history -s` and reading back `echo X!!:…X` from an interactive shell —
    /// the interactive path, because `history -p` masks the message body with
    /// its own `history expansion failed`.
    #[test]
    fn word_designators_name_a_range_and_reject_one_that_is_out_of_range() {
        let four = hist(&["a b c d"]);
        let one = hist(&["onlyone"]);
        let sel = |items: &[String], spec: &str| -> Result<String, String> {
            let line = format!("echo X{spec}X");
            match expand1(&line, &ctx(items)) {
                Expansion::Changed(s) | Expansion::ChangedQuietly(s) => Ok(s
                    .strip_prefix("echo X")
                    .and_then(|s| s.strip_suffix('X'))
                    .unwrap_or(&s)
                    .to_string()),
                Expansion::NotFound(e) => Err(e),
                other => panic!("unexpected {other:?} for {spec}"),
            }
        };
        // The plain shapes. Note `!!:$-` and `!!:*-`: a `$` or `*` ends the
        // designator, so the following `-` survives as literal text, while a
        // `^` or a number does not (`!!:^-`, `!!:2-` are ranges).
        for (spec, want) in [
            ("!!:0", "a"),
            ("!!:1", "b"),
            ("!!:3", "d"),
            ("!!:^", "b"),
            ("!!:$", "d"),
            ("!!:*", "b c d"),
            ("!$", "d"),
            ("!^", "b"),
            ("!*", "b c d"),
            ("!!^", "b"),
            ("!!$", "d"),
            ("!!*", "b c d"),
            ("!!-", "a b c"),
            // A number, unlike `^`/`$`/`*`/`-`, needs the colon: without one it
            // is literal text after the whole event.
            ("!!1", "a b c d1"),
            ("!!2", "a b c d2"),
            // Ranges.
            ("!!:1-2", "b c"),
            ("!!:0-0", "a"),
            ("!!:3-3", "d"),
            ("!!:0-$", "a b c d"),
            ("!!:^-$", "b c d"),
            ("!!:^-^", "b"),
            ("!!:1-^", "b"),
            ("!!:2-", "c"),
            ("!!:0-", "a b c"),
            // A start with no end at all runs to the second-to-last word, which
            // may be *before* the start — empty, and not an error.
            ("!!:3-", ""),
            ("!!:^-", "b c"),
            ("!!:-", "a b c"),
            ("!!:-0", "a"),
            ("!!:-2", "a b c"),
            ("!!:-$", "a b c d"),
            ("!!:-^", "a b"),
            ("!!-2", "a b c"),
            ("!!-$", "a b c d"),
            ("!!^-2", "b c"),
            // `n*` — to the last word.
            ("!!:0*", "a b c d"),
            ("!!:3*", "d"),
            ("!!:^*", "b c d"),
            // A bare `^` may stand in for `-^` as the end of a range…
            ("!!:0^", "a b"),
            ("!!:1^", "b"),
            ("!!:^^", "b"),
            // …but only one of them, and a `$` or a number never does.
            ("!!:^^^", "b^"),
            ("!!:^2", "b2"),
            ("!!:2$", "c$"),
            ("!!:^$", "b$"),
            ("!!:1-^^", "b^"),
            ("!!:-^^", "a b^"),
            // `$`, `*` and a defaulted range end all stop the scan.
            ("!!:$-", "d-"),
            ("!!:$$", "d$"),
            ("!!:$^", "d^"),
            ("!!:$*", "d*"),
            ("!!:$z", "dz"),
            ("!!$-", "d-"),
            ("!!:*-", "b c d-"),
            ("!!:*$", "b c d$"),
            ("!!:*^", "b c d^"),
            ("!!:*x", "b c dx"),
            ("!!*-", "b c d-"),
            ("!!:--", "a b c-"),
            ("!!:1-*", "b c*"),
            ("!!:^z", "bz"),
            ("!!:^-z", "b cz"),
            ("!!:2*x", "c dx"),
        ] {
            assert_eq!(sel(&four, spec).as_deref(), Ok(want), "for {spec}");
        }
        // Out of range: a start past the last word, an end past the last word,
        // or a written-out end before the start. The message quotes back the
        // designator only — `!!:4:h` reports `:4`, not `:4:h`.
        for spec in ["!!:4", "!!:9", "!!:1-9", "!!:0-9", "!!:3-1", "!!:4-", "!!:9*", "!!:4*"] {
            let body = spec.strip_prefix("!!").unwrap_or(spec);
            assert_eq!(
                sel(&four, spec),
                Err(format!("{body}: bad word specifier")),
                "for {spec}"
            );
        }
        assert_eq!(sel(&four, "!!:4:h"), Err(":4: bad word specifier".to_string()));
        assert_eq!(sel(&four, "!!:-9"), Err(":-9: bad word specifier".to_string()));
        // The same edges on a one-word event, where word 1 does not exist. `*`
        // is the exception: it names words 1 through the last and is simply
        // empty here rather than an error — and so is a defaulted range end.
        for (spec, want) in
            [("!!:0", "onlyone"), ("!!:$", "onlyone"), ("!$", "onlyone"), ("!!:*", ""), ("!*", "")]
        {
            assert_eq!(sel(&one, spec).as_deref(), Ok(want), "for {spec}");
        }
        for (spec, want) in
            [("!!:-", ""), ("!!:-0", "onlyone"), ("!!:0-0", "onlyone"), ("!!:0*", "onlyone")]
        {
            assert_eq!(sel(&one, spec).as_deref(), Ok(want), "for {spec}");
        }
        for spec in ["!!:^", "!^", "!!:2-", "!!:3-", "!!:3*", "!!:-2", "!!:1-2", "!!:^-$", "!!:^-^"]
        {
            assert!(sel(&one, spec).is_err(), "{spec} should be out of range");
        }
        // `$` still ends the designator on a one-word event.
        assert_eq!(sel(&one, "!!:$-").as_deref(), Ok("onlyone-"));
        assert_eq!(sel(&one, "!!:*-").as_deref(), Ok("-"));
        assert_eq!(sel(&one, "!!:--").as_deref(), Ok("-"));
    }

    /// `%` — the word the most recent `?string?` search matched. Measured
    /// against bash 5.2 with `history -p`; the cross-line half of it by
    /// following one `history -p` with another, since the memory outlives the
    /// line that set it. See `tests/corpus/histexpand-percent.sh`.
    #[test]
    fn a_percent_designator_names_the_word_the_last_search_matched() {
        // Which word: the one holding the *last* occurrence of the search
        // string, because readline scans the matched line backwards.
        assert_eq!(search_match("xa ya za", "a"), "za");
        assert_eq!(search_match("alpha beta gamma", "ha bet"), "alpha");
        assert_eq!(search_match("alpha beta gamma", "gam"), "gamma");
        // An operator is a word of its own, so a match starting on one is that
        // word; whitespace is in no word, so a match starting there is nothing.
        assert_eq!(search_match("alpha; beta gamma", "; bet"), ";");
        assert_eq!(search_match("alpha (beta) gamma", "(bet"), "(");
        assert_eq!(search_match("alpha beta gamma", " bet"), "");
        assert_eq!(search_match("alpha beta gamma", "nope"), "");

        // The history is fixed; only the shell's expansion state carries over,
        // which is exactly what these sequences exercise.
        let items = hist(&["alpha beta gamma", "one two three"]);
        let run = |lines: &[&str]| -> Vec<Result<String, String>> {
            let mut state = HistState::default();
            lines
                .iter()
                .map(|line| match expand(line, &ctx(&items), &mut state) {
                    Expansion::Changed(s)
                    | Expansion::ChangedQuietly(s)
                    | Expansion::PrintOnly(s) => Ok(s),
                    Expansion::Unchanged => Ok((*line).to_string()),
                    Expansion::NotFound(e) => Err(e),
                })
                .collect()
        };
        let one = |line: &str| -> Result<String, String> {
            run(&[line]).pop().unwrap_or_else(|| panic!("no result for {line}"))
        };

        // With no search yet it is empty — and empty is not a failure, on any
        // event, including one that has no word 1 to reach for.
        assert_eq!(one("echo X!%X").as_deref(), Ok("echo XX"));
        assert_eq!(one("echo X!!:%X").as_deref(), Ok("echo XX"));
        assert_eq!(one("echo X!!%X").as_deref(), Ok("echo XX"));

        // A search records the word, for this line and for later ones. Note the
        // third line: the event it applies to has no `gamma` in it at all.
        assert_eq!(
            run(&["echo !?gam?:%", "echo X!%X", "echo X!!:%X !!:1"]),
            [
                Ok("echo gamma".to_string()),
                Ok("echo XgammaX".to_string()),
                Ok("echo XgammaX two".to_string()),
            ]
        );

        // Like `$` it ends the designator, so what follows is literal text; and
        // where a range endpoint would go it is literal too, leaving the range
        // with the end it would have had with nothing there.
        for (spec, want) in [
            ("!!:%*", "gamma*"),
            ("!!:%-", "gamma-"),
            ("!!:%%", "gamma%"),
            ("!!:%1", "gamma1"),
            ("!!:1%", "two%"),
            ("!!:1-%", "two%"),
            ("!!:-%", "one two%"),
            ("!!:*%", "two three%"),
            ("!!:$%", "three%"),
            ("!!:%:q", "'gamma'"),
        ] {
            let got = run(&["echo !?gam?:%", &format!("echo X{spec}X")]).pop();
            assert_eq!(got.as_ref().map(|r| r.as_deref()), Some(Ok(&*format!("echo X{want}X"))));
        }

        // A prefix search does not record one, and neither does a quick
        // substitution: both leave the previous match in place.
        assert_eq!(
            run(&["echo !?gam?:%", "echo !one", "echo X!%X"]).pop(),
            Some(Ok("echo XgammaX".to_string()))
        );
        assert_eq!(
            run(&["echo !?gam?:%", "^one^ONE^", "echo X!%X"]).pop(),
            Some(Ok("echo XgammaX".to_string()))
        );
        // A failed search leaves it alone too…
        let after_failure = run(&["echo !?gam?:%", "echo !?zzz?", "echo X!%X"]);
        assert_eq!(after_failure.get(1), Some(&Err("!?zzz?: event not found".to_string())));
        assert_eq!(after_failure.get(2).map(|r| r.as_deref()), Some(Ok("echo XgammaX")));
        // …while a search that succeeds and only then fails still records it.
        let then_bad = run(&["echo !?gam?:9", "echo X!%X"]);
        assert_eq!(then_bad.first(), Some(&Err(":9: bad word specifier".to_string())));
        assert_eq!(then_bad.get(1).map(|r| r.as_deref()), Some(Ok("echo XgammaX")));

        // An empty search string means the previous one, so `!??` finds the
        // older event rather than the most recent — and a `!??` that finds
        // nothing quotes back what was written, not what it searched for.
        assert_eq!(
            run(&["echo !?gam?:%", "echo !??"]).pop(),
            Some(Ok("echo alpha beta gamma".to_string()))
        );
        assert_eq!(one("echo !??"), Err("!??: event not found".to_string()));
        assert_eq!(one("echo !?"), Err("!?: event not found".to_string()));
        // Whatever the failure, the body is the designator as written.
        assert_eq!(one("echo !?zzz"), Err("!?zzz: event not found".to_string()));
        assert_eq!(one("echo x !?zzz?y"), Err("!?zzz?: event not found".to_string()));
        assert_eq!(
            run(&["echo !?zzz?:%", "echo !??"]).pop(),
            Some(Err("!??: event not found".to_string()))
        );
    }

    /// readline's `history_comment_char`: an unquoted `#` starting a word makes
    /// the rest of the line literal. Measured the same way as
    /// [`shell_syntax_inhibits_expansion`].
    #[test]
    fn a_comment_makes_the_rest_of_the_line_literal() {
        let h = hist(&["one"]);
        let unchanged = |line: &str| {
            let c = HistCtx { entries: &h, base: 1, extglob: false, open_quote: None };
            matches!(expand1(line, &c), Expansion::Unchanged)
        };
        // At index 0, and after each of readline's `history_word_delimiters`.
        for line in [
            "#!q",
            "# !q",
            "echo x # !q",
            "echo x\t#!q",
            "echo x;# !q",
            "echo x&#!q",
            ": $(echo z)#!q",
            "(:)#!q",
            ": a|#!q",
            ": <<<x #!q",
            "echo a >f #!q",
        ] {
            assert!(unchanged(line), "{line} should not expand");
        }
        // A `#` that does not start a word is not a comment…
        assert!(!unchanged("echo a#!q"), "echo a#!q should expand");
        // …and quoting the `#` takes its comment power away, in either quote,
        // even though double quotes do not protect the `!` itself.
        assert!(!unchanged("echo \"text # !q\""), "a quoted # should not comment");
        assert!(!unchanged(": '#' !q"), "a quoted # should not comment");
        // A comment only shields what follows it: an earlier `!` still expands.
        let c = HistCtx { entries: &h, base: 1, extglob: false, open_quote: None };
        match expand1("echo !! # !q", &c) {
            Expansion::Changed(s) => assert_eq!(s, "echo one # !q"),
            _ => panic!("expected the leading !! to expand"),
        }
    }

    #[test]
    fn word_designators() {
        let h = ["echo aa bb cc dd"];
        assert_eq!(expanded("echo !!:1", &h), "echo aa");
        assert_eq!(expanded("echo !$", &h), "echo dd");
        assert_eq!(expanded("echo !^", &h), "echo aa");
        assert_eq!(expanded("echo !!:1-2", &h), "echo aa bb");
        assert_eq!(expanded("echo !!:2*", &h), "echo bb cc dd");
        assert_eq!(expanded("echo !*", &h), "echo aa bb cc dd");
    }

    #[test]
    fn pathname_modifiers() {
        let h = ["echo /usr/local/lib/file.tar.gz"];
        assert_eq!(expanded("echo !!:$:h", &h), "echo /usr/local/lib");
        assert_eq!(expanded("echo !!:$:t", &h), "echo file.tar.gz");
        assert_eq!(expanded("echo !!:$:r", &h), "echo /usr/local/lib/file.tar");
        assert_eq!(expanded("echo !!:$:e", &h), "echo .gz");
    }

    /// readline's pathname modifiers are one `strrchr` each over the whole
    /// selected text: no basename, no leading-separator special case, and the
    /// text unchanged when the separator is missing. Every expectation here was
    /// measured against bash 5.2.
    #[test]
    fn pathname_modifiers_are_naive_strrchr() {
        // No separator at all leaves the text alone rather than emptying it.
        let h = ["echo abc"];
        assert_eq!(expanded("echo !!:h", &h), "echo echo abc");
        assert_eq!(expanded("echo !!:t", &h), "echo echo abc");
        assert_eq!(expanded("echo !!:r", &h), "echo echo abc");
        assert_eq!(expanded("echo !!:e", &h), "echo echo abc");
        // A leading `/` is not preserved as a root: `:h` keeps only what is
        // before it, which for the bare word is nothing.
        let h = ["echo /abc"];
        assert_eq!(expanded("echo !!:1:h", &h), "echo ");
        assert_eq!(expanded("echo !!:1:t", &h), "echo abc");
        // A dot in a directory name counts, and so does a leading one.
        let h = ["echo a.b/c"];
        assert_eq!(expanded("echo !!:1:r", &h), "echo a");
        assert_eq!(expanded("echo !!:1:e", &h), "echo .b/c");
        let h = ["echo .abc"];
        assert_eq!(expanded("echo !!:1:r", &h), "echo ");
        assert_eq!(expanded("echo !!:1:e", &h), "echo .abc");
    }

    /// A `:` commits to a modifier: an unknown letter or a line that ends right
    /// after the `:` aborts the expansion. A character that is *not* preceded by
    /// a `:` merely ends the modifier run and stays literal.
    #[test]
    fn unrecognized_modifier_aborts_the_expansion() {
        let h = hist(&["echo abc"]);
        for (line, name) in [
            ("echo !!:z", "z"),
            ("echo !!:1:z", "z"),
            ("echo !!:s^abc^x^:z", "z"),
            ("echo !!:Z", "Z"),
            // The `g`/`a` prefix is consumed first, so the *following*
            // character is the one bash names.
            ("echo !!:gz", "z"),
            // Nothing after the `:` at all: bash names an empty character.
            ("echo !!:", ""),
            ("echo !!:g", ""),
            ("echo !!:h:", ""),
        ] {
            match expand1(line, &ctx(&h)) {
                Expansion::NotFound(msg) => {
                    assert_eq!(msg, format!("{name}: unrecognized history modifier"), "{line}");
                }
                other => panic!("expected {line} to fail, got {other:?}"),
            }
        }
        // Not a `:`, so it is literal text appended to the modified event.
        assert_eq!(expanded("echo !!:hz", &["echo abc"]), "echo echo abcz");
        // `:s` with no delimiter at all is consumed and changes nothing.
        assert_eq!(expanded("echo !!:s", &["echo abc"]), "echo echo abc");
    }

    #[test]
    fn substitution_modifier() {
        assert_eq!(
            expanded("!!:s/two/TWO/", &["echo one two three"]),
            "echo one TWO three"
        );
        assert_eq!(
            expanded("!!:gs/a/A/", &["echo banana"]),
            "echo bAnAnA"
        );
    }

    #[test]
    fn quick_substitution_rewrites_previous() {
        assert_eq!(expanded("^world^planet^", &["echo hello world"]), "echo hello planet");
        // The trailing delimiter may be omitted.
        assert_eq!(expanded("^world^planet", &["echo hello world"]), "echo hello planet");
    }

    /// `^old^new^` is `!!:s` prefixed to the line, so text after the closing
    /// delimiter is part of the rewritten line and gets expanded with it.
    #[test]
    fn quick_substitution_keeps_what_follows_the_delimiter() {
        let h = ["echo one two"];
        assert_eq!(expanded("^one^X^ tail", &h), "echo X two tail");
        assert_eq!(expanded("^one^X^^", &h), "echo X two^");
        assert_eq!(expanded("^one^X^y", &h), "echo X twoy");
        assert_eq!(expanded("^one^X^ | cat", &h), "echo X two | cat");
        // A second event reference in the tail is expanded in its turn.
        assert_eq!(expanded("^one^X^ !!", &h), "echo X two echo one two");
        // The modifier scan is reached, and rejects what bash rejects.
        match expand1("^one^X^:2", &ctx(&hist(&h))) {
            Expansion::NotFound(msg) => assert_eq!(msg, "2: unrecognized history modifier"),
            other => panic!("expected a rejected modifier, got {other:?}"),
        }
        match expand1("^one^X^:p", &ctx(&hist(&h))) {
            Expansion::PrintOnly(s) => assert_eq!(s, "echo X two"),
            other => panic!("expected a preview, got {other:?}"),
        }
    }

    /// Because the rewrite is textual, the failure is the *rewritten* line's:
    /// with no history at all it is the `!!` that cannot be resolved, and the
    /// substitution is never reached.
    #[test]
    fn quick_substitution_failures_name_the_rewritten_spec() {
        let empty = hist(&[]);
        for line in ["^a^b^", "^a^b^ tail"] {
            match expand1(line, &ctx(&empty)) {
                Expansion::NotFound(msg) => assert_eq!(msg, "!!: event not found", "{line}"),
                other => panic!("expected {line} to fail, got {other:?}"),
            }
        }
        let h = hist(&["echo one"]);
        match expand1("^zz^two^", &ctx(&h)) {
            Expansion::NotFound(msg) => assert_eq!(msg, ":s^zz^two^: substitution failed"),
            other => panic!("expected a failed substitution, got {other:?}"),
        }
        // An empty `old` reuses the previous substitution, of which there is none.
        for line in ["^", "^^", "^^^"] {
            match expand1(line, &ctx(&h)) {
                Expansion::NotFound(msg) => {
                    assert_eq!(msg, format!(":s{line}: no previous substitution"), "{line}");
                }
                other => panic!("expected {line} to fail, got {other:?}"),
            }
        }
    }

    #[test]
    fn missing_event_is_reported() {
        let h = hist(&["echo one"]);
        match expand1("!999", &ctx(&h)) {
            Expansion::NotFound(msg) => assert_eq!(msg, "!999: event not found"),
            _ => panic!("expected not-found"),
        }
        match expand1("!nosuch", &ctx(&h)) {
            Expansion::NotFound(msg) => assert_eq!(msg, "!nosuch: event not found"),
            _ => panic!("expected not-found"),
        }
    }

    #[test]
    fn base_offsets_absolute_numbers() {
        // A stifled history whose first retained entry is number 5.
        let entries = hist(&["five", "six"]);
        let c = HistCtx {
            entries: &entries,
            base: 5,
            extglob: false,
            open_quote: None,
        };
        match expand1("!5", &c) {
            Expansion::Changed(s) => assert_eq!(s, "five"),
            _ => panic!("expected change"),
        }
        match expand1("!6", &c) {
            Expansion::Changed(s) => assert_eq!(s, "six"),
            _ => panic!("expected change"),
        }
        // Number 4 has been dropped off the front.
        assert!(matches!(expand1("!4", &c), Expansion::NotFound(_)));
    }
}
