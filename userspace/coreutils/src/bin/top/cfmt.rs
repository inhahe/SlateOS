//! C's `snprintf`, as `top` calls it: a format string and its arguments.
//!
//! Most of what `top` prints goes through a `printf` format -- its own, or
//! one of the message tables' (`nls`) -- so the formats are kept as written
//! and interpreted here, each conversion rendered by the shared `cprintf`
//! crate, whose floating point is glibc's to the last digit. The arguments
//! carry the types C's would after the default promotions: a `float` is
//! passed as the `double` it widens to, so a value computed in single
//! precision is rounded where upstream's is.

use std::sync::atomic::{AtomicU8, Ordering};

use cprintf::cfmt::{self, Spec, Value};
use cprintf::extfloat::ExtF80;

/// The locale's decimal point, which `%f` writes for C's `.`.
static RADIX: AtomicU8 = AtomicU8::new(b'.');

/// Use `radix` as the decimal point from now on: `setlocale (LC_ALL, "")`'s
/// `LC_NUMERIC`, as `printf` would see it.
pub fn set_radix(radix: u8) {
    RADIX.store(radix, Ordering::Relaxed);
}

/// One argument.
#[derive(Clone, Copy, Debug)]
pub enum Arg<'a> {
    /// `int`, `long`, `long long`: for `%d`, and for a `*` width or
    /// precision.
    I(i64),
    /// `unsigned` and wider: for `%u`, `%x`, `%lu`.
    U(u64),
    /// `double` (a `float` widened): for `%f`.
    F(f64),
    /// `const char *`: for `%s`.
    S(&'a [u8]),
    /// `int` holding a character: for `%c`.
    C(u8),
}

