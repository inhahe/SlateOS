//! The orderings a `sort` key can be compared under.
//!
//! Each function takes two key slices — already cut out of their lines by
//! [`crate::keydef`] — and answers how they compare. They work on bytes, never
//! on `str`: `sort` must put a file of arbitrary bytes in order, and a line
//! that is not valid UTF-8 is a line to be sorted, not an error.
//!
//! All of this is C-locale ordering: bytes compare as bytes and the month
//! names are English. SlateOS has no collation tables, so there is nothing
//! else it could be; `scripts/sort-diff.sh` pins `LC_ALL=C` for the same
//! reason. A locale-aware collation would change *every* comparison here and
//! is a separate piece of work (`known-issues.md`).

use std::cmp::Ordering;

/// Which characters a key ignores before it is compared.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ignore {
    /// `-d`: keep only blanks and alphanumerics.
    NonDictionary,
    /// `-i`: keep only printable characters.
    NonPrinting,
}

/// Whether a byte counts as a blank. `sort` means space and tab, not the
/// whole of `isspace` — a form feed is not a field separator.
pub fn is_blank(b: u8) -> bool {
    b == b' ' || b == b'\t'
}

/// The default ordering: bytes, after dropping ignored characters and folding
/// case if asked.
///
/// The fast path matters — this runs O(n log n) times on every input — so a
/// key with no transformations compares the slices directly rather than
/// copying them first.
pub fn default_order(a: &[u8], b: &[u8], ignore: Option<Ignore>, fold: bool) -> Ordering {
    if ignore.is_none() && !fold {
        return a.cmp(b);
    }
    let mut ia = a.iter().copied().filter_map(|c| keep(c, ignore, fold));
    let mut ib = b.iter().copied().filter_map(|c| keep(c, ignore, fold));
    loop {
        match (ia.next(), ib.next()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) => match x.cmp(&y) {
                Ordering::Equal => {}
                other => return other,
            },
        }
    }
}

/// The byte a transformed key contributes, or `None` if it is dropped.
fn keep(c: u8, ignore: Option<Ignore>, fold: bool) -> Option<u8> {
    let dropped = match ignore {
        Some(Ignore::NonDictionary) => !(is_blank(c) || c.is_ascii_alphanumeric()),
        // `-i` keeps the printable ASCII range. A byte above 127 is not
        // printable in the C locale, which is the locale we are in.
        Some(Ignore::NonPrinting) => !(0x20..0x7f).contains(&c),
        None => false,
    };
    if dropped {
        return None;
    }
    Some(if fold { c.to_ascii_uppercase() } else { c })
}

// ── -n, the exact decimal ordering ──────────────────────────────────────────

/// A number as `-n` reads it: a sign and two runs of digits.
///
/// It is deliberately *not* an `f64`. `sort -n` on a column of 20-digit
/// identifiers has to order them exactly, and the moment the value goes
/// through a double it stops being able to: `18446744073709551616` and
/// `18446744073709551617` become the same number. Keeping the digit strings
/// costs nothing — the comparison never needs the value, only the order.
#[derive(Debug, Default)]
struct Number<'a> {
    negative: bool,
    /// Integer digits with leading zeros already dropped.
    integer: &'a [u8],
    /// Fractional digits with trailing zeros already dropped.
    fraction: &'a [u8],
}

impl Number<'_> {
    fn is_zero(&self) -> bool {
        self.integer.is_empty() && self.fraction.is_empty()
    }
}

/// Read the number at the front of a key, GNU's way.
///
/// Three details are not what a general-purpose number parser would do, and
/// all three are observable:
///
/// - A leading `+` is **not** accepted, so `+5` is not five. It has no digits
///   at all as far as `-n` is concerned and therefore compares as zero, which
///   is why GNU sorts `+5` before `12`.
/// - There is no exponent and no `0x`: `1e3` is one and `0x10` is zero.
/// - A key with no digits is zero rather than an error, so `sort -n` on prose
///   leaves it all tied and the last-resort comparison decides.
fn parse_number(key: &[u8]) -> Number<'_> {
    let mut i = 0usize;
    while key.get(i).copied().is_some_and(is_blank) {
        i = i.saturating_add(1);
    }
    let negative = key.get(i) == Some(&b'-');
    if negative {
        i = i.saturating_add(1);
    }

    let int_start = i;
    while key.get(i).copied().is_some_and(|c| c.is_ascii_digit()) {
        i = i.saturating_add(1);
    }
    let integer = key.get(int_start..i).unwrap_or_default();

    let mut fraction: &[u8] = &[];
    if key.get(i) == Some(&b'.') {
        let frac_start = i.saturating_add(1);
        i = frac_start;
        while key.get(i).copied().is_some_and(|c| c.is_ascii_digit()) {
            i = i.saturating_add(1);
        }
        fraction = key.get(frac_start..i).unwrap_or_default();
    }

    Number {
        negative,
        integer: strip_leading_zeros(integer),
        fraction: strip_trailing_zeros(fraction),
    }
}

