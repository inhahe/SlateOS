//! util-linux's `lib/strutils.c` number parsing, ported: what every util-linux
//! program does with a numeric argument, and the words it uses to refuse one.
//!
//! Extracted from `coreutils/src/bin/cal.rs` on 2026-09-26, where the port was
//! first written, when `logger` needed the same conversions -- its `-S`
//! (`strtosize_or_err`) and `--id=` (`strtoul_or_err`). The rules are
//! upstream's and are easy to get subtly wrong, which is why they live once:
//!
//! * [`ul_strtou64`] rejects a negative number by first converting it signed
//!   -- `-0` passes, `-1` is a RANGE error, not an invalid one.
//! * [`parse_size`] reads `strtoumax(str, &end, 0)`, so `010` is eight and
//!   `0x10` sixteen, and accepts a fraction only before a suffix: `1.5K` is
//!   1536, `1.5` is refused. [`parse_size_res`] is the same call as a
//!   caller that ignores its status sees it (`lscpu`'s cache sizes).
//! * The two message shapes differ, and the difference is observable (see
//!   [`num_error_message`] and [`size_error_message`]).
//! * [`strtotimeval`] is `strtold` then two truncations toward zero, so
//!   `0.0000001` is a zero timeout and `-1.5` is `{-1, -500000}`.
//!
//! The same file's other string helpers live here too, as the programs
//! ported on top of them need them: [`size_to_human_string`] (every size a
//! util-linux table prints), [`string_add_to_idarray`] (every `-o` column
//! list), and [`isdigit_string`]. So does glibc's [`strverscmp`], the order
//! `scandir(..., versionsort)` gives the sysfs directories util-linux reads.
//!
//! And the parsers util-linux's table programs lean on: glibc's [`strtod`]
//! (with its `errno`, which libsmartcols' `width=` property reads),
//! [`ul_optstr_next`] (a `name=value,...` options string, as column
//! properties are given) and [`parse_range`] (`N-M`), and `optutils.h`'s
//! [`err_exclusive_options`].
//!
//! Pure functions over bytes; no I/O. Callers wrap the messages in their own
//! error types, because each program reports through its own diagnostic path.

mod matching;
mod optstr;
mod optutils;
mod strtod;

pub use matching::match_fstype;
pub use optstr::{OptstrInvalid, OptstrItem, parse_range, ul_optstr_next};
pub use optutils::err_exclusive_options;
pub use strtod::{Strtod, strtod};

use quoting::escaped_in_quotes_os;
use std::ffi::OsStr;

/// What went wrong in a C string-to-number conversion, which is `errno` and is
/// observable: `ERANGE` reaches the user through `err()` and so carries
/// `: Numerical result out of range`, while `EINVAL` reaches it through
/// `errx()` and carries nothing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NumErr {
    /// `EINVAL`: empty, no digits, or bytes left over.
    Invalid,
    /// `ERANGE`: the value does not fit, or is out of the caller's bounds.
    Range,
}

impl NumErr {
    /// `strerror` for the two values that get here.
    #[must_use]
    pub fn strerror(self) -> &'static str {
        match self {
            NumErr::Invalid => "Invalid argument",
            NumErr::Range => "Numerical result out of range",
        }
    }
}

/// C's `isspace` in the C locale.
#[must_use]
pub fn c_isspace(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// The sign, magnitude and end index of a C integer conversion.
///
/// `magnitude` saturates rather than wrapping, and `saturated` says which
/// happened, because that is the difference between a value and `ERANGE`.
pub struct Scanned {
    /// A leading `-` was read.
    pub negative: bool,
    /// The digits' value, saturating (see `saturated`).
    pub magnitude: u128,
    /// The value outgrew anything a caller converts to -- C's `ERANGE`.
    pub saturated: bool,
    /// Index one past the last digit — C's `endptr`.
    pub end: usize,
}

/// The digit-scanning half of `strtoimax`/`strtoumax`, shared by both.
///
/// `base` is 10 or 0; 0 means C's "guess from the prefix" rule, which
/// [`parse_size`] relies on and which is why `cal -c 010` is eight columns.
/// Returns `None` for "no conversion performed", where C leaves `endptr` equal
/// to the input.
#[must_use]
#[allow(
    clippy::arithmetic_side_effects,
    reason = "indices step through `s`; the magnitude stops growing once past 2^100, so `magnitude * radix + digit` stays far inside u128"
)]
pub fn scan_integer(s: &[u8], base: u32) -> Option<Scanned> {
    let mut i = 0usize;
    while s.get(i).copied().is_some_and(c_isspace) {
        i += 1;
    }
    let mut negative = false;
    if let Some(&c) = s.get(i)
        && (c == b'+' || c == b'-')
    {
        negative = c == b'-';
        i += 1;
    }

    let mut radix = base;
    if radix == 0 {
        if s.get(i) == Some(&b'0') {
            match s.get(i + 1) {
                Some(&b'x' | &b'X') if s.get(i + 2).is_some_and(u8::is_ascii_hexdigit) => {
                    radix = 16;
                    i += 2;
                }
                _ => radix = 8,
            }
        } else {
            radix = 10;
        }
    } else if radix == 16
        && s.get(i) == Some(&b'0')
        && matches!(s.get(i + 1), Some(&b'x' | &b'X'))
        && s.get(i + 2).is_some_and(u8::is_ascii_hexdigit)
    {
        // As in base 0: `0x` is a prefix only when a hex digit follows it.
        // `0xg` is the number 0 ending before the `x`, as glibc reads it.
        i += 2;
    }

    let digits_start = i;
    // Above `SATURATE` the value cannot be represented in any type this program
    // converts to, so accumulation stops and only the flag matters.
    const SATURATE: u128 = 1 << 100;
    let mut magnitude: u128 = 0;
    let mut saturated = false;
    while let Some(&c) = s.get(i) {
        let Some(d) = (c as char).to_digit(radix) else {
            break;
        };
        if !saturated {
            magnitude = magnitude * u128::from(radix) + u128::from(d);
            if magnitude > SATURATE {
                saturated = true;
            }
        }
        i += 1;
    }
    if i == digits_start {
        return None;
    }
    Some(Scanned {
        negative,
        magnitude,
        saturated,
        end: i,
    })
}

/// `ul_strtos64(str, &num, base)` from `lib/strutils.c`.
///
/// The three refusals are C's, in C's order: an empty string is `EINVAL`;
/// `strtoimax` overflowing is `ERANGE`; and anything left over after the digits
/// — including nothing having been converted at all — is `EINVAL`.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "`i64::MAX.unsigned_abs() + 1` is 2^63 in u128, and negating a magnitude of at most 2^63 in i128 cannot overflow"
)]
pub fn ul_strtos64(s: &[u8], base: u32) -> Result<i64, NumErr> {
    if s.is_empty() {
        return Err(NumErr::Invalid);
    }
    let Some(sc) = scan_integer(s, base) else {
        return Err(NumErr::Invalid);
    };
    let limit = if sc.negative {
        u128::from(i64::MAX.unsigned_abs()) + 1
    } else {
        u128::from(i64::MAX.unsigned_abs())
    };
    if sc.saturated || sc.magnitude > limit {
        return Err(NumErr::Range);
    }
    if sc.end != s.len() {
        return Err(NumErr::Invalid);
    }
    // `magnitude` is at most `i64::MAX + 1`, so both branches are exact in
    // `i128` and only the negative one can reach `i64::MIN`.
    let magnitude = i128::try_from(sc.magnitude).map_err(|_| NumErr::Range)?;
    let value = if sc.negative { -magnitude } else { magnitude };
    i64::try_from(value).map_err(|_| NumErr::Range)
}