/// `snprintf (NULL, 0, fmt, args...)`'s text: everything the format writes,
/// before any buffer would cut it. A conversion whose argument is missing or
/// of another type renders as its zero.
#[must_use]
pub fn sprintf(fmt: &[u8], args: &[Arg<'_>]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut args = args.iter();
    let mut i = 0usize;
    while let Some(&c) = fmt.get(i) {
        i = i.saturating_add(1);
        if c != b'%' {
            out.push(c);
            continue;
        }
        if fmt.get(i) == Some(&b'%') {
            out.push(b'%');
            i = i.saturating_add(1);
            continue;
        }
        let mut spec = Spec {
            minus: false,
            plus: false,
            space: false,
            hash: false,
            zero: false,
            width: 0,
            precision: None,
            conv: b'd',
        };
        // Flags.
        while let Some(&f) = fmt.get(i) {
            match f {
                b'-' => spec.minus = true,
                b'+' => spec.plus = true,
                b' ' => spec.space = true,
                b'#' => spec.hash = true,
                b'0' => spec.zero = true,
                _ => break,
            }
            i = i.saturating_add(1);
        }
        // Width.
        if fmt.get(i) == Some(&b'*') {
            i = i.saturating_add(1);
            let w = int_arg(args.next());
            if w < 0 {
                spec.minus = true;
            }
            spec.width = usize::try_from(w.unsigned_abs()).unwrap_or(usize::MAX);
        } else {
            let (w, n) = digits(fmt.get(i..).unwrap_or_default());
            spec.width = w;
            i = i.saturating_add(n);
        }
        // Precision.
        if fmt.get(i) == Some(&b'.') {
            i = i.saturating_add(1);
            if fmt.get(i) == Some(&b'*') {
                i = i.saturating_add(1);
                let p = int_arg(args.next());
                // A negative precision is taken as if it were omitted.
                spec.precision = usize::try_from(p).ok();
            } else {
                let (p, n) = digits(fmt.get(i..).unwrap_or_default());
                spec.precision = Some(p);
                i = i.saturating_add(n);
            }
        }
        // Length modifiers: the argument already has its type.
        while matches!(
            fmt.get(i),
            Some(b'l' | b'h' | b'z' | b'j' | b't' | b'L' | b'q')
        ) {
            i = i.saturating_add(1);
        }
        let Some(&conv) = fmt.get(i) else {
            break;
        };
        i = i.saturating_add(1);
        spec.conv = conv;
        let arg = args.next().copied();
        let value = match conv {
            b'd' | b'i' => Value::Signed(match arg {
                Some(Arg::I(v)) => v,
                Some(Arg::U(v)) => i64::from_le_bytes(v.to_le_bytes()),
                Some(Arg::C(c)) => i64::from(c),
                _ => 0,
            }),
            b'u' | b'o' | b'x' | b'X' => Value::Unsigned(match arg {
                Some(Arg::U(v)) => v,
                Some(Arg::I(v)) => u64::from_le_bytes(v.to_le_bytes()),
                Some(Arg::C(c)) => u64::from(c),
                _ => 0,
            }),
            b'c' => Value::Byte(match arg {
                Some(Arg::C(c)) => c,
                Some(Arg::I(v)) => v.to_le_bytes()[0],
                Some(Arg::U(v)) => v.to_le_bytes()[0],
                _ => 0,
            }),
            b's' => Value::Text(match arg {
                Some(Arg::S(s)) => s,
                _ => b"(null)",
            }),
            b'f' | b'F' | b'e' | b'E' | b'g' | b'G' | b'a' | b'A' => {
                Value::Float(ExtF80::from_f64(match arg {
                    Some(Arg::F(v)) => v,
                    _ => 0.0,
                }))
            }
            // Not a conversion C knows: printed as written, as glibc does.
            _ => {
                out.push(b'%');
                out.push(conv);
                continue;
            }
        };
        let float = matches!(value, Value::Float(_));
        let text = cfmt::render(&spec, value);
        let radix = RADIX.load(Ordering::Relaxed);
        if float && radix != b'.' {
            out.extend(text.iter().map(|&b| if b == b'.' { radix } else { b }));
        } else {
            out.extend_from_slice(&text);
        }
    }
    out
}

/// `snprintf (buf, size, …)`: the text cut to `size - 1` bytes, and the
/// length the whole would have had (its return value).
#[must_use]
pub fn snprintf(size: usize, fmt: &[u8], args: &[Arg<'_>]) -> (Vec<u8>, usize) {
    let mut text = sprintf(fmt, args);
    let full = text.len();
    text.truncate(size.saturating_sub(1));
    (text, full)
}

/// A `*` width or precision's argument.
fn int_arg(arg: Option<&Arg<'_>>) -> i64 {
    match arg {
        Some(Arg::I(v)) => *v,
        Some(Arg::U(v)) => i64::from_le_bytes(v.to_le_bytes()),
        _ => 0,
    }
}

/// A run of decimal digits: its value and its length.
fn digits(s: &[u8]) -> (usize, usize) {
    let n = s.iter().take_while(|b| b.is_ascii_digit()).count();
    let v = s
        .get(..n)
        .unwrap_or_default()
        .iter()
        .fold(0usize, |acc, &d| {
            acc.saturating_mul(10)
                .saturating_add(usize::from(d.wrapping_sub(b'0')))
        });
    (v, n)
}

/// `sprintf` over a format and arguments written inline.
#[macro_export]
macro_rules! cf {
    ($fmt:expr $(, $arg:expr)* $(,)?) => {
        $crate::cfmt::sprintf(AsRef::<[u8]>::as_ref($fmt), &[$($arg),*])
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(fmt: &str, args: &[Arg<'_>]) -> String {
        String::from_utf8(sprintf(fmt.as_bytes(), args)).unwrap_or_default()
    }

    #[test]
    fn the_conversions_top_uses() {
        assert_eq!(
            s("%5d|%-5d|%ld", &[Arg::I(42), Arg::I(-7), Arg::I(9)]),
            "   42|-7   |9"
        );
        assert_eq!(
            s(
                "%*.*s|%-*.*s",
                &[
                    Arg::I(5),
                    Arg::I(2),
                    Arg::S(b"abcd"),
                    Arg::I(4),
                    Arg::I(3),
                    Arg::S(b"xyz1")
                ]
            ),
            "   ab|xyz "
        );
        assert_eq!(
            s(
                "%#5.1f|%-#5.1f|%#.1f",
                &[Arg::F(3.0), Arg::F(2.25), Arg::F(0.0)]
            ),
            "  3.0|2.2  |0.0"
        );
        assert_eq!(
            s(
                "%.0f|%*.0f|%3.0f",
                &[Arg::F(2.5), Arg::I(4), Arg::F(99.5), Arg::F(0.4)]
            ),
            "2| 100|  0"
        );
        assert_eq!(
            s("%08lx|%%|%c", &[Arg::U(0xabc), Arg::C(b'R')]),
            "00000abc|%|R"
        );
        assert_eq!(s("%%Cp%c%-3d:", &[Arg::C(b'u'), Arg::I(1)]), "%Cpu1  :");
        assert_eq!(
            s(
                "%9.9s|%.*s",
                &[Arg::S(b"1234567890"), Arg::I(-1), Arg::S(b"all")]
            ),
            "123456789|all"
        );
    }

    #[test]
    fn a_cut_buffer_says_how_long_the_whole_was() {
        assert_eq!(snprintf(4, b"%d", &[Arg::I(123_456)]), (b"123".to_vec(), 6));
    }
}
