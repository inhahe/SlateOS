//! The LZMA decoder: liblzma's `lzma_decoder.c` and `range_decoder.h`, run
//! over a whole buffer.
//!
//! liblzma's decoder is a resumable state machine -- a `switch` over some
//! sixty `SEQ_` labels -- because it may run out of input or output in the
//! middle of any bit. With the whole input in hand there is nothing to
//! resume: running out of input is [`Error::UnexpectedEnd`], and the labels
//! become the straight-line code they label. What is kept exactly is every
//! decision that decides between a stream accepted and a stream refused: the
//! range decoder normalises *before* each bit, not after, so it reads input
//! in the same order; its first byte must be zero; a match must not reach
//! past the dictionary or past a known size; the end-of-payload marker is
//! refused when the size is known; and a finished stream must leave the range
//! decoder's `code` at zero.
//!
//! The dictionary is the output itself: decoding appends to the caller's
//! `Vec`, and the dictionary is its tail since the last reset, as many bytes
//! of it as the dictionary size allows (`Dict`).

use alloc::vec;
use alloc::vec::Vec;

use crate::{Error, Result};

/// `RC_TOP_VALUE`: below this the range is renormalised by a byte.
pub(crate) const RC_TOP: u32 = 1 << 24;
/// `RC_BIT_MODEL_TOTAL_BITS`
pub(crate) const BIT_MODEL_TOTAL_BITS: u32 = 11;
pub(crate) const BIT_MODEL_TOTAL: u32 = 1 << BIT_MODEL_TOTAL_BITS;
/// `RC_MOVE_BITS`
pub(crate) const MOVE_BITS: u32 = 5;
/// `bit_reset`: an even probability.
pub(crate) const PROB_INIT: u16 = (BIT_MODEL_TOTAL >> 1) as u16;

/// `STATES`, `LIT_STATES`
pub(crate) const STATES: usize = 12;
pub(crate) const LIT_STATES: u32 = 7;
/// `POS_STATES_MAX`: `pb` is at most 4.
pub(crate) const POS_STATES_MAX: usize = 16;
/// `LITERAL_CODER_SIZE` and `LITERAL_CODERS_MAX` (`lc + lp` is at most 4).
pub(crate) const LITERAL_CODER_SIZE: usize = 0x300;
pub(crate) const LITERAL_CODERS_MAX: usize = 16;
/// `MATCH_LEN_MIN`, and the length coder's three symbol counts.
pub(crate) const MATCH_LEN_MIN: u32 = 2;
pub(crate) const LEN_LOW_SYMBOLS: u32 = 8;
pub(crate) const LEN_MID_SYMBOLS: u32 = 8;
pub(crate) const LEN_HIGH_SYMBOLS: u32 = 256;
/// `DIST_STATES`, `DIST_SLOT_BITS`
pub(crate) const DIST_STATES: usize = 4;
/// The distance state every length from 5 on shares (`get_dist_state`).
pub(crate) const LAST_DIST_STATE: u32 = DIST_STATES as u32 - 1;
pub(crate) const DIST_SLOT_BITS: u32 = 6;
/// `DIST_MODEL_START`, `DIST_MODEL_END`, `FULL_DISTANCES`
pub(crate) const DIST_MODEL_START: u32 = 4;
pub(crate) const DIST_MODEL_END: u32 = 14;
pub(crate) const FULL_DISTANCES: usize = 128;
/// `ALIGN_BITS`
pub(crate) const ALIGN_BITS: u32 = 4;

/// The literal state after a literal (`next_state[]` in `lzma_decode`).
pub(crate) const NEXT_STATE_AFTER_LITERAL: [u32; STATES] = [0, 0, 0, 0, 1, 2, 3, 4, 5, 6, 4, 5];
/// `STATE_LIT_MATCH`, `STATE_LIT_LONGREP`, `STATE_LIT_SHORTREP`,
/// `STATE_NONLIT_MATCH`, `STATE_NONLIT_REP`
pub(crate) const STATE_LIT_MATCH: u32 = 7;
pub(crate) const STATE_LIT_LONGREP: u32 = 8;
pub(crate) const STATE_LIT_SHORTREP: u32 = 9;
pub(crate) const STATE_NONLIT_MATCH: u32 = 10;
pub(crate) const STATE_NONLIT_REP: u32 = 11;

/// `lc`, `lp` and `pb`: the literal context bits, literal position bits and
/// position bits, as one properties byte packs them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Props {
    pub(crate) lc: u32,
    pub(crate) lp: u32,
    pub(crate) pb: u32,
}

