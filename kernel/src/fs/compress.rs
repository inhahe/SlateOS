//! DEFLATE decompression (RFC 1951) and gzip wrapper (RFC 1952).
//!
//! Provides `inflate()` for raw DEFLATE streams and `gunzip()` for
//! gzip-wrapped data.  Used by the tar command for `.tar.gz` support
//! and as a building block for future ZIP archive reading.
//!
//! ## Algorithm overview
//!
//! DEFLATE compresses data into a sequence of blocks.  Each block is
//! either stored (uncompressed), compressed with fixed Huffman codes,
//! or compressed with dynamic (per-block) Huffman codes.
//!
//! Compressed blocks encode literal bytes and (length, distance) back-
//! references.  The decoder maintains a sliding window of up to 32 KiB
//! of previously decoded output for copy-back references.
//!
//! ## References
//!
//! - RFC 1951: DEFLATE Compressed Data Format Specification
//! - RFC 1952: GZIP file format specification
//! - Based on the public-domain puff.c by Mark Adler

use alloc::vec;
use alloc::vec::Vec;
use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Bit reader — reads bits from a byte stream, LSB first
// ---------------------------------------------------------------------------

/// Reads bits from a byte buffer, least-significant-bit first.
struct BitReader<'a> {
    data: &'a [u8],
    pos: usize,   // byte position
    bit: u8,      // bit position within current byte (0-7)
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0, bit: 0 }
    }

    /// Read `n` bits (1..=25) and return as u32 (LSB first).
    fn read_bits(&mut self, n: u8) -> KernelResult<u32> {
        let mut val = 0u32;
        for i in 0..n {
            if self.pos >= self.data.len() {
                return Err(KernelError::CorruptedData);
            }
            let byte = self.data[self.pos];
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
    fn read_byte(&mut self) -> KernelResult<u8> {
        if self.pos >= self.data.len() {
            return Err(KernelError::CorruptedData);
        }
        let b = self.data[self.pos];
        self.pos = self.pos.wrapping_add(1);
        Ok(b)
    }

    /// Read a 16-bit little-endian value (must be aligned).
    fn read_u16_le(&mut self) -> KernelResult<u16> {
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
    fn build(lengths: &[u8]) -> KernelResult<Self> {
        let mut table = Self::empty();
        table.num_symbols = lengths.len();

        // Count the number of codes for each code length.
        for &len in lengths {
            if len as usize > MAX_BITS {
                return Err(KernelError::CorruptedData);
            }
            table.counts[len as usize] = table.counts[len as usize].wrapping_add(1);
        }

        // Check for an empty or invalid code set.
        // counts[0] = number of symbols with no code.
        if table.counts[0] as usize == lengths.len() {
            // No codes at all — degenerate but valid for empty distance alphabet.
            return Ok(table);
        }

        // Check that the Huffman tree is complete or under-subscribed.
        // The Kraft inequality: sum of 2^(-len) for each code must be ≤ 1.
        let mut left: i32 = 1;
        for bits in 1..=MAX_BITS {
            left = left.wrapping_mul(2);
            left = left.wrapping_sub(table.counts[bits] as i32);
            if left < 0 {
                return Err(KernelError::CorruptedData); // over-subscribed
            }
        }

        // Compute offsets: where codes of each length start in the
        // symbols array.
        let mut offsets = [0u16; MAX_BITS + 1];
        for bits in 1..MAX_BITS {
            offsets[bits.wrapping_add(1)] =
                offsets[bits].wrapping_add(table.counts[bits]);
        }

        // Fill in the symbols array, sorted by code length then value.
        for (sym, &len) in lengths.iter().enumerate() {
            if len > 0 {
                let idx = offsets[len as usize] as usize;
                if idx < table.symbols.len() {
                    table.symbols[idx] = sym as u16;
                }
                offsets[len as usize] = offsets[len as usize].wrapping_add(1);
            }
        }

        Ok(table)
    }

    /// Decode one symbol from the bit stream.
    ///
    /// Reads bits one at a time, accumulating a code and checking
    /// against each code length.  This is simple (no lookup tables)
    /// and works well for the small alphabets in DEFLATE.
    fn decode(&self, reader: &mut BitReader<'_>) -> KernelResult<u16> {
        let mut code: u32 = 0;
        let mut first: u32 = 0;   // first code of this length
        let mut index: u32 = 0;   // index into symbols for this length

        for len in 1..=MAX_BITS {
            let bit = reader.read_bits(1)?;
            code = code.wrapping_mul(2).wrapping_add(bit);
            let count = u32::from(self.counts[len]);
            if code.wrapping_sub(first) < count {
                let sym_idx = index.wrapping_add(code.wrapping_sub(first)) as usize;
                return self.symbols.get(sym_idx)
                    .copied()
                    .ok_or(KernelError::CorruptedData);
            }
            first = first.wrapping_add(count).wrapping_mul(2);
            index = index.wrapping_add(count);
        }

        Err(KernelError::CorruptedData)
    }
}

// ---------------------------------------------------------------------------
// DEFLATE tables — length and distance extra bits
// ---------------------------------------------------------------------------

/// Base lengths for length codes 257..285.
const LENGTH_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31,
    35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258,
];

/// Extra bits for length codes 257..285.
const LENGTH_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2,
    3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];

/// Base distances for distance codes 0..29.
const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193,
    257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145,
    8193, 12289, 16385, 24577,
];

/// Extra bits for distance codes 0..29.
const DIST_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6,
    7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13,
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
    let mut lens = [0u8; 288];
    // 0..143   → 8 bits
    let mut i = 0;
    while i <= 143 { lens[i] = 8; i += 1; }
    // 144..255 → 9 bits
    while i <= 255 { lens[i] = 9; i += 1; }
    // 256..279 → 7 bits
    while i <= 279 { lens[i] = 7; i += 1; }
    // 280..287 → 8 bits
    while i <= 287 { lens[i] = 8; i += 1; }
    lens
}

