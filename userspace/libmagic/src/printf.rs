//! C's `printf`, as libmagic calls it.
//!
//! libmagic builds every description with `file_printf`, which is
//! `vasprintf` over a format and the arguments its call site passed. A magic
//! rule's description *is* a format -- `%d`, `%#x`, `%.4s`, `%-10.3s` -- and
//! the value the rule read is the argument, so the conversions have to be
//! glibc's to the byte. They are: the work is `cprintf`'s, measured against
//! glibc, and this module only reads the directives out of a format and hands
//! each its argument.
//!
//! An argument is given as the C type the call site passes after the default
//! promotions -- `int`, `long long`, `double`, `char *` -- so that a `%u` of a
//! negative `int` and a `%c` of a short come out as C's do.

use cprintf::cfmt::{self, Spec, Value};
use cprintf::extfloat::ExtF80;

/// One argument, as the varargs call passes it.
#[derive(Clone, Copy, Debug)]
pub enum Arg<'a> {
    /// An `int` or `unsigned int`, or anything promoted to one: its 32 bits.
    I32(u32),
    /// A `long`, `long long`, `size_t` or an unsigned form of one: 64 bits.
    I64(u64),
    /// A `double`; a `float` is promoted to one.
    F64(f64),
    /// A `char *`: its bytes, which end at the first NUL.
    Str(&'a [u8]),
}

/// Why a format could not be applied: `vasprintf` returning -1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FormatError;

/// The length modifier of a directive.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Len {
    None,
    Hh,
    H,
    /// `l`, `ll`, `q`, `j`, `z`, `Z`, `t`, `L`: every one of them is 64 bits on
    /// the machines libmagic is measured on.
    Wide,
}

