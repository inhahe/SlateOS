#![no_std]
#![deny(clippy::all)]

//! Reading fixed-width fields out of a byte slice, where the bound is stated
//! at the read.
//!
//! # The shape this replaces
//!
//! Every binary-format parser in this tree is written the same way. It proves
//! once that the buffer is long enough, and then restates that proof at each
//! byte it reads:
//!
//! ```ignore
//! if data.len() < 54 {
//!     return None;
//! }
//! // ... twenty lines later ...
//! let width = i32::from_le_bytes([data[18], data[19], data[20], data[21]]);
//! ```
//!
//! Four index expressions, each a separate claim that byte 21 exists, none of
//! them checked, and all four resting on a `54` written in a different
//! statement — usually a different *screen*. The lint sweep found this in
//! `apps/explorer`'s BMP thumbnailer (a crafted file panicked it),
//! `apps/paint`'s BMP loader, and `apps/imageviewer`'s MP4 and Matroska
//! parsers, which between them held over a hundred such claims about offsets
//! read out of the file being parsed.
//!
//! The bug is not that any particular one of those claims is wrong. It is that
//! there are a hundred of them and each has to be checked by hand, so the
//! question "is this parser safe?" has no answer shorter than reading all
//! hundred. The functions here make each read carry its own bound, so the
//! answer is "yes, structurally" and a truncated or hostile file produces
//! `None` instead of a panic.
//!
//! # Two ways in
//!
//! [`field`] and the `_at` functions are for **random access** — formats that
//! give you an offset and a layout, like a BMP header or an MP4 box. The
//! offset is an argument, nothing is remembered between calls, and every one
//! returns `None` rather than panicking:
//!
//! ```
//! # use byteread::{u32_le_at, array_at};
//! let bmp = [b'B', b'M', 0, 0, 0, 0];
//! assert_eq!(array_at::<2>(&bmp, 0), Some([b'B', b'M']));
//! assert_eq!(u32_le_at(&bmp, 2), Some(0));
//! assert_eq!(u32_le_at(&bmp, 3), None); // three bytes left, four wanted
//! ```
//!
//! [`Reader`] is for **sequential access** — formats read front to back, like
//! an EBML element or a chunk header. It carries the position so the caller
//! does not do the offset arithmetic that is the other half of this defect
//! family:
//!
//! ```
//! # use byteread::Reader;
//! let mut r = Reader::new(&[0x00, 0x00, 0x00, 0x10, b'f', b't', b'y', b'p']);
//! assert_eq!(r.u32_be(), Some(16));
//! assert_eq!(r.array::<4>(), Some(*b"ftyp"));
//! assert_eq!(r.u8(), None);
//! ```
//!
//! # Endianness is named at the call, never inferred
//!
//! There is no `u32(&self)`. BMP is little-endian, MP4 is big-endian, and
//! Matroska is big-endian with variable-width integers; a parser that reads
//! more than one format — `apps/imageviewer` reads three — cannot have a
//! default that is right. Each function says which order it means, so moving a
//! line between two parsers is a compile error or a visible change, not a
//! silent byte swap.

use core::mem::size_of;

// ============================================================================
// Random access
// ============================================================================

/// Reads `N` bytes starting at `at`, or `None` if they are not all present.
///
/// This is the primitive the rest of the crate is written in terms of, and the
/// one to reach for when a format's field is not an integer — a FourCC, a
/// signature, a UUID. The addition is checked, so an `at` computed from the
/// file being parsed cannot wrap past the length test.
#[must_use]
pub fn array_at<const N: usize>(data: &[u8], at: usize) -> Option<[u8; N]> {
    data.get(at..at.checked_add(N)?)?.try_into().ok()
}

