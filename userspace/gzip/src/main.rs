//! OurOS gzip/gunzip/zcat compression utility.
//!
//! Multi-personality binary: detects operating mode from `argv[0]`.
//!
//! # Modes
//!
//! - **gzip**: compress files to `.gz`, or compress stdin to stdout
//! - **gunzip**: decompress `.gz` files, or decompress stdin to stdout
//! - **zcat**: decompress to stdout (equivalent to `gunzip -c`)
//!
//! # Format
//!
//! Implements RFC 1952 (gzip file format) and RFC 1951 (DEFLATE compressed
//! data format). The compressor uses LZ77 matching with fixed Huffman codes.
//! The decompressor handles all three DEFLATE block types: stored, fixed
//! Huffman, and dynamic Huffman.

use std::env;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process;

// ============================================================================
// Constants
// ============================================================================

/// gzip magic bytes (ID1, ID2).
const GZIP_ID1: u8 = 0x1f;
const GZIP_ID2: u8 = 0x8b;

/// Compression method: DEFLATE.
const CM_DEFLATE: u8 = 8;

/// gzip flag bits in the FLG byte.
const FTEXT: u8 = 1 << 0;
const FHCRC: u8 = 1 << 1;
const FEXTRA: u8 = 1 << 2;
const FNAME: u8 = 1 << 3;
const FCOMMENT: u8 = 1 << 4;

/// Maximum match distance in LZ77 (DEFLATE spec: 32768 bytes).
const MAX_DIST: usize = 32768;

/// Maximum match length in LZ77 (DEFLATE spec: 258 bytes).
const MAX_MATCH: usize = 258;

/// Minimum match length for LZ77 (DEFLATE spec: 3 bytes).
const MIN_MATCH: usize = 3;

/// Hash table size for LZ77 (power of 2 for fast modulo).
const HASH_SIZE: usize = 65536;

// ============================================================================
// CRC32 implementation
// ============================================================================

/// Precomputed CRC32 table using the standard polynomial 0xEDB88320.
const fn make_crc32_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut i = 0usize;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            if c & 1 != 0 {
                c = 0xEDB8_8320 ^ (c >> 1);
            } else {
                c >>= 1;
            }
            k += 1;
        }
        table[i] = c;
        i += 1;
    }
    table
}

static CRC32_TABLE: [u32; 256] = make_crc32_table();

/// Update a running CRC32 with a slice of bytes.
fn crc32_update(crc: u32, data: &[u8]) -> u32 {
    let mut c = !crc;
    for &b in data {
        let idx = ((c ^ u32::from(b)) & 0xFF) as usize;
        c = CRC32_TABLE[idx] ^ (c >> 8);
    }
    !c
}

/// Compute the CRC32 of a complete byte slice.
fn crc32(data: &[u8]) -> u32 {
    crc32_update(0, data)
}

// ============================================================================
// DEFLATE fixed Huffman code tables (RFC 1951 section 3.2.6)
// ============================================================================
//
// Fixed literal/length Huffman code lengths:
//   0-143:   8 bits
//   144-255: 9 bits
//   256-279: 7 bits
//   280-287: 8 bits
//
// Fixed distance code lengths: all 5 bits.

/// Encode a literal/length symbol with the fixed Huffman code.
/// Returns (bits_value, bit_count).
fn fixed_litlen_encode(sym: u16) -> (u32, u32) {
    match sym {
        0..=143 => {
            // 8-bit codes: 00110000 - 10111111
            (0b0011_0000 + u32::from(sym), 8)
        }
        144..=255 => {
            // 9-bit codes: 110010000 - 111111111
            (0b1_1001_0000 + u32::from(sym) - 144, 9)
        }
        256..=279 => {
            // 7-bit codes: 0000000 - 0010111
            (u32::from(sym) - 256, 7)
        }
        280..=287 => {
            // 8-bit codes: 11000000 - 11000111
            (0b1100_0000 + u32::from(sym) - 280, 8)
        }
        _ => (0, 0), // should not happen
    }
}

/// Reverse the low `n` bits of `v` — needed because DEFLATE packs bits LSB-first
/// but the fixed code values are specified MSB-first in the RFC.
fn reverse_bits(v: u32, n: u32) -> u32 {
    let mut r = 0u32;
    let mut val = v;
    for _ in 0..n {
        r = (r << 1) | (val & 1);
        val >>= 1;
    }
    r
}

// ============================================================================
// Length / distance extra bits tables (RFC 1951 tables 1 and 2)
// ============================================================================

/// (base_length, extra_bits) for length codes 257..285.
/// Index 0 corresponds to code 257 (length 3).
const LENGTH_TABLE: [(u16, u8); 29] = [
    (3, 0),   // 257
    (4, 0),   // 258
    (5, 0),   // 259
    (6, 0),   // 260
    (7, 0),   // 261
    (8, 0),   // 262
    (9, 0),   // 263
    (10, 0),  // 264
    (11, 1),  // 265
    (13, 1),  // 266
    (15, 1),  // 267
    (17, 1),  // 268
    (19, 2),  // 269
    (23, 2),  // 270
    (27, 2),  // 271
    (31, 2),  // 272
    (35, 3),  // 273
    (43, 3),  // 274
    (51, 3),  // 275
    (59, 3),  // 276
    (67, 4),  // 277
    (83, 4),  // 278
    (99, 4),  // 279
    (115, 4), // 280
    (131, 5), // 281
    (163, 5), // 282
    (195, 5), // 283
    (227, 5), // 284
    (258, 0), // 285 (special: length 258 exactly)
];

/// (base_distance, extra_bits) for distance codes 0..29.
const DISTANCE_TABLE: [(u16, u8); 30] = [
    (1, 0),     // 0
    (2, 0),     // 1
    (3, 0),     // 2
    (4, 0),     // 3
    (5, 1),     // 4
    (7, 1),     // 5
    (9, 2),     // 6
    (13, 2),    // 7
    (17, 3),    // 8
    (25, 3),    // 9
    (33, 4),    // 10
    (49, 4),    // 11
    (65, 5),    // 12
    (97, 5),    // 13
    (129, 6),   // 14
    (193, 6),   // 15
    (257, 7),   // 16
    (385, 7),   // 17
    (513, 8),   // 18
    (769, 8),   // 19
    (1025, 9),  // 20
    (1537, 9),  // 21
    (2049, 10), // 22
    (3073, 10), // 23
    (4097, 11), // 24
    (6145, 11), // 25
    (8193, 12), // 26
    (12289, 12),// 27
    (16385, 13),// 28
    (24577, 13),// 29
];

/// Find the length code (index into LENGTH_TABLE, i.e. 0-28) for a match length.
fn length_code(len: usize) -> usize {
    match len {
        3..=10 => len - 3,
        11..=12 => 8,
        13..=14 => 9,
        15..=16 => 10,
        17..=18 => 11,
        19..=22 => 12,
        23..=26 => 13,
        27..=30 => 14,
        31..=34 => 15,
        35..=42 => 16,
        43..=50 => 17,
        51..=58 => 18,
        59..=66 => 19,
        67..=82 => 20,
        83..=98 => 21,
        99..=114 => 22,
        115..=130 => 23,
        131..=162 => 24,
        163..=194 => 25,
        195..=226 => 26,
        227..=257 => 27,
        258 => 28,
        _ => 28, // clamp to max
    }
}

/// Find the distance code (0-29) for a match distance.
fn distance_code(dist: usize) -> usize {
    match dist {
        1 => 0,
        2 => 1,
        3 => 2,
        4 => 3,
        5..=6 => 4,
        7..=8 => 5,
        9..=12 => 6,
        13..=16 => 7,
        17..=24 => 8,
        25..=32 => 9,
        33..=48 => 10,
        49..=64 => 11,
        65..=96 => 12,
        97..=128 => 13,
        129..=192 => 14,
        193..=256 => 15,
        257..=384 => 16,
        385..=512 => 17,
        513..=768 => 18,
        769..=1024 => 19,
        1025..=1536 => 20,
        1537..=2048 => 21,
        2049..=3072 => 22,
        3073..=4096 => 23,
        4097..=6144 => 24,
        6145..=8192 => 25,
        8193..=12288 => 26,
        12289..=16384 => 27,
        16385..=24576 => 28,
        _ => 29, // 24577..=32768
    }
}

