//! EBML, the binary format Matroska is written in (RFC 8794): a tree of
//! elements, each an ID, a size and that many bytes of data -- a number, a
//! string, bytes, or more elements.
//!
//! IDs and sizes are *variable-size integers*: the count of leading zero bits
//! in the first byte, plus one, is the integer's length in bytes, and the bit
//! after them (the marker) is not part of the value. An ID keeps its marker,
//! as the specifications write IDs (`0x1A45DFA3`); a size drops it. A size
//! whose value bits are all ones is *unknown*: the element runs until
//! something that cannot be inside it begins, which Matroska allows only for
//! a Segment and a Cluster (a live stream's, written before its length is
//! known).
//!
//! This reads from a seekable source and keeps its own position, so that a
//! caller can always say where an element began; and every read is bounded
//! by the source's length, so that a size claiming more than the file holds
//! is an error, not an allocation.

use std::io::{self, BufReader, Read, Seek, SeekFrom};

use crate::Error;

/// An element's ID, its marker bits kept.
pub type Id = u32;

/// An element's data size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Size {
    Known(u64),
    /// All value bits set: the element ends where something that cannot be
    /// inside it begins, or at the end of the source.
    Unknown,
}

/// An element's header: what it is, how big, and where.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    pub id: Id,
    pub size: Size,
    /// Where the element's ID begins.
    pub start: u64,
    /// Where its data begins.
    pub data: u64,
}

impl Header {
    /// Where the element ends, if its size is known.
    pub fn end(&self) -> Option<u64> {
        match self.size {
            Size::Known(n) => self.data.checked_add(n),
            Size::Unknown => None,
        }
    }
}

/// The longest string this reads: FFmpeg's limit, 16 MiB.
pub const MAX_STRING: u64 = 0x100_0000;
/// The longest binary element this reads into memory: FFmpeg's, 256 MiB.
pub const MAX_BINARY: u64 = 0x1000_0000;

/// A variable-size integer's length in bytes, from its first byte: 1 to 8,
/// or `None` for a first byte of 0, which no valid integer has.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "a nonzero byte has at most 7 leading zeros"
)]
pub fn vint_len(first: u8) -> Option<u32> {
    (first != 0).then(|| first.leading_zeros() + 1)
}

/// Reads EBML from a seekable source, keeping its position.
pub struct Reader<R> {
    inner: BufReader<R>,
    pos: u64,
    len: u64,
}

impl<R: Read + Seek> Reader<R> {
    /// A reader at the start of `source`.
    ///
    /// # Errors
    ///
    /// When the source cannot be measured or rewound.
    pub fn new(mut source: R) -> Result<Self, Error> {
        let len = source.seek(SeekFrom::End(0))?;
        source.seek(SeekFrom::Start(0))?;
        Ok(Self {
            inner: BufReader::with_capacity(64 * 1024, source),
            pos: 0,
            len,
        })
    }

    /// The position of the next byte.
    pub fn pos(&self) -> u64 {
        self.pos
    }

    /// The source's length.
    pub fn len(&self) -> u64 {
        self.len
    }

    /// How many bytes are left.
    pub fn remaining(&self) -> u64 {
        self.len.saturating_sub(self.pos)
    }

    /// Move to `pos`, which must lie within the source.
    ///
    /// # Errors
    ///
    /// When `pos` is past the end, or the source cannot seek.
    pub fn seek_to(&mut self, pos: u64) -> Result<(), Error> {
        if pos > self.len {
            return Err(Error::Truncated);
        }
        let delta = i64::try_from(pos).ok().zip(i64::try_from(self.pos).ok());
        match delta {
            // Within reach of the buffer: keep it.
            Some((to, from)) => self.inner.seek_relative(to.wrapping_sub(from))?,
            None => {
                self.inner.seek(SeekFrom::Start(pos))?;
            }
        }
        self.pos = pos;
        Ok(())
    }

    /// Fill `buf` from the current position.
    ///
    /// # Errors
    ///
    /// [`Error::Truncated`] when the source holds fewer bytes.
    pub fn read_into(&mut self, buf: &mut [u8]) -> Result<(), Error> {
        self.read_exact(buf)
    }

    /// Fill `buf`, or fail with [`Error::Truncated`].
    fn read_exact(&mut self, buf: &mut [u8]) -> Result<(), Error> {
        let n = u64::try_from(buf.len()).map_err(|_| Error::Truncated)?;
        if n > self.remaining() {
            return Err(Error::Truncated);
        }
        self.inner.read_exact(buf).map_err(|e| match e.kind() {
            io::ErrorKind::UnexpectedEof => Error::Truncated,
            kind => Error::Io(kind),
        })?;
        self.pos = self.pos.saturating_add(n);
        Ok(())
    }

