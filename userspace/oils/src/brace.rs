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
use crate::bytes::{self, Ch, Str};

/// One flattened element of a word for brace scanning.
#[derive(Clone)]
enum Atom {
    /// A brace-significant literal character from an unquoted `Literal` part.
    ///
    /// A [`Ch`], not a `char`: a literal word is bytes, and a byte that is not
    /// part of a valid UTF-8 sequence still has to survive brace expansion
    /// unchanged — it may be part of a filename.
    Ch(Ch),
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
            WordPart::Literal(s) => out.extend(bytes::chars(s).map(Atom::Ch)),
            other => out.push(Atom::Opaque(other.clone())),
        }
    }
    out
}

fn unflatten(atoms: &[Atom]) -> Word {
    let mut parts = Vec::new();
    let mut lit = Str::new();
    for a in atoms {
        match a {
            Atom::Ch(c) => c.push_to(&mut lit),
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
        if let Atom::Ch(c) = a
            && *c == '{'
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
        match c.as_ascii() {
            Some('{') => depth += 1,
            Some('}') => {
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
            Some(',') if depth == 1 => commas.push(j),
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
    // The body of a sequence must be entirely literal characters — and
    // *characters*, not raw bytes: an endpoint has to be a number or a single
    // character to mean anything, so a byte that is not valid UTF-8 makes the
    // body a plain literal rather than a sequence, which is what bash does too
    // (it fails the same `{a..b}` shape check and leaves the braces alone).
    let s: String = body
        .iter()
        .map(|a| match a {
            Atom::Ch(c) => c.as_char(),
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
                .map(|c| vec![Atom::Ch(Ch::U(c))])
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

/// Determine the zero-pad width for a numeric sequence.
///
/// An endpoint asks for padding when it is *written* with a leading zero —
/// `01`, or `-01` where the zero follows the sign — and the width it asks for
/// is the length of the endpoint **as written, sign included**, because bash
/// renders the sequence with `printf`'s `%0*d`, whose field width covers the
/// sign. So `{-01..01}` is width 3 and counts `-01 000 001`, not `-01 00 01`:
/// the `-` occupies one of the three columns for the negative value and a zero
/// takes that column for the positive ones. An endpoint that merely *is*
/// negative asks for nothing — `{-1..01}` takes its width 2 from the `01`
/// alone and counts `-1 00 01`.
///
/// The two endpoints are considered in order and the wider wins, which only
/// matters when both ask: `{01..0001}` is width 4.
fn pad_width(a: &str, b: &str) -> usize {
    // `-0` needs a digit after it to be a padded *number* rather than just a
    // signed zero, which is why the signed form wants one more character.
    let asks = |s: &str| {
        let n = s.len();
        if (n > 1 && s.starts_with('0')) || (n > 2 && s.starts_with("-0")) { n } else { 0 }
    };
    asks(a).max(asks(b))
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

/// Format an integer to a zero-padded field of `width` columns (0 = no
/// padding), sign-aware the way `printf`'s `%0*d` is: the `-` goes in front of
/// the zeros and counts toward the width, so a width of 3 renders `-1` as
/// `-01` and `1` as `001`. Rust's `{:0N}` has exactly those semantics; a
/// fill-and-align spelling such as `{:0>N}` would not (it would pad *before*
/// the sign, giving `0-1`).
fn format_int(n: i64, width: usize) -> String {
    if width == 0 { n.to_string() } else { format!("{n:0width$}") }
}

fn str_to_atoms(s: &str) -> Vec<Atom> {
    s.chars().map(|c| Atom::Ch(Ch::U(c))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expand(src: &str) -> Vec<String> {
        let word = Word::literal(src);
        expand_braces(&word)
            .iter()
            .map(|w| match w.parts.first() {
                Some(WordPart::Literal(s)) if w.parts.len() == 1 => {
                    String::from_utf8(s.clone()).expect("test inputs are ASCII")
                }
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
    fn the_pad_width_counts_the_sign() {
        // bash renders a padded sequence with `%0*d`, whose field width covers
        // the `-`. So a width asked for by `-01` is three *columns*, not three
        // digits: the negative values spend one on the sign and the positive
        // ones fill it with a zero. Every case here is measured against bash
        // 5.2.37.
        assert_eq!(expand("{-01..01}"), vec!["-01", "000", "001"]);
        assert_eq!(expand("{-001..1}"), vec!["-001", "0000", "0001"]);
        assert_eq!(expand("{01..-1}"), vec!["01", "00", "-1"]);
        assert_eq!(expand("{0..-05}"), vec![
            "000", "-01", "-02", "-03", "-04", "-05"
        ]);
        assert_eq!(expand("{-05..05..2}"), vec![
            "-05", "-03", "-01", "001", "003", "005"
        ]);
        assert_eq!(expand("{010..-010..5}"), vec![
            "0010", "0005", "0000", "-005", "-010"
        ]);
        // Being negative is not itself a request for padding: `-1` asks for
        // nothing, so the width comes from `01` alone and `-1` stays two wide.
        assert_eq!(expand("{-1..01}"), vec!["-1", "00", "01"]);
        // `-0` is a signed zero rather than a padded number — it needs a digit
        // after the zero before it asks for a width.
        assert_eq!(expand("{-0..0}"), vec!["0"]);
        assert_eq!(expand("{-0..-0}"), vec!["0"]);
        assert_eq!(expand("{-00..1}"), vec!["000", "001"]);
        // When both endpoints ask, the wider one wins, in either position.
        assert_eq!(expand("{01..0001}"), vec!["0001"]);
        assert_eq!(expand("{0001..01}"), vec!["0001"]);
        assert_eq!(expand("{-0001..-01}"), vec!["-0001"]);
        // An unpadded range is untouched, sign or no sign.
        assert_eq!(expand("{-2..2..2}"), vec!["-2", "0", "2"]);
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
