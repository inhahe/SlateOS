//! POSIX pattern matching: `fnmatch()` (XSH `fnmatch`, XCU 2.13 "Pattern
//! Matching Notation"), with glibc's GNU flags.
//!
//! ## What it matches
//!
//! - `?` any one byte, `*` any string, `[...]` a bracket expression (XBD 9.3.5,
//!   with `!` for negation and `^` as well, as glibc and musl take it): single
//!   bytes, ranges, `[:class:]`, `[=c=]` and `[.c.]` -- the C locale's
//!   equivalence classes and collating symbols, which are single bytes. A `[`
//!   with no closing `]` is an ordinary character, as POSIX says.
//! - `\c`, unless `FNM_NOESCAPE`: `c` itself; a pattern ending in an
//!   unescaped `\` matches nothing.
//! - `FNM_PATHNAME`: a `/` is matched only by a `/` in the pattern -- never by
//!   `*`, `?` or a bracket expression.
//! - `FNM_PERIOD`: a leading `.` (the string's first byte, or with
//!   `FNM_PATHNAME` one after a `/`) only by a `.` in the pattern.
//! - glibc's: `FNM_LEADING_DIR`, a match of the pattern followed by `/` and
//!   anything; `FNM_CASEFOLD`, letters without their case; `FNM_EXTMATCH`,
//!   ksh's `?(a|b)` `*(a|b)` `+(a|b)` `@(a|b)` `!(a|b)`.
//!
//! Bytes, not characters: glibc's C locale's matching, and the locale this
//! library reports (known-issues.md -> open-questions D-Q7 is whether it
//! should report UTF-8's).
//!
//! glibc 2.39's answers are the oracle (`posix/tools/oracle/fnmatch_harness.py`,
//! `fnmatch_oracle.txt`: 42,828 patterns and flag sets against 50 strings each)
//! for everything POSIX and glibc's manual leave open -- what a bracket
//! expression with an unknown class, or an unterminated `[.`, or a range
//! whose ends are reversed, matches (nothing). Where glibc's answer
//! contradicts the standard or its own manual this follows the text, in three
//! places (design-decisions §1148; `fnmatch_deviations.txt` lists every case):
//!
//! | Pattern | Here | glibc 2.39 |
//! |---|---|---|
//! | `*\/`, `FNM_PATHNAME` | matches `a/`, as POSIX's escaped slash is a slash in the pattern | matches nothing |
//! | `*` then an extended group, `*@(a\|)` | matches all it can | misses every match whose group falls at the string's end, and reads `**(x)` as `*` |
//! | `FNM_LEADING_DIR` in an extended group | a directory the whole pattern matches, as the manual defines it | the flag applied inside some groups' alternatives and not others' |
//!
//! Until 2026-09-29 this knew three of the six flags, took an unterminated
//! `[` for a failed match, and read `[=a=]` and `[.a.]` as bracket
//! expressions of `=`, `a` and `.`: 16,603 of 2,017,500 answers were not
//! glibc's (known-issues.md ->
//! D-POSIX-FNMATCH-KNEW-HALF-ITS-FLAGS-AND-NO-EQUIVALENCE-CLASSES).
//!
//! ## How
//!
//! [`Matcher::run`] walks the pattern once, remembering only the last `*`:
//! when a later part fails, that star takes one more byte and the walk
//! resumes after it. That is enough -- a later `*` can absorb whatever an
//! earlier one would, and under `FNM_PATHNAME` neither crosses a `/` -- so a
//! pattern of many stars is linear in the string for each star, not
//! exponential. An extended group is matched by trying each end of the part
//! of the string it takes; the repeating ones (`*(...)`, `+(...)`) keep a
//! table of the ends reachable so far rather than recursing once per
//! repetition, so a long string cannot exhaust a thread's stack.

use crate::decfloat::MallocBuf;

/// Returned when the pattern does not match.
pub const FNM_NOMATCH: i32 = 1;
/// Wildcards and bracket expressions don't match `/`.
pub const FNM_PATHNAME: i32 = 1;
/// Backslash is an ordinary character.
pub const FNM_NOESCAPE: i32 = 2;
/// A leading `.` must be matched by a `.` in the pattern.
pub const FNM_PERIOD: i32 = 4;
/// Match a leading directory: the pattern, then `/` and anything (glibc).
pub const FNM_LEADING_DIR: i32 = 8;
/// Ignore the case of letters (glibc).
pub const FNM_CASEFOLD: i32 = 16;
/// ksh's extended patterns (glibc).
pub const FNM_EXTMATCH: i32 = 32;

/// No memory for an extended group's table of positions: `fnmatch`'s -1.
pub(crate) struct NoMemory;

/// Own archive member -- the matcher below is glob's too (glob.rs calls
/// [`matches`]), and a program that brings its own `fnmatch`, as GNU make
/// can, must still be able to link this library's `glob` without this
/// definition coming along. See string.rs's module header.
mod gnu_fnmatch {
    use super::{FNM_NOMATCH, NoMemory, matches};

    /// Match `string` against `pattern`: 0 if it matches, [`FNM_NOMATCH`]
    /// if not, -1 (with `errno` `ENOMEM`) if an extended pattern needed
    /// memory there was none of.
    ///
    /// # Safety
    ///
    /// Both must be NUL-terminated C strings (NULL is taken for no match).
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub unsafe extern "C" fn fnmatch(pattern: *const u8, string: *const u8, flags: i32) -> i32 {
        if pattern.is_null() || string.is_null() {
            return FNM_NOMATCH;
        }
        // SAFETY: C strings, the caller's.
        let (p, s) = unsafe {
            (
                core::slice::from_raw_parts(pattern, crate::string::strlen(pattern)),
                core::slice::from_raw_parts(string, crate::string::strlen(string)),
            )
        };
        match matches(p, s, flags) {
            Ok(true) => 0,
            Ok(false) => FNM_NOMATCH,
            Err(NoMemory) => {
                crate::errno::set_errno(crate::errno::ENOMEM);
                -1
            }
        }
    }
}
pub use gnu_fnmatch::fnmatch;

/// [`fnmatch`] over byte slices.
pub(crate) fn matches(pattern: &[u8], string: &[u8], flags: i32) -> Result<bool, NoMemory> {
    Matcher { s: string }.run(pattern, 0, 0, string.len(), flags)
}

/// The string being matched, which every part of the pattern -- an
/// alternative of an extended group too -- is matched against a part of.
struct Matcher<'a> {
    s: &'a [u8],
}

/// What a bracket expression did with a byte.
enum Bracket {
    /// It took the byte; the pattern continues at this index.
    Took(usize),
    /// It did not.
    Refused,
    /// It has no closing `]`: its `[` is an ordinary character.
    Unterminated,
    /// It reached an element that matches nothing -- an unknown class, a
    /// collating symbol of no one byte, a `[.` or `[=` unclosed -- before
    /// any element took the byte: this attempt fails.
    Invalid,
}

const fn fold(b: u8, flags: i32) -> u8 {
    if flags & FNM_CASEFOLD != 0 {
        b.to_ascii_lowercase()
    } else {
        b
    }
}