// ============================================================================
// Bit writer (LSB first, as DEFLATE requires)
// ============================================================================

/// Buffers bits and flushes complete bytes to the underlying writer.
struct BitWriter<W: Write> {
    inner: W,
    /// Bit accumulator.
    buf: u64,
    /// Number of valid bits in `buf`.
    bits: u32,
}

impl<W: Write> BitWriter<W> {
    fn new(inner: W) -> Self {
        Self { inner, buf: 0, bits: 0 }
    }

    /// Write `n` bits of `val` (LSB-first into the stream).
    fn write_bits(&mut self, val: u32, n: u32) -> io::Result<()> {
        self.buf |= u64::from(val) << self.bits;
        self.bits += n;
        while self.bits >= 8 {
            self.inner.write_all(&[(self.buf & 0xFF) as u8])?;
            self.buf >>= 8;
            self.bits -= 8;
        }
        Ok(())
    }

    /// Flush remaining bits (zero-padded to byte boundary) and return inner.
    fn finish(mut self) -> io::Result<W> {
        if self.bits > 0 {
            self.inner.write_all(&[(self.buf & 0xFF) as u8])?;
        }
        Ok(self.inner)
    }
}

// ============================================================================
// Bit reader (LSB first)
// ============================================================================

/// Reads bits LSB-first from the underlying byte stream.
struct BitReader<R: Read> {
    inner: R,
    /// Bit accumulator.
    buf: u32,
    /// Number of valid bits in `buf`.
    bits: u32,
}

impl<R: Read> BitReader<R> {
    fn new(inner: R) -> Self {
        Self { inner, buf: 0, bits: 0 }
    }

    /// Read the next byte from the underlying reader into the accumulator.
    fn refill(&mut self) -> io::Result<()> {
        let mut b = [0u8; 1];
        self.inner.read_exact(&mut b)?;
        self.buf |= u32::from(b[0]) << self.bits;
        self.bits += 8;
        Ok(())
    }

    /// Read `n` bits (1..=16) from the stream.
    fn read_bits(&mut self, n: u32) -> io::Result<u32> {
        while self.bits < n {
            self.refill()?;
        }
        let val = self.buf & ((1u32 << n) - 1);
        self.buf >>= n;
        self.bits -= n;
        Ok(val)
    }

    /// Discard remaining bits in the current byte (byte-alignment).
    fn align_to_byte(&mut self) {
        let waste = self.bits % 8;
        self.buf >>= waste;
        self.bits -= waste;
    }

    /// Read a little-endian u16 at byte alignment (for stored blocks).
    fn read_u16_le(&mut self) -> io::Result<u16> {
        self.align_to_byte();
        let lo = self.read_bits(8)?;
        let hi = self.read_bits(8)?;
        Ok((lo | (hi << 8)) as u16)
    }
}

// ============================================================================
// Dynamic Huffman code decoder
// ============================================================================

/// A Huffman code table for decoding.
///
/// Uses a simple canonical Huffman approach: for each possible code length,
/// stores the first code and a list of symbols. Decoding reads bits one at a
/// time until a match is found.
struct HuffmanTable {
    /// `counts[n]` = number of symbols with code length `n` (0-indexed by length).
    counts: [u16; 16],
    /// Symbols in canonical order.
    symbols: Vec<u16>,
    /// `first_code[n]` = starting code for length `n`.
    first_code: [u32; 16],
    /// `first_sym[n]` = starting index into `symbols` for length `n`.
    first_sym: [u32; 16],
    /// Maximum code length in this table.
    max_len: u32,
}

impl HuffmanTable {
    /// Build a Huffman decode table from an array of code lengths.
    ///
    /// `lengths[i]` is the bit-length of symbol `i`; length 0 means the symbol
    /// is not present.
    fn from_lengths(lengths: &[u8]) -> Result<Self, String> {
        let mut counts = [0u16; 16];
        for &l in lengths {
            if l > 0 {
                counts[l as usize] = counts[l as usize]
                    .checked_add(1)
                    .ok_or_else(|| "huffman: too many symbols at one length".to_string())?;
            }
        }

        // Assign canonical codes.
        let mut first_code = [0u32; 16];
        let mut code: u32 = 0;
        for bits in 1..16 {
            code = (code + u32::from(counts[bits - 1])) << 1;
            first_code[bits] = code;
        }

        // Determine first_sym offsets.
        let mut first_sym = [0u32; 16];
        let mut pos: u32 = 0;
        for bits in 1..16 {
            first_sym[bits] = pos;
            pos += u32::from(counts[bits]);
        }
        let total_syms = pos as usize;

        // Fill symbols array.
        let mut symbols = vec![0u16; total_syms];
        let mut offsets = first_sym;
        for (sym, &l) in lengths.iter().enumerate() {
            if l > 0 {
                let idx = offsets[l as usize] as usize;
                if idx < total_syms {
                    symbols[idx] = sym as u16;
                }
                offsets[l as usize] += 1;
            }
        }

        let max_len = lengths.iter().copied().fold(0u8, u8::max) as u32;

        Ok(HuffmanTable { counts, symbols, first_code, first_sym, max_len })
    }

    /// Decode a single symbol from the bit reader.
    fn decode<R: Read>(&self, reader: &mut BitReader<R>) -> io::Result<u16> {
        let mut code: u32 = 0;
        for bits in 1..=self.max_len {
            let b = reader.read_bits(1)?;
            code = (code << 1) | b;
            let count = u32::from(self.counts[bits as usize]);
            let f = self.first_code[bits as usize];
            if count > 0 && code >= f && code < f + count {
                // `first_sym[bits]` is the starting index into `symbols` for
                // this code length. We add the offset within this length's run.
                let idx = (self.first_sym[bits as usize] + (code - f)) as usize;
                return self.symbols.get(idx).copied().ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidData, "huffman: symbol index out of range")
                });
            }
        }
        Err(io::Error::new(io::ErrorKind::InvalidData, "huffman: no symbol found"))
    }
}

// ============================================================================
// Fixed Huffman decode tables (built once, used for fixed blocks)
// ============================================================================

/// Build the fixed literal/length Huffman decode table.
fn fixed_litlen_table() -> Result<HuffmanTable, String> {
    let mut lengths = [0u8; 288];
    for i in 0usize..=143 {
        lengths[i] = 8;
    }
    for i in 144usize..=255 {
        lengths[i] = 9;
    }
    for i in 256usize..=279 {
        lengths[i] = 7;
    }
    for i in 280usize..=287 {
        lengths[i] = 8;
    }
    HuffmanTable::from_lengths(&lengths)
}

/// Build the fixed distance Huffman decode table (all 5 bits).
fn fixed_dist_table() -> Result<HuffmanTable, String> {
    let lengths = [5u8; 32];
    HuffmanTable::from_lengths(&lengths)
}

// ============================================================================
// DEFLATE decompressor
// ============================================================================

