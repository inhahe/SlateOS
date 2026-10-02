//! awk's value model: a value is a number, a string, or — the thing that makes
//! awk awk — both at once.
//!
//! ## Why this is an enum and not an `f64` or a `Vec<u8>`
//!
//! `$1 == "10"` and `$1 == 10` are allowed to disagree, and in awk they do. A
//! field that looks like a number compares *numerically* against a number and
//! *textually* against a string, so `$1 == 10` is true of a line reading ` 10 `
//! and `$1 == "10"` is false of it. That is POSIX's **strnum** rule, and it is
//! the entire reason a value carries both readings rather than one.
//!
//! Getting it wrong is not a rounding error. `awk '$1 == "007"'` matching a
//! line whose first field is `7` is a wrong answer that looks like a right one.
//!
//! Only values that came from **input** are strnums: fields, `getline`'s
//! result, the elements `split` produces, `ARGV`, `ENVIRON`, and `-v`
//! assignments. A string *literal* in the program never is, because the program
//! said what it meant.
//!
//! ## Why strings are bytes
//!
//! awk is a filter. A line that is not UTF-8 has to come out the way it went
//! in, so a record is `Vec<u8>` and stays one; nothing here decodes. Where a
//! *character* count is required rather than a byte count — `length`, `substr`,
//! `index`, `RSTART`/`RLENGTH` — the character model in `ere::ch` is used, which
//! is the same one the regex engine indexes by, so the two cannot disagree
//! about where the third character starts.

use std::rc::Rc;

/// A byte string. awk's only string type.
pub type Str = Vec<u8>;

/// One awk value.
#[derive(Clone, Debug, Default)]
pub enum Value {
    /// Never assigned. Equal to both `""` and `0`, and false.
    #[default]
    Uninit,
    /// A number, and only a number.
    Num(f64),
    /// A string, and only a string — a program literal, or the result of a
    /// string operation.
    Str(Rc<Str>),
    /// A string *from input* that looks like a number, carrying both readings.
    StrNum(Rc<Str>, f64),
}

impl Value {
    /// A value built from input: a strnum if it looks like a number, else a
    /// plain string. This is the only place a `StrNum` is ever made.
    #[must_use]
    pub fn from_input(bytes: Str) -> Value {
        match numeric_string(&bytes) {
            Some(n) => Value::StrNum(Rc::new(bytes), n),
            None => Value::Str(Rc::new(bytes)),
        }
    }

    /// A program string literal, or any computed string. Never a strnum.
    #[must_use]
    pub fn str(bytes: Str) -> Value {
        Value::Str(Rc::new(bytes))
    }

    /// The value read as a number.
    ///
    /// A plain string converts by the `strtod` rule — as much of a number as
    /// there is at the front, zero if there is none — so `"3abc" + 0` is 3 and
    /// `"abc" + 0` is 0. That is a conversion, not an error; awk has no way to
    /// report one here.
    #[must_use]
    pub fn to_num(&self) -> f64 {
        match self {
            Value::Uninit => 0.0,
            Value::Num(n) | Value::StrNum(_, n) => *n,
            Value::Str(s) => num_prefix(s).map_or(0.0, |(n, _)| n),
        }
    }

    /// The value read as a string, formatting a number with `convfmt`.
    #[must_use]
    pub fn to_str(&self, convfmt: &[u8]) -> Rc<Str> {
        match self {
            Value::Uninit => Rc::new(Str::new()),
            Value::Num(n) => Rc::new(num_to_str(*n, convfmt)),
            Value::Str(s) | Value::StrNum(s, _) => Rc::clone(s),
        }
    }

    /// Whether the value is true in a condition.
    ///
    /// Note the asymmetry that trips people up: an *uninitialised* or *numeric*
    /// value is true when non-zero, but a plain string is true when non-empty —
    /// so the string `"0"` read from the program is **true** while the field
    /// `0` read from input is **false**. That is what strnum is for.
    #[must_use]
    pub fn truthy(&self) -> bool {
        match self {
            Value::Uninit => false,
            Value::Num(n) | Value::StrNum(_, n) => *n != 0.0,
            Value::Str(s) => !s.is_empty(),
        }
    }

    /// Whether this value takes part in a *numeric* comparison.
    #[must_use]
    pub fn numeric(&self) -> bool {
        matches!(self, Value::Uninit | Value::Num(_) | Value::StrNum(_, _))
    }
}

