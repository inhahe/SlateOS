//! The decode cursor, and the only place in the crate that indexes a wire buffer.
//!
//! # Why this is its own module
//!
//! `Reader` used to live in `lib.rs`, and its fields carried no `pub`. That
//! reads like encapsulation and is not: a field private to the crate root is
//! visible to every descendant module, so `control`, `input`, `scene`,
//! `submit` and `lib` itself could all reach past the checked accessors
//! straight into `buf` and `pos` — and every one of them did, with the same
//! four lines:
//!
//! ```ignore
//! let magic = [r.buf[0], r.buf[1], r.buf[2], r.buf[3]];
//! if magic != MAGIC { return Err(DecodeError::BadMagic); }
//! r.pos = 4;
//! ```
//!
//! Five copies of a bounds-check-free read, in a decoder whose module
//! documentation promises that "all payload reads are bounds-checked". They
//! happened to be safe, because each was preceded by a `need(HEADER_LEN)`
//! several lines earlier — a proof that lived in a different statement from
//! the code it justified, and that a later edit was free to invalidate
//! silently.
//!
//! Moving the type into a sibling module makes the fields private in the sense
//! the original code intended: a reach-around is now a compile error rather
//! than a lint, so the pattern cannot grow back. Every read in the crate goes
//! through [`Reader::take`], which is the single place where a bound is
//! checked and the single place where the cursor moves.

use crate::DecodeError;

