//! The LZMA encoder: liblzma's `lzma_encoder.c` and
//! `lzma_encoder_private.h` -- the probability model, the length coders and
//! their price tables, the coding of each symbol, and the loop that asks the
//! optimizer (`optimum`) what to code next.
//!
//! The model is one array of probabilities (see the offsets below), so that
//! the range encoder can queue a bit by its probability's index and update it
//! later, as liblzma's does (see `rc`).

// Probability indices are model offsets plus states, position states and
// symbols bounded by the model's shape; lengths are 2 to 273 and distances
// below 2^32; the counters are liblzma's own.
#![allow(clippy::arithmetic_side_effects)]

use alloc::vec;
use alloc::vec::Vec;

use super::mf::{MATCH_LEN_MAX, Match, Mf};
use super::price;
use super::rc::RangeEncoder;
use super::{LzmaOptions, Mode};
use crate::lzma::{
    DIST_MODEL_END, DIST_MODEL_START, DIST_SLOT_BITS, DIST_STATES, FULL_DISTANCES,
    LEN_HIGH_SYMBOLS, LEN_LOW_SYMBOLS, LEN_MID_SYMBOLS, LITERAL_CODER_SIZE, LITERAL_CODERS_MAX,
    MATCH_LEN_MIN, NEXT_STATE_AFTER_LITERAL, POS_STATES_MAX, PROB_INIT, STATE_LIT_LONGREP,
    STATE_LIT_MATCH, STATE_LIT_SHORTREP, STATE_NONLIT_MATCH, STATE_NONLIT_REP, STATES,
};

/// `REPS`: the four most recent distances.
pub(crate) const REPS: u32 = 4;

/// `OPTS`: the optimizer's lookahead, in positions.
pub(crate) const OPTS: u32 = 1 << 12;

/// `LOOP_INPUT_MAX`: the most one pass of the encoding loop consumes.
pub(crate) const LOOP_INPUT_MAX: u32 = OPTS + 1;

/// `LZMA2_CHUNK_MAX`: an LZMA2 chunk's compressed size, at most.
pub(crate) const LZMA2_CHUNK_MAX: u32 = 1 << 16;

/// `ALIGN_SIZE`, `ALIGN_MASK`
pub(crate) const ALIGN_SIZE: u32 = 16;
const ALIGN_MASK: u32 = ALIGN_SIZE - 1;
const ALIGN_BITS: u32 = 4;

/// `DIST_SLOTS`
pub(crate) const DIST_SLOTS: usize = 1 << 6;

/// `LEN_SYMBOLS`: lengths 2 to 273.
pub(crate) const LEN_SYMBOLS: usize =
    (LEN_LOW_SYMBOLS + LEN_MID_SYMBOLS + LEN_HIGH_SYMBOLS) as usize;

// --- the model's layout ----------------------------------------------------

const POS_STATES: usize = POS_STATES_MAX;
/// `is_match[STATES][POS_STATES_MAX]`
pub(crate) const IS_MATCH: usize = 0;
/// `is_rep[STATES]`
pub(crate) const IS_REP: usize = IS_MATCH + STATES * POS_STATES;
/// `is_rep0[STATES]`
pub(crate) const IS_REP0: usize = IS_REP + STATES;
/// `is_rep1[STATES]`
pub(crate) const IS_REP1: usize = IS_REP0 + STATES;
/// `is_rep2[STATES]`
pub(crate) const IS_REP2: usize = IS_REP1 + STATES;
/// `is_rep0_long[STATES][POS_STATES_MAX]`
pub(crate) const IS_REP0_LONG: usize = IS_REP2 + STATES;
/// `dist_slot[DIST_STATES][DIST_SLOTS]`
pub(crate) const DIST_SLOT: usize = IS_REP0_LONG + STATES * POS_STATES;
/// `dist_special[FULL_DISTANCES - DIST_MODEL_END]`
pub(crate) const DIST_SPECIAL: usize = DIST_SLOT + DIST_STATES * DIST_SLOTS;
/// `dist_align[ALIGN_SIZE]`
pub(crate) const DIST_ALIGN: usize = DIST_SPECIAL + (FULL_DISTANCES - DIST_MODEL_END as usize);
/// The match length coder (`match_len_encoder`).
pub(crate) const MATCH_LEN: usize = DIST_ALIGN + ALIGN_SIZE as usize;
/// The repeated-match length coder (`rep_len_encoder`).
pub(crate) const REP_LEN: usize = MATCH_LEN + LEN_CODER;
/// `literal[LITERAL_CODERS_MAX][LITERAL_CODER_SIZE]`
pub(crate) const LITERAL: usize = REP_LEN + LEN_CODER;
const PROBS: usize = LITERAL + LITERAL_CODERS_MAX * LITERAL_CODER_SIZE;

