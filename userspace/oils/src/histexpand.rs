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
pub enum Expansion {
    /// The line contained nothing to expand and is to be used as-is.
    Unchanged,
    /// The line was rewritten. bash echoes the result before running it.
    Changed(String),
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

/// Split a command into words the way history expansion counts them.
///
/// This is a deliberately simple whitespace split rather than a real lex: the
/// words are being sliced out of an already-recorded command line, and bash's
/// own word designators operate on the same shallow notion.
fn words(s: &str) -> Vec<&str> {
    s.split_whitespace().collect()
}

/// Apply a `:h`/`:t`/`:r`/`:e` pathname modifier.
fn modify_path(text: &str, which: char) -> String {
    match which {
        // head: everything before the last '/', or empty if there is none.
        'h' => match text.rfind('/') {
            Some(0) => "/".to_string(),
            Some(i) => text.get(..i).unwrap_or("").to_string(),
            None => String::new(),
        },
        // tail: the basename.
        't' => text.rsplit('/').next().unwrap_or(text).to_string(),
        // root: strip a trailing extension, looking only within the basename so
        // a dot in a parent directory name is not mistaken for one.
        'r' => split_ext(text).0.to_string(),
        // ext: the trailing extension, including its dot.
        'e' => split_ext(text).1.to_string(),
        _ => text.to_string(),
    }
}

/// Split `text` into (root, extension) at the last dot of its basename. The
/// extension includes the dot and is empty when there is none.
fn split_ext(text: &str) -> (&str, &str) {
    let base_at = text.rfind('/').map_or(0, |i| i.saturating_add(1));
    let base = text.get(base_at..).unwrap_or("");
    // A leading dot is part of the name, not an extension separator.
    match base.rfind('.').filter(|&i| i > 0) {
        Some(i) => {
            let cut = base_at.saturating_add(i);
            (text.get(..cut).unwrap_or(""), text.get(cut..).unwrap_or(""))
        }
        None => (text, ""),
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

/// The most recent `s/old/new/`, remembered so a later `:&` can repeat it.
///
/// bash keeps this for the life of the *shell*, not of one expansion: a `:&` on
/// one line repeats the `:s` from a line expanded earlier, and an empty pattern
/// (`:s//new/`) reuses that same `old`. So the caller owns it and threads it
/// through every [`expand`] call.
#[derive(Default, Clone)]
pub struct LastSubst {
    old: String,
    new: String,
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

/// Select the words of `event` named by a word designator, returning the
/// selected text and the index just past the designator.
///
/// bash lets the `:` be omitted when the designator begins with `^`, `$`, `*`
/// or `-`, which is what makes `!$` and `!!:1` both work.
fn apply_word_designator(event: &str, chars: &[char], start: usize) -> (String, usize) {
    let ws = words(event);
    let last = ws.len().saturating_sub(1);
    let mut i = start;
    let had_colon = chars.get(i) == Some(&':');
    if had_colon {
        i = i.saturating_add(1);
    }

    // Parse an endpoint: a number, `^` (1), `$` (last).
    let read_point = |chars: &[char], i: &mut usize| -> Option<usize> {
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

    let pick = |from: usize, to: usize| -> String {
        let to = to.min(last);
        if from > to {
            return String::new();
        }
        ws.get(from..=to).unwrap_or(&[]).join(" ")
    };

    match chars.get(i) {
        // `*` — all arguments, i.e. words 1..last. Empty when there are none.
        Some('*') => {
            i = i.saturating_add(1);
            (pick(1, last), i)
        }
        // `-m` — words 0 through m.
        Some('-') => {
            i = i.saturating_add(1);
            let end = read_point(chars, &mut i).unwrap_or(last);
            (pick(0, end), i)
        }
        Some(_) => {
            let Some(from) = read_point(chars, &mut i) else {
                // Not a word designator after all; leave the whole event and let
                // the caller reconsider this position as a modifier.
                if had_colon {
                    i = start;
                }
                return (event.to_string(), i);
            };
            match chars.get(i) {
                // `n*` — from n to the end.
                Some('*') => {
                    i = i.saturating_add(1);
                    (pick(from, last), i)
                }
                Some('-') => {
                    i = i.saturating_add(1);
                    match read_point(chars, &mut i) {
                        // `n-m` — an explicit range.
                        Some(to) => (pick(from, to), i),
                        // `n-` — from n to the second-to-last word.
                        None => (pick(from, last.saturating_sub(1)), i),
                    }
                }
                // A single word.
                _ => (pick(from, from), i),
            }
        }
        None => (event.to_string(), i),
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
    last: &mut LastSubst,
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
            // `:q` quotes the result as one word, `:x` as one word per
            // whitespace-separated field, so a later expansion pass leaves it
            // alone. bash uses single quotes for both.
            Some(&c @ ('q' | 'x')) => {
                text = if c == 'q' {
                    single_quote(&text)
                } else {
                    words(&text).iter().map(|w| single_quote(w)).collect::<Vec<_>>().join(" ")
                };
                i = j.saturating_add(1);
            }
            Some('s') => {
                let Some((old, new, end)) = parse_subst(chars, j.saturating_add(1)) else {
                    break;
                };
                // An empty pattern reuses the previous one, as in bash.
                let reused = old.is_empty();
                let old = if reused { last.old.clone() } else { old };
                if old.is_empty() {
                    return Err(format!("{}: no previous substitution", spec_text(chars, i, end)));
                }
                if !text.contains(&old) {
                    return Err(format!("{}: substitution failed", spec_text(chars, i, end)));
                }
                text = substitute(&text, &old, &new, global);
                *last = LastSubst { old, new };
                i = end;
            }
            Some('&') => {
                let end = j.saturating_add(1);
                if last.old.is_empty() {
                    return Err(format!("{}: no previous substitution", spec_text(chars, i, end)));
                }
                if !text.contains(&last.old) {
                    return Err(format!("{}: substitution failed", spec_text(chars, i, end)));
                }
                text = substitute(&text, &last.old, &last.new, global);
                i = end;
            }
            // Not a modifier we recognise — stop, leaving the `:` in place.
            _ => break,
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

/// Expand `line` against `ctx`.
///
/// Returns [`Expansion::Unchanged`] when the line held no history reference, so
/// the caller can skip the "echo the expanded line" behaviour entirely.
///
/// `last` carries the most recent `:s` across calls — see [`LastSubst`].
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
/// inhibited even though nothing about the `!` itself says so, and double quotes
/// inhibit nothing at all.
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
    if prev == Some('[') && rest.contains(&']') {
        return true;
    }
    false
}

pub fn expand(line: &str, ctx: &HistCtx, last: &mut LastSubst) -> Expansion {
    // `^old^new^` is a whole-line form: it only applies when it is the first
    // thing on the line.
    if line.starts_with('^') {
        return quick_substitution(line, ctx, last);
    }
    if !line.contains('!') {
        return Expansion::Unchanged;
    }

    let chars: Vec<char> = line.chars().collect();
    let mut out = String::new();
    let mut i = 0usize;
    let mut changed = false;
    let mut print_only = false;
    let mut in_single = false;
    // Tracked *only* for the comment rule below — a `!` in a double-quoted string
    // is still expanded, because the rewrite happens before quote removal.
    let mut in_double = false;
    let last_subst = last;

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

        match expand_one(&chars, i, ctx, last_subst, in_double) {
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
        (false, false) => Expansion::Unchanged,
    }
}

/// Expand the single designator starting at `chars[start]` (which is the `!`).
/// On success returns the replacement text and the index just past it; on
/// failure the whole message body bash would print (see [`Expansion::NotFound`]).
/// readline's `history_word_delimiters`, `" \t\n;&()|<>"`. It ends an event
/// search string and marks where a `#` may start a comment.
fn is_word_delimiter(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | ';' | '&' | '(' | ')' | '|' | '<' | '>')
}

fn expand_one(
    chars: &[char],
    start: usize,
    ctx: &HistCtx,
    last_subst: &mut LastSubst,
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
            let mut needle = String::new();
            while let Some(&c) = chars.get(i) {
                if c == '?' {
                    i = i.saturating_add(1);
                    break;
                }
                needle.push(c);
                i = i.saturating_add(1);
            }
            ctx.by_substring(&needle)
                .map(str::to_string)
                .ok_or_else(|| missing(&format!("!?{needle}?")))?
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

    let (selected, after_words) = apply_word_designator(&event, chars, i);
    apply_modifiers(selected, chars, after_words, last_subst)
}

/// The raw text of a designator, for an "event not found" message.
fn spec_text(chars: &[char], start: usize, end: usize) -> String {
    chars.get(start..end).unwrap_or(&[]).iter().collect()
}

/// `^old^new^` — re-run the previous command with the first occurrence of `old`
/// replaced by `new`.
fn quick_substitution(line: &str, ctx: &HistCtx, last: &mut LastSubst) -> Expansion {
    let chars: Vec<char> = line.chars().collect();
    let Some((old, new, end)) = parse_subst(&chars, 0) else {
        return Expansion::Unchanged;
    };
    // bash rewrites `^old^new^` into the equivalent `:s` modifier before running
    // it, and its diagnostics quote back *that* form — `:s^nomatch^x^`, delimiter
    // and all — rather than the line as typed.
    let spec = format!(":s{}", spec_text(&chars, 0, end));
    let Some(prev) = ctx.back(1) else {
        return Expansion::NotFound(format!("{spec}: event not found"));
    };
    if old.is_empty() {
        return Expansion::NotFound(format!("{spec}: no previous substitution"));
    }
    if !prev.contains(&old) {
        return Expansion::NotFound(format!("{spec}: substitution failed"));
    }
    let text = substitute(prev, &old, &new, false);
    // Anything after the closing delimiter is appended, so `^a^b^:p` still
    // reaches the modifier logic.
    *last = LastSubst { old, new };
    match apply_modifiers(text, &chars, end, last) {
        Ok((modified, _i, true)) => Expansion::PrintOnly(modified),
        Ok((modified, _i, false)) => Expansion::Changed(modified),
        Err(msg) => Expansion::NotFound(msg),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(entries: &[String]) -> HistCtx<'_> {
        HistCtx { entries, base: 1, extglob: false }
    }

    fn hist(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_string()).collect()
    }

    /// [`expand`] with a throwaway last-substitution state, for the cases that
    /// do not exercise a `:&` carried over from an earlier line.
    fn expand1(line: &str, ctx: &HistCtx) -> Expansion {
        expand(line, ctx, &mut LastSubst::default())
    }

    fn expanded(line: &str, items: &[&str]) -> String {
        let h = hist(items);
        match expand1(line, &ctx(&h)) {
            Expansion::Changed(s) => s,
            Expansion::Unchanged => line.to_string(),
            Expansion::PrintOnly(s) => panic!("unexpected print-only: {s}"),
            Expansion::NotFound(e) => panic!("unexpected not-found: {e}"),
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
        let mut last = LastSubst::default();
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
            let c = HistCtx { entries: &h, base: 1, extglob };
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

    /// readline's `history_comment_char`: an unquoted `#` starting a word makes
    /// the rest of the line literal. Measured the same way as
    /// [`shell_syntax_inhibits_expansion`].
    #[test]
    fn a_comment_makes_the_rest_of_the_line_literal() {
        let h = hist(&["one"]);
        let unchanged = |line: &str| {
            let c = HistCtx { entries: &h, base: 1, extglob: false };
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
        let c = HistCtx { entries: &h, base: 1, extglob: false };
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
