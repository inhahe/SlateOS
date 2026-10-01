//! The C library as upstream uses it: `fgets`, the handful of `scanf`
//! conversions it reads `/sys` and `/proc` with, glibc's `qsort`, `%f`, and
//! util-linux's string trimmers from `include/strutils.h`.
//!
//! Each is here because what it does at the edges reaches the output:
//! `fgets` splits a long line into pieces that are then parsed as lines of
//! their own; `%d` of `99999999999` is `-1`, not an error; `lookup()` drops
//! the last byte of a line whether or not it is a newline.

use std::io::BufRead;
use ulstrutils::c_isspace;

/// A C string: `bytes` up to its first NUL.
#[must_use]
pub fn c_str(bytes: &[u8]) -> &[u8] {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    bytes.get(..end).unwrap_or_default()
}

/// `isblank`.
#[must_use]
pub fn c_isblank(b: u8) -> bool {
    b == b' ' || b == b'\t'
}

/// `skip_space(p)`: past leading `isspace` bytes.
#[must_use]
pub fn skip_space(s: &[u8]) -> &[u8] {
    let start = s.iter().position(|&b| !c_isspace(b)).unwrap_or(s.len());
    s.get(start..).unwrap_or_default()
}

/// `skip_blank(p)`: past leading blanks and tabs.
#[must_use]
pub fn skip_blank(s: &[u8]) -> &[u8] {
    let start = s.iter().position(|&b| !c_isblank(b)).unwrap_or(s.len());
    s.get(start..).unwrap_or_default()
}

/// `rtrim_whitespace(str)`.
#[must_use]
pub fn rtrim_whitespace(s: &[u8]) -> &[u8] {
    let end = s
        .iter()
        .rposition(|&b| !c_isspace(b))
        .map_or(0, |i| i.saturating_add(1));
    s.get(..end).unwrap_or_default()
}

/// `normalize_whitespace(str)`: leading white space dropped, each run
/// inside cut to its *first* byte (a tab stays a tab), and a single
/// trailing one dropped.
#[must_use]
pub fn normalize_whitespace(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    let mut nsp = 0usize;
    let mut intext = false;
    for &b in s {
        if c_isspace(b) {
            nsp = nsp.saturating_add(1);
        } else {
            nsp = 0;
            intext = true;
        }
        if nsp > 1 || (nsp > 0 && !intext) {
            continue;
        }
        out.push(b);
    }
    if nsp > 0 && !out.is_empty() {
        out.pop();
    }
    out
}

/// `strstr(hay, needle)`: where `needle` first starts in `hay`.
#[must_use]
pub fn strstr(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    hay.windows(needle.len()).position(|w| w == needle)
}

/// `snprintf(buf, size, " %s ", str)` then `strstr(buf, " word ")`: whether
/// `word` is one of the blank-separated words of `flags`, looked for in the
/// first `size - 1` bytes as upstream's `BUFSIZ` buffer holds them.
#[must_use]
pub fn has_word(flags: &[u8], word: &[u8], size: usize) -> bool {
    let mut buf = Vec::with_capacity(flags.len().saturating_add(2));
    buf.push(b' ');
    buf.extend_from_slice(flags);
    buf.push(b' ');
    buf.truncate(size.saturating_sub(1));
    let mut needle = Vec::with_capacity(word.len().saturating_add(2));
    needle.push(b' ');
    needle.extend_from_slice(word);
    needle.push(b' ');
    strstr(&buf, &needle).is_some()
}

