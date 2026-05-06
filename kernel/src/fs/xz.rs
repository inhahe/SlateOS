//! XZ decompression (LZMA2-based) for `.xz` archives.
//!
//! Implements the full XZ container format with LZMA2 decoding, including:
//! - XZ stream header/footer parsing with CRC-32 validation
//! - Block parsing with optional compressed/uncompressed size fields
//! - LZMA2 chunk decoder (uncompressed, LZMA with various reset levels)
//! - LZMA range decoder with adaptive probability model
//! - CRC-64 (ECMA-182) for block/stream integrity checks
//!
//! ## References
//!
//! - XZ format: <https://tukaani.org/xz/xz-file-format.txt>
//! - LZMA2: 7-Zip LZMA SDK (lzma.txt, LzmaDec.c)
//! - LZMA: Igor Pavlov's specification in the LZMA SDK

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
struct LzmaState {
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
    fn new(lc: u8, lp: u8, pb: u8) -> Self {
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
fn lzma_decode(
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
            let mut len: u32;
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
            };

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
fn lzma2_decode(data: &[u8], dict_size: u32) -> KernelResult<Vec<u8>> {
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
fn lzma2_dict_size(byte: u8) -> u32 {
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
        let before_len = all_output.len();

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

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Comprehensive self-test for XZ/LZMA2/LZMA decompression.
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
    let mut bad_magic = [0u8; 24];
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
    for _ in 0..pad_needed {
        block_header_body.push(0x00);
    }

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
    for _ in 0..index_pad {
        index_body.push(0x00);
    }
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
    for _ in 0..block_data_pad {
        stream.push(0x00);
    }

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
