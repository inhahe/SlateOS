//! The C library behaviour libmagic leans on, in the C locale: `<ctype.h>`'s
//! classes for one byte, and the `strto*` family's parse -- prefixes,
//! signs, overflow and all -- which decides what a magic rule's numbers are.
//!
//! libmagic parses its database with `strtol`, `strtoul` and `strtoull` at
//! base 0, so `010` is eight, `0x10` sixteen, an overflow saturates, and a
//! minus sign negates even an unsigned result. A rule that says `>0x1000`
//! means what those functions make of it; this is that, and nothing kinder.

/// `isspace` in the C locale.
#[must_use]
pub fn isspace(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// `isdigit`.
#[must_use]
pub fn isdigit(c: u8) -> bool {
    c.is_ascii_digit()
}

/// `isalpha` in the C locale.
#[must_use]
pub fn isalpha(c: u8) -> bool {
    c.is_ascii_alphabetic()
}

/// `isprint` in the C locale: 0x20 through 0x7e.
#[must_use]
pub fn isprint(c: u8) -> bool {
    (0x20..0x7f).contains(&c)
}

/// The value of hex digit `c`, if it is one.
#[must_use]
pub fn hexval(c: u8) -> Option<u8> {
    char::from(c)
        .to_digit(16)
        .and_then(|d| u8::try_from(d).ok())
}

/// What `strtoull`'s scan found: the magnitude it read (saturated on
/// overflow), whether a `-` was written, how many bytes it consumed (0 when it
/// read no number at all), and whether it overflowed (`ERANGE`).
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

/// C's `strtoull(s, &end, base)`: the value, the bytes used (0 if none), and
/// whether it overflowed. A `-` negates the result modulo 2^64, as C does.
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

/// C's `strtoul`; `unsigned long` is 64 bits here, as on the LP64 systems
/// libmagic is measured on.
#[must_use]
pub fn strtoul(s: &[u8], base: u32) -> (u64, usize) {
    let (v, used, _) = strtoull(s, base);
    (v, used)
}

/// C's `strtol(s, &end, base)` for a 64-bit `long`: saturated at
/// `LONG_MIN`/`LONG_MAX` on overflow.
#[must_use]
pub fn strtol(s: &[u8], base: u32) -> (i64, usize) {
    let r = scan(s, base);
    if r.used == 0 {
        return (0, 0);
    }
    let v = if r.negative {
        if r.overflow || r.magnitude > 1u64 << 63 {
            i64::MIN
        } else {
            // In range: `0 - magnitude` for a magnitude up to 2^63.
            0i64.wrapping_sub_unsigned(r.magnitude)
        }
    } else if r.overflow || r.magnitude >= 1u64 << 63 {
        i64::MAX
    } else {
        i64::try_from(r.magnitude).unwrap_or(i64::MAX)
    };
    (v, r.used)
}

/// `(int32_t) strtol(...)`, which is how libmagic stores an offset: a value
/// past 32 bits keeps its low 32, as the C conversion does on every machine
/// libmagic runs on.
#[must_use]
pub fn strtol_i32(s: &[u8], base: u32) -> (i32, usize) {
    let (v, used) = strtol(s, base);
    #[allow(clippy::cast_possible_truncation)]
    let t = v as i32;
    (t, used)
}

/// The length of the C string in `s`: up to its first NUL, or all of it.
#[must_use]
pub fn cstrlen(s: &[u8]) -> usize {
    s.iter().position(|&b| b == 0).unwrap_or(s.len())
}

/// The C string in `s`.
#[must_use]
pub fn cstr(s: &[u8]) -> &[u8] {
    s.get(..cstrlen(s)).unwrap_or(s)
}

/// `memmem`: where `needle` first occurs in `hay`, or `None`. An empty needle
/// occurs at 0.
///
/// By its first byte, then the rest: a search rule looks through as much as
/// a file's first megabyte, and comparing a whole window at every position
/// costs a call to compare at every position.
#[must_use]
pub fn memmem(hay: &[u8], needle: &[u8]) -> Option<usize> {
    let Some((&first, rest)) = needle.split_first() else {
        return Some(0);
    };
    let last = hay.len().checked_sub(needle.len())?;
    let mut from = 0;
    while from <= last {
        let at = from + hay.get(from..=last)?.iter().position(|&b| b == first)?;
        if hay.get(at + 1..at + needle.len()) == Some(rest) {
            return Some(at);
        }
        from = at + 1;
    }
    None
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn memmem_finds_the_first_occurrence() {
        let naive = |h: &[u8], n: &[u8]| {
            if n.is_empty() {
                Some(0)
            } else {
                h.windows(n.len()).position(|w| w == n)
            }
        };
        let cases: &[(&[u8], &[u8])] = &[
            (b"hello", b"he"),
            (b"hello", b"lo"),
            (b"hello", b"l"),
            (b"hello", b"hello!"),
            (b"hello", b""),
            (b"", b"a"),
            (b"aaab", b"ab"),
            (b"abababc", b"ababc"),
            (b"xyz", b"xz"),
            (b"a\0b\0c", b"\0c"),
        ];
        for &(h, n) in cases {
            assert_eq!(memmem(h, n), naive(h, n), "{h:?} {n:?}");
        }
        // Every needle cut from a subject, against every subject.
        let subject = b"the cat sat on the mat; the end";
        for i in 0..subject.len() {
            for j in i..=subject.len().min(i + 6) {
                let n = &subject[i..j];
                assert_eq!(memmem(subject, n), naive(subject, n));
                assert_eq!(memmem(&subject[..i], n), naive(&subject[..i], n));
            }
        }
    }

    #[test]
    fn strtol_reads_c_prefixes_and_saturates() {
        assert_eq!(strtol(b"010", 0), (8, 3));
        assert_eq!(strtol(b"0x10z", 0), (16, 4));
        assert_eq!(strtol(b"0xz", 0), (0, 1));
        assert_eq!(strtol(b"  -12", 0), (-12, 5));
        assert_eq!(strtol(b"x", 0), (0, 0));
        assert_eq!(strtol(b"99999999999999999999", 0).0, i64::MAX);
        assert_eq!(strtol(b"-99999999999999999999", 0).0, i64::MIN);
        assert_eq!(strtol(b"-9223372036854775808", 0).0, i64::MIN);
        assert_eq!(strtol_i32(b"0xffffffff", 0), (-1, 10));
    }

    #[test]
    fn strtoull_negates_and_overflows_as_c_does() {
        assert_eq!(strtoull(b"-1", 0), (u64::MAX, 2, false));
        assert_eq!(strtoull(b"0x8000000000000000", 0).0, 1u64 << 63);
        assert_eq!(strtoull(b"0x1ffffffffffffffff", 0), (u64::MAX, 19, true));
        assert_eq!(strtoull(b"017", 0).0, 15);
        assert_eq!(strtoull(b"8", 8), (0, 0, false));
    }
}