/// `lookup(line, pattern, &value)` from `lscpu-cputype.c`: `PATTERN : VALUE`
/// at the start of `line`, the first match winning. The value's end is
/// found by dropping the line's last byte -- meant to be its newline, but
/// dropped whatever it is -- and the white space before it.
pub fn lookup(line: &[u8], pattern: &[u8], value: &mut Option<Vec<u8>>) -> bool {
    let line = c_str(line);
    if line.is_empty() || value.is_some() || !line.starts_with(pattern) {
        return false;
    }
    let mut p = pattern.len();
    while line.get(p).copied().is_some_and(c_isspace) {
        p = p.saturating_add(1);
    }
    if line.get(p) != Some(&b':') {
        return false;
    }
    p = p.saturating_add(1);
    while line.get(p).copied().is_some_and(c_isspace) {
        p = p.saturating_add(1);
    }
    if p >= line.len() {
        return false;
    }
    let v = p;
    // `for (p = line + strlen - 1; isspace(*(p-1)); p--); *p = '\0';` --
    // and when that NUL lands before the value starts (`Type: X`, no
    // newline), the value is left whole.
    let mut end = line.len().saturating_sub(1);
    while end > 0
        && line
            .get(end.saturating_sub(1))
            .copied()
            .is_some_and(c_isspace)
    {
        end = end.saturating_sub(1);
    }
    let found = if end >= v {
        line.get(v..end)
    } else {
        line.get(v..)
    };
    *value = Some(found.unwrap_or_default().to_vec());
    true
}

/// `fgets(buf, size, f)`: bytes up to and including the next newline, or
/// `size - 1` of them, whichever comes first; `None` at the end of the
/// file -- or on a read error, which glibc reports even when some bytes
/// had arrived.
pub fn fgets(reader: &mut impl BufRead, size: usize) -> Option<Vec<u8>> {
    let limit = size.saturating_sub(1);
    let mut line = Vec::new();
    while line.len() < limit {
        let available = match reader.fill_buf() {
            Ok(buf) => buf,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => return None,
        };
        if available.is_empty() {
            break;
        }
        let want = limit.saturating_sub(line.len()).min(available.len());
        let chunk = available.get(..want).unwrap_or_default();
        let (take, done) = match chunk.iter().position(|&b| b == b'\n') {
            Some(i) => (i.saturating_add(1), true),
            None => (chunk.len(), false),
        };
        line.extend_from_slice(chunk.get(..take).unwrap_or_default());
        reader.consume(take);
        if done {
            break;
        }
    }
    if line.is_empty() && limit > 0 {
        return None;
    }
    Some(line)
}

/// glibc 2.39's `qsort`: a top-down merge sort that takes from the left
/// half while `cmp(left, right) <= 0`. With a comparator that is a true
/// order this is any stable sort; with one that is not (upstream's
/// `*a - *b` on `int`s that overflow) it is still exactly glibc's answer,
/// where Rust's own sorts may panic.
pub fn msort<T: Clone>(v: &mut [T], cmp: &impl Fn(&T, &T) -> i32) {
    let n = v.len();
    if n <= 1 {
        return;
    }
    let n1 = n / 2;
    let (left, right) = v.split_at_mut(n1);
    msort(left, cmp);
    msort(right, cmp);
    let mut merged = Vec::with_capacity(n);
    let (mut i, mut j) = (0usize, 0usize);
    while let (Some(a), Some(b)) = (left.get(i), right.get(j)) {
        if cmp(a, b) <= 0 {
            merged.push(a.clone());
            i = i.saturating_add(1);
        } else {
            merged.push(b.clone());
            j = j.saturating_add(1);
        }
    }
    merged.extend_from_slice(left.get(i..).unwrap_or_default());
    merged.extend_from_slice(right.get(j..).unwrap_or_default());
    v.clone_from_slice(&merged);
}

/// `strcmp(a, b)` as an `int`'s sign.
#[must_use]
pub fn strcmp(a: &[u8], b: &[u8]) -> i32 {
    match c_str(a).cmp(c_str(b)) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }
}

/// `printf("%.*f", prec, x)` in the C locale: `nan` and `-nan` by the sign
/// bit, `inf` and `-inf`, and otherwise the exact value rounded to `prec`
/// places, a tie to even -- which Rust's formatting also does.
#[must_use]
pub fn fmt_f(x: f64, prec: usize) -> String {
    if x.is_nan() {
        return if x.is_sign_negative() { "-nan" } else { "nan" }.to_string();
    }
    if x.is_infinite() {
        return if x < 0.0 { "-inf" } else { "inf" }.to_string();
    }
    format!("{x:.prec$}")
}