// Within a length coder: `choice`, `choice2`, `low[POS_STATES_MAX][8]`,
// `mid[POS_STATES_MAX][8]`, `high[256]`.
const LEN_CHOICE: usize = 0;
const LEN_CHOICE2: usize = 1;
const LEN_LOW: usize = 2;
const LEN_MID: usize = LEN_LOW + POS_STATES * LEN_LOW_SYMBOLS as usize;
const LEN_HIGH: usize = LEN_MID + POS_STATES * LEN_MID_SYMBOLS as usize;
const LEN_CODER: usize = LEN_HIGH + LEN_HIGH_SYMBOLS as usize;

/// `update_literal`
pub(crate) fn next_after_literal(state: u32) -> u32 {
    NEXT_STATE_AFTER_LITERAL
        .get(state as usize)
        .copied()
        .unwrap_or(0)
}

/// `update_match`
pub(crate) const fn next_after_match(state: u32) -> u32 {
    if state < 7 {
        STATE_LIT_MATCH
    } else {
        STATE_NONLIT_MATCH
    }
}

/// `update_long_rep`
pub(crate) const fn next_after_long_rep(state: u32) -> u32 {
    if state < 7 {
        STATE_LIT_LONGREP
    } else {
        STATE_NONLIT_REP
    }
}

/// `update_short_rep`
pub(crate) const fn next_after_short_rep(state: u32) -> u32 {
    if state < 7 {
        STATE_LIT_SHORTREP
    } else {
        STATE_NONLIT_REP
    }
}

/// `is_literal_state`
pub(crate) const fn is_literal_state(state: u32) -> bool {
    state < 7
}

/// `get_dist_state`
pub(crate) const fn dist_state(len: u32) -> u32 {
    if len < DIST_STATES as u32 + MATCH_LEN_MIN {
        len.wrapping_sub(MATCH_LEN_MIN)
    } else {
        DIST_STATES as u32 - 1
    }
}

/// A length coder's price table (`lzma_length_encoder`'s `prices`,
/// `table_size` and `counters`); its probabilities are in the model at
/// `base`.
pub(crate) struct LengthPrices {
    base: usize,
    prices: Vec<u32>,
    table_size: u32,
    counters: [u32; POS_STATES],
}

impl LengthPrices {
    fn new(base: usize, table_size: u32) -> Self {
        Self {
            base,
            prices: vec![0; POS_STATES * LEN_SYMBOLS],
            table_size,
            counters: [0; POS_STATES],
        }
    }

    /// `get_len_price`: the price of length `len` at `pos_state`.
    pub(crate) fn price(&self, len: u32, pos_state: u32) -> u32 {
        let i = (pos_state as usize)
            .wrapping_mul(LEN_SYMBOLS)
            .wrapping_add(len.wrapping_sub(MATCH_LEN_MIN) as usize);
        self.prices.get(i).copied().unwrap_or(price::INFINITY)
    }