/// Decompress a DEFLATE stream from `reader` into `output`.
///
/// Returns `(bytes_written, crc32_of_output)`.
fn deflate_decompress<R: Read>(
    reader: &mut BitReader<R>,
    output: &mut Vec<u8>,
) -> Result<(u64, u32), String> {
    let start_len = output.len();
    let mut crc = 0u32;

    loop {
        let bfinal = reader
            .read_bits(1)
            .map_err(|e| format!("deflate: read BFINAL: {e}"))?;
        let btype = reader
            .read_bits(2)
            .map_err(|e| format!("deflate: read BTYPE: {e}"))?;

        match btype {
            0 => {
                // Stored block.
                let len = reader
                    .read_u16_le()
                    .map_err(|e| format!("deflate: read stored LEN: {e}"))?
                    as usize;
                let nlen = reader
                    .read_u16_le()
                    .map_err(|e| format!("deflate: read stored NLEN: {e}"))?
                    as usize;
                if (len ^ nlen) != 0xFFFF {
                    return Err(format!("deflate: stored block LEN/NLEN mismatch ({len} ^ {nlen} != 0xFFFF)"));
                }
                let old_len = output.len();
                output.resize(old_len + len, 0);
                // Read directly from byte stream.
                reader.align_to_byte();
                // Drain buffered bits byte by byte.
                let mut i = old_len;
                while reader.bits >= 8 && i < old_len + len {
                    output[i] = (reader.buf & 0xFF) as u8;
                    reader.buf >>= 8;
                    reader.bits -= 8;
                    i += 1;
                }
                // Read remaining from underlying stream.
                if i < old_len + len {
                    reader
                        .inner
                        .read_exact(&mut output[i..old_len + len])
                        .map_err(|e| format!("deflate: read stored data: {e}"))?;
                }
                crc = crc32_update(crc, &output[old_len..old_len + len]);
            }
            1 => {
                // Fixed Huffman codes.
                let litlen_table = fixed_litlen_table()
                    .map_err(|e| format!("deflate: build fixed litlen table: {e}"))?;
                let dist_table = fixed_dist_table()
                    .map_err(|e| format!("deflate: build fixed dist table: {e}"))?;
                let (written, block_crc) = decode_huffman_block(reader, output, &litlen_table, &dist_table)
                    .map_err(|e| format!("deflate: fixed block: {e}"))?;
                let _ = written;
                crc = crc32_update(crc, &[]);
                // Recompute crc over data we just wrote.
                let new_data_start = output.len() - (output.len() - start_len
                    - /* previously written */ {
                        // We don't track incremental, so just mark and re-run at end.
                        0
                    });
                let _ = block_crc;
                let _ = new_data_start;
            }
            2 => {
                // Dynamic Huffman codes.
                let (litlen_table, dist_table) = decode_dynamic_headers(reader)
                    .map_err(|e| format!("deflate: dynamic header: {e}"))?;
                let (_written, _block_crc) = decode_huffman_block(reader, output, &litlen_table, &dist_table)
                    .map_err(|e| format!("deflate: dynamic block: {e}"))?;
            }
            _ => {
                return Err(format!("deflate: reserved BTYPE {btype}"));
            }
        }

        if bfinal == 1 {
            break;
        }
    }

    // Compute CRC over the entire decompressed output.
    let bytes_written = (output.len() - start_len) as u64;
    let final_crc = crc32(&output[start_len..]);
    Ok((bytes_written, final_crc))
}

/// Decode a Huffman-coded block (used for both fixed and dynamic blocks).
///
/// Returns `(bytes_written, crc32_of_written_bytes)`.
fn decode_huffman_block<R: Read>(
    reader: &mut BitReader<R>,
    output: &mut Vec<u8>,
    litlen: &HuffmanTable,
    dist: &HuffmanTable,
) -> io::Result<(u64, u32)> {
    let start = output.len();
    loop {
        let sym = litlen.decode(reader)?;
        match sym {
            0..=255 => {
                output.push(sym as u8);
            }
            256 => {
                // End of block.
                break;
            }
            257..=285 => {
                // Length/distance back-reference.
                let lc = (sym - 257) as usize;
                if lc >= LENGTH_TABLE.len() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "deflate: length code out of range",
                    ));
                }
                let (base_len, extra_len_bits) = LENGTH_TABLE[lc];
                let extra_len = if extra_len_bits > 0 {
                    reader.read_bits(u32::from(extra_len_bits))? as u16
                } else {
                    0
                };
                let match_len = (base_len + extra_len) as usize;

                let dc = dist.decode(reader)? as usize;
                if dc >= DISTANCE_TABLE.len() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "deflate: distance code out of range",
                    ));
                }
                let (base_dist, extra_dist_bits) = DISTANCE_TABLE[dc];
                let extra_dist = if extra_dist_bits > 0 {
                    reader.read_bits(u32::from(extra_dist_bits))? as u16
                } else {
                    0
                };
                let match_dist = (base_dist + extra_dist) as usize;

                if match_dist > output.len() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "deflate: back-reference distance exceeds output",
                    ));
                }

                // Copy with potential overlap (RLE case: dist < len).
                let copy_start = output.len() - match_dist;
                for i in 0..match_len {
                    let b = output[copy_start + (i % match_dist)];
                    output.push(b);
                }
            }
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("deflate: invalid literal/length symbol {sym}"),
                ));
            }
        }
    }
    let written = (output.len() - start) as u64;
    let block_crc = crc32(&output[start..]);
    Ok((written, block_crc))
}

/// Decode the dynamic Huffman table headers (HLIT, HDIST, HCLEN + code lengths).
fn decode_dynamic_headers<R: Read>(
    reader: &mut BitReader<R>,
) -> io::Result<(HuffmanTable, HuffmanTable)> {
    let hlit = reader.read_bits(5)? as usize + 257; // number of literal/length codes
    let hdist = reader.read_bits(5)? as usize + 1;  // number of distance codes
    let hclen = reader.read_bits(4)? as usize + 4;  // number of code-length codes

    // Code-length code order (RFC 1951 §3.2.7).
    const CLEN_ORDER: [usize; 19] =
        [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];

    let mut clen_lengths = [0u8; 19];
    for i in 0..hclen {
        clen_lengths[CLEN_ORDER[i]] = reader.read_bits(3)? as u8;
    }

    let clen_table = HuffmanTable::from_lengths(&clen_lengths).map_err(|e| {
        io::Error::new(io::ErrorKind::InvalidData, e)
    })?;

    // Decode the literal/length + distance code lengths.
    let total = hlit + hdist;
    let mut lengths = vec![0u8; total];
    let mut i = 0;
    while i < total {
        let sym = clen_table.decode(reader)?;
        match sym {
            0..=15 => {
                lengths[i] = sym as u8;
                i += 1;
            }
            16 => {
                // Copy previous length 3–6 times.
                if i == 0 {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "deflate: RLE code 16 at start",
                    ));
                }
                let prev = lengths[i - 1];
                let repeat = reader.read_bits(2)? as usize + 3;
                for _ in 0..repeat {
                    if i >= total {
                        break;
                    }
                    lengths[i] = prev;
                    i += 1;
                }
            }
            17 => {
                // Repeat zero 3–10 times.
                let repeat = reader.read_bits(3)? as usize + 3;
                for _ in 0..repeat {
                    if i >= total {
                        break;
                    }
                    lengths[i] = 0;
                    i += 1;
                }
            }
            18 => {
                // Repeat zero 11–138 times.
                let repeat = reader.read_bits(7)? as usize + 11;
                for _ in 0..repeat {
                    if i >= total {
                        break;
                    }
                    lengths[i] = 0;
                    i += 1;
                }
            }
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("deflate: invalid code-length symbol {sym}"),
                ));
            }
        }
    }

    let litlen_table = HuffmanTable::from_lengths(&lengths[..hlit]).map_err(|e| {
        io::Error::new(io::ErrorKind::InvalidData, e)
    })?;
    let dist_table = HuffmanTable::from_lengths(&lengths[hlit..]).map_err(|e| {
        io::Error::new(io::ErrorKind::InvalidData, e)
    })?;

    Ok((litlen_table, dist_table))
}

// ============================================================================
// DEFLATE compressor (LZ77 + fixed Huffman)
// ============================================================================

/// LZ77 match result.
#[derive(Clone, Copy)]
enum Token {
    /// Literal byte.
    Literal(u8),
    /// Back-reference: (distance, length).
    Match(u16, u16),
}

/// Simple hash function for 3-byte sequences (for the LZ77 hash chain).
#[inline]
fn hash3(data: &[u8], pos: usize) -> usize {
    if pos + 2 >= data.len() {
        return 0;
    }
    let h = u32::from(data[pos])
        .wrapping_mul(2654435761)
        ^ u32::from(data[pos + 1]).wrapping_mul(1234567)
        ^ u32::from(data[pos + 2]);
    (h as usize) & (HASH_SIZE - 1)
}