    /// One byte.
    fn byte(&mut self) -> Result<u8, Error> {
        let mut b = [0u8];
        self.read_exact(&mut b)?;
        Ok(b[0])
    }

    /// A variable-size integer of at most `max` bytes: its length, and its
    /// value with the marker kept (`keep_marker`, for an ID) or dropped.
    fn vint(&mut self, max: u32, keep_marker: bool) -> Result<(u32, u64), Error> {
        let first = self.byte()?;
        let len = vint_len(first)
            .filter(|&n| n <= max)
            .ok_or(Error::Invalid("an EBML number's first byte"))?;
        let mut value = u64::from(if keep_marker {
            first
        } else {
            first & value_mask(len)
        });
        for _ in 1..len {
            value = (value << 8) | u64::from(self.byte()?);
        }
        Ok((len, value))
    }

    /// The next element's header, or `None` at the end of the source.
    ///
    /// # Errors
    ///
    /// When the header is damaged or cut short.
    #[allow(
        clippy::arithmetic_side_effects,
        reason = "an ID is 1 to 4 bytes and a size 1 to 8, so the value bits are 7 to 56: every shift is in range and every mask at least 1"
    )]
    pub fn header(&mut self) -> Result<Option<Header>, Error> {
        if self.remaining() == 0 {
            return Ok(None);
        }
        let start = self.pos;
        let (id_len, id) = self.vint(4, true)?;
        // An ID whose value bits are all ones or all zeros is reserved.
        let value_bits = id_len * 7;
        let value = id & ((1u64 << value_bits) - 1);
        if value == 0 || value == (1u64 << value_bits) - 1 {
            return Err(Error::Invalid("a reserved EBML ID"));
        }
        let (size_len, size) = self.vint(8, false)?;
        let size = if size == (1u64 << (size_len * 7)) - 1 {
            Size::Unknown
        } else {
            Size::Known(size)
        };
        Ok(Some(Header {
            id: u32::try_from(id).map_err(|_| Error::Invalid("an EBML ID"))?,
            size,
            start,
            data: self.pos,
        }))
    }

    /// A known size no larger than `max`.
    fn bounded(size: Size, max: u64, what: &'static str) -> Result<u64, Error> {
        match size {
            Size::Known(n) if n <= max => Ok(n),
            _ => Err(Error::Invalid(what)),
        }
    }

    /// An unsigned integer element's value: 0 to 8 bytes, big-endian; empty
    /// is `default`.
    ///
    /// # Errors
    ///
    /// When it is longer than 8 bytes, of unknown size, or cut short.
    pub fn uint(&mut self, size: Size, default: u64) -> Result<u64, Error> {
        let n = Self::bounded(size, 8, "an unsigned integer longer than 8 bytes")?;
        if n == 0 {
            return Ok(default);
        }
        let mut v = 0u64;
        for _ in 0..n {
            v = (v << 8) | u64::from(self.byte()?);
        }
        Ok(v)
    }

    /// A signed integer element's value: 0 to 8 bytes, two's complement.
    ///
    /// # Errors
    ///
    /// As [`Self::uint`].
    pub fn sint(&mut self, size: Size, default: i64) -> Result<i64, Error> {
        let n = Self::bounded(size, 8, "a signed integer longer than 8 bytes")?;
        if n == 0 {
            return Ok(default);
        }
        let mut v = i64::from(self.byte()?.cast_signed());
        for _ in 1..n {
            v = (v << 8) | i64::from(self.byte()?);
        }
        Ok(v)
    }

    /// A float element's value: 0, 4 or 8 bytes.
    ///
    /// # Errors
    ///
    /// When it is another length, or cut short.
    pub fn float(&mut self, size: Size, default: f64) -> Result<f64, Error> {
        match size {
            Size::Known(0) => Ok(default),
            Size::Known(4) => {
                let mut b = [0u8; 4];
                self.read_exact(&mut b)?;
                Ok(f64::from(f32::from_be_bytes(b)))
            }
            Size::Known(8) => {
                let mut b = [0u8; 8];
                self.read_exact(&mut b)?;
                Ok(f64::from_be_bytes(b))
            }
            _ => Err(Error::Invalid("a float of another length than 0, 4 or 8")),
        }
    }

    /// A string element's bytes, up to its first NUL (EBML pads strings with
    /// zeros); `None` if it is empty, for the caller's default.
    ///
    /// # Errors
    ///
    /// When it is over 16 MiB, of unknown size, or cut short.
    pub fn string(&mut self, size: Size) -> Result<Option<Vec<u8>>, Error> {
        let n = Self::bounded(size, MAX_STRING, "a string over 16 MiB")?;
        if n == 0 {
            return Ok(None);
        }
        let mut s = self.binary(Size::Known(n), MAX_STRING)?;
        if let Some(nul) = s.iter().position(|&b| b == 0) {
            s.truncate(nul);
        }
        Ok(Some(s))
    }

    /// Each child of `parent`, a master element of known size, in turn: `f`
    /// reads as much of the child as it wants, and the reader then moves to
    /// the child's end. A child of unknown size, or one running past its
    /// parent, is an error.
    ///
    /// # Errors
    ///
    /// When `parent` is of unknown size or runs past the source, a child is
    /// damaged, or `f` fails.
    pub fn children(
        &mut self,
        parent: &Header,
        mut f: impl FnMut(&mut Self, &Header) -> Result<(), Error>,
    ) -> Result<(), Error> {
        let end = parent
            .end()
            .ok_or(Error::Invalid("a master element of unknown size"))?;
        if end > self.len {
            return Err(Error::Truncated);
        }
        self.seek_to(parent.data)?;
        while self.pos < end {
            let child = self.header()?.ok_or(Error::Truncated)?;
            let child_end = child.end().ok_or(Error::Invalid(
                "an element of unknown size inside a sized one",
            ))?;
            if child_end > end {
                return Err(Error::Invalid("an element running past its parent"));
            }
            f(self, &child)?;
            self.seek_to(child_end)?;
        }
        Ok(())
    }

    /// A binary element's bytes, refused past `max`.
    ///
    /// # Errors
    ///
    /// When it is over `max`, of unknown size, or more than the source
    /// holds.
    pub fn binary(&mut self, size: Size, max: u64) -> Result<Vec<u8>, Error> {
        let n = Self::bounded(size, max, "a binary element too large")?;
        if n > self.remaining() {
            return Err(Error::Truncated);
        }
        let mut v = vec![0u8; usize::try_from(n).map_err(|_| Error::Truncated)?];
        self.read_exact(&mut v)?;
        Ok(v)
    }
}