/// Compare two values by POSIX's rule: numerically if both sides are numeric or
/// look numeric, textually otherwise.
///
/// Returns the sign of `a - b` — `Less`, `Equal` or `Greater`. NaN compares
/// unequal to everything including itself, which is why this returns an
/// `Option`; callers turn `None` into false for every relational operator, as C
/// does.
#[must_use]
pub fn compare(a: &Value, b: &Value, convfmt: &[u8]) -> Option<std::cmp::Ordering> {
    if a.numeric() && b.numeric() {
        return a.to_num().partial_cmp(&b.to_num());
    }
    Some(a.to_str(convfmt).cmp(&b.to_str(convfmt)))
}

/// `x1 ^ x2` as gawk computes it (`calc_exp` in eval.c): an integral exponent
/// by repeated squaring, anything else by `pow`.
///
/// Not `f64::powf`, because the two round differently: `1.1 ^ 50` is
/// `117.39085287969571` by gawk's squaring and `...79` by `pow`, and
/// `2 ^ -1074` is 0 to gawk (it takes `1 / 2^1074`, and `2^1074` is already
/// infinite) where `pow` gives the smallest subnormal. awk's `^` is gawk's
/// here, digit for digit.
#[must_use]
pub fn calc_exp(x1: f64, x2: f64) -> f64 {
    let lx = c_long(x2);
    // `(lx = x2) == x2`: an exponent that survives the trip through `long`.
    // Exact on purpose -- the question is whether the value is an integer --
    // and the cast is exact for every `lx` that can answer yes.
    #[allow(clippy::cast_precision_loss, clippy::float_cmp)]
    if lx as f64 == x2 {
        if lx == 0 {
            return 1.0;
        }
        return if lx > 0 {
            calc_exp_posint(x1, lx)
        } else {
            // `-lx` in C, which wraps for LONG_MIN exactly as this does.
            1.0 / calc_exp_posint(x1, lx.wrapping_neg())
        };
    }
    x1.powf(x2)
}

/// `x ^ n` for a positive `n` by squaring, in gawk's order of operations so
/// that it rounds where gawk's does.
fn calc_exp_posint(mut x: f64, mut n: i64) -> f64 {
    let mut mult = 1.0;
    while n > 1 {
        if n % 2 == 1 {
            mult *= x;
        }
        x *= x;
        n /= 2;
    }
    mult * x
}

/// A C `(long) d` as x86-64 performs it: truncation toward zero, and the
/// "integer indefinite" value `LONG_MIN` for NaN and anything out of range,
/// which is what gawk's `NR`, `FNR`, field numbers and `calc_exp` see.
#[must_use]
pub fn c_long(d: f64) -> i64 {
    // 2^63 is exactly representable; anything at or above it, or below
    // -2^63, does not fit.
    const LIMIT: f64 = 9_223_372_036_854_775_808.0;
    if d.is_nan() || d >= LIMIT || d < -LIMIT {
        return i64::MIN;
    }
    // In range by the guard above, so the cast truncates and nothing else.
    #[allow(clippy::cast_possible_truncation)]
    let l = d as i64;
    l
}

/// Render a number as awk renders it: as an integer when it is one, and through
/// `fmt` (CONVFMT or OFMT) when it is not.
///
/// The integer case is not an optimisation — `print 1/1` must say `1`, not
/// `1.000000` and not `1e+00`. The magnitude bound is where an `f64` stops
/// being able to name consecutive integers; past it, `%d` would be printing
/// digits it does not actually have.
#[must_use]
pub fn num_to_str(n: f64, fmt: &[u8]) -> Str {
    if n.is_nan() {
        return if n.is_sign_negative() {
            b"-nan".to_vec()
        } else {
            b"nan".to_vec()
        };
    }
    if n.is_infinite() {
        return if n < 0.0 {
            b"-inf".to_vec()
        } else {
            b"inf".to_vec()
        };
    }
    // Exact on purpose: whether `n` has a fractional part at all.
    #[allow(clippy::float_cmp)]
    let integral = n == n.trunc();
    if integral && n.abs() < 1e18 {
        // The cast is exact: the guard above put `n` inside i64's range and
        // established that it has no fractional part.
        #[allow(clippy::cast_possible_truncation)]
        let i = n as i64;
        return format!("{i}").into_bytes();
    }
    crate::fmt::sprintf_one_number(fmt, n)
}

