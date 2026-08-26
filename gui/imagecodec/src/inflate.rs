//! DEFLATE (RFC 1951) and zlib (RFC 1950) decompression.
//!
//! PNG's pixel data is a zlib stream and nothing else, so a PNG decoder that
//! cannot inflate is not a PNG decoder. This is that half, kept in its own
//! module because it has nothing to do with pictures and because the next
//! format that needs it (a `.zip`, a `.gz`, an OpenType `WOFF`) should be able
//! to take it without taking a PNG parser as well.
//!
//! # The two rules this module exists to keep
//!
//! Both come from the input being a file somebody else wrote.
//!
//! 1. **It never panics.** Every index is `get`, every arithmetic operation on
//!    a value the stream chose is checked or saturating. A corrupt wallpaper
//!    must be an error, not a dead shell.
//! 2. **The caller says how much output it will accept, before any is
//!    produced.** DEFLATE's expansion ratio is over 1000:1, so a few kilobytes
//!    of hostile input can ask for gigabytes — the "zip bomb". Every
//!    entry point here takes a `limit` and stops at it. The limit is not a
//!    guess: a PNG's decompressed size is *exactly* computable from its header,
//!    so the caller passes the number the header implies and a stream that
//!    wants more is, by definition, lying about one of the two.
//!
//! # Why not the kernel's copy
//!
//! `kernel/src/fs/compress.rs` has an inflate already. It is not reachable:
//! the kernel is a bare-metal binary crate, not a library a userspace GUI crate
//! can depend on. Unifying them means promoting one to a shared leaf crate at
//! the workspace root, which is a cross-lane change — see
//! `requests/c-a-two-inflates.md`.
//!
//! # Shape
//!
//! Canonical Huffman decoding is the counts-and-symbols form from Mark Adler's
//! `puff.c` rather than a lookup table: it decodes a bit at a time, which is
//! slower per symbol but has no table to build, no table to size wrongly, and
//! no way to read past the end of one. For pictures — decoded once and then
//! kept — that is the right side of the trade.

use alloc::vec;
use alloc::vec::Vec;
use core::fmt;

/// Why a compressed stream could not be read.
///
/// Every variant means "this input is not a valid stream", never "this decoder
/// gave up": there is no truncation, no partial result and no fallback.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InflateError {
    /// The stream ended in the middle of something.
    UnexpectedEnd,
    /// A block header named block type 3, which RFC 1951 reserves.
    ReservedBlockType,
    /// A stored block's length and its one's-complement disagree, which means
    /// the bytes are not what the writer wrote.
    StoredLengthMismatch,
    /// A Huffman code table is over-subscribed or otherwise not a valid
    /// canonical code.
    BadCodeLengths,
    /// A symbol was decoded that the table has no entry for.
    BadSymbol,
    /// A back-reference points further back than the output produced so far.
    /// Legal DEFLATE never does this; it is the classic sign of a stream
    /// assembled by hand.
    DistanceTooFar,
    /// The output would exceed the caller's limit. See the module docs.
    OutputTooLarge,
    /// The two-byte zlib header is not a zlib header, or declares a window
    /// size or a preset dictionary this decoder does not implement.
    BadZlibHeader,
    /// The zlib trailer's Adler-32 does not match the data that preceded it.
    ChecksumMismatch,
}

impl fmt::Display for InflateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::UnexpectedEnd => "compressed stream ended mid-symbol",
            Self::ReservedBlockType => "reserved DEFLATE block type 3",
            Self::StoredLengthMismatch => "stored block length does not match its complement",
            Self::BadCodeLengths => "invalid Huffman code lengths",
            Self::BadSymbol => "undecodable Huffman symbol",
            Self::DistanceTooFar => "back-reference points before the start of the output",
            Self::OutputTooLarge => "decompressed size exceeds the caller's limit",
            Self::BadZlibHeader => "not a zlib stream this decoder supports",
            Self::ChecksumMismatch => "zlib Adler-32 checksum mismatch",
        };
        f.write_str(s)
    }
}

