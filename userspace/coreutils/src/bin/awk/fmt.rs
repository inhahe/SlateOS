//! `printf` and `sprintf` — gawk's `format_tree`, over bytes.
//!
//! ## Whose printf
//!
//! awk's `printf` is C's in outline, and scripts depend on the exact columns it
//! produces, so this is gawk 5.2.1's `format_tree` under `--posix`, read from
//! its source and measured against it, conversion by conversion:
//!
//! * **The flags are a state machine, not a grammar.** `-`, `+`, space, `#`,
//!   `0` and `'` are taken while the width is still being read, so `%5-d` is
//!   `%-5d`; one after the precision has begun ends the conversion. A space
//!   after a `+` is ignored, a `+` after a space wins.
//! * **A conversion this does not know is printed as written**, `%k` and all,
//!   and so is one cut off by the end of the format (`abc%5`): gawk copies the
//!   text from the `%` and carries on.
//! * **C's length modifiers are fatal under `--posix`**: `%ld` is `` `l' is
//!   not permitted in POSIX awk formats``, and `%1$d` is `` `$' is not
//!   permitted in awk formats``.
//! * **`%d` of any finite number prints all its digits**: gawk converts through
//!   `%.0f`, so `%d` of 2^64 is `18446744073709551616`, not a saturated
//!   `long`. `%o`, `%x`, `%X` and `%u` take the value as a 64-bit integer (a
//!   negative one as its two's complement), and one that does not fit -- or an
//!   infinity or a NaN given to any integer conversion -- is printed by `%g`
//!   instead, gawk's "emergency" format, with the flags and width given.
//! * **The floating conversions are C's**, digit for digit: the digits come
//!   from Rust's exact formatting, which rounds the binary value exactly as
//!   glibc does (ties to even), and are laid out by C's rules -- `%g`'s choice
//!   of style and its trailing zeros, `#`, the zero padding after the sign,
//!   `%a` as glibc writes hexadecimal floating point.
//!
//! ## Where this is not gawk's
//!
//! * **A missing argument is the empty string or zero**, so `printf
//!   "%s-%s\n", "a"` prints `a-`. gawk stops with `not enough arguments to
//!   satisfy format string`; bwk and mawk do this, and scripts rely on it (see
//!   main.rs's table of deliberate differences).
//! * **Widths and precisions are counted in characters** and `%c` of a number
//!   is that code point, as gawk does in a UTF-8 locale -- the only kind this
//!   system has.
//! * **A width or precision past 2^20 is clamped**: `%1000000000d` would
//!   otherwise allocate a gigabyte from a twelve-character format.
//!
//! Output is bytes throughout: `%s` of a field that is not UTF-8 comes out
//! byte for byte.

use crate::value::{Str, Value, c_long};
use ere::ch;