fn strip_leading_zeros(digits: &[u8]) -> &[u8] {
    let first = digits
        .iter()
        .position(|&c| c != b'0')
        .unwrap_or(digits.len());
    digits.get(first..).unwrap_or_default()
}

fn strip_trailing_zeros(digits: &[u8]) -> &[u8] {
    let end = digits
        .iter()
        .rposition(|&c| c != b'0')
        .map_or(0, |i| i.saturating_add(1));
    digits.get(..end).unwrap_or_default()
}

/// `-n`: compare as decimal numbers of unlimited size.
pub fn numeric(a: &[u8], b: &[u8]) -> Ordering {
    compare_numbers(&parse_number(a), &parse_number(b))
}

fn compare_numbers(x: &Number<'_>, y: &Number<'_>) -> Ordering {
    // Negative zero is zero: `-0` and `0` tie, and only the last-resort
    // comparison separates them.
    let x_neg = x.negative && !x.is_zero();
    let y_neg = y.negative && !y.is_zero();
    match (x_neg, y_neg) {
        (true, false) => return Ordering::Less,
        (false, true) => return Ordering::Greater,
        _ => {}
    }
    let magnitude = compare_magnitude(x, y);
    if x_neg {
        magnitude.reverse()
    } else {
        magnitude
    }
}

fn compare_magnitude(x: &Number<'_>, y: &Number<'_>) -> Ordering {
    // More integer digits is a larger number, the leading zeros having gone.
    match x.integer.len().cmp(&y.integer.len()) {
        Ordering::Equal => {}
        other => return other,
    }
    match x.integer.cmp(y.integer) {
        Ordering::Equal => {}
        other => return other,
    }
    // The fractions are aligned at the point, so they compare left to right
    // and a prefix is the smaller — `.5` against `.55`. Trailing zeros are
    // gone, so `.50` and `.5` are the same slice.
    x.fraction.cmp(y.fraction)
}

// ── -g, the ordering that goes through a double ─────────────────────────────

/// `-g`: compare as C's `strtod` would read them, exponents and all.
///
/// This is the lossy one, and deliberately so — it is what buys `1e3` and
/// `0x10` and `inf`. GNU documents it as slower and less accurate than `-n`;
/// the inaccuracy is the point of keeping the two separate.
pub fn general(a: &[u8], b: &[u8]) -> Ordering {
    let x = strtod(a);
    let y = strtod(b);
    // A NaN sorts below everything, including another NaN, which is the only
    // way an ordering with NaN in it can be a total order at all.
    match (x.is_nan(), y.is_nan()) {
        (true, true) => Ordering::Equal,
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        (false, false) => x.partial_cmp(&y).unwrap_or(Ordering::Equal),
    }
}

/// The longest prefix of `s` that C's `strtod` accepts, as a value.
///
/// A key with no number at the front is zero, matching `-n`.
fn strtod(s: &[u8]) -> f64 {
    let start = s.iter().position(|&c| !is_blank(c)).unwrap_or(s.len());
    let rest = s.get(start..).unwrap_or_default();
    // C's `strtod` reads `0x10` as sixteen and Rust's parser does not, so the
    // hex form is handled before falling through to the decimal one. This is
    // the difference `-g` exists for: `-n` reads `0x10` as zero, and a column
    // of hex needs the other answer.
    if let Some(value) = hex_float(rest) {
        return value;
    }
    // Walk back from the whole slice: the parser is the authority on what its
    // own syntax is, so the longest prefix it accepts is the answer. The keys
    // this runs on are short.
    for end in (1..=rest.len()).rev() {
        let Some(candidate) = rest.get(..end) else {
            continue;
        };
        let Ok(text) = std::str::from_utf8(candidate) else {
            continue;
        };
        // Rust accepts forms C does not: reject the ones that would make
        // `-g` disagree with `strtod` on real input.
        // `infinity` is the case where C is the *more* permissive of the two:
        // `strtod` takes the long spelling, so take it here rather than let the
        // walk-back settle for the `inf` prefix.
        if text.eq_ignore_ascii_case("infinity") {
            if let Ok(v) = text.parse::<f64>() {
                return v;
            }
            continue;
        }
        // A trailing sign, exponent marker or point is where Rust and C part
        // company; drop the character and let the walk-back find what `strtod`
        // would have stopped at.
        if text.ends_with(['+', '-', 'e', 'E', '.']) {
            continue;
        }
        if let Ok(v) = text.parse::<f64>() {
            return v;
        }
    }
    0.0
}

