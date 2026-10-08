//! The orderings a `sort` key can be compared under.
//!
//! Each function takes two key slices — already cut out of their lines by
//! [`crate::keydef`] — and answers how they compare. They work on bytes, never
//! on `str`: `sort` must put a file of arbitrary bytes in order, and a line
//! that is not valid UTF-8 is a line to be sorted, not an error.
//!
//! All of this is `C.UTF-8`'s ordering, GNU's in a UTF-8 locale with no
//! collation tables: bytes compare as bytes -- its code-point collation is
//! exactly that -- and the month names are English. SlateOS is UTF-8
//! throughout (design-decisions §351) and has no collation tables, so there is
//! nothing else it could be; `scripts/sort-diff.sh` runs GNU under
//! `LC_ALL=C.UTF-8`, as the whole harness family does. The one place the UTF-8
//! locale differs from `C` here is what `-R` hashes ([`random`]). A
//! locale-aware collation would change *every* comparison here and is a
//! separate piece of work (`known-issues/TD-SORT-C-LOCALE-ONLY-COLLATION.md`).

use std::borrow::Cow;
use std::cmp::Ordering;

use coreutils::extfloat::{self, ExtF80};

/// Which characters a key ignores before it is compared.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ignore {
    /// `-d`: keep only blanks and alphanumerics.
    NonDictionary,
    /// `-i`: keep only printable characters.
    NonPrinting,
}

/// Whether a byte counts as a blank: upstream's `field_sep`, which is
/// `isblank` -- space and tab, not the whole of `isspace`, so a form feed is
/// not a field separator -- *and the newline*. A line can only hold a newline
/// under `-z`, and there it separates fields, is skipped by `-b`, and is kept
/// by `-d`, as GNU's are.
pub fn is_blank(b: u8) -> bool {
    b == b' ' || b == b'\t' || b == b'\n'
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
        // printable alone -- upstream's table asks `isprint` of each byte, and
        // under `C.UTF-8` as under `C` no byte above 127 is a character.
        Some(Ignore::NonPrinting) => !(0x20..0x7f).contains(&c),
        None => false,
    };
    if dropped {
        return None;
    }
    Some(if fold { c.to_ascii_uppercase() } else { c })
}

/// A key as every ordering but the default one receives it: upstream's
/// `keycompare` copies the key with the characters `-d`/`-i` ignore dropped
/// and the rest translated by `-f`, and hands the copy to `-n`, `-g`, `-h`,
/// `-M`, `-R` and `-V`. Borrowed when there is nothing to drop or translate,
/// which is every key with none of those letters.
pub fn filtered(key: &[u8], ignore: Option<Ignore>, fold: bool) -> Cow<'_, [u8]> {
    if ignore.is_none() && !fold {
        return Cow::Borrowed(key);
    }
    Cow::Owned(
        key.iter()
            .copied()
            .filter_map(|c| keep(c, ignore, fold))
            .collect(),
    )
}

// ── -R, the salted hash ─────────────────────────────────────────────────────

/// `-R`: upstream's `compare_random`, as it runs in a UTF-8 locale -- the
/// only kind SlateOS has (design-decisions §351).
///
/// `salted` is an MD5 state that has already absorbed sixteen random bytes
/// -- from `--random-source`, or the system's -- so each key's digest is
/// `MD5(salt, the key's bytes)`, and the digests are compared as bytes. Equal
/// keys have equal digests, which is what keeps them together; different keys
/// land in an order that a fixed source makes reproducible. Should two
/// different keys share a digest, the hashed bytes themselves decide, as
/// upstream's tiebreaker does (`memcmp` over the shorter, then the lengths --
/// which is a slice's `cmp`).
///
/// Which bytes are hashed is [`hashed`]: not the key as it stands, but what
/// upstream's locale path gives it.
pub fn random(a: &[u8], b: &[u8], salted: &md5::Md5) -> Ordering {
    let (a, b) = (hashed(a), hashed(b));
    let digest = |key: &[u8]| {
        let mut state = salted.clone();
        state.update(key);
        state.finalize()
    };
    digest(&a).cmp(&digest(&b)).then_with(|| a.cmp(&b))
}

