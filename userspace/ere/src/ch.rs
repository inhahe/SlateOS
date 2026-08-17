//! The *character* of a byte string, and the decoding that finds one.
//!
//! A byte string need not be text. SlateOS paths permit every byte but `/` and
//! NUL, a shell value is whatever bytes it was given, and a file grep is asked
//! to search is a stream of bytes with no encoding promised. So "character"
//! here means either a decoded Unicode scalar ([`Ch::U`]) *or* a single byte
//! that begins no valid UTF-8 sequence ([`Ch::B`]) — which is bash's rule, and
//! is what makes `.` match such a byte as **one** character rather than as a
//! third of an `é`.
//!
//! This model lives in this crate rather than in the shell because the regex
//! engine is defined over it: a pattern, a subject and a bracket range are all
//! byte strings, and every one of them may hold a byte that decodes to nothing.
//! `oils`'s `bytes` module re-exports these items, so there is exactly one
//! definition of what a character is and the shell and the utilities cannot
//! drift apart about it.

use bstr::ByteSlice;

/// An owned byte string: an arbitrary byte sequence, no encoding implied.
pub type Str = Vec<u8>;

/// A borrowed byte string. Named for symmetry with [`Str`]; it is a plain
/// `&[u8]`, so `bstr`'s [`ByteSlice`] methods apply directly.
pub type BStr<'a> = &'a [u8];

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
