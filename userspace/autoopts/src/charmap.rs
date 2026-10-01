//! libopts' character classes (`ag-char-map.h`) and its option-name
//! comparison (`streqvcmp.c`).
//!
//! Every scan of a configuration file is a walk over one of these classes,
//! and several of them are not what the name suggests: *whitespace* includes
//! backspace, a *value name* includes `:` (so `name:value` spans as one
//! word and is only split later, by `load_opt_line`), and no class ever
//! contains a byte of 128 or more. The table is copied from the generated
//! header rather than rebuilt from the class definitions, so that a
//! transcription error cannot hide in the rebuilding.
//!
//! Text is a NUL-terminated C string held in a byte slice: a position at or
//! past the end of the slice reads as NUL, exactly as the C code's pointer
//! would stop at the terminator.

/// `\t`, space, `\n`, `\v`, `\f`, `\r` and `\b`.
pub const WHITESPACE: u32 = 0x0000_0C01;
/// `!` through `~`.
pub const GRAPHIC: u32 = 0x0000_4000;
/// `0` through `7`.
pub const OCT_DIGIT: u32 = 0x0001_0000;
/// `0` through `9`.
pub const DEC_DIGIT: u32 = 0x0003_0000;
/// Decimal digits and `a`-`f`, `A`-`F`.
pub const HEX_DIGIT: u32 = 0x0007_0000;
/// `a` through `z`.
pub const LOWER_CASE: u32 = 0x0008_0000;
/// `_` and letters: what may start a configuration entry.
pub const VAR_FIRST: u32 = 0x0018_0040;
/// `^`, `-`, `_`, letters and digits.
pub const OPTION_NAME: u32 = 0x003B_0040;
/// An option name's characters and `:`.
pub const VALUE_NAME: u32 = 0x003B_0060;
/// `/`, `>` and whitespace: what ends an XML-ish token.
pub const END_XML_TOKEN: u32 = 0x0100_0C01;
/// NUL, `,` and whitespace.
pub const END_LIST_ENTRY: u32 = 0x0000_0C13;
/// `\t`, space and `-`: what `load_opt_line` skips before a name.
pub const LOAD_LINE_SKIP: u32 = 0x0000_0600;

/// `ag_char_map_table`, verbatim: the class bits of each ASCII byte.
#[rustfmt::skip]
const TABLE: [u32; 128] = [
    0x0000_0002, 0x0000_0000, 0x0000_0000, 0x0000_0000,
    0x0000_0000, 0x0000_0000, 0x0000_0000, 0x0000_0000,
    0x0000_0800, 0x0000_0400, 0x0000_0001, 0x0000_0800,
    0x0000_0800, 0x0000_0800, 0x0000_0000, 0x0000_0000,
    0x0000_0000, 0x0000_0000, 0x0000_0000, 0x0000_0000,
    0x0000_0000, 0x0000_0000, 0x0000_0000, 0x0000_0000,
    0x0000_0000, 0x0000_0000, 0x0000_0000, 0x0000_0000,
    0x0000_0000, 0x0000_0000, 0x0000_0000, 0x0000_0000,
    0x0000_0400, 0x0280_4000, 0x0200_5000, 0x0200_4000,
    0x0280_4100, 0x0280_4008, 0x0280_4000, 0x0200_5000,
    0x0200_6000, 0x0200_6000, 0x0200_4000, 0x1280_4080,
    0x0200_4010, 0x06A0_C200, 0x06C0_4000, 0x0380_4004,
    0x0881_4000, 0x0081_4000, 0x0081_4000, 0x0081_4000,
    0x0081_4000, 0x0081_4000, 0x0081_4000, 0x0081_4000,
    0x0082_4000, 0x0082_4000, 0x0280_4020, 0x0200_4000,
    0x0200_4000, 0x0200_4000, 0x0300_4000, 0x0200_4000,
    0x0280_4000, 0x0094_4000, 0x0094_4000, 0x0094_4000,
    0x0094_4000, 0x0094_4000, 0x0894_4000, 0x0090_4000,
    0x0090_4000, 0x0090_4000, 0x0090_4000, 0x0090_4000,
    0x0090_4000, 0x0090_4000, 0x0890_4000, 0x0090_4000,
    0x0090_4000, 0x0090_4000, 0x0090_4000, 0x0090_4000,
    0x0090_4000, 0x0090_4000, 0x0090_4000, 0x0090_4000,
    0x0090_4000, 0x0090_4000, 0x0090_4000, 0x0240_4000,
    0x0200_4004, 0x0240_4000, 0x02A0_4000, 0x0480_4040,
    0x0200_4000, 0x008C_4000, 0x008C_4000, 0x008C_4000,
    0x008C_4000, 0x008C_4000, 0x088C_4000, 0x0088_4000,
    0x0088_4000, 0x0088_4000, 0x0088_4000, 0x0088_4000,
    0x0088_4000, 0x0088_4000, 0x0888_4000, 0x0088_4000,
    0x0088_4000, 0x0088_4000, 0x0088_4000, 0x0088_4000,
    0x0088_4000, 0x0088_4000, 0x0088_4000, 0x0088_4000,
    0x0088_4000, 0x0088_4000, 0x0088_4000, 0x0200_4000,
    0x1280_4000, 0x0200_4000, 0x0280_C000, 0x0000_0000,
];

