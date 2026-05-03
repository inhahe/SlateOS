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
// Gzip wrapper (RFC 1952)
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
    let actual_size = (output.len() as u32) & 0xFFFF_FFFF;
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
/// This is the polynomial used by gzip, PNG, and Ethernet.
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

    crate::serial_println!("[compress] Self-test passed.");
    Ok(())
}