    /// `length_update_prices`
    fn update(&mut self, probs: &[u16], pos_state: u32) {
        let ps = pos_state as usize;
        let table_size = self.table_size;
        if let Some(c) = self.counters.get_mut(ps) {
            *c = table_size;
        }
        let p = |i: usize| probs.get(self.base.wrapping_add(i)).copied().unwrap_or(0);
        let tree = |off: usize, n: usize| {
            let start = self.base.wrapping_add(off);
            probs.get(start..start.wrapping_add(n)).unwrap_or(&[])
        };
        let a0 = price::bit0(p(LEN_CHOICE));
        let a1 = price::bit1(p(LEN_CHOICE));
        let b0 = a1.wrapping_add(price::bit0(p(LEN_CHOICE2)));
        let b1 = a1.wrapping_add(price::bit1(p(LEN_CHOICE2)));
        let low = tree(
            LEN_LOW + ps * LEN_LOW_SYMBOLS as usize,
            LEN_LOW_SYMBOLS as usize,
        );
        let mid = tree(
            LEN_MID + ps * LEN_MID_SYMBOLS as usize,
            LEN_MID_SYMBOLS as usize,
        );
        let high = tree(LEN_HIGH, LEN_HIGH_SYMBOLS as usize);
        let row = ps.wrapping_mul(LEN_SYMBOLS);
        for i in 0..table_size {
            let price = if i < LEN_LOW_SYMBOLS {
                a0.wrapping_add(price::bittree(low, 3, i))
            } else if i < LEN_LOW_SYMBOLS + LEN_MID_SYMBOLS {
                b0.wrapping_add(price::bittree(mid, 3, i - LEN_LOW_SYMBOLS))
            } else {
                b1.wrapping_add(price::bittree(
                    high,
                    8,
                    i - LEN_LOW_SYMBOLS - LEN_MID_SYMBOLS,
                ))
            };
            if let Some(slot) = self.prices.get_mut(row.wrapping_add(i as usize)) {
                *slot = price;
            }
        }
    }
}

/// One position of the optimizer's lookahead (`lzma_optimal`).
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Optimal {
    pub(crate) state: u32,
    pub(crate) prev_1_is_literal: bool,
    pub(crate) prev_2: bool,
    pub(crate) pos_prev_2: u32,
    pub(crate) back_prev_2: u32,
    pub(crate) price: u32,
    pub(crate) pos_prev: u32,
    pub(crate) back_prev: u32,
    pub(crate) backs: [u32; REPS as usize],
}

/// `lzma_lzma1_encoder`
pub(crate) struct Encoder {
    pub(crate) rc: RangeEncoder,
    pub(crate) probs: Vec<u16>,
    pub(crate) state: u32,
    pub(crate) reps: [u32; REPS as usize],
    pub(crate) matches: Vec<Match>,
    pub(crate) matches_count: u32,
    pub(crate) longest_match_length: u32,
    fast_mode: bool,
    is_initialized: bool,
    is_flushed: bool,
    pub(crate) pos_mask: u32,
    pub(crate) lc: u32,
    pub(crate) lp_mask: u32,
    pub(crate) match_len: LengthPrices,
    pub(crate) rep_len: LengthPrices,
    pub(crate) dist_slot_prices: Vec<u32>,
    pub(crate) dist_prices: Vec<u32>,
    pub(crate) dist_table_size: u32,
    pub(crate) match_price_count: u32,
    pub(crate) align_prices: [u32; ALIGN_SIZE as usize],
    pub(crate) align_price_count: u32,
    pub(crate) opts: Vec<Optimal>,
    pub(crate) opts_end_index: u32,
    pub(crate) opts_current_index: u32,
}