/// The bytes `compare_random` hashes for a key in a UTF-8 locale.
///
/// There, upstream's `hard_LC_COLLATE` is true, and it hashes the key one
/// NUL-terminated piece at a time, each piece run through `strxfrm` and
/// hashed *with* its terminating NUL. Under `C.UTF-8` `strxfrm` is the
/// identity -- measured for every byte, valid UTF-8 or not -- so what is
/// hashed is the key with a NUL after each piece: the key and one NUL, or the
/// key alone when it already ends in a NUL (the last piece's terminator is
/// that NUL), or nothing for an empty key (there is no piece). In the `C`
/// locale upstream hashes the bare key and orders differently; measured,
/// `printf 'b\na\nc\n' | sort -R --random-source=<16 zero bytes>` is
/// `a b c` under `LC_ALL=C` and `b c a` under `LC_ALL=C.UTF-8`.
fn hashed(key: &[u8]) -> Cow<'_, [u8]> {
    if key.is_empty() || key.last() == Some(&0) {
        return Cow::Borrowed(key);
    }
    let mut bytes = Vec::with_capacity(key.len().saturating_add(1));
    bytes.extend_from_slice(key);
    bytes.push(0);
    Cow::Owned(bytes)
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

// ── -g, the ordering that goes through a long double ────────────────────────

/// `-g`: upstream's `general_numcompare`. Each key is read by `strtold`, into
/// the x87 80-bit `long double` glibc computes with -- not a `double`, whose 53
/// bits tie `9223372036854775808` with `9223372036854775809` where GNU's 64 do
/// not ([`coreutils::extfloat`]) -- and the readings are ordered:
///
/// 1. a key `strtold` cannot read at all (`abc`, or an empty key) before
///    everything else, all such keys equal;
/// 2. then the NaNs, ordered among themselves by how `%Lf` prints them, which
///    puts `-nan` before `nan` (upstream's `nan_compare`);
/// 3. then the numbers by value, `-0` equal to `0`.
///
/// So a key that is not a number is *not* zero here, as it is to `-n`:
/// measured, GNU puts `abc` before `-1`. And `strtold` skips any leading white
/// space, vertical tab and form feed included, which is wider than `sort`'s
/// blanks.
pub fn general(a: &[u8], b: &[u8]) -> Ordering {
    let (x, y) = (extfloat::strtold(a), extfloat::strtold(b));
    match (x.consumed == 0, y.consumed == 0) {
        (true, true) => return Ordering::Equal,
        (true, false) => return Ordering::Less,
        (false, true) => return Ordering::Greater,
        (false, false) => {}
    }
    let (x, y) = (x.value, y.value);
    if let Some(order) = x.partial_cmp(y) {
        return order;
    }
    match (x.is_nan(), y.is_nan()) {
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        _ => nan_text(x).cmp(nan_text(y)),
    }
}

