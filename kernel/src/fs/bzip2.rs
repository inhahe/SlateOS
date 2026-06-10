//! Bzip2 compression and decompression.
//!
//! Implements the bzip2 format as documented in the bzip2 source code
//! and Julian Seward's specification.
//!
//! ## Compression pipeline
//!
//! 1. RLE1: runs of 4+ identical bytes → 4 copies + repeat count byte
//! 2. BWT forward transform (prefix-doubling suffix sort, O(n log² n))
//! 3. Move-to-front encoding
//! 4. Zero-run encoding (RUNA/RUNB bijective base-2)
//! 5. Multi-table Huffman coding with iterative table optimization
//! 6. Framing: stream header, block headers with CRC, stream trailer
//!
//! ## Format overview
//!
//! A bzip2 stream consists of:
//! 1. Stream header: magic "BZh", block size digit ('1'–'9')
//! 2. One or more compressed blocks, each containing:
//!    - Block header (48-bit magic 0x314159265359)
//!    - Block CRC-32
//!    - Randomized flag (always 0 in modern bzip2)
//!    - BWT primary index (origPtr)
//!    - Symbol map (which bytes appear in the block)
//!    - Huffman coding tables (delta-encoded, grouped)
//!    - Huffman-encoded data (MTF + RLE encoded BWT output)
//! 3. Stream trailer (48-bit magic 0x177245385090, stream CRC)
//!
//! Decompression reverses: Huffman decode → MTF decode → BWT inverse
//! → RLE decode → original data.
//!
//! ## References
//!
//! - bzip2 source code (compress.c, decompress.c) by Julian Seward
//! - https://en.wikipedia.org/wiki/Bzip2#File_format

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// MSB-first bit reader (bzip2 uses big-endian bit ordering)
// ---------------------------------------------------------------------------

/// Reads bits from a byte buffer, most-significant-bit first.
///
/// Bzip2 uses MSB-first bit ordering, opposite to DEFLATE's LSB-first.
struct MsbBitReader<'a> {
    data: &'a [u8],
    pos: usize,
    bit: u8, // bits remaining in current byte (8 = fresh byte)
    live: u32, // bit buffer
}

impl<'a> MsbBitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0, bit: 0, live: 0 }
    }

    /// Read `n` bits (1..=24) and return as u32 (MSB first).
    fn read_bits(&mut self, n: u8) -> KernelResult<u32> {
        let mut result = 0u32;
        let mut remaining = n;

        while remaining > 0 {
            if self.bit == 0 {
                if self.pos >= self.data.len() {
                    return Err(KernelError::CorruptedData);
                }
                self.live = u32::from(self.data[self.pos]);
                self.pos = self.pos.wrapping_add(1);
                self.bit = 8;
            }

            // Take as many bits as we can from the current byte.
            let take = remaining.min(self.bit);
            let shift = self.bit.wrapping_sub(take);
            let mask = (1u32 << take).wrapping_sub(1);
            let bits = (self.live >> shift) & mask;

            result = (result << take) | bits;
            self.bit = self.bit.wrapping_sub(take);
            remaining = remaining.wrapping_sub(take);
        }

        Ok(result)
    }

    /// Read a single bit.
    fn read_bit(&mut self) -> KernelResult<bool> {
        Ok(self.read_bits(1)? != 0)
    }

    /// Read a byte (8 bits).
    fn read_byte(&mut self) -> KernelResult<u8> {
        Ok(self.read_bits(8)? as u8)
    }
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Block header magic: 0x314159265359 (pi digits).
const BLOCK_MAGIC_HI: u32 = 0x3141_5926;
const BLOCK_MAGIC_LO: u16 = 0x5359;

/// Stream end magic: 0x177245385090 (sqrt(pi) digits).
const END_MAGIC_HI: u32 = 0x1772_4538;
const END_MAGIC_LO: u16 = 0x5090;

/// Maximum block size (level 9 = 900,000 bytes).
const MAX_BLOCK_SIZE: usize = 900_000;

/// Maximum number of symbols in bzip2 alphabet.
/// 256 byte values + RUNA (0) + RUNB (1) + EOB.
const MAX_ALPHA_SIZE: usize = 258;

/// Maximum number of Huffman coding groups.
const MAX_GROUPS: usize = 6;

/// Number of symbols per Huffman group selector.
const GROUP_SIZE: usize = 50;

/// Maximum Huffman code length in bzip2.
const MAX_HUF_LEN: u8 = 20;

// ---------------------------------------------------------------------------
// CRC-32 for bzip2 (same polynomial as gzip/ZIP, 0xEDB88320 reflected)
// ---------------------------------------------------------------------------

/// CRC-32 update (bzip2 processes bytes MSB-first, using the unreflected
/// polynomial 0x04C11DB7 with left shifts).
///
/// Bzip2 uses a non-reflected CRC-32 variant:
///   crc = (crc << 8) ^ table[(crc >> 24) ^ byte]
#[allow(clippy::arithmetic_side_effects)]
fn bz2_crc32_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut i = 0usize;
    while i < 256 {
        let mut crc = (i as u32) << 24;
        let mut j = 0;
        while j < 8 {
            if crc & 0x8000_0000 != 0 {
                crc = (crc << 1) ^ 0x04C1_1DB7;
            } else {
                crc <<= 1;
            }
            j += 1;
        }
        table[i] = crc;
        i += 1;
    }
    table
}

#[allow(clippy::arithmetic_side_effects)]
fn bz2_crc32(data: &[u8]) -> u32 {
    let table = bz2_crc32_table();
    let mut crc: u32 = 0xFFFF_FFFF;
    for &b in data {
        let idx = ((crc >> 24) ^ u32::from(b)) as u8;
        crc = (crc << 8) ^ table[idx as usize];
    }
    crc ^ 0xFFFF_FFFF
}

// ---------------------------------------------------------------------------
// Huffman decoding tables for bzip2
// ---------------------------------------------------------------------------

/// A bzip2 Huffman decode table built from code lengths.
///
/// Uses min/max code tracking per length for fast decode.
struct Bz2HuffTable {
    /// Minimum code value at each length (1..=MAX_HUF_LEN).
    min_code: [u32; MAX_HUF_LEN as usize + 1],
    /// Maximum code value at each length.
    max_code: [i32; MAX_HUF_LEN as usize + 1],
    /// Base index into perm[] for each length.
    base: [u32; MAX_HUF_LEN as usize + 1],
    /// Permuted symbol table.
    perm: [u16; MAX_ALPHA_SIZE],
}

impl Bz2HuffTable {
    fn new() -> Self {
        Self {
            min_code: [0; MAX_HUF_LEN as usize + 1],
            max_code: [-1; MAX_HUF_LEN as usize + 1],
            base: [0; MAX_HUF_LEN as usize + 1],
            perm: [0; MAX_ALPHA_SIZE],
        }
    }

    /// Build decode table from code lengths.
    ///
    /// `lengths[i]` is the code length for symbol `i`.
    #[allow(clippy::arithmetic_side_effects)]
    fn build(&mut self, lengths: &[u8], alpha_size: usize) -> KernelResult<()> {
        // Count codes of each length.
        let mut count = [0u32; MAX_HUF_LEN as usize + 1];
        for i in 0..alpha_size {
            let l = lengths[i] as usize;
            if l > MAX_HUF_LEN as usize {
                return Err(KernelError::CorruptedData);
            }
            count[l] = count[l].wrapping_add(1);
        }

        // Compute base codes and perm table.
        let mut code = 0u32;
        let mut idx = 0u32;

        for len in 1..=MAX_HUF_LEN as usize {
            self.base[len] = idx;
            self.min_code[len] = code;

            for i in 0..alpha_size {
                if lengths[i] as usize == len {
                    if (idx as usize) < MAX_ALPHA_SIZE {
                        self.perm[idx as usize] = i as u16;
                    }
                    idx = idx.wrapping_add(1);
                }
            }

            // max_code[len] = code + count[len] - 1 (or -1 if no codes at this length)
            if count[len] > 0 {
                self.max_code[len] = (code.wrapping_add(count[len]).wrapping_sub(1)) as i32;
            } else {
                self.max_code[len] = -1;
            }

            code = code.wrapping_add(count[len]);
            code <<= 1;
        }

        Ok(())
    }

