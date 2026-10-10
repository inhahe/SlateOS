//! The C library behaviour libmagic leans on, in the C locale: `<ctype.h>`'s
//! classes for one byte, and the `strto*` family's parse -- prefixes,
//! signs, overflow and all -- which decides what a magic rule's numbers are.
//!
//! libmagic parses its database with `strtol`, `strtoul` and `strtoull` at
//! base 0, so `010` is eight, `0x10` sixteen, an overflow saturates, and a
//! minus sign negates even an unsigned result. A rule that says `>0x1000`
//! means what those functions make of it; this is that, and nothing kinder.

// `isspace` and the `strto*` family are `cstrtol`'s, which they were
// extracted into on 2026-10-08 when `xxd` and `hexdump` needed them too.
pub use cstrtol::{isspace, strtol, strtoul, strtoull};

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

/// `(int32_t) strtol(...)`, which is how libmagic stores an offset: a value
/// past 32 bits keeps its low 32, as the C conversion does on every machine
/// libmagic runs on.
#[must_use]
pub fn strtol_i32(s: &[u8], base: u32) -> (i32, usize) {
    let (v, used) = strtol(s, base);
    (cstrtol::low_i32(v), used)
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
    fn the_strto_family_is_cstrtols() {
        // The rules themselves are tested in `cstrtol`; this is the one
        // libmagic adds on top.
        assert_eq!(strtol(b"010", 0), (8, 3));
        assert_eq!(strtol_i32(b"0xffffffff", 0), (-1, 10));
        assert_eq!(strtoull(b"-1", 0), (u64::MAX, 2, false));
    }
}
