//! glibc's `strtod(3)`, for util-linux code that reads a `double` and then
//! looks at where the number ended and at `errno` -- libsmartcols' `width=`
//! column property is the first (`scols_column_set_properties`).
//!
//! What a caller can observe of `strtod` is three things, and each is
//! glibc's here:
//!
//! * **Where it stopped.** Leading blanks, a sign, then the longest prefix
//!   that is a number: decimal (`1.5e3`, `.5`, `5.`), hexadecimal
//!   (`0x1.8p3`), `inf`/`infinity`, `nan`/`nan(chars)`, any case. An
//!   exponent is read only when a digit follows its `e` (or `p`) and sign,
//!   and `0x` only when a hex digit follows it -- `0xg` is the number 0,
//!   ending before the `x`. Nothing read leaves the end at the start.
//! * **The value**, correctly rounded, as glibc's is: decimal through Rust's
//!   own correctly rounded parser, hexadecimal rounded here, half to even.
//! * **`ERANGE`**, which glibc sets for an overflow to infinity, and for an
//!   underflow -- a result that is *tiny* (below the smallest normal
//!   double, judged after rounding to 53 bits) and *inexact*. So `1e-400`
//!   and `4.9e-324` set it and `0x1p-1074`, exactly the smallest subnormal,
//!   does not. Deciding exactness for a decimal takes exact arithmetic,
//!   done here on a small big-integer type.
//!
//! The decimal point is `.`, the C locale's -- and C.UTF-8's, the locales
//! SlateOS has.

use crate::c_isspace;

/// What `strtod(s, &end)` did.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Strtod {
    /// The value; 0 when nothing was read.
    pub value: f64,
    /// `end - s`: how many bytes were read -- 0 when nothing was, as `end`
    /// is then `s` itself.
    pub end: usize,
    /// It set `errno` to `ERANGE`.
    pub erange: bool,
}

/// The byte at `i`, or C's terminating NUL past the end.
fn at(s: &[u8], i: usize) -> u8 {
    s.get(i).copied().unwrap_or(0)
}

fn starts_with_ignore_case(s: &[u8], at_: usize, word: &[u8]) -> bool {
    s.get(at_..)
        .and_then(|rest| rest.get(..word.len()))
        .is_some_and(|head| head.eq_ignore_ascii_case(word))
}

/// `strtod(s, &end)`.
#[must_use]
pub fn strtod(s: &[u8]) -> Strtod {
    let mut i = 0usize;
    while c_isspace(at(s, i)) {
        i = i.saturating_add(1);
    }
    let negative = at(s, i) == b'-';
    if negative || at(s, i) == b'+' {
        i = i.saturating_add(1);
    }
    let signed = |v: f64| if negative { -v } else { v };

    if starts_with_ignore_case(s, i, b"inf") {
        let mut end = i.saturating_add(3);
        if starts_with_ignore_case(s, end, b"inity") {
            end = end.saturating_add(5);
        }
        return Strtod {
            value: signed(f64::INFINITY),
            end,
            erange: false,
        };
    }
    if starts_with_ignore_case(s, i, b"nan") {
        let mut end = i.saturating_add(3);
        if at(s, end) == b'(' {
            let mut k = end.saturating_add(1);
            while at(s, k).is_ascii_alphanumeric() || at(s, k) == b'_' {
                k = k.saturating_add(1);
            }
            if at(s, k) == b')' {
                end = k.saturating_add(1);
            }
        }
        return Strtod {
            value: signed(f64::NAN),
            end,
            erange: false,
        };
    }
    let after_0x = i.saturating_add(2);
    if at(s, i) == b'0'
        && matches!(at(s, i.saturating_add(1)), b'x' | b'X')
        && (at(s, after_0x).is_ascii_hexdigit()
            || (at(s, after_0x) == b'.' && at(s, after_0x.saturating_add(1)).is_ascii_hexdigit()))
    {
        let (value, end, erange) = hexadecimal(s, after_0x);
        return Strtod {
            value: signed(value),
            end,
            erange,
        };
    }
    match decimal(s, i) {
        Some((value, end, erange)) => Strtod {
            value: signed(value),
            end,
            erange,
        },
        None => Strtod {
            value: 0.0,
            end: 0,
            erange: false,
        },
    }
}