/// `ul_strtos32(str, &num, base)`: [`ul_strtos64`], then a range check to
/// `int32_t`, which is `ERANGE` like an overflow.
///
/// # Errors
///
/// As [`ul_strtos64`], and [`NumErr::Range`] outside `i32`.
pub fn ul_strtos32(s: &[u8], base: u32) -> Result<i32, NumErr> {
    let wide = ul_strtos64(s, base)?;
    i32::try_from(wide).map_err(|_| NumErr::Range)
}

/// A `struct timeval` as util-linux's `strtotimeval_or_err` builds one:
/// `strtold`, then `tv_sec = (time_t) x` and `tv_usec = (suseconds_t)((x -
/// tv_sec) * 1000000)` -- two truncations toward zero, so `1.9999999` is
/// `{1, 999999}` and `-1.5` is `{-1, -500000}`.
///
/// `Ok(None)` is a number `time_t` cannot hold -- infinite, NaN, or at least
/// 2^63 in size -- whose conversion C leaves undefined and x86-64 turns into
/// `INT64_MIN`: a negative time, which every timer refuses. Callers report it
/// as that refusal.
///
/// Decimal input is converted exactly, digit by digit, where upstream goes
/// through an 80-bit `long double`; the two differ only for an input of more
/// than about 18 significant digits that falls within a rounding error of a
/// microsecond boundary. Hexadecimal input (`0x1p-3`) goes through `f64`.
///
/// # Errors
///
/// [`NumErr::Invalid`] for what `strtold` cannot read whole -- nothing,
/// leading junk, anything left over; [`NumErr::Range`] for what it reads
/// with `ERANGE`, a magnitude outside `long double`'s range.
pub fn strtotimeval(s: &[u8]) -> Result<Option<(i64, i64)>, NumErr> {
    match strtold(s)? {
        LongDouble::NotFinite => Ok(None),
        LongDouble::Hex(v) => Ok(timeval_of_f64(v)),
        LongDouble::Decimal {
            negative,
            digits,
            point,
        } => Ok(timeval_of_decimal(negative, &digits, point)),
    }
}

/// What `strtold` read: kept exact for a decimal number, so the conversion
/// to a `timeval` can truncate exactly.
enum LongDouble {
    /// Infinity or NaN.
    NotFinite,
    /// A hexadecimal float, as `f64`.
    Hex(f64),
    /// `±0.DIGITS × 10^point`: the significant digits with no leading zero,
    /// and where the decimal point falls among them. No digits is zero.
    Decimal {
        negative: bool,
        digits: Vec<u8>,
        point: i64,
    },
}

/// `long double`'s range as a decimal exponent: `LDBL_MAX` is about
/// 1.19e4932, and below about 3.6e-4951 even a subnormal is zero.
const LDBL_MAX_10_EXP: i64 = 4932;
const LDBL_TRUE_MIN_10_EXP: i64 = -4950;

/// `strtold(s, &end)`, with `strtold_or_err`'s refusals: nothing read, or
/// anything left after what was read, or `ERANGE`.
fn strtold(s: &[u8]) -> Result<LongDouble, NumErr> {
    let mut i = s.iter().take_while(|&&b| c_isspace(b)).count();
    let mut negative = false;
    match s.get(i) {
        Some(b'-') => {
            negative = true;
            i = i.saturating_add(1);
        }
        Some(b'+') => i = i.saturating_add(1),
        _ => {}
    }
    let rest = s.get(i..).unwrap_or_default();
    let lower: Vec<u8> = rest.iter().map(u8::to_ascii_lowercase).collect();
    if lower == b"inf" || lower == b"infinity" {
        return Ok(LongDouble::NotFinite);
    }
    if let Some(after) = lower.strip_prefix(b"nan") {
        // `nan` or `nan(n-char-sequence)`; anything else after it is left over.
        let ok = after.is_empty()
            || (after.first() == Some(&b'(')
                && after.last() == Some(&b')')
                && after
                    .get(1..after.len().saturating_sub(1))
                    .is_some_and(|m| m.iter().all(|&c| c.is_ascii_alphanumeric() || c == b'_')));
        return if ok {
            Ok(LongDouble::NotFinite)
        } else {
            Err(NumErr::Invalid)
        };
    }
    if (lower.starts_with(b"0x"))
        && lower.get(2).is_some_and(|&c| {
            c.is_ascii_hexdigit() || (c == b'.' && lower.get(3).is_some_and(u8::is_ascii_hexdigit))
        })
    {
        return hex_float(negative, lower.get(2..).unwrap_or_default());
    }
    decimal(negative, rest)
}

/// The decimal form: digits with at most one `.`, at least one digit, then
/// an exponent only if a digit follows its `e` and sign.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "the exponent saturates at ±10^15, far past long double's range, and every sum below is of two such bounded values"
)]
fn decimal(negative: bool, s: &[u8]) -> Result<LongDouble, NumErr> {
    let mut mantissa: Vec<u8> = Vec::new();
    let mut point: Option<usize> = None;
    let mut i = 0usize;
    let mut any_digit = false;
    while let Some(&c) = s.get(i) {
        if c.is_ascii_digit() {
            any_digit = true;
            mantissa.push(c - b'0');
        } else if c == b'.' && point.is_none() {
            point = Some(mantissa.len());
        } else {
            break;
        }
        i += 1;
    }
    if !any_digit {
        return Err(NumErr::Invalid);
    }
    let mut exponent: i64 = 0;
    if matches!(s.get(i), Some(b'e' | b'E')) {
        let mut j = i + 1;
        let mut exp_negative = false;
        match s.get(j) {
            Some(b'-') => {
                exp_negative = true;
                j += 1;
            }
            Some(b'+') => j += 1,
            _ => {}
        }
        if s.get(j).is_some_and(u8::is_ascii_digit) {
            while let Some(&c) = s.get(j) {
                if !c.is_ascii_digit() {
                    break;
                }
                exponent = (exponent * 10 + i64::from(c - b'0')).min(1_000_000_000_000_000);
                j += 1;
            }
            if exp_negative {
                exponent = -exponent;
            }
            i = j;
        }
    }
    if i != s.len() {
        return Err(NumErr::Invalid);
    }
    // `0.DIGITS × 10^point`, leading zeros taken off the digits and into
    // the point.
    let int_len = i64::try_from(point.unwrap_or(mantissa.len())).unwrap_or(i64::MAX);
    let lead = mantissa.iter().take_while(|&&d| d == 0).count();
    let digits: Vec<u8> = mantissa.get(lead..).unwrap_or_default().to_vec();
    let point = int_len - i64::try_from(lead).unwrap_or(0) + exponent;
    if !digits.is_empty() {
        if point - 1 > LDBL_MAX_10_EXP {
            return Err(NumErr::Range);
        }
        if point - 1 < LDBL_TRUE_MIN_10_EXP {
            return Err(NumErr::Range);
        }
    }
    Ok(LongDouble::Decimal {
        negative,
        digits,
        point,
    })
}