/// Reader cursor over a byte slice, with bounds-checked primitive reads.
///
/// The cursor never passes the end of the buffer: [`Reader::take`] is the only
/// method that advances it, and it advances only after the range has been
/// confirmed to exist. Everything else is expressed in terms of `take`, so
/// there is one bounds check to audit rather than one per primitive.
pub(crate) struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub(crate) fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    /// How many bytes are left.
    ///
    /// `saturating_sub` is a formality given the invariant that `pos <= len`,
    /// but stating it here costs nothing and means the invariant being wrong
    /// would produce a short read rather than a panic in a decoder that is
    /// explicitly documented not to panic on malformed input.
    pub(crate) fn remaining(&self) -> usize {
        self.buf.len().saturating_sub(self.pos)
    }

    /// How far in the cursor has come — i.e. the number of bytes consumed.
    ///
    /// Callers return this as the "bytes consumed" half of a decode result.
    pub(crate) fn position(&self) -> usize {
        self.pos
    }

    /// Error unless at least `n` bytes remain.
    ///
    /// Used to reject a truncated frame up front, before any field is read, so
    /// that a short buffer reports `UnexpectedEof` rather than whatever the
    /// first few bytes happen to decode to.
    pub(crate) fn need(&self, n: usize) -> Result<(), DecodeError> {
        if self.remaining() < n {
            Err(DecodeError::UnexpectedEof)
        } else {
            Ok(())
        }
    }

    /// The unread remainder, without advancing.
    ///
    /// For the two places that hand the tail to another decoder and then
    /// advance by however much it reported consuming.
    pub(crate) fn rest(&self) -> &'a [u8] {
        // `get` rather than `&self.buf[self.pos..]`: the invariant says this is
        // always `Some`, and an empty tail is the honest answer if it ever is
        // not.
        self.buf.get(self.pos..).unwrap_or(&[])
    }

    /// Take the next `n` bytes and advance past them.
    ///
    /// **This is the crate's only bounds check on wire data.** Every other read
    /// is written in terms of it, so an out-of-range access cannot be
    /// introduced without changing this function.
    pub(crate) fn take(&mut self, n: usize) -> Result<&'a [u8], DecodeError> {
        // `checked_add` before the slice: `pos + n` overflowing would wrap to a
        // small number and could then name a range that exists, turning a
        // nonsense length into a successful read of the wrong bytes. A length
        // this large can only come from a corrupt frame, which is exactly what
        // `UnexpectedEof` means here — the frame claims more than it has.
        let end = self.pos.checked_add(n).ok_or(DecodeError::UnexpectedEof)?;
        let slice = self
            .buf
            .get(self.pos..end)
            .ok_or(DecodeError::UnexpectedEof)?;
        self.pos = end;
        Ok(slice)
    }

    /// Take exactly `N` bytes as a fixed-size array.
    ///
    /// The array form is what lets the primitive readers below destructure
    /// their bytes (`let [r, g, b, a] = ...`) instead of indexing them, which
    /// is why none of them needs a bounds check of its own.
    pub(crate) fn take_array<const N: usize>(&mut self) -> Result<[u8; N], DecodeError> {
        let slice = self.take(N)?;
        // Infallible in practice — `take` returned exactly `N` bytes or an
        // error — but expressed as a conversion rather than an `unwrap` so the
        // decoder keeps its no-panic guarantee by construction.
        <[u8; N]>::try_from(slice).map_err(|_| DecodeError::UnexpectedEof)
    }

    /// Skip `n` bytes, e.g. after a nested decoder reported what it consumed.
    pub(crate) fn advance(&mut self, n: usize) -> Result<(), DecodeError> {
        self.take(n).map(|_| ())
    }

    pub(crate) fn read_u8(&mut self) -> Result<u8, DecodeError> {
        let [v] = self.take_array::<1>()?;
        Ok(v)
    }

    pub(crate) fn read_u32(&mut self) -> Result<u32, DecodeError> {
        Ok(u32::from_le_bytes(self.take_array::<4>()?))
    }

    /// The inverse of `crate::write_i32`: the four bytes a `u32` occupies,
    /// reinterpreted as two's complement. Every bit pattern is a valid `i32`,
    /// so this cannot fail for a reason a `u32` read would not.
    pub(crate) fn read_i32(&mut self) -> Result<i32, DecodeError> {
        Ok(self.read_u32()?.cast_signed())
    }

    pub(crate) fn read_u64(&mut self) -> Result<u64, DecodeError> {
        Ok(u64::from_le_bytes(self.take_array::<8>()?))
    }

    pub(crate) fn read_f32(&mut self) -> Result<f32, DecodeError> {
        Ok(f32::from_bits(u32::from_le_bytes(self.take_array::<4>()?)))
    }

    pub(crate) fn read_color(&mut self) -> Result<crate::Color, DecodeError> {
        let [r, g, b, a] = self.take_array::<4>()?;
        Ok(crate::Color { r, g, b, a })
    }

    pub(crate) fn read_radii(&mut self) -> Result<crate::CornerRadii, DecodeError> {
        let tl = self.read_f32()?;
        let tr = self.read_f32()?;
        let br = self.read_f32()?;
        let bl = self.read_f32()?;
        Ok(crate::CornerRadii {
            top_left: tl,
            top_right: tr,
            bottom_right: br,
            bottom_left: bl,
        })
    }

    pub(crate) fn read_string(&mut self) -> Result<String, DecodeError> {
        let len = self.read_u32()?;
        // Checked before `take` so that an absurd length is reported as the
        // protocol violation it is, rather than as a truncated frame — the
        // caller's recovery differs: `StringTooLarge` is fatal, `UnexpectedEof`
        // means "read more and try again".
        if len > crate::MAX_STRING_LEN {
            return Err(DecodeError::StringTooLarge(len));
        }
        let slice = self.take(len as usize)?;
        Ok(core::str::from_utf8(slice)
            .map_err(|_| DecodeError::BadUtf8)?
            .to_string())
    }

    pub(crate) fn read_optional_f32(&mut self) -> Result<Option<f32>, DecodeError> {
        let tag = self.read_u8()?;
        match tag {
            0 => Ok(None),
            1 => Ok(Some(self.read_f32()?)),
            other => Err(DecodeError::BadTag(other)),
        }
    }

    /// Read a four-byte magic and check it, in one step.
    ///
    /// Every frame type in this crate begins with one, and each used to open
    /// its decoder with its own copy of the read-compare-then-`pos = 4`
    /// sequence. Sharing it means the check advances the cursor as a
    /// consequence of having read the magic, rather than by a separate
    /// assignment that has to be kept in step with the four bytes above it.
    pub(crate) fn expect_magic(&mut self, magic: [u8; 4]) -> Result<(), DecodeError> {
        if self.take_array::<4>()? == magic {
            Ok(())
        } else {
            Err(DecodeError::BadMagic)
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::indexing_slicing, clippy::unwrap_used, clippy::panic)]

    use super::Reader;
    use crate::DecodeError;

    #[test]
    fn take_past_the_end_reports_eof_and_does_not_move_the_cursor() {
        let mut r = Reader::new(&[1, 2, 3]);
        assert!(matches!(r.take(4), Err(DecodeError::UnexpectedEof)));
        // The failed read must not have consumed anything, or a caller that
        // retries with a longer buffer would resume in the wrong place.
        assert_eq!(r.position(), 0);
        assert_eq!(r.remaining(), 3);
    }

    #[test]
    fn a_length_that_overflows_the_cursor_is_an_error_not_a_wrap() {
        let mut r = Reader::new(&[1, 2, 3, 4]);
        r.take(2).unwrap();
        // `pos + n` would wrap to 1 and name a range that exists. If `take`
        // ever returns `Ok` here, a corrupt length has become a silent read of
        // the wrong bytes.
        assert!(matches!(
            r.take(usize::MAX),
            Err(DecodeError::UnexpectedEof)
        ));
        assert_eq!(r.position(), 2);
    }

    #[test]
    fn reads_are_little_endian_and_consume_exactly_their_width() {
        let mut r = Reader::new(&[0x01, 0x02, 0x03, 0x04, 0x05]);
        assert_eq!(r.read_u32().unwrap(), 0x0403_0201);
        assert_eq!(r.position(), 4);
        assert_eq!(r.read_u8().unwrap(), 0x05);
        assert_eq!(r.remaining(), 0);
    }

    #[test]
    fn rest_is_the_unread_tail_and_does_not_advance() {
        let mut r = Reader::new(&[9, 8, 7, 6]);
        r.take(1).unwrap();
        assert_eq!(r.rest(), &[8, 7, 6]);
        assert_eq!(r.position(), 1, "rest() must not consume");
        r.advance(2).unwrap();
        assert_eq!(r.rest(), &[6]);
    }

    #[test]
    fn expect_magic_consumes_on_success_and_reports_a_mismatch() {
        let mut ok = Reader::new(b"SLTE\xAA");
        ok.expect_magic(*b"SLTE").unwrap();
        assert_eq!(ok.position(), 4, "a matched magic is consumed");

        let mut bad = Reader::new(b"XXXX\xAA");
        assert!(matches!(
            bad.expect_magic(*b"SLTE"),
            Err(DecodeError::BadMagic)
        ));

        // Too short to hold a magic at all: that is a truncated frame, not a
        // corrupt one, and the two have different recoveries upstream.
        let mut short = Reader::new(b"SL");
        assert!(matches!(
            short.expect_magic(*b"SLTE"),
            Err(DecodeError::UnexpectedEof)
        ));
    }

    #[test]
    fn a_string_longer_than_the_cap_is_refused_before_it_is_read() {
        // Length prefix of MAX_STRING_LEN + 1, with no payload behind it. The
        // cap must be checked first, or this would report EOF and a caller
        // would sit waiting for bytes that are never coming.
        let len = crate::MAX_STRING_LEN.saturating_add(1);
        let bytes = len.to_le_bytes();
        let mut r = Reader::new(&bytes);
        assert!(matches!(
            r.read_string(),
            Err(DecodeError::StringTooLarge(_))
        ));
    }
}
