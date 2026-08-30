//! DEFLATE (RFC 1951), with the gzip (RFC 1952) and zlib (RFC 1950) wrappers.
//!
//! Compression and decompression both, for raw DEFLATE streams and for the
//! two framings that wrap them. This is the codec every `.tar.gz`, `.zip`,
//! `.png` and HTTP `Content-Encoding: gzip` body on the system passes
//! through.
//!
//! # Why this is its own crate
//!
//! It was `kernel/src/fs/compress.rs`, and a module of a *binary* crate
//! cannot be depended on. So when `gui/imagecodec` needed an inflater to
//! decode a PNG, it could not name `crate::fs::compress` and a second,
//! independent DEFLATE implementation was written instead
//! (`requests/c-a-two-inflates.md`). Two implementations of a decompressor is
//! not two of an ordinary function: a decompressor is a parser of untrusted
//! input, so each copy is its own attack surface, each copy has to get the
//! same 30-row length/distance tables right, and a bug fixed in one is not
//! fixed in the other. The kernel copy is the one with the wrappers, the
//! output cap and the boot-battery self-test, so it is the one that was
//! promoted.
//!
//! # The output limit
//!
//! [`inflate`] caps its output at [`MAX_OUTPUT`] (64 MiB), because a DEFLATE
//! stream can expand by roughly 1030:1 and the length is not declared up
//! front — a few kilobytes of hostile input can otherwise ask for gigabytes.
//! 64 MiB is the right cap for a kernel decompressing a `.tar.gz`, and much
//! too generous for an image decoder that already knows the PNG's dimensions
//! and can therefore compute the exact byte count it expects. So every entry
//! point has a `_limited` twin taking the cap as a parameter, and the
//! unsuffixed name delegates to it at `MAX_OUTPUT`.
//!
//! # Errors say what was wrong
//!
//! The kernel version returned `CorruptedData` for all thirty-odd failures
//! and printed the interesting ones to the serial console, which meant the
//! caller could not tell a truncated stream from a failed checksum, and the
//! detail went somewhere the caller could not read. [`Error`] distinguishes
//! them, and the two mismatch variants carry both values — so a caller can
//! report the mismatch, and a `no_std` caller with no console still gets it.
//!
//! # No index, anywhere
//!
//! There is not one `[i]` in this crate outside its tests, where building a
//! deliberately-malformed header by index is clearer. Every read goes
//! through `get`, every
//! write through `get_mut`, and the parallel tables are walked with `zip`
//! rather than a shared index — so `clippy::indexing_slicing` passes clean and
//! the property is checked on every build instead of re-argued on every
//! review. That matters more here than in most code: this crate is a parser
//! of untrusted input running in a kernel, where a panic is not an exception
//! to catch but a fault with nothing above it to handle. The tests carry the
//! same claim from the other direction — `single_byte_corruption_never_panics`
//! flips every byte of a compressed stream, three ways, through all three
//! framings, and asserts only that nothing panics.
//!
//! Where an index genuinely cannot be out of range, the `get` is still there
//! and the comment says why it cannot — an unreachable branch that costs a
//! predictable-not-taken test is a better price than an invariant held only
//! in prose.
//!
//! # References
//!
//! - RFC 1951: DEFLATE Compressed Data Format Specification
//! - RFC 1952: GZIP file format specification
//! - RFC 1950: ZLIB Compressed Data Format Specification
//! - Based on the public-domain `puff.c` by Mark Adler

#![no_std]

extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;
use core::cmp::Ordering;

/// Everything that can go wrong reading a DEFLATE, gzip or zlib stream.
///
/// Deliberately finer-grained than the single `CorruptedData` this replaced.
/// The distinction that matters most to a caller is between
/// [`Error::UnexpectedEnd`] — which a *truncated* download produces, and
/// which is worth retrying — and the rest, which mean the bytes that did
/// arrive are wrong and retrying will not help.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// The stream ended in the middle of something. Truncation, usually.
    UnexpectedEnd,
    /// Block type 3, which RFC 1951 reserves and does not define.
    ReservedBlockType,
    /// A stored block's LEN and NLEN were not one's complements.
    StoredLengthMismatch,
    /// A Huffman code-length table was over-subscribed, incomplete, or
    /// described more symbols than the alphabet has.
    InvalidHuffmanTable,
    /// A decoded symbol was outside the range its table can produce — a
    /// length code above 285, or a distance code above 29.
    InvalidSymbol,
    /// A back-reference pointed further back than the output produced so
    /// far. The classic malformed-stream signature: it would read memory
    /// that was never written.
    DistanceTooFar,
    /// Decompression reached the caller's output cap. Not necessarily a
    /// malformed stream — it is also what a decompression bomb looks like,
    /// and the two are indistinguishable from inside the decoder.
    OutputTooLarge,
    /// The gzip or zlib header was not one: bad magic, a compression method
    /// other than DEFLATE, or a header check that did not hold.
    BadWrapperHeader,
    /// The zlib stream declares a preset dictionary (FDICT). Well-formed but
    /// unsupported: decoding needs a dictionary that is not in the stream.
    PresetDictionary,
    /// The trailer's checksum did not match the decompressed output.
    ChecksumMismatch {
        /// The value the stream's trailer claims.
        expected: u32,
        /// The value the decompressed bytes actually produce.
        actual: u32,
    },
    /// The gzip trailer's ISIZE did not match the decompressed length.
    ///
    /// Separate from [`Error::ChecksumMismatch`] because it usually means
    /// something different: a checksum mismatch says the bytes are wrong,
    /// whereas a size mismatch on a stream whose CRC would have matched says
    /// the stream was concatenated or cut.
    SizeMismatch {
        /// The length the gzip trailer claims, modulo 2^32.
        expected: u32,
        /// The number of bytes decompression actually produced.
        actual: u32,
    },
}

/// The text a person sees when a compressed file will not open.
///
/// **Why this lives in the crate rather than in each caller.** [`Error`] is
/// `#[non_exhaustive]`, so a `match` on it from outside this crate needs a
/// wildcard arm — which means adding a variant here would silently give the
/// caller's table the fallback wording instead of failing its build. The match
/// below is local, so it needs no wildcard, and a new variant without a message
/// is a compile error in the same commit that adds the variant. That asymmetry,
/// not brevity, is the reason for the impl: it is the only place where "every
/// error has wording" can be enforced rather than merely intended.
///
/// (It is not hypothetical. `gui/imagecodec` carried exactly such a table, and
/// it was already missing [`Error::SizeMismatch`] — added here after the table
/// was written, absorbed by its wildcard, and reported as a generic failure.)
///
/// The two mismatch variants print the numbers they compared. A bare "checksum
/// mismatch" tells a reader the file is damaged; the pair of values tells them
/// whether it is damaged *at all* — `expected` of zero, for instance, is a
/// truncated download rather than a corrupted one.
impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match *self {
            Self::UnexpectedEnd => f.write_str("compressed stream ended mid-symbol"),
            Self::ReservedBlockType => f.write_str("reserved DEFLATE block type 3"),
            Self::StoredLengthMismatch => {
                f.write_str("stored block length does not match its complement")
            }
            Self::InvalidHuffmanTable => f.write_str("invalid Huffman code lengths"),
            Self::InvalidSymbol => f.write_str("undecodable Huffman symbol"),
            Self::DistanceTooFar => {
                f.write_str("back-reference points before the start of the output")
            }
            Self::OutputTooLarge => f.write_str("decompressed size exceeds the caller's limit"),
            Self::BadWrapperHeader => {
                f.write_str("not a gzip or zlib stream this decoder supports")
            }
            Self::PresetDictionary => f.write_str("zlib preset dictionary, which is not supported"),
            Self::ChecksumMismatch { expected, actual } => write!(
                f,
                "checksum mismatch: stream declares {expected:#010x}, \
                 the decompressed bytes give {actual:#010x}"
            ),
            Self::SizeMismatch { expected, actual } => write!(
                f,
                "length mismatch: gzip trailer declares {expected} byte(s), \
                 decompression produced {actual}"
            ),
        }
    }
}

/// Shorthand for this crate's fallible operations.
pub type Result<T> = core::result::Result<T, Error>;

// ---------------------------------------------------------------------------
// Bit reader — reads bits from a byte stream, LSB first
// ---------------------------------------------------------------------------

/// Reads bits from a byte buffer, least-significant-bit first.
struct BitReader<'a> {
    data: &'a [u8],
    pos: usize, // byte position
    bit: u8,    // bit position within current byte (0-7)
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            pos: 0,
            bit: 0,
        }
    }

    /// Read `n` bits (1..=25) and return as u32 (LSB first).
    fn read_bits(&mut self, n: u8) -> Result<u32> {
        let mut val = 0u32;
        for i in 0..n {
            let Some(&byte) = self.data.get(self.pos) else {
                return Err(Error::UnexpectedEnd);
            };
            let b = (byte >> self.bit) & 1;
            val |= u32::from(b) << i;
            self.bit = self.bit.wrapping_add(1);
            if self.bit >= 8 {
                self.bit = 0;
                self.pos = self.pos.wrapping_add(1);
            }
        }
        Ok(val)
    }

    /// Align to the next byte boundary (discard remaining bits).
    fn align(&mut self) {
        if self.bit > 0 {
            self.bit = 0;
            self.pos = self.pos.wrapping_add(1);
        }
    }

    /// Read a raw byte at the current byte position (must be aligned).
    fn read_byte(&mut self) -> Result<u8> {
        let Some(&b) = self.data.get(self.pos) else {
            return Err(Error::UnexpectedEnd);
        };
        self.pos = self.pos.wrapping_add(1);
        Ok(b)
    }

    /// Read a 16-bit little-endian value (must be aligned).
    fn read_u16_le(&mut self) -> Result<u16> {
        let lo = self.read_byte()?;
        let hi = self.read_byte()?;
        Ok(u16::from(lo) | (u16::from(hi) << 8))
    }

    /// Remaining bytes in the stream.
    #[allow(dead_code)]
    fn remaining(&self) -> usize {
        if self.pos >= self.data.len() {
            0
        } else {
            self.data.len().wrapping_sub(self.pos)
        }
    }

    /// Current byte position.
    fn byte_pos(&self) -> usize {
        self.pos
    }
}

// ---------------------------------------------------------------------------
// Huffman decoder
// ---------------------------------------------------------------------------

/// Maximum code length allowed by DEFLATE.
const MAX_BITS: usize = 15;

/// Maximum number of symbols for literal/length alphabet.
const MAX_LIT_CODES: usize = 288;

/// Maximum number of distance codes.
const MAX_DIST_CODES: usize = 32;

/// Maximum total codes for code length alphabet.
const MAX_CL_CODES: usize = 19;

/// A Huffman decode table built from a set of code lengths.
///
/// Uses a two-level lookup: codes up to `MAX_BITS` are stored in a
/// flat table indexed by reversed bit pattern.  For a kernel where
/// memory is limited, we use the "counts + symbols" approach from
/// puff.c which is compact and fast.
struct HuffmanTable {
    /// Number of codes of each length (index = length, 0..=MAX_BITS).
    counts: [u16; MAX_BITS + 1],
    /// Symbols sorted by code, then by symbol value.
    symbols: [u16; MAX_LIT_CODES + MAX_DIST_CODES],
    /// Number of valid symbols.
    num_symbols: usize,
}

impl HuffmanTable {
    const fn empty() -> Self {
        Self {
            counts: [0; MAX_BITS + 1],
            symbols: [0; MAX_LIT_CODES + MAX_DIST_CODES],
            num_symbols: 0,
        }
    }

    /// Build a Huffman table from an array of code lengths.
    ///
    /// `lengths[i]` is the code length for symbol `i`.  A length of 0
    /// means the symbol is not present in the alphabet.
    fn build(lengths: &[u8]) -> Result<Self> {
        let mut table = Self::empty();
        table.num_symbols = lengths.len();

        // Count the number of codes for each code length. The `get_mut` is
        // the length check as well as the bounds check: `counts` has exactly
        // `MAX_BITS + 1` slots, and DEFLATE cannot represent a code longer
        // than `MAX_BITS`, so a length that misses the array came from a
        // malformed stream rather than an unusual one.
        for &len in lengths {
            let slot = table
                .counts
                .get_mut(len as usize)
                .ok_or(Error::InvalidHuffmanTable)?;
            *slot = slot.wrapping_add(1);
        }

        // `counts[0]` is the number of symbols with no code at all. If that
        // is every symbol the alphabet is empty — degenerate, but legal: a
        // block containing no back-references has an empty distance alphabet.
        let uncoded = table.counts.first().copied().unwrap_or(0);
        if uncoded as usize == lengths.len() {
            return Ok(table);
        }

        // Check that the Huffman tree is complete or under-subscribed.
        // The Kraft inequality: sum of 2^(-len) for each code must be ≤ 1.
        let mut left: i32 = 1;
        for bits in 1..=MAX_BITS {
            let count = table.counts.get(bits).copied().unwrap_or(0);
            left = left.wrapping_mul(2).wrapping_sub(i32::from(count));
            if left < 0 {
                return Err(Error::InvalidHuffmanTable); // over-subscribed
            }
        }

        // Compute offsets: where codes of each length start in the
        // symbols array.
        let mut offsets = [0u16; MAX_BITS + 1];
        for bits in 1..MAX_BITS {
            let start = offsets.get(bits).copied().unwrap_or(0);
            let count = table.counts.get(bits).copied().unwrap_or(0);
            if let Some(next) = offsets.get_mut(bits.wrapping_add(1)) {
                *next = start.wrapping_add(count);
            }
        }

        // Fill in the symbols array, sorted by code length then value.
        for (sym, &len) in lengths.iter().enumerate() {
            if len == 0 {
                continue;
            }
            let Some(offset) = offsets.get_mut(len as usize) else {
                continue;
            };
            let idx = *offset as usize;
            *offset = offset.wrapping_add(1);
            if let Some(slot) = table.symbols.get_mut(idx) {
                *slot = sym as u16;
            }
        }

        Ok(table)
    }