/// Format `args` by `fmt`.
///
/// # Errors
/// gawk's fatal diagnostic, `fatal:` included, for what `--posix` refuses: a
/// C length modifier or a `$` argument index.
pub fn sprintf(fmt: &[u8], args: &[Value], convfmt: &[u8]) -> Result<Str, String> {
    let mut out = Str::new();
    let mut next = 0usize;
    let mut take = || -> Value {
        let v = args.get(next).cloned().unwrap_or(Value::Uninit);
        next = next.saturating_add(1);
        v
    };

    let mut i = 0usize;
    // Where the text not yet copied begins: a conversion that turns out to be
    // malformed leaves this at its `%`, so the text is copied as written.
    let mut s0 = 0usize;
    while let Some(&c) = fmt.get(i) {
        if c != b'%' {
            i = i.saturating_add(1);
            continue;
        }
        out.extend_from_slice(fmt.get(s0..i).unwrap_or_default());
        s0 = i;
        i = i.saturating_add(1);

        let mut spec = Spec::default();
        let mut cur = Cur::Width;
        // The conversion character, once one is reached; `None` means the
        // format ended, or a character ended the conversion, first.
        let mut conv = None;
        while let Some(&cs) = fmt.get(i) {
            i = i.saturating_add(1);
            match cs {
                b'%' => {
                    // `%%`, whatever came between: the flags, width and
                    // precision mean nothing to it.
                    out.push(b'%');
                    s0 = i;
                    break;
                }
                b'0'..=b'9' => {
                    if cur == Cur::Done {
                        break;
                    }
                    // A `0` while the width is open is the flag -- even after
                    // a `*` width, so `%*05d` is zero-filled to 5 -- and then a
                    // digit, unless the field is left-justified, when it is
                    // only the flag.
                    if cs == b'0' && cur == Cur::Width {
                        spec.zero = true;
                        if spec.left {
                            continue;
                        }
                    }
                    i = digits(fmt, i, cs, &mut spec, &mut cur);
                }
                b'$' => return Err("fatal: `$' is not permitted in awk formats".to_string()),
                b'*' => {
                    if cur == Cur::Done {
                        break;
                    }
                    let n = c_long(take().to_num());
                    match cur {
                        Cur::Width => {
                            if n < 0 {
                                spec.left = true;
                            }
                            spec.width = clamp(n.unsigned_abs());
                        }
                        Cur::Prec => {
                            // A negative precision is as if none were given.
                            spec.prec = u64::try_from(n).ok().map(clamp);
                            cur = Cur::Done;
                        }
                        Cur::Done => {}
                    }
                }
                b' ' | b'+' => {
                    if cs == b'+' || spec.sign.is_none() {
                        spec.sign = Some(cs);
                    }
                    if cur != Cur::Width {
                        break;
                    }
                }
                b'-' => {
                    if spec.negative_prec {
                        break;
                    }
                    if cur == Cur::Prec {
                        // `%.-3d`: a negative precision, which is no precision.
                        spec.negative_prec = true;
                        continue;
                    }
                    spec.left = true;
                    if cur != Cur::Width {
                        break;
                    }
                }
                b'.' => {
                    if cur != Cur::Width {
                        break;
                    }
                    cur = Cur::Prec;
                    spec.prec = Some(0);
                }
                b'#' | b'\'' => {
                    if cs == b'#' {
                        spec.alt = true;
                    }
                    // `'` asks for thousands separators, which the C locale's
                    // numbers -- the only ones awk prints -- do not have.
                    if cur != Cur::Width {
                        break;
                    }
                }
                // C's length modifiers: the first one is fatal under
                // `--posix`, so gawk's check for a repeated one never matters.
                b'h' | b'j' | b'l' | b'L' | b't' | b'z' => {
                    return Err(format!(
                        "fatal: `{}' is not permitted in POSIX awk formats",
                        char::from(cs)
                    ));
                }
                b'P' => {
                    // gawk's own "POSIX" flag; under `--posix` it changes
                    // nothing, and a second one ends the conversion.
                    if spec.magic {
                        break;
                    }
                    spec.magic = true;
                }
                b'c' | b's' | b'd' | b'i' | b'o' | b'u' | b'x' | b'X' | b'e' | b'E' | b'f'
                | b'F' | b'g' | b'G' | b'a' | b'A' => {
                    conv = Some(cs);
                    break;
                }
                // Anything else ends the conversion, unconverted: it is
                // copied as written when the next `%` or the end comes.
                _ => break,
            }
        }
        let Some(conv) = conv else {
            continue;
        };
        if spec.negative_prec {
            spec.prec = None;
        }
        let arg = take();
        let body = match conv {
            b'c' => character(&arg, convfmt),
            b's' => {
                let s = arg.to_str(convfmt);
                match spec.prec {
                    Some(p) => take_chars(&s, p),
                    None => s.as_ref().clone(),
                }
            }
            b'd' | b'i' => {
                pad_integer(&mut out, &spec, arg.to_num());
                s0 = i;
                continue;
            }
            b'o' | b'u' | b'x' | b'X' => {
                pad_unsigned(&mut out, &spec, conv, arg.to_num());
                s0 = i;
                continue;
            }
            _ => {
                out.extend_from_slice(&c_float(&spec, conv, arg.to_num()));
                s0 = i;
                continue;
            }
        };
        // `%c` and `%s`: spaces, on whichever side, to the width in characters.
        pad_text(&mut out, &spec, &body);
        s0 = i;
    }
    out.extend_from_slice(fmt.get(s0..).unwrap_or_default());
    Ok(out)
}

/// Format one number by a one-conversion format string -- what `CONVFMT` and
/// `OFMT` do to a number that is not integral.
///
/// `CONVFMT` is the program's to set, so it can be nonsense; a number has to
/// come out regardless, from a conversion that cannot report anything, so a
/// format this refuses falls back to the default. (gawk stops instead; a
/// conversion of a number to a string is not where this awk will.)
#[must_use]
pub fn sprintf_one_number(fmt: &[u8], n: f64) -> Str {
    sprintf(fmt, &[Value::Num(n)], b"%.6g")
        .unwrap_or_else(|_| sprintf(b"%.6g", &[Value::Num(n)], b"%.6g").unwrap_or_default())
}

/// A width or precision past this is a request to allocate gigabytes from a
/// few characters of format, so it is clamped rather than honoured.
const MAX_FIELD: usize = 1 << 20;