/// A `scanf` input: bytes, one at a time, with one of lookahead.
pub trait Source {
    /// The next byte, not consumed; `None` at the end (or a read error).
    fn peek(&mut self) -> Option<u8>;
    /// Consume the byte `peek` returned.
    fn bump(&mut self);
}

/// A string, as `sscanf` reads it: it ends at its NUL.
pub struct StrSource<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> StrSource<'a> {
    /// `sscanf(s, ...)`'s input.
    #[must_use]
    pub fn new(s: &'a [u8]) -> Self {
        StrSource {
            bytes: c_str(s),
            pos: 0,
        }
    }
}

impl Source for StrSource<'_> {
    fn peek(&mut self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }
    fn bump(&mut self) {
        self.pos = self.pos.saturating_add(1);
    }
}

/// A stream, as `fscanf` reads it.
pub struct StreamSource<R: BufRead> {
    reader: R,
}

impl<R: BufRead> StreamSource<R> {
    /// `fscanf(f, ...)`'s input.
    pub fn new(reader: R) -> Self {
        StreamSource { reader }
    }
}

impl<R: BufRead> Source for StreamSource<R> {
    fn peek(&mut self) -> Option<u8> {
        loop {
            match self.reader.fill_buf() {
                Ok(buf) => return buf.first().copied(),
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => return None,
            }
        }
    }
    fn bump(&mut self) {
        self.reader.consume(1);
    }
}

/// One value a conversion stored.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Scanned {
    /// An integer conversion's `strtol`/`strtoul` result, before the store
    /// cuts it to the argument's type: in `i64`'s range for a signed
    /// conversion, `u64`'s for an unsigned one.
    Int(i128),
    /// `%s`.
    Str(Vec<u8>),
}

impl Scanned {
    /// `*(int *) arg = (int) num.l`.
    #[must_use]
    pub fn as_int(&self) -> i32 {
        match self {
            Scanned::Int(v) => *v as i32,
            Scanned::Str(_) => 0,
        }
    }
    /// `*(unsigned int *) arg = (unsigned int) num.ul`.
    #[must_use]
    pub fn as_uint(&self) -> u32 {
        match self {
            Scanned::Int(v) => *v as u32,
            Scanned::Str(_) => 0,
        }
    }
    /// `*(long *) arg`, `*(long long *) arg`.
    #[must_use]
    pub fn as_long(&self) -> i64 {
        match self {
            Scanned::Int(v) => *v as i64,
            Scanned::Str(_) => 0,
        }
    }
    /// `*(unsigned long *) arg`, `*(size_t *) arg`.
    #[must_use]
    pub fn as_ulong(&self) -> u64 {
        match self {
            Scanned::Int(v) => *v as u64,
            Scanned::Str(_) => 0,
        }
    }
    /// `%s`'s bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        match self {
            Scanned::Str(s) => s,
            Scanned::Int(_) => &[],
        }
    }
}

/// `strtol` (signed) or `strtoul` (unsigned) of a collected sign and digit
/// run: clamped to `long`'s range, or `unsigned long`'s with a minus sign
/// negating modulo 2^64 -- except past `ULONG_MAX`, which is `ULONG_MAX`
/// whatever the sign.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "the magnitude saturates at 2^100, far inside i128, and is compared against the limits before negation"
)]
fn strto(negative: bool, digits: &[u8], base: u32, signed: bool) -> i128 {
    const CAP: u128 = 1 << 100;
    let mut magnitude: u128 = 0;
    for &d in digits {
        let v = char::from(d).to_digit(base).map_or(0, u128::from);
        if magnitude < CAP {
            magnitude = magnitude * u128::from(base) + v;
        }
    }
    if signed {
        let max = i128::from(i64::MAX);
        let m = i128::try_from(magnitude.min(CAP)).unwrap_or(max);
        if negative {
            (-m).max(i128::from(i64::MIN))
        } else {
            m.min(max)
        }
    } else if magnitude > u128::from(u64::MAX) {
        i128::from(u64::MAX)
    } else {
        let m = u64::try_from(magnitude).unwrap_or(u64::MAX);
        i128::from(if negative { m.wrapping_neg() } else { m })
    }
}