impl Encoder {
    /// `lzma_lzma_encoder_create` and `lzma_lzma_encoder_reset`, for valid
    /// options.
    pub(crate) fn new(opt: &LzmaOptions) -> Self {
        let fast_mode = opt.mode == Mode::Fast;
        // `dist_table_size`: twice the bits of the dictionary size, rounded
        // up; the length tables reach `nice_len`.
        let mut log_size = 0u32;
        while log_size < 32 && (1u64 << log_size) < u64::from(opt.dict_size) {
            log_size += 1;
        }
        let table_size = opt.nice_len.saturating_add(1).saturating_sub(MATCH_LEN_MIN);
        let mut enc = Self {
            rc: RangeEncoder::new(),
            probs: vec![PROB_INIT; PROBS],
            state: 0,
            reps: [0; REPS as usize],
            matches: vec![Match::default(); MATCH_LEN_MAX as usize + 1],
            matches_count: 0,
            longest_match_length: 0,
            fast_mode,
            is_initialized: false,
            is_flushed: false,
            pos_mask: 0,
            lc: 0,
            lp_mask: 0,
            match_len: LengthPrices::new(MATCH_LEN, table_size),
            rep_len: LengthPrices::new(REP_LEN, table_size),
            dist_slot_prices: vec![0; DIST_STATES * DIST_SLOTS],
            dist_prices: vec![0; DIST_STATES * FULL_DISTANCES],
            dist_table_size: log_size.wrapping_mul(2),
            match_price_count: 0,
            align_prices: [0; ALIGN_SIZE as usize],
            align_price_count: 0,
            opts: vec![Optimal::default(); OPTS as usize],
            opts_end_index: 0,
            opts_current_index: 0,
        };
        enc.reset(opt);
        enc
    }

    /// `lzma_lzma_encoder_reset`: a fresh model and state, as each LZMA2
    /// chunk after an uncompressed one starts with.
    pub(crate) fn reset(&mut self, opt: &LzmaOptions) {
        self.pos_mask = (1u32 << opt.pb).wrapping_sub(1);
        self.lc = opt.lc;
        self.lp_mask = (1u32 << opt.lp).wrapping_sub(1);
        self.rc.reset();
        self.state = 0;
        self.reps = [0; REPS as usize];
        // Every probability even: liblzma resets only those the options
        // use, and the rest are never read.
        self.probs.fill(PROB_INIT);
        if !self.fast_mode {
            for ps in 0..=self.pos_mask {
                self.match_len.update(&self.probs, ps);
                self.rep_len.update(&self.probs, ps);
            }
        }
        // The price counts start high enough that the tables are filled
        // before they are first read.
        self.match_price_count = u32::MAX / 2;
        self.align_price_count = u32::MAX / 2;
        self.opts_end_index = 0;
        self.opts_current_index = 0;
    }

    /// The probability at `i` of the model.
    pub(crate) fn prob(&self, i: usize) -> u16 {
        self.probs.get(i).copied().unwrap_or(0)
    }

    /// A slice of the model from `start`, for a bit tree's prices.
    pub(crate) fn probs_from(&self, start: usize, len: usize) -> &[u16] {
        self.probs
            .get(start..start.wrapping_add(len))
            .unwrap_or(&[])
    }

    /// The literal coder for a byte at `pos` after `prev_byte`
    /// (`literal_subcoder`): the offset of its 0x300 probabilities.
    pub(crate) fn literal_coder(&self, pos: u32, prev_byte: u8) -> usize {
        let i =
            ((pos & self.lp_mask) << self.lc).wrapping_add(u32::from(prev_byte) >> (8 - self.lc));
        LITERAL.wrapping_add((i as usize).wrapping_mul(LITERAL_CODER_SIZE))
    }

    // --- coding a symbol ----------------------------------------------------

    /// `literal`: the byte at the encoder's position.
    fn literal(&mut self, mf: &Mf<'_>, position: u32) {
        let at = mf.position();
        let cur_byte = mf.byte(at);
        let coder = self.literal_coder(position, mf.byte(at.wrapping_sub(1)));
        if is_literal_state(self.state) {
            self.rc.bittree(coder, 8, u32::from(cur_byte));
        } else {
            // After a match, the byte the last distance points at guides the
            // coding while the bits agree.
            let match_byte = mf.byte(at.wrapping_sub(self.reps[0] as usize).wrapping_sub(1));
            self.literal_matched(coder, u32::from(match_byte), u32::from(cur_byte));
        }
        self.state = next_after_literal(self.state);
    }