/// A variable-size integer at the start of `bytes`, as Matroska codes a
/// block's track number and its EBML lacing: its length and value (marker
/// dropped), or `None` if `bytes` does not hold one.
pub fn vint_in(bytes: &[u8]) -> Option<(usize, u64)> {
    let first = *bytes.first()?;
    let n = vint_len(first)?;
    let len = usize::try_from(n).ok()?;
    let rest = bytes.get(1..len)?;
    let mut v = u64::from(first & value_mask(n));
    for &b in rest {
        v = (v << 8) | u64::from(b);
    }
    Some((len, v))
}

/// The value bits of a `len`-byte integer's first byte: those after its
/// marker. An 8-byte integer's first byte has none.
fn value_mask(len: u32) -> u8 {
    0xffu8.checked_shr(len).unwrap_or(0)
}

/// A signed variable-size integer at the start of `bytes`, as EBML lacing
/// codes the difference between two laces' sizes: the unsigned value less
/// half its range.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "an integer of 1 to 8 bytes has 7 to 56 value bits: the bias is under 2^55 and the value under 2^56, so neither the shift nor the difference overflows"
)]
pub fn svint_in(bytes: &[u8]) -> Option<(usize, i64)> {
    let (len, v) = vint_in(bytes)?;
    let bias = (1i64 << (7 * len - 1)) - 1;
    Some((len, i64::try_from(v).ok()? - bias))
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        clippy::float_cmp,
        reason = "a test: a failure should be loud, and the floats are exact"
    )]

    use super::*;
    use std::io::Cursor;

    fn reader(bytes: &[u8]) -> Reader<Cursor<Vec<u8>>> {
        Reader::new(Cursor::new(bytes.to_vec())).unwrap()
    }

    #[test]
    fn a_header_is_an_id_and_a_size() {
        // The EBML header's ID, and a one-byte size of 35.
        let mut r = reader(&[0x1a, 0x45, 0xdf, 0xa3, 0xa3]);
        let h = r.header().unwrap().unwrap();
        assert_eq!(h.id, 0x1a45_dfa3);
        assert_eq!(h.size, Size::Known(35));
        assert_eq!((h.start, h.data), (0, 5));
        assert_eq!(r.header().unwrap(), None, "the end of the source");
    }

    #[test]
    fn all_ones_is_an_unknown_size_at_any_length() {
        for size in [
            &[0xffu8][..],
            &[0x7f, 0xff],
            &[0x01, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff],
        ] {
            let mut bytes = vec![0x1f, 0x43, 0xb6, 0x75];
            bytes.extend_from_slice(size);
            let h = reader(&bytes).header().unwrap().unwrap();
            assert_eq!(h.size, Size::Unknown, "{size:?}");
        }
        // Not all ones: a size.
        let h = reader(&[0x1f, 0x43, 0xb6, 0x75, 0x7f, 0xfe])
            .header()
            .unwrap()
            .unwrap();
        assert_eq!(h.size, Size::Known(0x3ffe));
    }

    #[test]
    fn a_damaged_header_is_an_error() {
        assert!(reader(&[0x00, 0x81]).header().is_err(), "a zero first byte");
        assert!(
            reader(&[0x08, 1, 2, 3, 4, 0x81]).header().is_err(),
            "a 5-byte ID"
        );
        assert!(reader(&[0xff, 0x81]).header().is_err(), "a reserved ID");
        assert!(reader(&[0x1a, 0x45]).header().is_err(), "cut short");
    }

    #[test]
    fn values_read_as_ebml_writes_them() {
        let mut r = reader(&[0x01, 0x00, 0xff, 0xfe, 0x3f, 0x80, 0, 0, b'a', b'b', 0, 0]);
        assert_eq!(r.uint(Size::Known(2), 9).unwrap(), 0x100);
        assert_eq!(r.sint(Size::Known(2), 9).unwrap(), -2);
        assert_eq!(r.float(Size::Known(4), 9.0).unwrap(), 1.0);
        assert_eq!(
            r.string(Size::Known(4)).unwrap().unwrap(),
            b"ab",
            "zero-padded"
        );
        assert_eq!(r.uint(Size::Known(0), 7).unwrap(), 7, "empty: the default");
        assert!(r.uint(Size::Known(9), 0).is_err(), "too long");
        assert!(r.float(Size::Known(3), 0.0).is_err());
    }

    #[test]
    fn a_size_past_the_end_is_refused_without_allocating() {
        let mut r = reader(&[1, 2, 3]);
        assert_eq!(
            r.binary(Size::Known(1 << 27), MAX_BINARY),
            Err(Error::Truncated)
        );
        // A size no allocation could meet: refused before one is tried,
        // which would abort the process.
        assert_eq!(
            r.binary(Size::Known(1 << 62), u64::MAX),
            Err(Error::Truncated)
        );
        assert!(r.seek_to(4).is_err(), "past the end");
    }

    #[test]
    fn a_child_running_past_its_parent_is_an_error() {
        // A parent of 4 bytes (0x84) holding a child that claims 5 (0x85).
        let mut r = reader(&[
            0x1a, 0x45, 0xdf, 0xa3, 0x84, 0x42, 0x82, 0x85, b'w', 0, 0, 0, 0,
        ]);
        let parent = r.header().unwrap().unwrap();
        assert_eq!(
            r.children(&parent, |_, _| Ok(())),
            Err(Error::Invalid("an element running past its parent"))
        );
        // The same child inside a parent that holds it.
        let mut r = reader(&[
            0x1a, 0x45, 0xdf, 0xa3, 0x88, 0x42, 0x82, 0x85, b'w', 0, 0, 0, 0,
        ]);
        let parent = r.header().unwrap().unwrap();
        let mut seen = Vec::new();
        r.children(&parent, |_, c| {
            seen.push(c.id);
            Ok(())
        })
        .unwrap();
        assert_eq!(seen, [0x4282]);
    }

    #[test]
    fn lacing_numbers_read_from_bytes() {
        assert_eq!(vint_in(&[0x81]), Some((1, 1)));
        assert_eq!(vint_in(&[0x40, 0x02]), Some((2, 2)));
        assert_eq!(vint_in(&[0x40]), None, "cut short");
        // Signed: 0xbf is 63 - 63 = 0; 0x80 is 0 - 63.
        assert_eq!(svint_in(&[0xbf]), Some((1, 0)));
        assert_eq!(svint_in(&[0x80]), Some((1, -63)));
        assert_eq!(svint_in(&[0x5f, 0xff]), Some((2, 0)));
    }
}