    /// Decode one symbol from the bit stream.
    ///
    /// Reads bits one at a time, accumulating a code and checking
    /// against each code length.  This is simple (no lookup tables)
    /// and works well for the small alphabets in DEFLATE.
    fn decode(&self, reader: &mut BitReader<'_>) -> Result<u16> {
        let mut code: u32 = 0;
        let mut first: u32 = 0; // first code of this length
        let mut index: u32 = 0; // index into symbols for this length

        for len in 1..=MAX_BITS {
            let bit = reader.read_bits(1)?;
            code = code.wrapping_mul(2).wrapping_add(bit);
            let count = u32::from(self.counts.get(len).copied().unwrap_or(0));
            if code.wrapping_sub(first) < count {
                let sym_idx = index.wrapping_add(code.wrapping_sub(first)) as usize;
                return self
                    .symbols
                    .get(sym_idx)
                    .copied()
                    .ok_or(Error::InvalidSymbol);
            }
            first = first.wrapping_add(count).wrapping_mul(2);
            index = index.wrapping_add(count);
        }

        Err(Error::InvalidSymbol)
    }
}

// ---------------------------------------------------------------------------
// DEFLATE tables — length and distance extra bits
// ---------------------------------------------------------------------------

/// Base lengths for length codes 257..285.
const LENGTH_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];

/// Extra bits for length codes 257..285.
const LENGTH_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];

/// Base distances for distance codes 0..29.
const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];

/// Extra bits for distance codes 0..29.
const DIST_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];

/// Permutation order for reading code-length code lengths (RFC 1951 §3.2.7).
const CL_ORDER: [u8; 19] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
];

// ---------------------------------------------------------------------------
// Fixed Huffman tables (DEFLATE block type 1)
// ---------------------------------------------------------------------------

/// Build the fixed literal/length Huffman code (RFC 1951 §3.2.6).
fn fixed_lit_lengths() -> [u8; 288] {
    // Written as a total function of the symbol rather than four cursor loops:
    // every index is produced by `from_fn` and so cannot be out of range, and
    // the four ranges are visible side by side. The 7-bit run in the middle is
    // what makes this code canonical even though the literals are not in
    // length order — see `fixed_table_shape` in the tests.
    core::array::from_fn(|sym| match sym {
        0..=143 => 8,
        144..=255 => 9,
        256..=279 => 7,
        _ => 8,
    })
}

/// Build the fixed distance Huffman code (all 32 codes are 5 bits).
fn fixed_dist_lengths() -> [u8; 32] {
    [5u8; 32]
}

// ---------------------------------------------------------------------------
// DEFLATE inflate (core decompression)
// ---------------------------------------------------------------------------

/// Default output cap: 64 MiB.
///
/// DEFLATE's maximum expansion ratio is about 1030:1 (a 258-byte match coded
/// in a handful of bits), and the decompressed length is not declared
/// anywhere in the format — so without a cap, 64 KiB of hostile input can ask
/// for 64 GiB and the only thing that stops it is the allocator failing.
/// 64 MiB is generous for the kernel's callers (`.tar.gz` payloads, HTTP
/// bodies, swap pages); a caller that knows its expected size should pass a
/// tighter one to [`inflate_limited`] rather than accept this.
pub const MAX_OUTPUT: usize = 64 * 1024 * 1024;

/// Decompress a raw DEFLATE stream (no gzip/zlib header).
///
/// Output is capped at [`MAX_OUTPUT`]; use [`inflate_limited`] to set the cap.
///
/// # Errors
///
/// See [`Error`]. [`Error::UnexpectedEnd`] means the stream was truncated;
/// every other variant means the bytes present are wrong.
pub fn inflate(data: &[u8]) -> Result<Vec<u8>> {
    inflate_limited(data, MAX_OUTPUT)
}

/// Decompress a raw DEFLATE stream, refusing to produce more than `limit`
/// bytes.
///
/// The cap is checked *before* each byte is appended, so the output never
/// exceeds `limit` and the peak allocation is bounded by it — which is the
/// point. A caller that already knows the answer (an image decoder holding
/// the PNG's width, height and bit depth) should pass that exact figure: it
/// converts a decompression bomb from something that succeeds and eats memory
/// into an immediate [`Error::OutputTooLarge`].
///
/// Passing `limit == 0` is legal and rejects every stream that produces any
/// output at all, including well-formed ones. An empty stream still succeeds.
///
/// # Errors
///
/// As [`inflate`], plus [`Error::OutputTooLarge`] once the output reaches
/// `limit`.
pub fn inflate_limited(data: &[u8], limit: usize) -> Result<Vec<u8>> {
    let mut reader = BitReader::new(data);
    let mut output = Vec::with_capacity(data.len().saturating_mul(2).min(limit));

    loop {
        // Read block header: BFINAL (1 bit) + BTYPE (2 bits).
        let bfinal = reader.read_bits(1)?;
        let btype = reader.read_bits(2)?;

        match btype {
            0 => inflate_stored(&mut reader, &mut output, limit)?,
            1 => inflate_fixed(&mut reader, &mut output, limit)?,
            2 => inflate_dynamic(&mut reader, &mut output, limit)?,
            _ => return Err(Error::ReservedBlockType), // reserved
        }

        if bfinal != 0 {
            break;
        }
    }

    Ok(output)
}

/// Decode a stored (uncompressed) block.
fn inflate_stored(reader: &mut BitReader<'_>, output: &mut Vec<u8>, limit: usize) -> Result<()> {
    reader.align();
    let len = reader.read_u16_le()?;
    let nlen = reader.read_u16_le()?;

    // LEN and NLEN should be one's complements of each other.
    if len != !nlen {
        return Err(Error::StoredLengthMismatch);
    }

    for _ in 0..len {
        if output.len() >= limit {
            return Err(Error::OutputTooLarge);
        }
        let b = reader.read_byte()?;
        output.push(b);
    }

    Ok(())
}

/// Decode a block with fixed Huffman codes.
fn inflate_fixed(reader: &mut BitReader<'_>, output: &mut Vec<u8>, limit: usize) -> Result<()> {
    let lit_table = HuffmanTable::build(&fixed_lit_lengths())?;
    let dist_table = HuffmanTable::build(&fixed_dist_lengths())?;
    inflate_codes(reader, &lit_table, &dist_table, output, limit)
}

/// Decode a block with dynamic Huffman codes.
fn inflate_dynamic(reader: &mut BitReader<'_>, output: &mut Vec<u8>, limit: usize) -> Result<()> {
    // Read the number of literal/length codes, distance codes, and
    // code-length codes.
    let hlit = reader.read_bits(5)?.wrapping_add(257) as usize; // 257..286
    let hdist = reader.read_bits(5)?.wrapping_add(1) as usize; // 1..32
    let hclen = reader.read_bits(4)?.wrapping_add(4) as usize; // 4..19

    if hlit > 286 || hdist > 30 || hclen > 19 {
        return Err(Error::InvalidHuffmanTable);
    }

    // Read code-length code lengths (3 bits each, in permuted order).
    let mut cl_lens = [0u8; MAX_CL_CODES];
    for &order in CL_ORDER.iter().take(hclen) {
        // Read first, place second: the three bits must leave the stream
        // whether or not the permuted slot exists, or every following symbol
        // is decoded from a shifted position. Every `CL_ORDER` entry is < 19
        // so the slot always does exist; `get_mut` states that rather than
        // trusting the table.
        let bits = reader.read_bits(3)? as u8;
        if let Some(slot) = cl_lens.get_mut(order as usize) {
            *slot = bits;
        }
    }

    let cl_table = HuffmanTable::build(&cl_lens)?;

    // Decode literal/length and distance code lengths.
    let total = hlit.wrapping_add(hdist);
    let mut all_lens = [0u8; MAX_LIT_CODES + MAX_DIST_CODES];
    let mut i = 0;

    while i < total {
        let sym = cl_table.decode(reader)?;

        match sym {
            0..=15 => {
                // Literal code length.
                if let Some(slot) = all_lens.get_mut(i) {
                    *slot = sym as u8;
                }
                i = i.wrapping_add(1);
            }
            16 => {
                // Repeat previous length 3..6 times.
                if i == 0 {
                    return Err(Error::InvalidHuffmanTable);
                }
                let repeat = reader.read_bits(2)?.wrapping_add(3) as usize;
                let prev = all_lens.get(i.wrapping_sub(1)).copied().unwrap_or(0);
                for _ in 0..repeat {
                    if i >= total {
                        return Err(Error::InvalidHuffmanTable);
                    }
                    if let Some(slot) = all_lens.get_mut(i) {
                        *slot = prev;
                    }
                    i = i.wrapping_add(1);
                }
            }
            17 => {
                // Repeat zero 3..10 times.
                let repeat = reader.read_bits(3)?.wrapping_add(3) as usize;
                for _ in 0..repeat {
                    if i >= total {
                        return Err(Error::InvalidHuffmanTable);
                    }
                    if let Some(slot) = all_lens.get_mut(i) {
                        *slot = 0;
                    }
                    i = i.wrapping_add(1);
                }
            }
            18 => {
                // Repeat zero 11..138 times.
                let repeat = reader.read_bits(7)?.wrapping_add(11) as usize;
                for _ in 0..repeat {
                    if i >= total {
                        return Err(Error::InvalidHuffmanTable);
                    }
                    if let Some(slot) = all_lens.get_mut(i) {
                        *slot = 0;
                    }
                    i = i.wrapping_add(1);
                }
            }
            _ => return Err(Error::InvalidHuffmanTable),
        }
    }

    let lit_table = HuffmanTable::build(all_lens.get(..hlit).ok_or(Error::InvalidHuffmanTable)?)?;
    let dist_table = HuffmanTable::build(
        all_lens
            .get(hlit..total)
            .ok_or(Error::InvalidHuffmanTable)?,
    )?;

    inflate_codes(reader, &lit_table, &dist_table, output, limit)
}

