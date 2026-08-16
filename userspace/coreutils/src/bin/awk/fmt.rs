//! `printf` and `sprintf` — C's format strings, over bytes.
//!
//! ## Why this is written out rather than delegated to Rust's formatter
//!
//! `format!` cannot do this. It has no `%5.2f`, no `%-8s`, no `%o`/`%x` with a
//! `#` prefix, no `*` taking the width from the argument list, and no notion of
//! consuming arguments positionally as it goes. awk's `printf` is C's, and
//! scripts depend on the exact column alignment it produces — a report whose
//! columns move is a broken report even though every character in it is
//! individually correct.
//!
//! ## The rules that are awk's rather than C's
//!
//! * **Too few arguments is not an error.** A missing argument is the empty
//!   string or zero, so `printf "%s-%s\n", "a"` prints `a-`. Every awk does
//!   this and scripts rely on it.
//! * **Extra arguments are ignored**, not an error, and not recycled.
//! * **A conversion this does not know is a fatal error**, because the
//!   alternative — printing it literally — silently drops the argument that was
//!   meant for it.
//! * `%c` takes a *character* from a string argument, or a numeric argument as
//!   a character code.
//!
//! Output is bytes throughout: `%s` of a field that is not UTF-8 must come out
//! byte for byte, and width is counted in characters so that a column of
//! accented words still lines up.

use crate::value::{Str, Value};
use ere::ch;

/// Format `args` by `fmt`, or say why it cannot be done.
///
/// # Errors
/// Returns the diagnostic text for an unknown or malformed conversion.
pub fn sprintf(fmt: &[u8], args: &[Value], convfmt: &[u8]) -> Result<Str, String> {
    let mut out = Str::new();
    let mut next = 0usize;
    let mut i = 0usize;
    // A missing argument is the empty string / zero, so this never fails; it is
    // a function only so the three call sites below cannot forget the rule.
    let take = |next: &mut usize| -> Value {
        let v = args.get(*next).cloned().unwrap_or(Value::Uninit);
        *next = next.saturating_add(1);
        v
    };

    while let Some(&c) = fmt.get(i) {
        if c != b'%' {
            out.push(c);
            i = i.saturating_add(1);
            continue;
        }
        i = i.saturating_add(1);
        if fmt.get(i) == Some(&b'%') {
            out.push(b'%');
            i = i.saturating_add(1);
            continue;
        }

        let mut spec = Spec::default();
        // Flags.
        loop {
            match fmt.get(i) {
                Some(b'-') => spec.left = true,
                Some(b'+') => spec.plus = true,
                Some(b' ') => spec.space = true,
                Some(b'#') => spec.alt = true,
                Some(b'0') => spec.zero = true,
                _ => break,
            }
            i = i.saturating_add(1);
        }
        // Width, possibly taken from the argument list.
        if fmt.get(i) == Some(&b'*') {
            i = i.saturating_add(1);
            let w = take(&mut next).to_num();
            // A negative `*` width means left-justify, as in C.
            if w < 0.0 {
                spec.left = true;
                spec.width = clamp_len(-w);
            } else {
                spec.width = clamp_len(w);
            }
        } else {
            let mut w = 0usize;
            let mut any = false;
            while let Some(d) = fmt.get(i).filter(|b| b.is_ascii_digit()) {
                w = w.saturating_mul(10).saturating_add(usize::from(d.wrapping_sub(b'0')));
                any = true;
                i = i.saturating_add(1);
            }
            if any {
                spec.width = w.min(MAX_FIELD);
            }
        }
        // Precision.
        if fmt.get(i) == Some(&b'.') {
            i = i.saturating_add(1);
            if fmt.get(i) == Some(&b'*') {
                i = i.saturating_add(1);
                let p = take(&mut next).to_num();
                // C says a negative `*` precision is as if it were omitted.
                spec.prec = if p < 0.0 { None } else { Some(clamp_len(p)) };
            } else {
                let mut p = 0usize;
                while let Some(d) = fmt.get(i).filter(|b| b.is_ascii_digit()) {
                    p = p.saturating_mul(10).saturating_add(usize::from(d.wrapping_sub(b'0')));
                    i = i.saturating_add(1);
                }
                // `%.f` means precision zero, which is not the same as no
                // precision: `%.f` of 1.5 is `2`, `%f` of it is `1.500000`.
                spec.prec = Some(p.min(MAX_FIELD));
            }
        }
        // Length modifiers exist in C and mean nothing here; skip them so a
        // format string written for C still works.
        while matches!(fmt.get(i), Some(b'h' | b'l' | b'L' | b'q' | b'j' | b'z' | b't')) {
            i = i.saturating_add(1);
        }

        let Some(&conv) = fmt.get(i) else {
            return Err("printf: format string ends with an incomplete conversion".to_string());
        };
        i = i.saturating_add(1);

        let body = match conv {
            b'd' | b'i' => integer(&spec, take(&mut next).to_num(), 10, false, true),
            b'o' => integer(&spec, take(&mut next).to_num(), 8, false, false),
            b'x' => integer(&spec, take(&mut next).to_num(), 16, false, false),
            b'X' => integer(&spec, take(&mut next).to_num(), 16, true, false),
            b'u' => integer(&spec, take(&mut next).to_num(), 10, false, false),
            b'c' => character(&take(&mut next), convfmt),
            b's' => {
                let v = take(&mut next);
                let s = v.to_str(convfmt);
                match spec.prec {
                    // A precision on `%s` is a maximum length, in characters.
                    Some(p) => take_chars(&s, p),
                    None => s.as_ref().clone(),
                }
            }
            b'e' | b'E' | b'f' | b'F' | b'g' | b'G' | b'a' | b'A' => {
                floating(&spec, conv, take(&mut next).to_num())
            }
            other => {
                let shown = char::from(other);
                return Err(format!("printf: unknown conversion `%{shown}'"));
            }
        };
        let kind = match conv {
            b'd' | b'i' | b'o' | b'x' | b'X' | b'u' => Pad::Integer,
            b'e' | b'E' | b'f' | b'F' | b'g' | b'G' | b'a' | b'A' => Pad::Float,
            _ => Pad::Text,
        };
        pad(&mut out, &body, &spec, kind);
    }
    Ok(out)
}

