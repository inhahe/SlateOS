//! The shell string type, and the few operations `bstr` deliberately lacks.
//!
//! A shell string is a **byte string**, not a Rust `String`. SlateOS paths
//! permit every byte except `/` and NUL, so a file may legitimately be named
//! `a\xffb`; a shell whose value type is `String` cannot name it, and — worse
//! — `String::from_utf8_lossy` would turn that byte into U+FFFD and have the
//! shell open, `stat` or delete a *different* file. See `design-decisions.md
//! §93` and `known-issues.md TD-OILS-BYTE-STRINGS`.
//!
//! So osh's value type is [`Str`] (`Vec<u8>`) and its borrowed form is
//! [`BStr`] (`&[u8]`), with the [`bstr`] crate supplying the `str`-shaped
//! methods over them. What lives here is only what `bstr` does not have:
//!
//! * [`to_lowercase`] / [`to_uppercase`] — `bstr`'s are ASCII-only unless its
//!   heavyweight `unicode` feature (and `regex-automata` with it) is enabled.
//! * [`char_count`] / [`char_slice`] / [`char_at`] — bash's character counting
//!   for `${#v}` and `${v:off:len}`, where an invalid byte counts as exactly
//!   one character.
//! * [`bfmt!`] — a concatenating byte-string builder, because `format!` has no
//!   byte-string counterpart and `Display for BStr` is *lossy*.
//! * [`Ch`] — the *character* of a byte string, for the operations bash defines
//!   over characters rather than bytes (glob's `?`, `${#v}`, `${v^^}`).
//! * [`os_to_bytes`] / [`bytes_to_path`] — the boundary with `std::fs`, where a
//!   filesystem name becomes a shell value and back.
//!
//! **Names are not values.** Variable, function and option names stay `String`:
//! the grammar already restricts them to the portable character set, and
//! keeping them `String` preserves `HashMap<String, _>` lookup by `&str`.

use bstr::ByteSlice;

/// An owned shell string: an arbitrary byte sequence, no encoding implied.
pub type Str = Vec<u8>;

/// A borrowed shell string. Named for symmetry with [`Str`]; it is a plain
/// `&[u8]`, so `bstr`'s [`ByteSlice`] methods apply directly.
pub type BStr<'a> = &'a [u8];

/// Build a [`Str`] by concatenating its arguments.
///
/// This is osh's replacement for `format!`. It concatenates rather than parsing
/// a format string, and that is deliberate: `format!` would route a shell value
/// through `Display`, and `Display for bstr::BStr` replaces every invalid byte
/// with U+FFFD — reintroducing at every diagnostic and every string-building
/// site the exact corruption this module exists to prevent. With no `{}` there
/// is no place for a lossy conversion to hide.
///
/// Each argument may be anything implementing [`PushBytes`]: `&[u8]`, `&Vec<u8>`,
/// `&str`, `String`, `u8` (one byte), `char` (its UTF-8), or any integer (its
/// decimal spelling, which is ASCII and so lossless through `format!`).
///
/// ```ignore
/// let msg = bfmt![name, b": ", path, b": ", err_text];
/// ```
#[macro_export]
macro_rules! bfmt {
    [$($arg:expr),* $(,)?] => {{
        let mut __buf: $crate::bytes::Str = $crate::bytes::Str::new();
        $( $crate::bytes::PushBytes::push_bytes(&$arg, &mut __buf); )*
        __buf
    }};
}

/// Appending to a byte buffer, for [`bfmt!`].
///
/// Implemented by reference (`&self`) rather than by value so `bfmt!` can take
/// a reference to each argument uniformly and never move it — the macro must
/// work with `bfmt![name, b": ", value]` where `name` and `value` are borrowed
/// from `self` at the call site.
pub trait PushBytes {
    /// Append this value's byte spelling to `out`.
    fn push_bytes(&self, out: &mut Str);
}

impl PushBytes for [u8] {
    fn push_bytes(&self, out: &mut Str) {
        out.extend_from_slice(self);
    }
}

impl<const N: usize> PushBytes for [u8; N] {
    fn push_bytes(&self, out: &mut Str) {
        out.extend_from_slice(self);
    }
}