/// `vasprintf(fmt, args)`.
///
/// # Errors
/// [`FormatError`] when the format ends inside a directive, which glibc
/// reports as `EINVAL` and `vasprintf` as -1.
pub fn format(fmt: &[u8], args: &[Arg<'_>]) -> Result<Vec<u8>, FormatError> {
    let mut out = Vec::new();
    let mut next = args.iter();
    let mut i = 0usize;
    // A format is a C string: it ends at its first NUL.
    let fmt = fmt.get(..fmt.iter().position(|&c| c == 0).unwrap_or(fmt.len())).unwrap_or(fmt);
    while i < fmt.len() {
        let c = fmt[i];
        if c != b'%' {
            out.push(c);
            i += 1;
            continue;
        }
        let start = i;
        i += 1;
        let mut spec = Spec {
            minus: false,
            plus: false,
            space: false,
            hash: false,
            zero: false,
            width: 0,
            precision: None,
            conv: 0,
        };
        // Flags, in any order and repeated. `'` groups digits, which the C
        // locale has none of; `I` asks for the locale's digits, likewise.
        while let Some(&f) = fmt.get(i) {
            match f {
                b'-' => spec.minus = true,
                b'+' => spec.plus = true,
                b' ' => spec.space = true,
                b'#' => spec.hash = true,
                b'0' => spec.zero = true,
                b'\'' | b'I' => {}
                _ => break,
            }
            i += 1;
        }
        // Width: digits, or `*` for an `int` argument, a negative one meaning
        // `-` and its magnitude.
        if fmt.get(i) == Some(&b'*') {
            i += 1;
            #[allow(clippy::cast_possible_wrap)]
            let w = int_arg(next.next()) as i32;
            if w < 0 {
                spec.minus = true;
            }
            spec.width = usize::try_from(w.unsigned_abs()).unwrap_or(usize::MAX);
        } else {
            while let Some(&d) = fmt.get(i).filter(|d| d.is_ascii_digit()) {
                spec.width = spec
                    .width
                    .saturating_mul(10)
                    .saturating_add(usize::from(d - b'0'));
                i += 1;
            }
        }
        // Precision: `.` and digits (none is zero), or `.*`, where a negative
        // argument is as if there were no precision at all.
        if fmt.get(i) == Some(&b'.') {
            i += 1;
            if fmt.get(i) == Some(&b'*') {
                i += 1;
                #[allow(clippy::cast_possible_wrap)]
                let p = int_arg(next.next()) as i32;
                spec.precision = usize::try_from(p).ok();
            } else {
                let mut p = 0usize;
                while let Some(&d) = fmt.get(i).filter(|d| d.is_ascii_digit()) {
                    p = p.saturating_mul(10).saturating_add(usize::from(d - b'0'));
                    i += 1;
                }
                spec.precision = Some(p);
            }
        }
        let len = match fmt.get(i) {
            Some(b'h') if fmt.get(i + 1) == Some(&b'h') => {
                i += 2;
                Len::Hh
            }
            Some(b'h') => {
                i += 1;
                Len::H
            }
            Some(b'l') if fmt.get(i + 1) == Some(&b'l') => {
                i += 2;
                Len::Wide
            }
            Some(b'l' | b'q' | b'j' | b'z' | b'Z' | b't' | b'L') => {
                i += 1;
                Len::Wide
            }
            _ => Len::None,
        };
        let Some(&conv) = fmt.get(i) else {
            // The format ended inside the directive.
            return Err(FormatError);
        };
        i += 1;
        spec.conv = conv;
        match conv {
            b'%' => out.push(b'%'),
            b'd' | b'i' => {
                let v = signed(next.next(), len);
                out.extend_from_slice(&cfmt::render(&spec, Value::Signed(v)));
            }
            b'u' | b'o' | b'x' | b'X' => {
                let v = unsigned(next.next(), len);
                out.extend_from_slice(&cfmt::render(&spec, Value::Unsigned(v)));
            }
            b'c' => {
                #[allow(clippy::cast_possible_truncation)]
                let b = int_arg(next.next()) as u8;
                out.extend_from_slice(&cfmt::render(&spec, Value::Byte(b)));
            }
            b's' => {
                let s = match next.next() {
                    Some(Arg::Str(s)) => *s,
                    _ => b"(null)",
                };
                let s = s.get(..s.iter().position(|&c| c == 0).unwrap_or(s.len())).unwrap_or(s);
                out.extend_from_slice(&cfmt::render(&spec, Value::Text(s)));
            }
            b'a' | b'A' => {
                let v = float_arg(next.next());
                out.extend_from_slice(&hex_double(&spec, v));
            }
            b'e' | b'E' | b'f' | b'F' | b'g' | b'G' => {
                let v = float_arg(next.next());
                out.extend_from_slice(&cfmt::render(&spec, Value::Float(ExtF80::from_f64(v))));
            }
            // `%n` stores a count through a pointer, which no call site passes;
            // `%p` and `%m` no call site uses. Each takes its argument and
            // writes what glibc would for a null pointer or nothing.
            b'n' => {
                next.next();
            }
            b'p' => {
                next.next();
                out.extend_from_slice(&cfmt::render(&Spec { conv: b's', ..spec }, Value::Text(b"(nil)")));
            }
            _ => {
                // An unknown conversion: glibc prints the directive as it was
                // written.
                out.extend_from_slice(fmt.get(start..i).unwrap_or_default());
            }
        }
    }
    Ok(out)
}

/// The next argument read as an `int`'s bits.
fn int_arg(a: Option<&Arg<'_>>) -> u32 {
    match a {
        Some(Arg::I32(v)) => *v,
        #[allow(clippy::cast_possible_truncation)]
        Some(Arg::I64(v)) => *v as u32,
        _ => 0,
    }
}

/// The next argument read as `%d` with this length modifier reads it.
fn signed(a: Option<&Arg<'_>>, len: Len) -> i64 {
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    match (a, len) {
        (Some(Arg::I64(v)), Len::Wide) => *v as i64,
        (_, Len::Wide) => i64::from(int_arg(a) as i32),
        (_, Len::Hh) => i64::from(int_arg(a) as u8 as i8),
        (_, Len::H) => i64::from(int_arg(a) as u16 as i16),
        (_, Len::None) => i64::from(int_arg(a) as i32),
    }
}

/// The next argument read as `%u` with this length modifier reads it.
fn unsigned(a: Option<&Arg<'_>>, len: Len) -> u64 {
    #[allow(clippy::cast_possible_truncation)]
    match (a, len) {
        (Some(Arg::I64(v)), Len::Wide) => *v,
        (_, Len::Wide) => u64::from(int_arg(a)),
        (_, Len::Hh) => u64::from(int_arg(a) as u8),
        (_, Len::H) => u64::from(int_arg(a) as u16),
        (_, Len::None) => u64::from(int_arg(a)),
    }
}

/// The next argument read as a `double`.
fn float_arg(a: Option<&Arg<'_>>) -> f64 {
    match a {
        Some(Arg::F64(v)) => *v,
        _ => 0.0,
    }
}

/// `%a` of a `double`, as glibc writes it: `0x1.8p+1`, the leading digit the
/// integer bit -- not the `long double` form, whose leading digit is the top
/// four bits of a 64-bit significand.
fn hex_double(spec: &Spec, v: f64) -> Vec<u8> {
    let upper = spec.conv == b'A';
    let neg = v.is_sign_negative();
    let sign: &[u8] = if neg {
        b"-"
    } else if spec.plus {
        b"+"
    } else if spec.space {
        b" "
    } else {
        b""
    };
    let body: Vec<u8> = if v.is_nan() || v.is_infinite() {
        let word: &[u8] = match (v.is_nan(), upper) {
            (true, false) => b"nan",
            (true, true) => b"NAN",
            (false, false) => b"inf",
            (false, true) => b"INF",
        };
        let mut s = sign.to_vec();
        s.extend_from_slice(word);
        return pad_plain(spec, &s);
    } else {
        let bits = v.to_bits();
        let frac = bits & ((1u64 << 52) - 1);
        #[allow(clippy::cast_possible_truncation)]
        let biased = ((bits >> 52) & 0x7ff) as i32;
        // Zero is 0x0p+0; a subnormal is 0x0.<frac>p-1022.
        let (lead, mut frac, exp) = if biased == 0 {
            (0u64, frac, if frac == 0 { 0 } else { -1022 })
        } else {
            (1u64, frac, biased - 1023)
        };
        let mut lead = lead;
        // Thirteen hex digits of fraction; a precision rounds them, half to
        // even, carrying into the leading digit.
        let mut ndig = 13usize;
        if let Some(p) = spec.precision {
            if p < 13 {
                #[allow(clippy::cast_possible_truncation)]
                let drop = (13 - p as u32) * 4;
                let half = 1u64 << (drop - 1);
                let rest = frac & ((1u64 << drop) - 1);
                frac >>= drop;
                if rest > half || (rest == half && frac & 1 == 1) {
                    frac += 1;
                    if frac >> (p * 4) != 0 && p < 13 {
                        frac &= (1u64 << (p * 4)) - 1;
                        lead += 1;
                    }
                }
                ndig = p;
            }
        }
        let digits = if upper { b"0123456789ABCDEF" } else { b"0123456789abcdef" };
        let mut s = Vec::new();
        s.extend_from_slice(if upper { b"0X" } else { b"0x" });
        s.push(digits[usize::try_from(lead).unwrap_or(0) & 15]);
        let mut fd: Vec<u8> = (0..ndig)
            .map(|k| digits[usize::try_from((frac >> ((ndig - 1 - k) * 4)) & 15).unwrap_or(0)])
            .collect();
        if spec.precision.is_none() {
            while fd.last() == Some(&b'0') {
                fd.pop();
            }
        }
        // `%.13a` pads with zeros past the thirteen digits there are.
        if let Some(p) = spec.precision {
            while fd.len() < p {
                fd.push(b'0');
            }
        }
        if !fd.is_empty() || spec.hash {
            s.push(b'.');
        }
        s.extend_from_slice(&fd);
        s.push(if upper { b'P' } else { b'p' });
        s.extend_from_slice(format!("{exp:+}").as_bytes());
        s
    };
    // `0` pads between the `0x` and the digits.
    if spec.zero && !spec.minus && sign.len() + body.len() < spec.width {
        let mut s = sign.to_vec();
        s.extend_from_slice(body.get(..2).unwrap_or_default());
        s.resize(spec.width - body.len() + 2, b'0');
        s.extend_from_slice(body.get(2..).unwrap_or_default());
        return s;
    }
    let mut s = sign.to_vec();
    s.extend_from_slice(&body);
    pad_plain(spec, &s)
}

/// Pad to the field width with spaces, on the side `-` chooses.
fn pad_plain(spec: &Spec, s: &[u8]) -> Vec<u8> {
    if s.len() >= spec.width {
        return s.to_vec();
    }
    let fill = vec![b' '; spec.width - s.len()];
    if spec.minus {
        [s, &fill].concat()
    } else {
        [&fill[..], s].concat()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn f(fmt: &str, args: &[Arg<'_>]) -> String {
        String::from_utf8(format(fmt.as_bytes(), args).unwrap()).unwrap()
    }

    #[test]
    fn integers_read_their_c_type() {
        assert_eq!(f("%d", &[Arg::I32(0xffff_ffff)]), "-1");
        assert_eq!(f("%u", &[Arg::I32(0xffff_ffff)]), "4294967295");
        assert_eq!(f("%#x", &[Arg::I32(255)]), "0xff");
        assert_eq!(f("%lld", &[Arg::I64(u64::MAX)]), "-1");
        assert_eq!(f("%llu", &[Arg::I64(u64::MAX)]), "18446744073709551615");
        assert_eq!(f("%hhd", &[Arg::I32(0x1ff)]), "-1");
        assert_eq!(f("%c", &[Arg::I32(0x141)]), "A");
        assert_eq!(f("[%5d|%-5d|%05d]", &[Arg::I32(42), Arg::I32(42), Arg::I32(42)]), "[   42|42   |00042]");
    }

    #[test]
    fn strings_end_at_their_nul_and_their_precision() {
        assert_eq!(f("<%s>", &[Arg::Str(b"ab\0cd")]), "<ab>");
        assert_eq!(f("<%.3s>", &[Arg::Str(b"abcdef")]), "<abc>");
        assert_eq!(f("<%-6.2s>", &[Arg::Str(b"abcdef")]), "<ab    >");
        assert_eq!(f("%s, %s", &[Arg::Str(b"x"), Arg::Str(b"y")]), "x, y");
    }

    #[test]
    fn floats_are_doubles() {
        assert_eq!(f("%g", &[Arg::F64(0.1)]), "0.1");
        assert_eq!(f("%.2f", &[Arg::F64(2.675)]), "2.67");
        assert_eq!(f("%e", &[Arg::F64(1e300)]), "1.000000e+300");
        assert_eq!(f("%a", &[Arg::F64(3.0)]), "0x1.8p+1");
        assert_eq!(f("%a", &[Arg::F64(0.0)]), "0x0p+0");
        assert_eq!(f("%a", &[Arg::F64(f64::from_bits(1))]), "0x0.0000000000001p-1022");
        assert_eq!(f("%.1a", &[Arg::F64(1.96875)]), "0x2.0p+0");
    }

    #[test]
    fn odd_directives_are_glibcs() {
        assert_eq!(f("100%%", &[]), "100%");
        assert_eq!(f("%y", &[]), "%y");
        assert_eq!(f("%*d|%-*d", &[Arg::I32(4), Arg::I32(7), Arg::I32(3), Arg::I32(7)]), "   7|7  ");
        assert!(format(b"abc%", &[]).is_err());
        assert!(format(b"abc%-5", &[]).is_err());
    }
}