/// Longest Huffman code DEFLATE permits.
const MAX_BITS: usize = 15;

/// The order RFC 1951 §3.2.7 lists code-length code lengths in.
///
/// Not sorted: the lengths for the symbols that are usually zero come last, so
/// a stream can stop early and leave them implicitly zero.
const CODE_LENGTH_ORDER: [usize; 19] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
];

/// Base match length for length symbols 257..=285 (RFC 1951 §3.2.5).
const LENGTH_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];

/// Extra bits read after each length symbol.
const LENGTH_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];

/// Base back-reference distance for distance symbols 0..=29.
const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];

/// Extra bits read after each distance symbol.
const DIST_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];

/// A least-significant-bit-first bit reader over a byte slice.
///
/// DEFLATE packs bits from the low end of each byte upward, which is the
/// opposite of every network protocol on this tree's wire, so this is
/// deliberately a separate reader rather than a reuse of one.
struct BitReader<'a> {
    data: &'a [u8],
    /// Index of the next byte to pull into the accumulator.
    pos: usize,
    /// Bits already pulled and not yet consumed, low bit first.
    acc: u32,
    /// How many bits in `acc` are live.
    live: u32,
}

impl<'a> BitReader<'a> {
    const fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            pos: 0,
            acc: 0,
            live: 0,
        }
    }

    /// Read `need` bits, low bit first. `need` is at most 16 at every call
    /// site, which is what keeps `acc` (at most `need - 1 + 8` live bits) from
    /// overflowing its 32 bits.
    fn bits(&mut self, need: u32) -> Result<u32, InflateError> {
        while self.live < need {
            let byte = *self.data.get(self.pos).ok_or(InflateError::UnexpectedEnd)?;
            self.pos = self.pos.saturating_add(1);
            self.acc |= u32::from(byte) << self.live;
            self.live = self.live.saturating_add(8);
        }
        // `need <= 16 < 32`, so the shift is always defined.
        let mask = (1u32 << need).saturating_sub(1);
        let out = self.acc & mask;
        self.acc >>= need;
        self.live = self.live.saturating_sub(need);
        Ok(out)
    }

    /// Drop the rest of the current byte. Stored blocks are byte-aligned.
    const fn align(&mut self) {
        let drop = self.live % 8;
        self.acc >>= drop;
        self.live = self.live.saturating_sub(drop);
    }

    /// Take `n` whole bytes, which requires the reader to be byte-aligned.
    fn bytes(&mut self, n: usize) -> Result<&'a [u8], InflateError> {
        // Bits already in the accumulator are bytes already consumed from
        // `data`, so rewind past them rather than reading them twice.
        let buffered = (self.live / 8) as usize;
        let start = self.pos.saturating_sub(buffered);
        let end = start.checked_add(n).ok_or(InflateError::UnexpectedEnd)?;
        let out = self
            .data
            .get(start..end)
            .ok_or(InflateError::UnexpectedEnd)?;
        self.pos = end;
        self.acc = 0;
        self.live = 0;
        Ok(out)
    }

    /// How many whole bytes of the input have been consumed, rounding a
    /// partially-consumed byte up. Used to find the zlib trailer.
    const fn bytes_consumed(&self) -> usize {
        let buffered = (self.live / 8) as usize;
        self.pos.saturating_sub(buffered)
    }
}

/// A canonical Huffman code, stored as "how many codes of each length" plus
/// "the symbols in canonical order".
///
/// This is `puff.c`'s representation. It is two small allocations instead of a
/// decode table, and decoding walks one bit at a time comparing against the
/// running count — which cannot index outside `symbols` because the walk stops
/// as soon as the accumulated count covers the code.
struct Huffman {
    /// `counts[n]` is the number of symbols whose code is `n` bits long.
    counts: [u16; MAX_BITS + 1],
    /// Symbols ordered by (code length, symbol value).
    symbols: Vec<u16>,
}

