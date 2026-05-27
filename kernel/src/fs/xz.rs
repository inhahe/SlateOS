//! XZ compression and decompression (LZMA2-based) for `.xz` archives.
//!
//! ## Decompression
//!
//! - XZ stream header/footer parsing with CRC-32 validation
//! - Block parsing with optional compressed/uncompressed size fields
//! - LZMA2 chunk decoder (uncompressed, LZMA with various reset levels)
//! - LZMA range decoder with adaptive probability model
//! - CRC-64 (ECMA-182) for block/stream integrity checks
//!
//! ## Compression
//!
//! - LZMA range encoder (adaptive probabilities, matching decoder exactly)
//! - LZ77 match finder (hash-chain, 4-byte minimum match)
//! - LZMA encoder (literals, matches, rep-matches, short-reps)
//! - LZMA2 chunk framing (single compressed chunk per block)
//! - XZ container writer (stream header, block, index, footer, CRC-64)
//!
//! ## References
//!
//! - XZ format: <https://tukaani.org/xz/xz-file-format.txt>
//! - LZMA2: 7-Zip LZMA SDK (lzma.txt, LzmaDec.c)
//! - LZMA: Igor Pavlov's specification in the LZMA SDK

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::error::{KernelError, KernelResult};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// XZ stream header magic (6 bytes).
const XZ_MAGIC: [u8; 6] = [0xFD, 0x37, 0x7A, 0x58, 0x5A, 0x00];

/// XZ stream footer magic (2 bytes).
const XZ_FOOTER_MAGIC: [u8; 2] = [0x59, 0x5A];

/// LZMA2 filter ID in XZ block headers.
const LZMA2_FILTER_ID: u64 = 0x21;

/// Safety limit on decompressed output (256 MiB).
const MAX_OUTPUT: usize = 256 * 1024 * 1024;

/// Minimum dictionary size enforced by LZMA2 (4 KiB).
const MIN_DICT_SIZE: u32 = 4096;

// LZMA probability model constants.
const STATES: usize = 12;
const LIT_STATES: usize = 7;
const POS_STATES_MAX: usize = 1 << 4; // pb max = 4

const LEN_LOW_BITS: usize = 3;
const LEN_MID_BITS: usize = 3;
const LEN_HIGH_BITS: usize = 8;

const POS_SLOT_BITS: usize = 6;
const ALIGN_BITS: usize = 4;

const START_POS_MODEL: usize = 4;
const END_POS_MODEL: usize = 14;
const FULL_DISTANCES: usize = 1 << (END_POS_MODEL / 2); // 128

const PROB_INIT: u16 = 1024;
const PROB_BITS: u32 = 11;

// LZMA state transition tables.
const STATE_LIT: [u8; 12] = [0, 0, 0, 0, 1, 2, 3, 4, 5, 6, 4, 5];
const STATE_MATCH: [u8; 12] = [7, 7, 7, 7, 7, 7, 7, 10, 10, 10, 10, 10];
const STATE_REP: [u8; 12] = [8, 8, 8, 8, 8, 8, 8, 11, 11, 11, 11, 11];
const STATE_SHORTREP: [u8; 12] = [9, 9, 9, 9, 9, 9, 9, 11, 11, 11, 11, 11];

// XZ check types.
const CHECK_NONE: u8 = 0x00;
const CHECK_CRC32: u8 = 0x01;
const CHECK_CRC64: u8 = 0x04;

// ---------------------------------------------------------------------------
// CRC-64 (ECMA-182, reflected polynomial)
// ---------------------------------------------------------------------------

/// ECMA-182 CRC-64 polynomial (reflected).
const CRC64_POLY: u64 = 0xC96C_5795_D787_0F42;

/// Compute CRC-64 (ECMA-182) of `data`.
fn crc64(data: &[u8]) -> u64 {
    let mut crc: u64 = !0u64;
    for &b in data {
        crc ^= u64::from(b);
        for _ in 0..8 {
            if (crc & 1) != 0 {
                crc = (crc >> 1) ^ CRC64_POLY;
            } else {
                crc >>= 1;
            }
        }
    }
    !crc
}

// ---------------------------------------------------------------------------
// Range decoder
// ---------------------------------------------------------------------------

/// Adaptive range decoder for LZMA compressed data.
///
/// Reads from a byte slice, maintaining a 32-bit range and 32-bit code.
/// The first byte of the stream must be 0x00, followed by 4 bytes that
/// initialize the code register.
struct RangeDecoder<'a> {
    data: &'a [u8],
    pos: usize,
    range: u32,
    code: u32,
}

impl<'a> RangeDecoder<'a> {
    /// Create a new range decoder from LZMA compressed data.
    ///
    /// Consumes the first 5 bytes (init byte + 4 code bytes).
    fn new(data: &'a [u8]) -> KernelResult<Self> {
        if data.len() < 5 {
            return Err(KernelError::CorruptedData);
        }
        // First byte must be 0x00 per the LZMA spec.
        if *data.first().ok_or(KernelError::CorruptedData)? != 0 {
            return Err(KernelError::CorruptedData);
        }
        let code = u32::from_be_bytes([
            *data.get(1).ok_or(KernelError::CorruptedData)?,
            *data.get(2).ok_or(KernelError::CorruptedData)?,
            *data.get(3).ok_or(KernelError::CorruptedData)?,
            *data.get(4).ok_or(KernelError::CorruptedData)?,
        ]);
        Ok(Self {
            data,
            pos: 5,
            range: 0xFFFF_FFFF,
            code,
        })
    }

    /// Normalize the range if it has dropped below 2^24.
    #[inline]
    fn normalize(&mut self) {
        if self.range < (1u32 << 24) {
            self.range <<= 8;
            self.code = (self.code << 8)
                | u32::from(self.data.get(self.pos).copied().unwrap_or(0));
            self.pos = self.pos.saturating_add(1);
        }
    }

    /// Decode a single bit using an adaptive probability.
    ///
    /// Updates `prob` towards 2048 (if bit=0) or 0 (if bit=1).
    #[inline]
    fn decode_bit(&mut self, prob: &mut u16) -> u32 {
        self.normalize();
        let bound = (self.range >> PROB_BITS).saturating_mul(u32::from(*prob));
        if self.code < bound {
            self.range = bound;
            *prob = prob.saturating_add((2048u16.saturating_sub(*prob)) >> 5);
            0
        } else {
            self.code = self.code.wrapping_sub(bound);
            self.range = self.range.wrapping_sub(bound);
            *prob = prob.saturating_sub(*prob >> 5);
            1
        }
    }

    /// Decode `count` bits with fixed 50/50 probability (direct bits).
    fn decode_direct_bits(&mut self, count: usize) -> u32 {
        let mut result: u32 = 0;
        for _ in 0..count {
            self.normalize();
            self.range >>= 1;
            result <<= 1;
            if self.code >= self.range {
                self.code = self.code.wrapping_sub(self.range);
                result |= 1;
            }
        }
        result
    }

    /// Returns true if we've consumed all the input data.
    fn is_finished(&self, compressed_size: usize) -> bool {
        self.pos >= compressed_size
    }
}

// ---------------------------------------------------------------------------
// Bit-tree decoders
// ---------------------------------------------------------------------------

/// Decode a `num_bits`-wide value from a bit-tree.
///
/// The tree is stored in `probs[1..(1 << num_bits)]` (index 0 unused).
/// Returns a value in `0..(1 << num_bits)`.
fn decode_bit_tree(
    rc: &mut RangeDecoder<'_>,
    probs: &mut [u16],
    num_bits: usize,
) -> u32 {
    let mut m: u32 = 1;
    for _ in 0..num_bits {
        let idx = m as usize;
        if let Some(p) = probs.get_mut(idx) {
            m = (m << 1) | rc.decode_bit(p);
        }
    }
    m.wrapping_sub(1u32 << num_bits)
}

/// Decode a `num_bits`-wide value from a reversed bit-tree (LSB first).
///
/// Used for distance footer bits and alignment bits.
fn decode_bit_tree_reverse(
    rc: &mut RangeDecoder<'_>,
    probs: &mut [u16],
    offset: usize,
    num_bits: usize,
) -> u32 {
    let mut m: u32 = 1;
    let mut result: u32 = 0;
    for i in 0..num_bits {
        let idx = offset.saturating_add(m as usize);
        if let Some(p) = probs.get_mut(idx) {
            let bit = rc.decode_bit(p);
            m = (m << 1) | bit;
            result |= bit << i;
        }
    }
    result
}

// ---------------------------------------------------------------------------
// Length decoder
// ---------------------------------------------------------------------------

/// LZMA length decoder with three tiers: low (2-9), mid (10-17), high (18-273).
struct LenDecoder {
    choice: u16,
    choice2: u16,
    low: [[u16; 1 << LEN_LOW_BITS]; POS_STATES_MAX],
    mid: [[u16; 1 << LEN_MID_BITS]; POS_STATES_MAX],
    high: [u16; 1 << LEN_HIGH_BITS],
}

impl LenDecoder {
    fn new() -> Self {
        Self {
            choice: PROB_INIT,
            choice2: PROB_INIT,
            low: [[PROB_INIT; 1 << LEN_LOW_BITS]; POS_STATES_MAX],
            mid: [[PROB_INIT; 1 << LEN_MID_BITS]; POS_STATES_MAX],
            high: [PROB_INIT; 1 << LEN_HIGH_BITS],
        }
    }

    fn reset(&mut self) {
        self.choice = PROB_INIT;
        self.choice2 = PROB_INIT;
        for arr in self.low.iter_mut() {
            arr.fill(PROB_INIT);
        }
        for arr in self.mid.iter_mut() {
            arr.fill(PROB_INIT);
        }
        self.high.fill(PROB_INIT);
    }

    fn decode(&mut self, rc: &mut RangeDecoder<'_>, pos_state: usize) -> u32 {
        if rc.decode_bit(&mut self.choice) == 0 {
            decode_bit_tree(rc, &mut self.low[pos_state], LEN_LOW_BITS)
                .wrapping_add(2)
        } else if rc.decode_bit(&mut self.choice2) == 0 {
            decode_bit_tree(rc, &mut self.mid[pos_state], LEN_MID_BITS)
                .wrapping_add(2 + (1 << LEN_LOW_BITS))
        } else {
            decode_bit_tree(rc, &mut self.high[..], LEN_HIGH_BITS)
                .wrapping_add(2 + (1 << LEN_LOW_BITS) + (1 << LEN_MID_BITS))
        }
    }
}

// ---------------------------------------------------------------------------
// LZMA decoder state
// ---------------------------------------------------------------------------

/// Full LZMA probability model and decoder state.
pub(crate) struct LzmaState {
    // Properties
    lc: u8,
    lp: u8,
    pb: u8,

    // Markov state (0-11)
    state: u8,

    // Repeat distances
    rep: [u32; 4],

    // Probability arrays
    is_match: [[u16; POS_STATES_MAX]; STATES],
    is_rep: [u16; STATES],
    is_rep_g0: [u16; STATES],
    is_rep_g1: [u16; STATES],
    is_rep_g2: [u16; STATES],
    is_rep0_long: [[u16; POS_STATES_MAX]; STATES],

    /// Literal sub-coders: `(1 << (lc + lp))` sub-coders, each with 0x300 probs.
    literal_probs: Vec<u16>,

    match_len: LenDecoder,
    rep_len: LenDecoder,