/// Format one number by a one-conversion format string — what CONVFMT and OFMT
/// do to a non-integral number.
#[must_use]
pub fn sprintf_one_number(fmt: &[u8], n: f64) -> Str {
    // CONVFMT is under the program's control, so it can be nonsense. A number
    // has to come out regardless — this is called from string conversion, which
    // has no way to report an error — so fall back on the default format.
    sprintf(fmt, &[Value::Num(n)], b"%.6g")
        .unwrap_or_else(|_| sprintf(b"%.6g", &[Value::Num(n)], b"%.6g").unwrap_or_default())
}

/// A width or precision past this is a request to allocate gigabytes from a
/// three-character format string, so it is clamped rather than honoured.
const MAX_FIELD: usize = 1 << 20;

fn clamp_len(v: f64) -> usize {
    if !v.is_finite() || v <= 0.0 {
        return 0;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let n = if v >= MAX_FIELD as f64 { MAX_FIELD } else { v as usize };
    n
}

#[derive(Default, Clone, Copy)]
struct Spec {
    left: bool,
    plus: bool,
    space: bool,
    alt: bool,
    zero: bool,
    width: usize,
    prec: Option<usize>,
}

/// What kind of thing is being padded, which is all `pad` needs to know about
/// the conversion — the three kinds differ only in how they treat `0`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Pad {
    Integer,
    Float,
    Text,
}