impl Huffman {
    /// Build a code from a per-symbol length table.
    ///
    /// Lengths of zero mean "this symbol has no code". An *incomplete* code —
    /// one that does not use up the whole code space — is accepted, because
    /// RFC 1951 permits the one-symbol distance code that a stream of literals
    /// with a single back-reference produces. An *over-subscribed* one is not:
    /// it has no consistent assignment at all.
    fn build(lengths: &[u8]) -> Result<Self, InflateError> {
        let mut counts = [0u16; MAX_BITS + 1];
        for &len in lengths {
            let idx = len as usize;
            if idx > MAX_BITS {
                return Err(InflateError::BadCodeLengths);
            }
            if let Some(slot) = counts.get_mut(idx) {
                *slot = slot.saturating_add(1);
            }
        }

        // Kraft inequality: each additional bit doubles the space, each code of
        // that length spends one. Going negative means two symbols were handed
        // the same code.
        let mut left: i32 = 1;
        for len in 1..=MAX_BITS {
            left = left.saturating_mul(2);
            left = left.saturating_sub(i32::from(*counts.get(len).unwrap_or(&0)));
            if left < 0 {
                return Err(InflateError::BadCodeLengths);
            }
        }

        // Where each length's run of symbols starts.
        let mut offsets = [0u16; MAX_BITS + 2];
        for len in 1..=MAX_BITS {
            let prev = *offsets.get(len).unwrap_or(&0);
            let count = *counts.get(len).unwrap_or(&0);
            if let Some(slot) = offsets.get_mut(len.saturating_add(1)) {
                *slot = prev.saturating_add(count);
            }
        }

        let mut symbols = vec![0u16; lengths.len()];
        for (sym, &len) in lengths.iter().enumerate() {
            if len == 0 {
                continue;
            }
            let idx = len as usize;
            let at = *offsets.get(idx).unwrap_or(&0) as usize;
            if let Some(slot) = symbols.get_mut(at) {
                *slot = u16::try_from(sym).unwrap_or(u16::MAX);
            }
            if let Some(slot) = offsets.get_mut(idx) {
                *slot = slot.saturating_add(1);
            }
        }

        Ok(Self { counts, symbols })
    }

    /// Decode one symbol, consuming between 1 and 15 bits.
    fn decode(&self, r: &mut BitReader<'_>) -> Result<u16, InflateError> {
        // `code` is the bits read so far as a number; `first` is the first
        // canonical code of the current length; `index` is where that length's
        // symbols start in `symbols`.
        let mut code: i32 = 0;
        let mut first: i32 = 0;
        let mut index: i32 = 0;

        for len in 1..=MAX_BITS {
            code |= i32::try_from(r.bits(1)?).unwrap_or(0);
            let count = i32::from(*self.counts.get(len).unwrap_or(&0));
            let offset = code.saturating_sub(first);
            if offset < count {
                let at = usize::try_from(index.saturating_add(offset))
                    .map_err(|_| InflateError::BadSymbol)?;
                return self.symbols.get(at).copied().ok_or(InflateError::BadSymbol);
            }
            index = index.saturating_add(count);
            first = first.saturating_add(count).saturating_mul(2);
            code = code.saturating_mul(2);
        }
        Err(InflateError::BadSymbol)
    }

    /// The fixed literal/length code of RFC 1951 §3.2.6.
    fn fixed_literals() -> Result<Self, InflateError> {
        let mut lengths = [0u8; 288];
        for (sym, slot) in lengths.iter_mut().enumerate() {
            *slot = match sym {
                0..=143 => 8,
                144..=255 => 9,
                256..=279 => 7,
                _ => 8,
            };
        }
        Self::build(&lengths)
    }

    /// The fixed distance code: thirty-two five-bit codes, two of which are
    /// never emitted by a valid stream but are present so the code is complete.
    fn fixed_distances() -> Result<Self, InflateError> {
        Self::build(&[5u8; 32])
    }
}

/// Decompress a raw DEFLATE stream.
///
/// `limit` is the most output bytes the caller will accept; see the module
/// docs for why it is not optional.
///
/// # Errors
///
/// [`InflateError`] — every variant means the input is invalid or exceeds
/// `limit`. Never panics, for any input.
pub fn inflate(data: &[u8], limit: usize) -> Result<Vec<u8>, InflateError> {
    let mut r = BitReader::new(data);
    let mut out = Vec::new();
    inflate_into(&mut r, &mut out, limit)?;
    Ok(out)
}