    /// `literal_matched`
    fn literal_matched(&mut self, coder: usize, match_byte: u32, symbol: u32) {
        let mut offset = 0x100u32;
        let mut symbol = symbol | 0x100;
        let mut match_byte = match_byte;
        while symbol < 1 << 16 {
            match_byte <<= 1;
            let match_bit = match_byte & offset;
            let index = offset.wrapping_add(match_bit).wrapping_add(symbol >> 8);
            let bit = (symbol >> 7) & 1;
            self.rc.bit(coder.wrapping_add(index as usize), bit);
            symbol <<= 1;
            offset &= !(match_byte ^ symbol);
        }
    }

    /// `length`: a match length with the coder at `base`, and the price
    /// table refreshed once it has been used `table_size` times -- from the
    /// probabilities as they stand before this length's bits are coded.
    fn length(&mut self, rep: bool, pos_state: u32, len: u32) {
        let base = if rep { REP_LEN } else { MATCH_LEN };
        let ps = pos_state as usize;
        let mut len = len.wrapping_sub(MATCH_LEN_MIN);
        if len < LEN_LOW_SYMBOLS {
            self.rc.bit(base + LEN_CHOICE, 0);
            self.rc
                .bittree(base + LEN_LOW + ps * LEN_LOW_SYMBOLS as usize, 3, len);
        } else {
            self.rc.bit(base + LEN_CHOICE, 1);
            len -= LEN_LOW_SYMBOLS;
            if len < LEN_MID_SYMBOLS {
                self.rc.bit(base + LEN_CHOICE2, 0);
                self.rc
                    .bittree(base + LEN_MID + ps * LEN_MID_SYMBOLS as usize, 3, len);
            } else {
                self.rc.bit(base + LEN_CHOICE2, 1);
                len -= LEN_MID_SYMBOLS;
                self.rc.bittree(base + LEN_HIGH, 8, len);
            }
        }
        if !self.fast_mode {
            let coder = if rep {
                &mut self.rep_len
            } else {
                &mut self.match_len
            };
            if let Some(c) = coder.counters.get_mut(ps) {
                *c = c.wrapping_sub(1);
                if *c == 0 {
                    coder.update(&self.probs, pos_state);
                }
            }
        }
    }

    /// `match`: a new distance.
    fn match_(&mut self, pos_state: u32, distance: u32, len: u32) {
        self.state = next_after_match(self.state);
        self.length(false, pos_state, len);
        let slot = price::dist_slot(distance);
        let ds = dist_state(len) as usize;
        self.rc
            .bittree(DIST_SLOT + ds * DIST_SLOTS, DIST_SLOT_BITS, slot);
        if slot >= DIST_MODEL_START {
            let footer_bits = (slot >> 1).wrapping_sub(1);
            let base = (2 | (slot & 1)) << footer_bits;
            let reduced = distance.wrapping_sub(base);
            if slot < DIST_MODEL_END {
                // The tree's root is at `base - slot`; liblzma's pointer is
                // one before it, as the tree coder starts at index 1.
                let tree = (DIST_SPECIAL + base as usize).wrapping_sub(slot as usize + 1);
                self.rc.bittree_reverse(tree, footer_bits, reduced);
            } else {
                self.rc
                    .direct(reduced >> ALIGN_BITS, footer_bits.wrapping_sub(ALIGN_BITS));
                self.rc
                    .bittree_reverse(DIST_ALIGN, ALIGN_BITS, reduced & ALIGN_MASK);
                self.align_price_count = self.align_price_count.wrapping_add(1);
            }
        }
        self.reps = [distance, self.reps[0], self.reps[1], self.reps[2]];
        self.match_price_count = self.match_price_count.wrapping_add(1);
    }

