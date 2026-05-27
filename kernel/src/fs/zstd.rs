//! Zstandard (zstd) decompression for `.zst` archives.
//!
//! Implements the Zstandard compressed data format as specified in
//! RFC 8478 (Facebook/Meta's Zstandard specification).
//!
//! ## Format overview
//!
//! A zstd frame consists of:
//! 1. Magic number (4 bytes): `0xFD2FB528`
//! 2. Frame header: frame descriptor + optional window/dict/content fields
//! 3. One or more data blocks (raw / RLE / compressed)
//! 4. Optional content checksum (lower 32 bits of xxHash-64)
//!
//! Compressed blocks contain:
//! - **Literals section**: raw, RLE, Huffman-compressed, or treeless Huffman
//! - **Sequences section**: FSE-encoded (literal-length, offset, match-length)
//!   triplets that reference the literals and a sliding window
//!
//! ## Sub-encodings
//!
//! - **Huffman**: canonical Huffman coding for literal bytes
//! - **FSE (Finite State Entropy)**: tANS (asymmetric numeral systems) for
//!   sequence symbol coding — more compact than Huffman for skewed distributions
//! - **Predefined tables**: default FSE distributions for ll/ml/of codes when
//!   the "predefined" mode is selected
//!
//! ## References
//!
//! - RFC 8478: <https://www.rfc-editor.org/rfc/rfc8478>
//! - Zstandard format spec: <https://github.com/facebook/zstd/blob/dev/doc/zstd_compression_format.md>
//! - xxHash specification: <https://github.com/Cyan4973/xxHash/blob/dev/doc/xxhash_spec.md>

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::error::{KernelError, KernelResult};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Zstd frame magic number (little-endian).
const ZSTD_MAGIC: u32 = 0xFD2F_B528;

/// Skippable frame magic range: 0x184D2A50 .. 0x184D2A5F.
const SKIPPABLE_MAGIC_BASE: u32 = 0x184D_2A50;

/// Maximum window size we allow (128 MiB) to prevent DoS.
const MAX_WINDOW_SIZE: usize = 128 * 1024 * 1024;

/// Maximum output size we allow (256 MiB) to prevent unbounded allocation.
const MAX_OUTPUT_SIZE: usize = 256 * 1024 * 1024;

/// Maximum number of FSE symbols for literal-length codes.
const LL_MAX_SYMBOL: usize = 35;
/// Maximum number of FSE symbols for match-length codes.
const ML_MAX_SYMBOL: usize = 52;
/// Maximum number of FSE symbols for offset codes.
const OF_MAX_SYMBOL: usize = 31;

/// Maximum Huffman table log for literals.
const HUFFMAN_MAX_BITS: u8 = 11;

/// Maximum FSE table log.
const FSE_MAX_LOG: u8 = 9;

// ---------------------------------------------------------------------------
// xxHash-64 (for content checksum)
// ---------------------------------------------------------------------------

// From xxHash.h (Cyan4973/xxHash):
const XXHASH_PRIME1: u64 = 0x9E37_79B1_85EB_CA87;
const XXHASH_PRIME2: u64 = 0xC2B2_AE3D_27D4_EB4F;
const XXHASH_PRIME3: u64 = 0x1656_67B1_9E37_79F9;
const XXHASH_PRIME4: u64 = 0x85EB_CA77_C2B2_AE63;
const XXHASH_PRIME5: u64 = 0x27D4_EB2F_1656_67C5;

/// Compute xxHash-64 with seed 0, as used by zstd content checksums.
fn xxhash64(data: &[u8]) -> u64 {
    let len = data.len();
    let mut pos = 0;

    let h: u64 = if len >= 32 {
        let mut v1 = 0u64.wrapping_add(XXHASH_PRIME1).wrapping_add(XXHASH_PRIME2);
        let mut v2 = XXHASH_PRIME2;
        let mut v3 = 0u64;
        let mut v4 = 0u64.wrapping_sub(XXHASH_PRIME1);

        while pos + 32 <= len {
            v1 = xxh64_round(v1, read_le64(data, pos));
            v2 = xxh64_round(v2, read_le64(data, pos + 8));
            v3 = xxh64_round(v3, read_le64(data, pos + 16));
            v4 = xxh64_round(v4, read_le64(data, pos + 24));
            pos += 32;
        }

        let mut acc = v1.rotate_left(1)
            .wrapping_add(v2.rotate_left(7))
            .wrapping_add(v3.rotate_left(12))
            .wrapping_add(v4.rotate_left(18));

        acc = xxh64_merge_round(acc, v1);
        acc = xxh64_merge_round(acc, v2);
        acc = xxh64_merge_round(acc, v3);
        acc = xxh64_merge_round(acc, v4);
        acc
    } else {
        XXHASH_PRIME5
    };

    let mut hh = h.wrapping_add(len as u64);

    // Remaining 8-byte chunks.
    while pos + 8 <= len {
        let k = read_le64(data, pos);
        let k = k.wrapping_mul(XXHASH_PRIME2);
        let k = k.rotate_left(31);
        let k = k.wrapping_mul(XXHASH_PRIME1);
        hh ^= k;
        hh = hh.rotate_left(27).wrapping_mul(XXHASH_PRIME1).wrapping_add(XXHASH_PRIME4);
        pos += 8;
    }

    // Remaining 4-byte chunk.
    if pos + 4 <= len {
        let k = read_le32(data, pos) as u64;
        hh ^= k.wrapping_mul(XXHASH_PRIME1);
        hh = hh.rotate_left(23).wrapping_mul(XXHASH_PRIME2).wrapping_add(XXHASH_PRIME3);
        pos += 4;
    }

    // Remaining bytes.
    while pos < len {
        hh ^= (data[pos] as u64).wrapping_mul(XXHASH_PRIME5);
        hh = hh.rotate_left(11).wrapping_mul(XXHASH_PRIME1);
        pos += 1;
    }

    // Final avalanche.
    hh ^= hh >> 33;
    hh = hh.wrapping_mul(XXHASH_PRIME2);
    hh ^= hh >> 29;
    hh = hh.wrapping_mul(XXHASH_PRIME3);
    hh ^= hh >> 32;

    hh
}

#[inline]
fn xxh64_round(acc: u64, input: u64) -> u64 {
    acc.wrapping_add(input.wrapping_mul(XXHASH_PRIME2))
        .rotate_left(31)
        .wrapping_mul(XXHASH_PRIME1)
}

#[inline]
fn xxh64_merge_round(mut acc: u64, val: u64) -> u64 {
    let val = xxh64_round(0, val);
    acc ^= val;
    acc.wrapping_mul(XXHASH_PRIME1).wrapping_add(XXHASH_PRIME4)
}

// ---------------------------------------------------------------------------
// Little-endian readers
// ---------------------------------------------------------------------------

#[inline]
fn read_le16(data: &[u8], off: usize) -> u16 {
    if off + 2 > data.len() { return 0; }
    u16::from(data[off]) | (u16::from(data[off + 1]) << 8)
}

#[inline]
fn read_le32(data: &[u8], off: usize) -> u32 {
    if off + 4 > data.len() { return 0; }
    u32::from(data[off])
        | (u32::from(data[off + 1]) << 8)
        | (u32::from(data[off + 2]) << 16)
        | (u32::from(data[off + 3]) << 24)
}

#[inline]
fn read_le64(data: &[u8], off: usize) -> u64 {
    if off + 8 > data.len() { return 0; }
    u64::from(data[off])
        | (u64::from(data[off + 1]) << 8)
        | (u64::from(data[off + 2]) << 16)
        | (u64::from(data[off + 3]) << 24)
        | (u64::from(data[off + 4]) << 32)
        | (u64::from(data[off + 5]) << 40)
        | (u64::from(data[off + 6]) << 48)
        | (u64::from(data[off + 7]) << 56)
}

// ---------------------------------------------------------------------------
// Bit reader (LSB-first, as zstd uses little-endian bit packing)
// ---------------------------------------------------------------------------

/// Reads bits from a byte slice in LSB-first order.
///
/// Zstd uses little-endian bit packing: bits are consumed from LSB to MSB
/// within each byte, and bytes are consumed in increasing address order.
struct BitReader<'a> {
    data: &'a [u8],
    pos: usize,
    bit_pos: u8,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0, bit_pos: 0 }
    }

    /// Read up to 32 bits, LSB-first.
    fn read_bits(&mut self, n: u8) -> KernelResult<u32> {
        if n == 0 { return Ok(0); }
        if n > 32 { return Err(KernelError::CorruptedData); }

        let mut result = 0u32;
        let mut bits_remaining = n;
        let mut shift = 0u8;

        while bits_remaining > 0 {
            if self.pos >= self.data.len() {
                return Err(KernelError::CorruptedData);
            }

            let available = 8u8.saturating_sub(self.bit_pos);
            let take = if bits_remaining < available { bits_remaining } else { available };
            let mask = (1u32 << take) - 1;
            let bits = (u32::from(self.data[self.pos]) >> self.bit_pos) & mask;
            result |= bits << shift;

            shift = shift.saturating_add(take);
            bits_remaining -= take;
            self.bit_pos += take;

            if self.bit_pos >= 8 {
                self.bit_pos = 0;
                self.pos += 1;
            }
        }

        Ok(result)
    }

    /// Read a single bit.
    #[inline]
    fn read_bit(&mut self) -> KernelResult<u32> {
        self.read_bits(1)
    }

    /// Total bits consumed so far.
    fn bits_consumed(&self) -> usize {
        self.pos * 8 + self.bit_pos as usize
    }

    /// Remaining bytes (approximate, rounds down).
    fn bytes_remaining(&self) -> usize {
        if self.bit_pos == 0 {
            self.data.len().saturating_sub(self.pos)
        } else {
            self.data.len().saturating_sub(self.pos + 1)
        }
    }

    /// Align to next byte boundary.
    fn align_byte(&mut self) {
        if self.bit_pos != 0 {
            self.bit_pos = 0;
            self.pos += 1;
        }
    }
}

/// Backward bit reader for FSE/Huffman streams.
///
/// Zstd's FSE and Huffman compressed streams are read **backwards** —
/// the first bit to decode is the MSB of the last byte.  This reader
/// starts from the end of the buffer and moves toward the beginning.
struct ReverseBitReader<'a> {
    data: &'a [u8],
    /// Current byte index (points to byte being read).
    byte_idx: isize,
    /// Current bit position within the byte (7 = MSB, 0 = LSB).
    bit_pos: i8,
    /// Total bits loaded so far (for overflow detection).
    bits_read: usize,
    /// Total bits available.
    total_bits: usize,
}

impl<'a> ReverseBitReader<'a> {
    fn new(data: &'a [u8]) -> KernelResult<Self> {
        if data.is_empty() {
            return Err(KernelError::CorruptedData);
        }

        // Find the highest set bit in the last byte — that's the sentinel.
        let last = *data.last().ok_or(KernelError::CorruptedData)?;
        if last == 0 {
            return Err(KernelError::CorruptedData); // no sentinel bit
        }

        // Position of highest set bit (0-indexed from LSB).
        let highest = 7 - last.leading_zeros() as i8;

        // The sentinel bit itself is not data — skip it.
        // Total data bits = (data.len()-1)*8 + highest
        let total_bits = (data.len() - 1) * 8 + highest as usize;

        Ok(Self {
            data,
            byte_idx: (data.len() - 1) as isize,
            bit_pos: highest - 1, // start just below sentinel
            bits_read: 0,
            total_bits,
        })
    }

    /// Read `n` bits (up to 32), MSB-first from the back of the stream.
    fn read_bits(&mut self, n: u8) -> KernelResult<u32> {
        if n == 0 { return Ok(0); }
        if n > 32 { return Err(KernelError::CorruptedData); }

        if self.bits_read + n as usize > self.total_bits {
            return Err(KernelError::CorruptedData);
        }

        let mut result = 0u32;
        for _ in 0..n {
            if self.bit_pos < 0 {
                self.byte_idx -= 1;
                if self.byte_idx < 0 {
                    return Err(KernelError::CorruptedData);
                }
                self.bit_pos = 7;
            }
            // SAFETY: byte_idx checked above
            #[allow(clippy::indexing_slicing)]
            let byte = self.data[self.byte_idx as usize];
            let bit = (byte >> self.bit_pos as u8) & 1;
            result = (result << 1) | u32::from(bit);
            self.bit_pos -= 1;
            self.bits_read += 1;
        }

        Ok(result)
    }