impl Props {
    /// `lzma_lzma_lclppb_decode`: `None` for a byte above `(4 * 5 + 4) * 9 +
    /// 8` or with `lc + lp` above 4.
    pub(crate) fn from_byte(byte: u8) -> Option<Self> {
        let mut b = u32::from(byte);
        if b > (4 * 5 + 4) * 9 + 8 {
            return None;
        }
        let pb = b / (9 * 5);
        b = b.wrapping_sub(pb.wrapping_mul(9 * 5));
        let lp = b / 9;
        let lc = b.wrapping_sub(lp.wrapping_mul(9));
        if lc.wrapping_add(lp) > 4 {
            return None;
        }
        Some(Self { lc, lp, pb })
    }

    /// The properties byte (`lzma_lzma_lclppb_encode`).
    pub(crate) const fn to_byte(self) -> u8 {
        self.pb
            .wrapping_mul(5)
            .wrapping_add(self.lp)
            .wrapping_mul(9)
            .wrapping_add(self.lc) as u8
    }
}

/// The dictionary: the part of the output since the last dictionary reset,
/// at most `window` bytes of it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Dict {
    /// Where the dictionary began in the output (its last reset).
    pub(crate) start: usize,
    /// The window: liblzma's dictionary buffer size -- the declared size, at
    /// least 4096, rounded up to a multiple of 16 (`lzma_lz_decoder_init`).
    pub(crate) window: usize,
}

impl Dict {
    /// The window liblzma allocates for a declared dictionary size.
    pub(crate) fn window_for(dict_size: u32) -> usize {
        let size = usize::try_from(dict_size.max(4096)).unwrap_or(usize::MAX);
        size.saturating_add(15) & !15
    }

    /// `dict.full`: how many bytes back a distance may reach.
    fn full(&self, out: &[u8]) -> usize {
        out.len().saturating_sub(self.start).min(self.window)
    }

    /// `dict.pos`, for the position bits: bytes written since the reset.
    /// Only its low four bits are used, and the window is a multiple of 16,
    /// so it need not wrap as liblzma's circular `pos` does.
    fn pos(&self, out: &[u8]) -> u32 {
        (out.len().saturating_sub(self.start) & 0xf) as u32
    }

    /// `dict_get`: the byte `distance + 1` back, or 0 from an empty
    /// dictionary (liblzma zeroes the byte before position 0 on a reset).
    fn get(&self, out: &[u8], distance: u32) -> u8 {
        let back = usize::try_from(distance)
            .unwrap_or(usize::MAX)
            .saturating_add(1);
        if back > out.len().saturating_sub(self.start) {
            return 0;
        }
        out.get(out.len().wrapping_sub(back)).copied().unwrap_or(0)
    }
}

/// The range decoder (`lzma_range_decoder`), over the whole input from the
/// position it was started at.
pub(crate) struct Rc<'a> {
    input: &'a [u8],
    /// The next input byte.
    pub(crate) pos: usize,
    range: u32,
    code: u32,
}

impl<'a> Rc<'a> {
    /// `rc_reset` and `rc_read_init`: five bytes, the first of which must be
    /// zero.
    pub(crate) fn new(input: &'a [u8], pos: usize) -> Result<Self> {
        let &first = input.get(pos).ok_or(Error::UnexpectedEnd)?;
        if first != 0 {
            return Err(Error::InvalidData);
        }
        let mut code = 0u32;
        for i in 1..5 {
            let &b = input
                .get(pos.saturating_add(i))
                .ok_or(Error::UnexpectedEnd)?;
            code = (code << 8) | u32::from(b);
        }
        Ok(Self {
            input,
            pos: pos.saturating_add(5),
            range: u32::MAX,
            code,
        })
    }

    /// `rc_normalize`.
    fn normalize(&mut self) -> Result<()> {
        if self.range < RC_TOP {
            let &b = self.input.get(self.pos).ok_or(Error::UnexpectedEnd)?;
            self.range <<= 8;
            self.code = (self.code << 8) | u32::from(b);
            self.pos = self.pos.wrapping_add(1);
        }
        Ok(())
    }

    /// `rc_if_0` and `rc_update_0`/`rc_update_1`: one bit, adapting `prob`.
    fn bit(&mut self, prob: &mut u16) -> Result<u32> {
        self.normalize()?;
        let p = u32::from(*prob);
        let bound = (self.range >> BIT_MODEL_TOTAL_BITS).wrapping_mul(p);
        if self.code < bound {
            self.range = bound;
            // `p` is below 2048, so the step is below 64 and the sum fits.
            *prob = p.wrapping_add(BIT_MODEL_TOTAL.wrapping_sub(p) >> MOVE_BITS) as u16;
            Ok(0)
        } else {
            self.range = self.range.wrapping_sub(bound);
            self.code = self.code.wrapping_sub(bound);
            *prob = prob.wrapping_sub(*prob >> MOVE_BITS);
            Ok(1)
        }
    }