/// The hexadecimal form, after its `0x`: hex digits with at most one `.`,
/// then a binary exponent only if a digit follows its `p` and sign.
#[allow(
    clippy::arithmetic_side_effects,
    clippy::cast_precision_loss,
    reason = "the value is built in f64, as its approximation is documented to be; the exponent saturates far past f64's range"
)]
fn hex_float(negative: bool, s: &[u8]) -> Result<LongDouble, NumErr> {
    let mut value = 0f64;
    let mut scale: i64 = 0;
    let mut seen_point = false;
    let mut i = 0usize;
    while let Some(&c) = s.get(i) {
        if let Some(d) = char::from(c).to_digit(16) {
            value = value * 16.0 + f64::from(d);
            if seen_point {
                scale -= 4;
            }
        } else if c == b'.' && !seen_point {
            seen_point = true;
        } else {
            break;
        }
        i += 1;
    }
    let mut exponent: i64 = 0;
    if s.get(i) == Some(&b'p') {
        let mut j = i + 1;
        let mut exp_negative = false;
        match s.get(j) {
            Some(b'-') => {
                exp_negative = true;
                j += 1;
            }
            Some(b'+') => j += 1,
            _ => {}
        }
        if s.get(j).is_some_and(u8::is_ascii_digit) {
            while let Some(&c) = s.get(j) {
                if !c.is_ascii_digit() {
                    break;
                }
                exponent = (exponent * 10 + i64::from(c - b'0')).min(1_000_000);
                j += 1;
            }
            if exp_negative {
                exponent = -exponent;
            }
            i = j;
        }
    }
    if i != s.len() {
        return Err(NumErr::Invalid);
    }
    let power = i32::try_from((scale + exponent).clamp(-100_000, 100_000)).unwrap_or(0);
    let magnitude = value * 2f64.powi(power);
    if !magnitude.is_finite() {
        return Err(NumErr::Range);
    }
    Ok(LongDouble::Hex(if negative {
        -magnitude
    } else {
        magnitude
    }))
}

/// `{(time_t) x, (suseconds_t)((x - tv_sec) * 1e6)}` for a decimal `x`,
/// exactly: the integer digits, and the first six after the point.
fn timeval_of_decimal(negative: bool, digits: &[u8], point: i64) -> Option<(i64, i64)> {
    let digit_at = |k: i64| -> u8 {
        // The digit worth 10^(point - 1 - k); zero outside `digits`.
        usize::try_from(k)
            .ok()
            .and_then(|k| digits.get(k))
            .copied()
            .unwrap_or(0)
    };
    let mut sec: i64 = 0;
    for k in 0..point.max(0) {
        sec = sec.checked_mul(10)?.checked_add(i64::from(digit_at(k)))?;
    }
    let mut usec: i64 = 0;
    for k in point..point.saturating_add(6) {
        // Six digits cannot overflow an i64.
        usec = usec
            .saturating_mul(10)
            .saturating_add(i64::from(digit_at(k)));
    }
    if negative {
        // `usec` is at most 999 999, so its negation is exact.
        Some((sec.checked_neg()?, usec.saturating_neg()))
    } else {
        Some((sec, usec))
    }
}

/// The same conversion for a hexadecimal float, through `f64`.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    reason = "both values are range-checked first, and truncation toward zero is the conversion being reproduced"
)]
fn timeval_of_f64(v: f64) -> Option<(i64, i64)> {
    // 2^63: at or beyond it, `(time_t) x` is undefined.
    const LIMIT: f64 = 9_223_372_036_854_775_808.0;
    if !v.is_finite() || v.abs() >= LIMIT {
        return None;
    }
    let sec = v.trunc();
    Some((sec as i64, ((v - sec) * 1_000_000.0).trunc() as i64))
}

/// `ul_strtou64(str, &num, base)`.
///
/// The odd shape is upstream's: it runs `strtoimax` first purely to reject a
/// leading `-`, then *clears* `errno` and re-runs `strtoumax`, which is why
/// `-n 10000000000000000000` is a range error from the bound check rather than
/// from the first conversion.
pub fn ul_strtou64(s: &[u8], base: u32) -> Result<u64, NumErr> {
    if s.is_empty() {
        return Err(NumErr::Invalid);
    }
    let Some(sc) = scan_integer(s, base) else {
        return Err(NumErr::Invalid);
    };
    if sc.negative && (sc.saturated || sc.magnitude != 0) {
        return Err(NumErr::Range);
    }
    if sc.saturated || sc.magnitude > u128::from(u64::MAX) {
        return Err(NumErr::Range);
    }
    if sc.end != s.len() {
        return Err(NumErr::Invalid);
    }
    u64::try_from(sc.magnitude).map_err(|_| NumErr::Range)
}

/// `do_scale_by_power`: multiply by `base` `power` times, refusing to wrap.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "`base` is 1000 or 1024, never 0, and the product is taken only after `u64::MAX / base >= *x` proved it fits"
)]
fn do_scale_by_power(x: &mut u64, base: u64, power: i32) -> Result<(), NumErr> {
    for _ in 0..power {
        if u64::MAX / base < *x {
            return Err(NumErr::Range);
        }
        *x *= base;
    }
    Ok(())
}

/// `parse_size` from `lib/strutils.c`, fractions and all.
///
/// `cal -c` and `logger -S` read their sizes with it: `1.5K` is 1536 there,
/// where a plain integer parse would refuse it.
/// The decimal-point branch is the whole reason this is 60 lines rather than 6.
///
/// Note the base: the leading conversion is `strtoumax(str, &end, 0)`, so `010`
/// is eight and `0x10` is sixteen.
///
/// # Errors
///
/// As upstream: [`NumErr::Invalid`] for no number, a sign, or a suffix it
/// does not know; [`NumErr::Range`] for a number or a scaled size past
/// `UINTMAX_MAX`.
pub fn parse_size(s: &[u8]) -> Result<u64, NumErr> {
    parse_size_res(s).0
}