/// C99's hexadecimal float, or `None` if `s` does not start with one.
///
/// `0x1.8p3` is twelve: the digits are base 16, the exponent after `p` is a
/// power of *two*, and it is decimal. A `0x` with no digit after it is not a
/// hex float — C reads it as a plain zero and leaves the `x` — so this returns
/// `None` and the decimal path answers.
fn hex_float(s: &[u8]) -> Option<f64> {
    let mut i = 0usize;
    let sign = match s.first() {
        Some(b'-') => {
            i = 1;
            -1.0f64
        }
        Some(b'+') => {
            i = 1;
            1.0f64
        }
        _ => 1.0f64,
    };
    if s.get(i) != Some(&b'0') {
        return None;
    }
    if !matches!(s.get(i.saturating_add(1)), Some(b'x' | b'X')) {
        return None;
    }
    i = i.saturating_add(2);

    let mut mantissa = 0.0f64;
    let mut any = false;
    while let Some(digit) = s.get(i).and_then(|c| hex_value(*c)) {
        mantissa = mantissa.mul_add(16.0, f64::from(digit));
        any = true;
        i = i.saturating_add(1);
    }
    if s.get(i) == Some(&b'.') {
        i = i.saturating_add(1);
        let mut place = 1.0f64 / 16.0;
        while let Some(digit) = s.get(i).and_then(|c| hex_value(*c)) {
            mantissa += f64::from(digit) * place;
            place /= 16.0;
            any = true;
            i = i.saturating_add(1);
        }
    }
    if !any {
        return None;
    }

    let mut exponent = 0i32;
    if matches!(s.get(i), Some(b'p' | b'P')) {
        let mut j = i.saturating_add(1);
        let negative = s.get(j) == Some(&b'-');
        if negative || s.get(j) == Some(&b'+') {
            j = j.saturating_add(1);
        }
        let mut digits = 0usize;
        let mut value = 0i32;
        while let Some(c) = s.get(j).copied().filter(u8::is_ascii_digit) {
            value = value
                .saturating_mul(10)
                .saturating_add(i32::from(c.wrapping_sub(b'0')));
            digits = digits.saturating_add(1);
            j = j.saturating_add(1);
        }
        // A `p` with no digits after it is not an exponent; the number simply
        // ends before it.
        if digits > 0 {
            exponent = if negative { -value } else { value };
        }
    }
    Some(sign * mantissa * (2.0f64).powi(exponent))
}

fn hex_value(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c.wrapping_sub(b'0')),
        b'a'..=b'f' => Some(c.wrapping_sub(b'a').saturating_add(10)),
        b'A'..=b'F' => Some(c.wrapping_sub(b'A').saturating_add(10)),
        _ => None,
    }
}

// ── -h, the ordering that understands K and M ───────────────────────────────

/// `-h`: compare `2K` below `1M`, the way `du -h` output has to be sorted.
///
/// The suffix outranks the digits: every positive value with an `M` is above
/// every positive value with a `K`, whatever the numbers say, because that is
/// what makes the ordering useful on a column of sizes. Only within one
/// suffix do the digits decide, and there they decide by [`numeric`] — the
/// suffix is not applied as a multiplier at all, so `1024K` is below `1M`
/// rather than equal to it.
///
/// Two details are not what a first reading suggests, and both are GNU's
/// `find_unit_order`:
///
/// - **A zero has no order.** `0K` and `0M` are both plain zero, so they tie
///   with each other and sort *below* `900` rather than above it. A suffix
///   scaling nothing is nothing.
/// - **A negative number's order is negative**, so `-1M` is below `-1K`: the
///   sign is applied to the magnitude before the suffix is consulted, which is
///   what puts the largest negative first.
pub fn human(a: &[u8], b: &[u8]) -> Ordering {
    match unit_order(a).cmp(&unit_order(b)) {
        Ordering::Equal => numeric(a, b),
        other => other,
    }
}

