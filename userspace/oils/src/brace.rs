//! Brace expansion (`{a,b,c}`, `{1..5}`, `{a..z}`, `{01..10}`, `{1..9..2}`).
//!
//! Brace expansion is the *first* expansion bash performs — purely textual and
//! before parameter/arithmetic/command/tilde expansion. It respects quoting:
//! braces and commas inside single/double quotes (or other expansions like
//! `$var` / `$(…)` / `${…}`) are literal and never introduce brace syntax.
//!
//! This module operates on the already-parsed [`Word`] structure. Each
//! [`WordPart::Literal`]'s characters are brace-significant; every other part
//! (quotes, params, substitutions) is treated as an opaque unit that may sit
//! inside a brace alternative but can never itself contribute a `{`, `,`, or
//! `}`. The result is the ordered list of words the one input word expands to
//! (a single unchanged word when there is no valid brace pattern).

use crate::ast::{Word, WordPart};

/// One flattened element of a word for brace scanning.
#[derive(Clone)]
enum Atom {
    /// A brace-significant literal character from an unquoted `Literal` part.
    Ch(char),
    /// An opaque, non-literal part (quotes/params/subs); never brace syntax.
    Opaque(WordPart),
}

/// A matched, *valid* brace expression within a flattened word.
struct BraceMatch {
    /// Index of the opening `{`.
    open: usize,
    /// Index of the matching `}`.
    close: usize,
    /// Absolute indices of the top-level commas. Empty means the body is a
    /// `x..y[..incr]` sequence rather than a comma list.
    commas: Vec<usize>,
}

/// Expand brace patterns in `word`, returning one or more words in order.
/// A word with no valid brace pattern comes back unchanged as a single element.
#[must_use]
pub fn expand_braces(word: &Word) -> Vec<Word> {
    let atoms = flatten(word);
    expand_atoms(&atoms).iter().map(|a| unflatten(a)).collect()
}

fn flatten(word: &Word) -> Vec<Atom> {
    let mut out = Vec::new();
    for part in &word.parts {
        match part {
            WordPart::Literal(s) => out.extend(s.chars().map(Atom::Ch)),
            other => out.push(Atom::Opaque(other.clone())),
        }
    }
    out
}

fn unflatten(atoms: &[Atom]) -> Word {
    let mut parts = Vec::new();
    let mut lit = String::new();
    for a in atoms {
        match a {
            Atom::Ch(c) => lit.push(*c),
            Atom::Opaque(p) => {
                if !lit.is_empty() {
                    parts.push(WordPart::Literal(std::mem::take(&mut lit)));
                }
                parts.push(p.clone());
            }
        }
    }
    if !lit.is_empty() {
        parts.push(WordPart::Literal(lit));
    }
    Word { parts }
}

/// Recursively expand the first valid brace expression in `atoms`.
fn expand_atoms(atoms: &[Atom]) -> Vec<Vec<Atom>> {
    let Some(m) = find_brace(atoms) else {
        return vec![atoms.to_vec()];
    };
    let pre = &atoms[..m.open];
    let post = &atoms[m.close + 1..];

    let alternatives = if m.commas.is_empty() {
        // Body is a sequence (validated by `find_brace`).
        match sequence_of(&atoms[m.open + 1..m.close]) {
            Some(seq) => seq,
            // Should not happen (find_brace validated it), but stay safe.
            None => return vec![atoms.to_vec()],
        }
    } else {
        split_commas(atoms, m.open, m.close, &m.commas)
    };

    let mut results = Vec::new();
    for alt in alternatives {
        let mut combined = Vec::with_capacity(pre.len() + alt.len() + post.len());
        combined.extend_from_slice(pre);
        combined.extend(alt);
        combined.extend_from_slice(post);
        results.extend(expand_atoms(&combined));
    }
    results
}

/// Find the first `{` that begins a *valid* brace expansion (a top-level comma
/// list or a `x..y[..incr]` sequence). Invalid braces (`{}`, `{abc}`) are
/// skipped so a later valid brace in the same word is still found.
fn find_brace(atoms: &[Atom]) -> Option<BraceMatch> {
    for (i, a) in atoms.iter().enumerate() {
        if let Atom::Ch('{') = a
            && let Some(m) = match_brace(atoms, i)
        {
            return Some(m);
        }
    }
    None
}