/// The byte at `i`, or NUL past the end: a C string's view of the slice.
#[must_use]
pub fn at(s: &[u8], i: usize) -> u8 {
    s.get(i).copied().unwrap_or(0)
}

/// `IS_xxx_CHAR`: whether `c` is in the class. NUL is in exactly the classes
/// that name it (`END_LIST_ENTRY`); bytes of 128 and up are in none.
#[must_use]
pub fn is(c: u8, mask: u32) -> bool {
    TABLE
        .get(usize::from(c))
        .is_some_and(|&bits| bits & mask != 0)
}

/// `SPN_xxx_CHARS`: the first position from `i` whose byte is not in the
/// class. NUL is never spanned, whatever the class: the spanner tables leave
/// entry 0 clear.
#[must_use]
pub fn spn(s: &[u8], mut i: usize, mask: u32) -> usize {
    loop {
        let c = at(s, i);
        if c == 0 || !is(c, mask) {
            return i;
        }
        i = i.saturating_add(1);
    }
}

/// `BRK_xxx_CHARS`: the first position from `i` whose byte is NUL or in the
/// class.
#[must_use]
pub fn brk(s: &[u8], mut i: usize, mask: u32) -> usize {
    loop {
        let c = at(s, i);
        if c == 0 || is(c, mask) {
            return i;
        }
        i = i.saturating_add(1);
    }
}

/// `SPN_xxx_BACK(s, e)`: back from `e` over bytes in the class, not past
/// `start`. An `e` at or before `start` means "the end of the string", as in
/// the C.
#[must_use]
pub fn spn_back(s: &[u8], start: usize, e: usize, mask: u32) -> usize {
    let mut e = if start >= e {
        start.saturating_add(strlen(s, start))
    } else {
        e
    };
    while e > start {
        let prev = e.saturating_sub(1);
        let c = at(s, prev);
        if c == 0 || !is(c, mask) {
            break;
        }
        e = prev;
    }
    e
}

/// `strlen(s + i)`.
#[must_use]
pub fn strlen(s: &[u8], i: usize) -> usize {
    s.get(i..).map_or(0, |rest| {
        rest.iter().position(|&b| b == 0).unwrap_or(rest.len())
    })
}

/// The C string at `i`, copied out.
#[must_use]
pub fn cstr(s: &[u8], i: usize) -> Vec<u8> {
    let n = strlen(s, i);
    s.get(i..i.saturating_add(n))
        .map(<[u8]>::to_vec)
        .unwrap_or_default()
}