    /// A bit with the probability at `probs[i]`; an index outside the table
    /// (which the callers' bounds make impossible) is corrupt data rather
    /// than a panic.
    fn bit_at(&mut self, probs: &mut [u16], i: usize) -> Result<u32> {
        let prob = probs.get_mut(i).ok_or(Error::InvalidData)?;
        self.bit(prob)
    }

    /// A `bits`-bit symbol from the bit tree `probs`, most significant bit
    /// first; returns it with the tree's leading 1 still on top.
    fn bittree(&mut self, probs: &mut [u16], bits: u32) -> Result<u32> {
        let mut symbol = 1u32;
        for _ in 0..bits {
            let b = self.bit_at(probs, usize::try_from(symbol).unwrap_or(usize::MAX))?;
            symbol = (symbol << 1) | b;
        }
        Ok(symbol)
    }

    /// `rc_direct`: a bit with no probability, shifted into `dest`.
    fn direct(&mut self, dest: &mut u32) -> Result<()> {
        self.normalize()?;
        self.range >>= 1;
        self.code = self.code.wrapping_sub(self.range);
        let bound = 0u32.wrapping_sub(self.code >> 31);
        self.code = self.code.wrapping_add(self.range & bound);
        *dest = (*dest << 1).wrapping_add(bound.wrapping_add(1));
        Ok(())
    }

    /// `rc_is_finished`: a properly ended stream leaves `code` at zero.
    pub(crate) const fn is_finished(&self) -> bool {
        self.code == 0
    }
}

/// `lzma_length_decoder`.
#[derive(Clone)]
struct LenDecoder {
    choice: u16,
    choice2: u16,
    low: [[u16; LEN_LOW_SYMBOLS as usize]; POS_STATES_MAX],
    mid: [[u16; LEN_MID_SYMBOLS as usize]; POS_STATES_MAX],
    high: [u16; LEN_HIGH_SYMBOLS as usize],
}

impl LenDecoder {
    const fn new() -> Self {
        Self {
            choice: PROB_INIT,
            choice2: PROB_INIT,
            low: [[PROB_INIT; LEN_LOW_SYMBOLS as usize]; POS_STATES_MAX],
            mid: [[PROB_INIT; LEN_MID_SYMBOLS as usize]; POS_STATES_MAX],
            high: [PROB_INIT; LEN_HIGH_SYMBOLS as usize],
        }
    }

    /// `len_decode`: a match length, 2 to 273.
    fn decode(&mut self, rc: &mut Rc<'_>, pos_state: usize) -> Result<u32> {
        if rc.bit(&mut self.choice)? == 0 {
            let probs = self.low.get_mut(pos_state).ok_or(Error::InvalidData)?;
            let symbol = rc.bittree(probs, 3)?;
            // A 3-bit tree's symbol is 8 to 15: the arithmetic stays small.
            return Ok(symbol
                .wrapping_sub(LEN_LOW_SYMBOLS)
                .wrapping_add(MATCH_LEN_MIN));
        }
        if rc.bit(&mut self.choice2)? == 0 {
            let probs = self.mid.get_mut(pos_state).ok_or(Error::InvalidData)?;
            let symbol = rc.bittree(probs, 3)?;
            return Ok(symbol
                .wrapping_sub(LEN_MID_SYMBOLS)
                .wrapping_add(MATCH_LEN_MIN + LEN_LOW_SYMBOLS));
        }
        let symbol = rc.bittree(&mut self.high, 8)?;
        Ok(symbol
            .wrapping_sub(LEN_HIGH_SYMBOLS)
            .wrapping_add(MATCH_LEN_MIN + LEN_LOW_SYMBOLS + LEN_MID_SYMBOLS))
    }
}

/// How a run of the decoder ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum End {
    /// The known uncompressed size was reached.
    Size,
    /// The end-of-payload marker was read (only when the size is unknown).
    Marker,
}

/// `lzma_lzma1_decoder`: the probabilities and the state that carry from one
/// symbol, and in LZMA2 from one chunk, to the next.
pub(crate) struct Decoder {
    literal: Vec<u16>,
    is_match: [[u16; POS_STATES_MAX]; STATES],
    is_rep: [u16; STATES],
    is_rep0: [u16; STATES],
    is_rep1: [u16; STATES],
    is_rep2: [u16; STATES],
    is_rep0_long: [[u16; POS_STATES_MAX]; STATES],
    dist_slot: [[u16; 1 << DIST_SLOT_BITS]; DIST_STATES],
    pos_special: [u16; FULL_DISTANCES - DIST_MODEL_END as usize],
    pos_align: [u16; 1 << ALIGN_BITS],
    match_len: LenDecoder,
    rep_len: LenDecoder,
    state: u32,
    rep: [u32; 4],
    props: Props,
}