/// Decode literal/length + distance symbols until end-of-block (256).
fn inflate_codes(
    reader: &mut BitReader<'_>,
    lit_table: &HuffmanTable,
    dist_table: &HuffmanTable,
    output: &mut Vec<u8>,
    limit: usize,
) -> Result<()> {
    loop {
        let sym = lit_table.decode(reader)?;

        // 256 is the end-of-block symbol, and it is the *only* thing that ends
        // a block: below it is a literal, above it a length code. Written as a
        // three-way comparison because that is what the alphabet is.
        match sym.cmp(&256) {
            Ordering::Less => {
                // Literal byte.
                if output.len() >= limit {
                    return Err(Error::OutputTooLarge);
                }
                output.push(sym as u8);
            }
            Ordering::Equal => return Ok(()),
            Ordering::Greater => {
                // Length/distance pair — back-reference.
                let len_idx = (sym as usize).wrapping_sub(257);
                let base_len = *LENGTH_BASE.get(len_idx).ok_or(Error::InvalidSymbol)?;
                let extra = *LENGTH_EXTRA.get(len_idx).ok_or(Error::InvalidSymbol)?;
                let length = usize::from(base_len).wrapping_add(reader.read_bits(extra)? as usize);

                let dist_sym = dist_table.decode(reader)? as usize;
                let base_dist = *DIST_BASE.get(dist_sym).ok_or(Error::InvalidSymbol)?;
                let dist_extra = *DIST_EXTRA.get(dist_sym).ok_or(Error::InvalidSymbol)?;
                let distance =
                    usize::from(base_dist).wrapping_add(reader.read_bits(dist_extra)? as usize);

                if distance == 0 || distance > output.len() {
                    return Err(Error::DistanceTooFar);
                }

                // Copy from the sliding window. Source and destination overlap
                // whenever `length > distance` — distance=1, length=100 is a
                // hundred copies of one byte — so the read walks forward through
                // bytes this very loop is appending. That is the definition, not
                // an accident: a cursor `distance` behind the tail reproduces the
                // modulo-cycling formulation exactly, because by the time it
                // reaches a repeated position the byte there has been written.
                // It also removes the `% distance`, and with it any question
                // about a zero divisor.
                let mut src = output.len().wrapping_sub(distance);
                for _ in 0..length {
                    if output.len() >= limit {
                        return Err(Error::OutputTooLarge);
                    }
                    // `src` trails the tail by exactly `distance`, which was
                    // checked against the length above, so it is always in range.
                    let Some(&b) = output.get(src) else {
                        return Err(Error::DistanceTooFar);
                    };
                    output.push(b);
                    src = src.wrapping_add(1);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// DEFLATE deflate (compression)
// ---------------------------------------------------------------------------

/// Bit writer — writes bits to a byte buffer, LSB first.
struct BitWriter {
    data: Vec<u8>,
    current: u8,
    bits_used: u8,
}

impl BitWriter {
    fn new() -> Self {
        Self {
            data: Vec::new(),
            current: 0,
            bits_used: 0,
        }
    }

    /// Write `n` bits (1..=16) from `val` (LSB first).
    fn write_bits(&mut self, val: u32, n: u8) {
        let mut v = val;
        let mut remaining = n;
        while remaining > 0 {
            let space = 8u8.wrapping_sub(self.bits_used);
            let take = remaining.min(space);
            let mask = (1u32 << take).wrapping_sub(1);
            self.current |= ((v & mask) as u8) << self.bits_used;
            v >>= take;
            self.bits_used = self.bits_used.wrapping_add(take);
            remaining = remaining.wrapping_sub(take);
            if self.bits_used >= 8 {
                self.data.push(self.current);
                self.current = 0;
                self.bits_used = 0;
            }
        }
    }

    /// Flush any remaining partial byte (pad with zeros).
    fn flush(&mut self) {
        if self.bits_used > 0 {
            self.data.push(self.current);
            self.current = 0;
            self.bits_used = 0;
        }
    }

    /// Write a raw byte (must be byte-aligned).
    fn write_byte(&mut self, b: u8) {
        if self.bits_used == 0 {
            self.data.push(b);
        } else {
            self.write_bits(u32::from(b), 8);
        }
    }

    fn into_bytes(mut self) -> Vec<u8> {
        self.flush();
        self.data
    }
}

/// Fixed Huffman code table for the encoder.
///
/// Returns (code, length) for a literal/length symbol.
/// The codes are reversed (LSB-first) for the bit writer.
fn fixed_code(sym: u16) -> (u16, u8) {
    match sym {
        0..=143 => {
            // 8-bit codes: 00110000..10111111 (0x30..0xBF)
            let code = sym.wrapping_add(0x30);
            (reverse_bits(code, 8), 8)
        }
        144..=255 => {
            // 9-bit codes: 110010000..111111111 (0x190..0x1FF)
            let code = sym.wrapping_sub(144).wrapping_add(0x190);
            (reverse_bits(code, 9), 9)
        }
        256..=279 => {
            // 7-bit codes: 0000000..0010111 (0x00..0x17)
            let code = sym.wrapping_sub(256);
            (reverse_bits(code, 7), 7)
        }
        280..=287 => {
            // 8-bit codes: 11000000..11000111 (0xC0..0xC7)
            let code = sym.wrapping_sub(280).wrapping_add(0xC0);
            (reverse_bits(code, 8), 8)
        }
        _ => (0, 0),
    }
}

/// Reverse `n` bits of `val`.
fn reverse_bits(val: u16, n: u8) -> u16 {
    let mut result: u16 = 0;
    let mut v = val;
    for _ in 0..n {
        result = (result << 1) | (v & 1);
        v >>= 1;
    }
    result
}

/// Fixed Huffman code for a distance (0..29): all are 5-bit codes.
fn fixed_dist_code(dist_sym: u16) -> (u16, u8) {
    (reverse_bits(dist_sym, 5), 5)
}

/// Find the length code (257..285) and extra bits for a match length.
fn encode_length(length: usize) -> Option<(u16, u32, u8)> {
    // Zipped rather than indexed: the two tables are parallel by construction
    // and `zip` is what makes a future edit that lengthens one of them fail to
    // compile instead of reading past the other.
    for (i, (&base, &extra)) in LENGTH_BASE.iter().zip(LENGTH_EXTRA.iter()).enumerate() {
        let range = 1usize << extra;
        let base = usize::from(base);
        if length >= base && length < base.wrapping_add(range) {
            let sym = (i as u16).wrapping_add(257);
            let extra_val = length.wrapping_sub(base) as u32;
            return Some((sym, extra_val, extra));
        }
    }
    None
}

/// Find the distance code (0..29) and extra bits for a match distance.
fn encode_distance(distance: usize) -> Option<(u16, u32, u8)> {
    for (i, (&base, &extra)) in DIST_BASE.iter().zip(DIST_EXTRA.iter()).enumerate() {
        let range = 1usize << extra;
        let base = usize::from(base);
        if distance >= base && distance < base.wrapping_add(range) {
            let sym = i as u16;
            let extra_val = distance.wrapping_sub(base) as u32;
            return Some((sym, extra_val, extra));
        }
    }
    None
}

/// Minimum match length for LZ77.
const MIN_MATCH: usize = 3;

/// Maximum match length (symbol 285 = length 258).
const MAX_MATCH: usize = 258;

/// Hash table size for LZ77 string matching (must be power of 2).
const HASH_SIZE: usize = 4096;

/// Maximum backward distance for LZ77 (32 KiB DEFLATE window).
const MAX_DISTANCE: usize = 32768;

/// Default maximum chain length for hash chain traversal.
///
/// Longer chains find better matches but slow down compression.
/// zlib default at compression level 6 uses chain=128.  We use 16
/// as a good balance for kernel use (fast enough, good compression).
///
/// This is what [`deflate`] uses, which by [`level_max_chain`]'s table makes
/// `deflate()` equivalent to level **3**.  It is deliberately *not* level 6:
/// every existing caller (`kernel::fs::compress`, `ziparchive`, [`gzip`],
/// [`zlib_deflate`]) was tuned against this value, and changing it would
/// silently re-encode all of them.  Callers that want to choose pay for the
/// choice explicitly via [`deflate_level`].
const DEFAULT_MAX_CHAIN: usize = 16;

/// Map a compression level, 1 (fastest) … 9 (smallest), to a hash-chain depth.
///
/// Levels outside 1..=9 clamp rather than error: every caller validates its
/// own flag before reaching here, so a `Result` would be an error they could
/// prove impossible and would have to `unwrap` or plumb — and an `unwrap` in
/// production code is exactly what `CLAUDE.md` forbids. Clamping keeps the
/// signature total.
///
/// # Why this table and not zlib's
///
/// Real zlib's `max_chain` per level is 4, 8, 32, 16, 32, 128, 256, 1024,
/// 4096 — not monotonic at levels 3–4 (it varies `good_length`/`nice_length`
/// alongside, which we do not), and much steeper at the top. The table here
/// is the one `userspace/zip` already maps its `-1`..`-9` flags onto, adopted
/// verbatim on purpose: it makes lane B's switch to this crate a pure
/// deletion with **no change to any archive `zip` produces**, which is worth
/// more than nominal fidelity to a table we would not be implementing
/// faithfully anyway. Monotonicity is the property callers actually rely on,
/// and this table has it where zlib's does not.
///
/// Do not "correct" this toward zlib's numbers without re-reading that: it
/// would change `zip`'s output for every level.
#[must_use]
pub fn level_max_chain(level: u8) -> usize {
    match level {
        0 | 1 => 4,
        2 => 8,
        3 => 16,
        4 => 32,
        5 => 64,
        6 => 128,
        7 => 256,
        8 => 512,
        _ => 1024,
    }
}

/// Compute a hash for 3 bytes.
fn lz77_hash(data: &[u8], pos: usize) -> usize {
    // Three bytes taken as one slice: the pattern is the length check, so a
    // position within two bytes of the end hashes to bucket 0 without any
    // separate comparison to keep in step with the reads.
    let Some(&[a, b, c]) = data.get(pos..pos.wrapping_add(3)) else {
        return 0;
    };
    let h = (u32::from(a) << 10) ^ (u32::from(b) << 5) ^ u32::from(c);
    (h as usize) & (HASH_SIZE.wrapping_sub(1))
}

// ---------------------------------------------------------------------------
// LZ77 tokenizer (shared by fixed and dynamic Huffman)
// ---------------------------------------------------------------------------

/// An LZ77 token: either a literal byte or a length/distance match.
///
/// `Copy` so the encoders can take an `Iterator<Item = LzToken>` rather than a
/// `&[LzToken]`. That is what lets `deflate` try a literals-only encoding of
/// the same input without materialising a second, `data.len()`-entry token
/// vector — the literal stream is `data.iter().map(LzToken::Literal)`, which
/// allocates nothing. At six bytes a token that second vector would have cost
/// six bytes per input byte, which is not a price a kernel compressing a
/// filesystem block should pay for a candidate it usually discards.
#[derive(Clone, Copy)]
enum LzToken {
    Literal(u8),
    Match { length: u16, distance: u16 },
}

/// Insert position `pos` into the hash chain and return the previous
/// head of that chain (for match searching).
///
/// `head[h]` stores the most recent position with hash `h`.
/// `prev[pos % MAX_DISTANCE]` links to the previous position in the chain.
fn insert_hash(data: &[u8], pos: usize, head: &mut [u32], prev: &mut [u32]) -> u32 {
    let h = lz77_hash(data, pos);
    // `h` is masked to `HASH_SIZE` and the link index to `MAX_DISTANCE`, so
    // both are in range for the vectors `lz77_tokenize` allocates. `get_mut`
    // is what keeps that true if a future caller sizes them differently:
    // a mis-sized table then loses chain entries and costs compression ratio,
    // which is recoverable, rather than panicking.
    let Some(slot) = head.get_mut(h) else {
        return 0;
    };
    let old_head = *slot;
    *slot = pos as u32;
    if let Some(link) = prev.get_mut(pos % MAX_DISTANCE) {
        *link = old_head;
    }
    old_head
}

/// Find the longest match at `pos` by walking the hash chain.
///
/// Returns (length, distance) of the best match found, or (0, 0) if
/// no match of at least `MIN_MATCH` bytes exists.
/// `chain_start` must be the value [`insert_hash`] returned for this `pos` —
/// the head of the chain *before* `pos` was linked into it.
///
/// It is deliberately a parameter rather than a re-read of `head[h]`. The
/// caller inserts before it searches, so by this point `head[h] == pos`, and
/// a search starting there fails the `candidate < pos` guard on its very
/// first iteration and returns "no match" for every position of every input.
/// That was a real defect, not a hypothetical: the LZ77 stage found zero
/// matches on all data, and `deflate` silently degraded to Huffman-only
/// coding. See `known-issues.md` → `A-DEFLATE-LZ77-NEVER-FOUND-A-MATCH`.
fn find_best_match(
    data: &[u8],
    pos: usize,
    chain_start: usize,
    prev: &[u32],
    max_chain: usize,
) -> (usize, usize) {
    let remaining = data.len().wrapping_sub(pos);
    if remaining < MIN_MATCH {
        return (0, 0);
    }

    let mut candidate = chain_start;
    let max_len = remaining.min(MAX_MATCH);

    let mut best_len: usize = MIN_MATCH.wrapping_sub(1);
    let mut best_dist: usize = 0;
    let mut chain_count = 0usize;

    while candidate < pos && pos.wrapping_sub(candidate) <= MAX_DISTANCE && chain_count < max_chain
    {
        // Compare bytes at candidate vs pos, starting from the current
        // best length downward (zlib trick: check the last matching byte
        // first to quickly reject poor candidates).
        let dist = pos.wrapping_sub(candidate);

        // Quick rejection: if bytes at the current best length don't match,
        // this candidate can't be better.
        if best_len > 0
            && data.get(candidate.wrapping_add(best_len)) != data.get(pos.wrapping_add(best_len))
        {
            candidate = prev.get(candidate % MAX_DISTANCE).copied().unwrap_or(0) as usize;
            chain_count = chain_count.wrapping_add(1);
            continue;
        }

        // Full comparison.
        let mut len = 0;
        while len < max_len
            && data.get(candidate.wrapping_add(len)) == data.get(pos.wrapping_add(len))
        {
            len = len.wrapping_add(1);
        }

        if len > best_len {
            best_len = len;
            best_dist = dist;
            if len == max_len {
                break; // Can't do better than MAX_MATCH.
            }
        }

        candidate = prev.get(candidate % MAX_DISTANCE).copied().unwrap_or(0) as usize;
        chain_count = chain_count.wrapping_add(1);
    }

    // Reject a bare 3-byte match at a long distance: encoding it costs a
    // length symbol plus a distance symbol with up to 13 extra bits, which is
    // routinely *more* than the three literals it replaces.  zlib calls the
    // threshold TOO_FAR and applies exactly this rule in `deflate_slow`.
    //
    // This is not a micro-optimisation, it is what keeps incompressible input
    // from inflating.  Without it, repairing the match-finding bug above made
    // random data 12% *larger* than the (accidentally) Huffman-only encoder it
    // replaced — matches were being found, and every one of them was a loss.
    const TOO_FAR: usize = 4096;
    if best_len == MIN_MATCH && best_dist > TOO_FAR {
        return (0, 0);
    }

    if best_len >= MIN_MATCH {
        (best_len, best_dist)
    } else {
        (0, 0)
    }
}

/// Run the LZ77 pass over `data` and return a token stream.
///
/// Uses hash chains (up to `max_chain` candidates per position) with
/// lazy matching: when a match is found at position P, also check P+1.
/// If P+1 has a strictly longer match, emit a literal for P and use
/// the longer match.
///
/// Hash chains allow checking multiple previous positions that hash to
/// the same value, finding the longest match among them.  Combined with
/// lazy matching, this is equivalent to zlib's deflate_slow() at level 6.
///
/// Based on zlib's deflate.c (compress.c) by Jean-loup Gailly.
#[allow(clippy::arithmetic_side_effects)]
fn lz77_tokenize(data: &[u8], max_chain: usize) -> Vec<LzToken> {
    let mut tokens = Vec::with_capacity(data.len() / 2);

    // Hash chain: head[h] = most recent pos with hash h.
    // prev[pos % MAX_DISTANCE] = previous pos in same chain.
    //
    // `head` is heap-allocated (a `Vec`, like `prev`) rather than a stack
    // array: at HASH_SIZE = 4096 the `[u32; HASH_SIZE]` form occupied 16 KiB
    // of the 64 KiB kernel task stack, and combined with the rest of the
    // gzip/deflate call chain (plus any interrupt that nests on the same
    // stack) it overflowed into the guard page — a double fault (#DF) in the
    // benchmark suite.  Keeping it on the heap removes that 16 KiB frame.
    // See known-issues.md B-DF1.
    let mut head = vec![0u32; HASH_SIZE];
    let mut prev = vec![0; MAX_DISTANCE];

    let mut pos: usize = 0;

    // Pending match from the previous position (for lazy matching).
    let mut pending_len: usize = 0;
    let mut pending_dist: usize = 0;
    // The byte the pending match starts on. Carried rather than re-read as
    // `data[pos - 1]`: when lazy matching drops a pending match it has to emit
    // that byte as a literal, and remembering it is both cheaper and one fewer
    // index than reaching backwards for it.
    let mut pending_byte: u8 = 0;

    // `data.get(pos)` is exactly the `pos < data.len()` test, and it hands the
    // byte over at the same time — which is what removes the three `data[pos]`
    // reads this loop used to make under a separately-written bound.
    while let Some(&here) = data.get(pos) {
        let remaining = data.len().wrapping_sub(pos);

        if remaining < MIN_MATCH {
            // Flush any pending match.
            if pending_len >= MIN_MATCH {
                tokens.push(LzToken::Match {
                    length: pending_len as u16,
                    distance: pending_dist as u16,
                });
                let match_start = pos.wrapping_sub(1);
                for i in 1..pending_len {
                    let mpos = match_start.wrapping_add(i);
                    if mpos.wrapping_add(2) < data.len() {
                        insert_hash(data, mpos, &mut head, &mut prev);
                    }
                }
                pos = match_start.wrapping_add(pending_len);
                pending_len = 0;
                continue;
            }
            tokens.push(LzToken::Literal(here));
            pos = pos.wrapping_add(1);
            continue;
        }

        // Insert this position into the hash chain, keeping the previous head
        // as the point to start searching from.  Discarding this return value
        // is what disabled match-finding entirely: the search would then begin
        // at `head[h]`, which the insert has just set to `pos` itself.
        let chain_start = insert_hash(data, pos, &mut head, &mut prev) as usize;

        // Find the best match at this position using the chain.
        let (cur_len, cur_dist) = find_best_match(data, pos, chain_start, &prev, max_chain);

        if pending_len >= MIN_MATCH {
            // We have a pending match from pos-1.  Compare with current.
            if cur_len > pending_len {
                // Current match is better — drop pending, emit literal for pos-1.
                tokens.push(LzToken::Literal(pending_byte));
                pending_len = cur_len;
                pending_dist = cur_dist;
                pending_byte = here;
                pos = pos.wrapping_add(1);
            } else {
                // Pending match is equal or better — emit it.
                tokens.push(LzToken::Match {
                    length: pending_len as u16,
                    distance: pending_dist as u16,
                });
                // Update hash for skipped positions inside the match.
                let match_start = pos.wrapping_sub(1);
                for i in 1..pending_len {
                    let mpos = match_start.wrapping_add(i);
                    if mpos.wrapping_add(2) < data.len() && mpos != pos {
                        insert_hash(data, mpos, &mut head, &mut prev);
                    }
                }
                pos = match_start.wrapping_add(pending_len);
                pending_len = 0;
            }
        } else if cur_len >= MIN_MATCH {
            // New match — defer for lazy evaluation at next position.
            pending_len = cur_len;
            pending_dist = cur_dist;
            pending_byte = here;
            pos = pos.wrapping_add(1);
        } else {
            // No match.
            tokens.push(LzToken::Literal(here));
            pos = pos.wrapping_add(1);
        }
    }

    // Flush any remaining pending match.
    if pending_len >= MIN_MATCH {
        tokens.push(LzToken::Match {
            length: pending_len as u16,
            distance: pending_dist as u16,
        });
    }

    tokens
}

// ---------------------------------------------------------------------------
// Dynamic Huffman tree construction (RFC 1951 §3.2.7)
// ---------------------------------------------------------------------------

/// Maximum code length bits for lit/len and distance codes.
const MAX_CODE_BITS: u8 = 15;

/// Build canonical Huffman code lengths from symbol frequencies.
///
/// Uses a simplified package-merge / length-limited Huffman algorithm:
/// 1. Sort symbols by frequency (non-zero only).
/// 2. Build a standard Huffman tree.
/// 3. Limit code lengths to `max_bits` by pushing overflow down.
///
/// Returns a vector of code lengths indexed by symbol (0 = unused).
#[allow(clippy::arithmetic_side_effects)]
fn build_code_lengths(freqs: &[u32], max_bits: u8) -> Vec<u8> {
    let n = freqs.len();
    let mut lengths = vec![0; n];

    // Collect non-zero frequency symbols.
    let mut symbols: Vec<(u32, usize)> = freqs
        .iter()
        .enumerate()
        .filter(|(_, f)| **f > 0)
        .map(|(i, f)| (*f, i))
        .collect();

    if symbols.is_empty() {
        return lengths;
    }

    if let [(_, only)] = symbols.as_slice() {
        // Single symbol — assign length 1. Matched as a one-element slice
        // rather than tested with `len() == 1` and then indexed, so the
        // element is produced by the same step that establishes it exists.
        if let Some(slot) = lengths.get_mut(*only) {
            *slot = 1;
        }
        return lengths;
    }

    // Sort by frequency (ascending), break ties by symbol index.
    symbols.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));

    // Build Huffman tree via bottom-up merge.
    // We track tree depths using a second array.
    let count = symbols.len();
    // Internal nodes: freq and max leaf depth.
    let mut node_freq: Vec<u32> = symbols.iter().map(|s| s.0).collect();
    let mut node_depth: Vec<u8> = vec![0; count];

    // Repeatedly merge the two lowest-frequency nodes.
    // We use a simple O(n^2) approach (fine for 286 symbols).
    let mut active: Vec<bool> = Vec::new();
    active.resize(count, true);
    // parent_of[i] = index of the merged node, or usize::MAX if root.
    let mut parent_of: Vec<usize> = Vec::new();
    parent_of.resize(count, usize::MAX);

    // We need count-1 merges to build the tree.
    let total_nodes = count.saturating_mul(2).saturating_sub(1);
    node_freq.resize(total_nodes, 0);
    node_depth.resize(total_nodes, 0);
    active.resize(total_nodes, false);
    parent_of.resize(total_nodes, usize::MAX);

    let mut next_node = count;

    for _ in 0..count.saturating_sub(1) {
        // Find two smallest active nodes.
        let mut min1 = usize::MAX;
        let mut min2 = usize::MAX;
        let mut f1 = u32::MAX;
        let mut f2 = u32::MAX;

        // Zipped and truncated to `next_node` rather than indexed by a range:
        // the two vectors are parallel and the iterator is what enforces it.
        for (i, (&is_active, &freq)) in active
            .iter()
            .zip(node_freq.iter())
            .enumerate()
            .take(next_node)
        {
            if !is_active {
                continue;
            }
            if freq < f1 || (freq == f1 && i < min1) {
                f2 = f1;
                min2 = min1;
                f1 = freq;
                min1 = i;
            } else if freq < f2 || (freq == f2 && i < min2) {
                f2 = freq;
                min2 = i;
            }
        }

        if min1 == usize::MAX || min2 == usize::MAX {
            break;
        }

        // Create merged node. Every write below is inside the `total_nodes`
        // sizing computed above; going through `get_mut` means a future change
        // to that sizing costs compression ratio rather than panicking in a
        // kernel that has no way to recover from one.
        let merged = next_node;
        next_node = next_node.saturating_add(1);
        let depth1 = node_depth.get(min1).copied().unwrap_or(0);
        let depth2 = node_depth.get(min2).copied().unwrap_or(0);
        if let Some(slot) = node_freq.get_mut(merged) {
            *slot = f1.saturating_add(f2);
        }
        if let Some(slot) = node_depth.get_mut(merged) {
            *slot = depth1.max(depth2).saturating_add(1);
        }
        if let Some(slot) = active.get_mut(merged) {
            *slot = true;
        }
        if let Some(slot) = active.get_mut(min1) {
            *slot = false;
        }
        if let Some(slot) = active.get_mut(min2) {
            *slot = false;
        }
        if let Some(slot) = parent_of.get_mut(min1) {
            *slot = merged;
        }
        if let Some(slot) = parent_of.get_mut(min2) {
            *slot = merged;
        }
    }

    // Compute depth of each leaf by walking up to root. A merged node's index
    // is always greater than either child's, so the walk strictly increases
    // and cannot cycle — but it is bounded by `total_nodes` anyway, because a
    // cycle here would hang the kernel rather than fault it, and a hang is the
    // harder of the two to diagnose from a serial log.
    for (i, &(_, sym)) in symbols.iter().enumerate() {
        let mut depth: u8 = 0;
        let mut node = i;
        for _ in 0..total_nodes {
            let Some(&parent) = parent_of.get(node) else {
                break;
            };
            if parent == usize::MAX {
                break;
            }
            depth = depth.saturating_add(1);
            node = parent;
        }
        if let Some(slot) = lengths.get_mut(sym) {
            *slot = depth;
        }
    }

    // Limit to max_bits.  If any code exceeds max_bits, redistribute
    // by a simple heuristic: increment the shortest code and decrement
    // the longest until all fit.
    for _ in 0..64 {
        // Safety iteration limit.
        let overflow = lengths.iter().any(|l| *l > max_bits);
        if !overflow {
            break;
        }

        // Find the longest code and the shortest code > 1.
        let mut longest_sym = 0usize;
        let mut longest_len = 0u8;
        let mut shortest_sym = 0usize;
        let mut shortest_len = u8::MAX;

        for (i, &l) in lengths.iter().enumerate() {
            if l > longest_len {
                longest_len = l;
                longest_sym = i;
            }
            if l > 0 && l < shortest_len {
                shortest_len = l;
                shortest_sym = i;
            }
        }

        if longest_len <= max_bits {
            break;
        }

        // Push down: shorten the longest, lengthen the shortest.
        if let Some(slot) = lengths.get_mut(longest_sym) {
            *slot = slot.saturating_sub(1);
        }
        if shortest_sym != longest_sym {
            if let Some(slot) = lengths.get_mut(shortest_sym) {
                *slot = slot.saturating_add(1);
            }
        }
    }

    lengths
}

/// Build canonical Huffman codes from code lengths.
///
/// Returns (code, length) for each symbol.  Codes are reversed
/// (LSB-first) for the DEFLATE bit writer.
///
/// RFC 1951 §3.2.2: canonical codes are assigned in ascending order
/// of code length, then symbol value within the same length.
#[allow(clippy::arithmetic_side_effects)]
fn build_canonical_codes(lengths: &[u8]) -> Vec<(u16, u8)> {
    let max_len = lengths.iter().copied().max().unwrap_or(0);
    let mut codes = Vec::new();
    codes.resize(lengths.len(), (0u16, 0u8));

    if max_len == 0 {
        return codes;
    }

    // Count codes of each length.
    let mut bl_count: Vec<u16> = vec![0; max_len as usize + 1];
    for &l in lengths {
        if l > 0 {
            if let Some(slot) = bl_count.get_mut(l as usize) {
                *slot = slot.wrapping_add(1);
            }
        }
    }

    // Compute starting code for each length. Both vectors were sized from
    // `max_len`, which is the maximum over the same `lengths`, so every index
    // below is in range by construction.
    let mut next_code: Vec<u16> = vec![0; max_len as usize + 1];
    let mut code: u16 = 0;
    for bits in 1..=max_len {
        let shorter = bl_count.get(bits as usize - 1).copied().unwrap_or(0);
        code = code.wrapping_add(shorter) << 1;
        if let Some(slot) = next_code.get_mut(bits as usize) {
            *slot = code;
        }
    }

    // Assign codes.
    for (sym, &l) in lengths.iter().enumerate() {
        if l == 0 {
            continue;
        }
        let Some(slot) = next_code.get_mut(l as usize) else {
            continue;
        };
        let c = *slot;
        *slot = c.wrapping_add(1);
        if let Some(out) = codes.get_mut(sym) {
            *out = (reverse_bits(c, l), l);
        }
    }

    codes
}

// CL_ORDER for the encoder uses the same constant defined at line 256
// (the [u8; 19] array).  Encoder code casts to usize as needed.

/// Run-length encode code lengths for the dynamic Huffman header.
///
/// Uses symbols 0-15 for actual lengths, 16 (repeat previous 3-6x),
/// 17 (repeat zero 3-10x), 18 (repeat zero 11-138x).
#[allow(clippy::arithmetic_side_effects)]
fn rle_code_lengths(lengths: &[u8]) -> Vec<(u8, u8)> {
    // Vec of (symbol, extra_bits_value)
    let mut rle: Vec<(u8, u8)> = Vec::new();
    let mut i = 0;

    while let Some(&val) = lengths.get(i) {
        if val == 0 {
            // Count consecutive zeros. `get(..) == Some(&0)` folds the bound
            // and the comparison into one expression, so the two can never
            // drift apart.
            let mut run = 1usize;
            while lengths.get(i.wrapping_add(run)) == Some(&0) && run < 138 {
                run = run.wrapping_add(1);
            }

            if run >= 11 {
                // Symbol 18: repeat zero 11-138 times.
                rle.push((18, (run.wrapping_sub(11)) as u8));
            } else if run >= 3 {
                // Symbol 17: repeat zero 3-10 times.
                rle.push((17, (run.wrapping_sub(3)) as u8));
            } else {
                for _ in 0..run {
                    rle.push((0, 0));
                }
            }
            i = i.wrapping_add(run);
        } else {
            rle.push((val, 0));
            i = i.wrapping_add(1);

            // Count consecutive repeats of `val`.
            let mut run = 0usize;
            while lengths.get(i.wrapping_add(run)) == Some(&val) && run < 6 {
                run = run.wrapping_add(1);
            }

            if run >= 3 {
                // Symbol 16: repeat previous 3-6 times.
                rle.push((16, (run.wrapping_sub(3)) as u8));
                i = i.wrapping_add(run);
            }
        }
    }

    rle
}

/// Count one occurrence of `sym` in a frequency table.
///
/// Every caller's symbol is bounded by the alphabet the table was sized for,
/// so the miss branch is unreachable — but there are eight such sites, and
/// routing them through one function is what keeps that argument from having
/// to be made eight times.
fn bump(freqs: &mut [u32], sym: usize) {
    if let Some(slot) = freqs.get_mut(sym) {
        *slot = slot.saturating_add(1);
    }
}

/// Look up a canonical Huffman code, defaulting to a zero-length code.
///
/// A zero-length code writes no bits, which is exactly what a symbol absent
/// from the alphabet should contribute — so a lookup that misses degrades the
/// stream rather than aborting mid-block with a half-written bit buffer.
fn code_for(codes: &[(u16, u8)], sym: usize) -> (u16, u8) {
    codes.get(sym).copied().unwrap_or((0, 0))
}

/// Encode tokens using dynamic Huffman codes (BTYPE=10).
///
/// Builds optimal Huffman trees from the token frequencies, encodes
/// the tree in the block header, then encodes all tokens.
///
/// `tokens` is an iterator rather than a slice, and must be `Clone`, because
/// this function walks it twice: once to histogram the symbols and once to
/// emit them. The `Clone` bound is what lets a caller pass a synthesised
/// stream (`data.iter().map(LzToken::Literal)`) with no backing allocation.
#[allow(clippy::arithmetic_side_effects)]
fn encode_dynamic<I>(writer: &mut BitWriter, tokens: I, bfinal: bool)
where
    I: Iterator<Item = LzToken> + Clone,
{
    // --- Count symbol frequencies ---
    let mut lit_freq = [0u32; 286]; // 0-255 literals, 256 end, 257-285 lengths
    let mut dist_freq = [0u32; 30]; // 0-29 distance codes

    bump(&mut lit_freq, 256); // End-of-block always present.

    for token in tokens.clone() {
        match token {
            LzToken::Literal(b) => {
                bump(&mut lit_freq, b as usize);
            }
            LzToken::Match { length, distance } => {
                if let Some((sym, _, _)) = encode_length(length as usize) {
                    bump(&mut lit_freq, sym as usize);
                }
                if let Some((sym, _, _)) = encode_distance(distance as usize) {
                    bump(&mut dist_freq, sym as usize);
                }
            }
        }
    }

    // --- Build code lengths ---
    let lit_lengths = build_code_lengths(&lit_freq, MAX_CODE_BITS);
    let dist_lengths = build_code_lengths(&dist_freq, MAX_CODE_BITS);

    // HLIT: number of lit/len codes - 257 (max 286 codes).
    // Find the last non-zero length.
    let hlit = lit_lengths
        .iter()
        .rposition(|&l| l > 0)
        .map(|p| p.saturating_add(1))
        .unwrap_or(257)
        .max(257);
    // HDIST: number of distance codes - 1 (at least 1).
    let hdist = dist_lengths
        .iter()
        .rposition(|&l| l > 0)
        .map(|p| p.saturating_add(1))
        .unwrap_or(1)
        .max(1);

    // Concatenate lit/len + distance code lengths for RLE.
    let mut all_lengths: Vec<u8> = Vec::with_capacity(hlit.wrapping_add(hdist));
    all_lengths.extend_from_slice(lit_lengths.get(..hlit).unwrap_or(&lit_lengths));
    // Pad if needed.
    while all_lengths.len() < hlit {
        all_lengths.push(0);
    }
    all_lengths.extend_from_slice(dist_lengths.get(..hdist).unwrap_or(&dist_lengths));
    while all_lengths.len() < hlit.wrapping_add(hdist) {
        all_lengths.push(0);
    }

    // Run-length encode the combined lengths.
    let rle = rle_code_lengths(&all_lengths);

    // Count frequencies of the RLE symbols (0-18).
    let mut cl_freq = [0u32; 19];
    for &(sym, _) in &rle {
        bump(&mut cl_freq, sym as usize);
    }

    // Build code-length Huffman tree.
    let cl_lengths = build_code_lengths(&cl_freq, 7);
    let cl_codes = build_canonical_codes(&cl_lengths);

    // HCLEN: number of code-length codes - 4.
    // Find the last non-zero in the permuted order.
    let mut hclen = 4usize;
    for (i, &order) in CL_ORDER.iter().enumerate().rev() {
        if cl_lengths.get(order as usize).copied().unwrap_or(0) > 0 {
            hclen = i.saturating_add(1).max(4);
            break;
        }
    }

    // --- Emit block header ---
    writer.write_bits(u32::from(bfinal), 1); // BFINAL
    writer.write_bits(2, 2); // BTYPE=10 (dynamic)

    writer.write_bits((hlit.wrapping_sub(257)) as u32, 5); // HLIT
    writer.write_bits((hdist.wrapping_sub(1)) as u32, 5); // HDIST
    writer.write_bits((hclen.wrapping_sub(4)) as u32, 4); // HCLEN

    // Emit code-length code lengths (3 bits each, permuted order).
    for &order in CL_ORDER.iter().take(hclen) {
        let l = cl_lengths.get(order as usize).copied().unwrap_or(0);
        writer.write_bits(u32::from(l), 3);
    }

    // Emit RLE-encoded literal/length + distance code lengths.
    for &(sym, extra) in &rle {
        let (code, bits) = code_for(&cl_codes, sym as usize);
        if bits > 0 {
            writer.write_bits(u32::from(code), bits);
        }
        // Extra bits for repeat symbols.
        match sym {
            16 => writer.write_bits(u32::from(extra), 2), // 3-6
            17 => writer.write_bits(u32::from(extra), 3), // 3-10
            18 => writer.write_bits(u32::from(extra), 7), // 11-138
            _ => {}
        }
    }

    // --- Build and use the actual lit/len and distance codes ---
    let lit_codes = build_canonical_codes(&lit_lengths);
    let dist_codes = build_canonical_codes(&dist_lengths);

    // Emit tokens.
    for token in tokens {
        match token {
            LzToken::Literal(b) => {
                let (code, bits) = code_for(&lit_codes, b as usize);
                writer.write_bits(u32::from(code), bits);
            }
            LzToken::Match { length, distance } => {
                if let (
                    Some((len_sym, len_extra, len_ebits)),
                    Some((dist_sym, dist_extra, dist_ebits)),
                ) = (
                    encode_length(length as usize),
                    encode_distance(distance as usize),
                ) {
                    let (lcode, lbits) = code_for(&lit_codes, len_sym as usize);
                    writer.write_bits(u32::from(lcode), lbits);
                    if len_ebits > 0 {
                        writer.write_bits(len_extra, len_ebits);
                    }
                    let (dcode, dbits) = code_for(&dist_codes, dist_sym as usize);
                    writer.write_bits(u32::from(dcode), dbits);
                    if dist_ebits > 0 {
                        writer.write_bits(dist_extra, dist_ebits);
                    }
                }
            }
        }
    }

    // End of block.
    let (code, bits) = code_for(&lit_codes, 256);
    writer.write_bits(u32::from(code), bits);
}

/// Encode tokens using fixed Huffman codes (BTYPE=01).
///
/// Takes an iterator for the same reason [`encode_dynamic`] does, though this
/// one walks it only once, so the `Clone` bound is not needed here.
#[allow(clippy::arithmetic_side_effects)]
fn encode_fixed<I>(writer: &mut BitWriter, tokens: I, bfinal: bool)
where
    I: Iterator<Item = LzToken>,
{
    writer.write_bits(u32::from(bfinal), 1); // BFINAL
    writer.write_bits(1, 2); // BTYPE=01

    for token in tokens {
        match token {
            LzToken::Literal(b) => {
                let (code, bits) = fixed_code(u16::from(b));
                writer.write_bits(u32::from(code), bits);
            }
            LzToken::Match { length, distance } => {
                if let (
                    Some((len_sym, len_extra, len_ebits)),
                    Some((dist_sym, dist_extra, dist_ebits)),
                ) = (
                    encode_length(length as usize),
                    encode_distance(distance as usize),
                ) {
                    let (lcode, lbits) = fixed_code(len_sym);
                    writer.write_bits(u32::from(lcode), lbits);
                    if len_ebits > 0 {
                        writer.write_bits(len_extra, len_ebits);
                    }
                    let (dcode, dbits) = fixed_dist_code(dist_sym);
                    writer.write_bits(u32::from(dcode), dbits);
                    if dist_ebits > 0 {
                        writer.write_bits(dist_extra, dist_ebits);
                    }
                }
            }
        }
    }

    // End of block.
    let (code, bits) = fixed_code(256);
    writer.write_bits(u32::from(code), bits);
}

/// Compress data using DEFLATE with optimal Huffman encoding.
///
/// Uses LZ77 with hash-chain string matching, then tries both fixed
/// and dynamic Huffman encoding and picks the smaller output.  Dynamic
/// Huffman typically saves 20-40% for text and structured data.
///
/// Based on RFC 1951 §3.2.5–3.2.7 and zlib's deflate strategy.
pub fn deflate(data: &[u8]) -> Vec<u8> {
    deflate_with_chain(data, DEFAULT_MAX_CHAIN)
}

/// Compress with an explicit effort level, 1 (fastest) … 9 (smallest).
///
/// Identical to [`deflate`] except for how hard the LZ77 pass looks for a
/// match: the level selects the hash-chain depth via [`level_max_chain`].
/// The output is a valid DEFLATE stream at every level and decodes with
/// [`inflate`] regardless — the level trades encoder time for ratio and
/// changes nothing about the format.
///
/// Levels outside 1..=9 clamp; see [`level_max_chain`] for why this is not a
/// `Result`.
///
/// `deflate(data)` is **not** `deflate_level(data, 6)` — it is level 3. See
/// [`DEFAULT_MAX_CHAIN`] for why the default was left where it was.
///
/// # Example
///
/// ```
/// let data = b"the quick brown fox jumps over the lazy dog".repeat(20);
/// let small = deflate::deflate_level(&data, 9);
/// let fast = deflate::deflate_level(&data, 1);
/// assert_eq!(deflate::inflate(&small).unwrap(), data);
/// assert_eq!(deflate::inflate(&fast).unwrap(), data);
/// ```
#[must_use]
pub fn deflate_level(data: &[u8], level: u8) -> Vec<u8> {
    deflate_with_chain(data, level_max_chain(level))
}

/// The largest payload one stored (BTYPE=00) block can carry.
///
/// RFC 1951 §3.2.4 gives a stored block a 16-bit LEN, so a longer run of
/// incompressible data has to be split across several blocks.
const MAX_STORED_BLOCK: usize = 65535;

/// The fixed overhead a stored block adds to its payload: one byte holding the
/// three header bits after the flush to a byte boundary, then LEN and NLEN.
const STORED_BLOCK_OVERHEAD: usize = 5;

/// An upper bound, in bytes, on a literals-only dynamic-Huffman block header.
///
/// HLIT/HDIST/HCLEN and the permuted code-length lengths come to at most 71
/// bits; the RLE of the 286 literal lengths plus the single (empty) distance
/// length is at most 287 symbols at 7 code bits + 7 extra bits each. That is
/// 4089 bits, or 512 bytes, and 600 leaves room without being so slack that
/// the estimate stops discriminating. Used only to decide whether the
/// literals-only candidate is worth *encoding* — never to size a buffer.
const DYNAMIC_HEADER_BOUND: usize = 600;

/// Exact payload size, plus a header bound, for encoding `data` as literals
/// only under a dynamic Huffman code.
///
/// The payload figure is not a guess: it runs the same [`build_code_lengths`]
/// the real encoder would, over the same histogram, and sums length × freq.
/// So this answers "could switching the LZ77 stage off possibly win here?"
/// cheaply — one pass over the data and one 286-symbol tree build — and lets
/// the common case (data that actually compresses) skip a whole extra encode.
fn literal_only_size_estimate(data: &[u8]) -> usize {
    let mut freq = [0u32; 286];
    for &b in data {
        bump(&mut freq, b as usize);
    }
    bump(&mut freq, 256); // End-of-block.

    let lengths = build_code_lengths(&freq, MAX_CODE_BITS);
    let mut bits: u64 = 0;
    for (f, l) in freq.iter().zip(lengths.iter()) {
        bits = bits.saturating_add(u64::from(*f).saturating_mul(u64::from(*l)));
    }
    let bytes = usize::try_from(bits.div_ceil(8)).unwrap_or(usize::MAX);
    bytes.saturating_add(DYNAMIC_HEADER_BOUND)
}

/// The exact size [`deflate_stored`] will produce for `data`.
fn stored_size(data: &[u8]) -> usize {
    let blocks = data.len().div_ceil(MAX_STORED_BLOCK).max(1);
    data.len()
        .saturating_add(blocks.saturating_mul(STORED_BLOCK_OVERHEAD))
}

/// Compress `data`, choosing among every encoding this crate can produce.
///
/// # Why there are four candidates and not two
///
/// DEFLATE lets an encoder pick per block, and the only way to be sure the
/// pick is right is to price each one. Two of these exist to bound the damage
/// rather than to win often:
///
/// | Candidate | Wins on | Guarantee it provides |
/// |---|---|---|
/// | LZ77 + fixed Huffman | short, repetitive input where a tree costs more than it saves | — |
/// | LZ77 + dynamic Huffman | almost everything real | — |
/// | literals + dynamic Huffman | data with a skewed byte distribution but no usable repeats | never worse than order-0 Huffman coding |
/// | stored | data with neither | never meaningfully larger than the input |
///
/// The last two are not hypothetical. `deflate` used to inflate 32 KiB of
/// random bytes to 32816 — *larger than its input* — because the stored path
/// was reachable only for inputs of 64 bytes or fewer. And repairing the LZ77
/// match-finder (see [`find_best_match`]) made low-alphabet random data, DNA
/// being the obvious real example, 12% larger than the accidentally
/// Huffman-only encoder it replaced: matches were found, and at two bits per
/// literal every one of them cost more than the literals it displaced. zlib
/// carries the same weakness; pricing a literals-only candidate removes it.
///
/// Neither guard costs anything on data that compresses. The literals-only
/// candidate is *estimated* first ([`literal_only_size_estimate`]) and only
/// encoded if the estimate beats the best LZ77 result, and the stored size is
/// arithmetic, not an encode.
fn deflate_with_chain(data: &[u8], max_chain: usize) -> Vec<u8> {
    // For very small inputs, just use a stored block.
    //
    // Level-independent on purpose: below the threshold there is no match to
    // find at any chain depth, so every level agrees here and the effort knob
    // is genuinely irrelevant rather than merely ignored.
    if data.len() <= 64 {
        let mut writer = BitWriter::new();
        deflate_stored(&mut writer, data, true);
        return writer.into_bytes();
    }

    // Run LZ77 once, then encode with both Huffman strategies.
    let tokens = lz77_tokenize(data, max_chain);

    let mut best = {
        let mut writer = BitWriter::new();
        encode_fixed(&mut writer, tokens.iter().copied(), true);
        writer.into_bytes()
    };

    {
        let mut writer = BitWriter::new();
        encode_dynamic(&mut writer, tokens.iter().copied(), true);
        let dynamic_output = writer.into_bytes();
        if dynamic_output.len() < best.len() {
            best = dynamic_output;
        }
    }

    // Candidate 3: the same bytes with the LZ77 stage switched off. Only worth
    // pricing if LZ77 actually emitted a match — with no match the two streams
    // are identical — and only worth encoding if the exact payload arithmetic
    // says it could win.
    let has_match = tokens.iter().any(|t| matches!(t, LzToken::Match { .. }));
    if has_match && literal_only_size_estimate(data) < best.len() {
        let mut writer = BitWriter::new();
        encode_dynamic(
            &mut writer,
            data.iter().copied().map(LzToken::Literal),
            true,
        );
        let literal_output = writer.into_bytes();
        if literal_output.len() < best.len() {
            best = literal_output;
        }
    }

    // Candidate 4: stored. Its size is known without encoding, so this costs a
    // division unless it wins.
    if stored_size(data) < best.len() {
        let mut writer = BitWriter::new();
        deflate_stored(&mut writer, data, true);
        return writer.into_bytes();
    }

    best
}

/// Write `data` as stored (uncompressed) DEFLATE blocks.
///
/// Emits as many blocks as it takes: LEN is 16 bits, so a payload above
/// [`MAX_STORED_BLOCK`] cannot go in one. `bfinal` applies to the *last* block
/// only, the earlier ones being interior blocks of the same stream.
///
/// The split is not cosmetic. This function previously wrote a single block
/// with `data.len() as u16`, which for any payload above 65535 bytes declared
/// a truncated LEN and emitted the full data behind it — a silently corrupt
/// stream. It was unreachable then, because the only caller was a
/// 64-byte-or-fewer fast path; the stored candidate in [`deflate_with_chain`]
/// makes it reachable for any size, which is why it is fixed here rather than
/// noted as a latent hazard.
fn deflate_stored(writer: &mut BitWriter, data: &[u8], bfinal: bool) {
    let mut offset = 0usize;
    loop {
        let end = data.len().min(offset.saturating_add(MAX_STORED_BLOCK));
        let chunk = data.get(offset..end).unwrap_or(&[]);
        let last = end >= data.len();

        writer.write_bits(u32::from(bfinal && last), 1); // BFINAL
        writer.write_bits(0, 2); // BTYPE=00 (stored)
        writer.flush(); // align to byte

        // `chunk.len() <= MAX_STORED_BLOCK` by construction, so this conversion
        // is exact and the fallback is unreachable.
        let len = u16::try_from(chunk.len()).unwrap_or(u16::MAX);
        // LEN
        writer.write_byte(len as u8);
        writer.write_byte((len >> 8) as u8);
        // NLEN
        let nlen = !len;
        writer.write_byte(nlen as u8);
        writer.write_byte((nlen >> 8) as u8);
        // Raw data.
        for &b in chunk {
            writer.write_byte(b);
        }

        offset = end;
        if last {
            break;
        }
    }
}

/// Create a gzip-compressed byte vector from uncompressed data.
///
/// Wraps `deflate()` output in a gzip header (RFC 1952) with
/// CRC-32 and original-size trailer.
pub fn gzip(data: &[u8]) -> Vec<u8> {
    let compressed = deflate(data);
    let crc = crc32::crc32(data);
    let size = data.len() as u32;

    // 10-byte header plus 8-byte trailer around the compressed payload.
    let mut out = Vec::with_capacity(compressed.len().saturating_add(18));
    // Gzip header (10 bytes).
    out.push(0x1F);
    out.push(0x8B); // ID
    out.push(0x08); // CM = deflate
    out.push(0x00); // FLG
    out.extend_from_slice(&[0, 0, 0, 0]); // MTIME
    out.push(0x04); // XFL = fastest compression
    out.push(0xFF); // OS = unknown
    // DEFLATE data.
    out.extend_from_slice(&compressed);
    // Trailer: CRC32 + ISIZE.
    out.extend_from_slice(&crc.to_le_bytes());
    out.extend_from_slice(&size.to_le_bytes());
    out
}

// ---------------------------------------------------------------------------
// Gzip wrapper (RFC 1952) — decompression
// ---------------------------------------------------------------------------

/// Gzip flag bits.
#[allow(dead_code)]
const FTEXT: u8 = 1;
const FHCRC: u8 = 2;
const FEXTRA: u8 = 4;
const FNAME: u8 = 8;
const FCOMMENT: u8 = 16;

/// Decompress gzip-compressed data.
///
/// Parses the gzip header (RFC 1952), decompresses the DEFLATE payload,
/// and verifies the CRC32 and original size from the trailer.
///
/// Output is capped at [`MAX_OUTPUT`]; use [`gunzip_limited`] to set the cap.
///
/// # Errors
///
/// [`Error::BadWrapperHeader`] if the magic or compression method is wrong,
/// [`Error::SizeMismatch`] or [`Error::ChecksumMismatch`] if the trailer
/// disagrees with what was decompressed, or any [`inflate`] error from the
/// payload.
pub fn gunzip(data: &[u8]) -> Result<Vec<u8>> {
    gunzip_limited(data, MAX_OUTPUT)
}

/// [`gunzip`] with a caller-chosen output cap.
///
/// Note that gzip's trailer declares the original size, so a caller could in
/// principle read ISIZE first and size its buffer from that. It must not: the
/// trailer is attacker-controlled and is only *checked* after decompression,
/// so trusting it to bound the work is trusting the thing being validated.
/// The cap has to come from the caller's own knowledge.
///
/// # Errors
///
/// As [`gunzip`], plus [`Error::OutputTooLarge`] once the output reaches
/// `limit`.
pub fn gunzip_limited(data: &[u8], limit: usize) -> Result<Vec<u8>> {
    if data.len() < 18 {
        return Err(Error::UnexpectedEnd); // minimum gzip size
    }

    let mut reader = BitReader::new(data);

    // Gzip header.
    let id1 = reader.read_byte()?;
    let id2 = reader.read_byte()?;
    if id1 != 0x1F || id2 != 0x8B {
        return Err(Error::BadWrapperHeader); // not gzip
    }

    let cm = reader.read_byte()?;
    if cm != 8 {
        return Err(Error::BadWrapperHeader); // only deflate
    }

    let flg = reader.read_byte()?;
    let _mtime = reader.read_u16_le()?; // skip MTIME (4 bytes)
    let _mtime_hi = reader.read_u16_le()?;
    let _xfl = reader.read_byte()?; // extra flags
    let _os = reader.read_byte()?; // OS identifier

    // Skip optional extra field.
    if (flg & FEXTRA) != 0 {
        let xlen = reader.read_u16_le()?;
        for _ in 0..xlen {
            let _ = reader.read_byte()?;
        }
    }

    // Skip optional original filename (null-terminated).
    if (flg & FNAME) != 0 {
        loop {
            let b = reader.read_byte()?;
            if b == 0 {
                break;
            }
        }
    }

    // Skip optional comment (null-terminated).
    if (flg & FCOMMENT) != 0 {
        loop {
            let b = reader.read_byte()?;
            if b == 0 {
                break;
            }
        }
    }

    // Skip optional header CRC16.
    if (flg & FHCRC) != 0 {
        let _ = reader.read_u16_le()?;
    }

    // The remaining data (from current position to 8 bytes before
    // the end) is the DEFLATE compressed data.  The last 8 bytes
    // are CRC32 + ISIZE.
    let deflate_start = reader.byte_pos();
    if data.len() < deflate_start.wrapping_add(8) {
        return Err(Error::UnexpectedEnd);
    }
    let deflate_end = data.len().wrapping_sub(8);
    let deflate_data = data
        .get(deflate_start..deflate_end)
        .ok_or(Error::UnexpectedEnd)?;

    let output = inflate_limited(deflate_data, limit)?;

    // Verify CRC32 and ISIZE from trailer.
    let (expected_crc, expected_size) = match data.get(deflate_end..) {
        Some(&[c0, c1, c2, c3, s0, s1, s2, s3]) => (
            u32::from_le_bytes([c0, c1, c2, c3]),
            u32::from_le_bytes([s0, s1, s2, s3]),
        ),
        _ => return Err(Error::UnexpectedEnd),
    };

    // Verify size (mod 2^32). Checked before the CRC because it is the
    // cheaper of the two and because, when both are wrong, the size is the
    // more informative failure: it says how much of the stream arrived.
    let actual_size = output.len() as u32;
    if actual_size != expected_size {
        return Err(Error::SizeMismatch {
            expected: expected_size,
            actual: actual_size,
        });
    }

    // Verify CRC32 (using the ISO 3309 / ITU-T V.42 polynomial, not
    // CRC32C).  Gzip uses CRC-32 (polynomial 0xEDB88320 reflected).
    let actual_crc = crc32::crc32(&output);
    if actual_crc != expected_crc {
        return Err(Error::ChecksumMismatch {
            expected: expected_crc,
            actual: actual_crc,
        });
    }

    Ok(output)
}

// ---------------------------------------------------------------------------
// Adler-32 checksum (RFC 1950)
// ---------------------------------------------------------------------------

/// Compute the Adler-32 checksum of `data`.
///
/// Used by zlib as an integrity check.  Simpler and faster than CRC-32
/// but with slightly weaker error detection.
///
/// RFC 1950 §8: adler32 = (s2 << 16) | s1
///   s1 = 1 + sum of all bytes mod 65521
///   s2 = sum of all running s1 values mod 65521
#[allow(clippy::arithmetic_side_effects)]
pub fn adler32(data: &[u8]) -> u32 {
    const MOD: u32 = 65521;
    let mut s1: u32 = 1;
    let mut s2: u32 = 0;

    // Process in chunks of 5552 to avoid overflow of the u32
    // accumulators before taking the modulus.  5552 is the largest
    // n such that 255*n*(n+1)/2 + n*255 < 2^32.
    for chunk in data.chunks(5552) {
        for &byte in chunk {
            s1 = s1.wrapping_add(u32::from(byte));
            s2 = s2.wrapping_add(s1);
        }
        s1 %= MOD;
        s2 %= MOD;
    }

    (s2 << 16) | s1
}

// ---------------------------------------------------------------------------
// zlib wrapper (RFC 1950)
// ---------------------------------------------------------------------------

/// Decompress zlib-wrapped data (RFC 1950).
///
/// Format: 2-byte header + raw DEFLATE + 4-byte Adler-32 (big-endian).
///
/// The header encodes compression method (must be 8 = deflate) and
/// window size.  We validate the checksum on the decompressed output.
///
/// Output is capped at [`MAX_OUTPUT`]; use [`zlib_inflate_limited`] to set
/// the cap.
///
/// # Errors
///
/// [`Error::BadWrapperHeader`] if the two header bytes are not a zlib header,
/// [`Error::PresetDictionary`] if the stream needs a dictionary it does not
/// carry, [`Error::ChecksumMismatch`] if the Adler-32 disagrees, or any
/// [`inflate`] error from the DEFLATE payload.
pub fn zlib_inflate(data: &[u8]) -> Result<Vec<u8>> {
    zlib_inflate_limited(data, MAX_OUTPUT)
}

/// [`zlib_inflate`] with a caller-chosen output cap.
///
/// # Errors
///
/// As [`zlib_inflate`], plus [`Error::OutputTooLarge`] once the output
/// reaches `limit`.
pub fn zlib_inflate_limited(data: &[u8], limit: usize) -> Result<Vec<u8>> {
    // Six bytes is the shortest possible zlib stream: two header bytes, a
    // one-byte empty final block, four bytes of Adler-32. Checking it here
    // rather than in the header reads below is what lets `cmf`/`flg` be
    // taken without a bounds test each.
    let (cmf, flg) = match data {
        [cmf, flg, ..] if data.len() >= 6 => (*cmf, *flg),
        _ => return Err(Error::UnexpectedEnd),
    };

    // CMF: lower 4 bits = CM (compression method), upper 4 = CINFO.
    let cm = cmf & 0x0F;
    if cm != 8 {
        // Only deflate (method 8) is defined.
        return Err(Error::BadWrapperHeader);
    }

    // Header checksum: (CMF*256 + FLG) must be divisible by 31.
    let header_check = u16::from(cmf)
        .wrapping_mul(256)
        .wrapping_add(u16::from(flg));
    if header_check % 31 != 0 {
        return Err(Error::BadWrapperHeader);
    }

    // FDICT bit (bit 5 of FLG): preset dictionary.  We don't support
    // preset dictionaries — they're rare (used in some PDF streams).
    if flg & 0x20 != 0 {
        return Err(Error::PresetDictionary);
    }

    // Compressed data starts at offset 2.
    let compressed = data
        .get(2..data.len().saturating_sub(4))
        .ok_or(Error::UnexpectedEnd)?;

    // --- Decompress ---
    let decompressed = inflate_limited(compressed, limit)?;

    // --- Verify Adler-32 (big-endian, last 4 bytes) ---
    let trailer_start = data.len().saturating_sub(4);
    let stored_adler = match data.get(trailer_start..) {
        Some(&[a, b, c, d]) => u32::from_be_bytes([a, b, c, d]),
        // Unreachable given the six-byte check above, but expressing it as a
        // match rather than four indexes means the bound is checked rather
        // than argued for in a comment.
        _ => return Err(Error::UnexpectedEnd),
    };

    let computed_adler = adler32(&decompressed);
    if stored_adler != computed_adler {
        return Err(Error::ChecksumMismatch {
            expected: stored_adler,
            actual: computed_adler,
        });
    }

    Ok(decompressed)
}

/// Compress data into zlib format (RFC 1950).
///
/// Produces: CMF + FLG + raw DEFLATE + Adler-32 (big-endian).
///
/// Uses deflation level implied by our fixed-Huffman `deflate()`.
/// Window size is set to 32768 (maximum, CINFO=7).
#[allow(clippy::arithmetic_side_effects)]
pub fn zlib_deflate(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();

    // --- Header ---
    // CMF: CM=8 (deflate), CINFO=7 (32K window) → 0x78
    let cmf: u8 = 0x78;
    // FLG: FLEVEL=2 (default compression), FDICT=0
    // Must satisfy (CMF*256 + FLG) % 31 == 0
    // 0x78 * 256 = 30720.  30720 + FLG ≡ 0 (mod 31).
    // 30720 mod 31 = 30720 - 991*31 = 30720 - 30721 = ... let me compute.
    // 31 * 991 = 30721.  30720 mod 31 = 30720 - 990*31 = 30720 - 30690 = 30.
    // So FLG must be 31 - 30 = 1.  But FLEVEL=2 means bits 6-7 = 10 (0x80).
    // 30720 + 0x80 = 30848. 30848 mod 31 = 30848 - 995*31 = 30848 - 30845 = 3.
    // FLG = 0x80 + (31-3) = 0x80 + 28 = 0x9C.
    // Check: 30720 + 0x9C = 30720 + 156 = 30876. 30876 / 31 = 996. 996*31=30876. ✓
    let flg: u8 = 0x9C;

    out.push(cmf);
    out.push(flg);

    // --- Compressed data ---
    let compressed = deflate(data);
    out.extend_from_slice(&compressed);

    // --- Adler-32 (big-endian) ---
    let checksum = adler32(data);
    out.push((checksum >> 24) as u8);
    out.push((checksum >> 16) as u8);
    out.push((checksum >> 8) as u8);
    out.push(checksum as u8);

    out
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]
mod tests {
    use super::{
        BitWriter, DEFAULT_MAX_CHAIN, Error, HuffmanTable, LzToken, MAX_OUTPUT, adler32, deflate,
        deflate_level, deflate_stored, encode_dynamic, encode_fixed, fixed_lit_lengths, gunzip,
        gunzip_limited, gzip, inflate, inflate_limited, level_max_chain, lz77_tokenize,
        stored_size, zlib_deflate, zlib_inflate, zlib_inflate_limited,
    };
    use alloc::format;
    use alloc::string::String;
    use alloc::vec::Vec;

    /// Wrap `payload` in a gzip stream whose DEFLATE part is a single stored
    /// block, so a test can assert on the *wrapper* without depending on the
    /// compressor. `crc` and `size` are taken as parameters so a test can
    /// deliberately put the wrong ones in the trailer.
    fn gzip_stored(payload: &[u8], crc: u32, size: u32) -> Vec<u8> {
        let mut gz = Vec::new();
        gz.extend_from_slice(&[0x1F, 0x8B, 0x08, 0x00, 0, 0, 0, 0, 0x00, 0xFF]);
        gz.push(0x01); // BFINAL=1, BTYPE=00 (stored)
        let len = payload.len() as u16;
        gz.extend_from_slice(&len.to_le_bytes());
        gz.extend_from_slice(&(!len).to_le_bytes());
        gz.extend_from_slice(payload);
        gz.extend_from_slice(&crc.to_le_bytes());
        gz.extend_from_slice(&size.to_le_bytes());
        gz
    }

    /// A stored block is the one DEFLATE encoding that can be written by hand
    /// and read by eye, which makes it the right vector for proving the bit
    /// reader's byte alignment rather than the Huffman decoder.
    #[test]
    fn stored_block() {
        let stored = [0x01, 0x05, 0x00, 0xFA, 0xFF, b'h', b'e', b'l', b'l', b'o'];
        assert_eq!(inflate(&stored).unwrap(), b"hello");
    }

    /// The fixed Huffman table's shape is fully determined by RFC 1951 3.2.6,
    /// so these three counts are a check value: 24 seven-bit codes, 152
    /// eight-bit, 112 nine-bit. A transcription error in `fixed_lit_lengths`
    /// moves one of them.
    #[test]
    fn fixed_table_shape() {
        let table = HuffmanTable::build(&fixed_lit_lengths()).unwrap();
        assert_eq!(table.counts[7], 24);
        assert_eq!(table.counts[8], 152);
        assert_eq!(table.counts[9], 112);
    }

    /// Round-tripping is the compressor's only real test: the encoder and the
    /// decoder are independent enough that agreeing on a non-trivial input is
    /// evidence, and the input mixes literals, long runs and repeats so that
    /// all three of the encoder's block strategies get exercised.
    #[test]
    fn deflate_round_trip() {
        let original: &[u8] = b"Hello, world! Hello, world! This is a test of DEFLATE \
            compression. AAAAAAAAAAAAAAAAAAAAAA BBBBBBBBBBBBBBBB repetition helps.";
        assert_eq!(inflate(&deflate(original)).unwrap(), original);
    }

    /// The empty input is the case every framing gets wrong in a different
    /// way: gzip must still emit a final block, and zlib must still emit an
    /// Adler-32 of 1.
    #[test]
    fn empty_round_trips_through_every_framing() {
        assert!(inflate(&deflate(b"")).unwrap().is_empty());
        assert!(gunzip(&gzip(b"")).unwrap().is_empty());
        assert!(zlib_inflate(&zlib_deflate(b"")).unwrap().is_empty());
        assert_eq!(adler32(b""), 1);
    }

    /// The three input shapes that drive the encoder into different block
    /// types: incompressible, maximally compressible, and mixed.
    #[test]
    fn round_trip_pathological_inputs() {
        let all_same = [0x5Au8; 1024];
        assert_eq!(inflate(&deflate(&all_same)).unwrap(), &all_same[..]);

        // A Lehmer generator, so the bytes are reproducible but have no
        // structure the LZ77 matcher can find.
        let mut seed: u32 = 12345;
        let mut noise = Vec::with_capacity(1024);
        for _ in 0..1024 {
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
            noise.push((seed >> 16) as u8);
        }
        assert_eq!(inflate(&deflate(&noise)).unwrap(), noise);

        let mut mixed = Vec::with_capacity(8192);
        for i in 0..8192u32 {
            mixed.push(if i % 7 == 0 { b'x' } else { (i % 251) as u8 });
        }
        assert_eq!(inflate(&deflate(&mixed)).unwrap(), mixed);
    }

    /// Adler-32's published check values (both match Python's
    /// `zlib.adler32`).
    #[test]
    fn adler32_check_value() {
        assert_eq!(adler32(b"123456789"), 0x091E_01DE);
        assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
    }

    /// A gzip stream whose payload is a stored block, so this tests the
    /// wrapper and the trailer rather than the codec.
    #[test]
    fn gzip_wrapper() {
        let payload = b"hello\n";
        let gz = gzip_stored(payload, crc32::crc32(payload), payload.len() as u32);
        assert_eq!(gunzip(&gz).unwrap(), payload);
    }

    /// The two trailer fields fail differently and must be told apart: a
    /// wrong CRC says the bytes are wrong, a wrong ISIZE says the stream was
    /// cut or concatenated. The kernel version returned `CorruptedData` for
    /// both and printed the difference to a console the caller could not
    /// read.
    #[test]
    fn gzip_trailer_mismatches_are_distinguishable() {
        let payload = b"hello\n";
        let good = crc32::crc32(payload);
        let size = payload.len() as u32;

        let bad_crc = gzip_stored(payload, good ^ 1, size);
        assert_eq!(
            gunzip(&bad_crc),
            Err(Error::ChecksumMismatch {
                expected: good ^ 1,
                actual: good,
            })
        );

        // Checked before the CRC, so a stream wrong in both ways reports the
        // size.
        let bad_size = gzip_stored(payload, good, size + 1);
        assert_eq!(
            gunzip(&bad_size),
            Err(Error::SizeMismatch {
                expected: size + 1,
                actual: size,
            })
        );
    }

    /// zlib's header is three independent checks, and a decoder that skips
    /// any of them will happily decode something that is not a zlib stream.
    #[test]
    fn zlib_header_is_validated() {
        let mut z = zlib_deflate(b"payload");
        assert_eq!(zlib_inflate(&z).unwrap(), b"payload");

        // CM must be 8. Changing the low nibble also breaks the %31 check, so
        // pick a CM that keeps it: this asserts the CM test exists in its own
        // right rather than being subsumed by the header checksum.
        let mut bad_cm = z.clone();
        bad_cm[0] = 0x77; // CM=7, CINFO=7
        // Search for an FLG that restores the header check *and* leaves FDICT
        // clear, so the only thing wrong with this header is the CM.
        while (u16::from(bad_cm[0]) * 256 + u16::from(bad_cm[1])) % 31 != 0 || bad_cm[1] & 0x20 != 0
        {
            bad_cm[1] = bad_cm[1].wrapping_add(1);
        }
        assert_eq!(zlib_inflate(&bad_cm), Err(Error::BadWrapperHeader));

        // The header checksum must hold.
        let mut bad_check = z.clone();
        bad_check[1] = bad_check[1].wrapping_add(1);
        assert_eq!(zlib_inflate(&bad_check), Err(Error::BadWrapperHeader));

        // FDICT is well-formed but unsupported, and says so. Search by ones,
        // not by 0x40: stepping the top two bits only visits four FLG values,
        // none of which happens to satisfy the header check for this CMF, so
        // that search would never terminate.
        let mut fdict = z.clone();
        while (u16::from(fdict[0]) * 256 + u16::from(fdict[1])) % 31 != 0 || fdict[1] & 0x20 == 0 {
            fdict[1] = fdict[1].wrapping_add(1);
        }
        assert_eq!(zlib_inflate(&fdict), Err(Error::PresetDictionary));

        // And the Adler-32 is verified, not merely parsed.
        let last = z.len() - 1;
        z[last] ^= 0xFF;
        assert!(matches!(
            zlib_inflate(&z),
            Err(Error::ChecksumMismatch { .. })
        ));
    }

    /// Not-gzip is rejected on the magic, and a truncated stream is reported
    /// as truncated rather than as corrupt -- the distinction a caller needs
    /// to decide whether retrying the download could help.
    #[test]
    fn gzip_header_and_truncation() {
        assert_eq!(gunzip(&[0u8; 32]), Err(Error::BadWrapperHeader));
        assert_eq!(gunzip(b"short"), Err(Error::UnexpectedEnd));
        let gz = gzip(b"some reasonably compressible payload payload payload");
        assert_eq!(gunzip(&gz[..gz.len() - 12]), Err(Error::UnexpectedEnd));
    }

    /// Block type 3 is reserved by RFC 1951 and has no meaning, so it must be
    /// refused rather than fall through to one of the three that do.
    #[test]
    fn reserved_block_type() {
        // BFINAL=1, BTYPE=11 -> low three bits 111.
        assert_eq!(inflate(&[0x07, 0, 0, 0]), Err(Error::ReservedBlockType));
    }

    /// A stored block whose NLEN is not the complement of LEN is the
    /// format's own integrity check, and it is the cheapest way to catch a
    /// stream that has slipped out of byte alignment.
    #[test]
    fn stored_length_complement_is_checked() {
        let bad = [0x01, 0x05, 0x00, 0xFB, 0xFF, b'h', b'e', b'l', b'l', b'o'];
        assert_eq!(inflate(&bad), Err(Error::StoredLengthMismatch));
    }

    /// The whole point of the limit parameter: the cap is the caller's, it is
    /// enforced before the byte is appended, and a stream that fits is
    /// unaffected. `limit == 0` accepts an empty stream and nothing else.
    #[test]
    fn output_limit_is_enforced() {
        let payload = [b'q'; 4096];
        let compressed = deflate(&payload);

        assert_eq!(inflate_limited(&compressed, 4096).unwrap(), &payload[..]);
        assert_eq!(
            inflate_limited(&compressed, 4095),
            Err(Error::OutputTooLarge)
        );
        assert_eq!(inflate_limited(&compressed, 0), Err(Error::OutputTooLarge));
        assert!(inflate_limited(&deflate(b""), 0).unwrap().is_empty());

        // The limit reaches through both wrappers, which is where a caller
        // decoding an image or an HTTP body actually needs it.
        let gz = gzip(&payload);
        assert_eq!(gunzip_limited(&gz, 4096).unwrap(), &payload[..]);
        assert_eq!(gunzip_limited(&gz, 4095), Err(Error::OutputTooLarge));

        let z = zlib_deflate(&payload);
        assert_eq!(zlib_inflate_limited(&z, 4096).unwrap(), &payload[..]);
        assert_eq!(zlib_inflate_limited(&z, 4095), Err(Error::OutputTooLarge));

        // And the unsuffixed names are exactly the limited ones at MAX_OUTPUT.
        assert_eq!(
            inflate(&compressed),
            inflate_limited(&compressed, MAX_OUTPUT)
        );
    }

    /// A stored block declaring more bytes than the stream holds must not
    /// read past the end. The cap is not what stops this -- the bit reader is
    /// -- so this is checked with a generous limit.
    #[test]
    fn stored_block_cannot_overrun() {
        let bad = [0x01, 0xFF, 0xFF, 0x00, 0x00, b'h', b'i'];
        assert_eq!(inflate_limited(&bad, MAX_OUTPUT), Err(Error::UnexpectedEnd));
    }

    /// Every prefix of a valid stream must fail rather than produce the whole
    /// payload. This is the cheap fuzz: truncation is the one malformation
    /// that can be generated exhaustively, and it reaches most of the
    /// decoder's early-return paths.
    #[test]
    fn no_prefix_of_a_valid_stream_decodes_to_the_whole() {
        let payload: &[u8] = b"the quick brown fox jumps over the lazy dog, twice over";
        let compressed = deflate(payload);
        for cut in 0..compressed.len() {
            if let Ok(partial) = inflate(&compressed[..cut]) {
                assert_ne!(
                    partial.as_slice(),
                    payload,
                    "a truncated stream decoded to the full payload at cut {cut}"
                );
            }
        }
        assert_eq!(inflate(&compressed).unwrap(), payload);
    }

    /// The encoder picks between a fixed Huffman table and a per-block one,
    /// and the choice is only worth making if the per-block table actually
    /// wins on skewed data. This compares the two encoders directly rather
    /// than inferring the choice from `deflate`'s output size, which is why
    /// it lives here and not in the kernel's boot self-test: `BitWriter`,
    /// `encode_fixed` and `encode_dynamic` are private, and exposing them so
    /// a boot rung could reach them would be an API that exists only for a
    /// test.
    #[test]
    fn dynamic_huffman_beats_fixed_on_skewed_data() {
        let mut skewed = Vec::with_capacity(2048);
        for i in 0..2048usize {
            skewed.push(match i % 16 {
                0..=7 => b'a',              // 50%
                8..=11 => b'b',             // 25%
                12..=13 => b'c',            // 12.5%
                _ => (i % 26) as u8 + b'd', // 12.5%, varied
            });
        }

        let tokens = lz77_tokenize(&skewed, DEFAULT_MAX_CHAIN);
        let mut fixed_w = BitWriter::new();
        encode_fixed(&mut fixed_w, tokens.iter().copied(), true);
        let fixed_size = fixed_w.into_bytes().len();

        let mut dyn_w = BitWriter::new();
        encode_dynamic(&mut dyn_w, tokens.iter().copied(), true);
        let dyn_size = dyn_w.into_bytes().len();

        assert!(
            dyn_size < fixed_size,
            "a per-block Huffman table should beat the fixed one on data this              skewed (dynamic {dyn_size}, fixed {fixed_size}); if it does not,              the table-building or the code-length RLE is wrong"
        );
        assert_eq!(inflate(&deflate(&skewed)).unwrap(), skewed);
    }

    /// Flipping one byte anywhere in a compressed stream must not panic. The
    /// decoder parses attacker-controlled input, so an index that is out of
    /// range has to become an error, and the only way to be sure is to try
    /// every single-byte corruption.
    #[test]
    fn single_byte_corruption_never_panics() {
        let payload: &[u8] = b"aaaaaabbbbbbccccccdddddd the quick brown fox 12345678901234567890";
        for framing in 0..3 {
            let good = match framing {
                0 => deflate(payload),
                1 => gzip(payload),
                _ => zlib_deflate(payload),
            };
            for pos in 0..good.len() {
                for delta in [0x01u8, 0x80, 0xFF] {
                    let mut bad = good.clone();
                    bad[pos] ^= delta;
                    // The result is uninteresting; not panicking is the test.
                    let _ = match framing {
                        0 => inflate_limited(&bad, 1 << 20),
                        1 => gunzip_limited(&bad, 1 << 20),
                        _ => zlib_inflate_limited(&bad, 1 << 20),
                    };
                }
            }
        }
    }

    /// Every variant must produce a sentence, and no two may produce the same
    /// one.
    ///
    /// The *exhaustiveness* half is enforced by the compiler, in the `Display`
    /// impl itself: it has no wildcard arm, so a new variant without a message
    /// fails the build. What a test adds is the half a compiler cannot check —
    /// that the wording is an explanation rather than an identifier, and that
    /// two different failures do not read alike. A duplicate message is a
    /// variant distinction that never reaches the person it was drawn for.
    #[test]
    fn every_error_variant_reads_as_a_sentence_and_no_two_read_alike() {
        let all = [
            Error::UnexpectedEnd,
            Error::ReservedBlockType,
            Error::StoredLengthMismatch,
            Error::InvalidHuffmanTable,
            Error::InvalidSymbol,
            Error::DistanceTooFar,
            Error::OutputTooLarge,
            Error::BadWrapperHeader,
            Error::PresetDictionary,
            Error::ChecksumMismatch {
                expected: 0x1234_5678,
                actual: 0x8765_4321,
            },
            Error::SizeMismatch {
                expected: 100,
                actual: 99,
            },
        ];
        let mut seen: Vec<String> = Vec::new();
        for e in all {
            let text = format!("{e}");
            assert!(!text.is_empty(), "{e:?} has no message");
            // A `Display` that forwards to `Debug` is the failure this catches:
            // it compiles, it is non-empty, and it puts `DistanceTooFar` in
            // front of somebody trying to open a wallpaper.
            assert_ne!(text, format!("{e:?}"), "{e:?} prints its identifier");
            assert!(
                text.chars().next().is_some_and(|c| !c.is_uppercase()),
                "{e:?}: capitalised, but callers embed this mid-sentence: {text}"
            );
            assert!(!seen.contains(&text), "two variants both say: {text}");
            seen.push(text);
        }
    }

    /// The two mismatch variants must print the values they compared.
    ///
    /// "checksum mismatch" says the file is damaged; the pair of numbers says
    /// *how* — an expected value of zero is a truncated download rather than a
    /// corrupted one, and that is the difference between retrying and not.
    #[test]
    fn a_mismatch_prints_both_of_the_numbers_it_compared() {
        let text = format!(
            "{}",
            Error::ChecksumMismatch {
                expected: 0x1234_5678,
                actual: 0x8765_4321,
            }
        );
        assert!(text.contains("12345678"), "expected value missing: {text}");
        assert!(text.contains("87654321"), "actual value missing: {text}");

        let text = format!(
            "{}",
            Error::SizeMismatch {
                expected: 100,
                actual: 99,
            }
        );
        assert!(text.contains("100"), "expected value missing: {text}");
        assert!(text.contains("99"), "actual value missing: {text}");
    }

    /// An incomplete (under-subscribed) Huffman code with a single symbol must
    /// be **accepted**, not rejected.
    ///
    /// It is what a DEFLATE stream with exactly one distance code produces,
    /// real encoders emit it, and a decoder that rejects it fails on files that
    /// open everywhere else. `build` already gets this right — it rejects only
    /// `left < 0`, i.e. over-subscription. The test exists because nothing
    /// pinned the property, and it is precisely what a later "tighten the
    /// validation" change breaks without meaning to.
    ///
    /// Reported by lane C in `requests/c-a-deflate-error-has-no-display.md`,
    /// found by diffing the deleted `gui/imagecodec` decoder's tests against
    /// this crate's.
    #[test]
    fn a_lone_one_bit_code_is_under_subscribed_and_still_legal() {
        assert!(
            HuffmanTable::build(&[1, 0, 0]).is_ok(),
            "a single one-bit code is legal and real encoders emit it"
        );
        // The neighbouring case must still fail: three symbols cannot all hold
        // a one-bit code, and accepting that would be over-subscription.
        assert_eq!(
            HuffmanTable::build(&[1, 1, 1]).err(),
            Some(Error::InvalidHuffmanTable),
            "over-subscription must still be rejected"
        );
    }

    // -----------------------------------------------------------------
    // Compression levels
    // -----------------------------------------------------------------

    /// Data over a deliberately tiny alphabet, which is what makes chain depth
    /// observable.
    ///
    /// The naive choice — repeated English phrases — does *not* work, and the
    /// reason is worth recording because it looks like a passing test. With
    /// highly repetitive input the very first candidate on the chain already
    /// yields a `MAX_MATCH` (258-byte) match, so `find_best_match` takes its
    /// `break` on iteration one and every level produces **byte-identical**
    /// output. That is indistinguishable from the bug this test exists to
    /// catch.
    ///
    /// A small alphabet alone does not work either: uniformly random symbols
    /// have no exploitable redundancy at all, so the encoder emits literals
    /// at the entropy limit and, again, every level agrees exactly.
    ///
    /// What chain depth actually discriminates on is a position where the
    /// *nearest* candidate match is shorter than a *farther* one — a shallow
    /// search takes the near, short match; a deep one walks past it to the
    /// long one. So the corpus is built the way LZ77 expects to find data:
    /// mostly copies of random-length runs from random earlier offsets, with
    /// occasional literals. That yields many candidates per hash bucket with
    /// genuinely varied lengths, which is the only shape that makes the level
    /// knob measurable.
    fn level_test_corpus() -> Vec<u8> {
        // A small LCG, so the corpus is deterministic across runs and
        // platforms -- a flaky ratio assertion would be worse than no test.
        let mut state: u32 = 0x1234_5678;
        let mut rand = move || {
            state = state.wrapping_mul(1_103_515_245).wrapping_add(12345);
            (state >> 16) & 0x7fff
        };

        let mut v = Vec::with_capacity(49152);
        for _ in 0..64 {
            v.push(b'a' + (rand() % 26) as u8);
        }
        while v.len() < 49152 {
            if rand() % 5 == 0 {
                v.push(b'a' + (rand() % 26) as u8);
            } else {
                let src = (rand() as usize) % v.len();
                let len = 4 + (rand() as usize) % 40;
                let end = (src + len).min(v.len());
                // Copy through an index range rather than extend_from_within,
                // so the source is a snapshot and cannot overlap the tail we
                // are appending to.
                for i in src..end {
                    let b = v[i];
                    v.push(b);
                }
            }
        }
        v
    }

    #[test]
    fn every_level_round_trips() {
        let data = level_test_corpus();
        // 0 and 10 are outside 1..=9 and must clamp, not panic or corrupt.
        for level in 0..=10u8 {
            let out = deflate_level(&data, level);
            assert_eq!(
                inflate(&out).expect("level output must inflate"),
                data,
                "level {level} did not round-trip"
            );
        }
    }

    /// The property `zip -1..-9` actually depends on: the levels must not all
    /// be the same setting wearing nine names.  This is the regression that
    /// lane B's request was filed to prevent — adopting a level-less
    /// `deflate()` would have made every flag produce identical bytes.
    /// `n` pseudo-random bytes drawn from an `alphabet`-symbol alphabet.
    ///
    /// The alphabet size is the knob that matters: at 256 the data is
    /// incompressible by any means and only a stored block can avoid making it
    /// bigger; at 4 it is DNA-shaped — two bits per byte of genuine order-0
    /// redundancy, and no repeats an LZ77 match can profitably name.
    fn random_alphabet(n: usize, alphabet: u32) -> Vec<u8> {
        let mut state: u32 = 0xDEAD_BEEF;
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            state = state.wrapping_mul(1_103_515_245).wrapping_add(12345);
            out.push(((state >> 16) % alphabet) as u8);
        }
        out
    }

    /// Compressing must never make data meaningfully bigger.
    ///
    /// It used to: 32 KiB of random bytes came back as 32816, larger than the
    /// input, because the stored-block path was reachable only for inputs of
    /// 64 bytes or fewer and every longer input was forced through Huffman
    /// coding that had nothing to exploit. `.zip` and the kernel's filesystem
    /// compression both feed this already-compressed data routinely — a JPEG
    /// inside a zip is exactly this case — so it was not a corner.
    ///
    /// The bound is the input plus five bytes per stored block, which is the
    /// format's own floor (RFC 1951 §3.2.4: three header bits padded to a byte,
    /// then LEN and NLEN), not a tolerance chosen to make the test pass.
    #[test]
    fn incompressible_input_is_never_inflated() {
        for len in [65, 1000, 32768, 200_000] {
            let data = random_alphabet(len, 256);
            let out = deflate(&data);
            let floor = len + 5 * len.div_ceil(65535).max(1);
            assert!(
                out.len() <= floor,
                "deflate inflated {len} incompressible bytes to {} (stored floor is {floor})",
                out.len()
            );
            assert_eq!(inflate(&out).unwrap(), data, "{len}-byte round trip");
        }
    }

    /// Running LZ77 must never cost more than not running it.
    ///
    /// On a four-symbol alphabet a literal costs two bits, while the shortest
    /// match costs a length symbol plus a distance symbol plus up to thirteen
    /// extra bits — so every match the finder reports is a net loss, and a
    /// greedy encoder takes them anyway. Before the literals-only candidate
    /// existed this input encoded to 10298 bytes; the same bytes coded as
    /// literals take 9217. zlib has the same weakness and simply lives with it.
    ///
    /// The comparison is against the literal encoding computed here rather than
    /// a hard-coded number, so the test states the property ("never worse than
    /// order-0 Huffman") instead of pinning today's output size.
    #[test]
    fn lz77_never_loses_to_plain_huffman() {
        let data = random_alphabet(32768, 4);

        let mut writer = BitWriter::new();
        encode_dynamic(
            &mut writer,
            data.iter().copied().map(LzToken::Literal),
            true,
        );
        let literals_only = writer.into_bytes().len();

        let out = deflate(&data);
        assert!(
            out.len() <= literals_only,
            "deflate produced {} bytes where coding the same input as plain \
             literals takes {literals_only}",
            out.len()
        );
        assert_eq!(inflate(&out).unwrap(), data);
    }

    /// A stored payload above 65535 bytes has to become several blocks.
    ///
    /// `deflate_stored` used to write `data.len() as u16`, which above 65535
    /// declared a truncated LEN and then emitted the whole payload behind it —
    /// a corrupt stream. It was unreachable while the only caller was the
    /// 64-byte fast path. Adding the stored candidate to `deflate_with_chain`
    /// made it reachable at any size, so this pins both halves: that the
    /// multi-block stream decodes, and that `stored_size` — which is what
    /// decides whether stored wins, without encoding — predicts the exact size.
    #[test]
    fn stored_blocks_split_above_the_sixteen_bit_length() {
        for len in [0usize, 1, 65535, 65536, 131_070, 200_000] {
            let data = random_alphabet(len, 256);
            let mut writer = BitWriter::new();
            deflate_stored(&mut writer, &data, true);
            let out = writer.into_bytes();
            assert_eq!(
                out.len(),
                stored_size(&data),
                "stored_size mispredicted the {len}-byte encoding"
            );
            assert_eq!(inflate(&out).unwrap(), data, "{len}-byte stored round trip");
        }
    }

    #[test]
    fn levels_are_distinguishable_and_monotonic_in_ratio() {
        let data = level_test_corpus();
        let sizes: Vec<usize> = (1..=9u8).map(|l| deflate_level(&data, l).len()).collect();

        assert!(
            sizes[8] < sizes[0],
            "level 9 must beat level 1: got {} vs {}",
            sizes[8],
            sizes[0]
        );

        // Not strictly monotonic — a deeper search can occasionally pick a
        // match that encodes one bit worse — so the assertion is on the trend,
        // which is what the level knob actually promises.
        for (i, w) in sizes.windows(2).enumerate() {
            assert!(
                w[1] <= w[0].saturating_add(w[0] / 100).saturating_add(8),
                "level {} was materially worse than level {}: {} vs {}",
                i + 2,
                i + 1,
                w[1],
                w[0]
            );
        }
    }

    /// `deflate()` must keep producing exactly what it produced before the
    /// level parameter existed, because `kernel::fs::compress`, `ziparchive`,
    /// `gzip` and `zlib_deflate` were all tuned against it.  Pinned by
    /// equivalence to its documented level rather than to a stored blob, so
    /// the test explains *why* the bytes are what they are.
    #[test]
    fn default_deflate_is_level_three() {
        assert_eq!(level_max_chain(3), DEFAULT_MAX_CHAIN);
        let data = level_test_corpus();
        assert_eq!(
            deflate(&data),
            deflate_level(&data, 3),
            "deflate() must stay byte-identical to level 3"
        );
    }

    /// Levels out of range clamp to the ends rather than erroring — the
    /// signature is total on purpose (see `level_max_chain`).
    #[test]
    fn out_of_range_levels_clamp() {
        assert_eq!(level_max_chain(0), level_max_chain(1));
        assert_eq!(level_max_chain(10), level_max_chain(9));
        assert_eq!(level_max_chain(255), level_max_chain(9));

        let data = level_test_corpus();
        assert_eq!(deflate_level(&data, 200), deflate_level(&data, 9));
    }

    /// Below the stored-block threshold the level cannot matter, and must not
    /// be silently *pretended* to matter either: all levels agree exactly.
    #[test]
    fn tiny_input_is_level_independent() {
        let tiny = b"short".to_vec();
        let base = deflate_level(&tiny, 1);
        for level in 1..=9u8 {
            assert_eq!(deflate_level(&tiny, level), base);
        }
        assert_eq!(inflate(&base).expect("tiny must inflate"), tiny);
    }
}