fn clamp(n: u64) -> usize {
    usize::try_from(n).unwrap_or(usize::MAX).min(MAX_FIELD)
}

/// What the digits being read set: the width, then (after `.`) the
/// precision, then nothing -- gawk's `cur`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Cur {
    Width,
    Prec,
    Done,
}

#[derive(Default, Clone, Copy)]
struct Spec {
    left: bool,
    /// `+` or a space, the one that wins.
    sign: Option<u8>,
    alt: bool,
    zero: bool,
    width: usize,
    prec: Option<usize>,
    /// `%.-`: the precision is negative, and so absent.
    negative_prec: bool,
    /// gawk's `P` flag.
    magic: bool,
}

/// Read a run of digits starting with `first` into the width or the
/// precision, returning where they end.
fn digits(fmt: &[u8], mut i: usize, first: u8, spec: &mut Spec, cur: &mut Cur) -> usize {
    let mut n = u64::from(first.wrapping_sub(b'0'));
    while let Some(d) = fmt.get(i).filter(|b| b.is_ascii_digit()) {
        n = n
            .saturating_mul(10)
            .saturating_add(u64::from(d.wrapping_sub(b'0')));
        i = i.saturating_add(1);
    }
    match *cur {
        Cur::Width => spec.width = clamp(n),
        Cur::Prec => {
            if !spec.negative_prec {
                spec.prec = Some(clamp(n));
            }
            *cur = Cur::Done;
        }
        Cur::Done => {}
    }
    i
}

/// `%c` and `%s`'s padding: spaces, counted in characters.
fn pad_text(out: &mut Str, spec: &Spec, body: &[u8]) {
    let len = ch::chars(body).count();
    let fill = spec.width.saturating_sub(len);
    if !spec.left {
        out.extend(std::iter::repeat_n(b' ', fill));
    }
    out.extend_from_slice(body);
    if spec.left {
        out.extend(std::iter::repeat_n(b' ', fill));
    }
}

/// Lay out a number's characters -- an optional sign, then `digits` -- in the
/// field, as gawk's integer paths and `pr_tail` do: zeros fill after the sign
/// when `zero_fill`, spaces before it otherwise, and nothing fills on the
/// left of a left-justified field.
fn lay_out(
    out: &mut Str,
    spec: &Spec,
    sign: Option<u8>,
    prefix: &[u8],
    digits: &[u8],
    zero_fill: bool,
) {
    let len = usize::from(sign.is_some())
        .saturating_add(prefix.len())
        .saturating_add(digits.len());
    let fill = spec.width.saturating_sub(len);
    if spec.left {
        out.extend(sign);
        out.extend_from_slice(prefix);
        out.extend_from_slice(digits);
        out.extend(std::iter::repeat_n(b' ', fill));
    } else if zero_fill {
        out.extend(sign);
        out.extend_from_slice(prefix);
        out.extend(std::iter::repeat_n(b'0', fill));
        out.extend_from_slice(digits);
    } else {
        out.extend(std::iter::repeat_n(b' ', fill));
        out.extend(sign);
        out.extend_from_slice(prefix);
        out.extend_from_slice(digits);
    }
}

/// When gawk fills an integer with zeros: never left-justified; with the `0`
/// flag and no precision (`%06d`), or a precision and no width (`%.10d`, where
/// the width becomes the precision).
fn integer_zero_fill(spec: &Spec) -> bool {
    !spec.left && ((spec.zero && spec.prec.is_none()) || (spec.width == 0 && spec.prec.is_some()))
}

/// `%d` and `%i`: the truncated value's every digit, through `%.0f` as gawk
/// does -- so a value past the range of a `long` still prints exactly -- and
/// an infinity or NaN through `%g`.
fn pad_integer(out: &mut Str, spec: &Spec, v: f64) {
    if !v.is_finite() {
        out.extend_from_slice(&c_float(spec, b'g', v));
        return;
    }
    let t = v.trunc();
    // "The result of converting a zero value with a precision of zero is no
    // characters" -- but the width is still filled.
    if spec.prec == Some(0) && t == 0.0 {
        lay_out(out, spec, None, b"", b"", false);
        return;
    }
    let mut digits = format!("{:.0}", t.abs()).into_bytes();
    if let Some(p) = spec.prec {
        while digits.len() < p {
            digits.insert(0, b'0');
        }
    }
    let sign = if t < 0.0 { Some(b'-') } else { spec.sign };
    lay_out(out, spec, sign, b"", &digits, integer_zero_fill(spec));
}