/// Run LZ77 on `input` and produce a sequence of tokens.
fn lz77_compress(input: &[u8], level: u8) -> Vec<Token> {
    let n = input.len();
    if n == 0 {
        return Vec::new();
    }

    // Limit search depth based on compression level.
    let max_chain: usize = match level {
        1 => 4,
        2 => 8,
        3 => 16,
        4 => 32,
        5 => 64,
        6 => 128,
        7 => 256,
        8 => 512,
        _ => 1024, // level 9
    };

    let mut tokens = Vec::with_capacity(n);

    // Hash table: head[h] = most recent position with that hash.
    // prev[pos % MAX_DIST] = previous position in the hash chain.
    let mut head = vec![usize::MAX; HASH_SIZE];
    let mut prev = vec![usize::MAX; MAX_DIST];

    let mut pos = 0;

    while pos < n {
        if pos + MIN_MATCH > n {
            // Not enough bytes for a match; emit literals.
            tokens.push(Token::Literal(input[pos]));
            pos += 1;
            continue;
        }

        let h = hash3(input, pos);
        let mut best_len = MIN_MATCH - 1;
        let mut best_dist = 0usize;

        let mut cur = head[h];
        let mut chain_len = 0;
        while cur != usize::MAX && chain_len < max_chain {
            let dist = pos.wrapping_sub(cur);
            if dist == 0 || dist > MAX_DIST {
                break;
            }
            // Count matching bytes.
            let max_cmp = (n - pos).min(MAX_MATCH);
            let mut mlen = 0;
            while mlen < max_cmp && input[pos + mlen] == input[cur + mlen] {
                mlen += 1;
            }
            if mlen > best_len {
                best_len = mlen;
                best_dist = dist;
                if best_len >= MAX_MATCH {
                    break;
                }
            }
            cur = prev[cur % MAX_DIST];
            chain_len += 1;
        }

        // Update hash chain.
        prev[pos % MAX_DIST] = head[h];
        head[h] = pos;

        if best_len >= MIN_MATCH {
            tokens.push(Token::Match(best_dist as u16, best_len as u16));
            // Insert hashes for the matched positions.
            for k in 1..best_len {
                if pos + k + MIN_MATCH <= n {
                    let hk = hash3(input, pos + k);
                    prev[(pos + k) % MAX_DIST] = head[hk];
                    head[hk] = pos + k;
                }
            }
            pos += best_len;
        } else {
            tokens.push(Token::Literal(input[pos]));
            pos += 1;
        }
    }

    tokens
}

/// Compress `input` using DEFLATE with fixed Huffman codes.
///
/// The output is a valid DEFLATE stream (one or more blocks, final block
/// marked with BFINAL=1).
fn deflate_compress(input: &[u8], level: u8) -> Result<Vec<u8>, String> {
    // For level 0, emit a single stored block.
    if level == 0 {
        return deflate_compress_stored(input);
    }

    let tokens = lz77_compress(input, level);
    let mut output = Vec::new();
    let mut bw = BitWriter::new(&mut output);

    // Single final block, fixed Huffman.
    bw.write_bits(1, 1).map_err(|e| format!("deflate: write BFINAL: {e}"))?;
    bw.write_bits(1, 2).map_err(|e| format!("deflate: write BTYPE: {e}"))?;

    for token in &tokens {
        match *token {
            Token::Literal(b) => {
                let (bits, n) = fixed_litlen_encode(u16::from(b));
                let rev = reverse_bits(bits, n);
                bw.write_bits(rev, n)
                    .map_err(|e| format!("deflate: write literal: {e}"))?;
            }
            Token::Match(dist, len) => {
                let lc = length_code(len as usize);
                let sym = (lc + 257) as u16;
                let (bits, n) = fixed_litlen_encode(sym);
                let rev = reverse_bits(bits, n);
                bw.write_bits(rev, n)
                    .map_err(|e| format!("deflate: write length code: {e}"))?;

                // Extra length bits.
                let (base_len, extra_len_bits) = LENGTH_TABLE[lc];
                if extra_len_bits > 0 {
                    let extra = u32::from(len) - u32::from(base_len);
                    bw.write_bits(extra, u32::from(extra_len_bits))
                        .map_err(|e| format!("deflate: write len extra: {e}"))?;
                }

                // Distance code.
                let dc = distance_code(dist as usize);
                // Distance codes use 5-bit fixed codes, sent LSB-first reversed.
                bw.write_bits(reverse_bits(dc as u32, 5), 5)
                    .map_err(|e| format!("deflate: write dist code: {e}"))?;

                // Extra distance bits.
                let (base_dist, extra_dist_bits) = DISTANCE_TABLE[dc];
                if extra_dist_bits > 0 {
                    let extra = u32::from(dist) - u32::from(base_dist);
                    bw.write_bits(extra, u32::from(extra_dist_bits))
                        .map_err(|e| format!("deflate: write dist extra: {e}"))?;
                }
            }
        }
    }

    // End-of-block symbol (256), 7-bit code 0000000.
    let (bits, n) = fixed_litlen_encode(256);
    let rev = reverse_bits(bits, n);
    bw.write_bits(rev, n)
        .map_err(|e| format!("deflate: write EOB: {e}"))?;

    bw.finish().map_err(|e| format!("deflate: flush: {e}"))?;
    Ok(output)
}

/// Compress as a single stored (uncompressed) DEFLATE block.
fn deflate_compress_stored(input: &[u8]) -> Result<Vec<u8>, String> {
    // Stored blocks have a 64 KiB limit each.
    let mut output = Vec::new();
    let mut pos = 0;
    while pos < input.len() || input.is_empty() {
        let chunk_end = (pos + 65535).min(input.len());
        let chunk = &input[pos..chunk_end];
        let len = chunk.len() as u16;
        let nlen = !len;
        let is_final = chunk_end == input.len();
        output.push(if is_final { 1 } else { 0 }); // BFINAL | (BTYPE=0 << 1)
        output.push((len & 0xFF) as u8);
        output.push((len >> 8) as u8);
        output.push((nlen & 0xFF) as u8);
        output.push((nlen >> 8) as u8);
        output.extend_from_slice(chunk);
        pos = chunk_end;
        if input.is_empty() {
            break;
        }
    }
    Ok(output)
}

// ============================================================================
// gzip header/trailer read and write
// ============================================================================

/// A parsed gzip member header.
// Fields are used by tests and by callers that want to inspect headers.
#[allow(dead_code)]
struct GzipHeader {
    /// Original filename (if FNAME flag set), without the NUL terminator.
    fname: Option<String>,
    /// Modification time (Unix timestamp, 0 if unknown).
    mtime: u32,
    /// OS byte.
    os: u8,
}

/// Read a gzip member header from a byte slice, returning the header and the
/// number of bytes consumed.
fn read_gzip_header(data: &[u8]) -> Result<(GzipHeader, usize), String> {
    if data.len() < 10 {
        return Err("gzip: truncated header (need at least 10 bytes)".to_string());
    }
    if data[0] != GZIP_ID1 || data[1] != GZIP_ID2 {
        return Err(format!(
            "gzip: invalid magic bytes (got 0x{:02x} 0x{:02x}, expected 0x1f 0x8b)",
            data[0], data[1]
        ));
    }
    if data[2] != CM_DEFLATE {
        return Err(format!("gzip: unsupported compression method {}", data[2]));
    }
    let flg = data[3];
    if flg & !( FTEXT | FHCRC | FEXTRA | FNAME | FCOMMENT) != 0 {
        // Reserved bits set: technically an error but we'll ignore.
    }
    let mtime = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let os = data[9];

    let mut pos = 10usize;

    // Extra field.
    if flg & FEXTRA != 0 {
        if pos + 2 > data.len() {
            return Err("gzip: truncated FEXTRA length".to_string());
        }
        let xlen = u16::from_le_bytes([data[pos], data[pos + 1]]) as usize;
        pos += 2 + xlen;
        if pos > data.len() {
            return Err("gzip: truncated FEXTRA data".to_string());
        }
    }

    // Original filename.
    let fname = if flg & FNAME != 0 {
        let start = pos;
        while pos < data.len() && data[pos] != 0 {
            pos += 1;
        }
        if pos >= data.len() {
            return Err("gzip: unterminated FNAME".to_string());
        }
        let name = String::from_utf8_lossy(&data[start..pos]).into_owned();
        pos += 1; // skip NUL
        Some(name)
    } else {
        None
    };

    // Comment.
    if flg & FCOMMENT != 0 {
        while pos < data.len() && data[pos] != 0 {
            pos += 1;
        }
        if pos >= data.len() {
            return Err("gzip: unterminated FCOMMENT".to_string());
        }
        pos += 1;
    }

    // Header CRC.
    if flg & FHCRC != 0 {
        if pos + 2 > data.len() {
            return Err("gzip: truncated FHCRC".to_string());
        }
        // We read but don't validate FHCRC for now.
        pos += 2;
    }

    Ok((GzipHeader { fname, mtime, os }, pos))
}