/// `parse_size(str, &res, NULL)` as a caller that ignores its status sees
/// it: the status, and what `*res` holds afterwards. That is 0 after most
/// failures, but after a size whose scaling overflowed it is the number as
/// far as it was scaled -- upstream falls through to `*res = x` with the
/// error still to return -- so `lscpu`, which ignores the status, shows a
/// cache of `16E` as 16 PiB.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "indices step through `s` and are read with `get`; the fraction arithmetic is upstream's, each multiply guarded by a `<= u64::MAX / 10` test and each divisor proved non-zero first"
)]
pub fn parse_size_res(s: &[u8]) -> (Result<u64, NumErr>, u64) {
    let fail = |e: NumErr| (Err(e), 0u64);
    if s.is_empty() {
        return fail(NumErr::Invalid);
    }

    // Only positive numbers are acceptable. The check is on the first
    // non-blank byte, while the conversion below still starts at the front.
    let mut lead = 0usize;
    while s.get(lead).copied().is_some_and(c_isspace) {
        lead += 1;
    }
    if s.get(lead) == Some(&b'-') {
        return fail(NumErr::Invalid);
    }

    let Some(sc) = scan_integer(s, 0) else {
        return fail(NumErr::Invalid);
    };
    if sc.saturated || sc.magnitude > u128::from(u64::MAX) {
        return fail(NumErr::Range);
    }
    let Ok(mut x) = u64::try_from(sc.magnitude) else {
        return fail(NumErr::Range);
    };
    let mut p = sc.end;
    if p >= s.len() {
        return (Ok(x), x); // without suffix
    }

    let mut base: u64 = 1024;
    let mut frac: u64 = 0;
    let mut frac_zeros = 0i32;

    // `check_suffix:`, which the decimal-point branch jumps back to.
    let at = |i: usize| -> u8 { s.get(i).copied().unwrap_or(0) };
    loop {
        if at(p + 1) == b'i' && (at(p + 2) == b'B' || at(p + 2) == b'b') && at(p + 3) == 0 {
            base = 1024; // XiB, 2^N
        } else if (at(p + 1) == b'B' || at(p + 1) == b'b') && at(p + 2) == 0 {
            base = 1000; // XB, 10^N
        } else if at(p + 1) != 0 {
            // The C locale's decimal point is `.` and is one byte long.
            if frac != 0 || at(p) != b'.' {
                return fail(NumErr::Invalid); // unexpected suffix
            }
            let mut fstr = p + 1;
            while at(fstr) == b'0' {
                frac_zeros += 1;
                fstr += 1;
            }
            let end = if at(fstr).is_ascii_digit() {
                let Some(fsc) = scan_integer(s.get(fstr..).unwrap_or_default(), 0) else {
                    return fail(NumErr::Invalid);
                };
                if fsc.saturated || fsc.magnitude > u128::from(u64::MAX) {
                    return fail(NumErr::Range);
                }
                let Ok(f) = u64::try_from(fsc.magnitude) else {
                    return fail(NumErr::Range);
                };
                frac = f;
                fstr + fsc.end
            } else {
                fstr
            };
            if frac != 0 && end >= s.len() {
                return fail(NumErr::Invalid); // a fraction with no suffix
            }
            p = end;
            continue;
        }
        break;
    }

    const SUF: &[u8] = b"KMGTPEZY";
    const SUF2: &[u8] = b"kmgtpezy";
    let here = at(p);
    let pwr = if let Some(i) = SUF.iter().position(|&c| c == here && c != 0) {
        i32::try_from(i).unwrap_or(0) + 1
    } else if let Some(i) = SUF2.iter().position(|&c| c == here && c != 0) {
        i32::try_from(i).unwrap_or(0) + 1
    } else {
        return fail(NumErr::Invalid);
    };

    let scaled = do_scale_by_power(&mut x, base, pwr);

    if frac != 0 && pwr != 0 {
        let mut frac_div: u64 = 10;
        let mut frac_poz: u64 = 1;
        let mut frac_base: u64 = 1;
        // Its overflow is discarded upstream, and so is it here.
        let _ = do_scale_by_power(&mut frac_base, base, pwr);

        // The divisor for the last digit: 100 for 0.05, 1000 for 0.054.
        while frac_div < frac {
            if frac_div <= u64::MAX / 10 {
                frac_div *= 10;
            } else {
                frac /= 10;
            }
        }
        for _ in 0..frac_zeros {
            if frac_div <= u64::MAX / 10 {
                frac_div *= 10;
            } else {
                frac /= 10;
            }
        }

        // Walk the fraction backwards from its last digit, adding what each
        // digit is worth in `frac_base` -- in `uintmax_t`, which wraps.
        loop {
            let seg = frac % 10;
            let seg_div = frac_div / frac_poz;
            frac /= 10;
            frac_poz = frac_poz.saturating_mul(10);
            if seg != 0 && seg_div / seg != 0 {
                x = x.wrapping_add(frac_base / (seg_div / seg));
            }
            if frac == 0 {
                break;
            }
        }
    }

    // `parse_size` writes the (possibly overflowed) result out and *then*
    // returns the error, so a caller that ignored the status would still see a
    // number. `strtosize_or_err` does not ignore it.
    (scaled.map(|()| x), x)
}

/// The argument is inside util-linux's own `'%s'`, escaped where it is not
/// printable (`quoting::escaped_in_quotes`): byte for byte upstream's for any
/// printable text, `it's` included.
///
/// The message `str2num_or_err` gives -- and so `strtos32_or_err`,
/// `strtou32_or_err`, `strtoul_or_err` and the rest -- for a failed
/// conversion of `arg`, without the program-name prefix.
///
/// Its `err:` label chooses between `err()` and `errx()` on
/// `errno == ERANGE`, so only the range case carries a `strerror`:
/// `failed to parse id: 'x'` but `failed to parse id: '9999…': Numerical
/// result out of range`.
#[must_use]
pub fn num_error_message(errmesg: &str, arg: &OsStr, e: NumErr) -> String {
    match e {
        NumErr::Invalid => format!("{errmesg}: {}", escaped_in_quotes_os(arg)),
        NumErr::Range => format!("{errmesg}: {}: {}", escaped_in_quotes_os(arg), e.strerror()),
    }
}

/// The message `strtosize_or_err` gives for a failed conversion of `arg`.
///
/// Its test is `if (errno) err(...)`, and `parse_size` always sets one, so
/// BOTH failures carry a `strerror` here -- `: Invalid argument` included,
/// where [`num_error_message`] says nothing.
#[must_use]
pub fn size_error_message(errmesg: &str, arg: &OsStr, e: NumErr) -> String {
    format!("{errmesg}: {}: {}", escaped_in_quotes_os(arg), e.strerror())
}

/// `isdigit_string(str)`: one or more ASCII digits and nothing else.
#[must_use]
pub fn isdigit_string(s: &[u8]) -> bool {
    !s.is_empty() && s.iter().all(u8::is_ascii_digit)
}

/// `SIZE_SUFFIX_1LETTER`: `B`, `K`, `M`, ... straight after the number.
pub const SIZE_SUFFIX_1LETTER: u32 = 0;
/// `SIZE_SUFFIX_3LETTER`: `KiB`, `MiB`, ... -- but still `B` for bytes.
pub const SIZE_SUFFIX_3LETTER: u32 = 1 << 0;
/// `SIZE_SUFFIX_SPACE`: a space between the number and its unit.
pub const SIZE_SUFFIX_SPACE: u32 = 1 << 1;
/// `SIZE_DECIMAL_2DIGITS`: two digits after the point rather than one.
pub const SIZE_DECIMAL_2DIGITS: u32 = 1 << 2;