/// Place `body` in a field `spec.width` wide.
///
/// Zero-padding goes *after* the sign, not before it, which is why the sign is
/// split back off here rather than being handled by the callers.
fn pad(out: &mut Str, body: &[u8], spec: &Spec, kind: Pad) {
    let len = ch::chars(body).count();
    if len >= spec.width {
        out.extend_from_slice(body);
        return;
    }
    let fill = spec.width.saturating_sub(len);
    if spec.left {
        out.extend_from_slice(body);
        out.extend(std::iter::repeat_n(b' ', fill));
        return;
    }
    // `0` pads numbers, never text, and is ignored when the field is
    // left-justified (handled above). The precision cancels it only for the
    // *integer* conversions — `%05.2f` of 3.5 is `03.50`, but `%05.2d` of 3
    // is `   03` — because for an integer the precision is itself a minimum
    // digit count, so two zero-padding rules would be fighting.
    let zero_ok = match kind {
        Pad::Integer => spec.prec.is_none(),
        Pad::Float => true,
        Pad::Text => false,
    };
    if spec.zero && zero_ok {
        let sign_len = usize::from(matches!(body.first(), Some(b'-' | b'+' | b' ')));
        out.extend_from_slice(body.get(..sign_len).unwrap_or_default());
        out.extend(std::iter::repeat_n(b'0', fill));
        out.extend_from_slice(body.get(sign_len..).unwrap_or_default());
        return;
    }
    out.extend(std::iter::repeat_n(b' ', fill));
    out.extend_from_slice(body);
}

/// `%d`, `%i`, `%o`, `%x`, `%X`, `%u`.
///
/// The value arrives as an `f64` because that is awk's only number type, and it
/// is truncated toward zero, as C's cast is.
fn integer(spec: &Spec, v: f64, base: u32, upper: bool, signed: bool) -> Str {
    let t = if v.is_finite() { v.trunc() } else { 0.0 };
    #[allow(clippy::cast_possible_truncation)]
    let n: i64 = if t >= 9.223_372_036_854_775e18 {
        i64::MAX
    } else if t <= -9.223_372_036_854_775e18 {
        i64::MIN
    } else {
        t as i64
    };
    let neg = signed && n < 0;
    // For the unsigned conversions a negative value is its two's-complement
    // bit pattern, as C's `(unsigned)` cast gives.
    #[allow(clippy::cast_sign_loss)]
    let mag: u64 = if signed { n.unsigned_abs() } else { n as u64 };

    let mut digits = to_radix(mag, base, upper);
    if let Some(p) = spec.prec {
        // A precision on an integer is a *minimum* digit count.
        while digits.len() < p {
            digits.insert(0, b'0');
        }
        // `%.0d` of zero prints nothing at all, which is the one case where a
        // conversion produces no characters.
        if p == 0 && mag == 0 {
            digits.clear();
        }
    }
    if spec.alt {
        match base {
            8 if !digits.starts_with(b"0") => digits.insert(0, b'0'),
            16 if mag != 0 => {
                let p: &[u8] = if upper { b"0X" } else { b"0x" };
                let mut with = p.to_vec();
                with.extend_from_slice(&digits);
                digits = with;
            }
            _ => {}
        }
    }
    let mut out = Str::new();
    if neg {
        out.push(b'-');
    } else if signed && spec.plus {
        out.push(b'+');
    } else if signed && spec.space {
        out.push(b' ');
    }
    out.extend_from_slice(&digits);
    out
}

fn to_radix(mut n: u64, base: u32, upper: bool) -> Str {
    if n == 0 {
        return b"0".to_vec();
    }
    let digits: &[u8] = if upper { b"0123456789ABCDEF" } else { b"0123456789abcdef" };
    let base64 = u64::from(base.max(2));
    let mut out = Str::new();
    while n > 0 {
        let d = usize::try_from(n % base64).unwrap_or(0);
        out.push(digits.get(d).copied().unwrap_or(b'0'));
        n /= base64;
    }
    out.reverse();
    out
}