    /// Decode one symbol from the bit stream.
    fn decode(&self, reader: &mut MsbBitReader<'_>) -> KernelResult<u16> {
        let mut code = 0u32;

        for len in 1..=MAX_HUF_LEN as usize {
            let bit = reader.read_bits(1)?;
            code = (code << 1) | bit;

            if self.max_code[len] >= 0 && code <= self.max_code[len] as u32 {
                let idx = self.base[len].wrapping_add(code.wrapping_sub(self.min_code[len]));
                return self.perm.get(idx as usize)
                    .copied()
                    .ok_or(KernelError::CorruptedData);
            }
        }

        Err(KernelError::CorruptedData)
    }
}

// ---------------------------------------------------------------------------
// BWT inverse transform
// ---------------------------------------------------------------------------

/// Inverse Burrows-Wheeler Transform.
///
/// Given the BWT output `block[0..block_len]` and the primary index
/// (position of the original first character in the sorted matrix),
/// reconstruct the original data.
///
/// Uses the efficient O(n) algorithm:
/// 1. Count character frequencies
/// 2. Build cumulative frequency → starting positions
/// 3. Build the LF-mapping (transformation vector T)
/// 4. Follow T from origPtr for block_len steps
#[allow(clippy::arithmetic_side_effects)]
fn bwt_inverse(block: &[u8], block_len: usize, orig_ptr: u32) -> KernelResult<Vec<u8>> {
    if block_len == 0 {
        return Ok(Vec::new());
    }
    if orig_ptr as usize >= block_len {
        return Err(KernelError::CorruptedData);
    }

    // Step 1: Count character frequencies.
    let mut freq = [0u32; 256];
    for i in 0..block_len {
        freq[block[i] as usize] = freq[block[i] as usize].wrapping_add(1);
    }

    // Step 2: Cumulative frequencies → starting positions.
    // cumul[c] = number of characters < c in the block.
    let mut cumul = [0u32; 257];
    for i in 0..256 {
        cumul[i + 1] = cumul[i].wrapping_add(freq[i]);
    }

    // Step 3: Build the transformation vector T.
    // T[i] = the position where block[i] would go in the sorted output.
    // Using the "counting sort position" approach:
    //   For each position i (in order), T[i] = cumul[block[i]]++
    let mut t_vec = vec![0; block_len];

    // Reset cumul for building T (we need the running version).
    let mut running = [0u32; 256];
    for i in 0..256 {
        running[i] = cumul[i];
    }

    for i in 0..block_len {
        let c = block[i] as usize;
        t_vec[running[c] as usize] = i as u32;
        running[c] = running[c].wrapping_add(1);
    }

    // Step 4: Follow the chain from origPtr.
    let mut output = Vec::with_capacity(block_len);
    let mut idx = t_vec[orig_ptr as usize];
    for _ in 0..block_len {
        output.push(block[idx as usize]);
        idx = t_vec[idx as usize];
    }

    Ok(output)
}

// ---------------------------------------------------------------------------
// Move-to-front decode
// ---------------------------------------------------------------------------

/// Decode MTF-encoded symbols back to byte values.
///
/// MTF encoding replaces each byte with its position in a list that is
/// updated after each symbol (the most recently seen byte moves to
/// position 0).  This clusters common bytes near 0, improving Huffman
/// compression.
#[allow(clippy::arithmetic_side_effects)]
fn mtf_decode(symbols: &[u16], in_use: &[bool; 256]) -> Vec<u8> {
    // Build the initial MTF list from the in-use symbol map.
    let mut mtf_list = Vec::with_capacity(256);
    for (i, used) in in_use.iter().enumerate() {
        if *used {
            mtf_list.push(i as u8);
        }
    }

    let mut output = Vec::with_capacity(symbols.len());

    for &sym in symbols {
        let idx = sym as usize;
        if idx < mtf_list.len() {
            let byte = mtf_list[idx];
            output.push(byte);
            // Move to front.
            if idx > 0 {
                // Shift elements right and put byte at position 0.
                let mut j = idx;
                while j > 0 {
                    mtf_list[j] = mtf_list[j - 1];
                    j -= 1;
                }
                mtf_list[0] = byte;
            }
        }
    }

    output
}

// ---------------------------------------------------------------------------
// RLE decode (bzip2's initial RLE layer)
// ---------------------------------------------------------------------------

/// Decode bzip2's RLE1 encoding (post-BWT layer).
///
/// Bzip2 applies a simple RLE before the BWT: runs of 4+ identical
/// bytes are encoded as 4 copies + a repeat count byte.
///
/// Example: AAAAAAA → AAAA\x03 (4 A's + 3 more = 7 total)
#[allow(clippy::arithmetic_side_effects)]
fn rle_decode(data: &[u8]) -> Vec<u8> {
    let mut output = Vec::with_capacity(data.len());
    let mut i = 0;

    while i < data.len() {
        let b = data[i];
        output.push(b);
        i += 1;

        // Count consecutive identical bytes (up to 4).
        let mut run = 1usize;
        while run < 4 && i < data.len() && data[i] == b {
            output.push(b);
            run += 1;
            i += 1;
        }

        // If we got exactly 4 in a row, the next byte is a repeat count.
        if run == 4 && i < data.len() {
            let extra = data[i] as usize;
            i += 1;
            for _ in 0..extra {
                output.push(b);
            }
        }
    }

    output
}

// ---------------------------------------------------------------------------
// RUNA/RUNB zero-run decoding
// ---------------------------------------------------------------------------

/// Decode RUNA/RUNB encoded runs into the MTF symbol stream.
///
/// Bzip2 encodes runs of zeros (MTF symbol 0) using a bijective
/// base-2 encoding with symbols RUNA (0) and RUNB (1):
///   RUNA          → 1 zero
///   RUNB          → 2 zeros
///   RUNA RUNA     → 1 + 2 = 3 zeros
///   RUNB RUNA     → 2 + 2 = 4 zeros
///   RUNA RUNB     → 1 + 4 = 5 zeros
///   RUNB RUNB     → 2 + 4 = 6 zeros
///   ...
/// The formula: count += (symbol+1) * 2^position
///
/// Returns the decoded symbols (with runs expanded to literal 0s)
/// and everything else shifted by -1 (since RUNA=0 and RUNB=1 are
/// the run symbols, the actual MTF values start at 2 for symbol
/// index 1, etc.).

// ---------------------------------------------------------------------------
// Main decompression entry point
// ---------------------------------------------------------------------------