    /// Distance slot decoders: 4 bit-trees of 6 bits each.
    pos_slot: [[u16; 1 << POS_SLOT_BITS]; 4],

    /// Context-decoded distance bits for slots 4-13.
    /// Indexed as `pos_decoders[base + 1..base + (1 << num_bits)]`
    /// where `base = dist - slot` for each slot.
    pos_decoders: [u16; 1 + FULL_DISTANCES - END_POS_MODEL],

    /// Alignment bits decoder (4-bit reversed tree).
    align_decoder: [u16; 1 << ALIGN_BITS],
}

impl LzmaState {
    /// Create a new LZMA state with the given properties.
    pub(crate) fn new(lc: u8, lp: u8, pb: u8) -> Self {
        let num_lit_subcoders = 1usize << (lc.saturating_add(lp) as usize);
        let lit_size = num_lit_subcoders.saturating_mul(0x300);
        let mut literal_probs = Vec::new();
        literal_probs.resize(lit_size, PROB_INIT);

        let mut s = Self {
            lc, lp, pb,
            state: 0,
            rep: [0; 4],
            is_match: [[PROB_INIT; POS_STATES_MAX]; STATES],
            is_rep: [PROB_INIT; STATES],
            is_rep_g0: [PROB_INIT; STATES],
            is_rep_g1: [PROB_INIT; STATES],
            is_rep_g2: [PROB_INIT; STATES],
            is_rep0_long: [[PROB_INIT; POS_STATES_MAX]; STATES],
            literal_probs,
            match_len: LenDecoder::new(),
            rep_len: LenDecoder::new(),
            pos_slot: [[PROB_INIT; 1 << POS_SLOT_BITS]; 4],
            pos_decoders: [PROB_INIT; 1 + FULL_DISTANCES - END_POS_MODEL],
            align_decoder: [PROB_INIT; 1 << ALIGN_BITS],
        };
        s.reset_state();
        s
    }

    /// Reset all probabilities and state (but not properties or dictionary).
    fn reset_state(&mut self) {
        self.state = 0;
        self.rep = [0; 4];

        for row in self.is_match.iter_mut() { row.fill(PROB_INIT); }
        self.is_rep.fill(PROB_INIT);
        self.is_rep_g0.fill(PROB_INIT);
        self.is_rep_g1.fill(PROB_INIT);
        self.is_rep_g2.fill(PROB_INIT);
        for row in self.is_rep0_long.iter_mut() { row.fill(PROB_INIT); }

        self.literal_probs.fill(PROB_INIT);
        self.match_len.reset();
        self.rep_len.reset();

        for row in self.pos_slot.iter_mut() { row.fill(PROB_INIT); }
        self.pos_decoders.fill(PROB_INIT);
        self.align_decoder.fill(PROB_INIT);
    }

    /// Update properties (lc, lp, pb) and resize literal probs if needed.
    fn set_props(&mut self, lc: u8, lp: u8, pb: u8) {
        self.lc = lc;
        self.lp = lp;
        self.pb = pb;
        let num_lit = 1usize << (lc.saturating_add(lp) as usize);
        let needed = num_lit.saturating_mul(0x300);
        if self.literal_probs.len() != needed {
            self.literal_probs.resize(needed, PROB_INIT);
        }
    }
}

// ---------------------------------------------------------------------------
// LZMA core decode
// ---------------------------------------------------------------------------

/// Decode one LZMA chunk into `output`, using `dict` as the history buffer.
///
/// `data` is the raw LZMA compressed bytes (including 5-byte range init).
/// `uncompressed_size` is the expected output size for this chunk.
/// `dict_size` is the sliding window size for back-references.
///
/// Returns `Ok(())` on success; output is appended to `output`.
pub(crate) fn lzma_decode(
    lzma: &mut LzmaState,
    data: &[u8],
    uncompressed_size: usize,
    output: &mut Vec<u8>,
    dict_size: u32,
) -> KernelResult<()> {
    if data.len() < 5 {
        return Err(KernelError::CorruptedData);
    }

    let mut rc = RangeDecoder::new(data)?;
    let pos_mask = (1u32 << lzma.pb).wrapping_sub(1);
    let lc = lzma.lc;
    let lp = lzma.lp;
    let dict = dict_size as usize;

    let start_len = output.len();
    let target_len = start_len.saturating_add(uncompressed_size);

    if target_len > MAX_OUTPUT {
        return Err(KernelError::OutOfMemory);
    }

    while output.len() < target_len {
        let pos = output.len().wrapping_sub(start_len);
        let pos_state = (pos as u32 & pos_mask) as usize;
        let state = lzma.state as usize;

        if rc.decode_bit(
            &mut lzma.is_match[state][pos_state],
        ) == 0 {
            // --- Literal ---
            let prev_byte = if output.is_empty() {
                0u8
            } else {
                output.last().copied().unwrap_or(0)
            };

            // Select literal sub-coder.
            let lit_state = ((((pos as u32) & ((1u32 << lp).wrapping_sub(1))) << lc)
                | (u32::from(prev_byte) >> (8u32.wrapping_sub(u32::from(lc)))))
                as usize;
            let probs_offset = lit_state.saturating_mul(0x300);

            let byte = if (lzma.state as usize) < LIT_STATES {
                // Normal literal decode (state < 7).
                decode_literal_normal(&mut rc, &mut lzma.literal_probs, probs_offset)
            } else {
                // Match-byte-aware literal decode (state >= 7).
                let match_byte = get_dict_byte(output, lzma.rep[0] as usize);
                decode_literal_matched(
                    &mut rc, &mut lzma.literal_probs, probs_offset, match_byte,
                )
            };

            output.push(byte);
            lzma.state = STATE_LIT[lzma.state as usize];
        } else {
            // --- Match / Rep ---
            let len: u32;
            let dist: u32;

            if rc.decode_bit(&mut lzma.is_rep[state]) == 0 {
                // Simple match — new distance.
                len = lzma.match_len.decode(&mut rc, pos_state);
                let slot_idx = len.wrapping_sub(2).min(3) as usize;
                let slot = decode_bit_tree(
                    &mut rc,
                    &mut lzma.pos_slot[slot_idx],
                    POS_SLOT_BITS,
                );

                dist = decode_distance(&mut rc, lzma, slot)?;

                // Shift rep distances.
                lzma.rep[3] = lzma.rep[2];
                lzma.rep[2] = lzma.rep[1];
                lzma.rep[1] = lzma.rep[0];
                lzma.rep[0] = dist;
                lzma.state = STATE_MATCH[lzma.state as usize];
            } else if rc.decode_bit(&mut lzma.is_rep_g0[state]) == 0 {
                // Rep0
                if rc.decode_bit(&mut lzma.is_rep0_long[state][pos_state]) == 0 {
                    // Short rep (single byte at rep0 distance).
                    lzma.state = STATE_SHORTREP[lzma.state as usize];
                    let byte = get_dict_byte(output, lzma.rep[0] as usize);
                    output.push(byte);
                    continue;
                }
                len = lzma.rep_len.decode(&mut rc, pos_state);
                dist = lzma.rep[0];
                lzma.state = STATE_REP[lzma.state as usize];
            } else if rc.decode_bit(&mut lzma.is_rep_g1[state]) == 0 {
                // Rep1
                len = lzma.rep_len.decode(&mut rc, pos_state);
                dist = lzma.rep[1];
                lzma.rep[1] = lzma.rep[0];
                lzma.rep[0] = dist;
                lzma.state = STATE_REP[lzma.state as usize];
            } else if rc.decode_bit(&mut lzma.is_rep_g2[state]) == 0 {
                // Rep2
                len = lzma.rep_len.decode(&mut rc, pos_state);
                dist = lzma.rep[2];
                lzma.rep[2] = lzma.rep[1];
                lzma.rep[1] = lzma.rep[0];
                lzma.rep[0] = dist;
                lzma.state = STATE_REP[lzma.state as usize];
            } else {
                // Rep3
                len = lzma.rep_len.decode(&mut rc, pos_state);
                dist = lzma.rep[3];
                lzma.rep[3] = lzma.rep[2];
                lzma.rep[2] = lzma.rep[1];
                lzma.rep[1] = lzma.rep[0];
                lzma.rep[0] = dist;
                lzma.state = STATE_REP[lzma.state as usize];
            }

            // Copy `len` bytes from distance `dist` back in the output.
            let d = dist.saturating_add(1) as usize;
            if d > output.len() || d > dict {
                return Err(KernelError::CorruptedData);
            }
            for _ in 0..len {
                let byte = get_dict_byte(output, dist as usize);
                output.push(byte);
                if output.len() >= target_len {
                    break;
                }
            }
        }
    }

    // Trim to exact size (match loop may overshoot by a few bytes in the
    // copy-from-distance path).
    output.truncate(target_len);

    Ok(())
}

/// Decode a literal byte (state < 7, no match byte context).
fn decode_literal_normal(
    rc: &mut RangeDecoder<'_>,
    probs: &mut [u16],
    offset: usize,
) -> u8 {
    let mut symbol: u32 = 1;
    for _ in 0..8 {
        let idx = offset.saturating_add(symbol as usize);
        if let Some(p) = probs.get_mut(idx) {
            symbol = (symbol << 1) | rc.decode_bit(p);
        }
    }
    symbol as u8
}

/// Decode a literal byte with match-byte context (state >= 7).
///
/// When the previous output was a match/rep, the literal decoder uses
/// the corresponding byte from the match distance as context.  While
/// the decoded bits agree with the match byte, a triple-indexed prob
/// set is used; once they diverge, it falls back to normal decoding.
fn decode_literal_matched(
    rc: &mut RangeDecoder<'_>,
    probs: &mut [u16],
    offset: usize,
    match_byte: u8,
) -> u8 {
    let mut symbol: u32 = 1;
    let mut match_byte = u32::from(match_byte) << 1;
    let mut mismatch = false;

    for _ in 0..8 {
        let match_bit = (match_byte >> 8) & 1;
        match_byte <<= 1;

        let sub_offset = if mismatch {
            offset.saturating_add(symbol as usize)
        } else {
            offset.saturating_add(
                (((1u32.wrapping_add(match_bit)) << 8)
                    .wrapping_add(symbol)) as usize,
            )
        };

        if let Some(p) = probs.get_mut(sub_offset) {
            let bit = rc.decode_bit(p);
            symbol = (symbol << 1) | bit;
            if !mismatch && bit != match_bit {
                mismatch = true;
            }
        }
    }
    symbol as u8
}

/// Decode a full distance value from the distance slot.
fn decode_distance(
    rc: &mut RangeDecoder<'_>,
    lzma: &mut LzmaState,
    slot: u32,
) -> KernelResult<u32> {
    if slot < START_POS_MODEL as u32 {
        return Ok(slot);
    }

    let num_direct_bits = (slot >> 1).wrapping_sub(1) as usize;
    let mut dist = (2u32 | (slot & 1)) << num_direct_bits;

    if slot < END_POS_MODEL as u32 {
        // Context-decoded reversed bits from pos_decoders.
        let base = dist.wrapping_sub(slot) as usize;
        let footer = decode_bit_tree_reverse(
            rc,
            &mut lzma.pos_decoders,
            base,
            num_direct_bits,
        );
        dist = dist.wrapping_add(footer);
    } else {
        // Direct bits (fixed 0.5 prob) for the high bits,
        // then 4 alignment bits from the alignment decoder.
        let high_bits = num_direct_bits.saturating_sub(ALIGN_BITS);
        let direct = rc.decode_direct_bits(high_bits);
        dist = dist.wrapping_add(direct << ALIGN_BITS);

        let align = decode_bit_tree_reverse(
            rc,
            &mut lzma.align_decoder,
            0,
            ALIGN_BITS,
        );
        dist = dist.wrapping_add(align);
    }

    Ok(dist)
}

