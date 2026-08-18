//! C's `printf` conversions for everything that is not a real number.
//!
//! [`extfloat`](crate::extfloat) answers `%a %e %f %g` and their capitals,
//! because those are the ones where the hard part is the arithmetic. This
//! module answers the rest — `%d %i %o %u %x %X %c %s` — where the hard part is
//! instead a pile of small rules that interact, each of which is obvious on its
//! own and none of which anybody remembers correctly:
//!
//! - a precision on an integer is a *minimum digit count*, not a maximum, and
//!   is a maximum on a string;
//! - a precision of zero applied to the value zero prints **nothing at all**,
//!   which is how `%.0d` differs from `%d`;
//! - `#` on `%o` does not prepend `0`, it raises the precision until the first
//!   digit *is* a zero, which is why `%#.0o` of `0` prints `0` while `%.0o`
//!   prints nothing;
//! - `#` on `%x` prepends `0x` — unless the value is zero, when it does not;
//! - `+` and a space are sign flags, so they apply to `%d` and `%i` and are
//!   silently ignored by `%u %o %x %X`, which have no sign to write;
//! - the `0` flag is ignored when `-` is also given, and ignored again when a
//!   precision is given, so `%05.2d` pads with *spaces*;
//! - the zeros the `0` flag pads with go after the sign and after `0x`, not
//!   before them.
//!
//! Every one of those is measured against glibc rather than recalled; see
//! `scripts/printf-diff.sh`, which compares this code's output byte for byte
//! against the C library's through GNU `printf`.
//!
//! It is a shared module rather than part of `printf` because `printf` is not
//! the only caller in the tree's future: `awk` has a `printf` and a `sprintf`
//! with these same conversions, and the shell has `printf` as a builtin. Three
//! hand-written copies of the `%#.0o` rule would be three different answers.
//!
//! The width and precision are `usize` here and `int` in C. The difference is
//! not observable: glibc's limit is `INT_MAX`, and a field that wide cannot be
//! written to any real stream, so the only distinction is *how* the program
//! fails, and `printf` refuses such a width before reaching this module.

use crate::extfloat::{self, ExtF80};

pub use crate::extfloat::Spec;

/// The argument a directive consumes, already converted to the type its
/// conversion names.
///
/// The caller does the conversion because the caller is the one that can
/// diagnose it: `printf '%d' abc` has to say `expected a numeric value` and
/// keep going with zero, and that sentence is `printf`'s, not this module's.
#[derive(Clone, Copy, Debug)]
pub enum Value<'a> {
    /// For `%d` and `%i`.
    Signed(i64),
    /// For `%o`, `%u`, `%x` and `%X`.
    Unsigned(u64),
    /// For `%a %A %e %E %f %F %g %G`, which are handed to [`extfloat::render`].
    Float(ExtF80),
    /// For `%c`. One byte, not one character: the C locale is the only one
    /// these utilities implement, and there a `char` is a byte.
    Byte(u8),
    /// For `%s`. Bytes rather than a `str` because an argument is whatever the
    /// caller typed, which need not be UTF-8.
    Text(&'a [u8]),
}

/// Convert one value under one directive.
///
/// The `conv` field of `spec` decides which of `value`'s shapes is expected;
/// a mismatch is a caller bug and renders as if the value were the conversion's
/// zero, because there is no sensible diagnostic to make from inside here.
#[must_use]
pub fn render(spec: &Spec, value: Value<'_>) -> Vec<u8> {
    match (spec.conv, value) {
        (b'd' | b'i', Value::Signed(v)) => integer(spec, v.unsigned_abs(), v < 0, 10, false),
        (b'u', Value::Unsigned(v)) => integer(spec, v, false, 10, false),
        (b'o', Value::Unsigned(v)) => integer(spec, v, false, 8, false),
        (b'x', Value::Unsigned(v)) => integer(spec, v, false, 16, false),
        (b'X', Value::Unsigned(v)) => integer(spec, v, false, 16, true),
        (b'c', Value::Byte(b)) => pad(spec, &[b]),
        (b's', Value::Text(t)) => {
            let body = match spec.precision {
                Some(p) if p < t.len() => &t[..p],
                _ => t,
            };
            pad(spec, body)
        }
        (_, Value::Float(v)) => extfloat::render(spec, v).into_bytes(),
        // A shape the conversion did not ask for. Render the conversion's zero
        // rather than panic: this is unreachable from `printf`, which picks the
        // shape from the same `conv` byte, and a panic in a formatting routine
        // is a worse outcome than a wrong digit in a case that cannot happen.
        (b'd' | b'i', _) => integer(spec, 0, false, 10, false),
        (b'u' | b'o' | b'x' | b'X', _) => integer(spec, 0, false, 10, false),
        (b'c', _) => pad(spec, &[0]),
        _ => pad(spec, b""),
    }
}