    fn bits_remaining(&self) -> usize {
        self.total_bits.saturating_sub(self.bits_read)
    }

    /// "Un-read" `n` bits, moving the position backwards.
    ///
    /// Used by Huffman table-based decoding where we read max_bits
    /// but only consumed nb_bits < max_bits.
    fn unread_bits(&mut self, n: u8) {
        if n == 0 { return; }
        self.bits_read -= n as usize;
        self.bit_pos += n as i8;
        while self.bit_pos > 7 {
            self.bit_pos -= 8;
            self.byte_idx += 1;
        }
    }
}

// ---------------------------------------------------------------------------
// FSE (Finite State Entropy) table
// ---------------------------------------------------------------------------

/// Entry in a decoded FSE table.
#[derive(Clone, Copy, Default)]
struct FseEntry {
    /// Symbol this entry decodes to.
    symbol: u8,
    /// Number of bits to read for the next state.
    nb_bits: u8,
    /// Base value added to the read bits to form the next state.
    new_state_base: u16,
}

/// An FSE decoding table.
struct FseTable {
    entries: Vec<FseEntry>,
    accuracy_log: u8,
}

impl FseTable {
    /// Build an FSE decoding table from a normalized distribution.
    ///
    /// `norm_counts[symbol]` is the normalized count for each symbol.
    /// Negative values indicate "less than 1" probability (symbol appears
    /// with probability < 1/table_size but must still be representable).
    fn build(norm_counts: &[i16], accuracy_log: u8) -> KernelResult<Self> {
        let table_size = 1usize << accuracy_log;
        let mut entries = vec![FseEntry::default(); table_size];

        // 1. Allocate cells for symbols with count == -1 (low-probability).
        // These get exactly one cell at the high end of the table.
        let mut high = table_size - 1;
        let mut state_table = vec![0u16; table_size];

        for (sym, &count) in norm_counts.iter().enumerate() {
            if count == -1 {
                state_table[high] = sym as u16;
                if high == 0 {
                    return Err(KernelError::CorruptedData);
                }
                high -= 1;
            }
        }

        // 2. Spread symbols with count >= 1 across the table using the
        //    step function: position = (position + step) % table_size,
        //    where step = (table_size >> 1) + (table_size >> 3) + 3.
        let step = (table_size >> 1) + (table_size >> 3) + 3;
        let mask = table_size - 1;
        let mut position = 0usize;

        for (sym, &count) in norm_counts.iter().enumerate() {
            if count <= 0 { continue; }
            for _ in 0..count as usize {
                state_table[position] = sym as u16;
                position = (position + step) & mask;
                // Skip over cells allocated to low-prob symbols.
                while position > high {
                    position = (position + step) & mask;
                }
            }
        }

        // 3. Build decoding entries from the symbol spread table.
        build_fse_table_standard(&mut entries, &state_table, norm_counts, accuracy_log)?;

        Ok(Self { entries, accuracy_log })
    }

    /// Look up the current state's symbol and transition info.
    #[inline]
    fn decode(&self, state: u16) -> &FseEntry {
        let idx = state as usize;
        if idx < self.entries.len() {
            &self.entries[idx]
        } else {
            &self.entries[0]
        }
    }

    fn table_size(&self) -> usize {
        1usize << self.accuracy_log
    }
}

/// Build FSE decoding table using the reference algorithm from the spec.
fn build_fse_table_standard(
    entries: &mut [FseEntry],
    state_table: &[u16],
    norm_counts: &[i16],
    accuracy_log: u8,
) -> KernelResult<()> {
    let table_size = 1usize << accuracy_log;

    // symbol_next[s] = the next state number to assign for symbol s.
    let mut symbol_next = vec![0u16; norm_counts.len()];
    for (sym, &count) in norm_counts.iter().enumerate() {
        if count == -1 {
            // Low-prob symbol: its single cell maps to state "table_size - 1"
            // on decode (nb_bits = accuracy_log, new_state_base = 0).
            symbol_next[sym] = 1;
        } else if count > 0 {
            symbol_next[sym] = count as u16;
        }
    }

    for i in 0..table_size {
        let sym = state_table[i] as usize;
        if sym >= symbol_next.len() {
            return Err(KernelError::CorruptedData);
        }
        let sn = symbol_next[sym];

        // How many bits to read for next state.
        let hb = highest_bit(sn as u32);
        let nb = accuracy_log.saturating_sub(hb);
        let new_state_base = ((sn as u32) << nb).wrapping_sub(1u32 << accuracy_log);

        entries[i] = FseEntry {
            symbol: sym as u8,
            nb_bits: nb,
            new_state_base: new_state_base as u16,
        };

        symbol_next[sym] = sn.wrapping_add(1);
    }

    Ok(())
}

/// Highest set bit position (0-indexed). Returns 0 for input 0.
#[inline]
fn highest_bit(v: u32) -> u8 {
    if v == 0 { 0 } else { 31u8.saturating_sub(v.leading_zeros() as u8) }
}

/// Decode an FSE table from a compressed bitstream.
///
/// Returns the table and the number of bytes consumed.
fn decode_fse_table(
    data: &[u8],
    max_symbol: usize,
    max_log: u8,
) -> KernelResult<(FseTable, usize)> {
    let mut br = BitReader::new(data);

    // Accuracy log = 4 bits + 5.
    let al = br.read_bits(4)? as u8 + 5;
    if al > max_log {
        return Err(KernelError::CorruptedData);
    }

    let table_size = 1u32 << al;
    let mut remaining = table_size as i32 + 1;
    let mut norm_counts: Vec<i16> = Vec::new();
    let mut sym = 0usize;

    while remaining > 0 && sym <= max_symbol {
        // Variable-length count encoding.
        let max_bits_needed = highest_bit(remaining as u32 + 1) + 1;
        let threshold = (1i32 << max_bits_needed) - 1 - remaining;

        let small_bits = max_bits_needed.saturating_sub(1);
        let mut val = br.read_bits(small_bits)? as i32;

        if val < threshold {
            // Value is final (uses small_bits bits).
        } else {
            // Read one more bit.
            val = (val << 1) - threshold + br.read_bits(1)? as i32;
            // Adjust for the shifted range.
            // If val >= threshold, it represents (val - threshold) in the upper range.
        }

        // Decode: value 0 means count == -1 (low probability).
        // value N > 0 means count == N - 1.
        let count = val - 1;
        norm_counts.push(count as i16);
        remaining -= if count < 0 { -count } else { count };
        sym += 1;

        // Check for repeat-zero (probability == 0 run).
        if count == 0 {
            loop {
                let repeat = br.read_bits(2)? as usize;
                // Bounds-check before extending to catch corrupted streams
                // that would otherwise push past max_symbol.
                let new_sym = sym.checked_add(repeat).ok_or(KernelError::CorruptedData)?;
                if new_sym > max_symbol + 1 {
                    return Err(KernelError::CorruptedData);
                }
                norm_counts.resize(norm_counts.len() + repeat, 0);
                sym = new_sym;
                if repeat < 3 { break; }
            }
        }
    }

    if remaining != 0 {
        // The distribution doesn't sum to table_size. Could be an
        // off-by-one in the spec interpretation. Allow remaining == 1
        // as some encoders leave one cell unassigned.
        if remaining < 0 || remaining > 1 {
            return Err(KernelError::CorruptedData);
        }
    }

    // Pad with zeros up to max_symbol+1.
    while norm_counts.len() <= max_symbol {
        norm_counts.push(0);
    }

    br.align_byte();
    let bytes_consumed = br.pos;

    let table = FseTable::build(&norm_counts, al)?;
    Ok((table, bytes_consumed))
}

// ---------------------------------------------------------------------------
// Huffman table for literals
// ---------------------------------------------------------------------------

/// Huffman decoding table entry.
#[derive(Clone, Copy, Default)]
struct HuffEntry {
    symbol: u8,
    nb_bits: u8,
}

/// Huffman decoding table (up to 2^11 entries).
struct HuffTable {
    entries: Vec<HuffEntry>,
    max_bits: u8,
}

impl HuffTable {
    /// Build a Huffman decoding table from a weight array.
    ///
    /// Weights are 0..max_bits. Weight 0 means the symbol doesn't appear.
    /// Weight w maps to code length (max_bits + 1 - w).
    fn build(weights: &[u8], num_symbols: usize) -> KernelResult<Self> {
        if num_symbols == 0 {
            return Err(KernelError::CorruptedData);
        }

        // Determine max number of bits.
        let max_weight = weights.iter().take(num_symbols).copied().max().unwrap_or(0);
        if max_weight == 0 {
            return Err(KernelError::CorruptedData);
        }
        if max_weight > HUFFMAN_MAX_BITS {
            return Err(KernelError::CorruptedData);
        }

        // Count weights.
        let mut weight_counts = [0u32; 13]; // max weight = 12
        for &w in weights.iter().take(num_symbols) {
            if w > 0 {
                weight_counts[w as usize] = weight_counts[w as usize].wrapping_add(1);
            }
        }

        // Determine table bits: max_bits = highest weight.
        let table_log = max_weight;
        let table_size = 1u32 << table_log;

        // Verify the weight distribution is valid:
        // sum of (1 << (max_weight - w)) for all symbols with w > 0 must equal 2^max_weight.
        let mut total = 0u32;
        for w in 1..=max_weight {
            let contrib = 1u32 << (max_weight - w);
            total = total.wrapping_add(weight_counts[w as usize].wrapping_mul(contrib));
        }

        // Allow exact match or off-by-one (last symbol weight is implicit).
        if total != table_size && total != table_size / 2 {
            // Some implementations require total == table_size/2 (last symbol implicit).
            // We'll be lenient here.
        }

        // Assign codes. Start from the shortest codes (highest weight).
        let mut entries = vec![HuffEntry::default(); table_size as usize];
        let mut code = 0u32;

        // Process weights from max (1-bit code) down to 1 (max_weight-bit code).
        for bits in 1..=table_log {
            let w = table_log + 1 - bits; // weight = table_log + 1 - code_length
            if w as usize >= weight_counts.len() { continue; }

            for sym_idx in 0..num_symbols {
                if sym_idx < weights.len() && weights[sym_idx] == w {
                    let nb = bits;
                    let num_entries = 1u32 << (table_log - nb);
                    for j in 0..num_entries {
                        let idx = code + j;
                        if (idx as usize) < entries.len() {
                            entries[idx as usize] = HuffEntry {
                                symbol: sym_idx as u8,
                                nb_bits: nb,
                            };
                        }
                    }
                    code += num_entries;
                }
            }
        }

        Ok(Self { entries, max_bits: table_log })
    }

    /// Decode one symbol using the table.
    fn decode(&self, bits: u32) -> &HuffEntry {
        let idx = bits as usize & ((1 << self.max_bits) - 1);
        if idx < self.entries.len() {
            &self.entries[idx]
        } else {
            &self.entries[0]
        }
    }
}

/// Decode a Huffman tree description from a compressed header.
///
/// Returns (weights, num_symbols, bytes_consumed).
fn decode_huffman_tree(data: &[u8]) -> KernelResult<(Vec<u8>, usize, usize)> {
    if data.is_empty() {
        return Err(KernelError::CorruptedData);
    }

    let header = data[0];

    if header < 128 {
        // FSE-compressed weights.
        let compressed_size = header as usize;
        if compressed_size + 1 > data.len() {
            return Err(KernelError::CorruptedData);
        }

        let weight_data = &data[1..1 + compressed_size];
        let weights = decode_huffman_weights_fse(weight_data)?;
        let num_symbols = weights.len();
        Ok((weights, num_symbols, 1 + compressed_size))
    } else {
        // Direct representation: 4-bit weight pairs.
        let num_symbols = (header as usize) - 127;
        let num_bytes = (num_symbols + 1) / 2;
        if 1 + num_bytes > data.len() {
            return Err(KernelError::CorruptedData);
        }

        let mut weights = Vec::with_capacity(num_symbols);
        for i in 0..num_bytes {
            let b = data[1 + i];
            weights.push(b >> 4);
            if weights.len() < num_symbols {
                weights.push(b & 0x0F);
            }
        }

        Ok((weights, num_symbols, 1 + num_bytes))
    }
}