impl PushBytes for Vec<u8> {
    fn push_bytes(&self, out: &mut Str) {
        out.extend_from_slice(self);
    }
}

impl PushBytes for str {
    fn push_bytes(&self, out: &mut Str) {
        out.extend_from_slice(self.as_bytes());
    }
}

impl PushBytes for String {
    fn push_bytes(&self, out: &mut Str) {
        out.extend_from_slice(self.as_bytes());
    }
}

impl PushBytes for u8 {
    fn push_bytes(&self, out: &mut Str) {
        out.push(*self);
    }
}

impl PushBytes for char {
    fn push_bytes(&self, out: &mut Str) {
        let mut buf = [0u8; 4];
        out.extend_from_slice(self.encode_utf8(&mut buf).as_bytes());
    }
}

impl<T: PushBytes + ?Sized> PushBytes for &T {
    fn push_bytes(&self, out: &mut Str) {
        (**self).push_bytes(out);
    }
}

/// `String`-shaped appending for a byte buffer.
///
/// Lets code that builds shell source keep reading the way it did when the
/// buffer was a `String` — `s.push_str("if ")` for ASCII syntax,
/// `s.push_str(&word)` for data — while the buffer underneath is bytes. The
/// argument is anything [`PushBytes`] accepts, so a `&str` literal and a `Str`
/// value append through the same call.
///
/// Deliberately no `push`: `Vec<u8>`'s own `push` takes a byte and would shadow
/// any `push(char)` here, so a `s.push('x')` left behind by a conversion fails
/// to compile rather than silently meaning something else. Write `s.push(b'x')`.
pub trait StrBuf {
    /// Append `s`'s byte spelling to this buffer.
    fn push_str(&mut self, s: &(impl PushBytes + ?Sized));
}

impl StrBuf for Str {
    fn push_str(&mut self, s: &(impl PushBytes + ?Sized)) {
        s.push_bytes(self);
    }
}

/// Concatenate `parts` with `sep` between them — `[T]::join` for byte strings.
#[must_use]
pub fn join(parts: &[Str], sep: BStr<'_>) -> Str {
    let mut out = Str::new();
    for (i, p) in parts.iter().enumerate() {
        if i > 0 {
            out.extend_from_slice(sep);
        }
        out.extend_from_slice(p);
    }
    out
}

/// Integers spell themselves in decimal. `format!` is lossless here because the
/// output is pure ASCII by construction.
macro_rules! push_bytes_via_display {
    ($($t:ty),*) => {$(
        impl PushBytes for $t {
            fn push_bytes(&self, out: &mut Str) {
                use std::io::Write as _;
                // Writing to a `Vec<u8>` is infallible.
                let _ = write!(out, "{self}");
            }
        }
    )*};
}
push_bytes_via_display!(i8, i16, i32, i64, i128, isize, u16, u32, u64, u128, usize);

/// Append a `char`'s UTF-8 spelling to a byte string.
///
/// The counterpart of `String::push` for the many loops that walk *source* text
/// (which the parser still hands over as `&str`) while building a *value*
/// (which is bytes). Lossless in both directions: a `char` is by definition
/// encodable, and what comes back out through [`chars`] is the same character.
pub fn push_char(out: &mut Str, c: char) {
    let mut buf = [0u8; 4];
    out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
}

/// Borrow `s` as `&str` when — and only when — it is valid UTF-8.
///
/// This is the *only* way this crate turns bytes into text: it never invents a
/// replacement character, it reports that the bytes are not text.
/// Use it where a value has to be interpreted as text to mean anything at all
/// — a numeric parse, a `strftime` format, a lookup in a `HashMap<String, _>`
/// keyed by a portable identifier — because for those the correct answer for
/// non-UTF-8 input is "this is not a number / not that name", not "here is a
/// mangled approximation".
#[must_use]
pub fn as_str(s: BStr<'_>) -> Option<&str> {
    std::str::from_utf8(s).ok()
}