/// Whether the whole string is a number, and if so which — the strnum test.
///
/// The *whole* string, up to surrounding blanks: `" 12 "` is a number and
/// `"12x"` is not. `inf` and `nan` are deliberately excluded, because a field
/// reading `nan` in a data file is far more likely to be a word than a float,
/// and POSIX mode in gawk excludes them for the same reason.
#[must_use]
pub fn numeric_string(s: &[u8]) -> Option<f64> {
    let t = trim_blanks(s);
    if t.is_empty() {
        return None;
    }
    let (n, used) = num_prefix(t)?;
    if used == t.len() { Some(n) } else { None }
}

/// The longest numeric prefix of `s`, and how many bytes it used — `strtod`
/// without the errno.
#[must_use]
pub fn num_prefix(s: &[u8]) -> Option<(f64, usize)> {
    let mut i = 0usize;
    while matches!(s.get(i), Some(b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)) {
        i = i.saturating_add(1);
    }
    let start = i;
    if matches!(s.get(i), Some(b'+' | b'-')) {
        i = i.saturating_add(1);
    }
    let mut digits = 0usize;
    while matches!(s.get(i), Some(c) if c.is_ascii_digit()) {
        i = i.saturating_add(1);
        digits = digits.saturating_add(1);
    }
    if s.get(i) == Some(&b'.') {
        i = i.saturating_add(1);
        while matches!(s.get(i), Some(c) if c.is_ascii_digit()) {
            i = i.saturating_add(1);
            digits = digits.saturating_add(1);
        }
    }
    if digits == 0 {
        return None;
    }
    // An exponent only counts if it is complete: `1e` is the number 1 followed
    // by the letter e, not a malformed float.
    if matches!(s.get(i), Some(b'e' | b'E')) {
        let mut j = i.saturating_add(1);
        if matches!(s.get(j), Some(b'+' | b'-')) {
            j = j.saturating_add(1);
        }
        if matches!(s.get(j), Some(c) if c.is_ascii_digit()) {
            while matches!(s.get(j), Some(c) if c.is_ascii_digit()) {
                j = j.saturating_add(1);
            }
            i = j;
        }
    }
    let text = s.get(start..i)?;
    // Every byte in `text` is ASCII by construction above.
    let n: f64 = std::str::from_utf8(text).ok()?.parse().ok()?;
    Some((n, i))
}

/// `s` without leading and trailing blanks, for the strnum test.
fn trim_blanks(s: &[u8]) -> &[u8] {
    let mut a = 0usize;
    let mut b = s.len();
    while matches!(s.get(a), Some(b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)) {
        a = a.saturating_add(1);
    }
    while b > a
        && matches!(
            s.get(b.saturating_sub(1)),
            Some(b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
        )
    {
        b = b.saturating_sub(1);
    }
    s.get(a..b).unwrap_or_default()
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    // The values compared are exactly representable; an epsilon would only
    // hide a conversion that came out a bit off.
    clippy::float_cmp,
    // The expected values are written as gawk printed them with `%.17g`,
    // which is the point of them, not the shortest literal for the `f64`.
    clippy::excessive_precision
)]
mod tests {
    use super::*;
    use std::cmp::Ordering;

    fn sv(s: &str) -> Value {
        Value::str(s.as_bytes().to_vec())
    }
    fn iv(s: &str) -> Value {
        Value::from_input(s.as_bytes().to_vec())
    }

    #[test]
    fn a_field_that_looks_numeric_compares_as_a_number() {
        // The rule the whole enum exists for.
        assert_eq!(
            compare(&iv(" 10 "), &Value::Num(10.0), b"%.6g"),
            Some(Ordering::Equal)
        );
        // …but against a string literal it is text, so the blanks count.
        assert_ne!(
            compare(&iv(" 10 "), &sv("10"), b"%.6g"),
            Some(Ordering::Equal)
        );
    }

    #[test]
    fn a_program_literal_is_never_a_strnum() {
        // `"007" == 7` is false: one side is a literal, so this is text.
        assert_ne!(
            compare(&sv("007"), &Value::Num(7.0), b"%.6g"),
            Some(Ordering::Equal)
        );
        // The same characters from input do compare equal.
        assert_eq!(
            compare(&iv("007"), &Value::Num(7.0), b"%.6g"),
            Some(Ordering::Equal)
        );
    }