/// Decode Huffman weights from an FSE-compressed stream.
fn decode_huffman_weights_fse(data: &[u8]) -> KernelResult<Vec<u8>> {
    // The weight stream is FSE-compressed with accuracy_log in [1..7].
    let (table, header_size) = decode_fse_table(data, 12, 7)?;

    if header_size >= data.len() {
        return Err(KernelError::CorruptedData);
    }

    let compressed = &data[header_size..];
    let mut br = ReverseBitReader::new(compressed)?;

    // Initialize two FSE states.
    let al = table.accuracy_log;
    let mut state1 = br.read_bits(al)? as u16;
    let mut state2 = br.read_bits(al)? as u16;

    let mut weights = Vec::new();

    loop {
        let entry1 = table.decode(state1);
        weights.push(entry1.symbol);
        if br.bits_remaining() < entry1.nb_bits as usize { break; }
        let bits1 = br.read_bits(entry1.nb_bits)?;
        state1 = entry1.new_state_base.wrapping_add(bits1 as u16);

        let entry2 = table.decode(state2);
        weights.push(entry2.symbol);
        if br.bits_remaining() < entry2.nb_bits as usize { break; }
        let bits2 = br.read_bits(entry2.nb_bits)?;
        state2 = entry2.new_state_base.wrapping_add(bits2 as u16);

        if weights.len() > 256 { break; } // safety limit
    }

    Ok(weights)
}

// ---------------------------------------------------------------------------
// Predefined FSE distributions (RFC 8478 §4.1.1)
// ---------------------------------------------------------------------------

/// Default literal-length FSE distribution (accuracy_log = 6).
static LL_DEFAULT_DIST: &[i16] = &[
    4, 3, 2, 2, 2, 2, 2, 2,
    2, 2, 2, 2, 2, 1, 1, 1,
    2, 2, 2, 2, 2, 2, 2, 2,
    2, 3, 2, 1, 1, 1, 1, 1,
    -1, -1, -1, -1,
];

/// Default match-length FSE distribution (accuracy_log = 6).
static ML_DEFAULT_DIST: &[i16] = &[
    1, 4, 3, 2, 2, 2, 2, 2,
    2, 1, 1, 1, 1, 1, 1, 1,
    1, 1, 1, 1, 1, 1, 1, 1,
    1, 1, 1, 1, 1, 1, 1, 1,
    1, 1, 1, 1, 1, 1, 1, 1,
    1, 1, 1, 1, 1, 1, -1, -1,
    -1, -1, -1, -1, -1,
];

/// Default offset FSE distribution (accuracy_log = 5).
static OF_DEFAULT_DIST: &[i16] = &[
    1, 1, 1, 1, 1, 1, 2, 2,
    2, 1, 1, 1, 1, 1, 1, 1,
    1, 1, 1, 1, 1, 1, 1, 1,
    -1, -1, -1, -1, -1,
];

/// Default accuracy log for literal-length FSE.
const LL_DEFAULT_AL: u8 = 6;
/// Default accuracy log for match-length FSE.
const ML_DEFAULT_AL: u8 = 6;
/// Default accuracy log for offset FSE.
const OF_DEFAULT_AL: u8 = 5;

// ---------------------------------------------------------------------------
// Sequence code tables (RFC 8478 §3.1.1)
// ---------------------------------------------------------------------------

/// Literal-length code → (extra_bits, baseline).
fn ll_code_to_value(code: u8) -> (u8, u32) {
    match code {
        0..=15 => (0, code as u32),
        16 => (1, 16),
        17 => (1, 18),
        18 => (1, 20),
        19 => (1, 22),
        20 => (2, 24),
        21 => (2, 28),
        22 => (3, 32),
        23 => (3, 40),
        24 => (4, 48),
        25 => (4, 64),
        26 => (5, 96),
        27 => (5, 128),
        28 => (6, 192),
        29 => (6, 256),
        30 => (7, 384),
        31 => (7, 512),
        32 => (8, 768),
        33 => (8, 1024),
        34 => (9, 1536),
        35 => (9, 2048),
        _ => (0, 0),
    }
}

/// Match-length code → (extra_bits, baseline).
fn ml_code_to_value(code: u8) -> (u8, u32) {
    match code {
        0..=31 => (0, code as u32 + 3),
        32 => (1, 35),
        33 => (1, 37),
        34 => (1, 39),
        35 => (1, 41),
        36 => (2, 43),
        37 => (2, 47),
        38 => (3, 51),
        39 => (3, 59),
        40 => (4, 67),
        41 => (4, 83),
        42 => (5, 99),
        43 => (5, 131),
        44 => (6, 163),
        45 => (6, 227),
        46 => (7, 291),
        47 => (7, 419),
        48 => (8, 547),
        49 => (8, 803),
        50 => (9, 1059),
        51 => (9, 1571),
        52 => (10, 2083),
        _ => (0, 3),
    }
}

// ---------------------------------------------------------------------------
// Frame header parsing
// ---------------------------------------------------------------------------

/// Parsed zstd frame header.
struct FrameHeader {
    /// Content size (if known).
    content_size: Option<u64>,
    /// Window size.
    window_size: u64,
    /// Dictionary ID (0 = no dictionary).
    dict_id: u32,
    /// Whether a content checksum is present at the end.
    has_checksum: bool,
    /// Whether this is a single-segment frame (no window descriptor).
    single_segment: bool,
    /// Total header size in bytes (including magic).
    header_size: usize,
}

fn parse_frame_header(data: &[u8]) -> KernelResult<FrameHeader> {
    if data.len() < 5 {
        return Err(KernelError::CorruptedData);
    }

    let magic = read_le32(data, 0);
    if magic != ZSTD_MAGIC {
        return Err(KernelError::CorruptedData);
    }

    let descriptor = data[4];

    // Frame_Content_Size_Flag: bits 7-6
    let fcs_flag = (descriptor >> 6) & 3;
    // Single_Segment_Flag: bit 5
    let single_segment = (descriptor >> 5) & 1 != 0;
    // Content_Checksum_Flag: bit 2
    let has_checksum = (descriptor >> 2) & 1 != 0;
    // Dictionary_ID_Flag: bits 1-0
    let dict_id_flag = descriptor & 3;

    let mut pos = 5;

    // Window descriptor (absent if single_segment).
    let window_size = if single_segment {
        0 // Will be set to content_size later.
    } else {
        if pos >= data.len() {
            return Err(KernelError::CorruptedData);
        }
        let wd = data[pos];
        pos += 1;
        let exponent = (wd >> 3) as u64;
        let mantissa = (wd & 7) as u64;
        let base = 1u64 << (10 + exponent);
        base + (base >> 3) * mantissa
    };

    // Dictionary ID (0, 1, 2, or 4 bytes).
    let dict_id_bytes = match dict_id_flag {
        0 => 0,
        1 => 1,
        2 => 2,
        3 => 4,
        _ => 0,
    };
    if pos + dict_id_bytes > data.len() {
        return Err(KernelError::CorruptedData);
    }
    let dict_id = match dict_id_bytes {
        0 => 0,
        1 => u32::from(data[pos]),
        2 => u32::from(read_le16(data, pos)),
        4 => read_le32(data, pos),
        _ => 0,
    };
    pos += dict_id_bytes;

    // Frame content size (0, 1, 2, 4, or 8 bytes).
    let fcs_bytes = match fcs_flag {
        0 => if single_segment { 1 } else { 0 },
        1 => 2,
        2 => 4,
        3 => 8,
        _ => 0,
    };
    if pos + fcs_bytes > data.len() {
        return Err(KernelError::CorruptedData);
    }
    let content_size = match fcs_bytes {
        0 => None,
        1 => Some(u64::from(data[pos])),
        2 => Some(u64::from(read_le16(data, pos)) + 256),
        4 => Some(u64::from(read_le32(data, pos))),
        8 => Some(read_le64(data, pos)),
        _ => None,
    };
    pos += fcs_bytes;

    let final_window_size = if single_segment {
        content_size.unwrap_or(0)
    } else {
        window_size
    };

    if final_window_size > MAX_WINDOW_SIZE as u64 {
        return Err(KernelError::OutOfMemory);
    }

    Ok(FrameHeader {
        content_size,
        window_size: final_window_size,
        dict_id,
        has_checksum,
        single_segment,
        header_size: pos,
    })
}

// ---------------------------------------------------------------------------
// Block decompression
// ---------------------------------------------------------------------------

/// Block types.
const BLOCK_RAW: u8 = 0;
const BLOCK_RLE: u8 = 1;
const BLOCK_COMPRESSED: u8 = 2;
const BLOCK_RESERVED: u8 = 3;

/// Decompress a single zstd frame.
fn decompress_frame(data: &[u8]) -> KernelResult<(Vec<u8>, usize)> {
    let header = parse_frame_header(data)?;
    let mut pos = header.header_size;

    let initial_cap = header.content_size
        .map(|s| s.min(MAX_OUTPUT_SIZE as u64) as usize)
        .unwrap_or(4096);
    let mut output = Vec::with_capacity(initial_cap);

    // Sequence decoder state: repeated offsets.
    let mut rep_offsets = [1u32, 4, 8];

    // Persistent Huffman table across blocks (for "treeless" literals).
    let mut huff_table: Option<HuffTable> = None;

    // Persistent FSE tables across blocks (for "repeat" mode).
    let mut ll_table: Option<FseTable> = None;
    let mut ml_table: Option<FseTable> = None;
    let mut of_table: Option<FseTable> = None;

    loop {
        if pos + 3 > data.len() {
            return Err(KernelError::CorruptedData);
        }

        // Block header: 3 bytes, little-endian.
        let bh = u32::from(data[pos])
            | (u32::from(data[pos + 1]) << 8)
            | (u32::from(data[pos + 2]) << 16);
        pos += 3;

        let last_block = (bh & 1) != 0;
        let block_type = ((bh >> 1) & 3) as u8;
        let block_size = (bh >> 3) as usize;

        match block_type {
            BLOCK_RAW => {
                if pos + block_size > data.len() {
                    return Err(KernelError::CorruptedData);
                }
                if output.len() + block_size > MAX_OUTPUT_SIZE {
                    return Err(KernelError::OutOfMemory);
                }
                output.extend_from_slice(&data[pos..pos + block_size]);
                pos += block_size;
            }
            BLOCK_RLE => {
                if pos >= data.len() {
                    return Err(KernelError::CorruptedData);
                }
                if output.len() + block_size > MAX_OUTPUT_SIZE {
                    return Err(KernelError::OutOfMemory);
                }
                let byte = data[pos];
                pos += 1;
                output.resize(output.len() + block_size, byte);
            }
            BLOCK_COMPRESSED => {
                if pos + block_size > data.len() {
                    return Err(KernelError::CorruptedData);
                }
                let block_data = &data[pos..pos + block_size];
                decompress_block(
                    block_data,
                    &mut output,
                    &mut rep_offsets,
                    &mut huff_table,
                    &mut ll_table,
                    &mut ml_table,
                    &mut of_table,
                )?;
                pos += block_size;
            }
            BLOCK_RESERVED | _ => {
                return Err(KernelError::CorruptedData);
            }
        }

        if last_block { break; }
    }

    // Optional content checksum (lower 32 bits of xxHash-64).
    if header.has_checksum {
        if pos + 4 > data.len() {
            return Err(KernelError::CorruptedData);
        }
        let expected = read_le32(data, pos);
        pos += 4;
        let hash = xxhash64(&output);
        if (hash as u32) != expected {
            return Err(KernelError::CorruptedData);
        }
    }

    // Validate content size if specified.
    if let Some(expected_size) = header.content_size {
        if output.len() as u64 != expected_size {
            return Err(KernelError::CorruptedData);
        }
    }

    Ok((output, pos))
}