/// `get_exp(n)`: the multiple of ten, at most 60, that is the power of two of
/// the unit `n` is shown in.
fn get_exp(n: u64) -> u32 {
    const BELOW: [(u64, u32); 6] = [
        (1 << 10, 0),
        (1 << 20, 10),
        (1 << 30, 20),
        (1 << 40, 30),
        (1 << 50, 40),
        (1 << 60, 50),
    ];
    BELOW
        .iter()
        .find(|&&(limit, _)| n < limit)
        .map_or(60, |&(_, exp)| exp)
}

/// `size_to_human_string(options, bytes)` from `lib/strutils.c`: a size in
/// the largest binary unit below it, as `lsblk`, `lsmem` and the rest show
/// sizes.
///
/// Upstream's rounding, which is not a round-to-nearest of the whole value:
/// the fraction is first cut to thousandths, then rounded half-up to one
/// digit (two with [`SIZE_DECIMAL_2DIGITS`]); a trailing zero is dropped, so
/// one digit or none shows; and a fraction that rounds up to a whole unit
/// carries into the number without moving to the next unit -- 1023.96 KiB is
/// `1024K`, not `1M`. The number is `%d` of the quotient: no thousands
/// separators.
///
/// The decimal point is the C locale's `.`. Upstream asks `localeconv()`,
/// and SlateOS's locales are C and C.UTF-8, whose point it is.
#[must_use]
#[allow(
    clippy::arithmetic_side_effects,
    reason = "exp is at most 60, so every shift is in range; frac < 2^exp, and the multiplications are guarded as upstream guards them"
)]
pub fn size_to_human_string(options: u32, bytes: u64) -> String {
    let mut suffix = String::new();
    if options & SIZE_SUFFIX_SPACE != 0 {
        suffix.push(' ');
    }
    let exp = get_exp(bytes);
    let unit = b"BKMGTPE"
        .get(usize::try_from(exp / 10).unwrap_or(0))
        .copied()
        .unwrap_or(b'B');
    let (mut dec, mut frac) = if exp == 0 {
        (bytes, 0)
    } else {
        (bytes >> exp, bytes & ((1u64 << exp) - 1))
    };
    suffix.push(char::from(unit));
    if options & SIZE_SUFFIX_3LETTER != 0 && unit != b'B' {
        suffix.push_str("iB");
    }

    if frac != 0 {
        // Three digits after the point.
        if frac >= u64::MAX / 1000 {
            frac = ((frac / 1024) * 1000) / (1u64 << (exp - 10));
        } else {
            frac = (frac * 1000) / (1u64 << exp);
        }
        if options & SIZE_DECIMAL_2DIGITS != 0 {
            frac = (frac + 5) / 10;
        } else {
            frac = ((frac + 50) / 100) * 10;
        }
        // Rounding could have overflowed.
        if frac == 100 {
            dec += 1;
            frac = 0;
        }
    }

    if frac == 0 {
        format!("{dec}{suffix}")
    } else {
        let mut text = format!("{dec}.{frac:02}");
        if text.ends_with('0') {
            text.pop();
        }
        text.push_str(&suffix);
        text
    }
}

/// Why a list of names was refused: `string_to_idarray`'s negative returns,
/// which the programs calling it tell apart only by exiting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdListError {
    /// `-1`: an empty list, an empty name, or one `name2id` did not know.
    Invalid,
    /// `-2`: more names than the array has room for.
    Full,
}

/// `string_to_idarray(list, ary, arysz, name2id)`: the comma-separated names
/// in `list`, each turned into an id by `name2id` and pushed onto `ary`,
/// which may take at most `arysz` of them. The number pushed, or why not.
///
/// Upstream's scan, kept exactly because its edges show: an empty name is
/// refused (`RANGE,,SIZE`) silently, but a trailing comma is not an empty
/// name -- it is part of the last one, which `name2id` then does not know
/// (`SIZE,`), and a lone `,` is a name of its own. The room is checked
/// before each byte, so a list one name too long is [`IdListError::Full`]
/// even when that name is unknown.
///
/// `name2id` is handed the name and, as upstream's is, everything from the
/// start of the name to the end of the list: util-linux's `unknown column`
/// warnings print that rest, not the name alone.
///
/// # Errors
///
/// [`IdListError::Invalid`] for an empty list or name or an unknown one,
/// [`IdListError::Full`] when `ary` would outgrow `arysz`.
pub fn string_to_idarray<T>(
    list: &[u8],
    ary: &mut Vec<T>,
    arysz: usize,
    mut name2id: impl FnMut(&[u8], &[u8]) -> Option<T>,
) -> Result<usize, IdListError> {
    if list.is_empty() || arysz == 0 {
        return Err(IdListError::Invalid);
    }
    let mut begin: Option<usize> = None;
    let mut n = 0usize;
    for (p, &byte) in list.iter().enumerate() {
        if n >= arysz {
            return Err(IdListError::Full);
        }
        let start = *begin.get_or_insert(p);
        let at_end = p.saturating_add(1) == list.len();
        let end = if at_end {
            p.saturating_add(1)
        } else if byte == b',' {
            p
        } else {
            continue;
        };
        if end <= start {
            return Err(IdListError::Invalid);
        }
        let name = list.get(start..end).unwrap_or_default();
        let rest = list.get(start..).unwrap_or_default();
        ary.push(name2id(name, rest).ok_or(IdListError::Invalid)?);
        n = n.saturating_add(1);
        begin = None;
        if at_end {
            break;
        }
    }
    Ok(n)
}

/// `string_add_to_idarray(list, ary, arysz, &ary_pos, name2id)`: as
/// [`string_to_idarray`], but a list that starts with `+` is added to what
/// `ary` already holds instead of replacing it -- `-o +NODE`. `ary.len()` is
/// upstream's `*ary_pos`, and like it is left where it was when the list is
/// refused.
///
/// # Errors
///
/// As [`string_to_idarray`]; also [`IdListError::Invalid`] for an empty
/// list, or `+` alone.
pub fn string_add_to_idarray<T>(
    list: &[u8],
    ary: &mut Vec<T>,
    arysz: usize,
    name2id: impl FnMut(&[u8], &[u8]) -> Option<T>,
) -> Result<usize, IdListError> {
    if list.is_empty() || ary.len() > arysz {
        return Err(IdListError::Invalid);
    }
    let add = match list.strip_prefix(b"+") {
        Some(rest) => rest,
        None => {
            ary.clear();
            list
        }
    };
    let pos = ary.len();
    let room = arysz.saturating_sub(pos);
    let added = string_to_idarray(add, ary, room, name2id);
    if added.is_err() {
        ary.truncate(pos);
    }
    added
}