impl Decoder {
    /// A decoder reset to `props` (`lzma_decoder_reset`).
    pub(crate) fn new(props: Props) -> Self {
        let mut d = Self {
            literal: vec![PROB_INIT; LITERAL_CODER_SIZE * LITERAL_CODERS_MAX],
            is_match: [[PROB_INIT; POS_STATES_MAX]; STATES],
            is_rep: [PROB_INIT; STATES],
            is_rep0: [PROB_INIT; STATES],
            is_rep1: [PROB_INIT; STATES],
            is_rep2: [PROB_INIT; STATES],
            is_rep0_long: [[PROB_INIT; POS_STATES_MAX]; STATES],
            dist_slot: [[PROB_INIT; 1 << DIST_SLOT_BITS]; DIST_STATES],
            pos_special: [PROB_INIT; FULL_DISTANCES - DIST_MODEL_END as usize],
            pos_align: [PROB_INIT; 1 << ALIGN_BITS],
            match_len: LenDecoder::new(),
            rep_len: LenDecoder::new(),
            state: 0,
            rep: [0; 4],
            props,
        };
        d.reset(props);
        d
    }

    /// The properties the decoder was last reset to.
    pub(crate) const fn props(&self) -> Props {
        self.props
    }

    /// `lzma_decoder_reset`: every probability even, the state and the
    /// four distances zero.
    pub(crate) fn reset(&mut self, props: Props) {
        self.props = props;
        self.literal.fill(PROB_INIT);
        self.is_match = [[PROB_INIT; POS_STATES_MAX]; STATES];
        self.is_rep = [PROB_INIT; STATES];
        self.is_rep0 = [PROB_INIT; STATES];
        self.is_rep1 = [PROB_INIT; STATES];
        self.is_rep2 = [PROB_INIT; STATES];
        self.is_rep0_long = [[PROB_INIT; POS_STATES_MAX]; STATES];
        self.dist_slot = [[PROB_INIT; 1 << DIST_SLOT_BITS]; DIST_STATES];
        self.pos_special = [PROB_INIT; FULL_DISTANCES - DIST_MODEL_END as usize];
        self.pos_align = [PROB_INIT; 1 << ALIGN_BITS];
        self.match_len = LenDecoder::new();
        self.rep_len = LenDecoder::new();
        self.state = 0;
        self.rep = [0; 4];
    }