/// Write a gzip member header into `out`.
fn write_gzip_header(out: &mut Vec<u8>, fname: Option<&str>, mtime: u32) {
    out.push(GZIP_ID1);
    out.push(GZIP_ID2);
    out.push(CM_DEFLATE); // CM
    let flg: u8 = if fname.is_some() { FNAME } else { 0 };
    out.push(flg);
    out.extend_from_slice(&mtime.to_le_bytes());
    out.push(0); // XFL (no specific compression level hints)
    out.push(255); // OS = unknown
    if let Some(name) = fname {
        out.extend_from_slice(name.as_bytes());
        out.push(0); // NUL terminator
    }
}

/// Write a gzip member trailer (CRC32 + ISIZE) into `out`.
fn write_gzip_trailer(out: &mut Vec<u8>, crc: u32, isize: u32) {
    out.extend_from_slice(&crc.to_le_bytes());
    out.extend_from_slice(&isize.to_le_bytes());
}

// ============================================================================
// High-level compress / decompress
// ============================================================================

/// Compress `input` into a complete gzip file (header + DEFLATE + trailer).
fn gzip_compress(
    input: &[u8],
    fname: Option<&str>,
    mtime: u32,
    level: u8,
) -> Result<Vec<u8>, String> {
    let deflated = deflate_compress(input, level)?;
    let input_crc = crc32(input);
    let isize = (input.len() & 0xFFFF_FFFF) as u32;

    let mut out = Vec::with_capacity(10 + deflated.len() + 8);
    write_gzip_header(&mut out, fname, mtime);
    out.extend_from_slice(&deflated);
    write_gzip_trailer(&mut out, input_crc, isize);
    Ok(out)
}

/// Decompress a gzip file `input`, returning the decompressed bytes.
///
/// Verifies the CRC32 and ISIZE in the trailer.
fn gzip_decompress(input: &[u8]) -> Result<Vec<u8>, String> {
    let (header, hdr_len) = read_gzip_header(input)?;
    let _ = header;

    if input.len() < hdr_len + 8 {
        return Err("gzip: file too short (no room for trailer)".to_string());
    }

    let deflate_data = &input[hdr_len..input.len() - 8];
    let trailer = &input[input.len() - 8..];
    let stored_crc = u32::from_le_bytes([trailer[0], trailer[1], trailer[2], trailer[3]]);
    let stored_isize = u32::from_le_bytes([trailer[4], trailer[5], trailer[6], trailer[7]]);

    let mut output = Vec::new();
    let cursor = io::Cursor::new(deflate_data);
    let mut reader = BitReader::new(cursor);
    let (bytes_written, computed_crc) =
        deflate_decompress(&mut reader, &mut output)
            .map_err(|e| format!("gzip: decompress: {e}"))?;

    let isize_actual = (bytes_written & 0xFFFF_FFFF) as u32;
    if isize_actual != stored_isize {
        return Err(format!(
            "gzip: ISIZE mismatch: stored={stored_isize}, actual={isize_actual}"
        ));
    }
    if computed_crc != stored_crc {
        return Err(format!(
            "gzip: CRC32 mismatch: stored=0x{stored_crc:08x}, actual=0x{computed_crc:08x}"
        ));
    }

    Ok(output)
}

/// Test the integrity of a gzip file without writing output.
fn gzip_test(input: &[u8]) -> Result<(), String> {
    gzip_decompress(input).map(|_| ())
}

// ============================================================================
// CLI mode detection and options
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToolMode {
    Gzip,
    Gunzip,
    Zcat,
}

/// Parsed command-line options.
struct Options {
    /// Original tool mode (gzip / gunzip / zcat). Preserved for diagnostics.
    #[allow(dead_code)]
    mode: ToolMode,
    /// Compression level 0-9.
    level: u8,
    /// Write to stdout (-c / zcat mode).
    stdout: bool,
    /// Keep original file after compressing/decompressing.
    keep: bool,
    /// Force overwrite of existing files.
    force: bool,
    /// Verbose output.
    verbose: bool,
    /// Recurse into directories.
    recursive: bool,
    /// Store / restore original filename in header.
    store_name: bool,
    /// Test integrity only.
    test: bool,
    /// Input files (empty means stdin).
    files: Vec<String>,
    /// Decompress mode (either gunzip or -d flag).
    decompress: bool,
}

/// Detect tool mode from `argv[0]`.
fn detect_mode(argv0: &str) -> ToolMode {
    // Strip path component, then any trailing .exe etc.
    let base = Path::new(argv0)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(argv0);
    match base {
        "gunzip" => ToolMode::Gunzip,
        "zcat" | "gzcat" => ToolMode::Zcat,
        _ => ToolMode::Gzip,
    }
}

fn parse_args() -> Result<Options, String> {
    let args: Vec<String> = env::args().collect();
    let argv0 = args.first().map(String::as_str).unwrap_or("gzip");
    let tool_mode = detect_mode(argv0);

    let mut level: u8 = 6;
    let mut stdout = tool_mode == ToolMode::Zcat;
    let mut keep = false;
    let mut force = false;
    let mut verbose = false;
    let mut recursive = false;
    let mut store_name = true;
    let mut test = false;
    let mut decompress = tool_mode == ToolMode::Gunzip || tool_mode == ToolMode::Zcat;
    let mut files: Vec<String> = Vec::new();

    let mut i = 1;
    while i < args.len() {
        let arg = args[i].as_str();
        match arg {
            "--help" | "-h" => {
                print_usage(tool_mode);
                process::exit(0);
            }
            "--version" => {
                println!("gzip (OurOS) 1.0");
                process::exit(0);
            }
            "-c" | "--stdout" | "--to-stdout" => stdout = true,
            "-d" | "--decompress" | "--uncompress" => decompress = true,
            "-k" | "--keep" => keep = true,
            "-f" | "--force" => force = true,
            "-v" | "--verbose" => verbose = true,
            "-r" | "--recursive" => recursive = true,
            "-N" | "--name" => store_name = true,
            "-n" | "--no-name" => store_name = false,
            "-t" | "--test" => test = true,
            "-1" | "--fast" => level = 1,
            "-2" => level = 2,
            "-3" => level = 3,
            "-4" => level = 4,
            "-5" => level = 5,
            "-6" => level = 6,
            "-7" => level = 7,
            "-8" => level = 8,
            "-9" | "--best" => level = 9,
            other if other.starts_with('-') && !other.starts_with("--") && other.len() > 2 => {
                // Bundled short flags like -cvk.
                for ch in other[1..].chars() {
                    match ch {
                        'c' => stdout = true,
                        'd' => decompress = true,
                        'k' => keep = true,
                        'f' => force = true,
                        'v' => verbose = true,
                        'r' => recursive = true,
                        'N' => store_name = true,
                        'n' => store_name = false,
                        't' => test = true,
                        '1'..='9' => level = ch as u8 - b'0',
                        _ => {
                            return Err(format!("gzip: unknown option: -{ch}"));
                        }
                    }
                }
            }
            other if other.starts_with('-') => {
                return Err(format!("gzip: unknown option: {other}"));
            }
            other => {
                files.push(other.to_string());
            }
        }
        i += 1;
    }

    // zcat always outputs to stdout.
    if tool_mode == ToolMode::Zcat {
        stdout = true;
        decompress = true;
    }

    Ok(Options {
        mode: tool_mode,
        level,
        stdout,
        keep,
        force,
        verbose,
        recursive,
        store_name,
        test,
        files,
        decompress,
    })
}

// ============================================================================
// File I/O helpers
// ============================================================================