/// The named class's test, for the C locale; `None` for no such class.
fn class(name: &[u8]) -> Option<fn(&u8) -> bool> {
    Some(match name {
        b"alpha" => u8::is_ascii_alphabetic,
        b"digit" => u8::is_ascii_digit,
        b"alnum" => u8::is_ascii_alphanumeric,
        b"upper" => u8::is_ascii_uppercase,
        b"lower" => u8::is_ascii_lowercase,
        b"space" => |b: &u8| matches!(*b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r'),
        b"blank" => |b: &u8| matches!(*b, b' ' | b'\t'),
        b"punct" => u8::is_ascii_punctuation,
        b"print" => |b: &u8| (0x20..=0x7e).contains(b),
        b"graph" => u8::is_ascii_graphic,
        b"cntrl" => u8::is_ascii_control,
        b"xdigit" => u8::is_ascii_hexdigit,
        _ => return None,
    })
}

/// A class name's bytes: `a` to `y`, as glibc reads them -- a `z`, or
/// anything else, makes `[:` an ordinary `[` and `:`.
const fn class_name_byte(b: u8) -> bool {
    b >= b'a' && b < b'z'
}

/// Where the `x]` closing a `[x` element that starts at `from` is: the index
/// of its `x`.
fn find_close(p: &[u8], from: usize, kind: u8) -> Option<usize> {
    let rest = p.get(from..)?;
    rest.windows(2)
        .position(|w| w == [kind, b']'])
        .map(|i| from.saturating_add(i))
}

/// After an element took the byte: the index past the bracket expression's
/// `]`, stepping over what is left of it without judging it; `None` if it
/// has no `]`, or an element it steps over has no end.
fn skip_rest(p: &[u8], mut i: usize, flags: i32) -> Option<usize> {
    loop {
        let c = *p.get(i)?;
        i = i.saturating_add(1);
        match c {
            b'\\' if flags & FNM_NOESCAPE == 0 => {
                p.get(i)?;
                i = i.saturating_add(1);
            }
            b'[' if p.get(i) == Some(&b':') => {
                let mut j = i.saturating_add(1);
                while p.get(j).is_some_and(|&b| class_name_byte(b)) {
                    j = j.saturating_add(1);
                }
                if p.get(j..j.saturating_add(2)) == Some(b":]") {
                    i = j.saturating_add(2);
                }
            }
            b'[' if p.get(i) == Some(&b'=') => {
                if p.get(i.saturating_add(2)..i.saturating_add(4)) != Some(b"=]") {
                    return None;
                }
                i = i.saturating_add(4);
            }
            b'[' if p.get(i) == Some(&b'.') => {
                i = find_close(p, i.saturating_add(1), b'.')?.saturating_add(2);
            }
            b']' => return Some(i),
            _ => {}
        }
    }
}

/// A single byte of a bracket expression, at `i`, as a range's end or an
/// element: `(byte, index after, is it a collating symbol)`; `None` for an
/// element that matches nothing.
fn bracket_byte(p: &[u8], i: usize, flags: i32) -> Option<(u8, usize, bool)> {
    let c = *p.get(i)?;
    if c == b'[' && p.get(i.saturating_add(1)) == Some(&b'.') {
        let end = find_close(p, i.saturating_add(2), b'.')?;
        let sym = p.get(i.saturating_add(2)..end)?;
        let [b] = *sym else { return None };
        return Some((b, end.saturating_add(2), true));
    }
    if c == b'\\' && flags & FNM_NOESCAPE == 0 {
        return Some((*p.get(i.saturating_add(1))?, i.saturating_add(2), false));
    }
    Some((c, i.saturating_add(1), false))
}

/// The bracket expression whose `[` is just before `pi`, against `byte`.
fn bracket(p: &[u8], pi: usize, byte: u8, flags: i32) -> Bracket {
    let ch = fold(byte, flags);
    let mut i = pi;
    let negate = matches!(p.get(i), Some(b'!' | b'^'));
    if negate {
        i = i.saturating_add(1);
    }
    let mut first = true;
    let took = |at: usize| match skip_rest(p, at, flags) {
        None => Bracket::Invalid,
        Some(_) if negate => Bracket::Refused,
        Some(after) => Bracket::Took(after),
    };
    loop {
        let Some(&c) = p.get(i) else {
            return Bracket::Unterminated;
        };
        if c == b']' && !first {
            return if negate {
                Bracket::Took(i.saturating_add(1))
            } else {
                Bracket::Refused
            };
        }
        first = false;
        let next = p.get(i.saturating_add(1)).copied();
        if c == b'[' && next == Some(b':') {
            let mut j = i.saturating_add(2);
            while p.get(j).is_some_and(|&b| class_name_byte(b)) {
                j = j.saturating_add(1);
            }
            if p.get(j..j.saturating_add(2)) == Some(b":]") {
                let Some(test) = p.get(i.saturating_add(2)..j).and_then(class) else {
                    return Bracket::Invalid;
                };
                i = j.saturating_add(2);
                // The byte as the string has it: glibc tests a class
                // unfolded.
                if test(&byte) {
                    return took(i);
                }
                continue;
            }
            // Not a class: this `[` is an ordinary byte of the expression.
        }
        if c == b'[' && next == Some(b'=') {
            let Some(end) = find_close(p, i.saturating_add(2), b'=') else {
                return Bracket::Invalid;
            };
            let Some(&[sym]) = p.get(i.saturating_add(2)..end) else {
                return Bracket::Invalid;
            };
            i = end.saturating_add(2);
            // Unfolded, as glibc compares an equivalence class.
            if sym == byte {
                return took(i);
            }
            continue;
        }
        let (lo, after, coll) = if c == b'[' && next == Some(b':') {
            (b'[', i.saturating_add(1), false)
        } else {
            match bracket_byte(p, i, flags) {
                Some(e) => e,
                None => return Bracket::Invalid,
            }
        };
        i = after;
        let hit =
            if p.get(i) == Some(&b'-') && p.get(i.saturating_add(1)).is_some_and(|&b| b != b']') {
                let Some((hi, after_hi, _)) = bracket_byte(p, i.saturating_add(1), flags) else {
                    return Bracket::Invalid;
                };
                i = after_hi;
                (fold(lo, flags) <= ch && ch <= fold(hi, flags))
                    || (flags & FNM_CASEFOLD != 0
                        && lo <= ch.to_ascii_uppercase()
                        && ch.to_ascii_uppercase() <= hi)
            } else if coll {
                // A collating symbol alone is compared unfolded, as glibc does.
                lo == byte
            } else {
                fold(lo, flags) == ch
            };
        if hit {
            return took(i);
        }
    }
}

/// Where the `)` closing the extended group whose `(` is at `open` is; `None`
/// if it has none. Bracket expressions and nested groups inside are stepped
/// over whole.
fn group_end(p: &[u8], open: usize) -> Option<usize> {
    let mut j = open.saturating_add(1);
    loop {
        let c = *p.get(j)?;
        if c == b'[' {
            j = skip_bracket(p, j)?;
            continue;
        }
        if matches!(c, b'?' | b'*' | b'+' | b'@' | b'!')
            && p.get(j.saturating_add(1)) == Some(&b'(')
        {
            j = group_end(p, j.saturating_add(1))?.saturating_add(1);
            continue;
        }
        if c == b')' {
            return Some(j);
        }
        j = j.saturating_add(1);
    }
}

/// `p[j]` is a `[` inside a group: the index past its `]`, as the group's
/// end is found -- the first `]` after the one a leading `]` may be.
fn skip_bracket(p: &[u8], j: usize) -> Option<usize> {
    let mut k = j.saturating_add(1);
    if matches!(p.get(k), Some(b'!' | b'^')) {
        k = k.saturating_add(1);
    }
    if p.get(k) == Some(&b']') {
        k = k.saturating_add(1);
    }
    while *p.get(k)? != b']' {
        k = k.saturating_add(1);
    }
    Some(k.saturating_add(1))
}

/// The `|`-separated alternatives of the group between `open` and `close`,
/// each as a range of `p`.
fn alternatives(p: &[u8], open: usize, close: usize) -> impl Iterator<Item = (usize, usize)> + '_ {
    let mut start = open.saturating_add(1);
    let mut j = start;
    let mut done = false;
    core::iter::from_fn(move || {
        if done {
            return None;
        }
        while j < close {
            let c = *p.get(j)?;
            if c == b'[' {
                j = skip_bracket(p, j)?;
                continue;
            }
            if matches!(c, b'?' | b'*' | b'+' | b'@' | b'!')
                && p.get(j.saturating_add(1)) == Some(&b'(')
            {
                j = group_end(p, j.saturating_add(1))?.saturating_add(1);
                continue;
            }
            if c == b'|' {
                let alt = (start, j);
                j = j.saturating_add(1);
                start = j;
                return Some(alt);
            }
            j = j.saturating_add(1);
        }
        done = true;
        Some((start, close))
    })
}