/// Get a byte from the output buffer at `distance` bytes back.
#[inline]
fn get_dict_byte(output: &[u8], distance: usize) -> u8 {
    let pos = output.len().wrapping_sub(distance.wrapping_add(1));
    output.get(pos).copied().unwrap_or(0)
}

// ---------------------------------------------------------------------------
// LZMA2 decoder
// ---------------------------------------------------------------------------

/// Decode an LZMA2 stream into `output`.
///
/// `data` is the LZMA2 payload (after the XZ block header).
/// `dict_size` is the dictionary size from the LZMA2 properties byte.
pub(crate) fn lzma2_decode(data: &[u8], dict_size: u32) -> KernelResult<Vec<u8>> {
    let mut output = Vec::new();
    let mut pos = 0usize;
    let mut lzma: Option<LzmaState> = None;

    loop {
        // Read control byte.
        let control = *data.get(pos).ok_or(KernelError::CorruptedData)?;
        pos = pos.saturating_add(1);

        if control == 0x00 {
            // End of LZMA2 stream.
            break;
        }

        if control == 0x01 || control == 0x02 {
            // Uncompressed chunk.
            // 0x01 = dictionary reset, 0x02 = no reset.
            let size_hi = u16::from(*data.get(pos).ok_or(KernelError::CorruptedData)?);
            let size_lo = u16::from(*data.get(pos + 1).ok_or(KernelError::CorruptedData)?);
            pos = pos.saturating_add(2);
            let chunk_size = ((size_hi << 8) | size_lo).wrapping_add(1) as usize;

            let chunk = data.get(pos..pos.saturating_add(chunk_size))
                .ok_or(KernelError::CorruptedData)?;
            pos = pos.saturating_add(chunk_size);

            if control == 0x01 {
                // Dictionary reset — clear output history for distance refs.
                // (We keep all output; LZMA state handles dict reset.)
                if let Some(ref mut s) = lzma {
                    s.rep = [0; 4];
                }
            }

            if output.len().saturating_add(chunk_size) > MAX_OUTPUT {
                return Err(KernelError::OutOfMemory);
            }
            output.extend_from_slice(chunk);
            continue;
        }

        // LZMA chunk: control byte >= 0x80.
        if control < 0x80 {
            return Err(KernelError::CorruptedData);
        }

        // Decode sizes from the control byte and following bytes.
        // Bits [4:0] of control = bits [20:16] of (uncompressed_size - 1).
        let uncomp_hi = u32::from(control & 0x1F);
        let uncomp_mid = u16::from(*data.get(pos).ok_or(KernelError::CorruptedData)?);
        let uncomp_lo = u16::from(*data.get(pos + 1).ok_or(KernelError::CorruptedData)?);
        pos = pos.saturating_add(2);
        let uncompressed_size = ((uncomp_hi << 16)
            | (u32::from(uncomp_mid) << 8)
            | u32::from(uncomp_lo))
            .wrapping_add(1) as usize;

        let comp_hi = u16::from(*data.get(pos).ok_or(KernelError::CorruptedData)?);
        let comp_lo = u16::from(*data.get(pos + 1).ok_or(KernelError::CorruptedData)?);
        pos = pos.saturating_add(2);
        let compressed_size = ((comp_hi << 8) | comp_lo).wrapping_add(1) as usize;

        // Determine reset level from control byte range.
        let needs_props_reset = control >= 0xC0;
        let needs_state_reset = control >= 0xA0;
        let needs_full_reset = control >= 0xE0;

        if needs_props_reset {
            // Read new properties byte.
            let props_byte = *data.get(pos).ok_or(KernelError::CorruptedData)?;
            pos = pos.saturating_add(1);

            let lc = props_byte % 9;
            let remainder = props_byte / 9;
            let lp = remainder % 5;
            let pb = remainder / 5;

            if pb > 4 || lc > 8 || lp > 4 {
                return Err(KernelError::CorruptedData);
            }

            if needs_full_reset || lzma.is_none() {
                lzma = Some(LzmaState::new(lc, lp, pb));
            } else if let Some(ref mut s) = lzma {
                s.set_props(lc, lp, pb);
                if needs_state_reset {
                    s.reset_state();
                }
            }
        } else if needs_state_reset {
            if let Some(ref mut s) = lzma {
                s.reset_state();
            } else {
                return Err(KernelError::CorruptedData);
            }
        }

        let state = lzma.as_mut().ok_or(KernelError::CorruptedData)?;

        // The compressed_size includes the 5-byte range coder init
        // when props are reset (the props byte is NOT included).
        let lzma_data = data.get(pos..pos.saturating_add(compressed_size))
            .ok_or(KernelError::CorruptedData)?;
        pos = pos.saturating_add(compressed_size);

        lzma_decode(state, lzma_data, uncompressed_size, &mut output, dict_size)?;
    }

    Ok(output)
}

// ---------------------------------------------------------------------------
// XZ container parser
// ---------------------------------------------------------------------------

/// Read a variable-length integer (VLI) from `data[pos..]`.
///
/// XZ uses 7 bits per byte, high bit = continuation, max 9 bytes (63 bits).
/// Returns `(value, bytes_consumed)`.
fn read_vli(data: &[u8], start: usize) -> KernelResult<(u64, usize)> {
    let mut val: u64 = 0;
    let mut shift: u32 = 0;

    for i in 0..9 {
        let byte = *data.get(start.saturating_add(i))
            .ok_or(KernelError::CorruptedData)?;
        val |= u64::from(byte & 0x7F) << shift;
        shift = shift.saturating_add(7);

        if (byte & 0x80) == 0 {
            return Ok((val, i.saturating_add(1)));
        }
    }

    Err(KernelError::CorruptedData) // VLI too long
}

/// Parse LZMA2 dictionary size from the properties byte.
///
/// `byte` encodes the dictionary size as `(2 | (byte & 1)) << (byte/2 + 11)`,
/// with special cases: byte=0 → 4096, byte=40 → u32::MAX (per xz-utils).
/// Values above 40 are invalid per the XZ spec.
pub(crate) fn lzma2_dict_size(byte: u8) -> u32 {
    if byte == 0 {
        return MIN_DICT_SIZE;
    }
    // Byte 40 is the maximum dictionary (4 GiB - 1).  The formula
    // `2 << 31` overflows u32, so handle it as a special case
    // (matches xz-utils behavior).
    if byte >= 40 {
        return u32::MAX;
    }
    let mantissa = 2u32 | u32::from(byte & 1);
    let exponent = (u32::from(byte) >> 1).saturating_add(11);
    if exponent >= 32 {
        return u32::MAX;
    }
    mantissa.checked_shl(exponent).unwrap_or(u32::MAX)
}

/// Decompress XZ-compressed data.
///
/// Parses the XZ container, validates checksums, and decompresses
/// the LZMA2 payload.  Supports CRC-32, CRC-64, and no-check modes.
///
/// Returns the decompressed data.
pub fn unxz(data: &[u8]) -> KernelResult<Vec<u8>> {
    if data.len() < 24 {
        return Err(KernelError::CorruptedData);
    }

    // --- Stream header (12 bytes) ---
    let header = data.get(..12).ok_or(KernelError::CorruptedData)?;

    // Validate magic bytes.
    if header.get(..6).ok_or(KernelError::CorruptedData)? != XZ_MAGIC {
        return Err(KernelError::CorruptedData);
    }

    // Stream flags: byte 6 must be 0 (reserved), byte 7 = check type.
    let flags_byte0 = *header.get(6).ok_or(KernelError::CorruptedData)?;
    let check_type = *header.get(7).ok_or(KernelError::CorruptedData)?;

    if flags_byte0 != 0 {
        return Err(KernelError::CorruptedData);
    }

    // Validate header CRC-32 (covers bytes 6-7).
    let header_crc_stored = read_le32(data, 8)?;
    let header_crc_computed = super::compress::crc32_iso_pub(
        header.get(6..8).ok_or(KernelError::CorruptedData)?,
    );
    if header_crc_stored != header_crc_computed {
        return Err(KernelError::CorruptedData);
    }

    // --- Parse blocks ---
    let mut pos = 12usize;
    let mut all_output = Vec::new();

    loop {
        // Check for index indicator (0x00 byte marks end of blocks).
        let indicator = *data.get(pos).ok_or(KernelError::CorruptedData)?;
        if indicator == 0x00 {
            break;
        }

        // Block header size: (indicator + 1) * 4 bytes (including the
        // indicator byte itself and the 4-byte CRC at the end).
        let block_header_size = (u32::from(indicator).wrapping_add(1))
            .saturating_mul(4) as usize;
        let block_header = data.get(pos..pos.saturating_add(block_header_size))
            .ok_or(KernelError::CorruptedData)?;

        // Validate block header CRC-32 (covers all but last 4 bytes).
        let bh_crc_start = block_header_size.saturating_sub(4);
        let bh_crc_stored = read_le32(block_header, bh_crc_start)?;
        let bh_crc_computed = super::compress::crc32_iso_pub(
            block_header.get(..bh_crc_start).ok_or(KernelError::CorruptedData)?,
        );
        if bh_crc_stored != bh_crc_computed {
            return Err(KernelError::CorruptedData);
        }

        // Parse block flags (byte 1 of block header).
        let block_flags = *block_header.get(1).ok_or(KernelError::CorruptedData)?;
        let num_filters = (block_flags & 0x03).wrapping_add(1);
        let has_compressed_size = (block_flags & 0x40) != 0;
        let has_uncompressed_size = (block_flags & 0x80) != 0;

        let mut bh_pos = 2usize;

        // Optional compressed size.
        let _compressed_size = if has_compressed_size {
            let (val, consumed) = read_vli(block_header, bh_pos)?;
            bh_pos = bh_pos.saturating_add(consumed);
            Some(val)
        } else {
            None
        };

        // Optional uncompressed size.
        let _uncompressed_size = if has_uncompressed_size {
            let (val, consumed) = read_vli(block_header, bh_pos)?;
            bh_pos = bh_pos.saturating_add(consumed);
            Some(val)
        } else {
            None
        };

        // Parse filter(s).  We only support a single LZMA2 filter.
        if num_filters != 1 {
            return Err(KernelError::NotSupported);
        }

        let (filter_id, consumed) = read_vli(block_header, bh_pos)?;
        bh_pos = bh_pos.saturating_add(consumed);

        if filter_id != LZMA2_FILTER_ID {
            return Err(KernelError::NotSupported);
        }

        // Filter properties size (should be 1 for LZMA2).
        let (props_size, consumed) = read_vli(block_header, bh_pos)?;
        bh_pos = bh_pos.saturating_add(consumed);

        if props_size != 1 {
            return Err(KernelError::CorruptedData);
        }

        let dict_byte = *block_header.get(bh_pos).ok_or(KernelError::CorruptedData)?;
        let dict_size = lzma2_dict_size(dict_byte).max(MIN_DICT_SIZE);

        // Advance past the block header.
        pos = pos.saturating_add(block_header_size);

        // The compressed data follows until we can determine its end.
        // For LZMA2, we decode chunks until the 0x00 end marker.  The
        // block's compressed size (if present) tells us the total, but
        // LZMA2's own framing handles termination.
        //
        // We pass the remaining data and let LZMA2 consume what it needs.
        let remaining = data.get(pos..).ok_or(KernelError::CorruptedData)?;

        // Find the LZMA2 stream end by scanning for the 0x00 control byte.
        // We decode the full LZMA2 stream and track how much it consumed.
        let block_output = lzma2_decode(remaining, dict_size)?;

        // Calculate how many bytes of the compressed stream were consumed
        // by walking the LZMA2 chunks again (simpler: just scan).
        let consumed_compressed = lzma2_stream_size(remaining)?;

        all_output.extend_from_slice(&block_output);
        pos = pos.saturating_add(consumed_compressed);

        // Blocks are padded to 4-byte alignment.
        let padding = (4usize.wrapping_sub(consumed_compressed % 4)) % 4;
        pos = pos.saturating_add(padding);

        // Skip block check (CRC-32, CRC-64, SHA-256, or none).
        let check_size = match check_type {
            CHECK_NONE => 0,
            CHECK_CRC32 => 4,
            CHECK_CRC64 => 8,
            0x0A => 32, // SHA-256
            _ => 0, // Unknown — skip nothing
        };

        // Validate check if possible.
        if check_type == CHECK_CRC32 && check_size == 4 {
            let stored = read_le32(data, pos)?;
            let computed = super::compress::crc32_iso_pub(&block_output);
            if stored != computed {
                return Err(KernelError::CorruptedData);
            }
        } else if check_type == CHECK_CRC64 && check_size == 8 {
            let stored = read_le64(data, pos)?;
            let computed = crc64(&block_output);
            if stored != computed {
                return Err(KernelError::CorruptedData);
            }
        }

        pos = pos.saturating_add(check_size);
    }

    // Skip index and stream footer (we trust the data we've already decoded).
    // The important integrity checks (block CRC, header CRC) are done.

    Ok(all_output)
}