/// Byte-offset of the first occurrence of `needle` in `haystack`.
///
/// `str::find` for byte strings. An empty needle matches at 0, matching
/// `str::find`'s convention so that callers translated from `&str` keep their
/// behaviour.
#[must_use]
pub fn find(haystack: BStr<'_>, needle: BStr<'_>) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    if needle.len() > haystack.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Byte-offset of the **last** occurrence of `needle` in `haystack`.
///
/// `str::rfind` for byte strings, and empty-needle-compatible with it: an empty
/// needle matches at the end, not at 0.
#[must_use]
pub fn rfind(haystack: BStr<'_>, needle: BStr<'_>) -> Option<usize> {
    if needle.is_empty() {
        return Some(haystack.len());
    }
    if needle.len() > haystack.len() {
        return None;
    }
    haystack.windows(needle.len()).rposition(|w| w == needle)
}

/// Whether `needle` occurs anywhere in `haystack` — `str::contains` for byte
/// strings.
#[must_use]
pub fn contains(haystack: BStr<'_>, needle: BStr<'_>) -> bool {
    find(haystack, needle).is_some()
}

/// Replace at most `n` non-overlapping occurrences of `from` with `to`, left to
/// right — [`str::replacen`], and [`str::replace`] when `n` is [`usize::MAX`].
///
/// An empty `from` replaces nothing. That is *not* what `str::replacen` does
/// (it splices `to` in at every character boundary), but every caller here
/// reached this function through a shell construct where an empty pattern means
/// "match nothing", and the `str` behaviour would insert text the user never
/// asked for.
#[must_use]
pub fn replacen(s: BStr<'_>, from: BStr<'_>, to: BStr<'_>, n: usize) -> Str {
    if from.is_empty() || n == 0 {
        return s.to_vec();
    }
    let mut out = Str::with_capacity(s.len());
    let mut rest = s;
    let mut done = 0usize;
    while done < n
        && let Some(at) = find(rest, from)
    {
        out.extend_from_slice(rest.get(..at).unwrap_or_default());
        out.extend_from_slice(to);
        rest = rest.get(at.saturating_add(from.len())..).unwrap_or_default();
        done = done.saturating_add(1);
    }
    out.extend_from_slice(rest);
    out
}