/// The signed power of 1024 a key's suffix names.
///
/// Zero when the number is zero, when there is no suffix, or when what
/// precedes the suffix is not a number at all — `+5K` counts as none of them,
/// because `-h`, like `-n`, does not accept a leading `+`.
fn unit_order(key: &[u8]) -> i32 {
    let mut i = key.iter().position(|&c| !is_blank(c)).unwrap_or(key.len());
    let negative = key.get(i) == Some(&b'-');
    if negative {
        i = i.saturating_add(1);
    }
    let mut nonzero = false;
    let mut digits = |i: &mut usize| {
        while let Some(c) = key.get(*i).copied().filter(u8::is_ascii_digit) {
            nonzero |= c != b'0';
            *i = i.saturating_add(1);
        }
    };
    digits(&mut i);
    if key.get(i) == Some(&b'.') {
        i = i.saturating_add(1);
        digits(&mut i);
    }
    // `0K` is zero, not "a thousand of nothing".
    if !nonzero {
        return 0;
    }
    let order = match key.get(i).copied() {
        Some(b'K' | b'k') => 1,
        Some(b'M') => 2,
        Some(b'G') => 3,
        Some(b'T') => 4,
        Some(b'P') => 5,
        Some(b'E') => 6,
        Some(b'Z') => 7,
        Some(b'Y') => 8,
        _ => 0,
    };
    if negative { -order } else { order }
}

// ── -M, the ordering that knows the calendar ────────────────────────────────

/// `-M`: compare by month name, `JAN` below `DEC`, unknown below all of them.
///
/// The comparison is on the first three letters, case-insensitively, after
/// leading blanks — so `january`, `Jan` and `JANUARY-2026` are all the first
/// month. Anything else is month zero and ties with every other non-month.
pub fn month(a: &[u8], b: &[u8]) -> Ordering {
    month_number(a).cmp(&month_number(b))
}

fn month_number(key: &[u8]) -> u8 {
    const NAMES: [&[u8; 3]; 12] = [
        b"JAN", b"FEB", b"MAR", b"APR", b"MAY", b"JUN", b"JUL", b"AUG", b"SEP", b"OCT", b"NOV",
        b"DEC",
    ];
    let start = key.iter().position(|&c| !is_blank(c)).unwrap_or(key.len());
    let Some(head) = key.get(start..start.saturating_add(3)) else {
        return 0;
    };
    for (index, name) in NAMES.iter().enumerate() {
        if head.eq_ignore_ascii_case(name.as_slice()) {
            return u8::try_from(index.saturating_add(1)).unwrap_or(0);
        }
    }
    0
}

// ── -V, the ordering that reads a version number ────────────────────────────