/// glibc's `strverscmp(s1, s2)`: the order of names holding version numbers
/// or indices, which is `versionsort`'s and so the order util-linux's
/// `scandir` calls list `memory0`, `memory1`, ..., `memory10`.
///
/// glibc 2.39's state machine, transcribed: runs of digits compare as
/// numbers, but a run with leading zeros is a fraction and sorts before a
/// run without (`000 < 00 < 01 < 010 < 09 < 0 < 1 < 9 < 10`). The end of a
/// slice is C's terminating NUL, so neither may hold one.
#[must_use]
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "state is below 12 and every class below 3, so both tables are indexed in range, and a difference of two bytes fits i32; the tests walk every reachable pair"
)]
pub fn strverscmp(s1: &[u8], s2: &[u8]) -> std::cmp::Ordering {
    // States: normal, comparing an integral part, a fractional part, and a
    // fractional part of leading zeros only.
    const S_N: usize = 0;
    const S_I: usize = 3;
    const S_F: usize = 6;
    const S_Z: usize = 9;
    // Result types: return the byte difference, or compare the runs' lengths.
    const CMP: i8 = 2;
    const LEN: i8 = 3;
    #[rustfmt::skip]
    const NEXT_STATE: [usize; 12] = [
        /* S_N */ S_N, S_I, S_Z,
        /* S_I */ S_N, S_I, S_I,
        /* S_F */ S_N, S_F, S_F,
        /* S_Z */ S_N, S_F, S_Z,
    ];
    #[rustfmt::skip]
    const RESULT_TYPE: [i8; 36] = [
        /* S_N */ CMP, CMP, CMP, CMP, LEN, CMP, CMP, CMP, CMP,
        /* S_I */ CMP, -1, -1, 1, LEN, LEN, 1, LEN, LEN,
        /* S_F */ CMP, CMP, CMP, CMP, CMP, CMP, CMP, CMP, CMP,
        /* S_Z */ CMP, 1, 1, -1, CMP, CMP, -1, CMP, CMP,
    ];
    // The class a byte adds to a state: 0 other, 1 a digit 1-9, 2 a zero.
    fn class(c: u8) -> usize {
        match c {
            b'0' => 2,
            b'1'..=b'9' => 1,
            _ => 0,
        }
    }
    let at = |s: &[u8], i: usize| s.get(i).copied().unwrap_or(0);

    let (mut i1, mut i2) = (1usize, 1usize);
    let mut c1 = at(s1, 0);
    let mut c2 = at(s2, 0);
    let mut state = S_N + class(c1);
    let diff = loop {
        let diff = i32::from(c1) - i32::from(c2);
        if diff != 0 {
            break diff;
        }
        if c1 == 0 {
            return std::cmp::Ordering::Equal;
        }
        state = NEXT_STATE[state];
        c1 = at(s1, i1);
        c2 = at(s2, i2);
        i1 = i1.saturating_add(1);
        i2 = i2.saturating_add(1);
        state += class(c1);
    };

    let result = match RESULT_TYPE[state * 3 + class(c2)] {
        CMP => diff,
        LEN => loop {
            let d1 = at(s1, i1);
            i1 = i1.saturating_add(1);
            if !d1.is_ascii_digit() {
                break if at(s2, i2).is_ascii_digit() {
                    -1
                } else {
                    diff
                };
            }
            let d2 = at(s2, i2);
            i2 = i2.saturating_add(1);
            if !d2.is_ascii_digit() {
                break 1;
            }
        },
        fixed => i32::from(fixed),
    };
    result.cmp(&0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_are_read_the_way_strtosize_reads_them() {
        assert_eq!(parse_size(b"3"), Ok(3));
        assert_eq!(parse_size(b"+3"), Ok(3));
        assert_eq!(parse_size(b" 5"), Ok(5));
        assert_eq!(parse_size(b"010"), Ok(8));
        assert_eq!(parse_size(b"0x10"), Ok(16));
        assert_eq!(parse_size(b"1.5K"), Ok(1536));
        assert_eq!(parse_size(b"0.5K"), Ok(512));
        assert_eq!(parse_size(b"1kiB"), Ok(1024));
        assert_eq!(parse_size(b"5KB"), Ok(5000));
        assert_eq!(parse_size(b"1EiB"), Ok(1 << 60));
        // A fraction with no suffix to divide into, and a bare `B`, are both
        // rejected -- the fraction because there is nothing to scale.
        assert_eq!(parse_size(b"1.9"), Err(NumErr::Invalid));
        assert_eq!(parse_size(b"1."), Err(NumErr::Invalid));
        assert_eq!(parse_size(b".5K"), Err(NumErr::Invalid));
        assert_eq!(parse_size(b"5B"), Err(NumErr::Invalid));
        assert_eq!(parse_size(b"5b"), Err(NumErr::Invalid));
        assert_eq!(parse_size(b"1x"), Err(NumErr::Invalid));
        assert_eq!(parse_size(b"abc"), Err(NumErr::Invalid));
        assert_eq!(parse_size(b"-1"), Err(NumErr::Invalid));
        // `Y` overflows 64 bits, and so does one past `u64::MAX`.
        assert_eq!(parse_size(b"1Y"), Err(NumErr::Range));
        assert_eq!(parse_size(b"18446744073709551616"), Err(NumErr::Range));
    }

    #[test]
    fn a_size_that_overflows_while_scaling_is_left_part_scaled() {
        // 16 * 1024^5 fits; a sixth 1024 does not, and `*res` keeps 16P.
        assert_eq!(parse_size_res(b"16E"), (Err(NumErr::Range), 16 << 50));
        assert_eq!(parse_size_res(b"1Y"), (Err(NumErr::Range), 1 << 60));
        // Failures before the scaling leave 0.
        assert_eq!(parse_size_res(b"garbage"), (Err(NumErr::Invalid), 0));
        assert_eq!(
            parse_size_res(b"99999999999999999999K"),
            (Err(NumErr::Range), 0)
        );
        assert_eq!(parse_size_res(b"32K"), (Ok(32768), 32768));
        // The fraction is added in `uintmax_t`, which wraps. Its `.9` is a
        // whole unit (`frac_base / (10 / 9)`), so 18.9EB is 19 * 10^18,
        // past 2^64.
        let wrapped = 18_000_000_000_000_000_000u64.wrapping_add(1_000_000_000_000_000_000);
        assert_eq!(parse_size_res(b"18.9EB"), (Ok(wrapped), wrapped));
        assert_eq!(parse_size(b"1.9K"), Ok(2048));
    }

    #[test]
    fn signed_numbers_are_read_the_way_strtos64_reads_them() {
        assert_eq!(ul_strtos64(b"0", 10), Ok(0));
        assert_eq!(ul_strtos64(b"-1", 10), Ok(-1));
        assert_eq!(ul_strtos64(b"2147483647", 10), Ok(2_147_483_647));
        assert_eq!(ul_strtos64(b"007", 10), Ok(7));
        assert_eq!(ul_strtos64(b"", 10), Err(NumErr::Invalid));
        assert_eq!(ul_strtos64(b"2x", 10), Err(NumErr::Invalid));
        assert_eq!(ul_strtos64(b"0x7", 10), Err(NumErr::Invalid));
        assert_eq!(ul_strtos64(b"99999999999999999999", 10), Err(NumErr::Range));
    }

    #[test]
    fn unsigned_numbers_refuse_a_minus_sign_as_out_of_range() {
        assert_eq!(ul_strtou64(b"42", 10), Ok(42));
        assert_eq!(ul_strtou64(b"-0", 10), Ok(0));
        assert_eq!(ul_strtou64(b"-1", 10), Err(NumErr::Range));
        assert_eq!(ul_strtou64(b"18446744073709551615", 10), Ok(u64::MAX));
        assert_eq!(ul_strtou64(b"18446744073709551616", 10), Err(NumErr::Range));
        assert_eq!(ul_strtou64(b"ff", 16), Ok(255));
        assert_eq!(ul_strtou64(b"0xff", 16), Ok(255));
        assert_eq!(ul_strtou64(b"x", 10), Err(NumErr::Invalid));
    }

    #[test]
    fn the_two_message_shapes() {
        let arg = OsStr::new("x");
        assert_eq!(
            num_error_message("failed to parse id", arg, NumErr::Invalid),
            "failed to parse id: 'x'"
        );
        assert_eq!(
            num_error_message("failed to parse id", arg, NumErr::Range),
            "failed to parse id: 'x': Numerical result out of range"
        );
        assert_eq!(
            size_error_message("failed to parse message size", arg, NumErr::Invalid),
            "failed to parse message size: 'x': Invalid argument"
        );
    }

    #[test]
    fn strtos32_is_strtos64_then_a_range_check() {
        assert_eq!(ul_strtos32(b"2147483647", 10), Ok(i32::MAX));
        assert_eq!(ul_strtos32(b"-2147483648", 10), Ok(i32::MIN));
        assert_eq!(ul_strtos32(b"2147483648", 10), Err(NumErr::Range));
        assert_eq!(ul_strtos32(b"x", 10), Err(NumErr::Invalid));
    }

    #[test]
    fn a_timeval_is_two_truncations_toward_zero() {
        assert_eq!(strtotimeval(b"1.5"), Ok(Some((1, 500_000))));
        assert_eq!(strtotimeval(b"1.9999999"), Ok(Some((1, 999_999))));
        assert_eq!(strtotimeval(b"-1.5"), Ok(Some((-1, -500_000))));
        assert_eq!(strtotimeval(b"0.0000001"), Ok(Some((0, 0))));
        assert_eq!(strtotimeval(b"0.3"), Ok(Some((0, 300_000))));
        assert_eq!(strtotimeval(b" +2e1"), Ok(Some((20, 0))));
        assert_eq!(strtotimeval(b"25e-1"), Ok(Some((2, 500_000))));
        assert_eq!(strtotimeval(b".5"), Ok(Some((0, 500_000))));
        assert_eq!(strtotimeval(b"5."), Ok(Some((5, 0))));
        assert_eq!(strtotimeval(b"0x1.8p1"), Ok(Some((3, 0))));
        assert_eq!(strtotimeval(b"-0"), Ok(Some((0, 0))));
    }

    #[test]
    fn what_time_t_cannot_hold_is_none() {
        assert_eq!(strtotimeval(b"inf"), Ok(None));
        assert_eq!(strtotimeval(b"-Infinity"), Ok(None));
        assert_eq!(strtotimeval(b"nan"), Ok(None));
        assert_eq!(strtotimeval(b"NaN(abc)"), Ok(None));
        assert_eq!(strtotimeval(b"1e30"), Ok(None));
        assert_eq!(
            strtotimeval(b"9223372036854775807"),
            Ok(Some((i64::MAX, 0)))
        );
        assert_eq!(strtotimeval(b"9223372036854775808"), Ok(None));
    }

    #[test]
    fn strtold_refuses_what_it_cannot_read_whole() {
        for bad in [
            &b""[..],
            b" ",
            b"abc",
            b"1x",
            b"1e",
            b"1e+",
            b"0x",
            b"1 ",
            b"--1",
            b".",
            b"nanx",
            b"infx",
        ] {
            assert_eq!(strtotimeval(bad), Err(NumErr::Invalid), "{bad:?}");
        }
        // An exponent with no digits is not read, so the `e` is left over.
        assert_eq!(strtotimeval(b"2e"), Err(NumErr::Invalid));
        assert_eq!(strtotimeval(b"1e5000"), Err(NumErr::Range));
        assert_eq!(strtotimeval(b"1e-5000"), Err(NumErr::Range));
        assert_eq!(strtotimeval(b"0e5000"), Ok(Some((0, 0))));
    }

    #[test]
    fn a_refused_argument_is_in_upstreams_own_quotes() {
        assert_eq!(
            num_error_message("invalid exit code", OsStr::new("it's"), NumErr::Invalid),
            "invalid exit code: 'it's'"
        );
        assert_eq!(
            num_error_message("x", OsStr::new("a\nb"), NumErr::Range),
            "x: 'a\\012b': Numerical result out of range"
        );
    }

    #[test]
    fn strtoumax_base_16_reads_0x_as_a_prefix_only_before_a_hex_digit() {
        let at = |s: &[u8]| scan_integer(s, 16).map(|sc| (sc.magnitude, sc.end));
        assert_eq!(at(b"0x1f"), Some((31, 4)));
        assert_eq!(at(b"8000000"), Some((0x800_0000, 7)));
        // `0xg` and `0x` are the number 0, ending before the `x`.
        assert_eq!(at(b"0xg"), Some((0, 1)));
        assert_eq!(at(b"0x"), Some((0, 1)));
        assert_eq!(at(b"zz"), None);
    }

    #[test]
    fn a_digit_string_is_digits_and_nothing_else() {
        assert!(isdigit_string(b"0"));
        assert!(isdigit_string(b"0123"));
        assert!(!isdigit_string(b""));
        assert!(!isdigit_string(b"12a"));
        assert!(!isdigit_string(b"-1"));
        assert!(!isdigit_string(b" 1"));
    }

    #[test]
    fn human_sizes_are_util_linuxs() {
        let h = |b| size_to_human_string(SIZE_SUFFIX_1LETTER, b);
        assert_eq!(h(0), "0B");
        assert_eq!(h(1023), "1023B");
        assert_eq!(h(1024), "1K");
        assert_eq!(h(1536), "1.5K");
        assert_eq!(h(128 << 20), "128M");
        assert_eq!(h(32 << 30), "32G");
        // lsmem's 3.9G: 0xf8000000 bytes is 3.875 GiB, and upstream rounds
        // the thousandths (875) half-up to one digit.
        assert_eq!(h(0xf800_0000), "3.9G");
        assert_eq!(h(0x7_0800_0000), "28.1G");
        // A fraction that rounds to a whole unit carries without a new unit.
        assert_eq!(h((1 << 20) - 1), "1024K");
        assert_eq!(h(u64::MAX), "16E");
        assert_eq!(h(1 << 60), "1E");
        assert_eq!(h((1 << 60) + (1 << 59)), "1.5E");
    }

    #[test]
    fn human_size_options_are_util_linuxs() {
        let three = SIZE_SUFFIX_3LETTER | SIZE_SUFFIX_SPACE;
        assert_eq!(size_to_human_string(three, 512), "512 B");
        assert_eq!(size_to_human_string(three, 1536), "1.5 KiB");
        let two = three | SIZE_DECIMAL_2DIGITS;
        assert_eq!(size_to_human_string(two, 1024 + 51), "1.05 KiB");
        assert_eq!(size_to_human_string(two, 1024 + 512), "1.5 KiB");
        assert_eq!(size_to_human_string(SIZE_SUFFIX_3LETTER, 1024), "1KiB");
    }

    /// `lsmem`'s `column_name_to_id`, for the list tests.
    fn column(name: &[u8], _rest: &[u8]) -> Option<usize> {
        [&b"RANGE"[..], b"SIZE", b"STATE"]
            .iter()
            .position(|c| c.eq_ignore_ascii_case(name))
    }

    #[test]
    fn a_list_of_names_is_split_as_upstream_splits_it() {
        let mut ary = Vec::new();
        assert_eq!(string_to_idarray(b"range,SIZE", &mut ary, 4, column), Ok(2));
        assert_eq!(ary, [0, 1]);
        let mut ary = Vec::new();
        // An empty name is refused; a trailing comma belongs to the name.
        assert_eq!(
            string_to_idarray(b"RANGE,,SIZE", &mut ary, 4, column),
            Err(IdListError::Invalid)
        );
        let mut seen = Vec::new();
        let mut ary = Vec::new();
        let refused = string_to_idarray(b"SIZE,", &mut ary, 4, |name, rest| {
            seen.push((name.to_vec(), rest.to_vec()));
            column(name, rest)
        });
        assert_eq!(refused, Err(IdListError::Invalid));
        assert_eq!(seen, [(b"SIZE,".to_vec(), b"SIZE,".to_vec())]);
        // A lone comma is a name of its own.
        let mut seen = Vec::new();
        let mut ary: Vec<usize> = Vec::new();
        let refused = string_to_idarray(b",", &mut ary, 4, |name, _| {
            seen.push(name.to_vec());
            None
        });
        assert_eq!(refused, Err(IdListError::Invalid));
        assert_eq!(seen, [b",".to_vec()]);
    }

    #[test]
    fn an_unknown_name_is_shown_with_the_rest_of_the_list() {
        let mut rests = Vec::new();
        let mut ary = Vec::new();
        let r = string_to_idarray(b"RANGE,FOO,SIZE", &mut ary, 4, |name, rest| {
            rests.push(rest.to_vec());
            column(name, rest)
        });
        assert_eq!(r, Err(IdListError::Invalid));
        assert_eq!(rests.last(), Some(&b"FOO,SIZE".to_vec()));
    }

    #[test]
    fn room_is_checked_before_each_byte() {
        let mut ary = Vec::new();
        assert_eq!(string_to_idarray(b"RANGE", &mut ary, 1, column), Ok(1));
        let mut ary = Vec::new();
        assert_eq!(
            string_to_idarray(b"RANGE,BOGUS", &mut ary, 1, column),
            Err(IdListError::Full)
        );
        let mut ary = Vec::new();
        assert_eq!(
            string_to_idarray(b"", &mut ary, 1, column),
            Err(IdListError::Invalid)
        );
    }

    #[test]
    fn a_plus_adds_to_the_list_and_anything_else_replaces_it() {
        let mut ary = vec![2];
        assert_eq!(string_add_to_idarray(b"+RANGE", &mut ary, 4, column), Ok(1));
        assert_eq!(ary, [2, 0]);
        assert_eq!(string_add_to_idarray(b"SIZE", &mut ary, 4, column), Ok(1));
        assert_eq!(ary, [1]);
        // Refused, the position stays where it was -- after the reset, when
        // the list was to replace.
        assert_eq!(
            string_add_to_idarray(b"+", &mut ary, 4, column),
            Err(IdListError::Invalid)
        );
        assert_eq!(ary, [1]);
        assert_eq!(
            string_add_to_idarray(b"+RANGE,BOGUS", &mut ary, 4, column),
            Err(IdListError::Invalid)
        );
        assert_eq!(ary, [1]);
        assert_eq!(
            string_add_to_idarray(b"RANGE,BOGUS", &mut ary, 4, column),
            Err(IdListError::Invalid)
        );
        assert!(ary.is_empty());
        // No room left at all is upstream's `!arysz` test, which comes
        // before the scan: refused as invalid, not as full.
        let mut full = vec![0, 1];
        assert_eq!(
            string_add_to_idarray(b"+RANGE", &mut full, 2, column),
            Err(IdListError::Invalid)
        );
        let mut nearly = vec![0];
        assert_eq!(
            string_add_to_idarray(b"+RANGE,SIZE", &mut nearly, 2, column),
            Err(IdListError::Full)
        );
        assert_eq!(nearly, [0]);
    }

    #[test]
    fn version_order_is_glibcs() {
        use std::cmp::Ordering::{Equal, Greater, Less};
        // The glibc manual's own example, in order.
        let ordered: [&[u8]; 9] = [b"000", b"00", b"01", b"010", b"09", b"0", b"1", b"9", b"10"];
        for pair in ordered.windows(2) {
            if let [a, b] = pair {
                assert_eq!(strverscmp(a, b), Less, "{a:?} < {b:?}");
                assert_eq!(strverscmp(b, a), Greater, "{b:?} > {a:?}");
            }
        }
        assert_eq!(strverscmp(b"memory9", b"memory10"), Less);
        assert_eq!(strverscmp(b"memory10", b"memory10"), Equal);
        assert_eq!(strverscmp(b"memory100", b"memory99"), Greater);
        assert_eq!(strverscmp(b"", b""), Equal);
        assert_eq!(strverscmp(b"a", b""), Greater);
        assert_eq!(strverscmp(b"item#99", b"item#100"), Less);
    }

    #[test]
    fn version_order_never_indexes_out_of_its_tables() {
        // Every arrangement of pieces holding each class and the end: every
        // state and class pair the machine can reach is visited, and the
        // order is antisymmetric throughout.
        let pieces: [&[u8]; 6] = [b"", b"0", b"1", b"9", b"a", b"00"];
        for a in pieces {
            for b in pieces {
                for c in pieces {
                    let s1 = [a, b, c].concat();
                    let s2 = [c, a, b].concat();
                    let one = strverscmp(&s1, &s2);
                    let other = strverscmp(&s2, &s1);
                    assert_eq!(one, other.reverse(), "{s1:?} {s2:?}");
                }
            }
        }
    }
}
