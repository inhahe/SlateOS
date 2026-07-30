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

/// **Refactor scaffolding — every call site is a bug until it is gone.**
///
/// Reinterpret a shell string as a Rust `String`, replacing anything that is
/// not valid UTF-8. That is exactly the silent corruption TD-OILS-BYTE-STRINGS
/// exists to remove, so this function must not survive the conversion: it is
/// here only to keep the tree compiling while osh is converted module by
/// module, at the seams where a byte-native producer still feeds a
/// `String`-typed consumer.
///
/// It is `#[deprecated]` on purpose — the resulting warnings *are* the list of
/// seams that remain, and the conversion is finished when the compiler stops
/// printing them and this function is deleted.
#[deprecated(
    note = "byte-string refactor scaffolding: this call site must become byte-native \
            before TD-OILS-BYTE-STRINGS can land"
)]
pub fn scaffold_lossy_string(v: BStr<'_>) -> String {
    String::from_utf8_lossy(v).into_owned()
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

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::{BStr, Str, char_at, char_count, char_offset, char_slice, to_lowercase, to_uppercase};

    /// `a\xffb` — the value that motivates this whole module: three characters
    /// under bash's counting rule, and not valid UTF-8.
    const LONE: BStr<'static> = b"a\xffb";

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
    fn char_offset_reports_the_end_past_the_last_character() {
        assert_eq!(char_offset(LONE, 0), 0);
        assert_eq!(char_offset(LONE, 2), 2);
        assert_eq!(char_offset(LONE, 3), 3);
        assert_eq!(char_offset(LONE, 4), 3);
        assert_eq!(char_offset("héllo".as_bytes(), 2), 3);
    }
}