/// Decompress bzip2-compressed data.
///
/// Handles the full bzip2 format: stream header, one or more compressed
/// blocks, and stream trailer with CRC verification.
///
/// ## Errors
///
/// Returns `CorruptedData` if the data is not valid bzip2, the Huffman
/// tables are malformed, or the CRC doesn't match.
#[allow(clippy::arithmetic_side_effects, clippy::too_many_lines)]
pub fn bunzip2(data: &[u8]) -> KernelResult<Vec<u8>> {
    if data.len() < 10 {
        return Err(KernelError::CorruptedData);
    }

    let mut reader = MsbBitReader::new(data);

    // --- Stream header ---
    // Magic: 'B' 'Z' 'h' block_size_char
    let b = reader.read_byte()?;
    let z = reader.read_byte()?;
    let h = reader.read_byte()?;
    if b != b'B' || z != b'Z' || h != b'h' {
        return Err(KernelError::CorruptedData);
    }

    let level_char = reader.read_byte()?;
    if level_char < b'1' || level_char > b'9' {
        return Err(KernelError::CorruptedData);
    }
    let block_size_100k = (level_char.wrapping_sub(b'0')) as usize;
    let max_block = block_size_100k.saturating_mul(100_000);

    if max_block > MAX_BLOCK_SIZE {
        return Err(KernelError::CorruptedData);
    }

    let mut output = Vec::new();
    let mut stream_crc: u32 = 0;

    // --- Process blocks ---
    loop {
        // Read 48-bit magic (block or end-of-stream).
        let magic_hi = reader.read_bits(24)?;
        let magic_lo = reader.read_bits(24)?;

        let magic48_hi = (magic_hi << 8) | (magic_lo >> 16);
        let magic48_lo = (magic_lo & 0xFFFF) as u16;

        // Check for end-of-stream marker.
        if magic48_hi == END_MAGIC_HI && magic48_lo == END_MAGIC_LO {
            // Read and verify stream CRC.
            let stored_crc_hi = reader.read_bits(16)?;
            let stored_crc_lo = reader.read_bits(16)?;
            let stored_crc = (stored_crc_hi << 16) | stored_crc_lo;

            if stored_crc != stream_crc {
                crate::serial_println!(
                    "[bzip2] Stream CRC mismatch: stored={:#010x} computed={:#010x}",
                    stored_crc, stream_crc,
                );
                return Err(KernelError::CorruptedData);
            }
            break;
        }

        // Check for block header magic.
        if magic48_hi != BLOCK_MAGIC_HI || magic48_lo != BLOCK_MAGIC_LO {
            return Err(KernelError::CorruptedData);
        }

        // Decompress one block.
        let block_data = decode_block(&mut reader, max_block)?;

        // Update stream CRC: stream_crc = (stream_crc << 1 | stream_crc >> 31) ^ block_crc
        let block_crc = bz2_crc32(&block_data);
        stream_crc = stream_crc.rotate_left(1);
        stream_crc ^= block_crc;

        output.extend_from_slice(&block_data);
    }

    Ok(output)
}

/// Decode a single bzip2 block (after the 48-bit block magic has been read).
///
/// Reads the block header, Huffman tables, and encoded data, then
/// reverses the compression pipeline: Huffman → MTF → BWT inverse → RLE.
#[allow(clippy::arithmetic_side_effects, clippy::too_many_lines)]
fn decode_block(reader: &mut MsbBitReader<'_>, max_block: usize) -> KernelResult<Vec<u8>> {
    // Block CRC (32 bits).
    let block_crc_hi = reader.read_bits(16)?;
    let block_crc_lo = reader.read_bits(16)?;
    let block_crc = (block_crc_hi << 16) | block_crc_lo;

    // Randomized flag (always 0 in modern bzip2, deprecated).
    let _randomized = reader.read_bit()?;

    // BWT primary index (origPtr): 24 bits.
    let orig_ptr = reader.read_bits(24)?;

    // --- Symbol map ---
    // Two-level bitmap: 16-bit range selector, then per-range 16-bit bitmap.
    let used_ranges = reader.read_bits(16)?;
    let mut in_use = [false; 256];
    let mut n_in_use: usize = 0;

    for range in 0..16u32 {
        if used_ranges & (1u32 << (15u32.wrapping_sub(range))) != 0 {
            let range_bits = reader.read_bits(16)?;
            for bit in 0..16u32 {
                if range_bits & (1u32 << (15u32.wrapping_sub(bit))) != 0 {
                    let sym = range.wrapping_mul(16).wrapping_add(bit) as usize;
                    in_use[sym] = true;
                    n_in_use = n_in_use.wrapping_add(1);
                }
            }
        }
    }

    if n_in_use == 0 {
        return Err(KernelError::CorruptedData);
    }

    // Alphabet: RUNA (0), RUNB (1), MTF symbols (2..n_in_use), EOB (n_in_use+1).
    let alpha_size = n_in_use.wrapping_add(2);
    if alpha_size > MAX_ALPHA_SIZE {
        return Err(KernelError::CorruptedData);
    }

    // --- Huffman coding tables ---
    let n_groups = reader.read_bits(3)? as usize;
    if n_groups < 2 || n_groups > MAX_GROUPS {
        return Err(KernelError::CorruptedData);
    }

    let n_selectors = reader.read_bits(15)? as usize;
    if n_selectors == 0 || n_selectors > 18_002 {
        return Err(KernelError::CorruptedData);
    }

    // Read MTF-encoded selectors.
    let mut selector_mtf = Vec::with_capacity(n_selectors);
    for _ in 0..n_selectors {
        let mut j = 0u8;
        while reader.read_bit()? {
            j = j.wrapping_add(1);
            if j as usize >= n_groups {
                return Err(KernelError::CorruptedData);
            }
        }
        selector_mtf.push(j);
    }

    // Undo MTF on selectors.
    let selectors = undo_mtf_selectors(&selector_mtf, n_groups)?;

    // Read Huffman code lengths for each group (delta-encoded).
    let tables = read_huffman_tables(reader, n_groups, alpha_size)?;

    // --- Decode Huffman-encoded symbols ---
    let eob = (alpha_size.wrapping_sub(1)) as u16;
    let mtf_symbols = decode_symbols(reader, &tables, &selectors, eob, max_block)?;

    // --- Reverse the pipeline: MTF → BWT → RLE ---
    let mtf_decoded = mtf_decode(&mtf_symbols, &in_use);

    if mtf_decoded.len() > max_block {
        return Err(KernelError::CorruptedData);
    }

    let bwt_output = bwt_inverse(&mtf_decoded, mtf_decoded.len(), orig_ptr)?;
    let block_data = rle_decode(&bwt_output);

    // Verify block CRC.
    let computed_crc = bz2_crc32(&block_data);
    if computed_crc != block_crc {
        crate::serial_println!(
            "[bzip2] CRC mismatch: stored={:#010x} computed={:#010x} dec={}B",
            block_crc, computed_crc, block_data.len(),
        );
        return Err(KernelError::CorruptedData);
    }

    Ok(block_data)
}

/// Undo MTF encoding on the selector array.
fn undo_mtf_selectors(selector_mtf: &[u8], n_groups: usize) -> KernelResult<Vec<u8>> {
    let mut selectors = Vec::with_capacity(selector_mtf.len());
    let mut mtf_sel = Vec::with_capacity(n_groups);
    for i in 0..n_groups {
        mtf_sel.push(i as u8);
    }
    for &s in selector_mtf {
        let idx = s as usize;
        if idx >= mtf_sel.len() {
            return Err(KernelError::CorruptedData);
        }
        let val = mtf_sel[idx];
        selectors.push(val);
        // Move to front.
        let mut j = idx;
        while j > 0 {
            mtf_sel[j] = mtf_sel[j.wrapping_sub(1)];
            j = j.wrapping_sub(1);
        }
        mtf_sel[0] = val;
    }
    Ok(selectors)
}

/// Read delta-encoded Huffman code lengths for all groups.
#[allow(clippy::arithmetic_side_effects)]
fn read_huffman_tables(
    reader: &mut MsbBitReader<'_>,
    n_groups: usize,
    alpha_size: usize,
) -> KernelResult<Vec<Bz2HuffTable>> {
    let mut tables = Vec::with_capacity(n_groups);

    for _ in 0..n_groups {
        let mut lengths = [0u8; MAX_ALPHA_SIZE];
        let mut curr = reader.read_bits(5)? as i32;

        for sym in 0..alpha_size {
            // Delta coding: 0-bit = use current length; 1-bit then 0=inc, 1=dec.
            loop {
                if curr < 1 || curr > MAX_HUF_LEN as i32 {
                    return Err(KernelError::CorruptedData);
                }
                if !reader.read_bit()? {
                    break;
                }
                if reader.read_bit()? {
                    curr = curr.wrapping_sub(1);
                } else {
                    curr = curr.wrapping_add(1);
                }
            }
            lengths[sym] = curr as u8;
        }

        let mut table = Bz2HuffTable::new();
        table.build(&lengths, alpha_size)?;
        tables.push(table);
    }

    Ok(tables)
}