/// Skip `isspace` bytes.
fn skip_ws(src: &mut impl Source) {
    while src.peek().is_some_and(c_isspace) {
        src.bump();
    }
}

/// How a conversion ended: `scanf`'s two kinds of failure.
enum Fail {
    /// The input ran out: `EOF` if nothing was stored yet.
    Input,
    /// The input did not match.
    Matching,
}

/// An integer conversion: blanks, a sign, for base 16 an optional `0x`,
/// then digits, `width` bytes at most in all.
fn scan_int(
    src: &mut impl Source,
    width: Option<usize>,
    base: u32,
    signed: bool,
) -> Result<i128, Fail> {
    skip_ws(src);
    if src.peek().is_none() {
        return Err(Fail::Input);
    }
    let mut left = width.unwrap_or(usize::MAX);
    let mut negative = false;
    if let Some(c @ (b'-' | b'+')) = src.peek()
        && left > 0
    {
        negative = c == b'-';
        src.bump();
        left = left.saturating_sub(1);
    }
    let mut digits = Vec::new();
    if base == 16 && left > 0 && src.peek() == Some(b'0') {
        src.bump();
        left = left.saturating_sub(1);
        digits.push(b'0');
        if left > 0 && matches!(src.peek(), Some(b'x' | b'X')) {
            src.bump();
            left = left.saturating_sub(1);
        }
    }
    while left > 0 {
        match src.peek() {
            Some(c) if char::from(c).is_digit(base) => {
                digits.push(c);
                src.bump();
                left = left.saturating_sub(1);
            }
            _ => break,
        }
    }
    if digits.is_empty() {
        return Err(Fail::Matching);
    }
    Ok(strto(negative, &digits, base, signed))
}

/// `scanf(fmt, ...)` for the directives upstream uses: white space,
/// literal bytes, `%d %u %x` with a width and an `l`, `ll` or `z` size,
/// `%s` with a width, and `%*[^\n]`. The values stored, in order; their
/// count is `scanf`'s return, except that `None` is its `EOF`.
///
/// A conversion upstream's formats never use is a matching failure.
pub fn scanf(src: &mut impl Source, fmt: &[u8]) -> Option<Vec<Scanned>> {
    let mut out = Vec::new();
    let mut f = 0usize;
    let result = loop {
        let Some(&fc) = fmt.get(f) else {
            break Ok(());
        };
        f = f.saturating_add(1);
        if c_isspace(fc) {
            skip_ws(src);
            continue;
        }
        if fc != b'%' {
            match src.peek() {
                None => break Err(Fail::Input),
                Some(c) if c == fc => src.bump(),
                Some(_) => break Err(Fail::Matching),
            }
            continue;
        }
        let suppress = fmt.get(f) == Some(&b'*');
        if suppress {
            f = f.saturating_add(1);
        }
        let digits = fmt
            .get(f..)
            .unwrap_or_default()
            .iter()
            .take_while(|b| b.is_ascii_digit())
            .count();
        let width = std::str::from_utf8(fmt.get(f..f.saturating_add(digits)).unwrap_or_default())
            .ok()
            .and_then(|w| w.parse::<usize>().ok())
            .filter(|&w| w > 0);
        f = f.saturating_add(digits);
        while matches!(fmt.get(f), Some(b'l' | b'z')) {
            f = f.saturating_add(1);
        }
        let Some(&conv) = fmt.get(f) else {
            break Err(Fail::Matching);
        };
        f = f.saturating_add(1);
        let value = match conv {
            b'd' => scan_int(src, width, 10, true).map(Scanned::Int),
            b'u' => scan_int(src, width, 10, false).map(Scanned::Int),
            b'x' => scan_int(src, width, 16, false).map(Scanned::Int),
            b's' => {
                skip_ws(src);
                let mut s = Vec::new();
                let mut left = width.unwrap_or(usize::MAX);
                while left > 0 {
                    match src.peek() {
                        Some(c) if !c_isspace(c) => {
                            s.push(c);
                            src.bump();
                            left = left.saturating_sub(1);
                        }
                        _ => break,
                    }
                }
                if s.is_empty() {
                    Err(Fail::Input)
                } else {
                    Ok(Scanned::Str(s))
                }
            }
            b'[' if fmt.get(f..).is_some_and(|r| r.starts_with(b"^\n]")) => {
                f = f.saturating_add(3);
                let mut s = Vec::new();
                let mut left = width.unwrap_or(usize::MAX);
                while left > 0 {
                    match src.peek() {
                        Some(c) if c != b'\n' => {
                            s.push(c);
                            src.bump();
                            left = left.saturating_sub(1);
                        }
                        _ => break,
                    }
                }
                if s.is_empty() {
                    Err(if src.peek().is_none() {
                        Fail::Input
                    } else {
                        Fail::Matching
                    })
                } else {
                    Ok(Scanned::Str(s))
                }
            }
            _ => Err(Fail::Matching),
        };
        match value {
            Ok(v) => {
                if !suppress {
                    out.push(v);
                }
            }
            Err(e) => break Err(e),
        }
    };
    match result {
        Err(Fail::Input) if out.is_empty() => None,
        _ => Some(out),
    }
}