/// `%e`, `%f`, `%g` and their uppercase and hexadecimal forms.
fn floating(spec: &Spec, conv: u8, v: f64) -> Str {
    let prec = spec.prec.unwrap_or(6);
    if !v.is_finite() {
        let word: &[u8] = if v.is_nan() {
            if conv.is_ascii_uppercase() { b"NAN" } else { b"nan" }
        } else if conv.is_ascii_uppercase() {
            b"INF"
        } else {
            b"inf"
        };
        let mut out = Str::new();
        if v.is_sign_negative() {
            out.push(b'-');
        } else if spec.plus {
            out.push(b'+');
        } else if spec.space {
            out.push(b' ');
        }
        out.extend_from_slice(word);
        return out;
    }

    let body = match conv {
        b'f' | b'F' => format!("{:.*}", prec, v.abs()),
        b'e' => exponential(v.abs(), prec, false, spec.alt),
        b'E' => exponential(v.abs(), prec, true, spec.alt),
        b'a' | b'A' => {
            // Hexadecimal floats are vanishingly rare in awk and Rust has no
            // formatter for them; `%e` is the closest honest answer, and it is
            // still a number of the right value.
            exponential(v.abs(), prec, conv == b'A', spec.alt)
        }
        _ => general(v.abs(), prec, conv == b'G', spec.alt),
    };
    let mut out = Str::new();
    if v.is_sign_negative() {
        out.push(b'-');
    } else if spec.plus {
        out.push(b'+');
    } else if spec.space {
        out.push(b' ');
    }
    out.extend_from_slice(body.as_bytes());
    // `%#f` keeps the point even at precision zero.
    if spec.alt && prec == 0 && !out.contains(&b'.') && matches!(conv, b'f' | b'F' | b'e' | b'E') {
        out.push(b'.');
    }
    out
}

/// `%e` of a non-negative, finite value: one digit, the point, `prec` digits,
/// then `e` and a signed exponent of at least two digits.
fn exponential(v: f64, prec: usize, upper: bool, alt: bool) -> String {
    // `{:e}` gives `1.5e2`; C wants `1.500000e+02`, so the exponent is
    // reassembled rather than reused.
    let (mant, exp) = if v == 0.0 {
        (0.0_f64, 0_i32)
    } else {
        let e = v.abs().log10().floor();
        #[allow(clippy::cast_possible_truncation)]
        let mut e = e as i32;
        let mut m = v / 10_f64.powi(e);
        // log10 of a power of ten can land a hair below the integer, and
        // rounding the mantissa can push it to 10; both are corrected here so
        // `%e` of 1e-5 is `1.000000e-05` and not `10.000000e-06`.
        if m >= 10.0 {
            m /= 10.0;
            e = e.saturating_add(1);
        } else if m < 1.0 {
            m *= 10.0;
            e = e.saturating_sub(1);
        }
        if format!("{:.*}", prec, m).starts_with("10") {
            m /= 10.0;
            e = e.saturating_add(1);
        }
        (m, e)
    };
    let mut s = format!("{:.*}", prec, mant);
    if alt && prec == 0 {
        s.push('.');
    }
    let sign = if exp < 0 { '-' } else { '+' };
    let e = if upper { 'E' } else { 'e' };
    format!("{s}{e}{sign}{:02}", exp.unsigned_abs())
}

/// `%g`: `%e` or `%f`, whichever is shorter, with trailing zeros removed.
fn general(v: f64, prec: usize, upper: bool, alt: bool) -> String {
    let p = prec.max(1);
    #[allow(clippy::cast_possible_truncation)]
    let exp: i32 = if v == 0.0 {
        0
    } else {
        let e = v.abs().log10().floor() as i32;
        // Re-derive from the rounded rendering, so a value that rounds up into
        // the next decade picks the same branch C would.
        let probe = format!("{:.*e}", p.saturating_sub(1), v);
        probe.split(['e', 'E']).nth(1).and_then(|t| t.parse::<i32>().ok()).unwrap_or(e)
    };
    #[allow(clippy::cast_possible_wrap)]
    let p_i = p as i32;
    let mut s = if exp < -4 || exp >= p_i {
        exponential(v, p.saturating_sub(1), upper, alt)
    } else {
        let decimals = usize::try_from(p_i.saturating_sub(1).saturating_sub(exp)).unwrap_or(0);
        format!("{:.*}", decimals, v)
    };
    if !alt {
        s = strip_trailing_zeros(&s);
    }
    s
}