/// `%o`, `%u`, `%x` and `%X`: the value as a 64-bit integer -- a negative one
/// as its two's complement -- and one that is not representable so, or not
/// finite, through `%g`.
fn pad_unsigned(out: &mut Str, spec: &Spec, conv: u8, v: f64) {
    let Some(u) = as_u64(v) else {
        out.extend_from_slice(&c_float(spec, b'g', v));
        return;
    };
    // The precision-zero rule looks at the value gawk has not yet truncated,
    // and `#` keeps a zero (C's `%#.0o` is `0`).
    if !spec.alt && spec.prec == Some(0) && v == 0.0 {
        lay_out(out, spec, None, b"", b"", false);
        return;
    }
    let base: u64 = match conv {
        b'o' => 8,
        b'u' => 10,
        _ => 16,
    };
    let mut digits = to_radix(u, base, conv == b'X');
    if let Some(p) = spec.prec {
        while digits.len() < p {
            digits.insert(0, b'0');
        }
    }
    let mut prefix: &[u8] = b"";
    if spec.alt && v != 0.0 {
        match conv {
            b'x' => prefix = b"0x",
            b'X' => prefix = b"0X",
            // gawk prepends the octal `0` whatever the digits already are.
            b'o' => digits.insert(0, b'0'),
            _ => {}
        }
    }
    lay_out(out, spec, None, prefix, &digits, integer_zero_fill(spec));
}

/// gawk's conversion of a number for `%o %u %x %X`: `(uintmax_t)` of a
/// non-negative value, `(uintmax_t)(intmax_t)` of a negative one, refused when
/// the integer does not give back the truncated value.
fn as_u64(v: f64) -> Option<u64> {
    if !v.is_finite() {
        return None;
    }
    let t = v.trunc();
    // 2^64 and 2^63, exactly representable.
    const TWO_64: f64 = 18_446_744_073_709_551_616.0;
    const TWO_63: f64 = 9_223_372_036_854_775_808.0;
    if t >= 0.0 {
        if t >= TWO_64 {
            return None;
        }
        // In range by the test above, so the cast truncates and nothing else.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let u = t as u64;
        Some(u)
    } else {
        if t < -TWO_63 {
            return None;
        }
        #[allow(clippy::cast_possible_truncation)]
        let i = t as i64;
        #[allow(clippy::cast_sign_loss)]
        let u = i as u64;
        Some(u)
    }
}

fn to_radix(mut n: u64, base: u64, upper: bool) -> Str {
    let digits: &[u8] = if upper {
        b"0123456789ABCDEF"
    } else {
        b"0123456789abcdef"
    };
    let mut out = Str::new();
    loop {
        // `base` is 8, 10 or 16, so neither can fail; `checked_*` says so
        // without an arithmetic lint exemption.
        let d = usize::try_from(n.checked_rem(base).unwrap_or(0)).unwrap_or(0);
        out.push(digits.get(d).copied().unwrap_or(b'0'));
        n = n.checked_div(base).unwrap_or(0);
        if n == 0 {
            break;
        }
    }
    out.reverse();
    out
}

/// `%e %E %f %F %g %G %a %A` exactly as glibc's `printf` writes them, flags,
/// width and all.
fn c_float(spec: &Spec, conv: u8, v: f64) -> Str {
    let upper = conv.is_ascii_uppercase();
    let sign = if v.is_sign_negative() {
        Some(b'-')
    } else {
        spec.sign
    };
    if !v.is_finite() {
        // glibc pads an infinity or a NaN with spaces whatever the `0` flag
        // says, and prints a NaN's sign bit.
        let word: &[u8] = match (v.is_nan(), upper) {
            (true, false) => b"nan",
            (true, true) => b"NAN",
            (false, false) => b"inf",
            (false, true) => b"INF",
        };
        let mut out = Str::new();
        lay_out(&mut out, spec, sign, b"", word, false);
        return out;
    }
    let a = v.abs();
    let prec = spec.prec;
    let body = match conv.to_ascii_lowercase() {
        b'f' => fixed(a, prec.unwrap_or(6), spec.alt),
        b'e' => exponential(a, prec.unwrap_or(6), upper, spec.alt),
        b'a' => hexadecimal(a, prec, upper, spec.alt),
        _ => general(a, prec.unwrap_or(6), upper, spec.alt),
    };
    let mut out = Str::new();
    // `0x` stays before the zero padding, as the sign does.
    let (prefix, digits): (&[u8], &[u8]) = if conv.eq_ignore_ascii_case(&b'a') && body.len() >= 2 {
        body.split_at(2)
    } else {
        (b"", &body)
    };
    lay_out(
        &mut out,
        spec,
        sign,
        prefix,
        digits,
        spec.zero && !spec.left,
    );
    out
}