/// Decode Huffman-encoded symbols with RUNA/RUNB zero-run expansion.
///
/// Reads symbols from the bit stream using grouped Huffman tables
/// (each group of 50 symbols uses a different table, selected by the
/// selector array).  RUNA (0) and RUNB (1) symbols encode runs of
/// zeros using bijective base-2 encoding.
#[allow(clippy::arithmetic_side_effects)]
fn decode_symbols(
    reader: &mut MsbBitReader<'_>,
    tables: &[Bz2HuffTable],
    selectors: &[u8],
    eob: u16,
    max_block: usize,
) -> KernelResult<Vec<u16>> {
    let mut mtf_symbols: Vec<u16> = Vec::with_capacity(max_block.min(65536));
    let mut group_idx = 0usize;
    let mut group_pos = 0usize;
    loop {
        // Select the Huffman table for this position.
        if group_idx >= selectors.len() {
            return Err(KernelError::CorruptedData);
        }
        let table_idx = selectors[group_idx] as usize;
        if table_idx >= tables.len() {
            return Err(KernelError::CorruptedData);
        }

        let sym = tables[table_idx].decode(reader)?;

        if sym == eob {
            break;
        }

        if sym == 0 || sym == 1 {
            // RUNA/RUNB: bijective base-2 encoding of a run of zeros.
            // Accumulate the run length, reading more RUNA/RUNB symbols.
            let mut run_len = 0u32;
            let mut run_power = 1u32;
            let mut s = sym;
            let mut emitted = false;

            loop {
                // RUNA adds 1*power, RUNB adds 2*power.
                run_len = run_len.wrapping_add(
                    (u32::from(s).wrapping_add(1)).wrapping_mul(run_power)
                );
                run_power = run_power.wrapping_mul(2);

                // Advance group position.
                group_pos = group_pos.wrapping_add(1);
                if group_pos >= GROUP_SIZE {
                    group_pos = 0;
                    group_idx = group_idx.wrapping_add(1);
                }

                // Peek at the next symbol.
                if group_idx >= selectors.len() {
                    break;
                }
                let next_ti = selectors[group_idx] as usize;
                if next_ti >= tables.len() {
                    return Err(KernelError::CorruptedData);
                }
                let next_sym = tables[next_ti].decode(reader)?;

                if next_sym == 0 || next_sym == 1 {
                    s = next_sym;
                    // Continue accumulating the run.
                } else {
                    // End of run.  Emit the accumulated zero-run.
                    for _ in 0..run_len {
                        if mtf_symbols.len() >= max_block {
                            return Err(KernelError::CorruptedData);
                        }
                        mtf_symbols.push(0);
                    }
                    emitted = true;

                    // Advance position for the symbol we just read.
                    group_pos = group_pos.wrapping_add(1);
                    if group_pos >= GROUP_SIZE {
                        group_pos = 0;
                        group_idx = group_idx.wrapping_add(1);
                    }

                    if next_sym == eob {
                        return Ok(mtf_symbols);
                    }

                    // Regular MTF symbol: subtract 1 (RUNA=0, RUNB=1 are the
                    // run symbols; actual MTF values start at sym 2 → index 1).
                    if mtf_symbols.len() >= max_block {
                        return Err(KernelError::CorruptedData);
                    }
                    mtf_symbols.push(next_sym.wrapping_sub(1));
                    break;
                }
            }

            // If the inner loop ended due to selector exhaustion (not
            // because a non-run symbol was found), the zeros have not
            // been emitted yet.  Emit them now.
            if !emitted {
                for _ in 0..run_len {
                    if mtf_symbols.len() >= max_block {
                        return Err(KernelError::CorruptedData);
                    }
                    mtf_symbols.push(0);
                }
            }

            continue;
        }

        // Regular MTF symbol (subtract 1 for the RUNA/RUNB offset).
        if mtf_symbols.len() >= max_block {
            return Err(KernelError::CorruptedData);
        }
        mtf_symbols.push(sym.wrapping_sub(1));

        // Advance group position.
        group_pos = group_pos.wrapping_add(1);
        if group_pos >= GROUP_SIZE {
            group_pos = 0;
            group_idx = group_idx.wrapping_add(1);
        }
    }

    Ok(mtf_symbols)
}

// ---------------------------------------------------------------------------
// Bzip2 compression — MSB-first bit writer
// ---------------------------------------------------------------------------

/// MSB-first bit writer for bzip2 compression.
///
/// Writes bits MSB-first into each byte, matching the bzip2 format.
/// When a byte fills up, it's flushed to the output buffer.
struct MsbBitWriter {
    data: Vec<u8>,
    current: u8,
    bits_left: u8, // unfilled bits in current byte (8 = empty)
}

#[allow(clippy::arithmetic_side_effects)]
impl MsbBitWriter {
    fn with_capacity(cap: usize) -> Self {
        Self { data: Vec::with_capacity(cap), current: 0, bits_left: 8 }
    }

    /// Write `n` bits from `value` (MSB first, 1 ≤ n ≤ 24).
    fn write_bits(&mut self, value: u32, n: u8) {
        let mut remaining = n;
        let mut val = value;

        while remaining > 0 {
            let take = remaining.min(self.bits_left);
            let shift = remaining - take;
            let bits = (val >> shift) & ((1u32 << take) - 1);

            self.current |= (bits as u8) << (self.bits_left - take);
            self.bits_left -= take;
            remaining -= take;

            // Clear consumed bits from val.
            if shift > 0 {
                val &= (1u32 << shift) - 1;
            } else {
                val = 0;
            }

            if self.bits_left == 0 {
                self.data.push(self.current);
                self.current = 0;
                self.bits_left = 8;
            }
        }
    }

    /// Write a single bit.
    fn write_bit(&mut self, bit: bool) {
        if bit {
            self.current |= 1 << (self.bits_left - 1);
        }
        self.bits_left -= 1;
        if self.bits_left == 0 {
            self.data.push(self.current);
            self.current = 0;
            self.bits_left = 8;
        }
    }

    /// Write a byte (8 bits).
    fn write_byte(&mut self, byte: u8) {
        self.write_bits(u32::from(byte), 8);
    }

    /// Flush any remaining bits (zero-padded) and return the output buffer.
    fn finish(mut self) -> Vec<u8> {
        if self.bits_left < 8 {
            self.data.push(self.current);
        }
        self.data
    }
}

// ---------------------------------------------------------------------------
// Bzip2 compression — Huffman encoder
// ---------------------------------------------------------------------------

/// Huffman encoder: maps symbols to their canonical Huffman codes.
///
/// Generates codes matching the bzip2 decoder's canonical order:
/// symbols are assigned codes sorted by (length, symbol_index).
struct HuffEncoder {
    codes: Vec<u32>,
    lengths: Vec<u8>,
}

#[allow(clippy::arithmetic_side_effects)]
impl HuffEncoder {
    /// Build canonical Huffman codes from code lengths.
    fn from_lengths(lengths: &[u8], alpha_size: usize) -> Self {
        let mut max_len: u8 = 0;
        for &l in &lengths[..alpha_size] {
            if l > max_len { max_len = l; }
        }

        // Count codes at each length.
        let mut count = vec![0u32; max_len as usize + 2];
        for &l in &lengths[..alpha_size] {
            if l > 0 {
                count[l as usize] = count[l as usize].wrapping_add(1);
            }
        }

        // Compute starting code for each length (same algorithm as decoder).
        let mut code_at_len = vec![0u32; max_len as usize + 2];
        let mut code: u32 = 0;
        for len in 1..=max_len as usize {
            code_at_len[len] = code;
            code = (code.wrapping_add(count[len])) << 1;
        }

        // Assign codes: within each length, symbols ordered by index.
        let mut codes = vec![0u32; alpha_size];
        for len in 1..=max_len as usize {
            for sym in 0..alpha_size {
                if lengths[sym] as usize == len {
                    codes[sym] = code_at_len[len];
                    code_at_len[len] = code_at_len[len].wrapping_add(1);
                }
            }
        }

        Self {
            codes,
            lengths: lengths[..alpha_size].to_vec(),
        }
    }