/// Reads the `len` bytes starting at `at`, or `None` if they are not all
/// present.
///
/// The slicing counterpart to [`array_at`], for a run whose length is only
/// known at run time. `&data[at..at + len]` panics twice over — once if the
/// addition wraps and once if the end is past the buffer — and this does
/// neither.
#[must_use]
pub fn slice_at(data: &[u8], at: usize, len: usize) -> Option<&[u8]> {
    data.get(at..at.checked_add(len)?)
}

/// Reads the single byte at `at`.
#[must_use]
pub fn u8_at(data: &[u8], at: usize) -> Option<u8> {
    data.get(at).copied()
}

/// A field whose width and byte order are given by the conversion function.
///
/// Named separately from [`array_at`] because it is what a caller adding a
/// width this crate does not spell — a `u128` GUID, an `f64` — should use
/// rather than reaching for indexing again.
///
/// ```
/// # use byteread::field;
/// let data = [0x01, 0x02, 0x03, 0x04];
/// assert_eq!(field(&data, 0, u32::from_be_bytes), Some(0x0102_0304));
/// ```
#[must_use]
pub fn field<const N: usize, T>(data: &[u8], at: usize, from: fn([u8; N]) -> T) -> Option<T> {
    Some(from(array_at::<N>(data, at)?))
}

macro_rules! scalar_at {
    ($name:ident, $ty:ty, $conv:path, $order:literal) => {
        #[doc = concat!("Reads a ", stringify!($ty), " at `at`, ", $order, ".")]
        #[must_use]
        pub fn $name(data: &[u8], at: usize) -> Option<$ty> {
            field(data, at, $conv)
        }
    };
}

scalar_at!(
    u16_le_at,
    u16,
    u16::from_le_bytes,
    "least-significant byte first"
);
scalar_at!(
    u16_be_at,
    u16,
    u16::from_be_bytes,
    "most-significant byte first"
);
scalar_at!(
    u32_le_at,
    u32,
    u32::from_le_bytes,
    "least-significant byte first"
);
scalar_at!(
    u32_be_at,
    u32,
    u32::from_be_bytes,
    "most-significant byte first"
);
scalar_at!(
    u64_le_at,
    u64,
    u64::from_le_bytes,
    "least-significant byte first"
);
scalar_at!(
    u64_be_at,
    u64,
    u64::from_be_bytes,
    "most-significant byte first"
);
scalar_at!(
    i16_le_at,
    i16,
    i16::from_le_bytes,
    "least-significant byte first"
);
scalar_at!(
    i32_le_at,
    i32,
    i32::from_le_bytes,
    "least-significant byte first"
);
scalar_at!(
    i32_be_at,
    i32,
    i32::from_be_bytes,
    "most-significant byte first"
);
scalar_at!(
    i64_be_at,
    i64,
    i64::from_be_bytes,
    "most-significant byte first"
);

/// Whether `data` begins with `prefix`.
///
/// The signature check every format parser opens with. `&data[..4] == sig`
/// panics on a file shorter than the signature — which is exactly the file a
/// fuzzer produces first — where this answers `false`.
///
/// ```
/// # use byteread::starts_with;
/// assert!(starts_with(b"RIFF....", b"RIFF"));
/// assert!(!starts_with(b"RI", b"RIFF"));
/// ```
#[must_use]
pub fn starts_with(data: &[u8], prefix: &[u8]) -> bool {
    data.get(..prefix.len()) == Some(prefix)
}

/// Whether the `needle` bytes appear contiguously anywhere in `haystack`.
///
/// An empty needle is present in everything, including an empty haystack,
/// which is the same convention [`str::contains`] uses.
#[must_use]
pub fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() {
        return true;
    }
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// The offset of the first contiguous occurrence of `needle` in `haystack`.
///
/// Returns `Some(0)` for an empty needle, matching [`contains`].
#[must_use]
pub fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

// ============================================================================
// Sequential access
// ============================================================================