/// Attempt to match a brace expression starting at `open`. Returns `None` if
/// there is no balanced `}` or the body is neither a comma list nor a sequence.
fn match_brace(atoms: &[Atom], open: usize) -> Option<BraceMatch> {
    let mut depth = 0usize;
    let mut commas = Vec::new();
    for (j, a) in atoms.iter().enumerate().skip(open) {
        let Atom::Ch(c) = a else { continue };
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    if !commas.is_empty() {
                        return Some(BraceMatch { open, close: j, commas });
                    }
                    if sequence_of(&atoms[open + 1..j]).is_some() {
                        return Some(BraceMatch { open, close: j, commas: Vec::new() });
                    }
                    return None;
                }
            }
            ',' if depth == 1 => commas.push(j),
            _ => {}
        }
    }
    None
}

/// Split a comma brace body into its alternatives (each a slice of atoms).
fn split_commas(atoms: &[Atom], open: usize, close: usize, commas: &[usize]) -> Vec<Vec<Atom>> {
    let mut alts = Vec::with_capacity(commas.len() + 1);
    let mut start = open + 1;
    for &c in commas {
        alts.push(atoms[start..c].to_vec());
        start = c + 1;
    }
    alts.push(atoms[start..close].to_vec());
    alts
}

/// If `body` is a `x..y[..incr]` sequence (all literal chars), expand it into
/// its ordered elements. Supports signed integers (with optional zero-padding)
/// and single-character ranges.
fn sequence_of(body: &[Atom]) -> Option<Vec<Vec<Atom>>> {
    // The body of a sequence must be entirely literal characters.
    let s: String = body
        .iter()
        .map(|a| match a {
            Atom::Ch(c) => Some(*c),
            Atom::Opaque(_) => None,
        })
        .collect::<Option<String>>()?;
    let segs: Vec<&str> = s.split("..").collect();
    if segs.len() != 2 && segs.len() != 3 {
        return None;
    }
    let incr_str = segs.get(2).copied();

    // Numeric sequence.
    if let (Some(start), Some(end)) = (parse_int(segs[0]), parse_int(segs[1])) {
        let incr = match incr_str {
            Some(x) => parse_int(x)?,
            None => 1,
        };
        let step = if incr == 0 { 1 } else { incr.unsigned_abs() };
        let pad = pad_width(segs[0], segs[1]);
        let nums = int_range(start, end, i64::try_from(step).unwrap_or(i64::MAX).max(1));
        return Some(nums.into_iter().map(|n| str_to_atoms(&format_int(n, pad))).collect());
    }

    // Single-character sequence (`{a..e}`, `{Z..A}`).
    let sc: Vec<char> = segs[0].chars().collect();
    let ec: Vec<char> = segs[1].chars().collect();
    if sc.len() == 1 && ec.len() == 1 {
        let incr = match incr_str {
            Some(x) => parse_int(x)?,
            None => 1,
        };
        let step = if incr == 0 {
            1
        } else {
            u32::try_from(incr.unsigned_abs()).unwrap_or(u32::MAX)
        };
        let (s0, e0) = (u32::from(sc[0]), u32::from(ec[0]));
        let range = char_range(s0, e0, step);
        // Each generated code point becomes a literal element as-is. Note this
        // includes U+005C `\`: a range spanning it (e.g. `{A..z}`, `{Y..a}`)
        // yields a literal `\` element, whereas bash yields an *empty* element
        // there — a side effect of bash re-applying quote removal to brace-range
        // output (a lone `\` is then eaten). osh deliberately treats brace-range
        // characters as final literal data and does not re-lex them, which is
        // both simpler and safer (bash's re-scan also turns a generated backtick
        // into command-substitution). Documented as TD-OILS-BRACE-BACKSLASH.
        return Some(
            range
                .into_iter()
                .filter_map(char::from_u32)
                .map(|c| vec![Atom::Ch(c)])
                .collect(),
        );
    }
    None
}

/// Parse a possibly-signed decimal integer, rejecting anything with extra
/// characters (so `1a` is not a valid sequence endpoint).
fn parse_int(s: &str) -> Option<i64> {
    s.parse::<i64>().ok()
}

/// Determine the zero-pad width for a numeric sequence: if either endpoint is
/// written with a leading zero (e.g. `01`, `-05`), pad every value to the width
/// of the widest endpoint's digit count.
fn pad_width(a: &str, b: &str) -> usize {
    let has_pad = |s: &str| {
        let digits = s.strip_prefix('-').unwrap_or(s);
        digits.len() > 1 && digits.starts_with('0')
    };
    if has_pad(a) || has_pad(b) {
        let digits = |s: &str| s.strip_prefix('-').unwrap_or(s).len();
        digits(a).max(digits(b))
    } else {
        0
    }
}