/// Read an entire file into a Vec<u8>.
fn read_file(path: &Path) -> Result<Vec<u8>, String> {
    let mut f = File::open(path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let mut data = Vec::new();
    f.read_to_end(&mut data)
        .map_err(|e| format!("{}: read error: {e}", path.display()))?;
    Ok(data)
}

/// Write a byte slice to a file, refusing to overwrite unless `force` is set.
fn write_file(path: &Path, data: &[u8], force: bool) -> Result<(), String> {
    if path.exists() && !force {
        return Err(format!(
            "{}: already exists; use -f to overwrite",
            path.display()
        ));
    }
    let mut f = File::create(path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    f.write_all(data)
        .map_err(|e| format!("{}: write error: {e}", path.display()))?;
    Ok(())
}

/// Get the modification time of a file (seconds since epoch), or 0 on error.
fn file_mtime(path: &Path) -> u32 {
    fs::metadata(path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| (d.as_secs() & 0xFFFF_FFFF) as u32)
        .unwrap_or(0)
}

/// Compute a human-readable size string.
fn human_size(n: usize) -> String {
    if n < 1024 {
        format!("{n} B")
    } else if n < 1024 * 1024 {
        format!("{:.1} KiB", n as f64 / 1024.0)
    } else {
        format!("{:.1} MiB", n as f64 / (1024.0 * 1024.0))
    }
}

// ============================================================================
// Compress one file or stdin
// ============================================================================

fn compress_file(opts: &Options, path: Option<&Path>) -> Result<(), String> {
    let (input, fname, mtime, src_desc) = if let Some(p) = path {
        let data = read_file(p)?;
        let mtime = if opts.store_name { file_mtime(p) } else { 0 };
        let name = if opts.store_name {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(str::to_string)
        } else {
            None
        };
        (data, name, mtime, p.display().to_string())
    } else {
        let mut data = Vec::new();
        io::stdin()
            .lock()
            .read_to_end(&mut data)
            .map_err(|e| format!("stdin: {e}"))?;
        (data, None, 0u32, "stdin".to_string())
    };

    let compressed = gzip_compress(
        &input,
        fname.as_deref(),
        mtime,
        opts.level,
    )?;

    if opts.verbose {
        let pct = if input.is_empty() {
            0.0
        } else {
            100.0 * (1.0 - compressed.len() as f64 / input.len() as f64)
        };
        eprintln!(
            "{src_desc}: {:.1}% ({} -> {})",
            pct,
            human_size(input.len()),
            human_size(compressed.len())
        );
    }

    if opts.stdout || path.is_none() {
        io::stdout()
            .lock()
            .write_all(&compressed)
            .map_err(|e| format!("stdout: {e}"))?;
    } else if let Some(p) = path {
        let out_path = {
            let mut s = p.as_os_str().to_os_string();
            s.push(".gz");
            PathBuf::from(s)
        };
        write_file(&out_path, &compressed, opts.force)?;
        if !opts.keep {
            fs::remove_file(p).map_err(|e| format!("{}: remove: {e}", p.display()))?;
        }
    }

    Ok(())
}

// ============================================================================
// Decompress one file or stdin
// ============================================================================

fn decompress_file(opts: &Options, path: Option<&Path>) -> Result<(), String> {
    let (input, src_desc, inferred_out_path) = if let Some(p) = path {
        let data = read_file(p)?;
        let out = output_path_for(p);
        (data, p.display().to_string(), out)
    } else {
        let mut data = Vec::new();
        io::stdin()
            .lock()
            .read_to_end(&mut data)
            .map_err(|e| format!("stdin: {e}"))?;
        (data, "stdin".to_string(), None)
    };

    if opts.test {
        gzip_test(&input).map_err(|e| format!("{src_desc}: {e}"))?;
        if opts.verbose {
            eprintln!("{src_desc}: OK");
        }
        return Ok(());
    }

    let decompressed = gzip_decompress(&input)
        .map_err(|e| format!("{src_desc}: {e}"))?;

    if opts.verbose {
        let pct = if decompressed.is_empty() {
            0.0
        } else {
            100.0 * (1.0 - input.len() as f64 / decompressed.len() as f64)
        };
        eprintln!(
            "{src_desc}: {:.1}% ({} -> {})",
            pct,
            human_size(input.len()),
            human_size(decompressed.len())
        );
    }

    if opts.stdout || path.is_none() {
        io::stdout()
            .lock()
            .write_all(&decompressed)
            .map_err(|e| format!("stdout: {e}"))?;
    } else if let Some(out_path) = inferred_out_path {
        write_file(&out_path, &decompressed, opts.force)?;
        if !opts.keep {
            if let Some(p) = path {
                fs::remove_file(p).map_err(|e| format!("{}: remove: {e}", p.display()))?;
            }
        }
    } else {
        // No .gz suffix — with -f we can still decompress to stdout-like fallback.
        if opts.force {
            io::stdout()
                .lock()
                .write_all(&decompressed)
                .map_err(|e| format!("stdout: {e}"))?;
        } else {
            return Err(format!(
                "{src_desc}: unknown suffix -- ignored (use -f to force)"
            ));
        }
    }

    Ok(())
}

/// Determine the decompressed output path from the `.gz` input path.
///
/// Returns `None` if the file does not have a `.gz` (or `.z`, `.tgz`, `.taz`)
/// extension.
fn output_path_for(path: &Path) -> Option<PathBuf> {
    let p = path.to_string_lossy();
    for suffix in &[".gz", ".z", ".Z", ".tgz", ".taz"] {
        if p.ends_with(suffix) {
            let without = &p[..p.len() - suffix.len()];
            let out = if *suffix == ".tgz" || *suffix == ".taz" {
                format!("{}.tar", without)
            } else {
                without.to_string()
            };
            return Some(PathBuf::from(out));
        }
    }
    None
}

// ============================================================================
// Recursive directory walking
// ============================================================================

/// Process all files under `dir` recursively.
fn process_dir(opts: &Options, dir: &Path, errors: &mut Vec<String>) {
    let entries = match fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) => {
            errors.push(format!("{}: {e}", dir.display()));
            return;
        }
    };

    let mut children: Vec<PathBuf> = Vec::new();
    for entry in entries {
        match entry {
            Ok(e) => children.push(e.path()),
            Err(e) => errors.push(format!("{}: {e}", dir.display())),
        }
    }
    children.sort();

    for child in &children {
        if child.is_dir() {
            process_dir(opts, child, errors);
        } else {
            let result = if opts.decompress {
                decompress_file(opts, Some(child))
            } else {
                compress_file(opts, Some(child))
            };
            if let Err(e) = result {
                errors.push(e);
            }
        }
    }
}

// ============================================================================
// Usage / help
// ============================================================================

fn print_usage(mode: ToolMode) {
    match mode {
        ToolMode::Gzip => eprintln!(
            "\
Usage: gzip [OPTIONS] [FILES...]

Compress FILES (or stdin if none) using DEFLATE, writing .gz files.

Options:
  -1 to -9      Compression level (1=fast, 9=best, default: 6)
  --fast, -1    Alias for -1
  --best, -9    Alias for -9
  -c, --stdout  Write output to stdout; keep input files unchanged
  -d            Decompress (same as gunzip)
  -k, --keep    Keep (do not delete) input files
  -f, --force   Force compression even if output exists
  -r, --recursive Operate recursively on directories
  -v, --verbose Print compression statistics
  -n, --no-name Don't save or restore original filename/timestamp
  -N, --name    Save and restore original filename/timestamp (default)
  -t, --test    Test compressed file integrity
  -h, --help    Show this help"
        ),
        ToolMode::Gunzip => eprintln!(
            "\
Usage: gunzip [OPTIONS] [FILES...]

Decompress .gz FILES (or stdin if none).

Options:
  -c, --stdout  Write output to stdout; keep input files unchanged
  -k, --keep    Keep (do not delete) input files
  -f, --force   Force decompression even without .gz suffix
  -r, --recursive Operate recursively on directories
  -v, --verbose Print decompression statistics
  -t, --test    Test compressed file integrity
  -h, --help    Show this help"
        ),
        ToolMode::Zcat => eprintln!(
            "\
Usage: zcat [FILES...]

Decompress .gz FILES (or stdin if none) to stdout.
Equivalent to: gunzip -c"
        ),
    }
}

// ============================================================================
// Entry point
// ============================================================================