/// An exponent's digits after `e`/`p` and its sign, if a digit follows:
/// the value, saturating far past any double's range, and where it ends.
fn exponent(s: &[u8], mark: usize) -> Option<(i64, usize)> {
    let mut j = mark.saturating_add(1);
    let negative = at(s, j) == b'-';
    if negative || at(s, j) == b'+' {
        j = j.saturating_add(1);
    }
    if !at(s, j).is_ascii_digit() {
        return None;
    }
    let mut e: i64 = 0;
    while at(s, j).is_ascii_digit() {
        e = e
            .saturating_mul(10)
            .saturating_add(i64::from(at(s, j).wrapping_sub(b'0')))
            .min(1 << 40);
        j = j.saturating_add(1);
    }
    Some((if negative { e.saturating_neg() } else { e }, j))
}

/// The decimal form: `(|value|, end, erange)`, or `None` when no digit was
/// read.
fn decimal(s: &[u8], start: usize) -> Option<(f64, usize, bool)> {
    let mut i = start;
    // The significant digits, leading zeros dropped; the value is
    // INT(digits) x 10^exp10.
    let mut digits: Vec<u8> = Vec::new();
    let mut exp10: i64 = 0;
    let mut any_digit = false;
    let mut seen_point = false;
    loop {
        let c = at(s, i);
        if c.is_ascii_digit() {
            any_digit = true;
            if !digits.is_empty() || c != b'0' {
                digits.push(c);
            }
            if seen_point {
                exp10 = exp10.saturating_sub(1);
            }
        } else if c == b'.' && !seen_point {
            seen_point = true;
        } else {
            break;
        }
        i = i.saturating_add(1);
    }
    if !any_digit {
        return None;
    }
    if matches!(at(s, i), b'e' | b'E')
        && let Some((e, end)) = exponent(s, i)
    {
        exp10 = exp10.saturating_add(e);
        i = end;
    }
    let Some((&first, rest)) = digits.split_first() else {
        return Some((0.0, i, false));
    };
    // `d.ddd e(exp10 + len - 1)`: an exponent a double's parser reads
    // without saturating, however many digits there are.
    let len = i64::try_from(digits.len()).unwrap_or(i64::MAX);
    let sci_exp = exp10.saturating_add(len.saturating_sub(1));
    let mut text = String::with_capacity(digits.len().saturating_add(24));
    text.push(char::from(first));
    text.push('.');
    text.extend(rest.iter().map(|&d| char::from(d)));
    text.push_str(&format!("e{sci_exp}"));
    let value: f64 = text.parse().unwrap_or(0.0);
    Some((value, i, decimal_erange(value, &digits, exp10)))
}

/// Whether glibc sets `ERANGE` for a decimal that rounded to `value`:
/// overflow, or a tiny inexact result.
fn decimal_erange(value: f64, digits: &[u8], exp10: i64) -> bool {
    let magnitude = value.abs();
    if magnitude.is_infinite() {
        return true;
    }
    if magnitude == 0.0 {
        // Some digit was not zero, or `digits` would be empty.
        return true;
    }
    if magnitude < f64::MIN_POSITIVE {
        // Subnormal: tiny, and an underflow unless exact.
        let k = magnitude.to_bits();
        return compare_decimal_to_binary(digits, exp10, u128::from(k), -1074)
            != std::cmp::Ordering::Equal;
    }
    if magnitude.to_bits() == f64::MIN_POSITIVE.to_bits() {
        // Rounded up to the smallest normal: still tiny if the exact value
        // is below the midpoint between it and the largest 53-bit number
        // under it, (2^54 - 1) x 2^-1076.
        return compare_decimal_to_binary(digits, exp10, (1u128 << 54) - 1, -1076)
            == std::cmp::Ordering::Less;
    }
    false
}