/// `%d %i %o %u %x %X`.
///
/// `magnitude` is the absolute value, so that `i64::MIN` — whose negation does
/// not fit an `i64` — needs no special case.
fn integer(spec: &Spec, magnitude: u64, negative: bool, base: u32, upper: bool) -> Vec<u8> {
    let mut digits = to_digits(magnitude, base, upper);

    // A precision is the *minimum* number of digits, and it is the one place
    // where asking for zero digits of the value zero really does mean zero
    // digits: `printf '%.0d' 0` writes nothing.
    if let Some(p) = spec.precision {
        if magnitude == 0 && p == 0 {
            digits.clear();
        }
        while digits.len() < p {
            digits.insert(0, b'0');
        }
    }

    // `#` on octal is defined as "increase the precision until the first digit
    // is a zero", which is one digit's worth whenever it is not already zero --
    // and which, unlike a plain prepend, still fires when the precision left us
    // with no digits at all.
    let mut prefix: &[u8] = b"";
    if spec.hash {
        match base {
            8 if digits.first() != Some(&b'0') => digits.insert(0, b'0'),
            // `%#x` of zero has no `0x`: there is nothing for the prefix to say
            // the base of.
            16 if magnitude != 0 => prefix = if upper { b"0X" } else { b"0x" },
            _ => {}
        }
    }

    // Only `%d` and `%i` have a sign, and only they are reached with `negative`
    // set; `+` and a space on an unsigned conversion are accepted and ignored,
    // exactly as glibc accepts and ignores them.
    let signed = matches!(spec.conv, b'd' | b'i');
    let sign: &[u8] = if negative {
        b"-"
    } else if !signed {
        b""
    } else if spec.plus {
        b"+"
    } else if spec.space {
        b" "
    } else {
        b""
    };

    let mut body = Vec::with_capacity(sign.len() + prefix.len() + digits.len());
    body.extend_from_slice(sign);
    body.extend_from_slice(prefix);
    body.extend_from_slice(&digits);

    let fill = spec.width.saturating_sub(body.len());
    if fill == 0 {
        return body;
    }
    if spec.minus {
        body.extend(std::iter::repeat_n(b' ', fill));
        return body;
    }
    // The `0` flag loses to `-` and loses to an explicit precision, so
    // `%-05d` and `%05.2d` both pad with spaces.
    if spec.zero && spec.precision.is_none() {
        // The zeros go *inside*: after the sign and after `0x`, so that
        // `%08.x` of 255 is `0x0000ff` and not `000x00ff`.
        let at = sign.len() + prefix.len();
        let mut out = Vec::with_capacity(body.len() + fill);
        out.extend_from_slice(&body[..at]);
        out.extend(std::iter::repeat_n(b'0', fill));
        out.extend_from_slice(&body[at..]);
        return out;
    }
    let mut out = Vec::with_capacity(body.len() + fill);
    out.extend(std::iter::repeat_n(b' ', fill));
    out.extend_from_slice(&body);
    out
}

/// The digits of `magnitude` in `base`, with no sign, no prefix and no padding.
/// Zero is one digit, `0` -- the caller removes it when a zero precision says
/// to.
fn to_digits(magnitude: u64, base: u32, upper: bool) -> Vec<u8> {
    if magnitude == 0 {
        return vec![b'0'];
    }
    let alphabet: &[u8] = if upper {
        b"0123456789ABCDEF"
    } else {
        b"0123456789abcdef"
    };
    let base = u64::from(base);
    let mut out = Vec::new();
    let mut v = magnitude;
    while v != 0 {
        let d = usize::try_from(v % base).unwrap_or(0);
        out.push(alphabet.get(d).copied().unwrap_or(b'0'));
        v /= base;
    }
    out.reverse();
    out
}