impl Matcher<'_> {
    /// Is `s[i]` a leading period, which only a `.` in the pattern matches?
    fn leading_period(&self, i: usize, flags: i32) -> bool {
        flags & FNM_PERIOD != 0
            && self.s.get(i) == Some(&b'.')
            && (i == 0
                || (flags & FNM_PATHNAME != 0
                    && i.checked_sub(1).and_then(|j| self.s.get(j)) == Some(&b'/')))
    }

    /// The single-byte part of the pattern at `pi` -- `?`, a bracket
    /// expression, an escaped byte or a plain one -- against `s[si]`, which
    /// is before `se`: the index after it if it takes the byte.
    fn one_byte(&self, p: &[u8], pi: usize, si: usize, se: usize, flags: i32) -> Option<usize> {
        if si >= se {
            return None;
        }
        let byte = *self.s.get(si)?;
        let c = *p.get(pi)?;
        let slash_barred = flags & FNM_PATHNAME != 0 && byte == b'/';
        match c {
            b'?' => {
                (!slash_barred && !self.leading_period(si, flags)).then(|| pi.saturating_add(1))
            }
            b'[' => {
                if self.leading_period(si, flags) || slash_barred {
                    return None;
                }
                match bracket(p, pi.saturating_add(1), byte, flags) {
                    Bracket::Took(after) => Some(after),
                    Bracket::Refused | Bracket::Invalid => None,
                    Bracket::Unterminated => {
                        (fold(b'[', flags) == fold(byte, flags)).then(|| pi.saturating_add(1))
                    }
                }
            }
            b'\\' if flags & FNM_NOESCAPE == 0 => {
                let lit = *p.get(pi.saturating_add(1))?;
                (fold(lit, flags) == fold(byte, flags)).then(|| pi.saturating_add(2))
            }
            _ => (fold(c, flags) == fold(byte, flags)).then(|| pi.saturating_add(1)),
        }
    }

    /// Does `p[pi..]` match `s[si..se]`?
    fn run(
        &self,
        p: &[u8],
        mut pi: usize,
        mut si: usize,
        se: usize,
        flags: i32,
    ) -> Result<bool, NoMemory> {
        let ext = flags & FNM_EXTMATCH != 0;
        let opens_group = |i: usize| {
            ext && matches!(p.get(i), Some(b'?' | b'*' | b'+' | b'@' | b'!'))
                && p.get(i.saturating_add(1)) == Some(&b'(')
        };
        // The last star: where the pattern resumes after it, and where in
        // the string its match ends so far.
        let mut star: Option<(usize, usize)> = None;
        loop {
            let advanced = if pi >= p.len() {
                if si == se
                    || (flags & FNM_LEADING_DIR != 0 && si < se && self.s.get(si) == Some(&b'/'))
                {
                    return Ok(true);
                }
                None
            } else if opens_group(pi) && group_end(p, pi.saturating_add(1)).is_some() {
                // The group and everything after it, at this place.
                if self.group(p, pi, si, se, flags)? {
                    return Ok(true);
                }
                None
            } else if p.get(pi) == Some(&b'*') {
                pi = pi.saturating_add(1);
                while p.get(pi) == Some(&b'*') && !opens_group(pi) {
                    pi = pi.saturating_add(1);
                }
                if self.leading_period(si, flags) {
                    None
                } else if pi >= p.len() {
                    // The pattern ends with the star: it takes the rest, which
                    // under FNM_PATHNAME must hold no `/` -- unless
                    // FNM_LEADING_DIR leaves what follows one aside.
                    let rest = self.s.get(si..se).unwrap_or(&[]);
                    return Ok(flags & FNM_PATHNAME == 0
                        || flags & FNM_LEADING_DIR != 0
                        || !rest.contains(&b'/'));
                } else {
                    star = Some((pi, si));
                    continue;
                }
            } else {
                self.one_byte(p, pi, si, se, flags)
            };
            match advanced {
                Some(next) => {
                    pi = next;
                    si = si.saturating_add(1);
                }
                None => {
                    // Back to the last star: it takes one more byte.
                    let Some((spi, ssi)) = star else {
                        return Ok(false);
                    };
                    if ssi >= se || (flags & FNM_PATHNAME != 0 && self.s.get(ssi) == Some(&b'/')) {
                        return Ok(false);
                    }
                    let ssi = ssi.saturating_add(1);
                    star = Some((spi, ssi));
                    pi = spi;
                    si = ssi;
                }
            }
        }
    }

    /// Does one of the group's alternatives match `s[from..to]`?
    fn any_alternative(
        &self,
        p: &[u8],
        open: usize,
        close: usize,
        from: usize,
        to: usize,
        flags: i32,
    ) -> Result<bool, NoMemory> {
        // An alternative matches a part of the string, whose tail
        // FNM_LEADING_DIR does not concern.
        let sub = flags & !FNM_LEADING_DIR;
        for (a0, a1) in alternatives(p, open, close) {
            let alt = p.get(a0..a1).unwrap_or(&[]);
            if self.run(alt, 0, from, to, sub)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// The extended group at `p[pi]` (`?*+@!` and its `(`), and the pattern
    /// after it, against `s[si..se]`.
    fn group(
        &self,
        p: &[u8],
        pi: usize,
        si: usize,
        se: usize,
        flags: i32,
    ) -> Result<bool, NoMemory> {
        let open = pi.saturating_add(1);
        let Some(close) = group_end(p, open) else {
            return Ok(false);
        };
        let rest = close.saturating_add(1);
        let kind = p.get(pi).copied().unwrap_or(b'@');
        match kind {
            b'@' | b'?' => {
                if kind == b'?' && self.run(p, rest, si, se, flags)? {
                    return Ok(true);
                }
                for k in si..=se {
                    if self.any_alternative(p, open, close, si, k, flags)?
                        && self.run(p, rest, k, se, flags)?
                    {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            b'!' => {
                for k in si..=se {
                    if !self.any_alternative(p, open, close, si, k, flags)?
                        && self.run(p, rest, k, se, flags)?
                    {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            // `*` and `+`: the ends one or more matches in a row can reach,
            // a table of them, so no repetition costs a stack frame.
            _ => {
                let n = se.saturating_sub(si).saturating_add(1);
                let mut reach = MallocBuf::<u8>::zeroed(n).ok_or(NoMemory)?;
                let table = reach.as_mut();
                if kind == b'*' {
                    if let Some(first) = table.first_mut() {
                        *first = 1;
                    }
                } else {
                    for k in si..=se {
                        if self.any_alternative(p, open, close, si, k, flags)? {
                            if let Some(t) = table.get_mut(k.saturating_sub(si)) {
                                *t = 1;
                            }
                        }
                    }
                }
                for i in si..=se {
                    if table.get(i.saturating_sub(si)) != Some(&1) {
                        continue;
                    }
                    if self.run(p, rest, i, se, flags)? {
                        return Ok(true);
                    }
                    for k in i.saturating_add(1)..=se {
                        if table.get(k.saturating_sub(si)) != Some(&1)
                            && self.any_alternative(p, open, close, i, k, flags)?
                        {
                            if let Some(t) = table.get_mut(k.saturating_sub(si)) {
                                *t = 1;
                            }
                        }
                    }
                }
                Ok(false)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::string::String;
    use std::vec::Vec;

    /// Helper: call `fnmatch` with byte slices (must be null-terminated).
    fn matches(pat: &[u8], s: &[u8], flags: i32) -> bool {
        let result = unsafe { fnmatch(pat.as_ptr(), s.as_ptr(), flags) };
        result == 0
    }

    /// `m("pattern", "string", flags)` over `str`s.
    fn m(p: &str, s: &str, flags: i32) -> bool {
        super::matches(p.as_bytes(), s.as_bytes(), flags).unwrap_or(false)
    }

    // -- glibc 2.39's answers, and where they are not these ---------------

    /// `\xNN` escapes back to bytes, as `fnmatch_harness.py` writes them;
    /// `\x` alone is the empty string.
    fn unescape(t: &str) -> Vec<u8> {
        let b = t.as_bytes();
        let mut out = Vec::new();
        let mut i = 0;
        while i < b.len() {
            if b[i] == b'\\' && b.get(i + 1) == Some(&b'x') {
                if i + 4 <= b.len() {
                    out.push(u8::from_str_radix(&t[i + 2..i + 4], 16).unwrap());
                    i += 4;
                } else {
                    i += 2;
                }
            } else {
                out.push(b[i]);
                i += 1;
            }
        }
        out
    }

    fn flag_set(names: &str) -> i32 {
        names.split('|').fold(0, |f, name| {
            f | match name {
                "0" => 0,
                "PATHNAME" => FNM_PATHNAME,
                "NOESCAPE" => FNM_NOESCAPE,
                "PERIOD" => FNM_PERIOD,
                "LEADING_DIR" => FNM_LEADING_DIR,
                "CASEFOLD" => FNM_CASEFOLD,
                "EXTMATCH" => FNM_EXTMATCH,
                other => panic!("{other}"),
            }
        })
    }

    /// Every case of `fnmatch_oracle.txt` answered as glibc 2.39 answers
    /// it -- except those `fnmatch_deviations.txt` lists, answered as it
    /// says (design-decisions §1148).
    #[test]
    fn fnmatch_is_glibcs_but_where_glibc_is_not_its_standard() {
        let mut deviations = std::collections::HashMap::new();
        for line in include_str!("fnmatch_deviations.txt")
            .lines()
            .filter(|l| !l.starts_with('#'))
        {
            let f: Vec<&str> = line.split(' ').collect();
            let [flags, pat, s, glibc, here] = f[..] else {
                panic!("{line}")
            };
            deviations.insert(
                (flags.to_string(), unescape(pat), unescape(s)),
                (glibc.chars().next().unwrap(), here.chars().next().unwrap()),
            );
        }
        let mut lines = include_str!("fnmatch_oracle.txt").lines();
        let strings: Vec<Vec<u8>> = loop {
            if let Some(rest) = lines.next().unwrap().strip_prefix("# strings: ") {
                break rest.split(' ').map(unescape).collect();
            }
        };
        let (mut n, mut used) = (0usize, 0usize);
        let mut bad = Vec::new();
        for line in lines {
            let mut parts = line.split(' ');
            let (flags, pat, answers) = (
                parts.next().unwrap(),
                parts.next().unwrap(),
                parts.next().unwrap(),
            );
            let p = unescape(pat);
            let f = flag_set(flags);
            for (s, glibc) in strings.iter().zip(answers.chars()) {
                let want = match deviations.get(&(flags.to_string(), p.clone(), s.clone())) {
                    Some(&(g, here)) => {
                        assert_eq!(g, glibc, "the deviation list is stale for {flags} {pat}");
                        used += 1;
                        here
                    }
                    None => glibc,
                };
                let got = match super::matches(&p, s, f) {
                    Ok(true) => '0',
                    Ok(false) => '1',
                    Err(NoMemory) => 'e',
                };
                n += 1;
                if got != want {
                    bad.push(std::format!(
                        "{flags} {:?} {:?}: want {want}, got {got}",
                        String::from_utf8_lossy(&p),
                        String::from_utf8_lossy(s)
                    ));
                }
            }
        }
        assert_eq!(
            used,
            deviations.len(),
            "every listed deviation is a case of the oracle"
        );
        assert!(n > 2_000_000, "{n} cases");
        assert!(
            bad.is_empty(),
            "{} of {n} cases wrong, the first:\n{}",
            bad.len(),
            bad.iter().take(40).cloned().collect::<Vec<_>>().join("\n")
        );
    }

    /// The three departures, each by the text it follows (the deviation list
    /// is `fnmatch_model.py`'s; these are reasoned, not generated).
    #[test]
    fn where_glibc_departs_this_follows_the_text() {
        // POSIX: "a <backslash> character in pattern followed by any other
        // character shall match that second character in string", and under
        // FNM_PATHNAME "a <slash> ... shall be explicitly matched by a
        // <slash> in pattern" -- which an escaped one is.
        assert!(m("*\\/", "a/", FNM_PATHNAME));
        assert!(m("*\\/", "/", FNM_PATHNAME));
        assert!(!m("*\\/", "a/b", FNM_PATHNAME));
        // An extended group: "the pattern matches if ... any of the patterns
        // in the pattern-list allow matching the input string". `*` takes
        // `b`, `@(a|)` the empty rest.
        assert!(m("*@(a|)", "b", FNM_EXTMATCH));
        assert!(m("*!(a)", "", FNM_EXTMATCH));
        assert!(m("*!(a)", "a", FNM_EXTMATCH));
        // `*`, then a group whose slash is the pattern's own.
        assert!(m("**(a/b)", "a/b", FNM_EXTMATCH | FNM_PATHNAME));
        // FNM_LEADING_DIR: "whether string starts with a directory name that
        // pattern matches" -- `a/b` is one `!(a)` matches, and neither `a`
        // nor `a/b` is one `*(a)b` does.
        assert!(m("!(a)", "a/b", FNM_EXTMATCH | FNM_LEADING_DIR));
        assert!(!m("*(a)b", "a/b", FNM_EXTMATCH | FNM_LEADING_DIR));
        assert!(!m("@(a)b", "a/b", FNM_EXTMATCH | FNM_LEADING_DIR));
    }

    // -- what glibc's leaves open, glibc's way --------------------------------

    #[test]
    fn an_unterminated_bracket_is_an_ordinary_character() {
        assert!(m("[", "[", 0));
        assert!(m("[*", "[x", 0));
        assert!(m("a[b", "a[b", 0));
        assert!(!m("[", "a", 0));
    }

    /// An element no byte can be -- an unknown class, a collating symbol of
    /// more than one byte -- fails the match where it is reached, but not
    /// after an earlier element took the byte.
    #[test]
    fn an_invalid_element_fails_only_when_reached() {
        assert!(!m("[[:foo:]]", "a", 0));
        assert!(!m("[[.hyphen.]]", "-", 0));
        assert!(m("[[[:foo:]]", "[", 0));
        assert!(!m("[[[:foo:]]", "a", 0));
        assert!(!m("[[.]", ".", 0));
    }

    #[test]
    fn equivalence_classes_and_collating_symbols_are_bytes_here() {
        assert!(m("[[=a=]]", "a", 0));
        assert!(!m("[[=a=]]", "b", 0));
        assert!(m("[[.-.]]", "-", 0));
        assert!(m("[a-[.c.]]", "b", 0));
        assert!(!m("[c-a]", "b", 0), "a reversed range is empty");
    }

    /// FNM_CASEFOLD folds bytes and ranges, and not classes, equivalence
    /// classes or collating symbols, which see the byte as it is.
    #[test]
    fn casefold_is_glibcs() {
        assert!(m("a", "A", FNM_CASEFOLD));
        assert!(m("[a-c]", "B", FNM_CASEFOLD));
        assert!(m("[[:upper:]]", "A", FNM_CASEFOLD));
        assert!(!m("[[:lower:]]", "A", FNM_CASEFOLD));
        assert!(!m("[[=a=]]", "A", FNM_CASEFOLD));
    }

    #[test]
    fn leading_dir_ignores_a_slash_and_what_follows() {
        assert!(m("a", "a/b/c", FNM_LEADING_DIR));
        assert!(m("a*", "ab/c", FNM_PATHNAME | FNM_LEADING_DIR));
        assert!(!m("a", "ab/c", FNM_LEADING_DIR));
    }

    #[test]
    fn extended_groups_are_kshs() {
        let e = FNM_EXTMATCH;
        assert!(m("?(a)", "", e) && m("?(a)", "a", e) && !m("?(a)", "aa", e));
        assert!(m("*(a|b)", "abba", e) && m("*(a|b)", "", e) && !m("*(a|b)", "abc", e));
        assert!(m("+(a|b)", "abba", e) && !m("+(a|b)", "", e));
        assert!(m("@(ab|a)", "ab", e) && !m("@(ab|a)", "aab", e));
        assert!(m("!(a)", "b", e) && !m("!(a)", "a", e) && m("!(a)", "", e));
        assert!(m("a!(b)c", "axc", e) && !m("a!(b)c", "abc", e));
        assert!(m("@(@(a)|b)", "b", e));
        // Without the flag they are ordinary characters, and an unclosed
        // group is too.
        assert!(m("@(a)", "@(a)", 0));
        assert!(m("@(a", "@(a", e));
    }

    // -- it does not blow up ----------------------------------------------------

    /// Many stars against a long string that almost matches: every star but
    /// the last settles, so this is linear-ish, not exponential.
    #[test]
    fn many_stars_do_not_backtrack_exponentially() {
        let s = "a".repeat(5000);
        let p = "*a".repeat(40) + "b";
        let t = std::time::Instant::now();
        assert!(!m(&p, &s, 0));
        assert!(!m(&p, &s, FNM_PATHNAME));
        assert!(m(&("*a".repeat(40) + "*"), &s, 0));
        assert!(t.elapsed().as_secs() < 5, "{:?}", t.elapsed());
    }

    /// A repeating group over a long string keeps a table, not a stack of
    /// frames, so a small thread stack is enough.
    #[test]
    fn a_repeating_group_over_a_long_string_needs_no_deep_stack() {
        let s = "ab".repeat(1500);
        let r = std::thread::Builder::new()
            .stack_size(64 * 1024)
            .spawn(move || {
                (
                    m("*(ab)", &s, FNM_EXTMATCH),
                    m("+(a|b)", &s, FNM_EXTMATCH),
                    m("*(ab)c", &s, FNM_EXTMATCH),
                )
            })
            .unwrap()
            .join()
            .unwrap();
        assert_eq!(r, (true, true, false));
    }

    // -- the unit tests this module had before 2026-09-29 --------------

    // -----------------------------------------------------------------------
    // 1. Basic wildcard matching (* and ?)
    // -----------------------------------------------------------------------

    #[test]
    fn star_matches_empty() {
        assert!(matches(b"*\0", b"\0", 0));
    }

    #[test]
    fn star_matches_any_string() {
        assert!(matches(b"*\0", b"hello\0", 0));
    }

    #[test]
    fn star_matches_middle() {
        assert!(matches(b"he*lo\0", b"hello\0", 0));
        assert!(matches(b"he*lo\0", b"hemiddlelo\0", 0));
    }

    #[test]
    fn star_matches_beginning() {
        assert!(matches(b"*ello\0", b"hello\0", 0));
    }

    #[test]
    fn star_matches_end() {
        assert!(matches(b"hell*\0", b"hello\0", 0));
    }

    #[test]
    fn star_no_match() {
        assert!(!matches(b"he*lx\0", b"hello\0", 0));
    }

    #[test]
    fn consecutive_stars() {
        assert!(matches(b"**\0", b"abc\0", 0));
        assert!(matches(b"a***b\0", b"aXYZb\0", 0));
    }

    #[test]
    fn question_mark_single_char() {
        assert!(matches(b"?\0", b"a\0", 0));
        assert!(matches(b"h?llo\0", b"hello\0", 0));
    }

    #[test]
    fn question_mark_no_match_empty() {
        assert!(!matches(b"?\0", b"\0", 0));
    }

    #[test]
    fn question_mark_no_match_multiple() {
        assert!(!matches(b"?\0", b"ab\0", 0));
    }

    #[test]
    fn multiple_question_marks() {
        assert!(matches(b"???\0", b"abc\0", 0));
        assert!(!matches(b"???\0", b"ab\0", 0));
        assert!(!matches(b"???\0", b"abcd\0", 0));
    }

    #[test]
    fn literal_match() {
        assert!(matches(b"hello\0", b"hello\0", 0));
        assert!(!matches(b"hello\0", b"world\0", 0));
    }

    #[test]
    fn literal_empty() {
        assert!(matches(b"\0", b"\0", 0));
        assert!(!matches(b"\0", b"a\0", 0));
        assert!(!matches(b"a\0", b"\0", 0));
    }

    #[test]
    fn star_and_question_combined() {
        assert!(matches(b"*?*\0", b"x\0", 0));
        assert!(!matches(b"*?*\0", b"\0", 0));
        assert!(matches(b"?*?\0", b"ab\0", 0));
        assert!(matches(b"?*?\0", b"abc\0", 0));
        assert!(!matches(b"?*?\0", b"a\0", 0));
    }

    // -----------------------------------------------------------------------
    // 2. Character classes [abc], [a-z], [!abc]
    // -----------------------------------------------------------------------

    #[test]
    fn bracket_single_chars() {
        assert!(matches(b"[abc]\0", b"a\0", 0));
        assert!(matches(b"[abc]\0", b"b\0", 0));
        assert!(matches(b"[abc]\0", b"c\0", 0));
        assert!(!matches(b"[abc]\0", b"d\0", 0));
    }

    #[test]
    fn bracket_range() {
        assert!(matches(b"[a-z]\0", b"m\0", 0));
        assert!(matches(b"[a-z]\0", b"a\0", 0));
        assert!(matches(b"[a-z]\0", b"z\0", 0));
        assert!(!matches(b"[a-z]\0", b"A\0", 0));
        assert!(!matches(b"[a-z]\0", b"0\0", 0));
    }

    #[test]
    fn bracket_range_digits() {
        assert!(matches(b"[0-9]\0", b"5\0", 0));
        assert!(!matches(b"[0-9]\0", b"a\0", 0));
    }

    #[test]
    fn bracket_negation_excl() {
        assert!(!matches(b"[!abc]\0", b"a\0", 0));
        assert!(matches(b"[!abc]\0", b"d\0", 0));
    }

    #[test]
    fn bracket_negation_caret() {
        assert!(!matches(b"[^abc]\0", b"b\0", 0));
        assert!(matches(b"[^abc]\0", b"x\0", 0));
    }

    #[test]
    fn bracket_negated_range() {
        assert!(!matches(b"[!a-z]\0", b"m\0", 0));
        assert!(matches(b"[!a-z]\0", b"5\0", 0));
    }

    #[test]
    fn bracket_literal_close_bracket_first() {
        // ']' as first char in bracket is literal per POSIX.
        assert!(matches(b"[]abc]\0", b"]\0", 0));
        assert!(matches(b"[]abc]\0", b"a\0", 0));
    }

    #[test]
    fn bracket_in_pattern() {
        assert!(matches(b"file[0-9].txt\0", b"file3.txt\0", 0));
        assert!(!matches(b"file[0-9].txt\0", b"filea.txt\0", 0));
    }

    #[test]
    fn bracket_unclosed() {
        // Unclosed bracket: no match.
        assert!(!matches(b"[abc\0", b"a\0", 0));
    }

    #[test]
    fn bracket_mixed_chars_and_ranges() {
        // 'x' or digits 0-9
        assert!(matches(b"[x0-9]\0", b"x\0", 0));
        assert!(matches(b"[x0-9]\0", b"5\0", 0));
        assert!(!matches(b"[x0-9]\0", b"y\0", 0));
    }

    // -----------------------------------------------------------------------
    // 3. POSIX character classes [:alpha:], [:digit:], etc.
    // -----------------------------------------------------------------------

    #[test]
    fn posix_class_alpha() {
        assert!(matches(b"[[:alpha:]]\0", b"a\0", 0));
        assert!(matches(b"[[:alpha:]]\0", b"Z\0", 0));
        assert!(!matches(b"[[:alpha:]]\0", b"5\0", 0));
    }

    #[test]
    fn posix_class_digit() {
        assert!(matches(b"[[:digit:]]\0", b"7\0", 0));
        assert!(!matches(b"[[:digit:]]\0", b"a\0", 0));
    }

    #[test]
    fn posix_class_alnum() {
        assert!(matches(b"[[:alnum:]]\0", b"a\0", 0));
        assert!(matches(b"[[:alnum:]]\0", b"9\0", 0));
        assert!(!matches(b"[[:alnum:]]\0", b"!\0", 0));
    }

    #[test]
    fn posix_class_upper() {
        assert!(matches(b"[[:upper:]]\0", b"A\0", 0));
        assert!(!matches(b"[[:upper:]]\0", b"a\0", 0));
    }

    #[test]
    fn posix_class_lower() {
        assert!(matches(b"[[:lower:]]\0", b"a\0", 0));
        assert!(!matches(b"[[:lower:]]\0", b"A\0", 0));
    }

    #[test]
    fn posix_class_space() {
        assert!(matches(b"[[:space:]]\0", b" \0", 0));
        assert!(matches(b"[[:space:]]\0", b"\t\0", 0));
        assert!(!matches(b"[[:space:]]\0", b"a\0", 0));
    }

    #[test]
    fn posix_class_punct() {
        assert!(matches(b"[[:punct:]]\0", b"!\0", 0));
        assert!(matches(b"[[:punct:]]\0", b".\0", 0));
        assert!(!matches(b"[[:punct:]]\0", b"a\0", 0));
    }

    #[test]
    fn posix_class_xdigit() {
        assert!(matches(b"[[:xdigit:]]\0", b"a\0", 0));
        assert!(matches(b"[[:xdigit:]]\0", b"F\0", 0));
        assert!(matches(b"[[:xdigit:]]\0", b"9\0", 0));
        assert!(!matches(b"[[:xdigit:]]\0", b"g\0", 0));
    }

    #[test]
    fn posix_class_blank() {
        assert!(matches(b"[[:blank:]]\0", b" \0", 0));
        assert!(matches(b"[[:blank:]]\0", b"\t\0", 0));
        assert!(!matches(b"[[:blank:]]\0", b"\n\0", 0));
    }

    #[test]
    fn posix_class_print() {
        assert!(matches(b"[[:print:]]\0", b" \0", 0));
        assert!(matches(b"[[:print:]]\0", b"~\0", 0));
        assert!(!matches(b"[[:print:]]\0", b"\x01\0", 0));
    }

    #[test]
    fn posix_class_graph() {
        assert!(matches(b"[[:graph:]]\0", b"!\0", 0));
        assert!(!matches(b"[[:graph:]]\0", b" \0", 0));
        assert!(!matches(b"[[:graph:]]\0", b"\x01\0", 0));
    }

    #[test]
    fn posix_class_cntrl() {
        assert!(matches(b"[[:cntrl:]]\0", b"\x01\0", 0));
        assert!(matches(b"[[:cntrl:]]\0", b"\x7f\0", 0));
        assert!(!matches(b"[[:cntrl:]]\0", b"a\0", 0));
    }

    #[test]
    fn posix_class_unknown() {
        // Unknown class name should not match.
        assert!(!matches(b"[[:bogus:]]\0", b"a\0", 0));
    }

    #[test]
    fn posix_class_in_context() {
        assert!(matches(
            b"file[[:digit:]][[:digit:]].txt\0",
            b"file42.txt\0",
            0
        ));
        assert!(!matches(
            b"file[[:digit:]][[:digit:]].txt\0",
            b"fileAB.txt\0",
            0
        ));
    }

    #[test]
    fn posix_class_negated() {
        assert!(!matches(b"[![:digit:]]\0", b"5\0", 0));
        assert!(matches(b"[![:digit:]]\0", b"a\0", 0));
    }

    // -----------------------------------------------------------------------
    // 4. FNM_PATHNAME flag (wildcards don't match /)
    // -----------------------------------------------------------------------

    #[test]
    fn pathname_star_does_not_cross_slash() {
        assert!(matches(b"*\0", b"a/b\0", 0)); // Without flag, * matches /
        assert!(!matches(b"*\0", b"a/b\0", FNM_PATHNAME));
    }

    #[test]
    fn pathname_question_does_not_match_slash() {
        assert!(matches(b"a?b\0", b"a/b\0", 0)); // Without flag
        assert!(!matches(b"a?b\0", b"a/b\0", FNM_PATHNAME));
    }

    #[test]
    fn pathname_explicit_slash_matches() {
        assert!(matches(b"a/b\0", b"a/b\0", FNM_PATHNAME));
        assert!(matches(b"*/b\0", b"a/b\0", FNM_PATHNAME));
        assert!(matches(b"a/*\0", b"a/b\0", FNM_PATHNAME));
    }

    #[test]
    fn pathname_star_per_component() {
        assert!(matches(b"*/*\0", b"a/b\0", FNM_PATHNAME));
        assert!(!matches(b"*/*\0", b"a/b/c\0", FNM_PATHNAME));
        assert!(matches(b"*/*/*\0", b"a/b/c\0", FNM_PATHNAME));
    }

    #[test]
    fn pathname_bracket_does_not_match_slash() {
        assert!(!matches(b"a[/]b\0", b"a/b\0", FNM_PATHNAME));
    }

    // -----------------------------------------------------------------------
    // 5. FNM_PERIOD flag (leading . must be explicit)
    // -----------------------------------------------------------------------

    #[test]
    fn period_star_does_not_match_leading_dot() {
        assert!(matches(b"*\0", b".hidden\0", 0)); // Without flag
        assert!(!matches(b"*\0", b".hidden\0", FNM_PERIOD));
    }

    #[test]
    fn period_question_does_not_match_leading_dot() {
        assert!(matches(b"?hidden\0", b".hidden\0", 0));
        assert!(!matches(b"?hidden\0", b".hidden\0", FNM_PERIOD));
    }

    #[test]
    fn period_explicit_dot_matches() {
        assert!(matches(b".hidden\0", b".hidden\0", FNM_PERIOD));
        assert!(matches(b".*\0", b".hidden\0", FNM_PERIOD));
    }

    #[test]
    fn period_not_leading_dot_ok() {
        // Dot that is not leading should still match *.
        assert!(matches(b"*\0", b"file.txt\0", FNM_PERIOD));
    }

    #[test]
    fn period_bracket_does_not_match_leading_dot() {
        assert!(!matches(b"[.]\0", b".\0", FNM_PERIOD));
    }

    #[test]
    fn period_with_pathname_after_slash() {
        // With both FNM_PATHNAME and FNM_PERIOD, a dot at the start of a
        // path component (after /) should require an explicit match.
        let flags = FNM_PATHNAME | FNM_PERIOD;
        assert!(!matches(b"dir/*\0", b"dir/.hidden\0", flags));
        assert!(matches(b"dir/.*\0", b"dir/.hidden\0", flags));
    }

    // -----------------------------------------------------------------------
    // 6. FNM_NOESCAPE flag
    // -----------------------------------------------------------------------

    #[test]
    fn noescape_backslash_literal() {
        // With FNM_NOESCAPE, backslash is treated as ordinary character.
        assert!(matches(b"\\\0", b"\\\0", FNM_NOESCAPE));
        assert!(!matches(b"\\\0", b"a\0", FNM_NOESCAPE));
    }

    #[test]
    fn noescape_backslash_in_bracket() {
        // With FNM_NOESCAPE, backslash in bracket is literal.
        assert!(matches(b"[\\\\a]\0", b"\\\0", FNM_NOESCAPE));
    }

    // -----------------------------------------------------------------------
    // 7. Backslash escaping (when FNM_NOESCAPE is NOT set)
    // -----------------------------------------------------------------------

    #[test]
    fn escape_star() {
        // \* matches literal *
        assert!(matches(b"\\*\0", b"*\0", 0));
        assert!(!matches(b"\\*\0", b"abc\0", 0));
    }

    #[test]
    fn escape_question() {
        assert!(matches(b"\\?\0", b"?\0", 0));
        assert!(!matches(b"\\?\0", b"a\0", 0));
    }

    #[test]
    fn escape_bracket() {
        assert!(matches(b"\\[\0", b"[\0", 0));
    }

    #[test]
    fn escape_backslash() {
        assert!(matches(b"\\\\\0", b"\\\0", 0));
    }

    #[test]
    fn escape_in_bracket_range() {
        // Escaped character as a range endpoint.
        assert!(matches(b"[\\a-\\c]\0", b"b\0", 0));
    }

    #[test]
    fn escape_trailing_backslash_no_match() {
        // Pattern ending with lone backslash (without FNM_NOESCAPE) cannot
        // match because the escaped char is NUL.
        assert!(!matches(b"a\\\0", b"a\0", 0));
    }

    // -----------------------------------------------------------------------
    // 8. Edge cases: empty strings, null pointers, adjacent wildcards
    // -----------------------------------------------------------------------

    #[test]
    fn null_pattern() {
        let result = unsafe { fnmatch(core::ptr::null(), b"test\0".as_ptr(), 0) };
        assert_eq!(result, FNM_NOMATCH);
    }

    #[test]
    fn null_string() {
        let result = unsafe { fnmatch(b"*\0".as_ptr(), core::ptr::null(), 0) };
        assert_eq!(result, FNM_NOMATCH);
    }

    #[test]
    fn both_null() {
        let result = unsafe { fnmatch(core::ptr::null(), core::ptr::null(), 0) };
        assert_eq!(result, FNM_NOMATCH);
    }

    #[test]
    fn empty_pattern_empty_string() {
        assert!(matches(b"\0", b"\0", 0));
    }

    #[test]
    fn empty_pattern_nonempty_string() {
        assert!(!matches(b"\0", b"a\0", 0));
    }

    #[test]
    fn nonempty_pattern_empty_string() {
        assert!(!matches(b"a\0", b"\0", 0));
    }

    #[test]
    fn star_only_empty_string() {
        assert!(matches(b"*\0", b"\0", 0));
    }

    #[test]
    fn question_only_single_char() {
        assert!(matches(b"?\0", b"x\0", 0));
    }

    #[test]
    fn adjacent_stars() {
        assert!(matches(b"***\0", b"anything\0", 0));
        assert!(matches(b"***\0", b"\0", 0));
    }

    #[test]
    fn star_question_star() {
        // At least one character required (due to ?).
        assert!(matches(b"*?*\0", b"a\0", 0));
        assert!(!matches(b"*?*\0", b"\0", 0));
    }

    #[test]
    fn long_string_star_prefix() {
        // Stress: * at beginning with long string.
        assert!(matches(b"*end\0", b"a]very]long]string]with]end\0", 0));
        assert!(!matches(b"*end\0", b"a]very]long]string]with]enD\0", 0));
    }

    #[test]
    fn pattern_longer_than_string() {
        assert!(!matches(b"abcdef\0", b"abc\0", 0));
    }

    #[test]
    fn string_longer_than_pattern() {
        assert!(!matches(b"abc\0", b"abcdef\0", 0));
    }

    // -----------------------------------------------------------------------
    // 9. Complex patterns combining features
    // -----------------------------------------------------------------------

    #[test]
    fn complex_glob_file_matching() {
        assert!(matches(b"*.txt\0", b"readme.txt\0", 0));
        assert!(!matches(b"*.txt\0", b"readme.md\0", 0));
        assert!(matches(b"*.tar.gz\0", b"archive.tar.gz\0", 0));
    }

    #[test]
    fn complex_directory_pattern() {
        assert!(matches(
            b"src/*/test_*.rs\0",
            b"src/module/test_foo.rs\0",
            0,
        ));
    }

    #[test]
    fn complex_bracket_and_star() {
        assert!(matches(b"[a-z]*.log\0", b"server.log\0", 0));
        assert!(!matches(b"[a-z]*.log\0", b"Server.log\0", 0));
        assert!(!matches(b"[a-z]*.log\0", b"1server.log\0", 0));
    }

    #[test]
    fn complex_multiple_brackets() {
        assert!(matches(b"[abc][def][ghi]\0", b"adg\0", 0));
        assert!(matches(b"[abc][def][ghi]\0", b"cfi\0", 0));
        assert!(!matches(b"[abc][def][ghi]\0", b"aaa\0", 0));
    }

    #[test]
    fn complex_escaped_in_pattern() {
        // Match literal "[test].txt"
        assert!(matches(b"\\[test\\].txt\0", b"[test].txt\0", 0));
    }

    #[test]
    fn complex_pathname_period_combined() {
        let flags = FNM_PATHNAME | FNM_PERIOD;
        assert!(matches(b"src/*/*.rs\0", b"src/mod/lib.rs\0", flags));
        assert!(!matches(b"src/*/*.rs\0", b"src/.hidden/lib.rs\0", flags));
        assert!(matches(b"src/.*/*.rs\0", b"src/.hidden/lib.rs\0", flags));
    }

    #[test]
    fn complex_question_in_extension() {
        assert!(matches(b"file.???\0", b"file.txt\0", 0));
        assert!(matches(b"file.???\0", b"file.htm\0", 0));
        assert!(!matches(b"file.???\0", b"file.html\0", 0));
        assert!(!matches(b"file.???\0", b"file.rs\0", 0));
    }

    #[test]
    fn complex_posix_class_with_star() {
        assert!(matches(b"[[:upper:]]*\0", b"Hello\0", 0));
        assert!(!matches(b"[[:upper:]]*\0", b"hello\0", 0));
    }

    #[test]
    fn complex_all_flags() {
        let flags = FNM_PATHNAME | FNM_PERIOD | FNM_NOESCAPE;
        // Backslash is literal (NOESCAPE), star stops at / (PATHNAME),
        // leading dot must be explicit (PERIOD).
        assert!(matches(b"dir/*.c\0", b"dir/main.c\0", flags));
        assert!(!matches(b"dir/*.c\0", b"dir/.hidden.c\0", flags));
        assert!(!matches(b"*\0", b"a/b\0", flags));
        // Backslash is literal in pattern when NOESCAPE.
        assert!(matches(b"a\\b\0", b"a\\b\0", flags));
    }

    #[test]
    fn complex_nested_path_components() {
        let flags = FNM_PATHNAME;
        assert!(matches(b"a/*/c\0", b"a/b/c\0", flags));
        assert!(!matches(b"a/*/c\0", b"a/b/d/c\0", flags));
    }

    #[test]
    fn complex_star_at_path_boundary() {
        let flags = FNM_PATHNAME;
        assert!(matches(b"*/file\0", b"dir/file\0", flags));
        assert!(!matches(b"*/file\0", b"dir/sub/file\0", flags));
    }

    #[test]
    fn return_values() {
        // fnmatch returns 0 on match, FNM_NOMATCH (1) on no-match.
        let result = unsafe { fnmatch(b"abc\0".as_ptr(), b"abc\0".as_ptr(), 0) };
        assert_eq!(result, 0);
        let result = unsafe { fnmatch(b"abc\0".as_ptr(), b"xyz\0".as_ptr(), 0) };
        assert_eq!(result, FNM_NOMATCH);
    }

    // -------------------------------------------------------------------
    // Stress tests — fnmatch additional edge cases
    // -------------------------------------------------------------------

    #[test]
    fn stress_star_empty_string() {
        // "*" should match empty string.
        assert!(matches(b"*\0", b"\0", 0));
    }

    #[test]
    fn stress_question_single_char() {
        assert!(matches(b"?\0", b"a\0", 0));
        assert!(!matches(b"?\0", b"\0", 0)); // ? requires exactly one char
        assert!(!matches(b"?\0", b"ab\0", 0)); // ? matches one, not two
    }

    #[test]
    fn stress_multiple_stars() {
        // "**" should still work like "*".
        assert!(matches(b"**\0", b"hello\0", 0));
        assert!(matches(b"**\0", b"\0", 0));
    }

    #[test]
    fn stress_star_question_combo() {
        // "*?" matches at least one character.
        assert!(matches(b"*?\0", b"x\0", 0));
        assert!(matches(b"*?\0", b"hello\0", 0));
        assert!(!matches(b"*?\0", b"\0", 0)); // needs at least one char
    }

    #[test]
    fn stress_bracket_range_digits() {
        assert!(matches(b"[0-9]\0", b"5\0", 0));
        assert!(!matches(b"[0-9]\0", b"a\0", 0));
        assert!(matches(b"[0-9][0-9]\0", b"42\0", 0));
        assert!(!matches(b"[0-9][0-9]\0", b"4a\0", 0));
    }

    #[test]
    fn stress_bracket_literal_dash() {
        // Dash at start of bracket expr is literal.
        assert!(matches(b"[-abc]\0", b"-\0", 0));
        assert!(matches(b"[-abc]\0", b"a\0", 0));
        assert!(!matches(b"[-abc]\0", b"x\0", 0));
    }

    #[test]
    fn stress_bracket_literal_close_bracket() {
        // ] at start of bracket expr is literal.
        assert!(matches(b"[]abc]\0", b"]\0", 0));
        assert!(matches(b"[]abc]\0", b"a\0", 0));
    }

    #[test]
    fn stress_negated_bracket_caret() {
        // [^...] is synonym for [!...]
        assert!(matches(b"[^abc]\0", b"x\0", 0));
        assert!(!matches(b"[^abc]\0", b"a\0", 0));
    }

    #[test]
    fn stress_pathname_star_no_slash() {
        // With FNM_PATHNAME, * doesn't match /.
        assert!(!matches(b"a*c\0", b"a/c\0", FNM_PATHNAME));
        assert!(matches(b"a*c\0", b"abc\0", FNM_PATHNAME));
    }

    #[test]
    fn stress_pathname_question_no_slash() {
        // With FNM_PATHNAME, ? doesn't match /.
        assert!(!matches(b"a?c\0", b"a/c\0", FNM_PATHNAME));
        assert!(matches(b"a?c\0", b"abc\0", FNM_PATHNAME));
    }

    #[test]
    fn stress_period_leading_dot() {
        // With FNM_PERIOD, leading . must be matched explicitly.
        assert!(!matches(b"*\0", b".hidden\0", FNM_PERIOD));
        assert!(matches(b".*\0", b".hidden\0", FNM_PERIOD));
        assert!(!matches(b"?\0", b".\0", FNM_PERIOD));
        assert!(matches(b".\0", b".\0", FNM_PERIOD));
    }

    #[test]
    fn stress_period_after_slash_pathname() {
        // With FNM_PATHNAME | FNM_PERIOD, dot after / must be explicit.
        let flags = FNM_PATHNAME | FNM_PERIOD;
        assert!(!matches(b"dir/*\0", b"dir/.hidden\0", flags));
        assert!(matches(b"dir/.*\0", b"dir/.hidden\0", flags));
    }

    #[test]
    fn stress_escape_special_chars() {
        // Without NOESCAPE, backslash escapes *, ?, [.
        assert!(matches(b"\\*\0", b"*\0", 0));
        assert!(!matches(b"\\*\0", b"abc\0", 0));
        assert!(matches(b"\\?\0", b"?\0", 0));
        assert!(!matches(b"\\?\0", b"a\0", 0));
    }

    #[test]
    fn stress_noescape_backslash_literal() {
        // With FNM_NOESCAPE, backslash is literal.
        assert!(matches(b"a\\b\0", b"a\\b\0", FNM_NOESCAPE));
        assert!(!matches(b"a\\b\0", b"ab\0", FNM_NOESCAPE));
    }

    #[test]
    fn stress_exact_long_pattern() {
        // Exact match of a long string.
        let pattern = b"abcdefghijklmnopqrstuvwxyz\0";
        let text = b"abcdefghijklmnopqrstuvwxyz\0";
        assert!(matches(pattern, text, 0));
    }

    #[test]
    fn stress_star_in_middle() {
        assert!(matches(b"hello*world\0", b"hello beautiful world\0", 0));
        assert!(matches(b"hello*world\0", b"helloworld\0", 0));
        assert!(!matches(b"hello*world\0", b"hello beautiful place\0", 0));
    }

    #[test]
    fn stress_multiple_bracket_expressions() {
        assert!(matches(b"[abc][def][ghi]\0", b"adg\0", 0));
        assert!(matches(b"[abc][def][ghi]\0", b"beh\0", 0));
        assert!(!matches(b"[abc][def][ghi]\0", b"aaa\0", 0));
    }

    #[test]
    fn stress_posix_class_lower() {
        assert!(matches(b"[[:lower:]]\0", b"a\0", 0));
        assert!(!matches(b"[[:lower:]]\0", b"A\0", 0));
        assert!(!matches(b"[[:lower:]]\0", b"1\0", 0));
    }

    #[test]
    fn stress_posix_class_upper() {
        assert!(matches(b"[[:upper:]]\0", b"A\0", 0));
        assert!(!matches(b"[[:upper:]]\0", b"a\0", 0));
    }

    #[test]
    fn stress_posix_class_digit_in_combo() {
        // Mix POSIX class with range.
        assert!(matches(b"[[:digit:]a-f]\0", b"0\0", 0));
        assert!(matches(b"[[:digit:]a-f]\0", b"a\0", 0));
        assert!(matches(b"[[:digit:]a-f]\0", b"f\0", 0));
        assert!(!matches(b"[[:digit:]a-f]\0", b"g\0", 0));
    }

    #[test]
    fn stress_pattern_literal_only() {
        // Pattern with no wildcards is exact match.
        assert!(matches(b"hello\0", b"hello\0", 0));
        assert!(!matches(b"hello\0", b"Hello\0", 0));
        assert!(!matches(b"hello\0", b"hello world\0", 0));
    }
}