/// The hexadecimal form after its `0x`: `(|value|, end, erange)`.
fn hexadecimal(s: &[u8], start: usize) -> (f64, usize, bool) {
    // The first 15 significant hex digits (60 bits); later ones only move
    // the exponent and, if not zero, make the value inexact.
    let mut mant: u64 = 0;
    let mut significant = 0u32;
    let mut sticky = false;
    let mut exp2: i64 = 0;
    let mut seen_point = false;
    let mut i = start;
    loop {
        let c = at(s, i);
        if let Some(d) = char::from(c).to_digit(16) {
            if mant == 0 && d == 0 {
                if seen_point {
                    exp2 = exp2.saturating_sub(4);
                }
            } else if significant < 15 {
                mant = (mant << 4) | u64::from(d);
                significant = significant.saturating_add(1);
                if seen_point {
                    exp2 = exp2.saturating_sub(4);
                }
            } else {
                sticky |= d != 0;
                if !seen_point {
                    exp2 = exp2.saturating_add(4);
                }
            }
        } else if c == b'.' && !seen_point {
            seen_point = true;
        } else {
            break;
        }
        i = i.saturating_add(1);
    }
    if matches!(at(s, i), b'p' | b'P')
        && let Some((e, end)) = exponent(s, i)
    {
        exp2 = exp2.saturating_add(e);
        i = end;
    }
    let (value, erange) = round_binary(mant, exp2, sticky);
    (value, i, erange)
}

/// `mant x 2^exp2`, plus a nonzero fraction below its last bit when
/// `sticky`, rounded to a double half to even: the value and whether glibc
/// sets `ERANGE` for it.
#[allow(
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "mant < 2^60, so its bit length is at most 60; exp2 saturates near 2^40, so every exponent sum below stays far inside i64; shifts are bounded before they are taken"
)]
fn round_binary(mant: u64, exp2: i64, sticky: bool) -> (f64, bool) {
    if mant == 0 {
        return (0.0, false);
    }
    let len = i64::from(64 - mant.leading_zeros());
    // The exponent of the leading bit.
    let e = len - 1 + exp2;
    if e > 1023 {
        return (f64::INFINITY, true);
    }
    // Rounded to 53 bits with an unbounded exponent, does it reach 2^-1022?
    // That is glibc's (after-rounding) test of whether it is tiny.
    let tiny = e < -1022 && !(e == -1023 && round_carries(mant, len, 53, sticky));

    // The exponent of the result's last bit: 53 bits, or the subnormals'.
    let mut q = (e - 52).max(-1074);
    let shift = q - exp2;
    let (mut kept, inexact): (u128, bool) = if shift <= 0 {
        (u128::from(mant) << ((-shift) as u32), sticky)
    } else if shift > 100 {
        // Everything is below the last bit, and less than half of it.
        (0, true)
    } else {
        let shift = shift as u32;
        let m = u128::from(mant);
        let kept = m >> shift;
        let rest = m & ((1u128 << shift) - 1);
        let half = 1u128 << (shift - 1);
        let up = rest > half || (rest == half && (sticky || kept & 1 == 1));
        (kept + u128::from(up), rest != 0 || sticky)
    };
    if kept == 1u128 << 53 {
        kept >>= 1;
        q += 1;
    }
    if q + 52 > 1023 {
        return (f64::INFINITY, true);
    }
    let erange = tiny && inexact;
    if kept == 0 {
        return (0.0, erange);
    }
    let bits = if kept < 1u128 << 52 {
        // Subnormal: q is -1074, and the bits are the value.
        kept as u64
    } else {
        (((q + 52 + 1023) as u64) << 52) | ((kept - (1u128 << 52)) as u64)
    };
    (f64::from_bits(bits), erange)
}

/// Whether rounding `mant` (`len` bits long) to `keep` bits carries into a
/// new leading bit.
#[allow(
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "len is at most 60 and keep 53, so the shift is below 64"
)]
fn round_carries(mant: u64, len: i64, keep: i64, sticky: bool) -> bool {
    if len <= keep {
        return false;
    }
    let shift = (len - keep) as u32;
    let kept = mant >> shift;
    let rest = mant & ((1u64 << shift) - 1);
    let half = 1u64 << (shift - 1);
    let up = rest > half || (rest == half && (sticky || kept & 1 == 1));
    up && kept == (1u64 << keep) - 1
}