/// A position in a byte slice that only ever moves forward, and never past the
/// end.
///
/// Every method returns `None` and **leaves the position unmoved** when the
/// remaining bytes are too few. That matters more than it looks: a reader that
/// consumed a partial field on failure would make a caller's retry read
/// garbage, so a failed read is a no-op and the caller can decide whether to
/// give up or seek elsewhere.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    /// A reader positioned at the start of `data`.
    #[must_use]
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    /// A reader positioned at `pos`, which is clamped to the end of `data`
    /// rather than being rejected — a parser that computes a start offset from
    /// the file wants "there is nothing there", not a second error path.
    #[must_use]
    pub fn at(data: &'a [u8], pos: usize) -> Self {
        Self {
            data,
            pos: pos.min(data.len()),
        }
    }

    /// The current offset from the start of the underlying slice.
    #[must_use]
    pub fn position(&self) -> usize {
        self.pos
    }

    /// The number of bytes not yet consumed.
    #[must_use]
    pub fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    /// Whether every byte has been consumed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.remaining() == 0
    }

    /// The bytes not yet consumed, without consuming them.
    #[must_use]
    pub fn rest(&self) -> &'a [u8] {
        self.data.get(self.pos..).unwrap_or(&[])
    }

    /// Advances by `n` bytes, or returns `None` and stays put if there are
    /// fewer than `n` left.
    pub fn skip(&mut self, n: usize) -> Option<()> {
        let next = self.pos.checked_add(n)?;
        if next > self.data.len() {
            return None;
        }
        self.pos = next;
        Some(())
    }

    /// Moves to an absolute offset, or returns `None` and stays put if it is
    /// past the end.
    ///
    /// Unlike [`Reader::at`] this does *not* clamp: a caller seeking to a
    /// specific structure wants to know that the structure is not there.
    pub fn seek(&mut self, pos: usize) -> Option<()> {
        if pos > self.data.len() {
            return None;
        }
        self.pos = pos;
        Some(())
    }

    /// Consumes and returns the next `n` bytes.
    pub fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.pos.checked_add(n)?;
        let out = self.data.get(self.pos..end)?;
        self.pos = end;
        Some(out)
    }

    /// Consumes and returns the next `N` bytes as an array.
    pub fn array<const N: usize>(&mut self) -> Option<[u8; N]> {
        let out = array_at::<N>(self.data, self.pos)?;
        self.pos = self.pos.checked_add(N)?;
        Some(out)
    }

    /// Reads `N` bytes `offset` bytes ahead without consuming anything.
    #[must_use]
    pub fn peek<const N: usize>(&self, offset: usize) -> Option<[u8; N]> {
        array_at::<N>(self.data, self.pos.checked_add(offset)?)
    }

    /// Consumes a field of `N` bytes and converts it.
    pub fn field<const N: usize, T>(&mut self, from: fn([u8; N]) -> T) -> Option<T> {
        Some(from(self.array::<N>()?))
    }

    /// Consumes the next byte.
    pub fn u8(&mut self) -> Option<u8> {
        self.field::<1, u8>(|b| b.first().copied().unwrap_or(0))
    }
}

macro_rules! scalar_read {
    ($name:ident, $ty:ty, $conv:path, $order:literal) => {
        #[doc = concat!("Consumes a ", stringify!($ty), ", ", $order, ".")]
        pub fn $name(&mut self) -> Option<$ty> {
            self.field::<{ size_of::<$ty>() }, $ty>($conv)
        }
    };
}

impl Reader<'_> {
    scalar_read!(
        u16_le,
        u16,
        u16::from_le_bytes,
        "least-significant byte first"
    );
    scalar_read!(
        u16_be,
        u16,
        u16::from_be_bytes,
        "most-significant byte first"
    );
    scalar_read!(
        u32_le,
        u32,
        u32::from_le_bytes,
        "least-significant byte first"
    );
    scalar_read!(
        u32_be,
        u32,
        u32::from_be_bytes,
        "most-significant byte first"
    );
    scalar_read!(
        u64_le,
        u64,
        u64::from_le_bytes,
        "least-significant byte first"
    );
    scalar_read!(
        u64_be,
        u64,
        u64::from_be_bytes,
        "most-significant byte first"
    );
    scalar_read!(
        i16_le,
        i16,
        i16::from_le_bytes,
        "least-significant byte first"
    );
    scalar_read!(
        i32_le,
        i32,
        i32::from_le_bytes,
        "least-significant byte first"
    );
    scalar_read!(
        i32_be,
        i32,
        i32::from_be_bytes,
        "most-significant byte first"
    );
}