/// `strchr(s + i, c)` for a non-NUL `c`.
#[must_use]
pub fn strchr(s: &[u8], i: usize, c: u8) -> Option<usize> {
    let n = strlen(s, i);
    s.get(i..i.saturating_add(n))?
        .iter()
        .position(|&b| b == c)
        .map(|p| i.saturating_add(p))
}

/// `strstr(s + i, needle)`.
#[must_use]
pub fn strstr(s: &[u8], i: usize, needle: &[u8]) -> Option<usize> {
    let n = strlen(s, i);
    let hay = s.get(i..i.saturating_add(n))?;
    if needle.is_empty() {
        return Some(i);
    }
    hay.windows(needle.len())
        .position(|w| w == needle)
        .map(|p| i.saturating_add(p))
}

/// `strncmp(s + i, word, word.len()) == 0`: `word` (which holds no NUL) is at
/// `i`.
#[must_use]
pub fn starts_with(s: &[u8], i: usize, word: &[u8]) -> bool {
    word.iter()
        .enumerate()
        .all(|(k, &b)| at(s, i.saturating_add(k)) == b)
}

/// streqvcmp's map: identity, except that letters compare without case and
/// `-`, `_` and `^` all compare as `-` (`strequate(zSepChars)`, done once by
/// `validate_struct` before any name is compared).
fn eqv(c: u8) -> u8 {
    match c {
        b'A'..=b'Z' => c.to_ascii_lowercase(),
        b'_' | b'^' => b'-',
        _ => c,
    }
}

/// `strneqvcmp`: compare at most `ct` bytes of two C strings under the
/// equivalence map. Zero when they match; otherwise the difference of the
/// first mapped bytes that differ.
#[must_use]
pub fn strneqvcmp(s1: &[u8], s2: &[u8], ct: usize) -> i32 {
    for i in 0..ct {
        let u1 = at(s1, i);
        let u2 = at(s2, i);
        if u1 == u2 {
            if u1 == 0 {
                return 0;
            }
            continue;
        }
        let dif = i32::from(eqv(u1)).wrapping_sub(i32::from(eqv(u2)));
        if dif != 0 {
            return dif;
        }
        if u1 == 0 {
            return 0;
        }
    }
    0
}

/// `strtoul(s + i, &end, base)` for base 8, 10 or 16, as glibc parses it:
/// leading whitespace, an optional sign (a `-` negates, modulo 2^64), a `0x`
/// prefix in base 16, then digits. With no digits the value is 0 and the end
/// is `i` itself; on overflow the value is `u64::MAX` and the end is still
/// past every digit.
#[must_use]
pub fn strtoul(s: &[u8], i: usize, base: u32) -> (u64, usize) {
    let mut p = i;
    while matches!(at(s, p), b' ' | b'\t' | b'\n' | b'\x0b' | b'\x0c' | b'\r') {
        p = p.saturating_add(1);
    }
    let negative = at(s, p) == b'-';
    if matches!(at(s, p), b'+' | b'-') {
        p = p.saturating_add(1);
    }
    if base == 16
        && at(s, p) == b'0'
        && matches!(at(s, p.saturating_add(1)), b'x' | b'X')
        && at(s, p.saturating_add(2)).is_ascii_hexdigit()
    {
        p = p.saturating_add(2);
    }
    let start = p;
    let mut value: u64 = 0;
    let mut overflow = false;
    while let Some(d) = char::from(at(s, p)).to_digit(base) {
        match value
            .checked_mul(u64::from(base))
            .and_then(|v| v.checked_add(u64::from(d)))
        {
            Some(v) => value = v,
            None => overflow = true,
        }
        p = p.saturating_add(1);
    }
    if p == start {
        return (0, i);
    }
    if overflow {
        return (u64::MAX, p);
    }
    (
        if negative {
            value.wrapping_neg()
        } else {
            value
        },
        p,
    )
}