/// The block loop, shared by [`inflate`] and [`zlib_decompress`].
fn inflate_into(
    r: &mut BitReader<'_>,
    out: &mut Vec<u8>,
    limit: usize,
) -> Result<(), InflateError> {
    loop {
        let final_block = r.bits(1)? == 1;
        match r.bits(2)? {
            0 => stored_block(r, out, limit)?,
            1 => {
                let lit = Huffman::fixed_literals()?;
                let dist = Huffman::fixed_distances()?;
                coded_block(r, out, limit, &lit, &dist)?;
            }
            2 => {
                let (lit, dist) = dynamic_tables(r)?;
                coded_block(r, out, limit, &lit, &dist)?;
            }
            _ => return Err(InflateError::ReservedBlockType),
        }
        if final_block {
            return Ok(());
        }
    }
}

/// An uncompressed block: byte-aligned, length-prefixed, copied verbatim.
fn stored_block(
    r: &mut BitReader<'_>,
    out: &mut Vec<u8>,
    limit: usize,
) -> Result<(), InflateError> {
    r.align();
    let header = r.bytes(4)?;
    let len = u16::from_le_bytes([*header.first().unwrap_or(&0), *header.get(1).unwrap_or(&0)]);
    let nlen = u16::from_le_bytes([*header.get(2).unwrap_or(&0), *header.get(3).unwrap_or(&0)]);
    if len != !nlen {
        return Err(InflateError::StoredLengthMismatch);
    }
    let body = r.bytes(len as usize)?;
    if out.len().saturating_add(body.len()) > limit {
        return Err(InflateError::OutputTooLarge);
    }
    out.extend_from_slice(body);
    Ok(())
}

/// A Huffman-coded block, fixed or dynamic: literals and back-references until
/// the end-of-block symbol.
fn coded_block(
    r: &mut BitReader<'_>,
    out: &mut Vec<u8>,
    limit: usize,
    lit: &Huffman,
    dist: &Huffman,
) -> Result<(), InflateError> {
    loop {
        let sym = lit.decode(r)?;
        if sym < 256 {
            if out.len() >= limit {
                return Err(InflateError::OutputTooLarge);
            }
            out.push(sym as u8);
            continue;
        }
        if sym == 256 {
            return Ok(());
        }

        // 257..=285 are length codes; 286 and 287 exist in the fixed table so
        // that it is a complete code, and a stream that emits one is invalid.
        let len_idx = (sym as usize).saturating_sub(257);
        let base = *LENGTH_BASE.get(len_idx).ok_or(InflateError::BadSymbol)?;
        let extra = *LENGTH_EXTRA.get(len_idx).ok_or(InflateError::BadSymbol)?;
        let length = (base as usize).saturating_add(r.bits(u32::from(extra))? as usize);

        let dsym = dist.decode(r)? as usize;
        let dbase = *DIST_BASE.get(dsym).ok_or(InflateError::BadSymbol)?;
        let dextra = *DIST_EXTRA.get(dsym).ok_or(InflateError::BadSymbol)?;
        let distance = (dbase as usize).saturating_add(r.bits(u32::from(dextra))? as usize);

        if distance == 0 || distance > out.len() {
            return Err(InflateError::DistanceTooFar);
        }
        if out.len().saturating_add(length) > limit {
            return Err(InflateError::OutputTooLarge);
        }

        // Byte at a time, deliberately: DEFLATE's back-references may overlap
        // the bytes they are producing (distance 1, length 100 is a run of one
        // byte repeated), so this cannot be a slice copy.
        let mut src = out.len().saturating_sub(distance);
        for _ in 0..length {
            let byte = *out.get(src).ok_or(InflateError::DistanceTooFar)?;
            out.push(byte);
            src = src.saturating_add(1);
        }
    }
}