#[cfg(test)]
mod tests {
    // A test that indexes out of range should fail loudly and point at the
    // line that did it — that is the diagnosis. The defensive lints exist to
    // keep panics out of code that runs on a user's data, which this is not.
    #![allow(
        clippy::indexing_slicing,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic
    )]

    use super::{
        Reader, array_at, contains, field, find, i32_le_at, slice_at, starts_with, u8_at,
        u16_be_at, u32_be_at, u32_le_at,
    };

    #[test]
    fn a_field_that_fits_is_read_and_one_that_does_not_is_none() {
        let data = [1u8, 2, 3, 4, 5];
        assert_eq!(array_at::<4>(&data, 0), Some([1, 2, 3, 4]));
        assert_eq!(array_at::<4>(&data, 1), Some([2, 3, 4, 5]));
        assert_eq!(array_at::<4>(&data, 2), None);
        assert_eq!(array_at::<0>(&data, 5), Some([]));
    }

    #[test]
    fn an_offset_at_the_top_of_the_address_space_does_not_wrap_into_range() {
        // This is the whole point. `data[at..at + 4]` with `at = usize::MAX - 1`
        // computes an end of 2 and slices the first two bytes of the buffer —
        // a read the caller believed was rejected. The addition is checked, so
        // the answer is "not present".
        let data = [1u8, 2, 3, 4];
        assert_eq!(array_at::<4>(&data, usize::MAX), None);
        assert_eq!(array_at::<4>(&data, usize::MAX - 1), None);
        assert_eq!(slice_at(&data, usize::MAX, 4), None);
        assert_eq!(u32_le_at(&data, usize::MAX - 3), None);
    }

    #[test]
    fn the_two_byte_orders_disagree_and_both_are_available() {
        let data = [0x12u8, 0x34, 0x56, 0x78];
        assert_eq!(u32_le_at(&data, 0), Some(0x7856_3412));
        assert_eq!(u32_be_at(&data, 0), Some(0x1234_5678));
        assert_eq!(u16_be_at(&data, 2), Some(0x5678));
    }

    #[test]
    fn a_signed_field_keeps_its_sign() {
        let data = (-1234i32).to_le_bytes();
        assert_eq!(i32_le_at(&data, 0), Some(-1234));
    }

    #[test]
    fn a_field_conversion_can_be_supplied_by_the_caller() {
        let data = [0u8, 0, 0, 0, 0, 0, 0, 7];
        assert_eq!(field(&data, 0, u64::from_be_bytes), Some(7));
        assert_eq!(field(&data, 1, u64::from_be_bytes), None);
    }

    #[test]
    fn a_slice_shorter_than_the_signature_is_not_a_match_rather_than_a_panic() {
        assert!(starts_with(b"RIFF0000", b"RIFF"));
        assert!(!starts_with(b"RIF", b"RIFF"));
        assert!(!starts_with(b"", b"RIFF"));
        assert!(starts_with(b"anything", b""));
    }

    #[test]
    fn a_subsequence_is_found_where_it_is() {
        assert_eq!(find(b"\x1aE\xdf\xa3webm", b"webm"), Some(4));
        assert!(contains(b"\x1aE\xdf\xa3webm", b"webm"));
        assert_eq!(find(b"short", b"longer needle"), None);
        assert!(!contains(b"short", b"longer needle"));
        // Empty needle: present at the start, including in nothing at all.
        assert_eq!(find(b"", b""), Some(0));
        assert!(contains(b"", b""));
    }

    #[test]
    fn u8_at_is_the_one_byte_case() {
        assert_eq!(u8_at(b"AB", 0), Some(b'A'));
        assert_eq!(u8_at(b"AB", 1), Some(b'B'));
        assert_eq!(u8_at(b"AB", 2), None);
    }

    #[test]
    fn a_reader_walks_a_header_front_to_back() {
        let mut r = Reader::new(&[0, 0, 0, 0x10, b'f', b't', b'y', b'p', 0xFF]);
        assert_eq!(r.position(), 0);
        assert_eq!(r.u32_be(), Some(16));
        assert_eq!(r.position(), 4);
        assert_eq!(r.array::<4>(), Some(*b"ftyp"));
        assert_eq!(r.remaining(), 1);
        assert_eq!(r.u8(), Some(0xFF));
        assert!(r.is_empty());
        assert_eq!(r.u8(), None);
    }

    #[test]
    fn a_read_that_does_not_fit_leaves_the_position_where_it_was() {
        // A reader that consumed a partial field on failure would make the
        // caller's next read start mid-field, which is a corruption rather
        // than an error.
        let mut r = Reader::new(&[1, 2, 3]);
        assert_eq!(r.u32_be(), None);
        assert_eq!(r.position(), 0);
        assert_eq!(r.take(4), None);
        assert_eq!(r.position(), 0);
        assert_eq!(r.array::<9>(), None);
        assert_eq!(r.position(), 0);
        assert_eq!(r.u16_be(), Some(0x0102));
        assert_eq!(r.position(), 2);
    }

    #[test]
    fn skipping_past_the_end_fails_instead_of_landing_outside() {
        let mut r = Reader::new(&[1, 2, 3, 4]);
        assert_eq!(r.skip(5), None);
        assert_eq!(r.position(), 0);
        assert_eq!(r.skip(4), Some(()));
        assert!(r.is_empty());
        assert_eq!(r.skip(usize::MAX), None);
    }

    #[test]
    fn seeking_rejects_a_target_past_the_end_but_starting_there_clamps() {
        // `at` is for an offset computed from the file, where "nothing there"
        // is the useful answer; `seek` is for looking up a structure the
        // caller believes exists, where absence is information.
        let data = [1u8, 2, 3];
        let clamped = Reader::at(&data, 99);
        assert_eq!(clamped.position(), 3);
        assert!(clamped.is_empty());

        let mut r = Reader::new(&data);
        assert_eq!(r.seek(99), None);
        assert_eq!(r.position(), 0);
        assert_eq!(r.seek(3), Some(()));
        assert_eq!(r.position(), 3);
    }

    #[test]
    fn peeking_does_not_consume() {
        let r = Reader::new(b"ABCDEF");
        assert_eq!(r.peek::<2>(0), Some(*b"AB"));
        assert_eq!(r.peek::<2>(4), Some(*b"EF"));
        assert_eq!(r.peek::<2>(5), None);
        assert_eq!(r.peek::<2>(usize::MAX), None);
        assert_eq!(r.position(), 0);
    }

    #[test]
    fn rest_is_what_take_would_return_and_never_panics() {
        let mut r = Reader::new(b"ABCD");
        assert_eq!(r.rest(), b"ABCD");
        r.skip(2).unwrap();
        assert_eq!(r.rest(), b"CD");
        r.skip(2).unwrap();
        assert_eq!(r.rest(), b"");
    }

    #[test]
    fn a_reader_over_nothing_answers_every_question_without_panicking() {
        let mut r = Reader::new(&[]);
        assert!(r.is_empty());
        assert_eq!(r.remaining(), 0);
        assert_eq!(r.rest(), b"");
        assert_eq!(r.u8(), None);
        assert_eq!(r.u32_le(), None);
        assert_eq!(r.take(0), Some(&[][..]));
        assert_eq!(r.array::<0>(), Some([]));
    }
}