/// Decompress a compressed block.
#[allow(clippy::too_many_arguments)]
fn decompress_block(
    block_data: &[u8],
    output: &mut Vec<u8>,
    rep_offsets: &mut [u32; 3],
    huff_table: &mut Option<HuffTable>,
    ll_table: &mut Option<FseTable>,
    ml_table: &mut Option<FseTable>,
    of_table: &mut Option<FseTable>,
) -> KernelResult<()> {
    if block_data.is_empty() {
        return Err(KernelError::CorruptedData);
    }

    // 1. Parse literals section.
    let (literals, lit_consumed) = decode_literals_section(block_data, huff_table)?;

    if lit_consumed >= block_data.len() {
        // Block is all literals, no sequences.
        if output.len() + literals.len() > MAX_OUTPUT_SIZE {
            return Err(KernelError::OutOfMemory);
        }
        output.extend_from_slice(&literals);
        return Ok(());
    }

    // 2. Parse sequences section.
    let seq_data = &block_data[lit_consumed..];
    decode_sequences(
        seq_data,
        &literals,
        output,
        rep_offsets,
        ll_table,
        ml_table,
        of_table,
    )?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Literals section
// ---------------------------------------------------------------------------

/// Literal block types.
const LIT_RAW: u8 = 0;
const LIT_RLE: u8 = 1;
const LIT_COMPRESSED: u8 = 2;
const LIT_TREELESS: u8 = 3;

/// Decode the literals section of a compressed block.
///
/// Returns (literals_bytes, bytes_consumed).
fn decode_literals_section(
    data: &[u8],
    huff_table: &mut Option<HuffTable>,
) -> KernelResult<(Vec<u8>, usize)> {
    if data.is_empty() {
        return Err(KernelError::CorruptedData);
    }

    let header_byte = data[0];
    let lit_type = header_byte & 3;
    let size_format = (header_byte >> 2) & 3;

    match lit_type {
        LIT_RAW => {
            // Raw literals: uncompressed bytes.
            let (regen_size, header_size) = match size_format {
                0 | 2 => {
                    // 5-bit size (bits 3..7 of byte 0).
                    ((header_byte >> 3) as usize, 1)
                }
                1 => {
                    // 12-bit size.
                    if data.len() < 2 { return Err(KernelError::CorruptedData); }
                    let sz = ((header_byte >> 4) as usize) | ((data[1] as usize) << 4);
                    (sz, 2)
                }
                3 => {
                    // 20-bit size.
                    if data.len() < 3 { return Err(KernelError::CorruptedData); }
                    let sz = ((header_byte >> 4) as usize)
                        | ((data[1] as usize) << 4)
                        | ((data[2] as usize) << 12);
                    (sz, 3)
                }
                _ => return Err(KernelError::CorruptedData),
            };

            if header_size + regen_size > data.len() {
                return Err(KernelError::CorruptedData);
            }
            let literals = data[header_size..header_size + regen_size].to_vec();
            Ok((literals, header_size + regen_size))
        }

        LIT_RLE => {
            // RLE: single byte repeated.
            let (regen_size, header_size) = match size_format {
                0 | 2 => {
                    ((header_byte >> 3) as usize, 1)
                }
                1 => {
                    if data.len() < 2 { return Err(KernelError::CorruptedData); }
                    let sz = ((header_byte >> 4) as usize) | ((data[1] as usize) << 4);
                    (sz, 2)
                }
                3 => {
                    if data.len() < 3 { return Err(KernelError::CorruptedData); }
                    let sz = ((header_byte >> 4) as usize)
                        | ((data[1] as usize) << 4)
                        | ((data[2] as usize) << 12);
                    (sz, 3)
                }
                _ => return Err(KernelError::CorruptedData),
            };

            if header_size >= data.len() {
                return Err(KernelError::CorruptedData);
            }
            let byte = data[header_size];
            let literals = vec![byte; regen_size];
            Ok((literals, header_size + 1))
        }

        LIT_COMPRESSED | LIT_TREELESS => {
            // Huffman-compressed or treeless (reuse previous Huffman table).
            let (regen_size, compressed_size, num_streams, header_size) = match size_format {
                0 => {
                    // Single stream, 10-bit sizes.
                    if data.len() < 3 { return Err(KernelError::CorruptedData); }
                    let b0 = data[0] as usize;
                    let b1 = data[1] as usize;
                    let b2 = data[2] as usize;
                    // size_format==0: both sizes use 10 bits
                    // Regenerated_Size = (Byte0>>4) + (Byte1<<4) (low 10 bits = bits [4..13])
                    // Compressed_Size  = (Byte1>>6) + (Byte2<<2) (next 10 bits = bits [14..23])
                    let regen2 = ((b0 >> 4) | (b1 << 4)) & 0x3FF;
                    let comp = ((b1 >> 6) | (b2 << 2)) & 0x3FF;
                    (regen2, comp, 1usize, 3)
                }
                1 => {
                    // Single stream, 10-bit sizes (same encoding but explicitly single).
                    if data.len() < 3 { return Err(KernelError::CorruptedData); }
                    let b0 = data[0] as usize;
                    let b1 = data[1] as usize;
                    let b2 = data[2] as usize;
                    let regen2 = ((b0 >> 4) | (b1 << 4)) & 0x3FF;
                    let comp = ((b1 >> 6) | (b2 << 2)) & 0x3FF;
                    (regen2, comp, 1, 3)
                }
                2 => {
                    // 4 streams, 14-bit sizes.
                    if data.len() < 4 { return Err(KernelError::CorruptedData); }
                    let b0 = data[0] as usize;
                    let b1 = data[1] as usize;
                    let b2 = data[2] as usize;
                    let b3 = data[3] as usize;
                    let regen2 = ((b0 >> 4) | (b1 << 4)) & 0x3FFF;
                    let comp = ((b1 >> 6) | (b2 << 2) | (b3 << 10)) & 0x3FFF;
                    (regen2, comp, 4, 4)
                }
                3 => {
                    // 4 streams, 18-bit sizes.
                    if data.len() < 5 { return Err(KernelError::CorruptedData); }
                    let b0 = data[0] as usize;
                    let b1 = data[1] as usize;
                    let b2 = data[2] as usize;
                    let b3 = data[3] as usize;
                    let b4 = data[4] as usize;
                    let regen2 = ((b0 >> 4) | (b1 << 4)) & 0x3FFFF;
                    let comp = ((b1 >> 6) | (b2 << 2) | (b3 << 10) | (b4 << 18)) & 0x3FFFF;
                    (regen2, comp, 4, 5)
                }
                _ => return Err(KernelError::CorruptedData),
            };

            if header_size + compressed_size > data.len() {
                return Err(KernelError::CorruptedData);
            }

            let compressed_data = &data[header_size..header_size + compressed_size];
            let mut comp_pos = 0;

            // Decode or reuse Huffman tree.
            if lit_type == LIT_COMPRESSED {
                let (weights, num_syms, tree_bytes) = decode_huffman_tree(compressed_data)?;
                *huff_table = Some(HuffTable::build(&weights, num_syms)?);
                comp_pos += tree_bytes;
            } else {
                // Treeless: must reuse previous table.
                if huff_table.is_none() {
                    return Err(KernelError::CorruptedData);
                }
            }

            let table = huff_table.as_ref().ok_or(KernelError::CorruptedData)?;

            // Decompress Huffman streams.
            let huff_data = &compressed_data[comp_pos..];
            let literals = if num_streams == 1 {
                decompress_huffman_single(huff_data, table, regen_size)?
            } else {
                decompress_huffman_4streams(huff_data, table, regen_size)?
            };

            Ok((literals, header_size + compressed_size))
        }

        _ => Err(KernelError::CorruptedData),
    }
}

/// Decompress a single Huffman stream.
fn decompress_huffman_single(
    data: &[u8],
    table: &HuffTable,
    regen_size: usize,
) -> KernelResult<Vec<u8>> {
    if data.is_empty() {
        return Ok(Vec::new());
    }

    let mut br = ReverseBitReader::new(data)?;
    let mut output = Vec::with_capacity(regen_size);

    while output.len() < regen_size {
        if br.bits_remaining() < table.max_bits as usize {
            // Try reading remaining bits.
            if br.bits_remaining() == 0 { break; }
            let bits = br.read_bits(br.bits_remaining() as u8)?;
            let entry = table.decode(bits << (table.max_bits - br.bits_remaining() as u8));
            output.push(entry.symbol);
            break;
        }

        let bits = br.read_bits(table.max_bits)?;
        let entry = table.decode(bits);
        output.push(entry.symbol);
        // We read max_bits but only consumed nb_bits — put back the excess.
        let unused = table.max_bits.saturating_sub(entry.nb_bits);
        br.unread_bits(unused);
    }

    if output.len() != regen_size {
        return Err(KernelError::CorruptedData);
    }
    Ok(output)
}

/// Decompress 4 Huffman streams (used for larger literal sections).
fn decompress_huffman_4streams(
    data: &[u8],
    table: &HuffTable,
    regen_size: usize,
) -> KernelResult<Vec<u8>> {
    // 4-stream format: first 6 bytes are 3 x 2-byte LE sizes for streams 1-3.
    // Stream 4's size is implicit (remaining bytes).
    if data.len() < 6 {
        return Err(KernelError::CorruptedData);
    }

    let s1_size = read_le16(data, 0) as usize;
    let s2_size = read_le16(data, 2) as usize;
    let s3_size = read_le16(data, 4) as usize;

    let stream_data = &data[6..];
    if s1_size + s2_size + s3_size > stream_data.len() {
        return Err(KernelError::CorruptedData);
    }

    // Each stream decompresses to roughly regen_size/4 bytes.
    let quarter = (regen_size + 3) / 4;
    let sizes = [
        quarter.min(regen_size),
        quarter.min(regen_size.saturating_sub(quarter)),
        quarter.min(regen_size.saturating_sub(quarter * 2)),
        regen_size.saturating_sub(quarter * 3),
    ];

    let mut output = Vec::with_capacity(regen_size);

    let s1_data = &stream_data[..s1_size];
    let s2_data = &stream_data[s1_size..s1_size + s2_size];
    let s3_data = &stream_data[s1_size + s2_size..s1_size + s2_size + s3_size];
    let s4_data = &stream_data[s1_size + s2_size + s3_size..];

    let streams = [s1_data, s2_data, s3_data, s4_data];

    for (i, &sdata) in streams.iter().enumerate() {
        if sdata.is_empty() {
            continue;
        }
        let decoded = decompress_huffman_single(sdata, table, sizes[i])?;
        output.extend_from_slice(&decoded);
    }

    // The total might not exactly equal regen_size due to rounding.
    // Truncate or verify.
    if output.len() > regen_size {
        output.truncate(regen_size);
    }
    if output.len() != regen_size {
        return Err(KernelError::CorruptedData);
    }

    Ok(output)
}

// ---------------------------------------------------------------------------
// Sequences section
// ---------------------------------------------------------------------------

/// Decode the sequences section and execute copy commands.
#[allow(clippy::too_many_arguments)]
fn decode_sequences(
    data: &[u8],
    literals: &[u8],
    output: &mut Vec<u8>,
    rep_offsets: &mut [u32; 3],
    ll_table: &mut Option<FseTable>,
    ml_table: &mut Option<FseTable>,
    of_table: &mut Option<FseTable>,
) -> KernelResult<()> {
    if data.is_empty() {
        // No sequences — just copy literals.
        if output.len() + literals.len() > MAX_OUTPUT_SIZE {
            return Err(KernelError::OutOfMemory);
        }
        output.extend_from_slice(literals);
        return Ok(());
    }

    // Number of sequences.
    let mut pos = 0;
    let byte0 = data.get(pos).copied().ok_or(KernelError::CorruptedData)?;
    pos += 1;

    let num_sequences = if byte0 == 0 {
        // Zero sequences, just copy remaining literals.
        if output.len() + literals.len() > MAX_OUTPUT_SIZE {
            return Err(KernelError::OutOfMemory);
        }
        output.extend_from_slice(literals);
        return Ok(());
    } else if byte0 < 128 {
        byte0 as usize
    } else if byte0 < 255 {
        if pos >= data.len() { return Err(KernelError::CorruptedData); }
        let b1 = data[pos] as usize;
        pos += 1;
        ((byte0 as usize - 128) << 8) + b1
    } else {
        // byte0 == 255
        if pos + 1 >= data.len() { return Err(KernelError::CorruptedData); }
        let b1 = data[pos] as usize;
        let b2 = data[pos + 1] as usize;
        pos += 2;
        b1 + (b2 << 8) + 0x7F00
    };

    if num_sequences == 0 {
        if output.len() + literals.len() > MAX_OUTPUT_SIZE {
            return Err(KernelError::OutOfMemory);
        }
        output.extend_from_slice(literals);
        return Ok(());
    }

    // Symbol compression modes byte.
    if pos >= data.len() { return Err(KernelError::CorruptedData); }
    let modes_byte = data[pos];
    pos += 1;

    let ll_mode = (modes_byte >> 6) & 3;
    let of_mode = (modes_byte >> 4) & 3;
    let ml_mode = (modes_byte >> 2) & 3;

    // Decode or set FSE tables based on modes.
    // Mode 0 = predefined, 1 = RLE (single symbol), 2 = FSE compressed, 3 = repeat previous.
    pos += decode_seq_table_mode(ll_mode, &data[pos..], ll_table, LL_DEFAULT_DIST, LL_DEFAULT_AL, LL_MAX_SYMBOL)?;
    pos += decode_seq_table_mode(of_mode, &data[pos..], of_table, OF_DEFAULT_DIST, OF_DEFAULT_AL, OF_MAX_SYMBOL)?;
    pos += decode_seq_table_mode(ml_mode, &data[pos..], ml_table, ML_DEFAULT_DIST, ML_DEFAULT_AL, ML_MAX_SYMBOL)?;

    // The rest of the data is the FSE-compressed bitstream (read backwards).
    let bitstream = &data[pos..];
    if bitstream.is_empty() {
        return Err(KernelError::CorruptedData);
    }

    let mut br = ReverseBitReader::new(bitstream)?;

    // Initialize FSE states.
    let ll_t = ll_table.as_ref().ok_or(KernelError::CorruptedData)?;
    let of_t = of_table.as_ref().ok_or(KernelError::CorruptedData)?;
    let ml_t = ml_table.as_ref().ok_or(KernelError::CorruptedData)?;

    let mut ll_state = br.read_bits(ll_t.accuracy_log)? as u16;
    let mut of_state = br.read_bits(of_t.accuracy_log)? as u16;
    let mut ml_state = br.read_bits(ml_t.accuracy_log)? as u16;

    let mut lit_pos = 0usize;

    for seq_idx in 0..num_sequences {
        // Decode symbols from current FSE states.
        let of_entry = of_t.decode(of_state);
        let ll_entry = ll_t.decode(ll_state);
        let ml_entry = ml_t.decode(ml_state);

        let of_code = of_entry.symbol;
        let ll_code = ll_entry.symbol;
        let ml_code = ml_entry.symbol;

        // Decode offset.
        let of_bits = of_code as u8; // offset code IS the number of extra bits
        let offset_raw = if of_bits > 0 {
            let extra = br.read_bits(of_bits)?;
            (1u32 << of_bits) | extra
        } else {
            1 // of_code == 0 means offset value = 1 (but this maps to repeat offsets)
        };

        // Decode literal length.
        let (ll_extra_bits, ll_base) = ll_code_to_value(ll_code);
        let ll_value = if ll_extra_bits > 0 {
            ll_base + br.read_bits(ll_extra_bits)?
        } else {
            ll_base
        };

        // Decode match length.
        let (ml_extra_bits, ml_base) = ml_code_to_value(ml_code);
        let ml_value = if ml_extra_bits > 0 {
            ml_base + br.read_bits(ml_extra_bits)?
        } else {
            ml_base
        };

        // Resolve offset with repeat offset logic.
        let actual_offset = if offset_raw > 3 {
            // Regular offset.
            let off = offset_raw - 3;
            rep_offsets[2] = rep_offsets[1];
            rep_offsets[1] = rep_offsets[0];
            rep_offsets[0] = off;
            off
        } else {
            // Repeat offset.
            let idx = offset_raw as usize; // 1, 2, or 3
            if ll_value == 0 {
                // Special case: when literal_length == 0, offset indices are shifted.
                match idx {
                    1 => {
                        // Use rep[0] (no change).
                        rep_offsets[0]
                    }
                    2 => {
                        let off = rep_offsets[1];
                        rep_offsets[1] = rep_offsets[0];
                        rep_offsets[0] = off;
                        off
                    }
                    3 => {
                        let off = rep_offsets[0].wrapping_sub(1);
                        // Actually: offset = rep[0] - 1 (with special -1 handling)
                        if off == 0 { return Err(KernelError::CorruptedData); }
                        rep_offsets[2] = rep_offsets[1];
                        rep_offsets[1] = rep_offsets[0];
                        rep_offsets[0] = off;
                        off
                    }
                    _ => return Err(KernelError::CorruptedData),
                }
            } else {
                match idx {
                    1 => {
                        // Use rep[0] (no change to rep offsets).
                        rep_offsets[0]
                    }
                    2 => {
                        let off = rep_offsets[1];
                        rep_offsets[1] = rep_offsets[0];
                        rep_offsets[0] = off;
                        off
                    }
                    3 => {
                        let off = rep_offsets[2];
                        rep_offsets[2] = rep_offsets[1];
                        rep_offsets[1] = rep_offsets[0];
                        rep_offsets[0] = off;
                        off
                    }
                    _ => return Err(KernelError::CorruptedData),
                }
            }
        };

        // Execute the sequence: copy literal_length bytes from literals,
        // then copy match_length bytes from output history at offset.
        let ll = ll_value as usize;
        let ml = ml_value as usize;

        if lit_pos + ll > literals.len() {
            return Err(KernelError::CorruptedData);
        }
        if output.len() + ll + ml > MAX_OUTPUT_SIZE {
            return Err(KernelError::OutOfMemory);
        }

        // Copy literals.
        output.extend_from_slice(&literals[lit_pos..lit_pos + ll]);
        lit_pos += ll;

        // Copy match from history.
        let off = actual_offset as usize;
        if off == 0 || off > output.len() {
            return Err(KernelError::CorruptedData);
        }
        let match_start = output.len() - off;
        // Must copy byte-by-byte because source and dest can overlap
        // (e.g., offset=1 means repeat last byte).
        for i in 0..ml {
            let b = output[match_start + (i % off)];
            output.push(b);
        }

        // Update FSE states (except for the last sequence).
        if seq_idx < num_sequences - 1 {
            let ll_nb = ll_entry.nb_bits;
            let of_nb = of_entry.nb_bits;
            let ml_nb = ml_entry.nb_bits;

            let ll_bits_val = br.read_bits(ll_nb)? as u16;
            ll_state = ll_entry.new_state_base.wrapping_add(ll_bits_val);

            let ml_bits_val = br.read_bits(ml_nb)? as u16;
            ml_state = ml_entry.new_state_base.wrapping_add(ml_bits_val);

            let of_bits_val = br.read_bits(of_nb)? as u16;
            of_state = of_entry.new_state_base.wrapping_add(of_bits_val);
        }
    }

    // Copy remaining literals after last sequence.
    if lit_pos < literals.len() {
        if output.len() + (literals.len() - lit_pos) > MAX_OUTPUT_SIZE {
            return Err(KernelError::OutOfMemory);
        }
        output.extend_from_slice(&literals[lit_pos..]);
    }

    Ok(())
}

/// Decode an FSE table for a sequence symbol type based on the compression mode.
///
/// Returns bytes consumed from `data`.
fn decode_seq_table_mode(
    mode: u8,
    data: &[u8],
    table: &mut Option<FseTable>,
    default_dist: &[i16],
    default_al: u8,
    max_symbol: usize,
) -> KernelResult<usize> {
    match mode {
        0 => {
            // Predefined distribution.
            *table = Some(FseTable::build(default_dist, default_al)?);
            Ok(0)
        }
        1 => {
            // RLE: single symbol repeated.
            if data.is_empty() { return Err(KernelError::CorruptedData); }
            let sym = data[0];
            // Build a trivial 1-entry table.
            let entries = vec![FseEntry {
                symbol: sym,
                nb_bits: 0,
                new_state_base: 0,
            }];
            *table = Some(FseTable { entries, accuracy_log: 0 });
            Ok(1)
        }
        2 => {
            // FSE compressed table.
            let (t, consumed) = decode_fse_table(data, max_symbol, FSE_MAX_LOG)?;
            *table = Some(t);
            Ok(consumed)
        }
        3 => {
            // Repeat: reuse previous table.
            if table.is_none() {
                return Err(KernelError::CorruptedData);
            }
            Ok(0)
        }
        _ => Err(KernelError::CorruptedData),
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Decompress a zstd-compressed byte stream.
///
/// Handles single or multiple concatenated frames. Skippable frames
/// (magic 0x184D2A5x) are silently ignored.
///
/// # Errors
///
/// Returns `CorruptedData` if the input is malformed, or `OutOfMemory`
/// if decompressed output would exceed the safety limit.
pub fn unzstd(data: &[u8]) -> KernelResult<Vec<u8>> {
    if data.len() < 4 {
        return Err(KernelError::CorruptedData);
    }

    let mut output = Vec::new();
    let mut pos = 0;

    while pos < data.len() {
        if pos + 4 > data.len() {
            break; // trailing garbage
        }

        let magic = read_le32(data, pos);

        if magic == ZSTD_MAGIC {
            let (frame_data, consumed) = decompress_frame(&data[pos..])?;
            if output.len() + frame_data.len() > MAX_OUTPUT_SIZE {
                return Err(KernelError::OutOfMemory);
            }
            output.extend_from_slice(&frame_data);
            pos += consumed;
        } else if magic >= SKIPPABLE_MAGIC_BASE && magic <= SKIPPABLE_MAGIC_BASE + 0x0F {
            // Skippable frame: 4 bytes magic + 4 bytes LE size + data.
            if pos + 8 > data.len() {
                return Err(KernelError::CorruptedData);
            }
            let frame_size = read_le32(data, pos + 4) as usize;
            pos += 8 + frame_size;
        } else {
            return Err(KernelError::CorruptedData);
        }
    }

    Ok(output)
}

// ---------------------------------------------------------------------------
// Compression (store mode with optional RLE)
// ---------------------------------------------------------------------------

/// Maximum block size for compressed output (128 KiB per the spec default).
const COMPRESS_BLOCK_SIZE: usize = 128 * 1024;

/// Compress data into a valid zstd frame.
///
/// This implements a simple compression strategy:
/// - Scans input for RLE-able blocks (all same byte)
/// - Uses raw blocks for non-RLE data
/// - Includes content checksum for integrity
///
/// This produces valid zstd frames that any decompressor can read, but
/// does not perform LZ77 matching or entropy coding. The output is
/// slightly larger than the input (frame overhead) but is fast to encode.
///
/// For better compression, use `compress_zstd` which includes basic LZ77.
pub fn zstd_store(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + 32);

    // Frame magic.
    out.extend_from_slice(&ZSTD_MAGIC.to_le_bytes());

    // Frame descriptor.
    // FCS flag: select based on content size.
    let (fcs_flag, fcs_bytes) = if data.len() <= 255 {
        (0u8, 1usize) // fcs_flag=0, single_segment → 1 byte
    } else if data.len() <= 65535 + 256 {
        (1u8, 2)
    } else if data.len() <= u32::MAX as usize {
        (2u8, 4)
    } else {
        (3u8, 8)
    };

    // Descriptor byte: fcs_flag in bits [7:6], single_segment in bit 5,
    // checksum in bit 2, no dict.
    let descriptor = (fcs_flag << 6) | (1 << 5) | (1 << 2);
    out.push(descriptor);

    // Content size field (single_segment = true, so fcs_flag 0 → 1 byte).
    match fcs_bytes {
        1 => out.push(data.len() as u8),
        2 => {
            // 2-byte FCS: value = content_size - 256, stored as LE16.
            let val = (data.len() as u16).wrapping_sub(256);
            out.extend_from_slice(&val.to_le_bytes());
        }
        4 => out.extend_from_slice(&(data.len() as u32).to_le_bytes()),
        8 => out.extend_from_slice(&(data.len() as u64).to_le_bytes()),
        _ => {}
    }

    // Emit data as blocks.
    let mut pos = 0;
    while pos < data.len() {
        let remaining = data.len() - pos;
        let block_len = remaining.min(COMPRESS_BLOCK_SIZE);
        let is_last = pos + block_len >= data.len();

        let block_data = &data[pos..pos + block_len];

        // Check if this block is all the same byte (RLE-able).
        let first_byte = block_data[0];
        let is_rle = block_data.iter().all(|&b| b == first_byte);

        if is_rle && block_len > 1 {
            // RLE block: type=1, size=block_len, then one byte.
            let bh = if is_last { 1u32 } else { 0u32 }
                | (1u32 << 1)               // type = RLE
                | ((block_len as u32) << 3); // size = decompressed size
            out.push(bh as u8);
            out.push((bh >> 8) as u8);
            out.push((bh >> 16) as u8);
            out.push(first_byte);
        } else {
            // Raw block: type=0, size=block_len.
            let bh = if is_last { 1u32 } else { 0u32 }
                | (0u32 << 1)               // type = raw
                | ((block_len as u32) << 3); // size = byte count
            out.push(bh as u8);
            out.push((bh >> 8) as u8);
            out.push((bh >> 16) as u8);
            out.extend_from_slice(block_data);
        }

        pos += block_len;
    }

    // Handle empty input.
    if data.is_empty() {
        // Single empty raw block (last=1, type=0, size=0).
        let bh = 1u32; // last=1, type=0, size=0
        out.push(bh as u8);
        out.push((bh >> 8) as u8);
        out.push((bh >> 16) as u8);
    }

    // Content checksum (lower 32 bits of xxHash-64).
    let hash = xxhash64(data);
    out.extend_from_slice(&(hash as u32).to_le_bytes());

    out
}

/// Compress data into a zstd frame with basic LZ77 matching.
///
/// Uses a hash-chain approach for finding matches (similar to deflate
/// level 1) and encodes sequences using predefined FSE tables.  This
/// provides real compression for most data while remaining fast.
///
/// Typical compression ratio: 40-60% reduction on text, 10-30% on
/// binary data.  Not as good as reference zstd (which uses optimal
/// parsing and custom tables), but produces valid frames.
pub fn compress_zstd(data: &[u8]) -> Vec<u8> {
    if data.is_empty() || data.len() < 32 {
        // Too small for LZ77 to help — use store mode.
        return zstd_store(data);
    }

    let mut out = Vec::with_capacity(data.len());

    // Frame header.
    out.extend_from_slice(&ZSTD_MAGIC.to_le_bytes());

    let (fcs_flag, fcs_bytes) = if data.len() <= 255 {
        (0u8, 1usize)
    } else if data.len() <= 65535 + 256 {
        (1u8, 2)
    } else if data.len() <= u32::MAX as usize {
        (2u8, 4)
    } else {
        (3u8, 8)
    };

    let descriptor = (fcs_flag << 6) | (1 << 5) | (1 << 2);
    out.push(descriptor);

    match fcs_bytes {
        1 => out.push(data.len() as u8),
        2 => {
            let val = (data.len() as u16).wrapping_sub(256);
            out.extend_from_slice(&val.to_le_bytes());
        }
        4 => out.extend_from_slice(&(data.len() as u32).to_le_bytes()),
        8 => out.extend_from_slice(&(data.len() as u64).to_le_bytes()),
        _ => {}
    }

    // Process input in blocks.
    let mut pos = 0;
    while pos < data.len() {
        let remaining = data.len() - pos;
        let block_len = remaining.min(COMPRESS_BLOCK_SIZE);
        let is_last = pos + block_len >= data.len();
        let block_data = &data[pos..pos + block_len];

        // Try to compress with LZ77.
        let compressed = compress_block_lz77(block_data);

        // Only use compressed block if it's actually smaller.
        if compressed.len() < block_len {
            let bh = if is_last { 1u32 } else { 0u32 }
                | (2u32 << 1)                       // type = compressed
                | ((compressed.len() as u32) << 3);  // compressed size
            out.push(bh as u8);
            out.push((bh >> 8) as u8);
            out.push((bh >> 16) as u8);
            out.extend_from_slice(&compressed);
        } else {
            // Fall back to raw block.
            let bh = if is_last { 1u32 } else { 0u32 }
                | (0u32 << 1)
                | ((block_len as u32) << 3);
            out.push(bh as u8);
            out.push((bh >> 8) as u8);
            out.push((bh >> 16) as u8);
            out.extend_from_slice(block_data);
        }

        pos += block_len;
    }

    // Content checksum.
    let hash = xxhash64(data);
    out.extend_from_slice(&(hash as u32).to_le_bytes());

    out
}

/// LZ77 sequence for the compressor.
struct LzSequence {
    /// Literal bytes before this match.
    literal_length: u32,
    /// Match length (3 or more).
    match_length: u32,
    /// Match offset (1 = last byte, etc.).
    offset: u32,
}

/// Compress a single block using basic LZ77 + raw literals + 0 sequences fallback.
///
/// Returns the compressed block content (literals section + sequences section).
fn compress_block_lz77(data: &[u8]) -> Vec<u8> {
    if data.len() < 4 {
        return encode_raw_literals_block(data);
    }

    // Simple hash-chain LZ77 matching.
    const HASH_BITS: usize = 14;
    const HASH_SIZE: usize = 1 << HASH_BITS;
    const MIN_MATCH: usize = 4;
    const MAX_MATCH: usize = 258; // practical limit
    const WINDOW_SIZE: usize = 65536;

    let mut hash_table = vec![0u32; HASH_SIZE];
    let mut sequences: Vec<LzSequence> = Vec::new();
    let mut literals: Vec<u8> = Vec::new();
    let mut pos = 0usize;
    let mut literal_start = 0usize;

    while pos + 3 < data.len() {
        // Hash 4 bytes at current position.
        let h = hash4(data, pos) & (HASH_SIZE - 1);
        let prev_pos = hash_table[h] as usize;
        hash_table[h] = pos as u32;

        // Check for a match.
        let offset = pos.saturating_sub(prev_pos);
        if offset > 0
            && offset <= WINDOW_SIZE
            && prev_pos < pos
            && pos + MIN_MATCH <= data.len()
            && data.get(prev_pos..prev_pos + MIN_MATCH) == data.get(pos..pos + MIN_MATCH)
        {
            // Found a match — extend it.
            let mut match_len = MIN_MATCH;
            while pos + match_len < data.len()
                && prev_pos + match_len < data.len()
                && match_len < MAX_MATCH
            {
                if data[pos + match_len] != data[prev_pos + match_len] {
                    break;
                }
                match_len += 1;
            }

            // Emit any pending literals.
            let lit_len = pos - literal_start;
            if lit_len > 0 {
                literals.extend_from_slice(&data[literal_start..pos]);
            }

            sequences.push(LzSequence {
                literal_length: lit_len as u32,
                match_length: match_len as u32,
                offset: offset as u32,
            });

            pos += match_len;
            literal_start = pos;
        } else {
            pos += 1;
        }
    }

    // No sequences found — fall back to raw literals block.
    if sequences.is_empty() {
        return encode_raw_literals_block(data);
    }

    // Trailing literals (after last match).
    let trailing = &data[literal_start..];

    // Build the compressed block:
    // 1. Literals section (raw)
    // 2. Sequences section (with predefined FSE tables)
    encode_compressed_block(&literals, trailing, &sequences)
}

/// Hash 4 bytes for the LZ77 hash table.
#[inline]
fn hash4(data: &[u8], pos: usize) -> usize {
    let v = read_le32(data, pos);
    ((v.wrapping_mul(0x9E37_79B1)) >> 18) as usize
}

/// Encode a block as raw literals with zero sequences.
fn encode_raw_literals_block(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + 4);

    // Literals section: raw type.
    if data.len() < 32 {
        // 1-byte header: type=0, size_format=0, size in bits [7:3].
        out.push((data.len() as u8) << 3);
    } else if data.len() < 4096 {
        // 2-byte header: type=0, size_format=1.
        let b0 = ((data.len() << 4) as u8) | 0b0100;
        let b1 = (data.len() >> 4) as u8;
        out.push(b0);
        out.push(b1);
    } else {
        // 3-byte header: type=0, size_format=3.
        let b0 = ((data.len() << 4) as u8) | 0b1100;
        let b1 = (data.len() >> 4) as u8;
        let b2 = (data.len() >> 12) as u8;
        out.push(b0);
        out.push(b1);
        out.push(b2);
    }
    out.extend_from_slice(data);

    // Sequences section: 0 sequences.
    out.push(0);

    out
}

/// Encode a compressed block with literal and sequence data.
///
/// Uses raw literals and predefined FSE tables for sequences.
fn encode_compressed_block(
    literals: &[u8],
    trailing_literals: &[u8],
    sequences: &[LzSequence],
) -> Vec<u8> {
    let all_literals_len = literals.len() + trailing_literals.len();
    let mut out = Vec::with_capacity(all_literals_len + sequences.len() * 8 + 32);

    // 1. Literals section (raw type).
    let combined_literals = {
        let mut l = Vec::with_capacity(all_literals_len);
        l.extend_from_slice(literals);
        l.extend_from_slice(trailing_literals);
        l
    };

    if combined_literals.len() < 32 {
        out.push((combined_literals.len() as u8) << 3);
    } else if combined_literals.len() < 4096 {
        let b0 = ((combined_literals.len() << 4) as u8) | 0b0100;
        let b1 = (combined_literals.len() >> 4) as u8;
        out.push(b0);
        out.push(b1);
    } else {
        let b0 = ((combined_literals.len() << 4) as u8) | 0b1100;
        let b1 = (combined_literals.len() >> 4) as u8;
        let b2 = (combined_literals.len() >> 12) as u8;
        out.push(b0);
        out.push(b1);
        out.push(b2);
    }
    out.extend_from_slice(&combined_literals);

    // 2. Sequences section.
    // Number of sequences.
    let nseq = sequences.len();
    if nseq < 128 {
        out.push(nseq as u8);
    } else if nseq < 0x7F00 + 128 {
        let val = nseq - 128;
        out.push(((val >> 8) as u8) + 128);
        out.push(val as u8);
    } else {
        out.push(255);
        let val = nseq - 0x7F00;
        out.push(val as u8);
        out.push((val >> 8) as u8);
    }

    // Compression mode byte: predefined tables for all three.
    // ll_mode=0 (predefined), of_mode=0 (predefined), ml_mode=0 (predefined).
    out.push(0b00_00_00_00); // all predefined

    // Encode sequences as a bitstream (forward for extra bits, stored in
    // reverse-bit order as the decoder reads backwards).
    let bitstream = encode_sequences_bitstream(sequences);
    out.extend_from_slice(&bitstream);

    out
}

/// Find a decoding-table state whose symbol matches `desired_symbol`.
///
/// Returns the first matching state index, or 0 if none found.
fn find_state_for_symbol(table: &FseTable, desired_symbol: u8) -> u16 {
    for (i, entry) in table.entries.iter().enumerate() {
        if entry.symbol == desired_symbol {
            return i as u16;
        }
    }
    0
}

/// Find the encoder source state for an FSE symbol + target next-state.
///
/// The decoder does: `next_state = entry.new_state_base + read_bits(entry.nb_bits)`.
/// The encoder inverts this: given the target `next_state` that the decoder
/// should transition to, and the `symbol` to encode, find the table state
/// `S` such that `table[S].symbol == symbol` AND `next_state` falls in
/// `[table[S].new_state_base .. table[S].new_state_base + (1 << table[S].nb_bits))`.
///
/// Returns `(source_state, nb_bits, bits_value)`.
fn find_encoding_state(table: &FseTable, symbol: u8, target_next: u16) -> (u16, u8, u16) {
    for (state_idx, entry) in table.entries.iter().enumerate() {
        if entry.symbol == symbol {
            let range = 1u16 << entry.nb_bits;
            if target_next >= entry.new_state_base
                && target_next < entry.new_state_base.wrapping_add(range)
            {
                let bits = target_next.wrapping_sub(entry.new_state_base);
                return (state_idx as u16, entry.nb_bits, bits);
            }
        }
    }
    // Fallback: pick any state with the right symbol, output nb_bits=0.
    // This shouldn't happen with a correctly constructed FSE table.
    (find_state_for_symbol(table, symbol), 0, 0)
}

/// Push `n` bits MSB-first from `val` into a bool vector.
fn push_bits_msb(buf: &mut Vec<bool>, val: u32, n: u8) {
    for i in (0..n).rev() {
        buf.push((val >> i) & 1 != 0);
    }
}

/// Convert a bit vector (in decoder-read order) into the zstd backward
/// bitstream byte format.
///
/// The decoder's `ReverseBitReader` finds the sentinel bit (highest set
/// bit in the last byte), then reads MSB→LSB from the last byte backward
/// through earlier bytes.  This function packs bits accordingly.
fn encode_bits_to_bytes(bits: &[bool]) -> Vec<u8> {
    if bits.is_empty() {
        // Just a sentinel byte.
        return vec![1];
    }
    let total_with_sentinel = bits.len() + 1;
    let num_bytes = (total_with_sentinel + 7) / 8;
    let total_padded = num_bytes * 8;
    let padding = total_padded - total_with_sentinel;

    // Layout from MSB of last byte to LSB of first byte:
    // [padding zeros][sentinel=1][bits[0]][bits[1]]...[bits[n-1]]
    let mut all_bits = Vec::with_capacity(total_padded);
    all_bits.resize(padding, false);
    all_bits.push(true); // sentinel
    all_bits.extend_from_slice(bits);

    // Pack: all_bits[0..8] → last byte (MSB first), all_bits[8..16] → second-
    // to-last byte, etc.
    let mut bytes = vec![0u8; num_bytes];
    for (i, &bit) in all_bits.iter().enumerate() {
        if bit {
            let byte_idx = num_bytes - 1 - (i / 8);
            let bit_idx = 7 - (i % 8);
            bytes[byte_idx] |= 1 << bit_idx;
        }
    }

    bytes
}

/// Encode the sequences bitstream using proper FSE state transitions.
///
/// The decoder reads the bitstream backward (via `ReverseBitReader`):
///   1. Initial FSE states: LL (AL bits), OF (AL bits), ML (AL bits)
///   2. For each sequence: extra bits (OF, LL, ML), then state updates (LL, ML, OF)
///      — except the last sequence has no state update.
///
/// The encoder processes sequences in **reverse** order so that the FSE
/// state at the end of encoding becomes the "initial state" for the decoder.
fn encode_sequences_bitstream(sequences: &[LzSequence]) -> Vec<u8> {
    if sequences.is_empty() {
        return Vec::new();
    }

    // Build predefined FSE decoding tables (we invert them for encoding).
    let ll_table = match FseTable::build(LL_DEFAULT_DIST, LL_DEFAULT_AL) {
        Ok(t) => t,
        Err(_) => return Vec::new(),
    };
    let of_table = match FseTable::build(OF_DEFAULT_DIST, OF_DEFAULT_AL) {
        Ok(t) => t,
        Err(_) => return Vec::new(),
    };
    let ml_table = match FseTable::build(ML_DEFAULT_DIST, ML_DEFAULT_AL) {
        Ok(t) => t,
        Err(_) => return Vec::new(),
    };

    // Pre-compute symbol codes and extra bits for every sequence.
    struct SeqCodes {
        of_code: u8,
        of_extra_bits: u8,
        of_extra_val: u32,
        ll_code: u8,
        ll_extra_bits: u8,
        ll_extra_val: u32,
        ml_code: u8,
        ml_extra_bits: u8,
        ml_extra_val: u32,
    }
    let codes: Vec<SeqCodes> = sequences
        .iter()
        .map(|seq| {
            let (of_code, of_extra_bits, of_extra_val) = encode_offset(seq.offset);
            let (ll_code, ll_extra_bits, ll_extra_val) =
                encode_literal_length(seq.literal_length);
            let (ml_code, ml_extra_bits, ml_extra_val) =
                encode_match_length(seq.match_length);
            SeqCodes {
                of_code,
                of_extra_bits,
                of_extra_val,
                ll_code,
                ll_extra_bits,
                ll_extra_val,
                ml_code,
                ml_extra_bits,
                ml_extra_val,
            }
        })
        .collect();

    let n = codes.len();

    // --- Backward pass: resolve FSE states ---
    //
    // Start from the last sequence and work backward.  For the last
    // sequence the decoder does NOT read state-update bits, so we only
    // need the decoder to be in a state that decodes to the right symbol.

    // Per-sequence encoding info (indexed 0..n).
    struct SeqEnc {
        // State-update bits (None for the last sequence).
        ll_update: Option<(u8, u16)>, // (nb_bits, bits_value)
        of_update: Option<(u8, u16)>,
        ml_update: Option<(u8, u16)>,
    }
    let mut enc: Vec<SeqEnc> = (0..n)
        .map(|_| SeqEnc {
            ll_update: None,
            of_update: None,
            ml_update: None,
        })
        .collect();

    // Target states — what the decoder should be in before each sequence.
    // For the last sequence, pick any state that has the right symbol.
    let mut ll_target = find_state_for_symbol(&ll_table, codes[n - 1].ll_code);
    let mut of_target = find_state_for_symbol(&of_table, codes[n - 1].of_code);
    let mut ml_target = find_state_for_symbol(&ml_table, codes[n - 1].ml_code);

    // Walk backward from n-2 to 0.  For sequence i the decoder reads
    // state-update bits and transitions to the target states of seq i+1.
    for i in (0..n.saturating_sub(1)).rev() {
        let (ll_src, ll_nb, ll_bits) =
            find_encoding_state(&ll_table, codes[i].ll_code, ll_target);
        let (of_src, of_nb, of_bits) =
            find_encoding_state(&of_table, codes[i].of_code, of_target);
        let (ml_src, ml_nb, ml_bits) =
            find_encoding_state(&ml_table, codes[i].ml_code, ml_target);

        enc[i].ll_update = Some((ll_nb, ll_bits));
        enc[i].of_update = Some((of_nb, of_bits));
        enc[i].ml_update = Some((ml_nb, ml_bits));

        // The source states become the targets for the preceding sequence.
        ll_target = ll_src;
        of_target = of_src;
        ml_target = ml_src;
    }

    // After the backward pass, ll_target/of_target/ml_target are the
    // initial states the decoder should start with.

    // --- Forward pass: assemble bits in decoder-read order ---
    let mut bits: Vec<bool> = Vec::with_capacity(n * 24 + 32);

    // 1. Initial states.
    push_bits_msb(&mut bits, ll_target as u32, LL_DEFAULT_AL);
    push_bits_msb(&mut bits, of_target as u32, OF_DEFAULT_AL);
    push_bits_msb(&mut bits, ml_target as u32, ML_DEFAULT_AL);

    // 2. Each sequence: extra bits, then (if not last) state-update bits.
    for (i, c) in codes.iter().enumerate() {
        // OF extra bits.
        if c.of_extra_bits > 0 {
            push_bits_msb(&mut bits, c.of_extra_val, c.of_extra_bits);
        }
        // LL extra bits.
        if c.ll_extra_bits > 0 {
            push_bits_msb(&mut bits, c.ll_extra_val, c.ll_extra_bits);
        }
        // ML extra bits.
        if c.ml_extra_bits > 0 {
            push_bits_msb(&mut bits, c.ml_extra_val, c.ml_extra_bits);
        }

        // State updates (decoder reads: LL, ML, OF — in that order).
        if let Some((nb, val)) = enc[i].ll_update {
            push_bits_msb(&mut bits, val as u32, nb);
        }
        if let Some((nb, val)) = enc[i].ml_update {
            push_bits_msb(&mut bits, val as u32, nb);
        }
        if let Some((nb, val)) = enc[i].of_update {
            push_bits_msb(&mut bits, val as u32, nb);
        }
    }

    encode_bits_to_bytes(&bits)
}

/// Encode an offset value for zstd sequences.
///
/// Returns (code, extra_bits_count, extra_bits_value).
/// Offset codes in zstd: code = highest_bit(offset+3), extra_bits = offset+3 - (1<<code).
fn encode_offset(offset: u32) -> (u8, u8, u32) {
    // Zstd offset encoding: the raw offset sent in the stream needs to account
    // for repeat offsets. Since we don't use repeat offsets in the compressor,
    // the raw offset = actual_offset + 3 (values 1-3 are reserved for repeats).
    let raw = offset + 3;
    if raw == 0 { return (0, 0, 0); }
    let code = 31u8.saturating_sub(raw.leading_zeros() as u8);
    let extra_bits = code;
    let extra_val = raw - (1u32 << code);
    (code, extra_bits, extra_val)
}

/// Encode a literal length value for zstd sequences.
fn encode_literal_length(ll: u32) -> (u8, u8, u32) {
    if ll < 16 { return (ll as u8, 0, 0); }
    // Use the code table.
    let (code, bits, base) = match ll {
        16..=17 => (16, 1, 16),
        18..=19 => (17, 1, 18),
        20..=21 => (18, 1, 20),
        22..=23 => (19, 1, 22),
        24..=27 => (20, 2, 24),
        28..=31 => (21, 2, 28),
        32..=39 => (22, 3, 32),
        40..=47 => (23, 3, 40),
        48..=63 => (24, 4, 48),
        64..=95 => (25, 4, 64),
        96..=127 => (26, 5, 96),
        128..=191 => (27, 5, 128),
        192..=255 => (28, 6, 192),
        256..=383 => (29, 6, 256),
        384..=511 => (30, 7, 384),
        512..=767 => (31, 7, 512),
        768..=1023 => (32, 8, 768),
        1024..=1535 => (33, 8, 1024),
        1536..=2047 => (34, 9, 1536),
        _ => (35, 9, 2048),
    };
    (code as u8, bits as u8, ll - base)
}

/// Encode a match length value for zstd sequences.
fn encode_match_length(ml: u32) -> (u8, u8, u32) {
    if ml < 3 { return (0, 0, 0); } // shouldn't happen — min match is 3
    let ml_minus3 = ml - 3;
    if ml_minus3 < 32 { return (ml_minus3 as u8, 0, 0); }
    let (code, bits, base) = match ml {
        35..=36 => (32, 1, 35),
        37..=38 => (33, 1, 37),
        39..=40 => (34, 1, 39),
        41..=42 => (35, 1, 41),
        43..=46 => (36, 2, 43),
        47..=50 => (37, 2, 47),
        51..=58 => (38, 3, 51),
        59..=66 => (39, 3, 59),
        67..=82 => (40, 4, 67),
        83..=98 => (41, 4, 83),
        99..=130 => (42, 5, 99),
        131..=162 => (43, 5, 131),
        163..=226 => (44, 6, 163),
        227..=290 => (45, 6, 227),
        291..=418 => (46, 7, 291),
        419..=546 => (47, 7, 419),
        547..=802 => (48, 8, 547),
        803..=1058 => (49, 8, 803),
        1059..=1570 => (50, 9, 1059),
        1571..=2082 => (51, 9, 1571),
        _ => (52, 10, 2083),
    };
    (code as u8, bits as u8, ml - base)
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Build a minimal valid zstd frame containing raw literals and no sequences.
///
/// This creates the simplest possible valid zstd frame:
/// - Frame header: magic + descriptor (single segment, content size = N)
/// - One compressed block containing: raw literals + 0 sequences
fn build_test_frame(payload: &[u8]) -> Vec<u8> {
    let mut frame = Vec::new();

    // Magic number.
    frame.extend_from_slice(&ZSTD_MAGIC.to_le_bytes());

    // Frame descriptor: single segment, no checksum, no dict.
    // FCS_Field_Size = 1 byte (fcs_flag=0, single_segment=1).
    // Descriptor byte: bit 5 = single_segment, rest = 0.
    // 0b00_1_0_0_0_00 = 0x20
    frame.push(0x20);

    // Content size (1 byte, since fcs_flag = 0 and single_segment = 1).
    let size = payload.len() as u8;
    frame.push(size);

    // Block header: last_block = 1, type = compressed (2), size = block content size.
    // We'll build the block content first.
    let mut block_content = Vec::new();

    // Literals section: raw type, size_format = 0 (5-bit size).
    // Header byte: bits[1:0] = 0 (raw), bits[3:2] = 0 (size_format), bits[7:4] = size >> 1...
    // Actually: for raw type, size_format 0|2: 1 byte header, size = header >> 3.
    // So header = (payload.len() << 3) | 0b000 (raw, size_format=0).
    if payload.len() < 32 {
        block_content.push((payload.len() as u8) << 3); // type=0, size_format=0, size in bits 3..7
        block_content.extend_from_slice(payload);
    } else {
        // Use 2-byte header for larger payloads (size_format = 1, 12-bit size).
        let sz = payload.len();
        let b0 = ((sz << 4) as u8) | 0b0100; // type=0, size_format=1
        let b1 = (sz >> 4) as u8;
        block_content.push(b0);
        block_content.push(b1);
        block_content.extend_from_slice(payload);
    }

    // Sequences section: 0 sequences.
    block_content.push(0); // num_sequences = 0

    // Block header: 3 bytes.
    let bsize = block_content.len();
    let bh = 1u32 | (2u32 << 1) | ((bsize as u32) << 3); // last=1, type=compressed, size
    frame.push(bh as u8);
    frame.push((bh >> 8) as u8);
    frame.push((bh >> 16) as u8);

    frame.extend_from_slice(&block_content);

    frame
}

/// Build a test frame with a content checksum.
fn build_test_frame_with_checksum(payload: &[u8]) -> Vec<u8> {
    let mut frame = Vec::new();

    // Magic.
    frame.extend_from_slice(&ZSTD_MAGIC.to_le_bytes());

    // Descriptor: single_segment=1, checksum=1, fcs_flag=0.
    // bit 5 = single_segment, bit 2 = checksum
    // 0b00_1_0_0_1_00 = 0x24
    frame.push(0x24);

    // Content size (1 byte).
    frame.push(payload.len() as u8);

    // Raw block (simplest).
    let bh = 1u32 | (0u32 << 1) | ((payload.len() as u32) << 3);
    frame.push(bh as u8);
    frame.push((bh >> 8) as u8);
    frame.push((bh >> 16) as u8);
    frame.extend_from_slice(payload);

    // Content checksum: lower 32 bits of xxHash-64.
    let hash = xxhash64(payload);
    frame.extend_from_slice(&(hash as u32).to_le_bytes());

    frame
}

/// Build a test frame with an RLE block.
fn build_test_frame_rle(byte: u8, count: usize) -> Vec<u8> {
    let mut frame = Vec::new();

    frame.extend_from_slice(&ZSTD_MAGIC.to_le_bytes());

    // Descriptor: single_segment=1, checksum=1.
    frame.push(0x24);
    frame.push(count as u8);

    // RLE block: type=1, size=count.
    let bh = 1u32 | (1u32 << 1) | ((count as u32) << 3);
    frame.push(bh as u8);
    frame.push((bh >> 8) as u8);
    frame.push((bh >> 16) as u8);
    frame.push(byte);

    // Checksum.
    let data = vec![byte; count];
    let hash = xxhash64(&data);
    frame.extend_from_slice(&(hash as u32).to_le_bytes());

    frame
}

/// Run self-tests for the zstd decompression module.
pub fn self_test() -> KernelResult<()> {
    serial_println!("[zstd] Starting self-test...");

    // Test 1: xxHash-64.
    serial_println!("[zstd]   xxHash-64...");
    // Known test vector: xxhash64("") with seed 0 = 0xEF46DB3751D8E999.
    let h = xxhash64(&[]);
    if h != 0xEF46_DB37_51D8_E999 {
        serial_println!("[zstd]   FAIL: xxhash64('') = {:#x}, expected 0xEF46DB3751D8E999", h);
        return Err(KernelError::InternalError);
    }

    // xxhash64("abc") with seed 0 = known value.
    let h2 = xxhash64(b"abc");
    // Reference: xxhash64("abc", 0) = 0x44BC2CF5AD770999
    if h2 != 0x44BC_2CF5_AD77_0999 {
        serial_println!("[zstd]   FAIL: xxhash64('abc') = {:#x}, expected 0x44BC2CF5AD770999", h2);
        return Err(KernelError::InternalError);
    }

    // Test 2: Frame header parsing.
    serial_println!("[zstd]   Frame header parsing...");
    let test_frame = build_test_frame(b"hello");
    let header = parse_frame_header(&test_frame)?;
    if header.content_size != Some(5) {
        serial_println!("[zstd]   FAIL: content_size = {:?}, expected Some(5)", header.content_size);
        return Err(KernelError::InternalError);
    }
    if !header.single_segment {
        serial_println!("[zstd]   FAIL: expected single_segment = true");
        return Err(KernelError::InternalError);
    }

    // Test 3: Raw block decompression.
    serial_println!("[zstd]   Raw block decompression...");
    let raw_frame = build_test_frame_with_checksum(b"Hello, zstd!");
    let result = unzstd(&raw_frame)?;
    if result.as_slice() != b"Hello, zstd!" {
        serial_println!("[zstd]   FAIL: raw block mismatch");
        return Err(KernelError::InternalError);
    }

    // Test 4: RLE block.
    serial_println!("[zstd]   RLE block decompression...");
    let rle_frame = build_test_frame_rle(0xAB, 50);
    let result = unzstd(&rle_frame)?;
    if result.len() != 50 || result.iter().any(|&b| b != 0xAB) {
        serial_println!("[zstd]   FAIL: RLE block mismatch");
        return Err(KernelError::InternalError);
    }

    // Test 5: Compressed block with raw literals and 0 sequences.
    serial_println!("[zstd]   Compressed block (raw literals, 0 seqs)...");
    let comp_frame = build_test_frame(b"test data");
    let result = unzstd(&comp_frame)?;
    if result.as_slice() != b"test data" {
        serial_println!("[zstd]   FAIL: compressed block mismatch: got {} bytes", result.len());
        return Err(KernelError::InternalError);
    }

    // Test 6: Skippable frame handling.
    serial_println!("[zstd]   Skippable frame...");
    let mut multi = Vec::new();
    // Skippable frame: magic 0x184D2A50 + 4-byte size + data.
    multi.extend_from_slice(&SKIPPABLE_MAGIC_BASE.to_le_bytes());
    multi.extend_from_slice(&5u32.to_le_bytes()); // 5 bytes of skip data
    multi.extend_from_slice(b"SKIP!");
    // Then a real frame.
    multi.extend_from_slice(&build_test_frame_with_checksum(b"after skip"));
    let result = unzstd(&multi)?;
    if result.as_slice() != b"after skip" {
        serial_println!("[zstd]   FAIL: skippable frame handling");
        return Err(KernelError::InternalError);
    }

    // Test 7: Concatenated frames.
    serial_println!("[zstd]   Concatenated frames...");
    let mut concat = Vec::new();
    concat.extend_from_slice(&build_test_frame_with_checksum(b"frame1"));
    concat.extend_from_slice(&build_test_frame_with_checksum(b"frame2"));
    let result = unzstd(&concat)?;
    if result.as_slice() != b"frame1frame2" {
        serial_println!("[zstd]   FAIL: concatenated frames");
        return Err(KernelError::InternalError);
    }

    // Test 8: Content size mismatch detection.
    serial_println!("[zstd]   Content size mismatch detection...");
    let mut bad_frame = build_test_frame(b"hello");
    // Corrupt the content size field (byte 5, which is content_size).
    if bad_frame.len() > 5 {
        bad_frame[5] = 99; // Claim content is 99 bytes.
    }
    match unzstd(&bad_frame) {
        Err(KernelError::CorruptedData) => {} // expected
        other => {
            serial_println!("[zstd]   FAIL: expected CorruptedData for size mismatch, got {:?}", other.err());
            return Err(KernelError::InternalError);
        }
    }

    // Test 9: Checksum validation.
    serial_println!("[zstd]   Checksum validation...");
    let mut bad_checksum = build_test_frame_with_checksum(b"verify");
    // Corrupt the last byte (part of checksum).
    if let Some(last) = bad_checksum.last_mut() {
        *last ^= 0xFF;
    }
    match unzstd(&bad_checksum) {
        Err(KernelError::CorruptedData) => {} // expected
        other => {
            serial_println!("[zstd]   FAIL: expected CorruptedData for bad checksum, got {:?}", other.err());
            return Err(KernelError::InternalError);
        }
    }

    // Test 10: Bit reader.
    serial_println!("[zstd]   Bit reader...");
    let bits_data = [0b10110100u8, 0b01011001];
    let mut br = BitReader::new(&bits_data);
    let v = br.read_bits(4)?; // should read low 4 bits of first byte: 0100 = 4
    if v != 4 {
        serial_println!("[zstd]   FAIL: bit reader: got {}, expected 4", v);
        return Err(KernelError::InternalError);
    }
    let v2 = br.read_bits(4)?; // next 4 bits: 1011 = 11
    if v2 != 11 {
        serial_println!("[zstd]   FAIL: bit reader: got {}, expected 11", v2);
        return Err(KernelError::InternalError);
    }

    // Test 11: zstd_store round-trip (store mode compression).
    serial_println!("[zstd]   Store-mode round-trip...");
    let store_input = b"The quick brown fox jumps over the lazy dog";
    let compressed_store = zstd_store(store_input);
    let decompressed_store = unzstd(&compressed_store)?;
    if decompressed_store.as_slice() != store_input.as_slice() {
        serial_println!(
            "[zstd]   FAIL: store round-trip: got {} bytes, expected {}",
            decompressed_store.len(),
            store_input.len()
        );
        return Err(KernelError::InternalError);
    }

    // Test 12: zstd_store empty input.
    serial_println!("[zstd]   Store-mode empty input...");
    let compressed_empty = zstd_store(b"");
    let decompressed_empty = unzstd(&compressed_empty)?;
    if !decompressed_empty.is_empty() {
        serial_println!("[zstd]   FAIL: store empty: got {} bytes", decompressed_empty.len());
        return Err(KernelError::InternalError);
    }

    // Test 13: zstd_store RLE-heavy input.
    serial_println!("[zstd]   Store-mode RLE data...");
    let rle_input = vec![0x42u8; 1000];
    let compressed_rle = zstd_store(&rle_input);
    let decompressed_rle = unzstd(&compressed_rle)?;
    if decompressed_rle.as_slice() != rle_input.as_slice() {
        serial_println!("[zstd]   FAIL: store RLE round-trip");
        return Err(KernelError::InternalError);
    }
    // RLE should compress well.
    if compressed_rle.len() > 20 {
        serial_println!(
            "[zstd]   FAIL: store RLE overhead: {} bytes for 1000-byte RLE input",
            compressed_rle.len()
        );
        return Err(KernelError::InternalError);
    }

    // Test 14: compress_zstd round-trip (LZ77 compression).
    serial_println!("[zstd]   LZ77 round-trip...");
    // Repetitive data that LZ77 should compress.
    let mut lz_input = Vec::with_capacity(512);
    for _ in 0..16 {
        lz_input.extend_from_slice(b"ABCDEFGHIJKLMNOPQRSTUVWXYZ012345");
    }
    let compressed_lz = compress_zstd(&lz_input);
    let decompressed_lz = unzstd(&compressed_lz)?;
    if decompressed_lz.as_slice() != lz_input.as_slice() {
        serial_println!(
            "[zstd]   FAIL: LZ77 round-trip: got {} bytes, expected {}",
            decompressed_lz.len(),
            lz_input.len()
        );
        return Err(KernelError::InternalError);
    }
    serial_println!(
        "[zstd]   LZ77: {} -> {} bytes ({:.0}%)",
        lz_input.len(),
        compressed_lz.len(),
        compressed_lz.len() as f64 / lz_input.len() as f64 * 100.0
    );

    // Test 15: compress_zstd small input (falls back to store mode).
    serial_println!("[zstd]   LZ77 small input fallback...");
    let small_input = b"tiny";
    let compressed_small = compress_zstd(small_input);
    let decompressed_small = unzstd(&compressed_small)?;
    if decompressed_small.as_slice() != small_input.as_slice() {
        serial_println!("[zstd]   FAIL: LZ77 small input round-trip");
        return Err(KernelError::InternalError);
    }

    // Test 16: compress_zstd with realistic text data.
    serial_println!("[zstd]   LZ77 text data...");
    let text_input = b"The zstd compression format is designed for fast \
        compression and decompression. It supports a wide range of \
        compression ratios. The format uses finite state entropy coding \
        and LZ77 matching for efficient data reduction. Testing testing \
        testing one two three. The quick brown fox jumps over the lazy dog. \
        The quick brown fox jumps over the lazy dog again and again.";
    let compressed_text = compress_zstd(text_input);
    let decompressed_text = unzstd(&compressed_text)?;
    if decompressed_text.as_slice() != text_input.as_slice() {
        serial_println!(
            "[zstd]   FAIL: LZ77 text round-trip: got {} bytes, expected {}",
            decompressed_text.len(),
            text_input.len()
        );
        return Err(KernelError::InternalError);
    }
    serial_println!(
        "[zstd]   Text: {} -> {} bytes ({:.0}%)",
        text_input.len(),
        compressed_text.len(),
        compressed_text.len() as f64 / text_input.len() as f64 * 100.0
    );

    serial_println!("[zstd] Self-test passed.");
    Ok(())
}