    /// `lzma_decode`: decodes symbols from `rc` onto `out` until `size` more
    /// bytes have been written (`Some`) or the end-of-payload marker is read
    /// (`None`), refusing to let `out` grow past `cap`.
    ///
    /// The range decoder must have been started at the stream's first byte;
    /// when this returns `Ok`, the stream has been checked to end where
    /// liblzma checks it -- with the range decoder's `code` at zero.
    // One function in liblzma; kept as one so that the two can be read side
    // by side. Every count is bounded: lengths by 273, distances by 2^32,
    // positions by the output's length.
    #[allow(clippy::too_many_lines)]
    pub(crate) fn decode(
        &mut self,
        rc: &mut Rc<'_>,
        out: &mut Vec<u8>,
        dict: Dict,
        size: Option<usize>,
        cap: usize,
    ) -> Result<End> {
        let pos_mask = (1u32 << self.props.pb).wrapping_sub(1);
        let lp_mask = (1u32 << self.props.lp).wrapping_sub(1);
        let lc = self.props.lc;
        // A declared size is not checked against the cap here: liblzma
        // decodes towards it and calls the stream corrupt if it is wrong, so
        // only output actually produced may hit the cap.
        let target = size.map(|n| out.len().saturating_add(n));
        let mut state = self.state;

        loop {
            if target.is_some_and(|t| out.len() >= t) {
                break;
            }
            let pos = dict.pos(out);
            let pos_state = usize::try_from(pos & pos_mask).unwrap_or(0);
            let st = usize::try_from(state).unwrap_or(0);

            let is_match = self
                .is_match
                .get_mut(st)
                .and_then(|row| row.get_mut(pos_state))
                .ok_or(Error::InvalidData)?;
            if rc.bit(is_match)? == 0 {
                // A literal.
                let prev = u32::from(dict.get(out, 0));
                let coder = usize::try_from(
                    ((pos & lp_mask) << lc).wrapping_add(prev >> 8u32.wrapping_sub(lc)),
                )
                .unwrap_or(usize::MAX);
                let base = coder.saturating_mul(LITERAL_CODER_SIZE);
                let probs = self
                    .literal
                    .get_mut(base..base.saturating_add(LITERAL_CODER_SIZE))
                    .ok_or(Error::InvalidData)?;
                let mut symbol = 1u32;
                if state < LIT_STATES {
                    for _ in 0..8 {
                        let b = rc.bit_at(probs, usize::try_from(symbol).unwrap_or(usize::MAX))?;
                        symbol = (symbol << 1) | b;
                    }
                } else {
                    // A "matched" literal: the byte at the last match's
                    // distance steers the probabilities until the first bit
                    // where the two differ.
                    let mut len = u32::from(dict.get(out, self.rep[0])) << 1;
                    let mut offset = 0x100u32;
                    for _ in 0..8 {
                        let match_bit = len & offset;
                        // At most 0x100 + 0x100 + 0xff.
                        let index = offset.wrapping_add(match_bit).wrapping_add(symbol);
                        let b = rc.bit_at(probs, usize::try_from(index).unwrap_or(usize::MAX))?;
                        if b == 0 {
                            symbol <<= 1;
                            offset &= !match_bit;
                        } else {
                            symbol = (symbol << 1) | 1;
                            offset &= match_bit;
                        }
                        len <<= 1;
                    }
                }
                state = NEXT_STATE_AFTER_LITERAL
                    .get(st)
                    .copied()
                    .ok_or(Error::InvalidData)?;
                if out.len() >= cap {
                    return Err(Error::OutputTooLarge);
                }
                out.push(symbol.to_le_bytes()[0]);
                continue;
            }

            // A match: a run of bytes repeated from the output.
            let len;
            let is_rep = self.is_rep.get_mut(st).ok_or(Error::InvalidData)?;
            if rc.bit(is_rep)? == 0 {
                // A new distance.
                state = if state < LIT_STATES {
                    STATE_LIT_MATCH
                } else {
                    STATE_NONLIT_MATCH
                };
                self.rep[3] = self.rep[2];
                self.rep[2] = self.rep[1];
                self.rep[1] = self.rep[0];

                len = self.match_len.decode(rc, pos_state)?;

                let dist_state =
                    usize::try_from(len.wrapping_sub(MATCH_LEN_MIN).min(LAST_DIST_STATE))
                        .unwrap_or(0);
                let probs = self
                    .dist_slot
                    .get_mut(dist_state)
                    .ok_or(Error::InvalidData)?;
                // A 6-bit tree's symbol is 64 to 127.
                let slot = rc
                    .bittree(probs, DIST_SLOT_BITS)?
                    .wrapping_sub(1 << DIST_SLOT_BITS);

                let mut rep0;
                if slot < DIST_MODEL_START {
                    rep0 = slot;
                } else {
                    // `slot` is 4 to 63 here, so `limit` is 1 to 30.
                    let mut limit = (slot >> 1).wrapping_sub(1);
                    rep0 = 2 | (slot & 1);
                    if slot < DIST_MODEL_END {
                        // Distances 4 to 127: the low bits by a reverse bit
                        // tree inside `pos_special`, starting at
                        // `rep0 - slot - 1` (which can be -1: the tree is
                        // indexed from 1).
                        rep0 <<= limit;
                        let base = i64::from(rep0)
                            .wrapping_sub(i64::from(slot))
                            .wrapping_sub(1);
                        let mut symbol = 1i64;
                        let mut offset = 0u32;
                        while limit > 0 {
                            let i =
                                usize::try_from(base.wrapping_add(symbol)).unwrap_or(usize::MAX);
                            let b = rc.bit_at(&mut self.pos_special, i)?;
                            symbol = (symbol << 1) | i64::from(b);
                            if b == 1 {
                                rep0 = rep0.wrapping_add(1 << offset);
                            }
                            offset = offset.wrapping_add(1);
                            limit = limit.wrapping_sub(1);
                        }
                    } else {
                        // Distances from 128: direct bits, then four
                        // aligned bits with probabilities.
                        // `slot` is 14 or more here, so `limit` is at least 6.
                        limit = limit.wrapping_sub(ALIGN_BITS);
                        while limit > 0 {
                            rc.direct(&mut rep0)?;
                            limit = limit.wrapping_sub(1);
                        }
                        rep0 <<= ALIGN_BITS;
                        let mut symbol = 1usize;
                        for offset in 0..ALIGN_BITS {
                            let b = rc.bit_at(&mut self.pos_align, symbol)?;
                            symbol = (symbol << 1) | usize::try_from(b).unwrap_or(0);
                            if b == 1 {
                                rep0 = rep0.wrapping_add(1 << offset);
                            }
                        }

                        if rep0 == u32::MAX {
                            // The end-of-payload marker, which a stream of
                            // known size must not have.
                            if size.is_some() {
                                return Err(Error::InvalidData);
                            }
                            rc.normalize()?;
                            self.state = state;
                            self.rep[0] = rep0;
                            if !rc.is_finished() {
                                return Err(Error::InvalidData);
                            }
                            return Ok(End::Marker);
                        }
                    }
                }
                self.rep[0] = rep0;

                // `dict_is_distance_valid`
                if usize::try_from(rep0).unwrap_or(usize::MAX) >= dict.full(out) {
                    return Err(Error::InvalidData);
                }
            } else {
                // A repeated distance -- impossible before any output.
                if dict.full(out) == 0 {
                    return Err(Error::InvalidData);
                }
                let is_rep0 = self.is_rep0.get_mut(st).ok_or(Error::InvalidData)?;
                if rc.bit(is_rep0)? == 0 {
                    let long = self
                        .is_rep0_long
                        .get_mut(st)
                        .and_then(|row| row.get_mut(pos_state))
                        .ok_or(Error::InvalidData)?;
                    if rc.bit(long)? == 0 {
                        // A "short rep": one byte from distance rep0.
                        state = if state < LIT_STATES {
                            STATE_LIT_SHORTREP
                        } else {
                            STATE_NONLIT_REP
                        };
                        let b = dict.get(out, self.rep[0]);
                        if out.len() >= cap {
                            return Err(Error::OutputTooLarge);
                        }
                        out.push(b);
                        continue;
                    }
                } else {
                    let is_rep1 = self.is_rep1.get_mut(st).ok_or(Error::InvalidData)?;
                    if rc.bit(is_rep1)? == 0 {
                        self.rep.swap(0, 1);
                    } else {
                        let is_rep2 = self.is_rep2.get_mut(st).ok_or(Error::InvalidData)?;
                        if rc.bit(is_rep2)? == 0 {
                            let distance = self.rep[2];
                            self.rep[2] = self.rep[1];
                            self.rep[1] = self.rep[0];
                            self.rep[0] = distance;
                        } else {
                            let distance = self.rep[3];
                            self.rep[3] = self.rep[2];
                            self.rep[2] = self.rep[1];
                            self.rep[1] = self.rep[0];
                            self.rep[0] = distance;
                        }
                    }
                }
                state = if state < LIT_STATES {
                    STATE_LIT_LONGREP
                } else {
                    STATE_NONLIT_REP
                };
                len = self.rep_len.decode(rc, pos_state)?;
            }

            // `dict_repeat`: a match running past a known size is corrupt.
            let len = usize::try_from(len).unwrap_or(usize::MAX);
            if target.is_some_and(|t| len > t.saturating_sub(out.len())) {
                return Err(Error::InvalidData);
            }
            if len > cap.saturating_sub(out.len()) {
                return Err(Error::OutputTooLarge);
            }
            copy_match(out, self.rep[0], len)?;
        }

        // The size is reached: one last normalisation, then the range
        // decoder must be finished.
        rc.normalize()?;
        self.state = state;
        if !rc.is_finished() {
            return Err(Error::InvalidData);
        }
        Ok(End::Size)
    }
}