    /// Encode a symbol, writing its Huffman code to the bit writer.
    fn encode(&self, writer: &mut MsbBitWriter, symbol: u16) {
        let sym = symbol as usize;
        // Use safe indexing for the lengths; a missing entry means
        // the symbol wasn't expected, but we fall back to length 0 (no write).
        let len = self.lengths.get(sym).copied().unwrap_or(0);
        if len == 0 { return; }
        let code = self.codes.get(sym).copied().unwrap_or(0);
        writer.write_bits(code, len);
    }
}

// ---------------------------------------------------------------------------
// Bzip2 compression — BWT forward (fast)
// ---------------------------------------------------------------------------

/// BWT forward transform using rank-accelerated sort (O(n log n) typical).
///
/// Uses prefix-doubling to build rank arrays that approximate cyclic
/// rotation order, then sorts using ranks as the primary key with a
/// full cyclic comparison as the tiebreaker.  This is correct for all
/// inputs and much faster than naive comparison sort for practical
/// block sizes.
///
/// Returns (transformed_data, primary_index).
#[allow(clippy::arithmetic_side_effects, clippy::indexing_slicing)]
fn bwt_forward_fast(data: &[u8]) -> (Vec<u8>, u32) {
    let n = data.len();
    if n == 0 {
        return (Vec::new(), 0);
    }
    if n == 1 {
        return (vec![data[0]], 0);
    }

    // Build rank array via prefix-doubling (Karp–Miller–Rosenberg).
    // rank[i] will encode the relative order of the 2^k-character cyclic
    // prefix starting at position i.
    let mut sa: Vec<u32> = (0..n as u32).collect();
    let mut rank: Vec<i32> = data.iter().map(|&b| b as i32).collect();
    let mut new_rank: Vec<i32> = vec![0; n];
    let mut h: usize = 1;

    while h < n {
        // Capture h by value so the closure is self-contained.
        let step = h;
        let r = &rank;
        let nn = n;
        sa.sort_by(|&a, &b| {
            let ai = a as usize;
            let bi = b as usize;
            let cmp1 = r[ai].cmp(&r[bi]);
            if cmp1 != core::cmp::Ordering::Equal {
                return cmp1;
            }
            r[(ai + step) % nn].cmp(&r[(bi + step) % nn])
        });

        // Recompute ranks from the sorted order.
        new_rank[sa[0] as usize] = 0;
        for i in 1..n {
            let curr = sa[i] as usize;
            let prev = sa[i - 1] as usize;
            let same = rank[curr] == rank[prev]
                && rank[(curr + step) % n] == rank[(prev + step) % n];
            new_rank[curr] = if same { new_rank[prev] } else { i as i32 };
        }

        rank.copy_from_slice(&new_rank);

        // If all ranks are unique, sorting is complete.
        let mut max_rank: i32 = 0;
        for &rv in rank.iter() {
            if rv > max_rank {
                max_rank = rv;
            }
        }
        if max_rank as usize == n - 1 {
            break;
        }

        h <<= 1;
    }

    // Final sort: use the rank array as primary key, with full cyclic
    // byte-by-byte comparison as tiebreaker.  This guarantees correct
    // output even if prefix-doubling didn't fully resolve all ties.
    {
        let r = &rank;
        let d = data;
        let nn = n;
        sa.sort_by(|&a, &b| {
            let ai = a as usize;
            let bi = b as usize;
            // Primary: use accumulated ranks (fast for most pairs).
            let cmp = r[ai].cmp(&r[bi]);
            if cmp != core::cmp::Ordering::Equal {
                return cmp;
            }
            // Tiebreaker: full cyclic byte comparison.
            for k in 0..nn {
                let ca = d[(ai + k) % nn];
                let cb = d[(bi + k) % nn];
                match ca.cmp(&cb) {
                    core::cmp::Ordering::Equal => continue,
                    other => return other,
                }
            }
            core::cmp::Ordering::Equal
        });
    }

    // Extract BWT output: the last character of each sorted rotation.
    let mut result = Vec::with_capacity(n);
    let mut orig_ptr = 0u32;
    for (i, &idx) in sa.iter().enumerate() {
        let pos = if idx == 0 { n - 1 } else { idx as usize - 1 };
        result.push(data[pos]);
        if idx == 0 {
            orig_ptr = i as u32;
        }
    }

    (result, orig_ptr)
}

// ---------------------------------------------------------------------------
// Bzip2 compression — zero-run encoding
// ---------------------------------------------------------------------------

/// Encode MTF symbols with RUNA/RUNB zero-run encoding.
///
/// Runs of MTF-value-0 are encoded using bijective base-2 with RUNA (0)
/// and RUNB (1).  Non-zero MTF values are shifted up by 1 (since symbols
/// 0 and 1 are reserved for RUNA/RUNB).  Appends EOB at the end.
#[allow(clippy::arithmetic_side_effects)]
fn zero_run_encode(mtf_symbols: &[u16], n_in_use: usize) -> Vec<u16> {
    let eob = (n_in_use + 1) as u16;
    let mut output = Vec::with_capacity(mtf_symbols.len().saturating_add(1));
    let mut i = 0;

    while i < mtf_symbols.len() {
        if mtf_symbols[i] == 0 {
            // Count run of zeros.
            let mut run_len = 0u32;
            while i < mtf_symbols.len() && mtf_symbols[i] == 0 {
                run_len = run_len.wrapping_add(1);
                i = i.wrapping_add(1);
            }

            // Encode in bijective base-2:
            // RUNA contributes 1 × 2^pos, RUNB contributes 2 × 2^pos.
            while run_len > 0 {
                run_len -= 1;
                if run_len & 1 == 0 {
                    output.push(0); // RUNA
                } else {
                    output.push(1); // RUNB
                }
                run_len >>= 1;
            }
        } else {
            // Non-zero MTF value: shift up by 1 (sym 2 → MTF idx 1, etc.).
            output.push(mtf_symbols[i].wrapping_add(1));
            i = i.wrapping_add(1);
        }
    }

    output.push(eob);
    output
}

// ---------------------------------------------------------------------------
// Bzip2 compression — Huffman code-length computation
// ---------------------------------------------------------------------------

/// Compute length-limited Huffman code lengths from symbol frequencies.
///
/// Uses a simple O(n²) tree construction (fine for bzip2's alpha_size ≤ 258)
/// and limits all code lengths to `max_len` (bzip2 uses 20).
#[allow(clippy::arithmetic_side_effects, clippy::indexing_slicing)]
fn compute_code_lengths(freqs: &[u32], alpha_size: usize, max_len: u8) -> Vec<u8> {
    if alpha_size == 0 {
        return Vec::new();
    }
    if alpha_size == 1 {
        return vec![1];
    }

    // Flat-array Huffman tree.  Nodes 0..alpha_size are leaves;
    // alpha_size..2*alpha_size-1 are internal nodes.
    let total_nodes = 2 * alpha_size - 1;
    let mut node_freq = vec![0u64; total_nodes];
    let mut parent = vec![0usize; total_nodes];
    let mut available = vec![false; total_nodes];

    // Initialize leaf frequencies (minimum 1 so every symbol gets a code).
    for i in 0..alpha_size {
        let f = freqs.get(i).copied().unwrap_or(0);
        node_freq[i] = u64::from(f).saturating_add(1);
        available[i] = true;
    }

    let mut next_internal = alpha_size;

    for _ in 0..alpha_size - 1 {
        // Find the two minimum-frequency available nodes.
        let mut min1 = 0usize;
        let mut min1_freq = u64::MAX;
        let mut min2 = 0usize;
        let mut min2_freq = u64::MAX;

        for j in 0..next_internal {
            if available[j] && node_freq[j] < min1_freq {
                min2 = min1;
                min2_freq = min1_freq;
                min1 = j;
                min1_freq = node_freq[j];
            } else if available[j] && node_freq[j] < min2_freq {
                min2 = j;
                min2_freq = node_freq[j];
            }
        }

        // Create internal node.
        node_freq[next_internal] = min1_freq.saturating_add(min2_freq);
        parent[min1] = next_internal;
        parent[min2] = next_internal;
        available[min1] = false;
        available[min2] = false;
        available[next_internal] = true;
        next_internal += 1;
    }

    // Compute depths from root.  Since parent[i] > i for all non-root nodes,
    // we can iterate downward from the root.
    let root = total_nodes - 1;
    parent[root] = root;
    let mut depth = vec![0u8; total_nodes];
    for i in (0..root).rev() {
        depth[i] = depth[parent[i]].saturating_add(1);
    }

    // Extract leaf depths, capping at max_len.
    let mut lengths: Vec<u8> = depth[..alpha_size]
        .iter()
        .map(|&d| d.min(max_len))
        .collect();

    // Fix the Kraft inequality after length limiting.
    fix_kraft_inequality(&mut lengths, max_len);

    lengths
}