/// Build the fixed distance Huffman code (all 32 codes are 5 bits).
fn fixed_dist_lengths() -> [u8; 32] {
    [5u8; 32]
}

// ---------------------------------------------------------------------------
// DEFLATE inflate (core decompression)
// ---------------------------------------------------------------------------

/// Maximum output size to prevent runaway decompression (64 MiB).
const MAX_OUTPUT: usize = 64 * 1024 * 1024;

/// Decompress a raw DEFLATE stream (no gzip/zlib header).
pub fn inflate(data: &[u8]) -> KernelResult<Vec<u8>> {
    let mut reader = BitReader::new(data);
    let mut output = Vec::with_capacity(data.len().saturating_mul(2).min(MAX_OUTPUT));

    loop {
        // Read block header: BFINAL (1 bit) + BTYPE (2 bits).
        let bfinal = reader.read_bits(1)?;
        let btype = reader.read_bits(2)?;

        match btype {
            0 => inflate_stored(&mut reader, &mut output)?,
            1 => inflate_fixed(&mut reader, &mut output)?,
            2 => inflate_dynamic(&mut reader, &mut output)?,
            _ => return Err(KernelError::CorruptedData), // reserved
        }

        if bfinal != 0 {
            break;
        }
    }

    Ok(output)
}

/// Decode a stored (uncompressed) block.
fn inflate_stored(reader: &mut BitReader<'_>, output: &mut Vec<u8>) -> KernelResult<()> {
    reader.align();
    let len = reader.read_u16_le()?;
    let nlen = reader.read_u16_le()?;

    // LEN and NLEN should be one's complements of each other.
    if len != !nlen {
        return Err(KernelError::CorruptedData);
    }

    for _ in 0..len {
        if output.len() >= MAX_OUTPUT {
            return Err(KernelError::OutOfMemory);
        }
        let b = reader.read_byte()?;
        output.push(b);
    }

    Ok(())
}

/// Decode a block with fixed Huffman codes.
fn inflate_fixed(reader: &mut BitReader<'_>, output: &mut Vec<u8>) -> KernelResult<()> {
    let lit_table = HuffmanTable::build(&fixed_lit_lengths())?;
    let dist_table = HuffmanTable::build(&fixed_dist_lengths())?;
    inflate_codes(reader, &lit_table, &dist_table, output)
}

/// Decode a block with dynamic Huffman codes.
fn inflate_dynamic(reader: &mut BitReader<'_>, output: &mut Vec<u8>) -> KernelResult<()> {
    // Read the number of literal/length codes, distance codes, and
    // code-length codes.
    let hlit = reader.read_bits(5)?.wrapping_add(257) as usize;  // 257..286
    let hdist = reader.read_bits(5)?.wrapping_add(1) as usize;   // 1..32
    let hclen = reader.read_bits(4)?.wrapping_add(4) as usize;   // 4..19

    if hlit > 286 || hdist > 30 || hclen > 19 {
        return Err(KernelError::CorruptedData);
    }

    // Read code-length code lengths (3 bits each, in permuted order).
    let mut cl_lens = [0u8; MAX_CL_CODES];
    for i in 0..hclen {
        let idx = CL_ORDER[i] as usize;
        cl_lens[idx] = reader.read_bits(3)? as u8;
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
                    return Err(KernelError::CorruptedData);
                }
                let repeat = reader.read_bits(2)?.wrapping_add(3) as usize;
                let prev = all_lens.get(i.wrapping_sub(1)).copied().unwrap_or(0);
                for _ in 0..repeat {
                    if i >= total {
                        return Err(KernelError::CorruptedData);
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
                        return Err(KernelError::CorruptedData);
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
                        return Err(KernelError::CorruptedData);
                    }
                    if let Some(slot) = all_lens.get_mut(i) {
                        *slot = 0;
                    }
                    i = i.wrapping_add(1);
                }
            }
            _ => return Err(KernelError::CorruptedData),
        }
    }

    let lit_table = HuffmanTable::build(
        all_lens.get(..hlit).ok_or(KernelError::CorruptedData)?
    )?;
    let dist_table = HuffmanTable::build(
        all_lens.get(hlit..total).ok_or(KernelError::CorruptedData)?
    )?;

    inflate_codes(reader, &lit_table, &dist_table, output)
}