/// `INT(digits) x 10^exp10` against `k x 2^exp2`, exactly.
#[allow(
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the powers are bounded before use: a decimal this is called for is within a few hundred orders of 2^-1074, and its digits bound the rest"
)]
fn compare_decimal_to_binary(digits: &[u8], exp10: i64, k: u128, exp2: i64) -> std::cmp::Ordering {
    // Both sides as integers: multiply each by the powers of 2 and 5 that
    // clear the negative exponents. D x 2^a x 5^b  vs  k x 2^c x 5^d.
    let mut left = Big::from_decimal(digits);
    let mut right = Big::from_u128(k);
    let (mut a, mut b, mut c, mut d) = (0i64, 0i64, 0i64, 0i64);
    if exp10 >= 0 {
        a += exp10;
        b += exp10;
    } else {
        c += -exp10;
        d += -exp10;
    }
    if exp2 >= 0 {
        c += exp2;
    } else {
        a += -exp2;
    }
    // Cancel the common power of two.
    let common = a.min(c);
    a -= common;
    c -= common;
    // A power beyond this means one side dwarfs the other: the value is a
    // hundred thousand orders away from a subnormal, which the caller never
    // asks about.
    const LIMIT: i64 = 100_000;
    if a > LIMIT || b > LIMIT || c > LIMIT || d > LIMIT {
        return (a + b).cmp(&(c + d));
    }
    left.mul_pow5(b as u32);
    left.shl(a as u32);
    right.mul_pow5(d as u32);
    right.shl(c as u32);
    left.cmp(&right)
}

/// A nonnegative integer as little-endian 32-bit limbs, with no zero limb
/// at the top.
#[derive(Debug, PartialEq, Eq)]
struct Big(Vec<u32>);

#[allow(
    clippy::arithmetic_side_effects,
    reason = "limb arithmetic in u64: a u32 limb times a u32 plus a u32 carry is below 2^64, and the shifts are by 1..=31"
)]
impl Big {
    fn trim(&mut self) {
        while self.0.last() == Some(&0) {
            self.0.pop();
        }
    }

    fn from_u128(mut v: u128) -> Big {
        let mut limbs = Vec::new();
        while v != 0 {
            limbs.push((v & 0xffff_ffff) as u32);
            v >>= 32;
        }
        Big(limbs)
    }

    fn mul_small_add(&mut self, m: u32, add: u32) {
        let mut carry = u64::from(add);
        for limb in &mut self.0 {
            let t = u64::from(*limb) * u64::from(m) + carry;
            *limb = (t & 0xffff_ffff) as u32;
            carry = t >> 32;
        }
        if carry != 0 {
            self.0.push(carry as u32);
        }
        self.trim();
    }

    fn from_decimal(digits: &[u8]) -> Big {
        let mut n = Big(Vec::new());
        for &d in digits {
            n.mul_small_add(10, u32::from(d.saturating_sub(b'0')));
        }
        n
    }

    fn mul_pow5(&mut self, mut n: u32) {
        // 5^13 is the largest power of five below 2^32.
        while n >= 13 {
            self.mul_small_add(1_220_703_125, 0);
            n -= 13;
        }
        if n > 0 {
            self.mul_small_add(5u32.pow(n), 0);
        }
    }

    fn shl(&mut self, bits: u32) {
        if self.0.is_empty() {
            return;
        }
        let limbs = (bits / 32) as usize;
        let bits = bits % 32;
        if bits != 0 {
            let mut carry = 0u32;
            for limb in &mut self.0 {
                let next = *limb >> (32 - bits);
                *limb = (*limb << bits) | carry;
                carry = next;
            }
            if carry != 0 {
                self.0.push(carry);
            }
        }
        let mut shifted = vec![0u32; limbs];
        shifted.append(&mut self.0);
        self.0 = shifted;
    }
}

impl PartialOrd for Big {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Big {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0
            .len()
            .cmp(&other.0.len())
            .then_with(|| self.0.iter().rev().cmp(other.0.iter().rev()))
    }
}

#[cfg(test)]
#[allow(
    clippy::float_cmp,
    clippy::unwrap_used,
    clippy::arithmetic_side_effects
)]
mod tests {
    use super::*;

    fn r(s: &str) -> Strtod {
        strtod(s.as_bytes())
    }