/// Read the literal/length and distance codes a dynamic block carries in front
/// of itself (RFC 1951 §3.2.7).
fn dynamic_tables(r: &mut BitReader<'_>) -> Result<(Huffman, Huffman), InflateError> {
    let hlit = (r.bits(5)? as usize).saturating_add(257);
    let hdist = (r.bits(5)? as usize).saturating_add(1);
    let hclen = (r.bits(4)? as usize).saturating_add(4);

    // 286 literal/length and 30 distance codes are the most RFC 1951 defines;
    // the five-bit fields can name more, and a stream that does is invalid
    // rather than merely unusual.
    if hlit > 286 || hdist > 30 {
        return Err(InflateError::BadCodeLengths);
    }

    let mut cl_lengths = [0u8; 19];
    for i in 0..hclen {
        let at = *CODE_LENGTH_ORDER
            .get(i)
            .ok_or(InflateError::BadCodeLengths)?;
        let len = u8::try_from(r.bits(3)?).unwrap_or(0);
        if let Some(slot) = cl_lengths.get_mut(at) {
            *slot = len;
        }
    }
    let cl = Huffman::build(&cl_lengths)?;

    // The two tables are decoded as one run, because a repeat code at the seam
    // legally copies the last literal/length length into the first distance
    // length. Splitting them first is the classic way to get that wrong.
    let total = hlit.saturating_add(hdist);
    let mut lengths = vec![0u8; total];
    let mut i = 0usize;
    while i < total {
        let sym = cl.decode(r)?;
        match sym {
            0..=15 => {
                if let Some(slot) = lengths.get_mut(i) {
                    *slot = u8::try_from(sym).unwrap_or(0);
                }
                i = i.saturating_add(1);
            }
            // 16: repeat the previous length 3..=6 times.
            16 => {
                if i == 0 {
                    return Err(InflateError::BadCodeLengths);
                }
                let prev = *lengths
                    .get(i.saturating_sub(1))
                    .ok_or(InflateError::BadCodeLengths)?;
                let count = (r.bits(2)? as usize).saturating_add(3);
                for _ in 0..count {
                    if i >= total {
                        return Err(InflateError::BadCodeLengths);
                    }
                    if let Some(slot) = lengths.get_mut(i) {
                        *slot = prev;
                    }
                    i = i.saturating_add(1);
                }
            }
            // 17 and 18: a run of zeroes, 3..=10 and 11..=138 long.
            17 | 18 => {
                let count = if sym == 17 {
                    (r.bits(3)? as usize).saturating_add(3)
                } else {
                    (r.bits(7)? as usize).saturating_add(11)
                };
                if i.saturating_add(count) > total {
                    return Err(InflateError::BadCodeLengths);
                }
                // The vector is already zero; stepping past is the whole job.
                i = i.saturating_add(count);
            }
            _ => return Err(InflateError::BadCodeLengths),
        }
    }

    let lit_lengths = lengths.get(..hlit).ok_or(InflateError::BadCodeLengths)?;
    let dist_lengths = lengths.get(hlit..).ok_or(InflateError::BadCodeLengths)?;

    // RFC 1951: the end-of-block symbol must have a code, or the block cannot
    // end and the decoder would run to the end of the input looking for one.
    if lit_lengths.get(256).copied().unwrap_or(0) == 0 {
        return Err(InflateError::BadCodeLengths);
    }

    Ok((Huffman::build(lit_lengths)?, Huffman::build(dist_lengths)?))
}

/// The Adler-32 checksum zlib puts in its trailer (RFC 1950 §9).
#[must_use]
pub fn adler32(data: &[u8]) -> u32 {
    // 65521 is the largest prime below 2^16; the running sums are reduced every
    // 5552 bytes, the most that can be added to `b` without overflowing 32 bits.
    let mut a: u32 = 1;
    let mut b: u32 = 0;
    for chunk in data.chunks(5552) {
        for &byte in chunk {
            a = a.saturating_add(u32::from(byte));
            b = b.saturating_add(a);
        }
        a %= 65521;
        b %= 65521;
    }
    (b << 16) | a
}

