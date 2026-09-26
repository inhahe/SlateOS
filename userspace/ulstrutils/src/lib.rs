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
//!   1536, `1.5` is refused.
//! * The two message shapes differ, and the difference is observable (see
//!   [`num_error_message`] and [`size_error_message`]).
//!
//! Pure functions over bytes; no I/O. Callers wrap the messages in their own
//! error types, because each program reports through its own diagnostic path.

use quoting::quoteaf_os;
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
    } else if radix == 16 && s.get(i) == Some(&b'0') && matches!(s.get(i + 1), Some(&b'x' | &b'X'))
    {
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
#[allow(
    clippy::arithmetic_side_effects,
    reason = "indices step through `s` and are read with `get`; the fraction arithmetic is upstream's, each multiply guarded by a `<= u64::MAX / 10` test and each divisor proved non-zero first"
)]
pub fn parse_size(s: &[u8]) -> Result<u64, NumErr> {
    if s.is_empty() {
        return Err(NumErr::Invalid);
    }

    // Only positive numbers are acceptable. The check is on the first
    // non-blank byte, while the conversion below still starts at the front.
    let mut lead = 0usize;
    while s.get(lead).copied().is_some_and(c_isspace) {
        lead += 1;
    }
    if s.get(lead) == Some(&b'-') {
        return Err(NumErr::Invalid);
    }

    let Some(sc) = scan_integer(s, 0) else {
        return Err(NumErr::Invalid);
    };
    if sc.saturated || sc.magnitude > u128::from(u64::MAX) {
        return Err(NumErr::Range);
    }
    let mut x = u64::try_from(sc.magnitude).map_err(|_| NumErr::Range)?;
    let mut p = sc.end;
    if p >= s.len() {
        return Ok(x); // without suffix
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
                return Err(NumErr::Invalid); // unexpected suffix
            }
            let mut fstr = p + 1;
            while at(fstr) == b'0' {
                frac_zeros += 1;
                fstr += 1;
            }
            let end = if at(fstr).is_ascii_digit() {
                let Some(fsc) = scan_integer(s.get(fstr..).unwrap_or_default(), 0) else {
                    return Err(NumErr::Invalid);
                };
                if fsc.saturated || fsc.magnitude > u128::from(u64::MAX) {
                    return Err(NumErr::Range);
                }
                frac = u64::try_from(fsc.magnitude).map_err(|_| NumErr::Range)?;
                fstr + fsc.end
            } else {
                fstr
            };
            if frac != 0 && end >= s.len() {
                return Err(NumErr::Invalid); // a fraction with no suffix
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
        return Err(NumErr::Invalid);
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
        // digit is worth in `frac_base`.
        loop {
            let seg = frac % 10;
            let seg_div = frac_div / frac_poz;
            frac /= 10;
            frac_poz = frac_poz.saturating_mul(10);
            if seg != 0 && seg_div / seg != 0 {
                x = x.saturating_add(frac_base / (seg_div / seg));
            }
            if frac == 0 {
                break;
            }
        }
    }

    // `parse_size` writes the (possibly overflowed) result out and *then*
    // returns the error, so a caller that ignored the status would still see a
    // number. `strtosize_or_err` does not ignore it.
    scaled.map(|()| x)
}

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
        NumErr::Invalid => format!("{errmesg}: {}", quoteaf_os(arg)),
        NumErr::Range => format!("{errmesg}: {}: {}", quoteaf_os(arg), e.strerror()),
    }
}

/// The message `strtosize_or_err` gives for a failed conversion of `arg`.
///
/// Its test is `if (errno) err(...)`, and `parse_size` always sets one, so
/// BOTH failures carry a `strerror` here -- `: Invalid argument` included,
/// where [`num_error_message`] says nothing.
#[must_use]
pub fn size_error_message(errmesg: &str, arg: &OsStr, e: NumErr) -> String {
    format!("{errmesg}: {}: {}", quoteaf_os(arg), e.strerror())
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
}