    #[test]
    fn a_number_ends_where_glibc_ends_it() {
        assert_eq!(
            r("5,right"),
            Strtod {
                value: 5.0,
                end: 1,
                erange: false
            }
        );
        assert_eq!(r("  -1.5e3x").value, -1500.0);
        assert_eq!(r("  -1.5e3x").end, 8);
        // An exponent needs a digit.
        assert_eq!(r("5e").end, 1);
        assert_eq!(r("5e+").end, 1);
        assert_eq!(r("5.e2").value, 500.0);
        assert_eq!(r(".5").value, 0.5);
        assert_eq!(r("5.").end, 2);
        // Nothing read: the end is the start, even past blanks and a sign.
        assert_eq!(
            r("  -x"),
            Strtod {
                value: 0.0,
                end: 0,
                erange: false
            }
        );
        assert_eq!(
            r("."),
            Strtod {
                value: 0.0,
                end: 0,
                erange: false
            }
        );
        assert_eq!(
            r(""),
            Strtod {
                value: 0.0,
                end: 0,
                erange: false
            }
        );
    }

    #[test]
    fn infinity_and_nan_in_any_case() {
        assert_eq!(r("inf").end, 3);
        assert_eq!(r("INFINITY!").end, 8);
        assert_eq!(r("infin").end, 3);
        assert!(r("-Inf").value.is_infinite() && r("-Inf").value < 0.0);
        assert!(r("nan").value.is_nan());
        assert_eq!(r("nan(abc_1)x").end, 10);
        // A bracket that does not close is not part of it.
        assert_eq!(r("nan(abc").end, 3);
        assert!(!r("inf").erange);
    }

    #[test]
    fn hexadecimal_floats() {
        assert_eq!(r("0x10").value, 16.0);
        assert_eq!(r("0x1.8p1").value, 3.0);
        assert_eq!(r("0x.8").value, 0.5);
        // `0x` with no hex digit after it is the number 0.
        assert_eq!(
            r("0xg"),
            Strtod {
                value: 0.0,
                end: 1,
                erange: false
            }
        );
        assert_eq!(r("0x.g").end, 1);
        assert_eq!(r("0x1p").end, 3);
        // Rounded half to even at the 53rd bit.
        assert_eq!(r("0x1.00000000000008p0").value, 1.0);
        assert_eq!(r("0x1.00000000000018p0").value, 1.0 + 2.0 * f64::EPSILON);
        assert_eq!(r("0x1.000000000000081p0").value, 1.0 + f64::EPSILON);
    }

    #[test]
    fn overflow_and_underflow_set_erange() {
        assert!(r("1e999").erange && r("1e999").value.is_infinite());
        assert!(r("-1e999").value.is_infinite());
        assert!(r("1e-400").erange && r("1e-400").value == 0.0);
        // The smallest subnormal, inexactly: tiny and inexact.
        assert!(r("4.9406564584124654e-324").erange);
        // Exactly the smallest subnormal: no underflow.
        assert!(!r("0x1p-1074").erange);
        assert_eq!(r("0x1p-1074").value, f64::from_bits(1));
        assert!(r("0x1.8p-1074").erange);
        // Normal numbers never, however long.
        assert!(!r("2.2250738585072014e-308").erange);
        assert!(!r("1.7976931348623157e308").erange);
        assert!(!r("0").erange);
        assert!(!r("0e-999").erange);
        assert!(r("0x1p1024").erange);
    }

    #[test]
    fn an_exact_decimal_subnormal_is_not_an_underflow() {
        // 2^-1074 written out in full is exact; one digit off is not.
        let mut exact = Big::from_u128(1);
        exact.mul_pow5(1074);
        let mut digits = String::new();
        let mut n = exact;
        // Decimal digits of 5^1074, most significant first.
        while !n.0.is_empty() {
            let mut rem = 0u64;
            for limb in n.0.iter_mut().rev() {
                let cur = (rem << 32) | u64::from(*limb);
                *limb = (cur / 10) as u32;
                rem = cur % 10;
            }
            n.trim();
            digits.insert(0, char::from(b'0' + rem as u8));
        }
        let text = format!("{digits}e-1074");
        let got = r(&text);
        assert_eq!(got.value, f64::from_bits(1));
        assert!(!got.erange, "2^-1074 exactly");
        let off = format!("{digits}1e-1075");
        assert!(r(&off).erange);
    }
}