    /// `rep_match`: a repeat of distance `rep` (0 to 3); length 1 is the
    /// "short rep".
    fn rep_match(&mut self, pos_state: u32, rep: u32, len: u32) {
        let st = self.state as usize;
        let ps = pos_state as usize;
        if rep == 0 {
            self.rc.bit(IS_REP0 + st, 0);
            self.rc
                .bit(IS_REP0_LONG + st * POS_STATES + ps, u32::from(len != 1));
        } else {
            let distance = self.reps.get(rep as usize).copied().unwrap_or(0);
            self.rc.bit(IS_REP0 + st, 1);
            if rep == 1 {
                self.rc.bit(IS_REP1 + st, 0);
            } else {
                self.rc.bit(IS_REP1 + st, 1);
                self.rc.bit(IS_REP2 + st, rep - 2);
                if rep == 3 {
                    self.reps[3] = self.reps[2];
                }
                self.reps[2] = self.reps[1];
            }
            self.reps[1] = self.reps[0];
            self.reps[0] = distance;
        }
        if len == 1 {
            self.state = next_after_short_rep(self.state);
        } else {
            self.length(true, pos_state, len);
            self.state = next_after_long_rep(self.state);
        }
    }

    /// `encode_symbol`: `back` is `u32::MAX` for a literal, below [`REPS`]
    /// for a repeat, and otherwise a distance plus [`REPS`].
    fn encode_symbol(&mut self, mf: &mut Mf<'_>, back: u32, len: u32, position: u32) {
        let pos_state = position & self.pos_mask;
        let is_match = IS_MATCH + self.state as usize * POS_STATES + pos_state as usize;
        if back == u32::MAX {
            self.rc.bit(is_match, 0);
            self.literal(mf, position);
        } else {
            self.rc.bit(is_match, 1);
            if back < REPS {
                self.rc.bit(IS_REP + self.state as usize, 1);
                self.rep_match(pos_state, back, len);
            } else {
                self.rc.bit(IS_REP + self.state as usize, 0);
                self.match_(pos_state, back - REPS, len);
            }
        }
        mf.read_ahead = mf.read_ahead.wrapping_sub(len);
    }

    /// `encode_init`: the first byte is always a literal, coded with no
    /// previous byte and no match byte.
    fn encode_init(&mut self, mf: &mut Mf<'_>) {
        if !mf.at_end() {
            mf.skip(1);
            mf.read_ahead = 0;
            self.rc.bit(IS_MATCH, 0);
            self.rc.bittree(LITERAL, 8, u32::from(mf.byte(0)));
        }
        self.is_initialized = true;
    }

    /// `encode_eopm`: the end-of-payload marker, a match at distance
    /// `u32::MAX`.
    fn encode_eopm(&mut self, position: u32) {
        let pos_state = position & self.pos_mask;
        self.rc.bit(
            IS_MATCH + self.state as usize * POS_STATES + pos_state as usize,
            1,
        );
        self.rc.bit(IS_REP + self.state as usize, 0);
        self.match_(pos_state, u32::MAX, MATCH_LEN_MIN);
    }

    /// `lzma_lzma_encode`: codes from the match finder's position into
    /// `out` -- for plain LZMA (`limit` `None`) to the end of the input and an
    /// end-of-payload marker; for an LZMA2 chunk until the input position
    /// reaches `limit` or the compressed size nears [`LZMA2_CHUNK_MAX`].
    pub(crate) fn encode(&mut self, mf: &mut Mf<'_>, out: &mut Vec<u8>, limit: Option<usize>) {
        if !self.is_initialized {
            self.encode_init(mf);
        }
        // Only the position's low bits are ever used.
        let mut position = mf.position() as u32;
        loop {
            self.rc.encode(&mut self.probs, out);
            if let Some(limit) = limit {
                let full = (out.len() as u64).saturating_add(self.rc.pending())
                    >= u64::from(LZMA2_CHUNK_MAX - LOOP_INPUT_MAX);
                if mf.position() >= limit || full {
                    break;
                }
            }
            if mf.at_end() && mf.read_ahead == 0 {
                break;
            }
            let (back, len) = if self.fast_mode {
                self.optimum_fast(mf)
            } else {
                self.optimum_normal(mf, position)
            };
            self.encode_symbol(mf, back, len, position);
            position = position.wrapping_add(len);
        }
        if !self.is_flushed {
            self.is_flushed = true;
            if limit.is_none() {
                self.encode_eopm(position);
            }
            self.rc.flush();
            self.rc.encode(&mut self.probs, out);
        }
        // Ready for the next LZMA2 chunk.
        self.is_flushed = false;
    }
}