/// Appends `len` bytes copied from `distance + 1` back, overlapping when the
/// distance is shorter than the length. The distance has been checked to lie
/// inside the dictionary.
fn copy_match(out: &mut Vec<u8>, distance: u32, len: usize) -> Result<()> {
    let back = usize::try_from(distance)
        .unwrap_or(usize::MAX)
        .saturating_add(1);
    let from = out.len().checked_sub(back).ok_or(Error::InvalidData)?;
    if back >= len {
        out.extend_from_within(from..from.saturating_add(len));
    } else {
        for i in 0..len {
            let b = out
                .get(from.saturating_add(i))
                .copied()
                .ok_or(Error::InvalidData)?;
            out.push(b);
        }
    }
    Ok(())
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]
mod tests {
    use super::*;

    #[test]
    fn properties_bytes_round_trip() {
        for byte in 0..=255u8 {
            match Props::from_byte(byte) {
                Some(p) => {
                    assert!(p.lc + p.lp <= 4 && p.pb <= 4);
                    assert_eq!(p.to_byte(), byte);
                }
                None => assert!(
                    byte > 224 || {
                        let b = u32::from(byte) % 45;
                        b / 9 + b % 9 > 4
                    }
                ),
            }
        }
        // xz's default: lc=3, lp=0, pb=2.
        assert_eq!(
            Props::from_byte(0x5d),
            Some(Props {
                lc: 3,
                lp: 0,
                pb: 2
            })
        );
    }