    #[test]
    fn the_string_zero_is_true_but_the_field_zero_is_false() {
        assert!(sv("0").truthy());
        assert!(!iv("0").truthy());
        assert!(!Value::Uninit.truthy());
        assert!(!sv("").truthy());
        assert!(sv("x").truthy());
    }

    /// gawk's `^`, digit for digit. The expected values are what gawk 5.2.1
    /// printed with `%.17g`, which round-trips an `f64` exactly.
    #[test]
    fn power_rounds_where_gawks_squaring_does() {
        assert_eq!(calc_exp(3.0, 33.0), 5_559_060_566_555_523.0);
        // `pow` gives ...79; squaring gives ...71.
        assert_eq!(calc_exp(1.1, 50.0), 117.390_852_879_695_71);
        assert_eq!(calc_exp(7.0, -21.0), 1.790_363_270_599_549_8e-18);
        // 2^1074 overflows before the reciprocal is taken.
        assert_eq!(calc_exp(2.0, -1074.0), 0.0);
        assert_eq!(calc_exp(2.0, 1024.0), f64::INFINITY);
        assert_eq!(calc_exp(0.0, 0.0), 1.0);
        assert_eq!(calc_exp(-0.0, -1.0), f64::NEG_INFINITY);
        assert_eq!(calc_exp(-2.0, 3.0), -8.0);
        // A fractional exponent is `pow`'s.
        assert_eq!(calc_exp(2.0, 0.5), 2f64.sqrt());
        assert!(calc_exp(-8.0, 1.0 / 3.0).is_nan());
    }

    /// A C `(long)` on x86-64: truncation, and `LONG_MIN` for anything that
    /// does not fit -- which is what gawk prints for `FNR = 1e30; print FNR`.
    #[test]
    fn a_c_long_truncates_and_saturates_to_long_min() {
        assert_eq!(c_long(2.5), 2);
        assert_eq!(c_long(-2.5), -2);
        assert_eq!(c_long(-0.5), 0);
        assert_eq!(c_long(1e30), i64::MIN);
        assert_eq!(c_long(-1e30), i64::MIN);
        assert_eq!(c_long(f64::NAN), i64::MIN);
        assert_eq!(c_long(f64::INFINITY), i64::MIN);
        assert_eq!(c_long(9_223_372_036_854_775_808.0), i64::MIN);
        assert_eq!(c_long(-9_223_372_036_854_775_808.0), i64::MIN);
        assert_eq!(
            c_long(9_223_372_036_854_774_784.0),
            9_223_372_036_854_774_784
        );
    }

    #[test]
    fn a_number_prints_as_an_integer_when_it_is_one() {
        assert_eq!(num_to_str(1.0, b"%.6g"), b"1");
        assert_eq!(num_to_str(-0.0, b"%.6g"), b"0");
        assert_eq!(num_to_str(1e17, b"%.6g"), b"100000000000000000");
        assert_eq!(num_to_str(0.5, b"%.6g"), b"0.5");
        assert_eq!(num_to_str(1.0 / 3.0, b"%.6g"), b"0.333333");
    }

    #[test]
    fn conversion_takes_the_numeric_prefix_and_no_more() {
        assert_eq!(sv("3abc").to_num(), 3.0);
        assert_eq!(sv("abc").to_num(), 0.0);
        assert_eq!(sv("  -2.5e2xyz").to_num(), -250.0);
        // `1e` is 1 followed by a letter, not a broken float.
        assert_eq!(sv("1e").to_num(), 1.0);
        assert_eq!(num_prefix(b"1e"), Some((1.0, 1)));
    }

    #[test]
    fn the_strnum_test_wants_the_whole_string() {
        assert_eq!(numeric_string(b" 12 "), Some(12.0));
        assert_eq!(numeric_string(b"12x"), None);
        assert_eq!(numeric_string(b""), None);
        assert_eq!(numeric_string(b"+.5"), Some(0.5));
        // Not numbers, on purpose: a field reading `nan` is a word.
        assert_eq!(numeric_string(b"nan"), None);
        assert_eq!(numeric_string(b"inf"), None);
    }
}