/// `%f` of a non-negative finite value.
fn fixed(a: f64, prec: usize, alt: bool) -> Str {
    let mut s = format!("{a:.prec$}").into_bytes();
    if alt && prec == 0 {
        s.push(b'.');
    }
    s
}

/// `%e` of a non-negative finite value: one digit, the point, `prec` digits,
/// then `e` and a signed exponent of at least two digits.
fn exponential(a: f64, prec: usize, upper: bool, alt: bool) -> Str {
    // Rust writes `1.5e2`; C wants `1.500000e+02`.
    let rust = format!("{a:.prec$e}");
    let (mant, exp) = rust.split_once('e').unwrap_or((rust.as_str(), "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    let mut s = mant.as_bytes().to_vec();
    if alt && prec == 0 {
        s.push(b'.');
    }
    s.push(if upper { b'E' } else { b'e' });
    s.push(if exp < 0 { b'-' } else { b'+' });
    s.extend_from_slice(format!("{:02}", exp.unsigned_abs()).as_bytes());
    s
}

/// `%g`: C's rule. With P the precision (0 meaning 1) and X the exponent the
/// value has once rounded to P significant digits, `%f` with precision P-1-X
/// when P > X >= -4, else `%e` with precision P-1; then, without `#`, the
/// trailing zeros and a trailing point removed.
fn general(a: f64, prec: usize, upper: bool, alt: bool) -> Str {
    let p = prec.max(1);
    let x: i64 = if a == 0.0 {
        0
    } else {
        let probe = format!("{a:.prec$e}", prec = p.saturating_sub(1));
        probe
            .split_once('e')
            .and_then(|(_, e)| e.parse().ok())
            .unwrap_or(0)
    };
    let p_i = i64::try_from(p).unwrap_or(i64::MAX);
    let mut s = if x < p_i && x >= -4 {
        let decimals = usize::try_from(p_i.saturating_sub(1).saturating_sub(x)).unwrap_or(0);
        fixed(a, decimals, alt)
    } else {
        exponential(a, p.saturating_sub(1), upper, alt)
    };
    if !alt {
        s = strip_trailing_zeros(&s);
    }
    s
}

/// Remove the trailing zeros `%g` does not print, and the point if nothing is
/// left after it -- in the mantissa only, never the exponent.
fn strip_trailing_zeros(s: &[u8]) -> Str {
    let cut = s
        .iter()
        .position(|b| *b == b'e' || *b == b'E')
        .unwrap_or(s.len());
    let (mant, exp) = s.split_at(cut);
    if !mant.contains(&b'.') {
        return s.to_vec();
    }
    let mut end = mant.len();
    while end > 0 && mant.get(end.saturating_sub(1)) == Some(&b'0') {
        end = end.saturating_sub(1);
    }
    if end > 0 && mant.get(end.saturating_sub(1)) == Some(&b'.') {
        end = end.saturating_sub(1);
    }
    let mut out = mant.get(..end).unwrap_or_default().to_vec();
    out.extend_from_slice(exp);
    out
}

/// `%a` of a non-negative finite value, as glibc writes it: `0x1.` and the
/// fraction's hex digits for a normal number, `0x0.` for a subnormal one, then
/// `p` and the binary exponent. With no precision the fraction is as long as it
/// must be to be exact; with one it is rounded to that many digits, ties to
/// even, a carry into the leading digit bumping it (`0x2.0p+0`), as glibc does.
fn hexadecimal(a: f64, prec: Option<usize>, upper: bool, alt: bool) -> Str {
    let bits = a.to_bits();
    let raw_exp = (bits >> 52) & 0x7ff;
    let frac = bits & ((1u64 << 52) - 1);
    let (mut lead, exp): (u64, i64) = if a == 0.0 {
        (0, 0)
    } else if raw_exp == 0 {
        // Subnormal: 0.frac * 2^-1022.
        (0, -1022)
    } else {
        (1, i64::try_from(raw_exp).unwrap_or(0).saturating_sub(1023))
    };
    // The fraction as 13 hex digits, most significant first.
    let mut digits: Vec<u8> = (0..13)
        .rev()
        .map(|k: u32| {
            u8::try_from(frac.checked_shr(k.saturating_mul(4)).unwrap_or(0) & 0xf).unwrap_or(0)
        })
        .collect();
    match prec {
        None => {
            while digits.last() == Some(&0) {
                digits.pop();
            }
        }
        Some(p) if p < digits.len() => {
            // Round at digit `p`: up if what is cut off is more than half a
            // unit, or exactly half and the kept part is odd.
            let rest = digits.split_off(p);
            let first = rest.first().copied().unwrap_or(0);
            let beyond = rest.iter().skip(1).any(|d| *d != 0);
            let kept_odd = digits.last().map_or(lead & 1 == 1, |d| d & 1 == 1);
            if first > 8 || (first == 8 && (beyond || kept_odd)) {
                let mut carry = true;
                for d in digits.iter_mut().rev() {
                    if !carry {
                        break;
                    }
                    if *d == 15 {
                        *d = 0;
                    } else {
                        *d = d.saturating_add(1);
                        carry = false;
                    }
                }
                if carry {
                    lead = lead.saturating_add(1);
                }
            }
        }
        Some(p) => digits.resize(p, 0),
    }
    let hex = |d: u8| -> u8 {
        let table: &[u8] = if upper {
            b"0123456789ABCDEF"
        } else {
            b"0123456789abcdef"
        };
        table.get(usize::from(d)).copied().unwrap_or(b'0')
    };
    let mut s: Str = if upper {
        b"0X".to_vec()
    } else {
        b"0x".to_vec()
    };
    s.push(hex(u8::try_from(lead).unwrap_or(0)));
    if !digits.is_empty() || alt {
        s.push(b'.');
    }
    s.extend(digits.iter().map(|d| hex(*d)));
    s.push(if upper { b'P' } else { b'p' });
    s.push(if exp < 0 { b'-' } else { b'+' });
    s.extend_from_slice(exp.unsigned_abs().to_string().as_bytes());
    s
}

/// `%c`: one character.
///
/// A string argument contributes its first character -- or, when it is empty,
/// the NUL that ends it in C, which gawk copies (`printf "%c", ""` writes one
/// zero byte). A number is a code point, as glibc's `wcrtomb` writes it in a
/// UTF-8 locale: the value as a 32-bit `wchar_t`, in the original UTF-8 of up
/// to six bytes; a negative one or a surrogate cannot be written so, and gawk
/// then writes the value's low byte. The split matters -- `printf "%c", 65` is
/// `A` but `printf "%c", "65"` is `6` -- and a strnum from input is a number.
fn character(v: &Value, convfmt: &[u8]) -> Str {
    if let Value::Str(s) = v {
        return ch::chars(s).next().map_or_else(|| vec![0], ch::Ch::to_str);
    }
    let _ = convfmt;
    // gawk's `get_number_uj`, then `(wchar_t)`: the low 32 bits, signed.
    let code = as_u64(v.to_num()).unwrap_or(0);
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let wc = code as u32 as i32;
    wcrtomb(wc).unwrap_or_else(|| vec![u8::try_from(code & 0xff).unwrap_or(0)])
}

/// glibc's `wcrtomb` in a UTF-8 locale: the original UTF-8, past U+10FFFF up
/// to 2^31, refusing a negative value and a surrogate.
fn wcrtomb(wc: i32) -> Option<Str> {
    let c = u32::try_from(wc).ok()?;
    if (0xd800..=0xdfff).contains(&c) {
        return None;
    }
    let (len, lead): (u32, u32) = match c {
        0..=0x7f => return Some(vec![u8::try_from(c).unwrap_or(0)]),
        0x80..=0x7ff => (2, 0xc0),
        0x800..=0xffff => (3, 0xe0),
        0x1_0000..=0x1f_ffff => (4, 0xf0),
        0x20_0000..=0x3ff_ffff => (5, 0xf8),
        _ => (6, 0xfc),
    };
    let mut out = vec![0u8; usize::try_from(len).unwrap_or(0)];
    let mut rest = c;
    for b in out.iter_mut().skip(1).rev() {
        *b = u8::try_from(0x80 | (rest & 0x3f)).unwrap_or(0);
        rest >>= 6;
    }
    if let Some(first) = out.first_mut() {
        *first = u8::try_from(lead | rest).unwrap_or(0);
    }
    Some(out)
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
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    // The expected values are glibc's digits, as gawk printed them with
    // `%.17g`, which is the point of them.
    clippy::excessive_precision
)]
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
        // gawk reads the flags as long as the width is still open.
        assert_eq!(f("[%5-d]", &[n(42.0)]), "[42   ]");
        assert_eq!(f("[%+ d]", &[n(42.0)]), "[+42]");
        assert_eq!(f("[% +d]", &[n(42.0)]), "[+42]");
        // `0` after a `*` width is the flag, and `05` the width, as in gawk.
        assert_eq!(f("[%*05d]", &[n(8.0), n(3.0)]), "[00003]");
        assert_eq!(f("[%-05d]", &[n(4.0)]), "[4    ]");
        assert_eq!(f("[%.-3d]", &[n(7.0)]), "[7]");
        assert_eq!(f("[%5.-3d]", &[n(8.0)]), "[    8]");
        assert_eq!(f("[%P5d]", &[n(7.0)]), "[    7]");
    }

    #[test]
    fn a_missing_argument_is_empty_rather_than_an_error() {
        // bwk and mawk do this, and scripts rely on it; gawk stops.
        assert_eq!(f("%s-%s", &[s("a")]), "a-");
        assert_eq!(f("%d", &[]), "0");
        // Extra arguments are dropped, not recycled.
        assert_eq!(f("%s", &[s("a"), s("b")]), "a");
    }

    /// gawk copies what it cannot convert as written, and refuses C's
    /// length modifiers under `--posix`.
    #[test]
    fn an_unknown_conversion_is_printed_as_written() {
        assert_eq!(f("[%k]", &[n(65.0)]), "[%k]");
        assert_eq!(f("[%5k]", &[n(65.0)]), "[%5k]");
        assert_eq!(f("[%-]", &[n(65.0)]), "[%-]");
        assert_eq!(f("[%.]", &[n(65.0)]), "[%.]");
        assert_eq!(f("abc%", &[]), "abc%");
        assert_eq!(f("[%5", &[]), "[%5");
        // The argument a malformed conversion did not take is the next one's.
        assert_eq!(f("%k %d", &[n(7.0)]), "%k 7");
        for m in ["h", "l", "L", "j", "t", "z"] {
            let e = sprintf(format!("%{m}d").as_bytes(), &[n(1.0)], b"%.6g").unwrap_err();
            assert_eq!(
                e,
                format!("fatal: `{m}' is not permitted in POSIX awk formats")
            );
        }
        assert_eq!(
            sprintf(b"%1$d", &[n(1.0)], b"%.6g").unwrap_err(),
            "fatal: `$' is not permitted in awk formats"
        );
    }

    #[test]
    fn the_bases_and_their_alternate_forms() {
        assert_eq!(f("%o", &[n(8.0)]), "10");
        assert_eq!(f("%#o", &[n(8.0)]), "010");
        assert_eq!(f("%x", &[n(255.0)]), "ff");
        assert_eq!(f("%X", &[n(255.0)]), "FF");
        assert_eq!(f("%#x", &[n(255.0)]), "0xff");
        assert_eq!(f("%#08x", &[n(255.0)]), "0x0000ff");
        assert_eq!(f("%.5d", &[n(42.0)]), "00042");
        assert_eq!(f("%x", &[n(-1.0)]), "ffffffffffffffff");
        let zeros = vec![n(0.0); 5];
        assert_eq!(
            f("[%#o] [%#.0o] [%#x] [%.0x] [%#.0x]", &zeros),
            "[0] [0] [0] [] [0]"
        );
        assert_eq!(f("%u", &[n(-1.0)]), "18446744073709551615");
        // Not a 64-bit integer: `%g`, gawk's emergency format.
        assert_eq!(f("%x", &[n(2f64.powi(70))]), "1.18059e+21");
    }

    /// `%d` prints every digit, through `%.0f`, and an infinity through `%g`.
    #[test]
    fn percent_d_prints_any_integer_whole() {
        assert_eq!(f("%d", &[n(2f64.powi(64))]), "18446744073709551616");
        assert_eq!(f("%d", &[n(-(2f64.powi(63)))]), "-9223372036854775808");
        assert_eq!(f("%d", &[n(-0.5)]), "0");
        assert_eq!(
            f("%d %d", &[n(f64::INFINITY), n(f64::NEG_INFINITY)]),
            "inf -inf"
        );
        assert_eq!(f("[%.0d]", &[n(0.0)]), "[]");
        assert_eq!(f("[%3.0d]", &[n(0.0)]), "[   ]");
    }

    #[test]
    fn floating_point_matches_c() {
        assert_eq!(f("%f", &[n(1.5)]), "1.500000");
        assert_eq!(f("%.0f", &[n(1.5)]), "2");
        assert_eq!(f("%.0f", &[n(2.5)]), "2");
        assert_eq!(f("%e", &[n(1500.0)]), "1.500000e+03");
        assert_eq!(f("%e", &[n(0.0)]), "0.000000e+00");
        assert_eq!(f("%.2e", &[n(0.000_015)]), "1.50e-05");
        assert_eq!(f("%g", &[n(100_000.0)]), "100000");
        assert_eq!(f("%g", &[n(1_000_000.0)]), "1e+06");
        assert_eq!(f("%g", &[n(0.0001)]), "0.0001");
        assert_eq!(f("%g", &[n(0.000_01)]), "1e-05");
        assert_eq!(f("%.3g", &[n(1.23456)]), "1.23");
        assert_eq!(f("%#g", &[n(1.0)]), "1.00000");
        assert_eq!(f("%#.0f", &[n(1.0)]), "1.");
        assert_eq!(f("%#.0e", &[n(1.0)]), "1.e+00");
        assert_eq!(f("[%08.2f]", &[n(-1.5)]), "[-0001.50]");
        // Subnormals, which the old layout from `log10` turned into `infe-320`.
        assert_eq!(f("%g", &[n(1e-320)]), "9.99989e-321");
        assert_eq!(f("%.3e", &[n(5e-324)]), "4.941e-324");
        // Digit for digit with glibc, the last one included.
        assert_eq!(
            f("%.17g", &[n(1.790_363_270_599_549_8e-18)]),
            "1.7903632705995498e-18"
        );
        assert_eq!(
            f(
                "%f %F %e",
                &[n(f64::INFINITY), n(f64::NEG_INFINITY), n(f64::NAN)]
            ),
            "inf -INF nan"
        );
        assert_eq!(f("[%05f]", &[n(f64::INFINITY)]), "[  inf]");
    }

    #[test]
    fn hexadecimal_floating_point_is_glibcs() {
        assert_eq!(f("%a", &[n(65.0)]), "0x1.04p+6");
        assert_eq!(f("%A", &[n(65.0)]), "0X1.04P+6");
        assert_eq!(f("%a", &[n(1.0)]), "0x1p+0");
        assert_eq!(f("%a", &[n(0.0)]), "0x0p+0");
        assert_eq!(f("%a", &[n(-0.5)]), "-0x1p-1");
        assert_eq!(f("%.1a", &[n(1.0 + 3.0 / 32.0)]), "0x1.2p+0");
        assert_eq!(f("%a", &[n(5e-324)]), "0x0.0000000000001p-1022");
        assert_eq!(f("%#a", &[n(1.0)]), "0x1.p+0");
    }

    #[test]
    fn a_star_takes_the_width_from_the_arguments() {
        assert_eq!(f("[%*d]", &[n(5.0), n(42.0)]), "[   42]");
        assert_eq!(f("[%-*d]", &[n(5.0), n(42.0)]), "[42   ]");
        // A negative `*` width left-justifies, as in C.
        assert_eq!(f("[%*d]", &[n(-5.0), n(42.0)]), "[42   ]");
        assert_eq!(f("[%.*f]", &[n(2.0), n(1.23456)]), "[1.23]");
        // A negative `*` precision is as if none were given.
        assert_eq!(f("[%.*f]", &[n(-1.0), n(1.5)]), "[1.500000]");
    }

    #[test]
    fn percent_c_takes_a_character_or_a_code() {
        assert_eq!(f("%c", &[n(65.0)]), "A");
        assert_eq!(f("%c", &[s("65")]), "6");
        assert_eq!(f("%c", &[n(9731.0)]), "\u{2603}");
        // The empty string's NUL, as gawk copies it.
        assert_eq!(sprintf(b"[%c]", &[s("")], b"%.6g").unwrap(), b"[\0]");
        // glibc's `wcrtomb`: past U+10FFFF in the old UTF-8; a surrogate or a
        // negative value as its low byte; past 2^32 the low 32 bits.
        assert_eq!(
            sprintf(b"%c", &[n(1_114_112.0)], b"%.6g").unwrap(),
            [0xf4, 0x90, 0x80, 0x80]
        );
        assert_eq!(
            sprintf(b"%c", &[n(2_147_483_647.0)], b"%.6g").unwrap(),
            [0xfd, 0xbf, 0xbf, 0xbf, 0xbf, 0xbf]
        );
        assert_eq!(sprintf(b"%c", &[n(57_343.0)], b"%.6g").unwrap(), [0xff]);
        assert_eq!(sprintf(b"%c", &[n(-1.0)], b"%.6g").unwrap(), [0xff]);
        assert_eq!(sprintf(b"%c", &[n(2_147_483_648.0)], b"%.6g").unwrap(), [0]);
        assert_eq!(f("%c", &[n(4_294_967_361.0)]), "A");
        assert_eq!(f("%%", &[]), "%");
        assert_eq!(f("[%5%]", &[]), "[%]");
    }

    #[test]
    fn a_byte_that_is_not_text_survives_percent_s() {
        let v = Value::str(vec![0xff, b'a']);
        assert_eq!(sprintf(b"%s", &[v], b"%.6g").unwrap(), vec![0xff, b'a']);
    }

    #[test]
    fn a_literal_percent_needs_no_argument() {
        assert_eq!(f("100%%", &[]), "100%");
    }
}