    #[test]
    fn the_window_is_liblzmas() {
        assert_eq!(Dict::window_for(0), 4096);
        assert_eq!(Dict::window_for(4097), 4112);
        assert_eq!(Dict::window_for(1 << 23), 1 << 23);
        assert_eq!(Dict::window_for(u32::MAX), (1usize << 32));
    }

    #[test]
    fn the_first_range_coder_byte_must_be_zero() {
        assert!(Rc::new(&[0, 1, 2, 3, 4], 0).is_ok());
        assert!(matches!(
            Rc::new(&[1, 0, 0, 0, 0], 0),
            Err(Error::InvalidData)
        ));
        assert!(matches!(Rc::new(&[0, 0, 0], 0), Err(Error::UnexpectedEnd)));
        // A non-zero first byte is refused before the rest is wanted.
        assert!(matches!(Rc::new(&[9], 0), Err(Error::InvalidData)));
    }

    #[test]
    fn an_overlapping_match_repeats() {
        let mut out = b"ab".to_vec();
        copy_match(&mut out, 1, 5).unwrap();
        assert_eq!(out, b"abababa");
        copy_match(&mut out, 6, 3).unwrap();
        assert_eq!(out, b"abababaaba");
        assert!(copy_match(&mut out, 10, 1).is_err());
    }

    /// A range encoder and symbol writer for the tests: liblzma's
    /// `range_encoder.h` (`rc_bit`, `rc_shift_low`, `rc_flush`) driving the
    /// same probability model the decoder reads, so that a test can write
    /// exactly the symbol it wants a stream to hold -- including ones no
    /// encoder would write.
    struct Writer {
        model: Decoder,
        low: u64,
        range: u32,
        cache: u8,
        cache_size: u64,
        out: Vec<u8>,
        state: usize,
        /// Bytes the stream stands for so far: the position bits' source.
        pos: u32,
    }

    impl Writer {
        fn new() -> Self {
            Self {
                model: Decoder::new(Props {
                    lc: 3,
                    lp: 0,
                    pb: 2,
                }),
                low: 0,
                range: u32::MAX,
                cache: 0,
                cache_size: 1,
                // The decoder's five initial bytes start with this zero.
                out: Vec::new(),
                state: 0,
                pos: 0,
            }
        }

        fn shift_low(&mut self) {
            if (self.low as u32) < 0xff00_0000 || (self.low >> 32) != 0 {
                loop {
                    self.out
                        .push(self.cache.wrapping_add((self.low >> 32) as u8));
                    self.cache = 0xff;
                    self.cache_size -= 1;
                    if self.cache_size == 0 {
                        break;
                    }
                }
                self.cache = ((self.low >> 24) & 0xff) as u8;
            }
            self.cache_size += 1;
            self.low = (self.low & 0x00ff_ffff) << 8;
        }

        fn bit(&mut self, prob: Prob, bit: u32) {
            if self.range < RC_TOP {
                self.shift_low();
                self.range <<= 8;
            }
            let p = self.prob(prob);
            let pv = u32::from(*p);
            if bit == 0 {
                self.range = (self.range >> BIT_MODEL_TOTAL_BITS) * pv;
                *self.prob(prob) = (pv + ((BIT_MODEL_TOTAL - pv) >> MOVE_BITS)) as u16;
            } else {
                let bound = pv * (self.range >> BIT_MODEL_TOTAL_BITS);
                self.low += u64::from(bound);
                self.range -= bound;
                *self.prob(prob) = (pv - (pv >> MOVE_BITS)) as u16;
            }
        }

        fn prob(&mut self, which: Prob) -> &mut u16 {
            let m = &mut self.model;
            match which {
                Prob::IsMatch(s, p) => &mut m.is_match[s][p],
                Prob::IsRep(s) => &mut m.is_rep[s],
                Prob::IsRep0(s) => &mut m.is_rep0[s],
                Prob::IsRep0Long(s, p) => &mut m.is_rep0_long[s][p],
                Prob::Literal(i) => &mut m.literal[i],
                Prob::Choice => &mut m.match_len.choice,
                Prob::Low(p, i) => &mut m.match_len.low[p][i],
                Prob::Slot(d, i) => &mut m.dist_slot[d][i],
            }
        }

        fn flush(mut self) -> Vec<u8> {
            self.range = u32::MAX;
            for _ in 0..5 {
                self.shift_low();
            }
            self.out
        }