/// How `%Lf` prints a NaN, which is all `nan_compare` compares: glibc writes
/// the sign and never the payload.
fn nan_text(v: ExtF80) -> &'static str {
    if v.sign_bit() { "-nan" } else { "nan" }
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
    let order: i32 = match key.get(i).copied() {
        Some(b'K' | b'k') => 1,
        Some(b'M') => 2,
        Some(b'G') => 3,
        Some(b'T') => 4,
        Some(b'P') => 5,
        Some(b'E') => 6,
        Some(b'Z') => 7,
        Some(b'Y') => 8,
        // Ronna and quetta, the SI prefixes of 2022, which coreutils 9.4's
        // table carries: measured, GNU sorts `2Z 1Y 1R 1Q` in that order.
        Some(b'R') => 9,
        Some(b'Q') => 10,
        _ => 0,
    };
    if negative {
        order.saturating_neg()
    } else {
        order
    }
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
        // Not a number is not zero: it sorts before every number.
        assert_eq!(general(b"abc", b"0"), Ordering::Less);
        assert_eq!(general(b"-inf", b"0"), Ordering::Less);
    }

    #[test]
    fn general_puts_the_unreadable_first_then_the_nans_then_numbers() {
        // Measured against GNU sort 9.4: `abc`, an empty key and ` x` tie
        // with each other below everything; `-nan` and `nan` follow, in that
        // order; then the numbers.
        let mut keys: Vec<&[u8]> = vec![b"0", b"nan", b"abc", b"-1", b"", b"-nan", b"-inf"];
        keys.sort_by(|a, b| general(a, b));
        let want: Vec<&[u8]> = vec![b"abc", b"", b"-nan", b"nan", b"-inf", b"-1", b"0"];
        assert_eq!(keys, want);
        assert_eq!(general(b"abc", b" x"), Ordering::Equal);
        assert_eq!(general(b"-0", b"0"), Ordering::Equal);
    }

    #[test]
    fn general_has_a_long_doubles_precision() {
        // 2^63 and 2^63 + 1 are one double and two long doubles.
        assert_eq!(
            general(b"9223372036854775808", b"9223372036854775809"),
            Ordering::Less
        );
        assert_eq!(general(b"1", b"1.0000000000000000009"), Ordering::Less);
    }

    #[test]
    fn general_skips_what_strtold_skips() {
        // Vertical tab and form feed are white space to `strtold`, though
        // they are no blanks of `sort`'s.
        assert_eq!(general(b"\x0b5", b"4"), Ordering::Greater);
        assert_eq!(general(b"\x0c3", b"4"), Ordering::Less);
    }

    #[test]
    fn human_knows_ronna_and_quetta() {
        assert_eq!(human(b"1R", b"2Y"), Ordering::Greater);
        assert_eq!(human(b"1Q", b"1R"), Ordering::Greater);
        assert_eq!(human(b"-1Q", b"-1R"), Ordering::Less);
    }

    #[test]
    fn a_newline_is_a_blank() {
        assert!(is_blank(b'\n'));
        assert!(!is_blank(b'\x0c'));
        // So `-n` steps over it, and `-d` keeps it.
        assert_eq!(numeric(b"\n2", b"1"), Ordering::Greater);
        assert_eq!(
            default_order(b"a\nb", b"ab", Some(Ignore::NonDictionary), false),
            Ordering::Less
        );
    }

    /// The MD5 state after sixteen zero bytes: the salt a `--random-source`
    /// file of zeros gives.
    fn zero_salted() -> md5::Md5 {
        let mut state = md5::Md5::new();
        state.update(&[0u8; 16]);
        state
    }

    #[test]
    fn random_orders_keys_as_gnu_does_under_a_utf8_locale() {
        // Measured: GNU sort 9.4, `LC_ALL=C.UTF-8`, a `--random-source` of
        // sixteen zero bytes, puts these seven keys in this order.
        let salted = zero_salted();
        let mut keys: Vec<&[u8]> = vec![b"b", b"a", b"c", b"d", b"A", b"B", b" a"];
        keys.sort_by(|a, b| random(a, b, &salted));
        let want: Vec<&[u8]> = vec![b"b", b"c", b"a", b"B", b" a", b"A", b"d"];
        assert_eq!(keys, want);
    }

    #[test]
    fn random_ties_exactly_the_keys_that_hash_alike() {
        let salted = zero_salted();
        assert_eq!(random(b"x", b"x", &salted), Ordering::Equal);
        assert_ne!(random(b"x", b"y", &salted), Ordering::Equal);
        // A key ending in NUL is hashed as it stands, and any other gains
        // one, so `a` and `a\0` are one key, as upstream's piece-by-piece
        // hash makes them.
        assert_eq!(random(b"a", b"a\0", &salted), Ordering::Equal);
        // The empty key hashes nothing after the salt; `\0` hashes a NUL.
        assert_ne!(random(b"", b"\0", &salted), Ordering::Equal);
    }

    #[test]
    fn hashed_gives_each_piece_its_nul() {
        assert_eq!(&*hashed(b""), b"");
        assert_eq!(&*hashed(b"a"), b"a\0");
        assert_eq!(&*hashed(b"a\0"), b"a\0");
        assert_eq!(&*hashed(b"a\0b"), b"a\0b\0");
        assert_eq!(&*hashed(b"\0"), b"\0");
    }

    #[test]
    fn filtered_copies_only_when_a_letter_asks() {
        assert!(matches!(filtered(b"a-b", None, false), Cow::Borrowed(_)));
        assert_eq!(
            &*filtered(b"a-b", Some(Ignore::NonDictionary), false),
            b"ab"
        );
        assert_eq!(&*filtered(b"1m", None, true), b"1M");
        assert_eq!(
            &*filtered(b"a\x01b", Some(Ignore::NonPrinting), true),
            b"AB"
        );
    }
}