/// `sscanf(s, fmt, ...)`.
#[must_use]
pub fn sscanf(s: &[u8], fmt: &[u8]) -> Option<Vec<Scanned>> {
    scanf(&mut StrSource::new(s), fmt)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ints(s: &str, fmt: &str) -> Option<Vec<i128>> {
        sscanf(s.as_bytes(), fmt.as_bytes()).map(|v| {
            v.iter()
                .map(|x| match x {
                    Scanned::Int(i) => *i,
                    Scanned::Str(_) => -999,
                })
                .collect()
        })
    }

    #[test]
    fn int_conversions_clamp_and_truncate_as_glibc() {
        assert_eq!(ints("  42\n", "%d"), Some(vec![42]));
        assert_eq!(ints("-7", "%d"), Some(vec![-7]));
        assert_eq!(ints("x", "%d"), Some(vec![]));
        assert_eq!(ints("", "%d"), None);
        assert_eq!(ints("   ", "%d"), None);
        let big = sscanf(b"99999999999999999999", b"%d").unwrap_or_default();
        // strtol clamps to LONG_MAX, and the store keeps its low 32 bits.
        assert_eq!(big.first().map(Scanned::as_int), Some(-1));
        let wrap = sscanf(b"4294967296", b"%d").unwrap_or_default();
        assert_eq!(wrap.first().map(Scanned::as_int), Some(0));
        let neg = sscanf(b"-1", b"%u").unwrap_or_default();
        assert_eq!(neg.first().map(Scanned::as_uint), Some(u32::MAX));
        let neg = sscanf(b"-1", b"%zu").unwrap_or_default();
        assert_eq!(neg.first().map(Scanned::as_ulong), Some(u64::MAX));
        let huge = sscanf(b"-99999999999999999999", b"%lu").unwrap_or_default();
        assert_eq!(huge.first().map(Scanned::as_ulong), Some(u64::MAX));
    }

    #[test]
    fn literals_and_white_space_match_as_glibc() {
        assert_eq!(
            ints(
                "CPU Topology SW: 0 0 4 2 3 8",
                "CPU Topology SW: %d %d %zu %zu %zu %zu"
            ),
            Some(vec![0, 0, 4, 2, 3, 8])
        );
        // A space in the format matches none at all.
        assert_eq!(
            ints(
                "CPUTopologySW:1 2 3 4 5 6",
                "CPU Topology SW: %d %d %zu %zu %zu %zu"
            ),
            Some(vec![1, 2, 3, 4, 5, 6])
        );
        assert_eq!(
            ints(
                "CPU Topology SW: 1 2",
                "CPU Topology SW: %d %d %zu %zu %zu %zu"
            ),
            Some(vec![1, 2])
        );
        assert_eq!(ints(" CPU", "CPU %d"), Some(vec![]));
        assert_eq!(ints("level=3 x", "level=%d"), Some(vec![3]));
    }

    #[test]
    fn widths_and_hex_as_glibc() {
        assert_eq!(
            ints("00f0\t8086abcd\tx", "%02x%02x\t%04x%04x\t%*[^\n]"),
            Some(vec![0, 0xf0, 0x8086, 0xabcd])
        );
        let s = sscanf(b"  full rest", b"%255s").unwrap_or_default();
        assert_eq!(s.first().map(Scanned::as_bytes), Some(&b"full"[..]));
    }

    #[test]
    fn lookup_drops_the_last_byte() {
        let mut v = None;
        assert!(lookup(b"Type:   2964  \n", b"Type", &mut v));
        assert_eq!(v.as_deref(), Some(&b"2964"[..]));
        let mut v = None;
        assert!(lookup(b"Type: 2964", b"Type", &mut v));
        assert_eq!(v.as_deref(), Some(&b"296"[..]));
        let mut v = None;
        assert!(lookup(b"Type:X", b"Type", &mut v));
        assert_eq!(v.as_deref(), Some(&b""[..]));
        // The NUL goes on the blank before the value, which stays whole.
        let mut v = None;
        assert!(lookup(b"Type: X", b"Type", &mut v));
        assert_eq!(v.as_deref(), Some(&b"X"[..]));
        let mut v = None;
        assert!(!lookup(b"Type Foo: 1\n", b"Type", &mut v));
        assert!(!lookup(b"Type:  \n", b"Type", &mut v));
        let mut v = Some(b"old".to_vec());
        assert!(!lookup(b"Type: 1\n", b"Type", &mut v));
    }

    #[test]
    fn fgets_splits_long_lines() {
        let mut r = std::io::Cursor::new(b"abcdef\ngh".to_vec());
        assert_eq!(fgets(&mut r, 4), Some(b"abc".to_vec()));
        assert_eq!(fgets(&mut r, 4), Some(b"def".to_vec()));
        assert_eq!(fgets(&mut r, 4), Some(b"\n".to_vec()));
        assert_eq!(fgets(&mut r, 4), Some(b"gh".to_vec()));
        assert_eq!(fgets(&mut r, 4), None);
    }

    #[test]
    fn normalize_keeps_the_first_byte_of_a_run() {
        assert_eq!(
            normalize_whitespace(b"  z/VM\t\t 6.4.0 \n"),
            b"z/VM\t6.4.0".to_vec()
        );
        assert_eq!(normalize_whitespace(b""), Vec::<u8>::new());
        assert_eq!(normalize_whitespace(b"   "), Vec::<u8>::new());
    }

    #[test]
    fn msort_is_glibcs_merge() {
        let mut v = vec![(2, 'a'), (1, 'b'), (2, 'c'), (1, 'd')];
        msort(&mut v, &|a: &(i32, char), b: &(i32, char)| a.0 - b.0);
        assert_eq!(v, vec![(1, 'b'), (1, 'd'), (2, 'a'), (2, 'c')]);
        // A comparator that is not an order does not panic.
        let mut w = vec![i32::MAX, -2, 5, i32::MIN, 0];
        msort(&mut w, &|a: &i32, b: &i32| a.wrapping_sub(*b));
        assert_eq!(w.len(), 5);
    }

    #[test]
    fn fmt_f_is_printfs() {
        assert_eq!(fmt_f(0.125, 2), "0.12");
        assert_eq!(fmt_f(0.375, 2), "0.38");
        assert_eq!(fmt_f(2.5, 0), "2");
        assert_eq!(fmt_f(3.5, 0), "4");
        assert_eq!(fmt_f(f64::NAN, 4), "nan");
        assert_eq!(fmt_f(-f64::NAN, 4), "-nan");
        assert_eq!(fmt_f(f64::NEG_INFINITY, 2), "-inf");
        assert_eq!(fmt_f(f64::from(4590.83_f32), 2), "4590.83");
        assert_eq!(fmt_f(-1.0, 4), "-1.0000");
    }

    #[test]
    fn has_word_looks_in_a_bufsiz_buffer() {
        assert!(has_word(b"fpu lm svm", b"lm", 8192));
        assert!(!has_word(b"fpu lmx", b"lm", 8192));
        assert!(!has_word(b"fpu lm", b"lm", 7));
        assert!(has_word(b"fpu lm", b"lm", 9));
    }
}