        fn pos_state(&self) -> usize {
            (self.pos & 3) as usize
        }

        /// A literal, after a literal (no match byte).
        fn literal(&mut self, byte: u8, prev: u8) {
            assert!(self.state < 7, "the writer only writes plain literals");
            let ps = self.pos_state();
            self.bit(Prob::IsMatch(self.state, ps), 0);
            let base = (usize::from(prev) >> 5) * LITERAL_CODER_SIZE;
            let mut symbol = 1usize;
            for i in (0..8).rev() {
                let b = u32::from(byte >> i) & 1;
                self.bit(Prob::Literal(base + symbol), b);
                symbol = (symbol << 1) | b as usize;
            }
            self.state = NEXT_STATE_AFTER_LITERAL[self.state] as usize;
            self.pos += 1;
        }

        /// A match of length 2 at a distance from 0 to 3 (one distance slot,
        /// no footer bits).
        fn short_match(&mut self, distance: u32) {
            assert!(distance < 4);
            let ps = self.pos_state();
            self.bit(Prob::IsMatch(self.state, ps), 1);
            self.bit(Prob::IsRep(self.state), 0);
            // Length 2: choice 0, low tree symbol 0.
            self.bit(Prob::Choice, 0);
            let mut m = 1usize;
            for _ in 0..3 {
                self.bit(Prob::Low(ps, m), 0);
                m <<= 1;
            }
            // Distance slot `distance`, dist state 0 (length 2).
            let mut m = 1usize;
            for i in (0..6).rev() {
                let b = (distance >> i) & 1;
                self.bit(Prob::Slot(0, m), b);
                m = (m << 1) | b as usize;
            }
            self.state = if self.state < 7 { 7 } else { 10 };
            self.pos += 2;
        }

        /// A "short rep": one byte from distance rep0.
        fn short_rep(&mut self) {
            let ps = self.pos_state();
            self.bit(Prob::IsMatch(self.state, ps), 1);
            self.bit(Prob::IsRep(self.state), 1);
            self.bit(Prob::IsRep0(self.state), 0);
            self.bit(Prob::IsRep0Long(self.state, ps), 0);
            self.state = if self.state < 7 { 9 } else { 11 };
            self.pos += 1;
        }
    }

    #[derive(Clone, Copy)]
    enum Prob {
        IsMatch(usize, usize),
        IsRep(usize),
        IsRep0(usize),
        IsRep0Long(usize, usize),
        Literal(usize),
        Choice,
        Low(usize, usize),
        Slot(usize, usize),
    }

    /// Decodes `stream` onto `prefix`, the dictionary starting after it (as
    /// an LZMA2 dictionary reset leaves earlier output outside it).
    fn decode_after(prefix: &[u8], stream: &[u8], size: usize) -> Result<Vec<u8>> {
        let mut out = prefix.to_vec();
        let mut rc = Rc::new(stream, 0)?;
        let mut d = Decoder::new(Props {
            lc: 3,
            lp: 0,
            pb: 2,
        });
        let dict = Dict {
            start: prefix.len(),
            window: 4096,
        };
        d.decode(&mut rc, &mut out, dict, Some(size), usize::MAX)?;
        Ok(out.split_off(prefix.len()))
    }

    /// `dict_is_distance_valid`: a distance must be below the bytes in the
    /// dictionary -- equal to them reaches one byte before it. Output from
    /// before the dictionary's reset is outside it, though it is there.
    #[test]
    fn a_distance_must_lie_inside_the_dictionary() {
        let mut w = Writer::new();
        w.literal(b'a', 0);
        w.literal(b'b', b'a');
        w.short_match(1);
        assert_eq!(decode_after(b"0123456789", &w.flush(), 4).unwrap(), b"abab");

        let mut w = Writer::new();
        w.literal(b'a', 0);
        w.literal(b'b', b'a');
        w.short_match(2);
        assert_eq!(
            decode_after(b"0123456789", &w.flush(), 4),
            Err(Error::InvalidData)
        );
    }

    /// A repeated match needs a byte to repeat: liblzma refuses one before
    /// any output, whatever came before the dictionary's reset.
    #[test]
    fn a_repeated_match_needs_output_to_repeat() {
        let mut w = Writer::new();
        w.short_rep();
        assert_eq!(decode_after(b"xyz", &w.flush(), 1), Err(Error::InvalidData));

        let mut w = Writer::new();
        w.literal(b'q', 0);
        w.short_rep();
        assert_eq!(decode_after(b"xyz", &w.flush(), 2).unwrap(), b"qq");
    }
}