/// Remove the trailing zeros `%g` does not print, and the point if nothing is
/// left after it. Applies to the mantissa only, never to the exponent.
fn strip_trailing_zeros(s: &str) -> String {
    let (mant, exp) = match s.find(['e', 'E']) {
        Some(i) => (s.get(..i).unwrap_or(s), s.get(i..).unwrap_or("")),
        None => (s, ""),
    };
    if !mant.contains('.') {
        return s.to_string();
    }
    let trimmed = mant.trim_end_matches('0');
    let trimmed = trimmed.strip_suffix('.').unwrap_or(trimmed);
    format!("{trimmed}{exp}")
}

/// `%c`: one character.
///
/// A string argument contributes its first character; a number is a character
/// *code*. The split matters — `printf "%c", 65` is `A` but `printf "%c", "65"`
/// is `6` — and awk decides by which reading the value has, so a strnum from
/// input takes the numeric branch, as gawk does.
fn character(v: &Value, convfmt: &[u8]) -> Str {
    if let Value::Str(s) = v {
        return ch::chars(s).next().map(ch::Ch::to_str).unwrap_or_default();
    }
    let n = v.to_num();
    if let Value::StrNum(s, _) = v {
        // An input field that happens to look numeric is still text to the eye;
        // but POSIX says a numeric value is a code, and a strnum is numeric.
        // Only fall back to the text when the number cannot name a character.
        if n < 0.0 || n > f64::from(u32::MAX) {
            return ch::chars(s).next().map(ch::Ch::to_str).unwrap_or_default();
        }
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let code = if (0.0..=f64::from(u32::MAX)).contains(&n) { n as u32 } else { 0 };
    // A code that is not a scalar value is emitted as the single byte it names
    // when it can be, because awk is a byte filter and `printf "%c", 200` in a
    // pipeline is expected to produce one byte.
    match char::from_u32(code) {
        Some(c) if code >= 0x80 => {
            let _ = convfmt;
            let mut b = [0u8; 4];
            c.encode_utf8(&mut b).as_bytes().to_vec()
        }
        Some(c) => vec![c as u8],
        None => Str::new(),
    }
}

/// The first `n` characters of `s`, as bytes.
fn take_chars(s: &[u8], n: usize) -> Str {
    let mut out = Str::new();
    for c in ch::chars(s).take(n) {
        c.push_to(&mut out);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(fmt: &str, args: &[Value]) -> String {
        String::from_utf8_lossy(&sprintf(fmt.as_bytes(), args, b"%.6g").unwrap()).into_owned()
    }
    fn n(v: f64) -> Value {
        Value::Num(v)
    }
    fn s(v: &str) -> Value {
        Value::str(v.as_bytes().to_vec())
    }

    #[test]
    fn widths_and_flags_line_columns_up() {
        assert_eq!(f("[%5d]", &[n(42.0)]), "[   42]");
        assert_eq!(f("[%-5d]", &[n(42.0)]), "[42   ]");
        assert_eq!(f("[%05d]", &[n(42.0)]), "[00042]");
        assert_eq!(f("[%+d]", &[n(42.0)]), "[+42]");
        assert_eq!(f("[% d]", &[n(42.0)]), "[ 42]");
        // The zero pad goes after the sign, not before it.
        assert_eq!(f("[%05d]", &[n(-42.0)]), "[-0042]");
        assert_eq!(f("[%8.3f]", &[n(1.23456)]), "[   1.235]");
        assert_eq!(f("[%-8s|]", &[s("hi")]), "[hi      |]");
        assert_eq!(f("[%.2s]", &[s("hello")]), "[he]");
    }

    #[test]
    fn a_missing_argument_is_empty_rather_than_an_error() {
        // Every awk does this, and scripts rely on it.
        assert_eq!(f("%s-%s", &[s("a")]), "a-");
        assert_eq!(f("%d", &[]), "0");
        // Extra arguments are dropped, not recycled.
        assert_eq!(f("%s", &[s("a"), s("b")]), "a");
    }

    #[test]
    fn an_unknown_conversion_is_refused_rather_than_printed() {
        // Printing `%w` literally would silently swallow its argument.
        let e = sprintf(b"%w", &[n(1.0)], b"%.6g").unwrap_err();
        assert!(e.contains("%w"), "{e}");
        // `z` is a length modifier, so `%z` is not unknown — it is unfinished,
        // and saying so is more useful than naming `z` as the conversion.
        let e = sprintf(b"%z", &[n(1.0)], b"%.6g").unwrap_err();
        assert!(e.contains("incomplete"), "{e}");
    }

    #[test]
    fn the_bases_and_their_alternate_forms() {
        assert_eq!(f("%o", &[n(8.0)]), "10");
        assert_eq!(f("%#o", &[n(8.0)]), "010");
        assert_eq!(f("%x", &[n(255.0)]), "ff");
        assert_eq!(f("%X", &[n(255.0)]), "FF");
        assert_eq!(f("%#x", &[n(255.0)]), "0xff");
        assert_eq!(f("%.5d", &[n(42.0)]), "00042");
    }

    #[test]
    fn floating_point_matches_c() {
        assert_eq!(f("%f", &[n(1.5)]), "1.500000");
        assert_eq!(f("%.0f", &[n(1.5)]), "2");
        assert_eq!(f("%e", &[n(1500.0)]), "1.500000e+03");
        assert_eq!(f("%e", &[n(0.0)]), "0.000000e+00");
        assert_eq!(f("%.2e", &[n(0.000_015)]), "1.50e-05");
        assert_eq!(f("%g", &[n(100_000.0)]), "100000");
        assert_eq!(f("%g", &[n(1_000_000.0)]), "1e+06");
        assert_eq!(f("%g", &[n(0.0001)]), "0.0001");
        assert_eq!(f("%g", &[n(0.000_01)]), "1e-05");
        assert_eq!(f("%.3g", &[n(1.23456)]), "1.23");
    }

    #[test]
    fn a_star_takes_the_width_from_the_arguments() {
        assert_eq!(f("[%*d]", &[n(5.0), n(42.0)]), "[   42]");
        assert_eq!(f("[%-*d]", &[n(5.0), n(42.0)]), "[42   ]");
        // A negative `*` width left-justifies, as in C.
        assert_eq!(f("[%*d]", &[n(-5.0), n(42.0)]), "[42   ]");
        assert_eq!(f("[%.*f]", &[n(2.0), n(1.23456)]), "[1.23]");
    }

    #[test]
    fn percent_c_takes_a_character_or_a_code() {
        assert_eq!(f("%c", &[n(65.0)]), "A");
        assert_eq!(f("%c", &[s("65")]), "6");
        assert_eq!(f("%%", &[]), "%");
    }

    #[test]
    fn a_byte_that_is_not_text_survives_percent_s() {
        let v = Value::str(vec![0xff, b'a']);
        assert_eq!(sprintf(b"%s", &[v], b"%.6g").unwrap(), vec![0xff, b'a']);
    }

    fn f2(fmt: &str) -> String {
        f(fmt, &[])
    }
    #[test]
    fn a_literal_percent_needs_no_argument() {
        assert_eq!(f2("100%%"), "100%");
    }
}