/// `-V`: order the way a human reads a release list — `1.9` below `1.10`.
///
/// One copy, shared with `ls -v`, in [`coreutils::vercmp`].
/// The two utilities are reached for in the same breath — `ls -v` to look at a
/// directory of releases and `sort -V` to feed the same names onward — so a
/// disagreement between them shows up as one directory ordered two ways in one
/// terminal. It is re-exported rather than re-implemented for exactly that
/// reason; the rules, and why a `filevercmp` is a *file name* comparison, are
/// documented there.
pub use coreutils::vercmp::version;

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    fn n(a: &str, b: &str) -> Ordering {
        numeric(a.as_bytes(), b.as_bytes())
    }

    #[test]
    fn numeric_orders_by_value_not_by_text() {
        assert_eq!(n("2", "10"), Ordering::Less);
        assert_eq!(n("-3", "2"), Ordering::Less);
        assert_eq!(n("  12", "3"), Ordering::Greater);
    }

    #[test]
    fn numeric_is_exact_beyond_what_a_double_can_hold() {
        // Both of these are the same `f64`. An `f64` implementation ties them
        // and lets the last-resort comparison decide, which is a different
        // answer whenever the shorter string sorts higher.
        assert_eq!(
            n("18446744073709551617", "18446744073709551616"),
            Ordering::Greater
        );
        assert_eq!(
            n("100000000000000000000.5", "100000000000000000000.25"),
            Ordering::Greater
        );
    }

    #[test]
    fn numeric_ignores_what_gnu_ignores() {
        // A leading `+` is not part of the number, so `+5` is zero.
        assert_eq!(n("+5", "1"), Ordering::Less);
        // No exponent and no hex: `1e3` is one, `0x10` is zero.
        assert_eq!(n("1e3", "2"), Ordering::Less);
        assert_eq!(n("0x10", "1"), Ordering::Less);
        // Prose is zero rather than an error.
        assert_eq!(n("abc", "0"), Ordering::Equal);
    }

    #[test]
    fn numeric_ties_the_spellings_of_one_value() {
        assert_eq!(n("1.10", "1.1"), Ordering::Equal);
        assert_eq!(n("01", "1"), Ordering::Equal);
        assert_eq!(n("-0", "0"), Ordering::Equal);
        assert_eq!(n(".5", "0.50"), Ordering::Equal);
    }

    #[test]
    fn human_puts_the_suffix_above_the_digits() {
        assert_eq!(human(b"2K", b"1M"), Ordering::Less);
        assert_eq!(human(b"1024K", b"1M"), Ordering::Less);
        assert_eq!(human(b"900", b"1K"), Ordering::Less);
        assert_eq!(human(b"3M", b"2M"), Ordering::Greater);
        assert_eq!(human(b"-1M", b"1K"), Ordering::Less);
    }

    #[test]
    fn human_gives_zero_no_order_at_all() {
        // A suffix scaling nothing is nothing, so `0K` is plain zero and sorts
        // below `900` rather than above it. Measured against GNU sort 8.32.
        assert_eq!(human(b"0K", b"900"), Ordering::Less);
        assert_eq!(human(b"0K", b"0M"), Ordering::Equal);
        assert_eq!(human(b"0.0M", b"0"), Ordering::Equal);
        // The sign reaches the order, so the largest negative comes first.
        assert_eq!(human(b"-1M", b"-1K"), Ordering::Less);
        assert_eq!(human(b"-2K", b"-1K"), Ordering::Less);
        // `+5K` is not a number to `-h` any more than it is to `-n`.
        assert_eq!(human(b"+5K", b"1"), Ordering::Less);
    }

    #[test]
    fn general_reads_hex_the_way_strtod_does() {
        assert_eq!(general(b"0x10", b"17"), Ordering::Less);
        assert_eq!(general(b"0x10", b"15"), Ordering::Greater);
        assert_eq!(general(b"0x1.8p3", b"12"), Ordering::Equal);
        assert_eq!(general(b"-0x10", b"0"), Ordering::Less);
        // `0x` with no digit is a plain zero with an `x` after it.
        assert_eq!(general(b"0x", b"0"), Ordering::Equal);
    }

    #[test]
    fn month_reads_three_letters_case_blind() {
        assert_eq!(month(b"JAN", b"FEB"), Ordering::Less);
        assert_eq!(month(b"december", b"Jan"), Ordering::Greater);
        assert_eq!(month(b"  MARCH 3", b"apr"), Ordering::Less);
        // Not a month at all, so month zero, below every real one.
        assert_eq!(month(b"xyz", b"JAN"), Ordering::Less);
        assert_eq!(month(b"xyz", b"nope"), Ordering::Equal);
    }

    #[test]
    fn default_order_can_fold_and_ignore() {
        assert_eq!(
            default_order(b"abc", b"ABC", None, false),
            Ordering::Greater
        );
        assert_eq!(default_order(b"abc", b"ABC", None, true), Ordering::Equal);
        // `-d` keeps blanks and alphanumerics only, so the punctuation goes.
        assert_eq!(
            default_order(b"a-b", b"ab", Some(Ignore::NonDictionary), false),
            Ordering::Equal
        );
        // `-i` drops the control character rather than comparing it.
        assert_eq!(
            default_order(b"a\x01b", b"ab", Some(Ignore::NonPrinting), false),
            Ordering::Equal
        );
    }

    #[test]
    fn general_understands_what_numeric_refuses() {
        assert_eq!(general(b"1e3", b"999"), Ordering::Greater);
        assert_eq!(general(b"+5", b"1"), Ordering::Greater);
        assert_eq!(general(b"abc", b"0"), Ordering::Equal);
        assert_eq!(general(b"-inf", b"0"), Ordering::Less);
    }
}
