//! C's `strtol`, `strtoul` and `strtoull`, over bytes, in the C locale.
//!
//! A port that reads its numbers the way its upstream does has to read them
//! with these functions' rules, not Rust's `parse`: leading white space is
//! skipped, a sign is allowed, base 0 takes `0x` as hexadecimal and a leading
//! `0` as octal, the number ends at the first byte that is not a digit --
//! which is not an error -- and an overflow saturates rather than failing.
//! Above all, a minus sign negates even an *unsigned* result, so
//! `strtoul ("-1")` is `ULONG_MAX`.
//!
//! `long` and `unsigned long` are 64 bits, as on the LP64 systems every
//! upstream here is measured on.
//!
//! This was `libmagic`'s `cstd` until 2026-10-08, when `xxd` needed the same
//! functions at base 0 and `hexdump` at base 16; `libmagic::cstd` re-exports
//! it. `coreutils::procps::scanf` (procps' readers, at bases 10 and 16) and
//! `autoopts` (option values and quoted strings) read through it too, in
//! place of the private copies each had.

/// `isspace` in the C locale: the six ASCII spaces.
#[must_use]
pub fn isspace(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// What the scan found: the magnitude it read (saturated on overflow),
/// whether a `-` was written, how many bytes it consumed (0 when it read no
/// number at all), and whether it overflowed (`ERANGE`).
struct Scanned {
    magnitude: u64,
    negative: bool,
    used: usize,
    overflow: bool,
}

/// glibc's scan for the `strto*l` family at `base` (0 for C's prefixes):
/// leading white space, a sign, `0x` (only when a hex digit follows, else the
/// `0` alone is the number), then digits.
fn scan(s: &[u8], base: u32) -> Scanned {
    let mut i = 0usize;
    while s.get(i).copied().is_some_and(isspace) {
        i = i.saturating_add(1);
    }
    let negative = s.get(i) == Some(&b'-');
    if matches!(s.get(i), Some(b'+' | b'-')) {
        i = i.saturating_add(1);
    }
    let mut base = base;
    let digit_at = |j: usize, b: u32| s.get(j).and_then(|&c| char::from(c).to_digit(b));
    if (base == 0 || base == 16)
        && s.get(i) == Some(&b'0')
        && matches!(s.get(i.saturating_add(1)), Some(b'x' | b'X'))
        && digit_at(i.saturating_add(2), 16).is_some()
    {
        i = i.saturating_add(2);
        base = 16;
    } else if base == 0 {
        base = if s.get(i) == Some(&b'0') { 8 } else { 10 };
    }
    let start = i;
    let mut magnitude: u64 = 0;
    let mut overflow = false;
    while let Some(d) = digit_at(i, base) {
        match magnitude
            .checked_mul(u64::from(base))
            .and_then(|m| m.checked_add(u64::from(d)))
        {
            Some(m) => magnitude = m,
            None => overflow = true,
        }
        i = i.saturating_add(1);
    }
    if i == start {
        return Scanned {
            magnitude: 0,
            negative: false,
            used: 0,
            overflow: false,
        };
    }
    Scanned {
        magnitude: if overflow { u64::MAX } else { magnitude },
        negative,
        used: i,
        overflow,
    }
}

/// C's `strtoull (s, &end, base)`: the value, the bytes used (0 if none), and
/// whether it overflowed. A `-` negates the result modulo 2^64, as C does;
/// an overflow is `ULONG_LONG_MAX` whatever the sign.
#[must_use]
pub fn strtoull(s: &[u8], base: u32) -> (u64, usize, bool) {
    let r = scan(s, base);
    if r.overflow {
        return (u64::MAX, r.used, true);
    }
    let v = if r.negative {
        r.magnitude.wrapping_neg()
    } else {
        r.magnitude
    };
    (v, r.used, false)
}

/// C's `strtoul (s, &end, base)`: [`strtoull`], since `unsigned long` is 64
/// bits.
#[must_use]
pub fn strtoul(s: &[u8], base: u32) -> (u64, usize) {
    let (v, used, _) = strtoull(s, base);
    (v, used)
}

/// C's `strtol (s, &end, base)` for a 64-bit `long`: saturated at
/// `LONG_MIN`/`LONG_MAX` on overflow.
#[must_use]
pub fn strtol(s: &[u8], base: u32) -> (i64, usize) {
    let (v, used, _) = strtol_overflow(s, base);
    (v, used)
}

/// [`strtol`], and whether it overflowed -- the one case in which glibc's
/// sets `errno` (`ERANGE`).
#[must_use]
pub fn strtol_overflow(s: &[u8], base: u32) -> (i64, usize, bool) {
    let r = scan(s, base);
    if r.used == 0 {
        return (0, 0, false);
    }
    let (v, overflow) = if r.negative {
        if r.overflow || r.magnitude > 1u64 << 63 {
            (i64::MIN, true)
        } else {
            // In range: `0 - magnitude` for a magnitude up to 2^63.
            (0i64.wrapping_sub_unsigned(r.magnitude), false)
        }
    } else if r.overflow || r.magnitude >= 1u64 << 63 {
        (i64::MAX, true)
    } else {
        (i64::try_from(r.magnitude).unwrap_or(i64::MAX), false)
    };
    (v, r.used, overflow)
}

/// C's conversion of a `long` to `int`: the low 32 bits, two's complement --
/// what `(int) strtol (...)` stores.
#[must_use]
pub fn low_i32(v: i64) -> i32 {
    let b = v.to_le_bytes();
    i32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strtol_reads_c_prefixes_and_saturates() {
        assert_eq!(strtol(b"010", 0), (8, 3));
        assert_eq!(strtol(b"0x10z", 0), (16, 4));
        assert_eq!(strtol(b"0xz", 0), (0, 1));
        assert_eq!(strtol(b"  -12", 0), (-12, 5));
        assert_eq!(strtol(b"x", 0), (0, 0));
        assert_eq!(strtol(b"", 0), (0, 0));
        assert_eq!(strtol(b"-", 0), (0, 0));
        assert_eq!(strtol(b"99999999999999999999", 0).0, i64::MAX);
        assert_eq!(strtol(b"-99999999999999999999", 0).0, i64::MIN);
        assert_eq!(strtol(b"-9223372036854775808", 0), (i64::MIN, 20));
        assert_eq!(strtol(b"9223372036854775807", 0).0, i64::MAX);
        assert_eq!(
            strtol_overflow(b"9223372036854775808", 0),
            (i64::MAX, 19, true)
        );
        assert_eq!(
            strtol_overflow(b"-9223372036854775808", 0),
            (i64::MIN, 20, false)
        );
        assert_eq!(low_i32(strtol(b"0xffffffff", 0).0), -1);
        assert_eq!(low_i32(strtol(b"4294967312", 0).0), 16);
    }

    #[test]
    fn strtoull_negates_and_overflows_as_c_does() {
        assert_eq!(strtoull(b"-1", 0), (u64::MAX, 2, false));
        assert_eq!(strtoull(b"0x8000000000000000", 0).0, 1u64 << 63);
        assert_eq!(strtoull(b"0x1ffffffffffffffff", 0), (u64::MAX, 19, true));
        assert_eq!(strtoull(b"-0x1ffffffffffffffff", 0), (u64::MAX, 20, true));
        assert_eq!(strtoull(b"017", 0).0, 15);
        assert_eq!(strtoull(b"8", 8), (0, 0, false));
        assert_eq!(strtoull(b"09", 0), (0, 1, false));
    }

    #[test]
    fn a_base_other_than_zero() {
        assert_eq!(strtoul(b"ff", 16), (255, 2));
        assert_eq!(strtoul(b"0xff", 16), (255, 4));
        assert_eq!(strtoul(b"0x", 16), (0, 1));
        assert_eq!(strtoul(b"0x10", 10), (0, 1));
        assert_eq!(strtoul(b"\t\n+7", 10), (7, 4));
        assert_eq!(strtoul(b"z", 36), (35, 1));
    }

    #[test]
    fn the_c_locale_spaces() {
        for c in [b' ', b'\t', b'\n', 0x0b, 0x0c, b'\r'] {
            assert!(isspace(c));
        }
        assert!(!isspace(0xa0));
        assert!(!isspace(0));
    }
}