fn main() {
    let opts = match parse_args() {
        Ok(o) => o,
        Err(e) => {
            eprintln!("gzip: {e}");
            process::exit(2);
        }
    };

    if opts.files.is_empty() {
        // Operate on stdin/stdout.
        let result = if opts.decompress {
            decompress_file(&opts, None)
        } else {
            compress_file(&opts, None)
        };
        if let Err(e) = result {
            eprintln!("gzip: {e}");
            process::exit(1);
        }
        return;
    }

    let mut had_error = false;

    for file_arg in &opts.files.clone() {
        let path = Path::new(file_arg);
        if path.is_dir() {
            if opts.recursive {
                let mut errors = Vec::new();
                process_dir(&opts, path, &mut errors);
                for e in &errors {
                    eprintln!("gzip: {e}");
                    had_error = true;
                }
            } else {
                eprintln!("gzip: {file_arg}: is a directory -- ignored (use -r for recursive)");
                had_error = true;
            }
            continue;
        }

        if !path.exists() {
            eprintln!("gzip: {file_arg}: No such file or directory");
            had_error = true;
            continue;
        }

        let result = if opts.decompress {
            decompress_file(&opts, Some(path))
        } else {
            compress_file(&opts, Some(path))
        };

        if let Err(e) = result {
            eprintln!("gzip: {e}");
            had_error = true;
        }
    }

    if had_error {
        process::exit(1);
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ---- CRC32 ----

    #[test]
    fn test_crc32_empty() {
        assert_eq!(crc32(b""), 0x0000_0000);
    }

    #[test]
    fn test_crc32_known_value() {
        // CRC32 of "123456789" is 0xCBF43926 per the standard.
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn test_crc32_hello_world() {
        // Known-good value computed independently.
        assert_eq!(crc32(b"Hello, World!"), 0xEC4A_C3D0);
    }

    #[test]
    fn test_crc32_incremental() {
        let data = b"Hello, World!";
        let full = crc32(data);
        let half = crc32_update(0, &data[..6]);
        let incr = crc32_update(half, &data[6..]);
        assert_eq!(full, incr);
    }

    // ---- Bit writer / reader ----

    #[test]
    fn test_bit_writer_single_byte() {
        let mut out = Vec::new();
        let mut bw = BitWriter::new(&mut out);
        bw.write_bits(0b1010_0101, 8).unwrap();
        bw.finish().unwrap();
        assert_eq!(out, &[0b1010_0101]);
    }

    #[test]
    fn test_bit_writer_partial_bits() {
        let mut out = Vec::new();
        let mut bw = BitWriter::new(&mut out);
        bw.write_bits(0b101, 3).unwrap(); // bits 0-2
        bw.write_bits(0b00100, 5).unwrap(); // bits 3-7
        bw.finish().unwrap();
        // Combined LSB-first: 101 | 00100_0 = 0b0010_0101 (reading LSB first)
        assert_eq!(out, &[0b0010_0101]);
    }

    #[test]
    fn test_bit_reader_single_byte() {
        let data: &[u8] = &[0b1010_0101];
        let mut reader = BitReader::new(io::Cursor::new(data));
        let b = reader.read_bits(8).unwrap();
        assert_eq!(b, 0b1010_0101);
    }

    #[test]
    fn test_bit_roundtrip() {
        let mut out = Vec::new();
        let mut bw = BitWriter::new(&mut out);
        bw.write_bits(0b1101, 4).unwrap();
        bw.write_bits(0b0010, 4).unwrap();
        bw.finish().unwrap();
        let mut reader = BitReader::new(io::Cursor::new(&out));
        let a = reader.read_bits(4).unwrap();
        let b = reader.read_bits(4).unwrap();
        assert_eq!(a, 0b1101);
        assert_eq!(b, 0b0010);
    }

    // ---- Fixed Huffman codes ----

    #[test]
    fn test_fixed_litlen_literal_0() {
        // Symbol 0 -> 8-bit code 0x30 (00110000).
        let (bits, n) = fixed_litlen_encode(0);
        assert_eq!(n, 8);
        assert_eq!(bits, 0b0011_0000);
    }

    #[test]
    fn test_fixed_litlen_eob() {
        // Symbol 256 -> 7-bit code 0 (0000000).
        let (bits, n) = fixed_litlen_encode(256);
        assert_eq!(n, 7);
        assert_eq!(bits, 0);
    }

    #[test]
    fn test_fixed_litlen_symbol_280() {
        // Symbol 280 -> 8-bit code 0xC0 (11000000).
        let (bits, n) = fixed_litlen_encode(280);
        assert_eq!(n, 8);
        assert_eq!(bits, 0b1100_0000);
    }

    // ---- Length / distance code tables ----

    #[test]
    fn test_length_code_min() {
        assert_eq!(length_code(3), 0); // code 257
    }

    #[test]
    fn test_length_code_max() {
        assert_eq!(length_code(258), 28); // code 285
    }

    #[test]
    fn test_length_code_midrange() {
        assert_eq!(length_code(10), 7); // code 264
    }

    #[test]
    fn test_distance_code_1() {
        assert_eq!(distance_code(1), 0);
    }

    #[test]
    fn test_distance_code_max() {
        assert_eq!(distance_code(32768), 29);
    }

    #[test]
    fn test_distance_code_midrange() {
        assert_eq!(distance_code(5), 4);
        assert_eq!(distance_code(6), 4);
        assert_eq!(distance_code(7), 5);
    }

    // ---- Huffman table builder ----

    #[test]
    fn test_huffman_table_from_lengths_basic() {
        // Single symbol with length 1.
        let lengths = [1u8, 1u8];
        let tbl = HuffmanTable::from_lengths(&lengths).unwrap();
        assert_eq!(tbl.max_len, 1);
    }

    #[test]
    fn test_huffman_fixed_tables_build() {
        fixed_litlen_table().expect("fixed litlen table should build");
        fixed_dist_table().expect("fixed dist table should build");
    }

    // ---- DEFLATE stored blocks ----

    #[test]
    fn test_deflate_stored_empty() {
        let compressed = deflate_compress_stored(b"").unwrap();
        let mut reader = BitReader::new(io::Cursor::new(&compressed));
        let mut out = Vec::new();
        deflate_decompress(&mut reader, &mut out).unwrap();
        assert_eq!(out, b"");
    }

    #[test]
    fn test_deflate_stored_small() {
        let input = b"hello world";
        let compressed = deflate_compress_stored(input).unwrap();
        let mut reader = BitReader::new(io::Cursor::new(&compressed));
        let mut out = Vec::new();
        deflate_decompress(&mut reader, &mut out).unwrap();
        assert_eq!(out, input);
    }

    // ---- DEFLATE compress/decompress roundtrips ----

    #[test]
    fn test_deflate_roundtrip_empty() {
        let input = b"";
        for level in 0u8..=3 {
            let compressed = deflate_compress(input, level).unwrap();
            let mut reader = BitReader::new(io::Cursor::new(&compressed));
            let mut out = Vec::new();
            deflate_decompress(&mut reader, &mut out).unwrap();
            assert_eq!(out.as_slice(), input, "level={level}");
        }
    }

    #[test]
    fn test_deflate_roundtrip_short() {
        let input = b"Hello, DEFLATE!";
        let compressed = deflate_compress(input, 1).unwrap();
        let mut reader = BitReader::new(io::Cursor::new(&compressed));
        let mut out = Vec::new();
        deflate_decompress(&mut reader, &mut out).unwrap();
        assert_eq!(out.as_slice(), input);
    }

    #[test]
    fn test_deflate_roundtrip_repeated() {
        // Highly compressible input.
        let input: Vec<u8> = (0u8..=9).cycle().take(1000).collect();
        for level in 1u8..=3 {
            let compressed = deflate_compress(&input, level).unwrap();
            assert!(
                compressed.len() < input.len(),
                "level={level}: compressed ({}) should be smaller than input ({})",
                compressed.len(),
                input.len()
            );
            let mut reader = BitReader::new(io::Cursor::new(&compressed));
            let mut out = Vec::new();
            deflate_decompress(&mut reader, &mut out).unwrap();
            assert_eq!(out, input, "level={level}");
        }
    }

    #[test]
    fn test_deflate_roundtrip_binary() {
        let input: Vec<u8> = (0u8..=255).cycle().take(2048).collect();
        let compressed = deflate_compress(&input, 6).unwrap();
        let mut reader = BitReader::new(io::Cursor::new(&compressed));
        let mut out = Vec::new();
        deflate_decompress(&mut reader, &mut out).unwrap();
        assert_eq!(out, input);
    }

    #[test]
    fn test_deflate_roundtrip_all_same_byte() {
        let input = vec![0xAAu8; 512];
        let compressed = deflate_compress(&input, 9).unwrap();
        let mut reader = BitReader::new(io::Cursor::new(&compressed));
        let mut out = Vec::new();
        deflate_decompress(&mut reader, &mut out).unwrap();
        assert_eq!(out, input);
    }

    // ---- gzip header ----

    #[test]
    fn test_gzip_header_roundtrip_no_name() {
        let mut buf = Vec::new();
        write_gzip_header(&mut buf, None, 0);
        let (hdr, len) = read_gzip_header(&buf).unwrap();
        assert!(hdr.fname.is_none());
        assert_eq!(hdr.mtime, 0);
        assert_eq!(len, 10);
    }

    #[test]
    fn test_gzip_header_roundtrip_with_name() {
        let mut buf = Vec::new();
        write_gzip_header(&mut buf, Some("test.txt"), 12345);
        let (hdr, len) = read_gzip_header(&buf).unwrap();
        assert_eq!(hdr.fname, Some("test.txt".to_string()));
        assert_eq!(hdr.mtime, 12345);
        assert_eq!(len, 10 + 8 + 1); // header + "test.txt" + NUL
    }

    #[test]
    fn test_gzip_header_bad_magic() {
        let buf = [0x00u8, 0x00, 0x08, 0x00, 0, 0, 0, 0, 0, 0];
        assert!(read_gzip_header(&buf).is_err());
    }

    #[test]
    fn test_gzip_header_truncated() {
        let buf = [GZIP_ID1, GZIP_ID2, CM_DEFLATE];
        assert!(read_gzip_header(&buf).is_err());
    }

    // ---- Full gzip roundtrips ----

    #[test]
    fn test_gzip_roundtrip_empty() {
        let input = b"";
        let gz = gzip_compress(input, None, 0, 1).unwrap();
        let out = gzip_decompress(&gz).unwrap();
        assert_eq!(out.as_slice(), input);
    }

    #[test]
    fn test_gzip_roundtrip_short_text() {
        let input = b"The quick brown fox jumps over the lazy dog";
        let gz = gzip_compress(input, None, 0, 6).unwrap();
        let out = gzip_decompress(&gz).unwrap();
        assert_eq!(out.as_slice(), input);
    }

    #[test]
    fn test_gzip_roundtrip_with_filename() {
        let input = b"file contents here";
        let gz = gzip_compress(input, Some("original.txt"), 1609459200, 6).unwrap();
        let (hdr, _) = read_gzip_header(&gz).unwrap();
        assert_eq!(hdr.fname, Some("original.txt".to_string()));
        assert_eq!(hdr.mtime, 1609459200);
        let out = gzip_decompress(&gz).unwrap();
        assert_eq!(out.as_slice(), input);
    }

    #[test]
    fn test_gzip_roundtrip_repeated_data() {
        let input: Vec<u8> = b"abcdefgh".iter().cycle().take(4096).copied().collect();
        let gz = gzip_compress(&input, None, 0, 9).unwrap();
        assert!(gz.len() < input.len(), "gzip should compress repeated data");
        let out = gzip_decompress(&gz).unwrap();
        assert_eq!(out, input);
    }

    #[test]
    fn test_gzip_roundtrip_binary_data() {
        let input: Vec<u8> = (0u8..=255).cycle().take(3000).collect();
        let gz = gzip_compress(&input, None, 0, 6).unwrap();
        let out = gzip_decompress(&gz).unwrap();
        assert_eq!(out, input);
    }

    #[test]
    fn test_gzip_crc_mismatch_detected() {
        let input = b"test data";
        let mut gz = gzip_compress(input, None, 0, 1).unwrap();
        // Corrupt the CRC32 in the trailer.
        let len = gz.len();
        gz[len - 5] ^= 0xFF;
        assert!(gzip_decompress(&gz).is_err());
    }

    #[test]
    fn test_gzip_isize_mismatch_detected() {
        let input = b"test data";
        let mut gz = gzip_compress(input, None, 0, 1).unwrap();
        let len = gz.len();
        // Corrupt ISIZE (last 4 bytes of trailer).
        gz[len - 1] ^= 0x01;
        assert!(gzip_decompress(&gz).is_err());
    }

    #[test]
    fn test_gzip_test_valid() {
        let input = b"integrity check test";
        let gz = gzip_compress(input, None, 0, 6).unwrap();
        assert!(gzip_test(&gz).is_ok());
    }

    #[test]
    fn test_gzip_test_corrupt() {
        let input = b"integrity check test";
        let mut gz = gzip_compress(input, None, 0, 6).unwrap();
        // Corrupt a byte in the middle of the DEFLATE stream.
        let mid = gz.len() / 2;
        gz[mid] ^= 0xFF;
        assert!(gzip_test(&gz).is_err());
    }

    // ---- Output path deduction ----

    #[test]
    fn test_output_path_gz() {
        let out = output_path_for(Path::new("file.txt.gz")).unwrap();
        assert_eq!(out, PathBuf::from("file.txt"));
    }

    #[test]
    fn test_output_path_tgz() {
        let out = output_path_for(Path::new("archive.tgz")).unwrap();
        assert_eq!(out, PathBuf::from("archive.tar"));
    }

    #[test]
    fn test_output_path_no_suffix() {
        assert!(output_path_for(Path::new("file.txt")).is_none());
    }

    #[test]
    fn test_output_path_z_uppercase() {
        let out = output_path_for(Path::new("file.Z")).unwrap();
        assert_eq!(out, PathBuf::from("file"));
    }

    // ---- Mode detection ----

    #[test]
    fn test_detect_mode_gzip() {
        assert_eq!(detect_mode("gzip"), ToolMode::Gzip);
        assert_eq!(detect_mode("/usr/bin/gzip"), ToolMode::Gzip);
    }

    #[test]
    fn test_detect_mode_gunzip() {
        assert_eq!(detect_mode("gunzip"), ToolMode::Gunzip);
    }

    #[test]
    fn test_detect_mode_zcat() {
        assert_eq!(detect_mode("zcat"), ToolMode::Zcat);
        assert_eq!(detect_mode("gzcat"), ToolMode::Zcat);
    }

    // ---- LZ77 ----

    #[test]
    fn test_lz77_empty() {
        let tokens = lz77_compress(b"", 6);
        assert!(tokens.is_empty());
    }

    #[test]
    fn test_lz77_no_matches_short() {
        // 2 bytes — below MIN_MATCH, no matches possible.
        let tokens = lz77_compress(b"ab", 6);
        assert_eq!(tokens.len(), 2);
        assert!(matches!(tokens[0], Token::Literal(b'a')));
        assert!(matches!(tokens[1], Token::Literal(b'b')));
    }

    #[test]
    fn test_lz77_finds_match() {
        // "aaaa" — first 3 literals, then a back-reference.
        let tokens = lz77_compress(b"aaaa", 9);
        let has_match = tokens.iter().any(|t| matches!(t, Token::Match(_, _)));
        assert!(has_match, "should find a match in repeated bytes");
    }

    // ---- Compression level coverage ----

    #[test]
    fn test_compress_all_levels() {
        let input = b"The quick brown fox jumps over the lazy dog. The quick brown fox.";
        for level in 0u8..=9 {
            let gz = gzip_compress(input, None, 0, level).unwrap();
            let out = gzip_decompress(&gz).unwrap();
            assert_eq!(out.as_slice(), input, "failed at level={level}");
        }
    }

    #[test]
    fn test_large_input_roundtrip() {
        // ~64 KiB — exercises stored-block boundaries and larger hash chains.
        let input: Vec<u8> = (0u8..=255)
            .cycle()
            .take(65536)
            .collect();
        let gz = gzip_compress(&input, None, 0, 6).unwrap();
        let out = gzip_decompress(&gz).unwrap();
        assert_eq!(out, input);
    }

    #[test]
    fn test_human_size() {
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(2048), "2.0 KiB");
    }
}