/// Build an inclusive integer range from `start` toward `end` stepping by
/// `step` (positive magnitude), capped to a sane element count.
fn int_range(start: i64, end: i64, step: i64) -> Vec<i64> {
    let step = step.max(1);
    let mut out = Vec::new();
    let mut v = start;
    loop {
        out.push(v);
        if v == end || out.len() > 100_000 {
            break;
        }
        if start <= end {
            if end - v < step {
                break;
            }
            v += step;
        } else {
            if v - end < step {
                break;
            }
            v -= step;
        }
    }
    out
}

/// Build an inclusive `u32` code-point range for a character sequence.
fn char_range(start: u32, end: u32, step: u32) -> Vec<u32> {
    let step = step.max(1);
    let mut out = Vec::new();
    let mut v = start;
    loop {
        out.push(v);
        if v == end || out.len() > 100_000 {
            break;
        }
        if start <= end {
            if end - v < step {
                break;
            }
            v += step;
        } else {
            if v - end < step {
                break;
            }
            v -= step;
        }
    }
    out
}

/// Format an integer, zero-padded to `width` digits (0 = no padding), keeping a
/// leading `-` outside the padding.
fn format_int(n: i64, width: usize) -> String {
    if width == 0 {
        return n.to_string();
    }
    if n < 0 {
        format!("-{:0>width$}", n.unsigned_abs(), width = width)
    } else {
        format!("{n:0>width$}")
    }
}

fn str_to_atoms(s: &str) -> Vec<Atom> {
    s.chars().map(Atom::Ch).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expand(src: &str) -> Vec<String> {
        let word = Word::literal(src);
        expand_braces(&word)
            .iter()
            .map(|w| match w.parts.first() {
                Some(WordPart::Literal(s)) if w.parts.len() == 1 => s.clone(),
                None => String::new(),
                _ => String::from("<parts>"),
            })
            .collect()
    }

    #[test]
    fn comma_list() {
        assert_eq!(expand("a{b,c,d}e"), vec!["abe", "ace", "ade"]);
    }

    #[test]
    fn empty_alternative() {
        assert_eq!(expand("{,x}"), vec!["", "x"]);
    }

    #[test]
    fn numeric_sequence() {
        assert_eq!(expand("{1..4}"), vec!["1", "2", "3", "4"]);
        assert_eq!(expand("{4..1}"), vec!["4", "3", "2", "1"]);
        assert_eq!(expand("{1..9..2}"), vec!["1", "3", "5", "7", "9"]);
    }

    #[test]
    fn padded_sequence() {
        assert_eq!(expand("{01..03}"), vec!["01", "02", "03"]);
        assert_eq!(expand("{08..10}"), vec!["08", "09", "10"]);
    }

    #[test]
    fn char_sequence() {
        assert_eq!(expand("{a..e}"), vec!["a", "b", "c", "d", "e"]);
        assert_eq!(expand("{c..a}"), vec!["c", "b", "a"]);
    }

    #[test]
    fn char_sequence_spanning_backslash_keeps_literal() {
        // A range crossing U+005C `\` emits a literal `\` element (osh treats
        // brace-range output as final literal data). bash instead yields an
        // empty element there via quote removal — a documented, intentional
        // divergence (TD-OILS-BRACE-BACKSLASH). `[`(91) `\`(92) `]`(93).
        assert_eq!(expand("{[..]}"), vec!["[", "\\", "]"]);
        // Element count still matches bash (9 for Y..a), only the `\` cell differs.
        assert_eq!(
            expand("{Y..a}"),
            vec!["Y", "Z", "[", "\\", "]", "^", "_", "`", "a"]
        );
    }

    #[test]
    fn nested_and_cross_product() {
        assert_eq!(expand("{a,b}{1,2}"), vec!["a1", "a2", "b1", "b2"]);
        assert_eq!(expand("{a,{b,c}}"), vec!["a", "b", "c"]);
    }

    #[test]
    fn invalid_stays_literal() {
        assert_eq!(expand("{abc}"), vec!["{abc}"]);
        assert_eq!(expand("{}"), vec!["{}"]);
        assert_eq!(expand("nobrace"), vec!["nobrace"]);
    }
}