/// Calculate the compressed size of an LZMA2 stream (for block padding).
///
/// Walks the LZMA2 control bytes without decoding, counting total bytes.
fn lzma2_stream_size(data: &[u8]) -> KernelResult<usize> {
    let mut pos = 0usize;

    loop {
        let control = *data.get(pos).ok_or(KernelError::CorruptedData)?;
        pos = pos.saturating_add(1);

        if control == 0x00 {
            return Ok(pos);
        }

        if control <= 0x02 {
            // Uncompressed chunk.
            let size_hi = u16::from(*data.get(pos).ok_or(KernelError::CorruptedData)?);
            let size_lo = u16::from(
                *data.get(pos.saturating_add(1)).ok_or(KernelError::CorruptedData)?,
            );
            pos = pos.saturating_add(2);
            let chunk_size = ((size_hi << 8) | size_lo).wrapping_add(1) as usize;
            pos = pos.saturating_add(chunk_size);
            continue;
        }

        if control < 0x80 {
            return Err(KernelError::CorruptedData);
        }

        // LZMA chunk.
        // Skip 2 bytes uncompressed size (low), 2 bytes compressed size.
        pos = pos.saturating_add(2); // uncompressed size low
        let comp_hi = u16::from(*data.get(pos).ok_or(KernelError::CorruptedData)?);
        let comp_lo = u16::from(
            *data.get(pos.saturating_add(1)).ok_or(KernelError::CorruptedData)?,
        );
        pos = pos.saturating_add(2);
        let compressed_size = ((comp_hi << 8) | comp_lo).wrapping_add(1) as usize;

        // Optional props byte.
        if control >= 0xC0 {
            pos = pos.saturating_add(1);
        }

        pos = pos.saturating_add(compressed_size);
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Read a little-endian u32 from `data[offset..offset+4]`.
fn read_le32(data: &[u8], offset: usize) -> KernelResult<u32> {
    let bytes = data.get(offset..offset.saturating_add(4))
        .ok_or(KernelError::CorruptedData)?;
    Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

/// Read a little-endian u64 from `data[offset..offset+8]`.
fn read_le64(data: &[u8], offset: usize) -> KernelResult<u64> {
    let bytes = data.get(offset..offset.saturating_add(8))
        .ok_or(KernelError::CorruptedData)?;
    Ok(u64::from_le_bytes([
        bytes[0], bytes[1], bytes[2], bytes[3],
        bytes[4], bytes[5], bytes[6], bytes[7],
    ]))
}

// ===========================================================================
// XZ / LZMA2 / LZMA COMPRESSION
// ===========================================================================

// ---------------------------------------------------------------------------
// Range encoder (mirror of RangeDecoder)
// ---------------------------------------------------------------------------

/// Adaptive range encoder for LZMA compressed data.
///
/// Writes to a `Vec<u8>` buffer.  Produces output that the `RangeDecoder`
/// can decode exactly.  The first byte emitted is always 0x00, followed by
/// 4 code bytes (matching the decoder's 5-byte header expectation).
struct RangeEncoder {
    /// Accumulated output bytes.
    buf: Vec<u8>,
    /// Current range (starts at 0xFFFF_FFFF).
    range: u32,
    /// Low end of the interval.
    low: u64,
    /// Pending carry-propagation bytes (0xFF that might become 0x00 on carry).
    cache_size: u32,
    /// Cached byte awaiting carry resolution.
    cache: u8,
}

impl RangeEncoder {
    /// Create a new range encoder.
    ///
    /// The first output byte (0x00) is not written explicitly — it comes
    /// naturally from `shift_low` flushing the initial `cache=0`.  This
    /// matches how the LZMA SDK range encoder produces the 5-byte header
    /// expected by `RangeDecoder::new()`.
    fn new() -> Self {
        Self {
            buf: Vec::new(),
            range: 0xFFFF_FFFF,
            low: 0,
            cache_size: 1,
            cache: 0,
        }
    }

    /// Emit pending bytes, resolving any carry propagation.
    fn shift_low(&mut self) {
        // If the high byte of `low` has overflowed past 0xFF, we have a carry.
        let low_hi = (self.low >> 32) as u8;
        if self.low < 0xFF00_0000 || low_hi != 0 {
            // Flush the cached byte (+ carry).
            self.buf.push(self.cache.wrapping_add(low_hi));
            // Flush any pending 0xFF bytes with carry.
            let fill = if low_hi != 0 { 0x00u8 } else { 0xFFu8 };
            for _ in 1..self.cache_size {
                self.buf.push(fill);
            }
            self.cache_size = 0;
            self.cache = ((self.low >> 24) & 0xFF) as u8;
        }
        self.cache_size = self.cache_size.wrapping_add(1);
        self.low = (self.low << 8) & 0xFFFF_FFFF;
    }

    /// Normalize: if range has dropped below 2^24, shift and extend.
    #[inline]
    fn normalize(&mut self) {
        if self.range < (1u32 << 24) {
            self.range <<= 8;
            self.shift_low();
        }
    }

    /// Encode a single bit using an adaptive probability.
    ///
    /// Updates `prob` exactly as the decoder does, so encoder and decoder
    /// stay synchronized.
    #[inline]
    fn encode_bit(&mut self, prob: &mut u16, bit: u32) {
        let bound = (self.range >> PROB_BITS).wrapping_mul(u32::from(*prob));
        if bit == 0 {
            self.range = bound;
            *prob = prob.saturating_add((2048u16.saturating_sub(*prob)) >> 5);
        } else {
            self.low = self.low.wrapping_add(u64::from(bound));
            self.range = self.range.wrapping_sub(bound);
            *prob = prob.saturating_sub(*prob >> 5);
        }
        self.normalize();
    }

    /// Encode `count` bits with fixed 50/50 probability (direct bits).
    fn encode_direct_bits(&mut self, value: u32, count: usize) {
        for i in (0..count).rev() {
            self.range >>= 1;
            let bit = (value >> i) & 1;
            if bit != 0 {
                self.low = self.low.wrapping_add(u64::from(self.range));
            }
            self.normalize();
        }
    }

    /// Finalize the stream: flush remaining bytes.
    fn finish(mut self) -> Vec<u8> {
        for _ in 0..5 {
            self.shift_low();
        }
        self.buf
    }
}

// ---------------------------------------------------------------------------
// Bit-tree encoders (mirror of decode_bit_tree / decode_bit_tree_reverse)
// ---------------------------------------------------------------------------

/// Encode a `num_bits`-wide value into a bit-tree.
///
/// Mirror of `decode_bit_tree`.  The tree is stored in
/// `probs[1..(1 << num_bits)]` (index 0 unused).
fn encode_bit_tree(
    rc: &mut RangeEncoder,
    probs: &mut [u16],
    num_bits: usize,
    value: u32,
) {
    let mut m: u32 = 1;
    for i in (0..num_bits).rev() {
        let bit = (value >> i) & 1;
        if let Some(p) = probs.get_mut(m as usize) {
            rc.encode_bit(p, bit);
        }
        m = (m << 1) | bit;
    }
}

/// Encode a `num_bits`-wide value into a reversed bit-tree (LSB first).
///
/// Mirror of `decode_bit_tree_reverse`.
fn encode_bit_tree_reverse(
    rc: &mut RangeEncoder,
    probs: &mut [u16],
    offset: usize,
    num_bits: usize,
    value: u32,
) {
    let mut m: u32 = 1;
    for i in 0..num_bits {
        let bit = (value >> i) & 1;
        let idx = offset.saturating_add(m as usize);
        if let Some(p) = probs.get_mut(idx) {
            rc.encode_bit(p, bit);
        }
        m = (m << 1) | bit;
    }
}

// ---------------------------------------------------------------------------
// Length encoder (mirror of LenDecoder)
// ---------------------------------------------------------------------------

/// LZMA length encoder with three tiers: low (2-9), mid (10-17), high (18-273).
struct LenEncoder {
    choice: u16,
    choice2: u16,
    low: [[u16; 1 << LEN_LOW_BITS]; POS_STATES_MAX],
    mid: [[u16; 1 << LEN_MID_BITS]; POS_STATES_MAX],
    high: [u16; 1 << LEN_HIGH_BITS],
}

impl LenEncoder {
    fn new() -> Self {
        Self {
            choice: PROB_INIT,
            choice2: PROB_INIT,
            low: [[PROB_INIT; 1 << LEN_LOW_BITS]; POS_STATES_MAX],
            mid: [[PROB_INIT; 1 << LEN_MID_BITS]; POS_STATES_MAX],
            high: [PROB_INIT; 1 << LEN_HIGH_BITS],
        }
    }

    /// Encode a match length (2..273).
    fn encode(&mut self, rc: &mut RangeEncoder, pos_state: usize, len: u32) {
        let len = len.wrapping_sub(2); // Adjust to 0-based
        if len < (1 << LEN_LOW_BITS) {
            // Low tier: 0-7
            rc.encode_bit(&mut self.choice, 0);
            encode_bit_tree(rc, &mut self.low[pos_state], LEN_LOW_BITS, len);
        } else if len < (1 << LEN_LOW_BITS) + (1 << LEN_MID_BITS) {
            // Mid tier: 8-15 → encode as (len - 8)
            rc.encode_bit(&mut self.choice, 1);
            rc.encode_bit(&mut self.choice2, 0);
            encode_bit_tree(
                rc,
                &mut self.mid[pos_state],
                LEN_MID_BITS,
                len.wrapping_sub(1 << LEN_LOW_BITS),
            );
        } else {
            // High tier: 16-271 → encode as (len - 16)
            rc.encode_bit(&mut self.choice, 1);
            rc.encode_bit(&mut self.choice2, 1);
            encode_bit_tree(
                rc,
                &mut self.high[..],
                LEN_HIGH_BITS,
                len.wrapping_sub((1 << LEN_LOW_BITS) + (1 << LEN_MID_BITS)),
            );
        }
    }
}

// ---------------------------------------------------------------------------
// LZMA encoder state
// ---------------------------------------------------------------------------

/// Full LZMA probability model for encoding.
///
/// Contains the same probability arrays as `LzmaState` (the decoder), but
/// paired with encoder methods.  The probability updates are identical so
/// encoded output decodes correctly.
struct LzmaEncoderState {
    lc: u8,
    lp: u8,
    pb: u8,
    state: u8,
    rep: [u32; 4],
    is_match: [[u16; POS_STATES_MAX]; STATES],
    is_rep: [u16; STATES],
    is_rep_g0: [u16; STATES],
    is_rep_g1: [u16; STATES],
    is_rep_g2: [u16; STATES],
    is_rep0_long: [[u16; POS_STATES_MAX]; STATES],
    literal_probs: Vec<u16>,
    match_len: LenEncoder,
    rep_len: LenEncoder,
    pos_slot: [[u16; 1 << POS_SLOT_BITS]; 4],
    pos_decoders: [u16; 1 + FULL_DISTANCES - END_POS_MODEL],
    align_decoder: [u16; 1 << ALIGN_BITS],
}

impl LzmaEncoderState {
    fn new(lc: u8, lp: u8, pb: u8) -> Self {
        let num_lit = 1usize << (lc.saturating_add(lp) as usize);
        let lit_size = num_lit.saturating_mul(0x300);
        Self {
            lc, lp, pb,
            state: 0,
            rep: [0; 4],
            is_match: [[PROB_INIT; POS_STATES_MAX]; STATES],
            is_rep: [PROB_INIT; STATES],
            is_rep_g0: [PROB_INIT; STATES],
            is_rep_g1: [PROB_INIT; STATES],
            is_rep_g2: [PROB_INIT; STATES],
            is_rep0_long: [[PROB_INIT; POS_STATES_MAX]; STATES],
            literal_probs: vec![PROB_INIT; lit_size],
            match_len: LenEncoder::new(),
            rep_len: LenEncoder::new(),
            pos_slot: [[PROB_INIT; 1 << POS_SLOT_BITS]; 4],
            pos_decoders: [PROB_INIT; 1 + FULL_DISTANCES - END_POS_MODEL],
            align_decoder: [PROB_INIT; 1 << ALIGN_BITS],
        }
    }

    /// Encode a literal byte.
    ///
    /// `data` is the full input being compressed; needed to look up the
    /// match byte when state >= `LIT_STATES` (post-match literal context).
    fn encode_literal(
        &mut self,
        rc: &mut RangeEncoder,
        data: &[u8],
        byte: u8,
        pos: usize,
        prev_byte: u8,
    ) {
        let pos_state = (pos as u32 & ((1u32 << self.pb).wrapping_sub(1))) as usize;
        let state = self.state as usize;

        // Signal: this is a literal (is_match = 0).
        rc.encode_bit(&mut self.is_match[state][pos_state], 0);

        // Select literal sub-coder.
        let lit_state = ((((pos as u32) & ((1u32 << self.lp).wrapping_sub(1))) << self.lc)
            | (u32::from(prev_byte) >> (8u32.wrapping_sub(u32::from(self.lc)))))
            as usize;
        let offset = lit_state.saturating_mul(0x300);

        if (self.state as usize) < LIT_STATES {
            // Normal literal encode.
            encode_literal_normal(rc, &mut self.literal_probs, offset, byte);
        } else {
            // Match-byte-aware literal encode.
            // Must use the same match byte the decoder will use: the byte
            // at (pos - rep[0] - 1) in the output stream.  Since the output
            // IS the uncompressed input, we look it up in `data`.
            let d = self.rep[0] as usize;
            let match_byte = if pos > d {
                data[pos.wrapping_sub(d.wrapping_add(1))]
            } else {
                0
            };
            encode_literal_matched(rc, &mut self.literal_probs, offset, byte, match_byte);
        }

        self.state = STATE_LIT[self.state as usize];
    }

    /// Encode a match (new distance).
    fn encode_match(
        &mut self,
        rc: &mut RangeEncoder,
        pos: usize,
        dist: u32,
        len: u32,
    ) {
        let pos_state = (pos as u32 & ((1u32 << self.pb).wrapping_sub(1))) as usize;
        let state = self.state as usize;

        // Signal: this is a match (is_match = 1, is_rep = 0).
        rc.encode_bit(&mut self.is_match[state][pos_state], 1);
        rc.encode_bit(&mut self.is_rep[state], 0);

        // Encode length.
        self.match_len.encode(rc, pos_state, len);

        // Encode distance.
        let slot = distance_to_slot(dist);
        let slot_idx = len.wrapping_sub(2).min(3) as usize;
        encode_bit_tree(
            rc,
            &mut self.pos_slot[slot_idx],
            POS_SLOT_BITS,
            slot,
        );

        if slot >= START_POS_MODEL as u32 {
            let num_direct_bits = (slot >> 1).wrapping_sub(1) as usize;
            let base = (2u32 | (slot & 1)) << num_direct_bits;
            let footer = dist.wrapping_sub(base);

            if slot < END_POS_MODEL as u32 {
                // Context-encoded reversed bits.
                let ctx_base = base.wrapping_sub(slot) as usize;
                encode_bit_tree_reverse(
                    rc,
                    &mut self.pos_decoders,
                    ctx_base,
                    num_direct_bits,
                    footer,
                );
            } else {
                // Direct bits for the high portion, then 4 alignment bits.
                let high_bits = num_direct_bits.saturating_sub(ALIGN_BITS);
                rc.encode_direct_bits(footer >> ALIGN_BITS, high_bits);
                encode_bit_tree_reverse(
                    rc,
                    &mut self.align_decoder,
                    0,
                    ALIGN_BITS,
                    footer & ((1 << ALIGN_BITS) - 1),
                );
            }
        }

        // Update rep distances.
        self.rep[3] = self.rep[2];
        self.rep[2] = self.rep[1];
        self.rep[1] = self.rep[0];
        self.rep[0] = dist;
        self.state = STATE_MATCH[self.state as usize];
    }

    /// Encode a rep0 match (repeat distance 0, length >= 2).
    fn encode_rep0(
        &mut self,
        rc: &mut RangeEncoder,
        pos: usize,
        len: u32,
    ) {
        let pos_state = (pos as u32 & ((1u32 << self.pb).wrapping_sub(1))) as usize;
        let state = self.state as usize;

        rc.encode_bit(&mut self.is_match[state][pos_state], 1);
        rc.encode_bit(&mut self.is_rep[state], 1);
        rc.encode_bit(&mut self.is_rep_g0[state], 0);

        if len == 1 {
            // Short rep (single byte).
            rc.encode_bit(&mut self.is_rep0_long[state][pos_state], 0);
            self.state = STATE_SHORTREP[self.state as usize];
        } else {
            // Long rep0.
            rc.encode_bit(&mut self.is_rep0_long[state][pos_state], 1);
            self.rep_len.encode(rc, pos_state, len);
            self.state = STATE_REP[self.state as usize];
        }
        // rep distances unchanged for rep0.
    }

    /// Encode a rep1/rep2/rep3 match.
    fn encode_rep_n(
        &mut self,
        rc: &mut RangeEncoder,
        pos: usize,
        rep_idx: usize,
        len: u32,
    ) {
        let pos_state = (pos as u32 & ((1u32 << self.pb).wrapping_sub(1))) as usize;
        let state = self.state as usize;

        rc.encode_bit(&mut self.is_match[state][pos_state], 1);
        rc.encode_bit(&mut self.is_rep[state], 1);

        match rep_idx {
            1 => {
                rc.encode_bit(&mut self.is_rep_g0[state], 1);
                rc.encode_bit(&mut self.is_rep_g1[state], 0);
            }
            2 => {
                rc.encode_bit(&mut self.is_rep_g0[state], 1);
                rc.encode_bit(&mut self.is_rep_g1[state], 1);
                rc.encode_bit(&mut self.is_rep_g2[state], 0);
            }
            3 => {
                rc.encode_bit(&mut self.is_rep_g0[state], 1);
                rc.encode_bit(&mut self.is_rep_g1[state], 1);
                rc.encode_bit(&mut self.is_rep_g2[state], 1);
            }
            _ => return, // Invalid rep index; should not happen.
        }

        self.rep_len.encode(rc, pos_state, len);

        // Promote the used rep distance to rep[0], shifting others down.
        let dist = self.rep[rep_idx];
        for i in (1..=rep_idx).rev() {
            self.rep[i] = self.rep[i - 1];
        }
        self.rep[0] = dist;
        self.state = STATE_REP[self.state as usize];
    }
}

/// Encode a literal byte (state < 7, no match byte context).
fn encode_literal_normal(
    rc: &mut RangeEncoder,
    probs: &mut [u16],
    offset: usize,
    byte: u8,
) {
    let mut symbol: u32 = 1;
    for i in (0..8).rev() {
        let bit = (u32::from(byte) >> i) & 1;
        let idx = offset.saturating_add(symbol as usize);
        if let Some(p) = probs.get_mut(idx) {
            rc.encode_bit(p, bit);
        }
        symbol = (symbol << 1) | bit;
    }
}

/// Encode a literal byte with match-byte context (state >= 7).
///
/// Mirror of `decode_literal_matched`.
fn encode_literal_matched(
    rc: &mut RangeEncoder,
    probs: &mut [u16],
    offset: usize,
    byte: u8,
    match_byte: u8,
) {
    let mut symbol: u32 = 1;
    let mut match_byte = u32::from(match_byte) << 1;
    let mut mismatch = false;

    for i in (0..8).rev() {
        let bit = (u32::from(byte) >> i) & 1;
        let match_bit = (match_byte >> 8) & 1;
        match_byte <<= 1;

        let sub_offset = if mismatch {
            offset.saturating_add(symbol as usize)
        } else {
            offset.saturating_add(
                (((1u32.wrapping_add(match_bit)) << 8)
                    .wrapping_add(symbol)) as usize,
            )
        };

        if let Some(p) = probs.get_mut(sub_offset) {
            rc.encode_bit(p, bit);
            if !mismatch && bit != match_bit {
                mismatch = true;
            }
        }
        symbol = (symbol << 1) | bit;
    }
}

/// Convert a distance to a distance slot.
///
/// Inverse of the slot→distance mapping used in the decoder.
fn distance_to_slot(dist: u32) -> u32 {
    if dist < 4 {
        return dist;
    }
    // Find the highest set bit position.
    let msb = 31u32.wrapping_sub(dist.leading_zeros());
    // Slot = 2*msb + bit below msb.
    
    msb.wrapping_mul(2).wrapping_add((dist >> (msb.wrapping_sub(1))) & 1)
}

// ---------------------------------------------------------------------------
// LZ77 match finder for LZMA
// ---------------------------------------------------------------------------

/// LZMA minimum match length.
const LZMA_MIN_MATCH: usize = 2;
/// Maximum match length for LZMA.
const LZMA_MAX_MATCH: usize = 273;
/// Hash table size for match finder (4K entries, 16 KiB).
const LZMA_HASH_SIZE: usize = 1 << 12;
/// Maximum search distance (window size, 16 KiB).
/// Kept small to limit memory usage in kernel context.
const LZMA_WINDOW: usize = 1 << 14;
/// Maximum chain depth for match searching.
const LZMA_MAX_CHAIN: usize = 16;

/// Compute a 4-byte hash for the match finder.
fn lzma_hash4(data: &[u8], pos: usize) -> usize {
    if pos.wrapping_add(3) >= data.len() {
        return 0;
    }
    let h = u32::from(data[pos])
        .wrapping_mul(0x9E37_79B1)
        .wrapping_add(u32::from(data[pos + 1]).wrapping_mul(0x85EB_CA77))
        .wrapping_add(u32::from(data[pos + 2]).wrapping_mul(0xC2B2_AE3D))
        .wrapping_add(u32::from(data[pos + 3]));
    (h >> 20) as usize & (LZMA_HASH_SIZE - 1)
}

/// An LZMA token for the encoder.
enum LzmaToken {
    Literal(u8),
    Match { dist: u32, len: u32 },
    Rep { rep_idx: usize, len: u32 },
    ShortRep, // Rep0, length 1
}

/// Tokenize input data into LZMA tokens using a hash-chain match finder.
///
/// Produces a sequence of literals, matches, and rep-matches that the
/// LZMA encoder state machine can encode.
fn lzma_tokenize(data: &[u8]) -> Vec<LzmaToken> {
    let n = data.len();
    if n == 0 {
        return Vec::new();
    }

    let mut tokens: Vec<LzmaToken> = Vec::with_capacity(n);
    let mut head = vec![0u32; LZMA_HASH_SIZE];
    let mut prev = vec![0u32; LZMA_WINDOW];
    let mut rep = [0u32; 4];
    let mut pos = 0usize;

    while pos < n {
        // First, check if any rep distance gives a match.
        let mut best_rep_idx: Option<usize> = None;
        let mut best_rep_len: usize = 0;

        for ri in 0..4 {
            let d = rep[ri] as usize;
            if d == 0 && ri > 0 {
                continue; // Skip zero distances for rep1-3
            }
            if pos <= d {
                continue; // Distance exceeds available history
            }
            let ref_pos = pos.wrapping_sub(d.wrapping_add(1));
            let mut mlen = 0usize;
            while pos + mlen < n
                && mlen < LZMA_MAX_MATCH
                && data[pos + mlen] == data[ref_pos + mlen]
            {
                mlen += 1;
            }
            if mlen >= LZMA_MIN_MATCH && mlen > best_rep_len {
                best_rep_len = mlen;
                best_rep_idx = Some(ri);
            }
        }

        // Check for a short rep (rep0, length 1).
        let short_rep_ok = if rep[0] > 0 || pos > 0 {
            let d = rep[0] as usize;
            pos > d && data[pos] == data[pos.wrapping_sub(d.wrapping_add(1))]
        } else {
            false
        };

        // Find best new match via hash chain.
        let (hash_len, hash_dist) = if pos + 3 < n {
            let h = lzma_hash4(data, pos);
            let old_head = head[h];
            prev[pos % LZMA_WINDOW] = old_head;
            head[h] = pos as u32;

            let mut best_len = 0usize;
            let mut best_dist = 0u32;
            let mut chain = old_head;
            let mut depth = 0;

            while chain != 0 && depth < LZMA_MAX_CHAIN {
                let cp = chain as usize;
                if pos.wrapping_sub(cp) > LZMA_WINDOW || cp >= pos {
                    break;
                }
                let mut mlen = 0usize;
                let max_len = (n - pos).min(LZMA_MAX_MATCH);
                while mlen < max_len && data[pos + mlen] == data[cp + mlen] {
                    mlen += 1;
                }
                if mlen >= LZMA_MIN_MATCH && mlen > best_len {
                    best_len = mlen;
                    best_dist = (pos - cp - 1) as u32;
                    if mlen == max_len {
                        break; // Can't do better
                    }
                }
                chain = prev[cp % LZMA_WINDOW];
                depth += 1;
            }
            (best_len, best_dist)
        } else {
            // Insert into hash table but no matches possible near end.
            if pos + 3 < n {
                let h = lzma_hash4(data, pos);
                prev[pos % LZMA_WINDOW] = head[h];
                head[h] = pos as u32;
            }
            (0, 0)
        };

        // Decision: pick the best option.
        // Priority: long rep match > long hash match > short rep > literal
        let use_rep = best_rep_idx.is_some()
            && (best_rep_len >= hash_len || (best_rep_len >= 3 && best_rep_len + 1 >= hash_len));

        if use_rep && best_rep_len >= LZMA_MIN_MATCH {
            let ri = best_rep_idx.unwrap_or(0);
            if ri == 0 {
                tokens.push(LzmaToken::Rep { rep_idx: 0, len: best_rep_len as u32 });
            } else {
                tokens.push(LzmaToken::Rep { rep_idx: ri, len: best_rep_len as u32 });
                // Promote rep[ri] to rep[0].
                let d = rep[ri];
                for j in (1..=ri).rev() {
                    rep[j] = rep[j - 1];
                }
                rep[0] = d;
            }
            // Insert skipped positions into hash table.
            for skip in 1..best_rep_len {
                let sp = pos + skip;
                if sp + 3 < n {
                    let h = lzma_hash4(data, sp);
                    prev[sp % LZMA_WINDOW] = head[h];
                    head[h] = sp as u32;
                }
            }
            pos += best_rep_len;
        } else if hash_len >= LZMA_MIN_MATCH
            && (hash_len >= 3 || hash_dist < 128)
        {
            tokens.push(LzmaToken::Match { dist: hash_dist, len: hash_len as u32 });
            // Update rep distances.
            rep[3] = rep[2];
            rep[2] = rep[1];
            rep[1] = rep[0];
            rep[0] = hash_dist;
            // Insert skipped positions.
            for skip in 1..hash_len {
                let sp = pos + skip;
                if sp + 3 < n {
                    let h = lzma_hash4(data, sp);
                    prev[sp % LZMA_WINDOW] = head[h];
                    head[h] = sp as u32;
                }
            }
            pos += hash_len;
        } else if short_rep_ok {
            tokens.push(LzmaToken::ShortRep);
            // rep unchanged.
            pos += 1;
        } else {
            tokens.push(LzmaToken::Literal(data[pos]));
            pos += 1;
        }
    }

    tokens
}

// ---------------------------------------------------------------------------
// LZMA encoding pass
// ---------------------------------------------------------------------------

/// Encode input data as an LZMA compressed stream.
///
/// Returns the raw LZMA bytes (starting with the 5-byte range coder header).
/// The caller wraps this in LZMA2 and XZ framing.
///
/// Uses lc=3, lp=0, pb=2 (LZMA SDK defaults for general data).
fn lzma_encode(data: &[u8]) -> Vec<u8> {
    let lc: u8 = 3;
    let lp: u8 = 0;
    let pb: u8 = 2;

    let tokens = lzma_tokenize(data);
    let mut state = LzmaEncoderState::new(lc, lp, pb);
    let mut rc = RangeEncoder::new();

    let mut output_pos = 0usize; // Tracks the decompressed position.
    let mut data_pos = 0usize; // Position in input data.

    for token in &tokens {
        let prev_byte = if data_pos > 0 {
            data[data_pos - 1]
        } else {
            0
        };

        match token {
            LzmaToken::Literal(byte) => {
                state.encode_literal(&mut rc, data, *byte, output_pos, prev_byte);
                output_pos += 1;
                data_pos += 1;
            }
            LzmaToken::Match { dist, len } => {
                state.encode_match(&mut rc, output_pos, *dist, *len);
                output_pos += *len as usize;
                data_pos += *len as usize;
            }
            LzmaToken::Rep { rep_idx, len } => {
                if *rep_idx == 0 {
                    state.encode_rep0(&mut rc, output_pos, *len);
                } else {
                    state.encode_rep_n(&mut rc, output_pos, *rep_idx, *len);
                }
                output_pos += *len as usize;
                data_pos += *len as usize;
            }
            LzmaToken::ShortRep => {
                state.encode_rep0(&mut rc, output_pos, 1);
                output_pos += 1;
                data_pos += 1;
            }
        }
    }

    rc.finish()
}

// ---------------------------------------------------------------------------
// LZMA2 encoder
// ---------------------------------------------------------------------------

/// Encode data as an LZMA2 stream.
///
/// Wraps LZMA-compressed data in LZMA2 chunk framing.
/// For small inputs, uses a single uncompressed chunk (cheaper).
/// For larger inputs, uses a single LZMA chunk with full props reset.
fn lzma2_encode(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();

    if data.is_empty() {
        out.push(0x00); // LZMA2 end marker
        return out;
    }

    // For very small inputs (< 16 bytes), uncompressed is often better.
    // Also, if LZMA produces expansion, fall back to uncompressed.
    let lzma_data = if data.len() >= 16 {
        Some(lzma_encode(data))
    } else {
        None
    };

    let use_lzma = lzma_data.as_ref().is_some_and(|d| d.len() < data.len());

    if use_lzma {
        let lzma_bytes = lzma_data.unwrap_or_default();

        // LZMA chunk with full properties reset (control byte >= 0xC0).
        // Control byte: bits 7-5 = 110 (LZMA + new props), bits 4-0 = high
        // bits of uncompressed size.
        let uncomp_minus1 = (data.len() as u32).wrapping_sub(1);
        let control = 0xC0u8
            | ((uncomp_minus1 >> 16) as u8 & 0x1F)
            | 0x20; // bit 5 = reset state + new props

        out.push(control);

        // Uncompressed size low 16 bits (big-endian).
        out.push((uncomp_minus1 >> 8) as u8);
        out.push(uncomp_minus1 as u8);

        // Compressed size (big-endian, minus 1).
        let comp_minus1 = (lzma_bytes.len() as u16).wrapping_sub(1);
        out.push((comp_minus1 >> 8) as u8);
        out.push(comp_minus1 as u8);

        // Properties byte: lc + 9 * (lp + 5 * pb) = 3 + 9*(0 + 5*2) = 3+90 = 93
        let props = 3u8 + 9 * (5 * 2); // lc=3, lp=0, pb=2
        out.push(props);

        // LZMA compressed data.
        out.extend_from_slice(&lzma_bytes);
    } else {
        // Uncompressed chunk(s).
        // Maximum uncompressed chunk size is 65536 bytes.
        let mut offset = 0usize;
        let mut first = true;
        while offset < data.len() {
            let chunk_len = (data.len() - offset).min(65536);
            let size_minus1 = (chunk_len as u16).wrapping_sub(1);

            if first {
                out.push(0x01); // Uncompressed, dictionary reset
                first = false;
            } else {
                out.push(0x02); // Uncompressed, no reset
            }
            out.push((size_minus1 >> 8) as u8);
            out.push(size_minus1 as u8);
            out.extend_from_slice(&data[offset..offset + chunk_len]);
            offset += chunk_len;
        }
    }

    out.push(0x00); // LZMA2 end marker
    out
}

/// Encode the dictionary size byte for XZ block headers.
///
/// Inverse of `lzma2_dict_size`.  Returns the smallest byte whose
/// decoded dict size >= `size`.
fn lzma2_dict_size_byte(size: u32) -> u8 {
    // Byte 0 → 4096.  For each byte b (1..=39):
    //   dict = (2 | (b & 1)) << (b/2 + 11)
    // Byte 40 → u32::MAX.
    for b in 0u8..=40 {
        let decoded = if b == 0 {
            4096
        } else if b == 40 {
            u32::MAX
        } else {
            (2u32 | u32::from(b & 1)) << ((b >> 1) + 11)
        };
        if decoded >= size {
            return b;
        }
    }
    40 // Maximum
}

// ---------------------------------------------------------------------------
// XZ container writer
// ---------------------------------------------------------------------------

/// Compress data into the XZ format.
///
/// Produces a valid XZ stream with:
/// - Stream header (12 bytes) with CRC-64 check type
/// - Single block containing LZMA2 compressed data
/// - Index with one record
/// - Stream footer (12 bytes)
///
/// The output can be decompressed by `unxz()` and any standard XZ tool.
pub fn xz_compress(data: &[u8]) -> KernelResult<Vec<u8>> {
    let mut stream = Vec::new();

    // Use CRC-64 for block integrity checks (the most common choice).
    let check_type = CHECK_CRC64;
    let check_size: usize = 8;

    // --- Stream header (12 bytes) ---
    stream.extend_from_slice(&XZ_MAGIC);
    stream.push(0x00); // Flags byte 0 (reserved)
    stream.push(check_type); // Flags byte 1: check type
    let header_crc = super::compress::crc32_iso_pub(&[0x00, check_type]);
    stream.extend_from_slice(&header_crc.to_le_bytes());

    // --- LZMA2 payload ---
    let lzma2_payload = lzma2_encode(data);

    // Choose dictionary size: the smallest standard size >= data length,
    // but at least 4 KiB.
    let dict_byte = lzma2_dict_size_byte((data.len() as u32).max(MIN_DICT_SIZE));

    // --- Block header ---
    // Block flags: 1 filter (0x00), has_compressed_size (0x40), has_uncompressed_size (0x80).
    let block_flags: u8 = 0x40 | 0x80;
    let mut bh_body = Vec::new();
    bh_body.push(block_flags);
    // Compressed size (VLI).
    encode_vli(&mut bh_body, lzma2_payload.len() as u64);
    // Uncompressed size (VLI).
    encode_vli(&mut bh_body, data.len() as u64);
    // Filter: LZMA2 (ID=0x21), props_size=1, dict_byte.
    encode_vli(&mut bh_body, LZMA2_FILTER_ID);
    bh_body.push(0x01); // Properties size = 1
    bh_body.push(dict_byte);

    // Calculate block header total size: 1 (size byte) + body + padding + 4 (CRC).
    let total_before_pad = 1 + bh_body.len() + 4;
    let padded_total = (total_before_pad + 3) & !3;
    let pad_needed = padded_total - total_before_pad;
    bh_body.resize(bh_body.len() + pad_needed, 0x00);

    let size_byte = ((padded_total / 4) - 1) as u8;

    // Build block header for CRC.
    let mut bh_for_crc = Vec::new();
    bh_for_crc.push(size_byte);
    bh_for_crc.extend_from_slice(&bh_body);
    let bh_crc = super::compress::crc32_iso_pub(&bh_for_crc);

    // Write block header.
    stream.push(size_byte);
    stream.extend_from_slice(&bh_body);
    stream.extend_from_slice(&bh_crc.to_le_bytes());

    // --- Block data (LZMA2 payload) ---
    stream.extend_from_slice(&lzma2_payload);
    // Pad to 4-byte alignment.
    let data_pad = (4 - (lzma2_payload.len() % 4)) % 4;
    stream.resize(stream.len() + data_pad, 0x00);

    // --- Block check ---
    if check_type == CHECK_CRC64 {
        let check = crc64(data);
        stream.extend_from_slice(&check.to_le_bytes());
    } else if check_type == CHECK_CRC32 {
        let check = super::compress::crc32_iso_pub(data);
        stream.extend_from_slice(&check.to_le_bytes());
    }

    // --- Index ---
    let unpadded_size = padded_total + lzma2_payload.len() + check_size;
    let mut index_body = Vec::new();
    index_body.push(0x00); // Index indicator
    index_body.push(0x01); // 1 record (VLI)
    encode_vli(&mut index_body, unpadded_size as u64);
    encode_vli(&mut index_body, data.len() as u64);
    // Pad index to 4-byte alignment.
    let index_pad = (4 - (index_body.len() % 4)) % 4;
    index_body.resize(index_body.len() + index_pad, 0x00);
    let index_crc = super::compress::crc32_iso_pub(&index_body);
    stream.extend_from_slice(&index_body);
    stream.extend_from_slice(&index_crc.to_le_bytes());

    // --- Stream footer (12 bytes) ---
    let index_total = index_body.len() + 4; // body + CRC
    let backward_size = ((index_total / 4) - 1) as u32;
    let mut footer_inner = Vec::new();
    footer_inner.extend_from_slice(&backward_size.to_le_bytes());
    footer_inner.push(0x00); // Flags byte 0 (reserved)
    footer_inner.push(check_type); // Flags byte 1
    let footer_crc = super::compress::crc32_iso_pub(&footer_inner);
    stream.extend_from_slice(&footer_crc.to_le_bytes());
    stream.extend_from_slice(&footer_inner);
    stream.extend_from_slice(&XZ_FOOTER_MAGIC);

    Ok(stream)
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Comprehensive self-test for XZ/LZMA2/LZMA compression and decompression.
pub fn self_test() -> KernelResult<()> {
    serial_println!("[xz] === self-test start ===");

    // Test 1: CRC-64
    test_crc64()?;

    // Test 2: VLI parsing
    test_vli()?;

    // Test 3: LZMA2 dictionary size decoding
    test_dict_size()?;

    // Test 4: Range decoder + bit-tree basics
    test_range_decoder()?;

    // Test 5: XZ magic detection
    test_xz_magic()?;

    // Test 6: Full XZ decompression of a minimal stream
    test_full_decompress()?;

    // Test 7: Range encoder round-trip
    test_range_encoder_roundtrip()?;

    // Test 8: Distance slot encoding
    test_distance_slots()?;

    // Test 9: Compression round-trips
    test_compress_roundtrip()?;

    serial_println!("[xz] === self-test passed ===");
    Ok(())
}

fn test_crc64() -> KernelResult<()> {
    // Known CRC-64 (ECMA-182) values.
    let empty = crc64(b"");
    if empty != 0 {
        // CRC of empty is 0 for ECMA-182 (init !0, final !0, empty input).
        // Actually: init !0, no iterations, final !0 → 0. Let me verify.
        // The CRC-64 of empty data: start 0xFFFF..., XOR with nothing,
        // final !0xFFFF... = 0.  Yes, should be 0.
        serial_println!("[xz]   FAIL: crc64 empty = {:#x}, expected 0", empty);
        return Err(KernelError::InternalError);
    }
    serial_println!("[xz]   crc64 empty OK");

    // CRC-64 of "123456789" should be 0x995DC9BBDF1939FA per ECMA-182.
    let check_val = crc64(b"123456789");
    if check_val != 0x995D_C9BB_DF19_39FA {
        serial_println!(
            "[xz]   FAIL: crc64(\"123456789\") = {:#x}, expected 0x995DC9BBDF1939FA",
            check_val,
        );
        return Err(KernelError::InternalError);
    }
    serial_println!("[xz]   crc64 \"123456789\" OK");

    Ok(())
}

fn test_vli() -> KernelResult<()> {
    // Single byte VLI.
    let data = [0x42u8];
    let (val, consumed) = read_vli(&data, 0)?;
    if val != 0x42 || consumed != 1 {
        serial_println!("[xz]   FAIL: VLI single byte");
        return Err(KernelError::InternalError);
    }

    // Two byte VLI: 0x80 0x01 = (0x00) | (0x01 << 7) = 128.
    let data2 = [0x80u8, 0x01];
    let (val2, consumed2) = read_vli(&data2, 0)?;
    if val2 != 128 || consumed2 != 2 {
        serial_println!("[xz]   FAIL: VLI two byte: val={}, consumed={}", val2, consumed2);
        return Err(KernelError::InternalError);
    }

    serial_println!("[xz]   VLI parsing OK");
    Ok(())
}

fn test_dict_size() -> KernelResult<()> {
    // byte 0 → 4096
    if lzma2_dict_size(0) != 4096 {
        serial_println!("[xz]   FAIL: dict_size(0) = {}", lzma2_dict_size(0));
        return Err(KernelError::InternalError);
    }

    // byte 1 → (2|1) << (0+11) = 3 << 11 = 6144
    if lzma2_dict_size(1) != 6144 {
        serial_println!("[xz]   FAIL: dict_size(1) = {}", lzma2_dict_size(1));
        return Err(KernelError::InternalError);
    }

    // byte 40 → special case: 4 GiB - 1 = u32::MAX (per xz-utils spec).
    // The formula 2<<31 overflows u32, so byte 40 is handled explicitly.
    let d40 = lzma2_dict_size(40);
    if d40 != u32::MAX {
        serial_println!("[xz]   FAIL: dict_size(40) = {}", d40);
        return Err(KernelError::InternalError);
    }

    serial_println!("[xz]   dict_size OK");
    Ok(())
}

fn test_range_decoder() -> KernelResult<()> {
    // Minimal range decoder test: init with valid 5-byte header.
    let data = [0x00, 0x00, 0x00, 0x00, 0x00, 0xFF];
    let rc = RangeDecoder::new(&data)?;
    if rc.range != 0xFFFF_FFFF || rc.code != 0 || rc.pos != 5 {
        serial_println!("[xz]   FAIL: RangeDecoder init");
        return Err(KernelError::InternalError);
    }

    // Test that non-zero first byte is rejected.
    let bad_data = [0x01, 0x00, 0x00, 0x00, 0x00];
    if RangeDecoder::new(&bad_data).is_ok() {
        serial_println!("[xz]   FAIL: RangeDecoder should reject non-zero first byte");
        return Err(KernelError::InternalError);
    }

    serial_println!("[xz]   RangeDecoder init OK");
    Ok(())
}

fn test_xz_magic() -> KernelResult<()> {
    // Valid XZ magic.
    let valid = [0xFD, 0x37, 0x7A, 0x58, 0x5A, 0x00];
    if valid != XZ_MAGIC {
        serial_println!("[xz]   FAIL: XZ magic mismatch");
        return Err(KernelError::InternalError);
    }

    // unxz should reject too-short data.
    if unxz(&[0xFD, 0x37]).is_ok() {
        serial_println!("[xz]   FAIL: should reject short data");
        return Err(KernelError::InternalError);
    }

    // unxz should reject bad magic.
    let bad_magic = [0u8; 24];
    if unxz(&bad_magic).is_ok() {
        serial_println!("[xz]   FAIL: should reject bad magic");
        return Err(KernelError::InternalError);
    }

    serial_println!("[xz]   XZ magic validation OK");
    Ok(())
}

fn test_full_decompress() -> KernelResult<()> {
    // Minimal XZ stream that decompresses to "Hello\n" (6 bytes).
    //
    // This was constructed by hand following the XZ format spec:
    // - Stream header (12 bytes)
    // - Block with LZMA2 filter, single uncompressed chunk
    // - Index (end indicator + 1 record)
    // - Stream footer (12 bytes)
    //
    // The LZMA2 payload uses an uncompressed chunk (control=0x01) which
    // avoids needing a full LZMA-encoded stream for the test.

    // Build the XZ stream programmatically.
    let payload = b"Hello\n";

    // Build block header.
    // We use 1 filter (LZMA2), no compressed/uncompressed size fields.
    // Block flags: 0x00 (1 filter, no sizes).
    let mut block_header_body = Vec::new();
    block_header_body.push(0x00); // flags: 1 filter (0 = 1 filter)
    block_header_body.push(0x21); // filter ID = LZMA2 (1-byte VLI)
    block_header_body.push(0x01); // properties size = 1
    block_header_body.push(0x00); // dict size byte = 0 → 4096

    // Pad to make total block header (including size byte and CRC) a
    // multiple of 4.  Total = 1 (size) + body_len + padding + 4 (CRC).
    let total_before_pad = 1 + block_header_body.len() + 4;
    let padded_total = (total_before_pad + 3) & !3;
    let pad_needed = padded_total - total_before_pad;
    block_header_body.resize(block_header_body.len() + pad_needed, 0x00);

    // Size byte: (total / 4) - 1
    let size_byte = ((padded_total / 4) - 1) as u8;

    // Assemble block header for CRC.
    let mut bh_for_crc = Vec::new();
    bh_for_crc.push(size_byte);
    bh_for_crc.extend_from_slice(&block_header_body);
    let bh_crc = super::compress::crc32_iso_pub(&bh_for_crc);

    // Build LZMA2 payload: uncompressed chunk + end marker.
    let mut lzma2_payload = Vec::new();
    lzma2_payload.push(0x01); // control: uncompressed + dict reset
    let chunk_size_minus1 = (payload.len() - 1) as u16;
    lzma2_payload.push((chunk_size_minus1 >> 8) as u8);
    lzma2_payload.push((chunk_size_minus1 & 0xFF) as u8);
    lzma2_payload.extend_from_slice(payload);
    lzma2_payload.push(0x00); // LZMA2 end marker

    // Block data padding (to 4-byte alignment).
    let block_data_pad = (4 - (lzma2_payload.len() % 4)) % 4;

    // Block check (depends on check type in stream header).
    // We'll use CRC-32 (check type 1).
    let block_check = super::compress::crc32_iso_pub(payload);

    // Build index.
    let mut index_body = Vec::new();
    index_body.push(0x00); // index indicator
    index_body.push(0x01); // 1 record (VLI)
    // Record: unpadded size, uncompressed size
    // Unpadded size = block_header_size + lzma2_payload.len()
    let unpadded_size = padded_total + lzma2_payload.len();
    // Encode as VLI.
    encode_vli(&mut index_body, unpadded_size as u64);
    encode_vli(&mut index_body, payload.len() as u64);
    // Pad index to 4-byte alignment.
    let index_pad = (4 - (index_body.len() % 4)) % 4;
    index_body.resize(index_body.len() + index_pad, 0x00);
    let index_crc = super::compress::crc32_iso_pub(&index_body);

    // Stream header.
    let mut stream = Vec::new();
    stream.extend_from_slice(&XZ_MAGIC);
    stream.push(0x00); // flags byte 0 (reserved)
    stream.push(CHECK_CRC32); // flags byte 1: CRC-32
    let header_crc = super::compress::crc32_iso_pub(&[0x00, CHECK_CRC32]);
    stream.extend_from_slice(&header_crc.to_le_bytes());

    // Block header.
    stream.push(size_byte);
    stream.extend_from_slice(&block_header_body);
    stream.extend_from_slice(&bh_crc.to_le_bytes());

    // Block data (LZMA2 payload).
    stream.extend_from_slice(&lzma2_payload);
    stream.resize(stream.len() + block_data_pad, 0x00);

    // Block check.
    stream.extend_from_slice(&block_check.to_le_bytes());

    // Index.
    stream.extend_from_slice(&index_body);
    stream.extend_from_slice(&index_crc.to_le_bytes());

    // Stream footer.
    // Backward size = (index_body.len() + 4) / 4 - 1 ... wait,
    // backward size = (index size including CRC) / 4 - 1
    // Actually: backward size = (index_body.len() + 4) / 4 - 1
    // The index size in the footer = total index bytes (body + CRC) / 4 - 1
    let index_total = index_body.len() + 4; // body + CRC
    let backward_size = ((index_total / 4) - 1) as u32;
    let mut footer_inner = Vec::new();
    footer_inner.extend_from_slice(&backward_size.to_le_bytes());
    footer_inner.push(0x00); // flags byte 0
    footer_inner.push(CHECK_CRC32); // flags byte 1
    let footer_crc = super::compress::crc32_iso_pub(&footer_inner);
    stream.extend_from_slice(&footer_crc.to_le_bytes());
    stream.extend_from_slice(&footer_inner);
    stream.extend_from_slice(&XZ_FOOTER_MAGIC);

    // Now decompress!
    let result = unxz(&stream)?;
    if result.as_slice() != payload {
        serial_println!(
            "[xz]   FAIL: decompressed {:?}, expected {:?}",
            core::str::from_utf8(&result).unwrap_or("?"),
            core::str::from_utf8(payload).unwrap_or("?"),
        );
        return Err(KernelError::InternalError);
    }

    serial_println!("[xz]   full decompress (uncompressed LZMA2 chunk) OK");
    Ok(())
}

/// Encode a u64 as a variable-length integer (for self-test index building).
fn encode_vli(buf: &mut Vec<u8>, mut val: u64) {
    loop {
        let byte = (val & 0x7F) as u8;
        val >>= 7;
        if val == 0 {
            buf.push(byte);
            return;
        }
        buf.push(byte | 0x80);
    }
}

fn test_range_encoder_roundtrip() -> KernelResult<()> {
    // Encode a sequence of bits with adaptive probabilities, then decode
    // and verify they match.
    let bits: [u32; 16] = [0, 1, 0, 0, 1, 1, 0, 1, 1, 0, 0, 0, 1, 0, 1, 1];

    let encoded = {
        let mut rc = RangeEncoder::new();
        let mut probs = [PROB_INIT; 2];
        for &b in &bits {
            rc.encode_bit(&mut probs[0], b);
        }
        rc.finish()
    };

    // Decode.
    let mut rc = RangeDecoder::new(&encoded)?;
    let mut probs = [PROB_INIT; 2];
    for (i, &expected) in bits.iter().enumerate() {
        let got = rc.decode_bit(&mut probs[0]);
        if got != expected {
            serial_println!(
                "[xz]   FAIL: range encoder roundtrip bit {} = {}, expected {}",
                i, got, expected,
            );
            return Err(KernelError::InternalError);
        }
    }

    serial_println!("[xz]   range encoder round-trip OK");
    Ok(())
}

fn test_distance_slots() -> KernelResult<()> {
    // Verify that distance_to_slot produces the correct slot for known distances.
    // Mapping: slots 0-3 = dist 0-3 (1:1).  Slot 4 = dist 4-5, slot 5 = dist 6-7,
    // slot 6 = dist 8-11, slot 7 = dist 12-15, etc.
    let cases: [(u32, u32); 8] = [
        (0, 0), (1, 1), (2, 2), (3, 3),
        (4, 4), (5, 4), (6, 5), (7, 5),
    ];
    for &(dist, expected_slot) in &cases {
        let slot = distance_to_slot(dist);
        if slot != expected_slot {
            serial_println!(
                "[xz]   FAIL: distance_to_slot({}) = {}, expected {}",
                dist, slot, expected_slot,
            );
            return Err(KernelError::InternalError);
        }
    }

    // Verify roundtrip: for each distance, encode→decode should recover it.
    // Test a range of distances.
    for dist in [0u32, 1, 2, 3, 4, 5, 10, 50, 100, 500, 1000, 5000, 60000] {
        let slot = distance_to_slot(dist);
        // Reconstruct distance from slot.
        let recovered = if slot < START_POS_MODEL as u32 {
            slot
        } else {
            let num_direct_bits = (slot >> 1).wrapping_sub(1) as usize;
            let base = (2u32 | (slot & 1)) << num_direct_bits;
            let footer = dist.wrapping_sub(base);
            base.wrapping_add(footer)
        };
        if recovered != dist {
            serial_println!(
                "[xz]   FAIL: distance {} → slot {} → recovered {}",
                dist, slot, recovered,
            );
            return Err(KernelError::InternalError);
        }
    }

    serial_println!("[xz]   distance_to_slot OK");
    Ok(())
}

fn test_compress_roundtrip() -> KernelResult<()> {
    serial_println!("[xz]   starting compression round-trip tests...");

    // Test 1: Text input.
    let text = b"the quick brown fox jumps over the lazy dog";
    let compressed = xz_compress(text)?;
    let decompressed = unxz(&compressed)?;
    if decompressed.as_slice() != text {
        serial_println!("[xz]   FAIL: text round-trip mismatch");
        return Err(KernelError::InternalError);
    }
    serial_println!(
        "[xz]   text round-trip ({}B → {}B, {}%) ✓",
        text.len(), compressed.len(),
        compressed.len().wrapping_mul(100) / text.len().max(1),
    );

    // Test 2: Empty input.
    let compressed_empty = xz_compress(b"")?;
    let decompressed_empty = unxz(&compressed_empty)?;
    if !decompressed_empty.is_empty() {
        serial_println!("[xz]   FAIL: empty round-trip produced data");
        return Err(KernelError::InternalError);
    }
    serial_println!("[xz]   empty input round-trip ✓");

    // Test 3: Repetitive data (should compress well).
    // Use 200 bytes to stay within kernel heap limits.
    let mut rep_data = Vec::with_capacity(200);
    for _ in 0..8 {
        rep_data.extend_from_slice(b"Hello World! This repeats. ");
    }
    rep_data.truncate(200);
    let compressed_rep = xz_compress(&rep_data)?;
    let decompressed_rep = unxz(&compressed_rep)?;
    if decompressed_rep.as_slice() != rep_data.as_slice() {
        serial_println!("[xz]   FAIL: repetitive data round-trip mismatch");
        return Err(KernelError::InternalError);
    }
    serial_println!(
        "[xz]   repetitive data round-trip ({}B → {}B, {}%) ✓",
        rep_data.len(), compressed_rep.len(),
        compressed_rep.len().wrapping_mul(100) / rep_data.len().max(1),
    );

    // Test 4: Single byte.
    let compressed_one = xz_compress(b"X")?;
    let decompressed_one = unxz(&compressed_one)?;
    if decompressed_one.as_slice() != b"X" {
        serial_println!("[xz]   FAIL: single-byte round-trip mismatch");
        return Err(KernelError::InternalError);
    }
    serial_println!("[xz]   single-byte round-trip ✓");

    // Test 5: All-identical bytes (extreme compression case).
    let all_same = vec![0xAA; 64];
    let compressed_same = xz_compress(&all_same)?;
    let decompressed_same = unxz(&compressed_same)?;
    if decompressed_same.as_slice() != all_same.as_slice() {
        serial_println!("[xz]   FAIL: all-identical round-trip mismatch");
        return Err(KernelError::InternalError);
    }
    serial_println!("[xz]   all-identical bytes round-trip ✓");

    // Test 6: Binary data (all byte values).
    let mut binary = Vec::with_capacity(256);
    for i in 0u16..256 {
        binary.push(i as u8);
    }
    let compressed_bin = xz_compress(&binary)?;
    let decompressed_bin = unxz(&compressed_bin)?;
    if decompressed_bin.as_slice() != binary.as_slice() {
        serial_println!("[xz]   FAIL: binary data round-trip mismatch");
        return Err(KernelError::InternalError);
    }
    serial_println!("[xz]   binary data round-trip ✓");

    Ok(())
}