/// Adjust code lengths so they satisfy the Kraft inequality after capping.
///
/// If the sum exceeds 1 (too many short codes for the number of symbols),
/// repeatedly lengthens the shortest code by 1 until the inequality holds.
#[allow(clippy::arithmetic_side_effects)]
fn fix_kraft_inequality(lengths: &mut [u8], max_len: u8) {
    // Safety limit to prevent infinite loop in degenerate cases.
    for _ in 0..2000 {
        let mut kraft: u64 = 0;
        for &l in lengths.iter() {
            if l > 0 && l <= max_len {
                kraft = kraft.saturating_add(1u64 << (max_len - l));
            }
        }

        let target = 1u64 << max_len;
        if kraft <= target {
            return;
        }

        // Over budget — find the shortest code (< max_len) and lengthen it.
        let mut min_len = max_len;
        for &l in lengths.iter() {
            if l > 0 && l < min_len {
                min_len = l;
            }
        }

        if min_len >= max_len {
            // All codes already at max_len; cannot fix further.
            return;
        }

        for l in lengths.iter_mut() {
            if *l == min_len {
                *l = (*l).saturating_add(1);
                break;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Bzip2 compression — multi-table Huffman optimization
// ---------------------------------------------------------------------------

/// Build Huffman tables with iterative group optimization.
///
/// Divides the encoded symbol stream into groups of 50, assigns each
/// group to one of `n_groups` Huffman tables, and iteratively refines
/// the assignments to minimize total encoded size (4 iterations,
/// matching the reference bzip2 compressor).
///
/// Returns (table_lengths, selectors).
#[allow(clippy::arithmetic_side_effects, clippy::indexing_slicing)]
fn build_tables_and_selectors(
    symbols: &[u16],
    alpha_size: usize,
    n_groups: usize,
) -> (Vec<Vec<u8>>, Vec<u8>) {
    let n_symbols = symbols.len();
    let n_selectors = n_symbols.div_ceil(GROUP_SIZE);

    // Initialize: distribute symbols evenly across groups for initial
    // frequency counts.
    let mut table_freqs: Vec<Vec<u32>> = vec![vec![0u32; alpha_size]; n_groups];
    let syms_per_group = if n_groups > 0 { n_symbols / n_groups } else { n_symbols };

    for (i, &sym) in symbols.iter().enumerate() {
        let group = if syms_per_group > 0 {
            (i / syms_per_group).min(n_groups - 1)
        } else {
            0
        };
        let s = sym as usize;
        if s < alpha_size {
            table_freqs[group][s] = table_freqs[group][s].wrapping_add(1);
        }
    }

    // Build initial Huffman tables from these frequencies.
    let mut table_lengths: Vec<Vec<u8>> = Vec::with_capacity(n_groups);
    for g in 0..n_groups {
        table_lengths.push(compute_code_lengths(&table_freqs[g], alpha_size, MAX_HUF_LEN));
    }

    let mut selectors = vec![0u8; n_selectors];

    // Iterative refinement: 4 passes (matching reference bzip2).
    for _iter in 0..4 {
        // Reset frequency tables.
        for g in 0..n_groups {
            for f in table_freqs[g].iter_mut() {
                *f = 0;
            }
        }

        // For each group of GROUP_SIZE symbols, find the best table.
        let mut sym_pos = 0usize;
        for sel_idx in 0..n_selectors {
            let end = (sym_pos + GROUP_SIZE).min(n_symbols);

            let mut best_group = 0u8;
            let mut best_bits = u64::MAX;

            for g in 0..n_groups {
                let mut bits = 0u64;
                for &sym in &symbols[sym_pos..end] {
                    let s = sym as usize;
                    if s < alpha_size {
                        bits = bits.wrapping_add(u64::from(table_lengths[g][s]));
                    }
                }
                if bits < best_bits {
                    best_bits = bits;
                    best_group = g as u8;
                }
            }

            selectors[sel_idx] = best_group;

            // Accumulate frequencies for the chosen table.
            for &sym in &symbols[sym_pos..end] {
                let s = sym as usize;
                if s < alpha_size {
                    table_freqs[best_group as usize][s] =
                        table_freqs[best_group as usize][s].wrapping_add(1);
                }
            }

            sym_pos = end;
        }

        // Rebuild Huffman tables from the new frequencies.
        for g in 0..n_groups {
            table_lengths[g] =
                compute_code_lengths(&table_freqs[g], alpha_size, MAX_HUF_LEN);
        }
    }

    (table_lengths, selectors)
}

/// MTF-encode selector indices for bitstream storage.
///
/// Produces small values (typically 0 or 1) that compress well as
/// unary codes in the bzip2 format.
#[allow(clippy::arithmetic_side_effects)]
fn mtf_encode_selectors(selectors: &[u8], n_groups: usize) -> Vec<u8> {
    let mut mtf: Vec<u8> = (0..n_groups as u8).collect();
    let mut output = Vec::with_capacity(selectors.len());

    for &sel in selectors {
        let mut pos = 0usize;
        while pos < mtf.len() && mtf[pos] != sel {
            pos = pos.wrapping_add(1);
        }
        output.push(pos as u8);
        if pos > 0 {
            let val = mtf[pos];
            let mut j = pos;
            while j > 0 {
                mtf[j] = mtf[j - 1];
                j -= 1;
            }
            mtf[0] = val;
        }
    }

    output
}

// ---------------------------------------------------------------------------
// Bzip2 compression — bitstream writers
// ---------------------------------------------------------------------------

/// Write the in-use symbol bitmap to the bitstream.
///
/// Two-level format: 16-bit range selector (which 16-byte ranges
/// contain used symbols), then for each active range a 16-bit bitmap.
#[allow(clippy::arithmetic_side_effects)]
fn write_symbol_map(writer: &mut MsbBitWriter, in_use: &[bool; 256]) {
    let mut range_used = [false; 16];
    for i in 0..256 {
        if in_use[i] {
            range_used[i / 16] = true;
        }
    }

    // Range selector (16 bits, MSB = range 0).
    for &used in &range_used {
        writer.write_bit(used);
    }

    // Per-range bitmaps (16 bits each for active ranges).
    for (range, &used) in range_used.iter().enumerate() {
        if used {
            for bit in 0..16usize {
                let idx = range * 16 + bit;
                writer.write_bit(in_use[idx]);
            }
        }
    }
}

/// Write delta-encoded Huffman code lengths for one table.
///
/// Format: 5-bit initial length, then per-symbol delta:
///   `0`  = use current length
///   `10` = increment current length by 1
///   `11` = decrement current length by 1
/// (repeat adjustments until current == target, then write `0`)
#[allow(clippy::arithmetic_side_effects, clippy::indexing_slicing)]
fn write_huffman_lengths(writer: &mut MsbBitWriter, lengths: &[u8], alpha_size: usize) {
    if alpha_size == 0 {
        return;
    }
    let mut curr = lengths[0] as i32;
    writer.write_bits(curr as u32, 5);

    for i in 0..alpha_size {
        let target = lengths[i] as i32;
        while curr < target {
            writer.write_bit(true);  // 1
            writer.write_bit(false); // 0 = increment
            curr += 1;
        }
        while curr > target {
            writer.write_bit(true);  // 1
            writer.write_bit(true);  // 1 = decrement
            curr -= 1;
        }
        writer.write_bit(false); // 0 = done, use current length
    }
}

// ---------------------------------------------------------------------------
// Bzip2 compression — block and stream encoding
// ---------------------------------------------------------------------------

/// Compress and write a single bzip2 block.
///
/// Applies the full bzip2 pipeline: RLE1 → BWT → MTF → zero-run →
/// Huffman.  Writes the block header and compressed data, and returns
/// the block's CRC-32 for stream CRC accumulation.
#[allow(clippy::arithmetic_side_effects, clippy::indexing_slicing)]
fn compress_block(writer: &mut MsbBitWriter, block: &[u8]) -> u32 {
    // Step 1: RLE1 encode (runs of 4+ identical bytes).
    let rle_data = rle_encode(block);

    // Step 2: BWT forward transform.
    let (bwt_data, orig_ptr) = bwt_forward_fast(&rle_data);

    // Step 3: Determine in-use symbols and MTF encode.
    let mut in_use = [false; 256];
    for &b in bwt_data.iter() {
        in_use[b as usize] = true;
    }
    let n_in_use = in_use.iter().filter(|&&b| b).count();
    let mtf_symbols = mtf_encode(&bwt_data, &in_use);

    // Step 4: Zero-run encode (RUNA/RUNB) with EOB appended.
    let encoded_symbols = zero_run_encode(&mtf_symbols, n_in_use);
    let alpha_size = n_in_use + 2; // RUNA + RUNB + MTF values + EOB

    // Step 5: Build Huffman tables with iterative optimization.
    // Number of groups depends on the encoded symbol count (matching
    // the reference bzip2 compressor's heuristic).
    let n_encoded = encoded_symbols.len();
    let n_groups = if n_encoded < 200 {
        2
    } else if n_encoded < 600 {
        3
    } else if n_encoded < 1200 {
        4
    } else if n_encoded < 2400 {
        5
    } else {
        6
    };

    let (table_lengths, selectors) =
        build_tables_and_selectors(&encoded_symbols, alpha_size, n_groups);

    // Step 6: MTF encode selectors.
    let selector_mtf = mtf_encode_selectors(&selectors, n_groups);

    // Step 7: Compute block CRC on original (pre-RLE) data.
    let block_crc = bz2_crc32(block);

    // --- Write block to bitstream ---

    // Block magic: 0x314159265359 (48 bits as two 24-bit halves).
    writer.write_bits(0x31_4159, 24);
    writer.write_bits(0x26_5359, 24);

    // Block CRC (32 bits as two 16-bit halves).
    writer.write_bits(block_crc >> 16, 16);
    writer.write_bits(block_crc & 0xFFFF, 16);

    // Randomized flag (1 bit: always 0 for modern bzip2).
    writer.write_bit(false);

    // BWT primary index (24 bits).
    writer.write_bits(orig_ptr, 24);

    // Symbol map.
    write_symbol_map(writer, &in_use);

    // Number of Huffman groups (3 bits).
    writer.write_bits(n_groups as u32, 3);

    // Number of selectors (15 bits).
    writer.write_bits(selectors.len() as u32, 15);

    // Write MTF-encoded selectors as unary codes (n ones followed by a zero).
    for &s in &selector_mtf {
        for _ in 0..s {
            writer.write_bit(true);
        }
        writer.write_bit(false);
    }

    // Write delta-encoded Huffman code lengths for each table.
    for g in 0..n_groups {
        write_huffman_lengths(writer, &table_lengths[g], alpha_size);
    }

    // Build encoders for the final tables.
    let mut encoders: Vec<HuffEncoder> = Vec::with_capacity(n_groups);
    for g in 0..n_groups {
        encoders.push(HuffEncoder::from_lengths(&table_lengths[g], alpha_size));
    }

    // Write Huffman-encoded symbols with group-based table selection.
    let mut group_pos = 0usize;
    let mut sel_idx = 0usize;

    for &sym in &encoded_symbols {
        let table = &encoders[selectors[sel_idx] as usize];
        table.encode(writer, sym);

        group_pos += 1;
        if group_pos >= GROUP_SIZE && sel_idx + 1 < selectors.len() {
            group_pos = 0;
            sel_idx += 1;
        }
    }

    block_crc
}

/// Compress data using bzip2.
///
/// `level` controls the block size: level 1 = 100 KiB blocks,
/// level 9 = 900 KiB blocks.  Higher levels give better compression
/// but use more memory (≈ 10× the block size for the BWT sort arrays).
///
/// The output is a complete bzip2 stream that can be decompressed
/// with [`bunzip2`].
pub fn bzip2_compress(data: &[u8], level: u8) -> Vec<u8> {
    let level = level.max(1).min(9);
    let block_size = (level as usize).saturating_mul(100_000);

    let mut writer = MsbBitWriter::with_capacity(data.len().saturating_add(256));

    // Stream header: 'B' 'Z' 'h' level_char.
    writer.write_byte(b'B');
    writer.write_byte(b'Z');
    writer.write_byte(b'h');
    #[allow(clippy::arithmetic_side_effects)]
    writer.write_byte(b'0' + level);

    let mut stream_crc: u32 = 0;
    let mut offset = 0usize;

    while offset < data.len() {
        let end = offset.saturating_add(block_size).min(data.len());
        let block = &data[offset..end];

        let block_crc = compress_block(&mut writer, block);

        // Update stream CRC: rotate left by 1, then XOR with block CRC.
        #[allow(clippy::arithmetic_side_effects)]
        {
            stream_crc = stream_crc.rotate_left(1);
            stream_crc ^= block_crc;
        }

        offset = end;
    }

    // Stream trailer: 0x177245385090 (48 bits).
    writer.write_bits(0x17_7245, 24);
    writer.write_bits(0x38_5090, 24);

    // Stream CRC (32 bits).
    #[allow(clippy::arithmetic_side_effects)]
    {
        writer.write_bits(stream_crc >> 16, 16);
        writer.write_bits(stream_crc & 0xFFFF, 16);
    }

    writer.finish()
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for bzip2 compression and decompression.
///
/// Validates individual transforms (BWT, MTF, RLE, CRC) and full
/// compression round-trip: bzip2_compress → bunzip2 → verify identity.
#[allow(clippy::arithmetic_side_effects)]
pub fn self_test() -> KernelResult<()> {
    crate::serial_println!("[bzip2] Running self-test...");

    // Test the BWT inverse with a known example.
    // BWT of "banana" with orig_ptr = 3 is "nnbaaa" (standard example).
    // Actually, let's use a simple known case:
    // BWT of "abcabc" → sorted rotations give a specific output.
    // For simplicity, test with a manually-constructed BWT:
    //
    // Original: "SIX.MIXED.PIXIES.SIFT.SIXTY.PIXIE.DUST.BOXES"
    // BWT:      "STEXYDST.E" (simplified — too complex for exact hand-computation)
    //
    // Instead, verify BWT inverse by applying BWT forward then inverse.
    // BWT forward: sort all rotations, take last column.
    let test_input = b"banana";
    let bwt_result = bwt_forward(test_input);
    let inverse = bwt_inverse(&bwt_result.0, bwt_result.0.len(), bwt_result.1)?;
    if inverse.as_slice() != test_input {
        crate::serial_println!(
            "[bzip2]   FAIL: BWT round-trip mismatch"
        );
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[bzip2]   BWT inverse round-trip verified ✓");

    // Test MTF encode/decode round-trip.
    let test_data = b"abracadabra";
    let mut in_use = [false; 256];
    for &b in test_data.iter() {
        in_use[b as usize] = true;
    }
    let mtf_encoded = mtf_encode(test_data, &in_use);
    let mtf_decoded = mtf_decode(&mtf_encoded, &in_use);
    if mtf_decoded.as_slice() != test_data {
        crate::serial_println!("[bzip2]   FAIL: MTF round-trip mismatch");
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[bzip2]   MTF encode/decode round-trip verified ✓");

    // Test RLE encode/decode round-trip.
    let rle_input = b"aaaaaabbbbcccccd";
    let rle_encoded = rle_encode(rle_input);
    let rle_decoded = rle_decode(&rle_encoded);
    if rle_decoded.as_slice() != rle_input {
        crate::serial_println!("[bzip2]   FAIL: RLE round-trip mismatch");
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[bzip2]   RLE encode/decode round-trip verified ✓");

    // Test bzip2 CRC-32.
    // bzip2 uses the same polynomial but unreflected (left-shifting).
    // CRC32 of "BZh" for sanity check — we'll verify against computed value.
    let crc = bz2_crc32(b"");
    if crc != 0 {
        // CRC of empty data with unreflected algorithm should be 0.
        // Actually... init=0xFFFFFFFF, XOR with 0xFFFFFFFF at end.
        // For empty data: just 0xFFFFFFFF ^ 0xFFFFFFFF = 0.
        crate::serial_println!(
            "[bzip2]   FAIL: CRC-32 empty data expected 0, got {:#010x}",
            crc
        );
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[bzip2]   CRC-32 verified ✓");

    // --- Compression round-trip tests ---
    crate::serial_println!("[bzip2]   Starting compression tests...");

    // Test 5: Compress and decompress a known string.
    let test_str = b"the quick brown fox jumps over the lazy dog";
    let compressed = bzip2_compress(test_str, 1);
    let decompressed = bunzip2(&compressed)?;
    if decompressed.as_slice() != test_str {
        crate::serial_println!("[bzip2]   FAIL: compression round-trip mismatch");
        return Err(KernelError::InternalError);
    }
    let ratio = if test_str.is_empty() {
        0
    } else {
        compressed.len().wrapping_mul(100) / test_str.len()
    };
    crate::serial_println!(
        "[bzip2]   Compression round-trip ({}B → {}B, {}%) ✓",
        test_str.len(), compressed.len(), ratio
    );

    // Test 6: Empty input round-trip.
    let compressed_empty = bzip2_compress(b"", 1);
    let decompressed_empty = bunzip2(&compressed_empty)?;
    if !decompressed_empty.is_empty() {
        crate::serial_println!("[bzip2]   FAIL: empty round-trip produced data");
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[bzip2]   Empty input round-trip ✓");

    // Test 7: Repetitive data (compresses well with BWT + Huffman).
    let mut rle_test = Vec::new();
    for _ in 0..500 {
        rle_test.extend_from_slice(b"ABCABC");
    }
    let comp7 = bzip2_compress(&rle_test, 1);
    let decomp7 = bunzip2(&comp7)?;
    if decomp7.as_slice() != rle_test.as_slice() {
        crate::serial_println!("[bzip2]   FAIL: repetitive data round-trip mismatch");
        return Err(KernelError::InternalError);
    }
    let ratio7 = if rle_test.is_empty() {
        0
    } else {
        comp7.len().wrapping_mul(100) / rle_test.len()
    };
    crate::serial_println!(
        "[bzip2]   Repetitive data round-trip ({}B → {}B, {}%) ✓",
        rle_test.len(), comp7.len(), ratio7
    );

    // Test 8: Single-byte input.
    let comp8 = bzip2_compress(b"X", 1);
    let decomp8 = bunzip2(&comp8)?;
    if decomp8.as_slice() != b"X" {
        crate::serial_println!("[bzip2]   FAIL: single-byte round-trip mismatch");
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[bzip2]   Single-byte round-trip ✓");

    // Test 9: All identical bytes (RLE + BWT edge case).
    let all_a = vec![b'A'; 300];
    let comp9 = bzip2_compress(&all_a, 1);
    let decomp9 = bunzip2(&comp9)?;
    if decomp9.as_slice() != all_a.as_slice() {
        crate::serial_println!("[bzip2]   FAIL: all-identical round-trip mismatch");
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[bzip2]   All-identical bytes round-trip ✓");

    // Test 10: BWT fast matches naive implementation.
    let bwt_test = b"abracadabra";
    let (naive_bwt, naive_ptr) = bwt_forward(bwt_test);
    let (fast_bwt, fast_ptr) = bwt_forward_fast(bwt_test);
    if naive_bwt != fast_bwt || naive_ptr != fast_ptr {
        crate::serial_println!(
            "[bzip2]   FAIL: BWT fast/naive mismatch: ptr {}/{}",
            naive_ptr, fast_ptr
        );
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[bzip2]   BWT fast matches naive ✓");

    crate::serial_println!("[bzip2] Self-test passed.");
    Ok(())
}

// ---------------------------------------------------------------------------
// Test helpers: BWT forward, MTF encode, RLE encode
// (Only used for self-tests — not needed for decompression)
// ---------------------------------------------------------------------------

/// BWT forward transform (for testing only).
///
/// Returns (transformed_data, primary_index).
#[allow(clippy::arithmetic_side_effects)]
fn bwt_forward(data: &[u8]) -> (Vec<u8>, u32) {
    let n = data.len();
    if n == 0 {
        return (Vec::new(), 0);
    }

    // Build all rotations' indices and sort them.
    let mut indices: Vec<usize> = (0..n).collect();

    indices.sort_by(|&a, &b| {
        for k in 0..n {
            let ca = data[(a + k) % n];
            let cb = data[(b + k) % n];
            match ca.cmp(&cb) {
                core::cmp::Ordering::Equal => continue,
                other => return other,
            }
        }
        core::cmp::Ordering::Equal
    });

    // Last column = data[(index + n - 1) % n] for each sorted rotation.
    let mut result = Vec::with_capacity(n);
    let mut orig_ptr = 0u32;
    for (rank, &idx) in indices.iter().enumerate() {
        result.push(data[(idx + n - 1) % n]);
        if idx == 0 {
            orig_ptr = rank as u32;
        }
    }

    (result, orig_ptr)
}

/// MTF encode (for testing only).
fn mtf_encode(data: &[u8], in_use: &[bool; 256]) -> Vec<u16> {
    let mut mtf_list = Vec::with_capacity(256);
    for (i, used) in in_use.iter().enumerate() {
        if *used {
            mtf_list.push(i as u8);
        }
    }

    let mut output = Vec::with_capacity(data.len());
    for &byte in data {
        let pos = mtf_list.iter().position(|&b| b == byte).unwrap_or(0);
        output.push(pos as u16);
        if pos > 0 {
            let val = mtf_list[pos];
            let mut j = pos;
            while j > 0 {
                mtf_list[j] = mtf_list[j - 1];
                j -= 1;
            }
            mtf_list[0] = val;
        }
    }

    output
}

/// RLE encode (for testing only — bzip2's initial RLE layer).
#[allow(clippy::arithmetic_side_effects)]
fn rle_encode(data: &[u8]) -> Vec<u8> {
    let mut output = Vec::with_capacity(data.len());
    let mut i = 0;

    while i < data.len() {
        let b = data[i];
        let mut run = 1usize;
        while i + run < data.len() && data[i + run] == b && run < 259 {
            run += 1;
        }

        if run >= 4 {
            // Emit 4 copies + repeat count.
            for _ in 0..4 {
                output.push(b);
            }
            let extra = run - 4;
            output.push(extra as u8);
            i += run;
        } else {
            // Emit individual bytes.
            for _ in 0..run {
                output.push(b);
            }
            i += run;
        }
    }

    output
}