/// The lines of `s`, exactly as [`str::lines`] splits text.
///
/// A line ends at a `\n`, which takes a `\r` immediately before it with it;
/// the last line need not be terminated; a trailing newline does *not* yield a
/// final empty line; and an empty input has no lines at all. Splitting a
/// history file or a `mapfile` source this way keeps every byte of every line,
/// which `str::lines` on a lossily-decoded string would not.
pub fn lines(s: BStr<'_>) -> impl Iterator<Item = BStr<'_>> {
    // `split` on an empty slice still yields one (empty) piece, which is the
    // single case where it disagrees with `str::lines`; `None` drops it.
    let body = if s.is_empty() {
        None
    } else {
        Some(s.strip_suffix(b"\n").unwrap_or(s))
    };
    body.into_iter()
        .flat_map(|b| b.split(|&c| c == b'\n'))
        .map(|l| l.strip_suffix(b"\r").unwrap_or(l))
}

/// Parse a shell value as a decimal integer the way the shell's numeric
/// contexts do: surrounding ASCII whitespace is ignored, and anything that is
/// not a well-formed number — including bytes that are not text at all —
/// yields `None`.
#[must_use]
pub fn parse_i64(s: BStr<'_>) -> Option<i64> {
    as_str(trim(s))?.parse::<i64>().ok()
}

/// Drop leading ASCII whitespace.
///
/// ASCII and not Unicode, because that is what bash trims: it reads a value one
/// byte at a time through `isspace`, which in the C locale is exactly
/// `\t \n \v \f \r` and the space. (`bstr`'s `trim_start` would need the
/// `unicode` feature, and would also trim characters bash keeps.)
#[must_use]
pub fn trim_start(s: BStr<'_>) -> BStr<'_> {
    let i = s.iter().position(|b| !is_space(*b)).unwrap_or(s.len());
    s.get(i..).unwrap_or_default()
}

/// Drop trailing ASCII whitespace. See [`trim_start`].
#[must_use]
pub fn trim_end(s: BStr<'_>) -> BStr<'_> {
    let i = s.iter().rposition(|b| !is_space(*b)).map_or(0, |i| i + 1);
    s.get(..i).unwrap_or_default()
}

/// Is `b` whitespace to C's `isspace` in the C locale — space, `\t`, `\n`,
/// `\v`, `\f`, `\r`?
///
/// Not `u8::is_ascii_whitespace`, which omits the **vertical tab**: Rust
/// deliberately follows the WhatWG definition there, while every byte-wise
/// space test bash makes goes through `isspace`. The difference is observable —
/// `v=$'\v5'; echo $((v))` is `5` in bash, because the arithmetic evaluator
/// trims the value before reading a number from it.
#[must_use]
pub const fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// Drop ASCII whitespace from both ends. See [`trim_start`].
#[must_use]
pub fn trim(s: BStr<'_>) -> BStr<'_> {
    trim_end(trim_start(s))
}

/// Case-map `s`, applying `map` to each `char` of every valid UTF-8 run and
/// passing invalid bytes through unchanged.
///
/// Splitting on `utf8_chunks()` is what makes this total: bash's case
/// conversion is defined over characters, but a shell value need not be text at
/// all, and a byte that is not part of any character has no case to change.
fn map_case<F, I>(s: BStr<'_>, map: F) -> Str
where
    F: Fn(char) -> I,
    I: Iterator<Item = char>,
{
    let mut out = Str::with_capacity(s.len());
    for chunk in s.utf8_chunks() {
        for ch in chunk.valid().chars() {
            let mut buf = [0u8; 4];
            for mapped in map(ch) {
                out.extend_from_slice(mapped.encode_utf8(&mut buf).as_bytes());
            }
        }
        out.extend_from_slice(chunk.invalid());
    }
    out
}

/// Unicode lowercase, byte-transparent. Invalid bytes survive unchanged.
///
/// `bstr`'s own `to_lowercase` is ASCII-only unless its `unicode` feature is
/// on, which would pull `regex-automata` into the shell for this alone.
pub fn to_lowercase(s: BStr<'_>) -> Str {
    map_case(s, char::to_lowercase)
}

/// Unicode uppercase, byte-transparent. See [`to_lowercase`].
pub fn to_uppercase(s: BStr<'_>) -> Str {
    map_case(s, char::to_uppercase)
}

/// The number of *characters* in `s`, as bash counts them for `${#v}`.
///
/// bash counts one character per multibyte sequence and one per byte that is
/// not part of a valid sequence. `bstr`'s `char_indices()` yields exactly that:
/// an invalid byte comes back as `(i, i + 1, U+FFFD)`.
pub fn char_count(s: BStr<'_>) -> usize {
    s.char_indices().count()
}

/// The byte offset of character index `n`, or `s.len()` when `n` is at or past
/// the end. Used to turn bash's character-indexed `${v:off:len}` into a byte
/// range.
pub fn char_offset(s: BStr<'_>, n: usize) -> usize {
    s.char_indices().nth(n).map_or(s.len(), |(start, _, _)| start)
}

/// `${v:off:len}` — `len` characters starting at character `off`, clamped to
/// the string. Character indices follow [`char_count`]'s rule.
pub fn char_slice(s: BStr<'_>, off: usize, len: usize) -> Str {
    let start = char_offset(s, off);
    // Count `len` characters *from the start offset* rather than from index
    // `off + len` of the whole string: the two agree, but this way the second
    // walk is over the tail only.
    let tail = s.get(start..).unwrap_or_default();
    let end = start.saturating_add(char_offset(tail, len));
    s.get(start..end).unwrap_or_default().to_vec()
}

/// The single character at character index `n`, as bytes; empty when `n` is out
/// of range. This is `${v:n:1}` and is what indexes a string in `${v[n]}`-style
/// character walks.
pub fn char_at(s: BStr<'_>, n: usize) -> Str {
    char_slice(s, n, 1)
}

/// One character of a shell string.
///
/// A shell string is bytes, but several shell operations are defined over
/// *characters*: glob's `?` matches one, `${#v}` counts them, `${v^^}` cases
/// them. A byte string need not be text, so "character" here means either a
/// decoded Unicode scalar ([`Ch::U`]) or a single byte that begins no valid
/// UTF-8 sequence ([`Ch::B`]) — which is exactly bash's rule.
///
/// The distinction is carried rather than flattened to raw bytes because
/// flattening would make `?` match a third of an `é`; and it is not flattened
/// to `char` either, because U+FFFD is a real character a value may legitimately
/// contain, so it cannot double as "some byte I could not decode".
///
/// The derived `Ord` — every [`Ch::U`] below every [`Ch::B`], each ordered by
/// its own value — is what a glob range like `[a-z]` compares with. Both
/// endpoints of a written range are always characters, so the useful part is
/// that an undecodable byte falls in no such range, which is right: it is not a
/// letter, and bash's collation would not place it among them either.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Ch {
    /// A decoded Unicode scalar value.
    U(char),
    /// A byte that is not part of any valid UTF-8 sequence.
    B(u8),
}

impl Ch {
    /// Append this character's bytes to `out`.
    pub fn push_to(self, out: &mut Str) {
        match self {
            Ch::U(c) => {
                let mut buf = [0u8; 4];
                out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            }
            Ch::B(b) => out.push(b),
        }
    }

    /// This character's bytes, as an owned string.
    #[must_use]
    pub fn to_str(self) -> Str {
        let mut out = Str::new();
        self.push_to(&mut out);
        out
    }

    /// How many bytes this character occupies.
    #[must_use]
    pub fn byte_len(self) -> usize {
        match self {
            Ch::U(c) => c.len_utf8(),
            Ch::B(_) => 1,
        }
    }

    /// The ASCII character this is, or `None` for anything else.
    ///
    /// Shell *syntax* — glob metacharacters, `[a-z]` range punctuation, IFS
    /// defaults — is entirely ASCII, so this is how a matcher asks "is this the
    /// `*` I care about" without having to think about encoding at all.
    #[must_use]
    pub fn as_ascii(self) -> Option<char> {
        match self {
            Ch::U(c) if c.is_ascii() => Some(c),
            _ => None,
        }
    }

    /// The `char` this character is, for the Unicode-defined operations
    /// (case folding, `[[:alpha:]]`). An undecodable byte is not any character,
    /// so it answers `None` and those operations leave it alone.
    #[must_use]
    pub fn as_char(self) -> Option<char> {
        match self {
            Ch::U(c) => Some(c),
            Ch::B(_) => None,
        }
    }

    /// Whether this is a control character, for the quoters that switch to the
    /// ANSI-C `$'…'` form when a value holds one.
    ///
    /// A byte that decodes to nothing is *not* a control character: every C0/C1
    /// control is a Unicode scalar value, so an undecodable byte is by
    /// definition none of them, and bash — reading the same bytes with
    /// `iscntrl` in the C locale — says the same, since an undecodable byte is
    /// always ≥ 0x80 while `iscntrl` there covers only 0x00–0x1F and 0x7F.
    #[must_use]
    pub fn is_control(self) -> bool {
        match self {
            Ch::U(c) => c.is_control(),
            Ch::B(_) => false,
        }
    }

    /// ASCII-only case folding, for `nocaseglob`-style matching that must keep
    /// the pattern's token structure 1:1 with the original.
    #[must_use]
    pub fn to_ascii_lowercase(self) -> Ch {
        match self {
            Ch::U(c) => Ch::U(c.to_ascii_lowercase()),
            Ch::B(b) => Ch::B(b.to_ascii_lowercase()),
        }
    }

    /// Unicode lowercase mapping. Returns a `Vec` because one character can map
    /// to several (`İ` → `i̇`); a byte that is no character has no case and maps
    /// to itself.
    #[must_use]
    pub fn to_lowercase(self) -> Vec<Ch> {
        match self {
            Ch::U(c) => c.to_lowercase().map(Ch::U).collect(),
            Ch::B(_) => vec![self],
        }
    }

    /// Unicode uppercase mapping. See [`Ch::to_lowercase`].
    #[must_use]
    pub fn to_uppercase(self) -> Vec<Ch> {
        match self {
            Ch::U(c) => c.to_uppercase().map(Ch::U).collect(),
            Ch::B(_) => vec![self],
        }
    }
}

impl From<char> for Ch {
    fn from(c: char) -> Self {
        Ch::U(c)
    }
}

impl PartialEq<char> for Ch {
    fn eq(&self, other: &char) -> bool {
        matches!(*self, Ch::U(c) if c == *other)
    }
}

/// Decode `s` into characters under [`Ch`]'s rule.
///
/// This is the byte-string counterpart of `str::chars()`, and the iterator every
/// character-wise shell operation should walk instead of `bstr`'s
/// `chars()` — which reports an undecodable byte as U+FFFD and so cannot tell it
/// apart from a value that really contains U+FFFD.
pub fn chars(s: BStr<'_>) -> impl Iterator<Item = Ch> + '_ {
    char_positions(s).map(|(_, c)| c)
}

/// [`chars`], but each character paired with its starting byte offset.
pub fn char_positions(s: BStr<'_>) -> impl Iterator<Item = (usize, Ch)> + '_ {
    s.char_indices().map(|(start, end, c)| {
        // `char_indices` signals "not decodable" by yielding U+FFFD; the span it
        // covers is what says whether the value truly held one.
        let ch = if c == '\u{fffd}' && s.get(start..end) != Some("\u{fffd}".as_bytes()) {
            Ch::B(s.get(start).copied().unwrap_or(0))
        } else {
            Ch::U(c)
        };
        (start, ch)
    })
}

/// Collect the characters of `s` back into a byte string.
pub fn from_chars<I: IntoIterator<Item = Ch>>(chars: I) -> Str {
    let mut out = Str::new();
    for c in chars {
        c.push_to(&mut out);
    }
    out
}

/// A host path/OS string as shell bytes.
///
/// SlateOS is `target-family = ["unix"]` (see `toolchain/x86_64-slateos.json`),
/// so on the real target this is the identity: `OsStr` *is* the byte sequence
/// the kernel handed back, and a directory entry named `a\xffb` survives
/// `read_dir` → glob → `open` unchanged. That round trip is the entire point of
/// TD-OILS-BYTE-STRINGS.
///
/// The Windows *development* host is the exception, and unavoidably so: its
/// filesystem names are UTF-16, not bytes, so there is no byte sequence to
/// recover and `to_string_lossy` is the only thing to do. It costs nothing in
/// practice — a Windows path cannot contain an unpaired byte to begin with.
#[cfg(unix)]
#[must_use]
pub fn os_to_bytes(s: &std::ffi::OsStr) -> Str {
    std::os::unix::ffi::OsStrExt::as_bytes(s).to_vec()
}

#[cfg(not(unix))]
#[must_use]
pub fn os_to_bytes(s: &std::ffi::OsStr) -> Str {
    s.to_string_lossy().into_owned().into_bytes()
}

/// Shell bytes as a host OS string. The inverse of [`os_to_bytes`], with the
/// same platform caveat.
#[cfg(unix)]
#[must_use]
pub fn bytes_to_os(b: BStr<'_>) -> std::ffi::OsString {
    <std::ffi::OsString as std::os::unix::ffi::OsStringExt>::from_vec(b.to_vec())
}

#[cfg(not(unix))]
#[must_use]
pub fn bytes_to_os(b: BStr<'_>) -> std::ffi::OsString {
    std::ffi::OsString::from(String::from_utf8_lossy(b).into_owned())
}

/// Shell bytes as a host path, for handing to `std::fs`.
#[must_use]
pub fn bytes_to_path(b: BStr<'_>) -> std::path::PathBuf {
    std::path::PathBuf::from(bytes_to_os(b))
}

/// A host path as shell bytes. See [`os_to_bytes`].
#[must_use]
pub fn path_to_bytes(p: &std::path::Path) -> Str {
    os_to_bytes(p.as_os_str())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::{
        BStr, Str, char_at, char_count, char_offset, char_slice, find, replacen, rfind,
        to_lowercase, to_uppercase, trim,
    };

    /// `a\xffb` — the value that motivates this whole module: three characters
    /// under bash's counting rule, and not valid UTF-8.
    const LONE: BStr<'static> = b"a\xffb";

    /// The vertical tab is whitespace to C's `isspace` — and so to every space
    /// test bash makes — but *not* to Rust's `u8::is_ascii_whitespace`, which
    /// follows the WhatWG definition instead. Trimming has to use C's rule, or
    /// `v=$'\v5'; echo $((v))` stops being `5`.
    #[test]
    fn trim_takes_the_vertical_tab_that_rust_leaves() {
        assert_eq!(trim(b"\x0b5\x0b"), b"5");
        assert_eq!(trim(b" \t\n\x0b\x0c\r"), b"");
        // Every other byte stays, including ones that are not text at all.
        assert_eq!(trim(b"\xa0x\xa0"), b"\xa0x\xa0");
    }

    #[test]
    fn rfind_and_replacen_are_str_semantics_over_bytes() {
        // The last occurrence, and one that a UTF-8 decode could not have found
        // at all because neither the needle nor its surroundings are text.
        assert_eq!(rfind(b"xa ya za", b"a"), Some(7));
        assert_eq!(rfind(b"\xffq\xffq", b"\xffq"), Some(2));
        assert_eq!(rfind(b"abc", b"z"), None);
        // Empty-needle conventions match `str`: `find` at 0, `rfind` at the end.
        assert_eq!(find(b"abc", b""), Some(0));
        assert_eq!(rfind(b"abc", b""), Some(3));
        // Replacement is left-to-right and non-overlapping.
        assert_eq!(replacen(b"aaa", b"a", b"b", 1), b"baa".to_vec());
        assert_eq!(replacen(b"aaa", b"a", b"b", usize::MAX), b"bbb".to_vec());
        assert_eq!(replacen(b"aaaa", b"aa", b"x", usize::MAX), b"xx".to_vec());
        // An empty pattern replaces nothing, unlike `str::replacen`.
        assert_eq!(replacen(b"abc", b"", b"-", usize::MAX), b"abc".to_vec());
        // Bytes that are not text pass through and can be replaced.
        assert_eq!(replacen(LONE, b"\xff", b"!", usize::MAX), b"a!b".to_vec());
    }

    #[test]
    fn bfmt_concatenates_every_argument_kind() {
        let owned: Str = b"val".to_vec();
        let got = bfmt![b"n=", &owned, b'/', 42u32, b'/', "s", b'/', 'x', b'/', -7i32];
        assert_eq!(got, b"n=val/42/s/x/-7".to_vec());
    }

    #[test]
    fn bfmt_is_byte_transparent() {
        // The point of not having a `{}`: an invalid byte cannot be routed
        // through `Display` and become U+FFFD.
        let got = bfmt![b"[", LONE, b"]"];
        assert_eq!(got, b"[a\xffb]".to_vec());
    }

    #[test]
    fn find_matches_str_find_semantics() {
        assert_eq!(find(LONE, b"\xffb"), Some(1));
        assert_eq!(find(LONE, b"b\xff"), None);
        // An empty needle matches at the front, as `str::find` does.
        assert_eq!(find(LONE, b""), Some(0));
        assert_eq!(find(b"", b""), Some(0));
        // A needle longer than the haystack must not panic in `windows`.
        assert_eq!(find(b"a", b"ab"), None);
        assert_eq!(find(b"", b"a"), None);
        assert_eq!(find(b"abcabc", b"ca"), Some(2));
    }

    #[test]
    fn case_mapping_leaves_invalid_bytes_alone() {
        assert_eq!(to_uppercase(LONE), b"A\xffB".to_vec());
        assert_eq!(to_lowercase(b"A\xffB"), LONE.to_vec());
        // A byte that would be a lowercase ASCII letter *inside* a UTF-8
        // sequence must not be touched — it is not a character on its own.
        assert_eq!(to_uppercase("é".as_bytes()), "É".as_bytes().to_vec());
        assert_eq!(to_lowercase("É".as_bytes()), "é".as_bytes().to_vec());
    }

    #[test]
    fn case_mapping_handles_multi_char_expansions() {
        // U+00DF LATIN SMALL LETTER SHARP S uppercases to two characters.
        assert_eq!(to_uppercase("stra\u{df}e".as_bytes()), b"STRASSE".to_vec());
    }

    #[test]
    fn char_count_treats_an_invalid_byte_as_one_character() {
        assert_eq!(char_count(LONE), 3);
        assert_eq!(char_count(b""), 0);
        assert_eq!(char_count("héllo".as_bytes()), 5);
        // Two stray continuation bytes are two characters, not one.
        assert_eq!(char_count(b"\x80\x80"), 2);
        // A truncated three-byte sequence is one character (bstr reports the
        // whole invalid run as a single replacement).
        assert_eq!(char_count(b"\xe2\x82"), 1);
    }

    #[test]
    fn char_slicing_is_by_character_and_clamps() {
        assert_eq!(char_slice(LONE, 1, 2), b"\xffb".to_vec());
        assert_eq!(char_slice(LONE, 0, 1), b"a".to_vec());
        assert_eq!(char_slice(LONE, 1, 1), b"\xff".to_vec());
        assert_eq!(char_slice(LONE, 3, 1), Str::new());
        assert_eq!(char_slice(LONE, 0, 99), LONE.to_vec());
        assert_eq!(char_slice("héllo".as_bytes(), 1, 2), "él".as_bytes().to_vec());
        assert_eq!(char_at(LONE, 1), b"\xff".to_vec());
        assert_eq!(char_at(LONE, 9), Str::new());
    }

    #[test]
    fn chars_distinguishes_an_undecodable_byte_from_a_real_replacement_char() {
        use super::{Ch, chars, from_chars};
        assert_eq!(
            chars(LONE).collect::<Vec<_>>(),
            vec![Ch::U('a'), Ch::B(0xff), Ch::U('b')]
        );
        // A value that genuinely contains U+FFFD must not be mistaken for a
        // decode failure — that is the whole reason `Ch` is not just `char`.
        assert_eq!(
            chars("\u{fffd}".as_bytes()).collect::<Vec<_>>(),
            vec![Ch::U('\u{fffd}')]
        );
        // Round trip: characters back to the same bytes.
        for s in [LONE, "héllo".as_bytes(), b"\x80\x80", b"", "\u{fffd}".as_bytes()] {
            assert_eq!(from_chars(chars(s)), s.to_vec());
        }
    }

    #[test]
    fn ch_answers_only_for_what_it_really_is() {
        use super::{Ch, char_positions};
        assert_eq!(Ch::B(0xff).as_char(), None);
        assert_eq!(Ch::B(0xff).as_ascii(), None);
        // A multibyte character is not ASCII, so glob syntax never sees it.
        assert_eq!(Ch::U('é').as_ascii(), None);
        assert_eq!(Ch::U('é').as_char(), Some('é'));
        assert_eq!(Ch::U('*').as_ascii(), Some('*'));
        assert_eq!(Ch::U('é').byte_len(), 2);
        assert_eq!(Ch::B(0xff).byte_len(), 1);
        assert_eq!(Ch::B(0xff).to_str(), b"\xff".to_vec());
        assert!(Ch::U('*') == '*');
        assert!(Ch::B(b'*') != '*');
        assert_eq!(
            char_positions("hé\u{ff}".as_bytes()).collect::<Vec<_>>(),
            vec![(0, Ch::U('h')), (1, Ch::U('é')), (3, Ch::U('\u{ff}'))]
        );
    }

    #[test]
    fn char_offset_reports_the_end_past_the_last_character() {
        assert_eq!(char_offset(LONE, 0), 0);
        assert_eq!(char_offset(LONE, 2), 2);
        assert_eq!(char_offset(LONE, 3), 3);
        assert_eq!(char_offset(LONE, 4), 3);
        assert_eq!(char_offset("héllo".as_bytes(), 2), 3);
    }
}