/// Width padding for the conversions that have no sign, no prefix and no
/// zero fill: `%c` and `%s`. Both reach here having already had the precision
/// applied, because it means something different for each.
fn pad(spec: &Spec, body: &[u8]) -> Vec<u8> {
    let fill = spec.width.saturating_sub(body.len());
    let mut out = Vec::with_capacity(body.len() + fill);
    if spec.minus {
        out.extend_from_slice(body);
        out.extend(std::iter::repeat_n(b' ', fill));
    } else {
        out.extend(std::iter::repeat_n(b' ', fill));
        out.extend_from_slice(body);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shape `printf` builds when it has scanned a directive: every flag
    /// off, no width, no precision.
    fn spec(conv: u8) -> Spec {
        Spec {
            minus: false,
            plus: false,
            space: false,
            hash: false,
            zero: false,
            width: 0,
            precision: None,
            conv,
        }
    }

    fn s(spec: &Spec, v: Value<'_>) -> String {
        String::from_utf8(render(spec, v)).expect("ascii")
    }

    #[test]
    fn plain_integers() {
        assert_eq!(s(&spec(b'd'), Value::Signed(0)), "0");
        assert_eq!(s(&spec(b'd'), Value::Signed(-5)), "-5");
        assert_eq!(s(&spec(b'i'), Value::Signed(42)), "42");
        assert_eq!(
            s(&spec(b'u'), Value::Unsigned(u64::MAX)),
            "18446744073709551615"
        );
        assert_eq!(
            s(&spec(b'x'), Value::Unsigned(u64::MAX)),
            "ffffffffffffffff"
        );
        assert_eq!(s(&spec(b'X'), Value::Unsigned(255)), "FF");
        assert_eq!(s(&spec(b'o'), Value::Unsigned(8)), "10");
    }

    /// `i64::MIN` has no positive counterpart, so a conversion that negates
    /// before formatting overflows on exactly one input out of 2^64.
    #[test]
    fn the_most_negative_integer_formats() {
        assert_eq!(
            s(&spec(b'd'), Value::Signed(i64::MIN)),
            "-9223372036854775808"
        );
    }

    /// A precision on an integer is a floor on the digit count -- except for
    /// the single case of no digits at all.
    #[test]
    fn a_zero_precision_erases_only_the_value_zero() {
        let mut sp = spec(b'd');
        sp.precision = Some(0);
        assert_eq!(s(&sp, Value::Signed(0)), "");
        assert_eq!(s(&sp, Value::Signed(5)), "5");
        let mut sp = spec(b'o');
        sp.precision = Some(0);
        assert_eq!(s(&sp, Value::Unsigned(0)), "");
    }

    #[test]
    fn a_precision_pads_with_zeros_on_the_left() {
        let mut sp = spec(b'd');
        sp.precision = Some(3);
        sp.width = 5;
        assert_eq!(s(&sp, Value::Signed(7)), "  007");
    }

    /// `#` on octal raises the precision rather than prepending, which is why
    /// it can bring a digit back that a zero precision had removed.
    #[test]
    fn hash_on_octal_forces_a_leading_zero_even_past_a_zero_precision() {
        let mut sp = spec(b'o');
        sp.hash = true;
        sp.precision = Some(0);
        assert_eq!(s(&sp, Value::Unsigned(0)), "0");
        let mut sp = spec(b'o');
        sp.hash = true;
        assert_eq!(s(&sp, Value::Unsigned(0)), "0");
        assert_eq!(s(&sp, Value::Unsigned(8)), "010");
        // Already begins with a zero, so nothing is added.
        let mut sp = spec(b'o');
        sp.hash = true;
        sp.precision = Some(4);
        assert_eq!(s(&sp, Value::Unsigned(8)), "0010");
    }

    #[test]
    fn hash_on_hex_prefixes_everything_but_zero() {
        let mut sp = spec(b'x');
        sp.hash = true;
        assert_eq!(s(&sp, Value::Unsigned(0)), "0");
        assert_eq!(s(&sp, Value::Unsigned(255)), "0xff");
        let mut sp = spec(b'X');
        sp.hash = true;
        assert_eq!(s(&sp, Value::Unsigned(255)), "0XFF");
    }

    /// The sign flags belong to the signed conversions; glibc takes them on the
    /// others and does nothing with them, and so do we.
    #[test]
    fn sign_flags_are_ignored_by_the_unsigned_conversions() {
        let mut sp = spec(b'd');
        sp.plus = true;
        assert_eq!(s(&sp, Value::Signed(5)), "+5");
        assert_eq!(s(&sp, Value::Signed(-5)), "-5");
        let mut sp = spec(b'd');
        sp.space = true;
        assert_eq!(s(&sp, Value::Signed(5)), " 5");
        let mut sp = spec(b'u');
        sp.plus = true;
        assert_eq!(s(&sp, Value::Unsigned(5)), "5");
        let mut sp = spec(b'x');
        sp.plus = true;
        assert_eq!(s(&sp, Value::Unsigned(5)), "5");
    }

    #[test]
    fn the_zero_flag_loses_to_minus_and_to_a_precision() {
        let mut sp = spec(b'd');
        sp.zero = true;
        sp.width = 5;
        assert_eq!(s(&sp, Value::Signed(7)), "00007");
        sp.minus = true;
        assert_eq!(s(&sp, Value::Signed(7)), "7    ");
        let mut sp = spec(b'd');
        sp.zero = true;
        sp.width = 5;
        sp.precision = Some(2);
        assert_eq!(s(&sp, Value::Signed(7)), "   07");
    }

    /// The fill goes between the sign and the digits, not before the sign --
    /// `-0007`, never `000-7`.
    #[test]
    fn zero_fill_goes_after_the_sign_and_after_the_radix_prefix() {
        let mut sp = spec(b'd');
        sp.zero = true;
        sp.width = 5;
        assert_eq!(s(&sp, Value::Signed(-7)), "-0007");
        let mut sp = spec(b'x');
        sp.zero = true;
        sp.hash = true;
        sp.width = 8;
        assert_eq!(s(&sp, Value::Unsigned(255)), "0x0000ff");
    }

    #[test]
    fn strings_are_truncated_by_a_precision_and_padded_by_a_width() {
        let mut sp = spec(b's');
        assert_eq!(s(&sp, Value::Text(b"abc")), "abc");
        sp.precision = Some(1);
        assert_eq!(s(&sp, Value::Text(b"abc")), "a");
        sp.precision = Some(0);
        assert_eq!(s(&sp, Value::Text(b"abc")), "");
        let mut sp = spec(b's');
        sp.width = 5;
        assert_eq!(s(&sp, Value::Text(b"ab")), "   ab");
        sp.minus = true;
        assert_eq!(s(&sp, Value::Text(b"ab")), "ab   ");
        let mut sp = spec(b's');
        sp.width = 5;
        sp.precision = Some(1);
        assert_eq!(s(&sp, Value::Text(b"abc")), "    a");
    }

    /// A string argument is bytes: `printf %s` of a name that is not UTF-8 has
    /// to come out unchanged, so nothing here may go through `str`.
    #[test]
    fn a_string_that_is_not_utf8_survives() {
        let sp = spec(b's');
        assert_eq!(render(&sp, Value::Text(b"\xff\xfe")), b"\xff\xfe");
    }

    #[test]
    fn a_char_is_one_byte_with_width_padding() {
        let mut sp = spec(b'c');
        assert_eq!(render(&sp, Value::Byte(b'x')), b"x");
        assert_eq!(render(&sp, Value::Byte(0)), b"\0");
        sp.width = 5;
        assert_eq!(s(&sp, Value::Byte(b'x')), "    x");
        sp.minus = true;
        assert_eq!(s(&sp, Value::Byte(b'x')), "x    ");
    }

    /// The floating point conversions are `extfloat`'s; this only checks that
    /// they are actually reached, not what they say.
    #[test]
    fn floats_are_handed_to_extfloat() {
        let mut sp = spec(b'f');
        sp.precision = Some(2);
        assert_eq!(s(&sp, Value::Float(ExtF80::ONE)), "1.00");
    }
}