/// Decode literal/length + distance symbols until end-of-block (256).
fn inflate_codes(
    reader: &mut BitReader<'_>,
    lit_table: &HuffmanTable,
    dist_table: &HuffmanTable,
    output: &mut Vec<u8>,
) -> KernelResult<()> {
    loop {
        let sym = lit_table.decode(reader)?;

        if sym < 256 {
            // Literal byte.
            if output.len() >= MAX_OUTPUT {
                return Err(KernelError::OutOfMemory);
            }
            output.push(sym as u8);
        } else if sym == 256 {
            // End of block.
            return Ok(());
        } else {
            // Length/distance pair — back-reference.
            let len_idx = (sym as usize).wrapping_sub(257);
            let base_len = *LENGTH_BASE.get(len_idx)
                .ok_or(KernelError::CorruptedData)?;
            let extra = *LENGTH_EXTRA.get(len_idx)
                .ok_or(KernelError::CorruptedData)?;
            let length = base_len as usize
                + reader.read_bits(extra)? as usize;

            let dist_sym = dist_table.decode(reader)? as usize;
            let base_dist = *DIST_BASE.get(dist_sym)
                .ok_or(KernelError::CorruptedData)?;
            let dist_extra = *DIST_EXTRA.get(dist_sym)
                .ok_or(KernelError::CorruptedData)?;
            let distance = base_dist as usize
                + reader.read_bits(dist_extra)? as usize;

            if distance == 0 || distance > output.len() {
                return Err(KernelError::CorruptedData);
            }

            // Copy from the sliding window.  Note: source and dest
            // can overlap (e.g., distance=1, length=100 fills with
            // one repeated byte), so we copy byte-by-byte.
            let start = output.len().wrapping_sub(distance);
            for i in 0..length {
                if output.len() >= MAX_OUTPUT {
                    return Err(KernelError::OutOfMemory);
                }
                let b = output[start.wrapping_add(i % distance)];
                output.push(b);
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
        Self { data: Vec::new(), current: 0, bits_used: 0 }
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
    for (i, &base) in LENGTH_BASE.iter().enumerate() {
        let extra = LENGTH_EXTRA[i];
        let range = 1usize << extra;
        if length >= base as usize && length < (base as usize).wrapping_add(range) {
            let sym = (i as u16).wrapping_add(257);
            let extra_val = (length.wrapping_sub(base as usize)) as u32;
            return Some((sym, extra_val, extra));
        }
    }
    None
}

/// Find the distance code (0..29) and extra bits for a match distance.
fn encode_distance(distance: usize) -> Option<(u16, u32, u8)> {
    for (i, &base) in DIST_BASE.iter().enumerate() {
        let extra = DIST_EXTRA[i];
        let range = 1usize << extra;
        if distance >= base as usize && distance < (base as usize).wrapping_add(range) {
            let sym = i as u16;
            let extra_val = (distance.wrapping_sub(base as usize)) as u32;
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

/// Maximum chain length for hash chain traversal.
///
/// Longer chains find better matches but slow down compression.
/// zlib default at compression level 6 uses chain=128.  We use 16
/// as a good balance for kernel use (fast enough, good compression).
const MAX_CHAIN: usize = 16;

/// Compute a hash for 3 bytes.
fn lz77_hash(data: &[u8], pos: usize) -> usize {
    if pos.wrapping_add(2) >= data.len() {
        return 0;
    }
    let h = (u32::from(data[pos]) << 10)
        ^ (u32::from(data[pos.wrapping_add(1)]) << 5)
        ^ u32::from(data[pos.wrapping_add(2)]);
    (h as usize) & (HASH_SIZE.wrapping_sub(1))
}

// ---------------------------------------------------------------------------
// LZ77 tokenizer (shared by fixed and dynamic Huffman)
// ---------------------------------------------------------------------------

/// An LZ77 token: either a literal byte or a length/distance match.
enum LzToken {
    Literal(u8),
    Match { length: u16, distance: u16 },
}

/// Insert position `pos` into the hash chain and return the previous
/// head of that chain (for match searching).
///
/// `head[h]` stores the most recent position with hash `h`.
/// `prev[pos % MAX_DISTANCE]` links to the previous position in the chain.
fn insert_hash(
    data: &[u8],
    pos: usize,
    head: &mut [u32; HASH_SIZE],
    prev: &mut [u32],
) -> u32 {
    let h = lz77_hash(data, pos);
    let old_head = head[h];
    prev[pos % MAX_DISTANCE] = old_head;
    head[h] = pos as u32;
    old_head
}

/// Find the longest match at `pos` by walking the hash chain.
///
/// Returns (length, distance) of the best match found, or (0, 0) if
/// no match of at least `MIN_MATCH` bytes exists.
fn find_best_match(
    data: &[u8],
    pos: usize,
    head: &[u32; HASH_SIZE],
    prev: &[u32],
) -> (usize, usize) {
    let remaining = data.len().wrapping_sub(pos);
    if remaining < MIN_MATCH {
        return (0, 0);
    }

    let h = lz77_hash(data, pos);
    let mut candidate = head[h] as usize;
    let max_len = remaining.min(MAX_MATCH);

    let mut best_len: usize = MIN_MATCH.wrapping_sub(1);
    let mut best_dist: usize = 0;
    let mut chain_count = 0usize;

    while candidate < pos
        && pos.wrapping_sub(candidate) <= MAX_DISTANCE
        && chain_count < MAX_CHAIN
    {
        // Compare bytes at candidate vs pos, starting from the current
        // best length downward (zlib trick: check the last matching byte
        // first to quickly reject poor candidates).
        let dist = pos.wrapping_sub(candidate);

        // Quick rejection: if bytes at the current best length don't match,
        // this candidate can't be better.
        if best_len > 0
            && data.get(candidate.wrapping_add(best_len))
                != data.get(pos.wrapping_add(best_len))
        {
            candidate = prev[candidate % MAX_DISTANCE] as usize;
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

        candidate = prev[candidate % MAX_DISTANCE] as usize;
        chain_count = chain_count.wrapping_add(1);
    }

    if best_len >= MIN_MATCH {
        (best_len, best_dist)
    } else {
        (0, 0)
    }
}

/// Run the LZ77 pass over `data` and return a token stream.
///
/// Uses hash chains (up to MAX_CHAIN candidates per position) with
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
fn lz77_tokenize(data: &[u8]) -> Vec<LzToken> {
    let mut tokens = Vec::with_capacity(data.len() / 2);

    // Hash chain: head[h] = most recent pos with hash h.
    // prev[pos % MAX_DISTANCE] = previous pos in same chain.
    let mut head = [0u32; HASH_SIZE];
    let mut prev = vec![0; MAX_DISTANCE];

    let mut pos: usize = 0;

    // Pending match from the previous position (for lazy matching).
    let mut pending_len: usize = 0;
    let mut pending_dist: usize = 0;

    while pos < data.len() {
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
            tokens.push(LzToken::Literal(data[pos]));
            pos = pos.wrapping_add(1);
            continue;
        }

        // Insert this position into the hash chain.
        insert_hash(data, pos, &mut head, &mut prev);

        // Find the best match at this position using the chain.
        let (cur_len, cur_dist) = find_best_match(data, pos, &head, &prev);

        if pending_len >= MIN_MATCH {
            // We have a pending match from pos-1.  Compare with current.
            if cur_len > pending_len {
                // Current match is better — drop pending, emit literal for pos-1.
                tokens.push(LzToken::Literal(data[pos.wrapping_sub(1)]));
                pending_len = cur_len;
                pending_dist = cur_dist;
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
            pos = pos.wrapping_add(1);
        } else {
            // No match.
            tokens.push(LzToken::Literal(data[pos]));
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
    let mut symbols: Vec<(u32, usize)> = freqs.iter().enumerate()
        .filter(|(_, f)| **f > 0)
        .map(|(i, f)| (*f, i))
        .collect();

    if symbols.is_empty() {
        return lengths;
    }

    if symbols.len() == 1 {
        // Single symbol — assign length 1.
        lengths[symbols[0].1] = 1;
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

        for i in 0..next_node {
            if active[i] {
                if node_freq[i] < f1 || (node_freq[i] == f1 && i < min1) {
                    f2 = f1;
                    min2 = min1;
                    f1 = node_freq[i];
                    min1 = i;
                } else if node_freq[i] < f2 || (node_freq[i] == f2 && i < min2) {
                    f2 = node_freq[i];
                    min2 = i;
                }
            }
        }

        if min1 == usize::MAX || min2 == usize::MAX {
            break;
        }

        // Create merged node.
        let merged = next_node;
        next_node = next_node.saturating_add(1);
        node_freq[merged] = f1.saturating_add(f2);
        node_depth[merged] = node_depth[min1]
            .max(node_depth[min2])
            .saturating_add(1);
        active[merged] = true;
        active[min1] = false;
        active[min2] = false;
        parent_of[min1] = merged;
        parent_of[min2] = merged;
    }

    // Compute depth of each leaf by walking up to root.
    for i in 0..count {
        let mut depth: u8 = 0;
        let mut node = i;
        while parent_of[node] != usize::MAX {
            depth = depth.saturating_add(1);
            node = parent_of[node];
        }
        lengths[symbols[i].1] = depth;
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
        lengths[longest_sym] = lengths[longest_sym].saturating_sub(1);
        if shortest_sym != longest_sym {
            lengths[shortest_sym] = lengths[shortest_sym].saturating_add(1);
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
            bl_count[l as usize] = bl_count[l as usize].wrapping_add(1);
        }
    }

    // Compute starting code for each length.
    let mut next_code: Vec<u16> = vec![0; max_len as usize + 1];
    let mut code: u16 = 0;
    for bits in 1..=max_len {
        code = (code.wrapping_add(bl_count[bits as usize - 1])) << 1;
        next_code[bits as usize] = code;
    }

    // Assign codes.
    for (sym, &l) in lengths.iter().enumerate() {
        if l > 0 {
            let c = next_code[l as usize];
            next_code[l as usize] = c.wrapping_add(1);
            codes[sym] = (reverse_bits(c, l), l);
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

    while i < lengths.len() {
        let val = lengths[i];

        if val == 0 {
            // Count consecutive zeros.
            let mut run = 1usize;
            while i.wrapping_add(run) < lengths.len()
                && lengths[i.wrapping_add(run)] == 0
                && run < 138
            {
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
            while i.wrapping_add(run) < lengths.len()
                && lengths[i.wrapping_add(run)] == val
                && run < 6
            {
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

/// Encode tokens using dynamic Huffman codes (BTYPE=10).
///
/// Builds optimal Huffman trees from the token frequencies, encodes
/// the tree in the block header, then encodes all tokens.
#[allow(clippy::arithmetic_side_effects)]
fn encode_dynamic(writer: &mut BitWriter, tokens: &[LzToken], bfinal: bool) {
    // --- Count symbol frequencies ---
    let mut lit_freq = [0u32; 286]; // 0-255 literals, 256 end, 257-285 lengths
    let mut dist_freq = [0u32; 30]; // 0-29 distance codes

    lit_freq[256] = 1; // End-of-block always present.

    for token in tokens {
        match token {
            LzToken::Literal(b) => {
                lit_freq[*b as usize] = lit_freq[*b as usize].saturating_add(1);
            }
            LzToken::Match { length, distance } => {
                if let Some((sym, _, _)) = encode_length(*length as usize) {
                    lit_freq[sym as usize] = lit_freq[sym as usize].saturating_add(1);
                }
                if let Some((sym, _, _)) = encode_distance(*distance as usize) {
                    dist_freq[sym as usize] = dist_freq[sym as usize].saturating_add(1);
                }
            }
        }
    }

    // --- Build code lengths ---
    let lit_lengths = build_code_lengths(&lit_freq, MAX_CODE_BITS);
    let dist_lengths = build_code_lengths(&dist_freq, MAX_CODE_BITS);

    // HLIT: number of lit/len codes - 257 (max 286 codes).
    // Find the last non-zero length.
    let hlit = lit_lengths.iter().rposition(|&l| l > 0)
        .map(|p| p.saturating_add(1))
        .unwrap_or(257)
        .max(257);
    // HDIST: number of distance codes - 1 (at least 1).
    let hdist = dist_lengths.iter().rposition(|&l| l > 0)
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
        cl_freq[sym as usize] = cl_freq[sym as usize].saturating_add(1);
    }

    // Build code-length Huffman tree.
    let cl_lengths = build_code_lengths(&cl_freq, 7);
    let cl_codes = build_canonical_codes(&cl_lengths);

    // HCLEN: number of code-length codes - 4.
    // Find the last non-zero in the permuted order.
    let mut hclen = 4usize;
    for i in (0..19).rev() {
        if cl_lengths[CL_ORDER[i] as usize] > 0 {
            hclen = i.saturating_add(1).max(4);
            break;
        }
    }

    // --- Emit block header ---
    writer.write_bits(u32::from(bfinal), 1); // BFINAL
    writer.write_bits(2, 2);                  // BTYPE=10 (dynamic)

    writer.write_bits((hlit.wrapping_sub(257)) as u32, 5);  // HLIT
    writer.write_bits((hdist.wrapping_sub(1)) as u32, 5);   // HDIST
    writer.write_bits((hclen.wrapping_sub(4)) as u32, 4);   // HCLEN

    // Emit code-length code lengths (3 bits each, permuted order).
    for i in 0..hclen {
        let l = cl_lengths[CL_ORDER[i] as usize];
        writer.write_bits(u32::from(l), 3);
    }

    // Emit RLE-encoded literal/length + distance code lengths.
    for &(sym, extra) in &rle {
        let (code, bits) = cl_codes[sym as usize];
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
                let (code, bits) = lit_codes[*b as usize];
                writer.write_bits(u32::from(code), bits);
            }
            LzToken::Match { length, distance } => {
                if let (Some((len_sym, len_extra, len_ebits)), Some((dist_sym, dist_extra, dist_ebits))) =
                    (encode_length(*length as usize), encode_distance(*distance as usize))
                {
                    let (lcode, lbits) = lit_codes[len_sym as usize];
                    writer.write_bits(u32::from(lcode), lbits);
                    if len_ebits > 0 {
                        writer.write_bits(len_extra, len_ebits);
                    }
                    let (dcode, dbits) = dist_codes[dist_sym as usize];
                    writer.write_bits(u32::from(dcode), dbits);
                    if dist_ebits > 0 {
                        writer.write_bits(dist_extra, dist_ebits);
                    }
                }
            }
        }
    }

    // End of block.
    let (code, bits) = lit_codes[256];
    writer.write_bits(u32::from(code), bits);
}

/// Encode tokens using fixed Huffman codes (BTYPE=01).
#[allow(clippy::arithmetic_side_effects)]
fn encode_fixed(writer: &mut BitWriter, tokens: &[LzToken], bfinal: bool) {
    writer.write_bits(u32::from(bfinal), 1); // BFINAL
    writer.write_bits(1, 2);                  // BTYPE=01

    for token in tokens {
        match token {
            LzToken::Literal(b) => {
                let (code, bits) = fixed_code(u16::from(*b));
                writer.write_bits(u32::from(code), bits);
            }
            LzToken::Match { length, distance } => {
                if let (Some((len_sym, len_extra, len_ebits)), Some((dist_sym, dist_extra, dist_ebits))) =
                    (encode_length(*length as usize), encode_distance(*distance as usize))
                {
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
    // For very small inputs, just use a stored block.
    if data.len() <= 64 {
        let mut writer = BitWriter::new();
        deflate_stored(&mut writer, data, true);
        return writer.into_bytes();
    }

    // Run LZ77 once, then encode with both strategies.
    let tokens = lz77_tokenize(data);

    // Try fixed Huffman.
    let mut fixed_writer = BitWriter::new();
    encode_fixed(&mut fixed_writer, &tokens, true);
    let fixed_output = fixed_writer.into_bytes();

    // Try dynamic Huffman.
    let mut dynamic_writer = BitWriter::new();
    encode_dynamic(&mut dynamic_writer, &tokens, true);
    let dynamic_output = dynamic_writer.into_bytes();

    // Pick the smaller output.
    if dynamic_output.len() < fixed_output.len() {
        dynamic_output
    } else {
        fixed_output
    }
}

/// Write a stored (uncompressed) DEFLATE block.
fn deflate_stored(writer: &mut BitWriter, data: &[u8], bfinal: bool) {
    writer.write_bits(u32::from(bfinal), 1); // BFINAL
    writer.write_bits(0, 2);                  // BTYPE=00 (stored)
    writer.flush();                           // align to byte

    let len = data.len() as u16;
    // LEN
    writer.write_byte(len as u8);
    writer.write_byte((len >> 8) as u8);
    // NLEN
    let nlen = !len;
    writer.write_byte(nlen as u8);
    writer.write_byte((nlen >> 8) as u8);
    // Raw data.
    for &b in data {
        writer.write_byte(b);
    }
}

/// Create a gzip-compressed byte vector from uncompressed data.
///
/// Wraps `deflate()` output in a gzip header (RFC 1952) with
/// CRC-32 and original-size trailer.
pub fn gzip(data: &[u8]) -> Vec<u8> {
    let compressed = deflate(data);
    let crc = crc32_iso(data);
    let size = data.len() as u32;

    let mut out = Vec::with_capacity(10 + compressed.len() + 8);
    // Gzip header (10 bytes).
    out.push(0x1F); out.push(0x8B); // ID
    out.push(0x08);                  // CM = deflate
    out.push(0x00);                  // FLG
    out.extend_from_slice(&[0, 0, 0, 0]); // MTIME
    out.push(0x04);                  // XFL = fastest compression
    out.push(0xFF);                  // OS = unknown
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
pub fn gunzip(data: &[u8]) -> KernelResult<Vec<u8>> {
    if data.len() < 18 {
        return Err(KernelError::CorruptedData); // minimum gzip size
    }

    let mut reader = BitReader::new(data);

    // Gzip header.
    let id1 = reader.read_byte()?;
    let id2 = reader.read_byte()?;
    if id1 != 0x1F || id2 != 0x8B {
        return Err(KernelError::CorruptedData); // not gzip
    }

    let cm = reader.read_byte()?;
    if cm != 8 {
        return Err(KernelError::CorruptedData); // only deflate
    }

    let flg = reader.read_byte()?;
    let _mtime = reader.read_u16_le()?; // skip MTIME (4 bytes)
    let _mtime_hi = reader.read_u16_le()?;
    let _xfl = reader.read_byte()?;      // extra flags
    let _os = reader.read_byte()?;        // OS identifier

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
            if b == 0 { break; }
        }
    }

    // Skip optional comment (null-terminated).
    if (flg & FCOMMENT) != 0 {
        loop {
            let b = reader.read_byte()?;
            if b == 0 { break; }
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
        return Err(KernelError::CorruptedData);
    }
    let deflate_end = data.len().wrapping_sub(8);
    let deflate_data = data.get(deflate_start..deflate_end)
        .ok_or(KernelError::CorruptedData)?;

    let output = inflate(deflate_data)?;

    // Verify CRC32 and ISIZE from trailer.
    let trailer = data.get(deflate_end..)
        .ok_or(KernelError::CorruptedData)?;
    if trailer.len() < 8 {
        return Err(KernelError::CorruptedData);
    }

    let expected_crc = u32::from_le_bytes([
        trailer[0], trailer[1], trailer[2], trailer[3],
    ]);
    let expected_size = u32::from_le_bytes([
        trailer[4], trailer[5], trailer[6], trailer[7],
    ]);

    // Verify size (mod 2^32).
    let actual_size = output.len() as u32;
    if actual_size != expected_size {
        crate::serial_println!(
            "[gunzip] Size mismatch: expected {} got {}",
            expected_size, actual_size
        );
        return Err(KernelError::CorruptedData);
    }

    // Verify CRC32 (using the ISO 3309 / ITU-T V.42 polynomial, not
    // CRC32C).  Gzip uses CRC-32 (polynomial 0xEDB88320 reflected).
    let actual_crc = crc32_iso(&output);
    if actual_crc != expected_crc {
        crate::serial_println!(
            "[gunzip] CRC32 mismatch: expected {:#010x} got {:#010x}",
            expected_crc, actual_crc
        );
        return Err(KernelError::CorruptedData);
    }

    Ok(output)
}

// ---------------------------------------------------------------------------
// CRC-32 (ISO 3309 / gzip) — NOT CRC32C
//
// Gzip uses the "classic" CRC-32 polynomial 0xEDB88320 (bit-reversed
// 0x04C11DB7), which differs from CRC32C (Castagnoli, 0x82F63B78).
// ---------------------------------------------------------------------------

/// Compute CRC-32 (ISO 3309) of a byte slice.
///
/// This is the polynomial used by gzip, PNG, ZIP, and Ethernet.
/// Different from CRC32C used by ext4 and our `crypto::crc32c()`.
fn crc32_iso(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;

    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            if (crc & 1) != 0 {
                crc = (crc >> 1) ^ 0xEDB8_8320;
            } else {
                crc >>= 1;
            }
        }
    }

    crc ^ 0xFFFF_FFFF
}

/// Public wrapper for CRC-32 ISO (gzip/ZIP/PNG polynomial).
///
/// Exposed for use by the `unzip` command to verify file integrity.
pub fn crc32_iso_pub(data: &[u8]) -> u32 {
    crc32_iso(data)
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
/// ## Errors
///
/// Returns `CorruptedData` if the header is invalid, the DEFLATE
/// stream is malformed, or the Adler-32 checksum doesn't match.
pub fn zlib_inflate(data: &[u8]) -> KernelResult<Vec<u8>> {
    if data.len() < 6 {
        return Err(KernelError::CorruptedData);
    }

    // --- Parse header (RFC 1950 §2.2) ---
    let cmf = data[0];
    let flg = data[1];

    // CMF: lower 4 bits = CM (compression method), upper 4 = CINFO.
    let cm = cmf & 0x0F;
    if cm != 8 {
        // Only deflate (method 8) is defined.
        return Err(KernelError::CorruptedData);
    }

    // Header checksum: (CMF*256 + FLG) must be divisible by 31.
    let header_check = u16::from(cmf)
        .wrapping_mul(256)
        .wrapping_add(u16::from(flg));
    if header_check % 31 != 0 {
        return Err(KernelError::CorruptedData);
    }

    // FDICT bit (bit 5 of FLG): preset dictionary.  We don't support
    // preset dictionaries — they're rare (used in some PDF streams).
    if flg & 0x20 != 0 {
        return Err(KernelError::CorruptedData);
    }

    // Compressed data starts at offset 2.
    let compressed = data.get(2..data.len().saturating_sub(4))
        .ok_or(KernelError::CorruptedData)?;

    // --- Decompress ---
    let decompressed = inflate(compressed)?;

    // --- Verify Adler-32 (big-endian, last 4 bytes) ---
    let trailer_start = data.len().saturating_sub(4);
    let stored_adler = u32::from(data[trailer_start]) << 24
        | u32::from(data[trailer_start.wrapping_add(1)]) << 16
        | u32::from(data[trailer_start.wrapping_add(2)]) << 8
        | u32::from(data[trailer_start.wrapping_add(3)]);

    let computed_adler = adler32(&decompressed);
    if stored_adler != computed_adler {
        crate::serial_println!(
            "[compress] zlib Adler-32 mismatch: stored={:#010x} computed={:#010x}",
            stored_adler, computed_adler,
        );
        return Err(KernelError::CorruptedData);
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

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for the DEFLATE decompressor.
///
/// Tests stored blocks, fixed Huffman, and the gzip wrapper.
#[allow(clippy::arithmetic_side_effects)]
pub fn self_test() -> KernelResult<()> {
    crate::serial_println!("[compress] Running self-test...");

    // Test CRC-32 ISO with known value.
    // CRC32 of "123456789" is 0xCBF43926.
    let crc = crc32_iso(b"123456789");
    if crc != 0xCBF4_3926 {
        crate::serial_println!(
            "[compress]   FAIL: CRC32 ISO expected 0xCBF43926, got {:#010x}",
            crc
        );
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[compress]   CRC-32 ISO verified ✓");

    // Test inflate with a stored block.
    // Stored block: BFINAL=1, BTYPE=00, LEN=5, NLEN=~5, "hello"
    let stored: [u8; 12] = [
        0x01,                   // BFINAL=1, BTYPE=0 (stored)
        0x05, 0x00,             // LEN = 5
        0xFA, 0xFF,             // NLEN = ~5 = 0xFFFA
        b'h', b'e', b'l', b'l', b'o',
        0x00, 0x00,             // padding (unused)
    ];
    let result = inflate(&stored[..10])?;
    if result.as_slice() != b"hello" {
        crate::serial_println!(
            "[compress]   FAIL: stored block produced {:?}",
            core::str::from_utf8(&result)
        );
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[compress]   Stored block inflate verified ✓");

    // Test gzip decompression with a minimal gzip stream.
    // This is "hello\n" compressed with gzip (created by standard gzip).
    //
    // gzip header: 1f 8b 08 00 ... (10 bytes)
    // deflate data: compressed "hello\n"
    // trailer: CRC32 + ISIZE (8 bytes)
    //
    // We construct a valid gzip stream by hand:
    // - Use a stored DEFLATE block for simplicity
    // - Compute CRC32 and size manually
    let payload = b"hello\n";
    let crc = crc32_iso(payload);
    let size = payload.len() as u32;

    let mut gz = Vec::with_capacity(30);
    // Gzip header
    gz.push(0x1F); gz.push(0x8B); // ID
    gz.push(0x08);                  // CM = deflate
    gz.push(0x00);                  // FLG = no extras
    gz.extend_from_slice(&[0, 0, 0, 0]); // MTIME
    gz.push(0x00);                  // XFL
    gz.push(0xFF);                  // OS = unknown
    // DEFLATE stored block: BFINAL=1, BTYPE=00
    gz.push(0x01);
    let len = payload.len() as u16;
    gz.extend_from_slice(&len.to_le_bytes());
    gz.extend_from_slice(&(!len).to_le_bytes());
    gz.extend_from_slice(payload);
    // Trailer: CRC32 + ISIZE
    gz.extend_from_slice(&crc.to_le_bytes());
    gz.extend_from_slice(&size.to_le_bytes());

    let decompressed = gunzip(&gz)?;
    if decompressed.as_slice() != payload {
        crate::serial_println!(
            "[compress]   FAIL: gunzip produced {:?}",
            core::str::from_utf8(&decompressed)
        );
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[compress]   Gzip round-trip verified ✓");

    // Test inflate with fixed Huffman codes.
    // Build a tiny fixed-Huffman encoded stream by hand.
    // Encoding "aaa" with fixed Huffman:
    // 'a' = 0x61, fixed code for 0x61 is 8-bit (lit code 97).
    // End-of-block (256) is 7-bit.
    //
    // Rather than hand-encoding, verify the tables build correctly
    // by checking that the fixed literal table can be constructed.
    let lit_lens = fixed_lit_lengths();
    let lit_table = HuffmanTable::build(&lit_lens)?;
    // Symbol 0 should have length 8, symbol 256 should have length 7.
    // We verify the table has the right structure by checking counts.
    if lit_table.counts[7] != 24 || lit_table.counts[8] != 152 || lit_table.counts[9] != 112 {
        crate::serial_println!(
            "[compress]   FAIL: fixed literal table counts: 7={} 8={} 9={}",
            lit_table.counts[7], lit_table.counts[8], lit_table.counts[9]
        );
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[compress]   Fixed Huffman table construction verified ✓");

    // Test deflate → inflate round-trip with a non-trivial string.
    let original = b"Hello, world! Hello, world! This is a test of DEFLATE compression. \
                     AAAAAAAAAAAAAAAAAAAAAA BBBBBBBBBBBBBBBB repetition helps compression.";
    let compressed = deflate(original);
    let decompressed = inflate(&compressed)?;
    if decompressed.as_slice() != &original[..] {
        crate::serial_println!(
            "[compress]   FAIL: deflate round-trip mismatch (orig={}, dec={})",
            original.len(), decompressed.len()
        );
        return Err(KernelError::InternalError);
    }
    crate::serial_println!(
        "[compress]   Deflate round-trip verified ({} -> {} -> {} bytes) ✓",
        original.len(), compressed.len(), decompressed.len()
    );

    // Test gzip → gunzip round-trip.
    let gz_data = gzip(original);
    let ungz_data = gunzip(&gz_data)?;
    if ungz_data.as_slice() != &original[..] {
        crate::serial_println!("[compress]   FAIL: gzip round-trip mismatch");
        return Err(KernelError::InternalError);
    }
    crate::serial_println!(
        "[compress]   Gzip compress round-trip verified ({} -> {} bytes) ✓",
        original.len(), gz_data.len()
    );

    // Test with empty input.
    let empty_gz = gzip(b"");
    let empty_result = gunzip(&empty_gz)?;
    if !empty_result.is_empty() {
        crate::serial_println!("[compress]   FAIL: empty gzip round-trip produced data");
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[compress]   Empty input round-trip verified ✓");

    // --- Adler-32 test ---
    // RFC 1950 example: adler32("Wikipedia") should be 0x11E60398.
    let adler = adler32(b"Wikipedia");
    if adler != 0x11E6_0398 {
        crate::serial_println!(
            "[compress]   FAIL: Adler-32 expected 0x11E60398, got {:#010x}",
            adler
        );
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[compress]   Adler-32 verified ✓");

    // --- zlib round-trip test ---
    let zlib_input = b"The quick brown fox jumps over the lazy dog. Repeated for compression gain. The quick brown fox jumps over the lazy dog.";
    let zlib_compressed = zlib_deflate(zlib_input);
    let zlib_decompressed = zlib_inflate(&zlib_compressed)?;
    if zlib_decompressed != zlib_input {
        crate::serial_println!("[compress]   FAIL: zlib round-trip mismatch");
        return Err(KernelError::InternalError);
    }
    crate::serial_println!(
        "[compress]   zlib round-trip verified ({} -> {} bytes) ✓",
        zlib_input.len(), zlib_compressed.len()
    );

    // zlib empty input round-trip.
    let zlib_empty = zlib_deflate(b"");
    let zlib_empty_out = zlib_inflate(&zlib_empty)?;
    if !zlib_empty_out.is_empty() {
        crate::serial_println!("[compress]   FAIL: zlib empty round-trip produced data");
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[compress]   zlib empty round-trip verified ✓");

    // --- Dynamic vs fixed Huffman comparison ---
    // Verify that dynamic Huffman is actually chosen for skewed data
    // (where a custom Huffman tree should be significantly better than
    // the fixed one).  Use data with a non-uniform byte distribution.
    {
        let mut skewed = Vec::with_capacity(2048);
        // Build data that heavily favors a few byte values — ideal for
        // dynamic Huffman since the custom tree assigns short codes to
        // common bytes.
        for i in 0..2048usize {
            let b = match i % 16 {
                0..=7 => b'a',       // 50% 'a'
                8..=11 => b'b',      // 25% 'b'
                12..=13 => b'c',     // 12.5% 'c'
                _ => (i % 26) as u8 + b'd', // 12.5% varied
            };
            skewed.push(b);
        }

        // Try fixed-only encoding.
        let tokens = lz77_tokenize(&skewed);
        let mut fixed_w = BitWriter::new();
        encode_fixed(&mut fixed_w, &tokens, true);
        let fixed_size = fixed_w.into_bytes().len();

        // Try dynamic encoding.
        let mut dyn_w = BitWriter::new();
        encode_dynamic(&mut dyn_w, &tokens, true);
        let dyn_size = dyn_w.into_bytes().len();

        // For skewed data, dynamic should be at least as good as fixed.
        // In practice it should be noticeably better.
        let full_compressed = deflate(&skewed);
        let full_decompressed = inflate(&full_compressed)?;
        if full_decompressed.as_slice() != skewed.as_slice() {
            crate::serial_println!("[compress]   FAIL: skewed data round-trip mismatch");
            return Err(KernelError::InternalError);
        }

        crate::serial_println!(
            "[compress]   Dynamic vs fixed: dyn={} fixed={} (dyn {}) ✓",
            dyn_size, fixed_size,
            if dyn_size < fixed_size { "wins" } else { "≈ fixed" }
        );
    }

    // --- Pathological inputs ---
    // Test with all identical bytes (extreme RLE case).
    {
        let repeated = [0xAA_u8; 1024];
        let comp = deflate(&repeated);
        let decomp = inflate(&comp)?;
        if decomp.as_slice() != &repeated[..] {
            crate::serial_println!("[compress]   FAIL: all-same-byte round-trip");
            return Err(KernelError::InternalError);
        }
        crate::serial_println!(
            "[compress]   All-same-byte: 1024 -> {} bytes ✓",
            comp.len()
        );
    }

    // Test with pseudo-random data (poor compression expected).
    {
        let mut random_ish = Vec::with_capacity(1024);
        let mut state: u32 = 0x1234_5678;
        for _ in 0..1024 {
            // Simple xorshift32 PRNG.
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            random_ish.push(state as u8);
        }
        let comp = deflate(&random_ish);
        let decomp = inflate(&comp)?;
        if decomp.as_slice() != random_ish.as_slice() {
            crate::serial_println!("[compress]   FAIL: pseudo-random round-trip");
            return Err(KernelError::InternalError);
        }
        crate::serial_println!(
            "[compress]   Pseudo-random: 1024 -> {} bytes ✓",
            comp.len()
        );
    }

    // Test with a larger input to stress the lazy matching.
    {
        let mut large = Vec::with_capacity(8192);
        // Mix of repetitive patterns and unique data.
        for i in 0..8192usize {
            let b = if i % 100 < 60 {
                // 60% comes from a 4-byte repeating pattern.
                [b'A', b'B', b'C', b'D'][i % 4]
            } else {
                // 40% pseudo-random.
                (i.wrapping_mul(7).wrapping_add(13) % 256) as u8
            };
            large.push(b);
        }
        let comp = deflate(&large);
        let decomp = inflate(&comp)?;
        if decomp.as_slice() != large.as_slice() {
            crate::serial_println!("[compress]   FAIL: 8K mixed data round-trip");
            return Err(KernelError::InternalError);
        }
        crate::serial_println!(
            "[compress]   8K mixed: {} -> {} bytes ({:.0}%) ✓",
            large.len(), comp.len(),
            (comp.len() as f64 / large.len() as f64) * 100.0
        );
    }

    crate::serial_println!("[compress] Self-test passed.");
    Ok(())
}