/// Decompress a zlib stream: a two-byte header, a DEFLATE stream, and an
/// Adler-32 trailer.
///
/// The checksum is verified. A PNG whose pixel data fails it is corrupt in a
/// way that would otherwise show up as a plausible-looking wrong picture, which
/// is the failure mode worth spending a pass over the output to avoid.
///
/// # Errors
///
/// [`InflateError`] — as [`inflate`], plus [`InflateError::BadZlibHeader`] and
/// [`InflateError::ChecksumMismatch`]. Never panics, for any input.
pub fn zlib_decompress(data: &[u8], limit: usize) -> Result<Vec<u8>, InflateError> {
    let cmf = *data.first().ok_or(InflateError::UnexpectedEnd)?;
    let flg = *data.get(1).ok_or(InflateError::UnexpectedEnd)?;

    // CM must be 8 (DEFLATE) and CINFO at most 7 (32 KiB window). The header is
    // a check value: the sixteen-bit big-endian pair must be a multiple of 31.
    if cmf & 0x0F != 8 || cmf >> 4 > 7 {
        return Err(InflateError::BadZlibHeader);
    }
    if (u16::from(cmf) << 8 | u16::from(flg)) % 31 != 0 {
        return Err(InflateError::BadZlibHeader);
    }
    // FDICT: a preset dictionary the file does not carry and we do not have.
    if flg & 0x20 != 0 {
        return Err(InflateError::BadZlibHeader);
    }

    let body = data.get(2..).ok_or(InflateError::UnexpectedEnd)?;
    let mut r = BitReader::new(body);
    let mut out = Vec::new();
    inflate_into(&mut r, &mut out, limit)?;

    // The trailer sits at the next byte boundary after the last block.
    let end = r.bytes_consumed();
    let trailer = body
        .get(end..end.saturating_add(4))
        .ok_or(InflateError::UnexpectedEnd)?;
    let want = u32::from_be_bytes([
        *trailer.first().unwrap_or(&0),
        *trailer.get(1).unwrap_or(&0),
        *trailer.get(2).unwrap_or(&0),
        *trailer.get(3).unwrap_or(&0),
    ]);
    if want != adler32(&out) {
        return Err(InflateError::ChecksumMismatch);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects
    )]

    use super::*;
    use alloc::vec;

    /// A stored (uncompressed) DEFLATE block wrapping `body`.
    fn stored(body: &[u8]) -> Vec<u8> {
        let len = u16::try_from(body.len()).unwrap();
        let mut out = vec![0x01]; // BFINAL=1, BTYPE=00
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&(!len).to_le_bytes());
        out.extend_from_slice(body);
        out
    }

    fn zlib(body: Vec<u8>, payload: &[u8]) -> Vec<u8> {
        let mut out = vec![0x78, 0x01];
        out.extend_from_slice(&body);
        out.extend_from_slice(&adler32(payload).to_be_bytes());
        out
    }

    #[test]
    fn a_stored_block_comes_back_byte_for_byte() {
        let body = b"the quick brown fox";
        assert_eq!(inflate(&stored(body), 1024).unwrap(), body);
    }

    #[test]
    fn an_empty_stored_block_is_an_empty_result_and_not_an_error() {
        // Zero-length is a legal encoding, and a PNG of a zero-row pass in an
        // interlaced image produces exactly it.
        assert_eq!(inflate(&stored(b""), 1024).unwrap(), Vec::<u8>::new());
    }

    #[test]
    fn a_stored_length_that_disagrees_with_its_complement_is_refused() {
        let mut bad = stored(b"abcd");
        bad[3] ^= 0xFF;
        assert_eq!(inflate(&bad, 1024), Err(InflateError::StoredLengthMismatch));
    }

    #[test]
    fn a_fixed_huffman_block_of_literals_round_trips() {
        // "A" twice, then end-of-block, hand-assembled with the fixed table:
        // 'A' is 0x41 -> code 8 bits, value 0x30 + 0x41 = 0x71 MSB-first.
        let mut bits = BitWriter::default();
        bits.push(1, 1); // BFINAL
        bits.push(1, 2); // BTYPE = 01, fixed
        bits.fixed_literal(u16::from(b'A'));
        bits.fixed_literal(u16::from(b'B'));
        bits.fixed_literal(256); // end of block
        assert_eq!(inflate(&bits.finish(), 64).unwrap(), b"AB");
    }

    #[test]
    fn a_back_reference_that_overlaps_itself_produces_a_run() {
        // 'x' then <length 5, distance 1> is "xxxxxx": the classic overlapping
        // copy that a slice-based implementation gets wrong.
        let mut bits = BitWriter::default();
        bits.push(1, 1);
        bits.push(1, 2);
        bits.fixed_literal(u16::from(b'x'));
        bits.fixed_literal(257 + 2); // length symbol 259 -> base 5, 0 extra
        bits.push_lsb(0, 5); // distance symbol 0 -> distance 1
        bits.fixed_literal(256);
        assert_eq!(inflate(&bits.finish(), 64).unwrap(), b"xxxxxx");
    }

    #[test]
    fn a_distance_reaching_before_the_output_is_refused_rather_than_wrapping() {
        let mut bits = BitWriter::default();
        bits.push(1, 1);
        bits.push(1, 2);
        bits.fixed_literal(257 + 2); // a match with nothing behind it
        bits.push_lsb(0, 5);
        assert_eq!(
            inflate(&bits.finish(), 64),
            Err(InflateError::DistanceTooFar)
        );
    }

    #[test]
    fn output_stops_at_the_callers_limit() {
        // The zip-bomb defence, at its smallest: a stored block of ten bytes
        // against a limit of nine.
        assert_eq!(
            inflate(&stored(b"0123456789"), 9),
            Err(InflateError::OutputTooLarge)
        );
    }

    #[test]
    fn a_run_that_would_cross_the_limit_is_refused_before_it_is_produced() {
        let mut bits = BitWriter::default();
        bits.push(1, 1);
        bits.push(1, 2);
        bits.fixed_literal(u16::from(b'x'));
        bits.fixed_literal(257 + 2);
        bits.push_lsb(0, 5);
        bits.fixed_literal(256);
        // One 'x' plus a five-byte run is six; a limit of five must refuse.
        assert_eq!(
            inflate(&bits.finish(), 5),
            Err(InflateError::OutputTooLarge)
        );
    }

    #[test]
    fn block_type_three_is_reserved_and_refused() {
        assert_eq!(inflate(&[0x07], 64), Err(InflateError::ReservedBlockType));
    }

    #[test]
    fn a_truncated_stream_is_an_error_and_not_a_partial_answer() {
        assert_eq!(inflate(&[], 64), Err(InflateError::UnexpectedEnd));
        assert_eq!(inflate(&[0x01, 0x05], 64), Err(InflateError::UnexpectedEnd));
    }

    #[test]
    fn a_zlib_stream_checks_its_own_adler() {
        let payload = b"hello zlib";
        let good = zlib(stored(payload), payload);
        assert_eq!(zlib_decompress(&good, 64).unwrap(), payload);

        let mut bad = good.clone();
        let last = bad.len() - 1;
        bad[last] ^= 0xFF;
        assert_eq!(
            zlib_decompress(&bad, 64),
            Err(InflateError::ChecksumMismatch)
        );
    }

    #[test]
    fn a_header_that_is_not_zlib_is_refused_before_anything_is_decompressed() {
        // A PNG whose IDAT does not start with a zlib header is either not a
        // PNG or has been re-written by something that does not know it.
        assert_eq!(
            zlib_decompress(&[0x00, 0x00, 0x00], 64),
            Err(InflateError::BadZlibHeader)
        );
        // Right CM, wrong check bits.
        assert_eq!(
            zlib_decompress(&[0x78, 0x00, 0x00], 64),
            Err(InflateError::BadZlibHeader)
        );
    }

    #[test]
    fn a_preset_dictionary_is_refused_rather_than_ignored() {
        // FDICT set. Ignoring it would decompress to plausible-looking rubbish.
        // 0x78 0x20 -> 0x7820 = 30752, not a multiple of 31; find one that is.
        let mut hdr = [0x78u8, 0x20];
        for flg in 0x20u8..=0x3F {
            if (u16::from(0x78u8) << 8 | u16::from(flg)) % 31 == 0 {
                hdr[1] = flg;
                break;
            }
        }
        assert_eq!(hdr[1] & 0x20, 0x20, "the probe must keep FDICT set");
        assert_eq!(zlib_decompress(&hdr, 64), Err(InflateError::BadZlibHeader));
    }

    #[test]
    fn an_over_subscribed_code_is_refused() {
        // Three symbols all one bit long: the code space holds two.
        assert_eq!(
            Huffman::build(&[1, 1, 1]).err(),
            Some(InflateError::BadCodeLengths)
        );
    }

    #[test]
    fn an_incomplete_code_is_accepted_because_a_single_distance_needs_one() {
        // One symbol with a one-bit code leaves half the space unused. RFC 1951
        // permits it, and rejecting it breaks every stream with exactly one
        // back-reference distance.
        assert!(Huffman::build(&[1, 0, 0]).is_ok());
    }

    #[test]
    fn every_byte_of_a_stream_can_be_corrupted_without_a_panic() {
        // The property that matters most: a wallpaper file is something the
        // user was handed. Nothing here may panic, whatever the bytes are.
        let payload = b"the quick brown fox jumps over the lazy dog";
        let good = zlib(stored(payload), payload);
        for i in 0..good.len() {
            for bit in 0..8u32 {
                let mut bad = good.clone();
                bad[i] ^= 1u8 << bit;
                let _ = zlib_decompress(&bad, 4096);
            }
        }
    }

    #[test]
    fn a_stream_of_random_bytes_never_panics() {
        // A cheap deterministic sweep over inputs that are not streams at all.
        let mut state = 0x1234_5678u32;
        for len in 0..64usize {
            let mut buf = vec![0u8; len];
            for b in &mut buf {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                *b = (state >> 24) as u8;
            }
            let _ = inflate(&buf, 4096);
            let _ = zlib_decompress(&buf, 4096);
        }
    }

    #[test]
    fn adler_of_the_empty_string_is_one() {
        // RFC 1950: the sums start at 1 and 0.
        assert_eq!(adler32(b""), 1);
        assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
    }

    /// Assembles a DEFLATE bitstream by hand, so the tests above do not need a
    /// compressor to have an input.
    #[derive(Default)]
    struct BitWriter {
        bytes: Vec<u8>,
        acc: u32,
        live: u32,
    }

    impl BitWriter {
        /// Push `n` bits of `v`, low bit first — the order DEFLATE stores
        /// everything except Huffman codes in.
        fn push_lsb(&mut self, v: u32, n: u32) {
            self.acc |= v << self.live;
            self.live += n;
            while self.live >= 8 {
                self.bytes.push((self.acc & 0xFF) as u8);
                self.acc >>= 8;
                self.live -= 8;
            }
        }

        fn push(&mut self, v: u32, n: u32) {
            self.push_lsb(v, n);
        }

        /// Push a Huffman code, which DEFLATE stores most-significant bit
        /// first even though everything around it is the other way up.
        fn push_msb(&mut self, code: u32, n: u32) {
            for i in (0..n).rev() {
                self.push_lsb((code >> i) & 1, 1);
            }
        }

        /// Push one symbol of the fixed literal/length code.
        fn fixed_literal(&mut self, sym: u16) {
            match sym {
                0..=143 => self.push_msb(0x30 + u32::from(sym), 8),
                144..=255 => self.push_msb(0x190 + u32::from(sym) - 144, 9),
                256..=279 => self.push_msb(u32::from(sym) - 256, 7),
                _ => self.push_msb(0xC0 + u32::from(sym) - 280, 8),
            }
        }

        fn finish(mut self) -> Vec<u8> {
            if self.live > 0 {
                self.bytes.push((self.acc & 0xFF) as u8);
            }
            self.bytes
        }
    }
}
