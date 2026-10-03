//! LZMA decoding exactly as 7-Zip does it: `C/LzmaDec.c` from the LZMA SDK
//! 26.00 (Igor Pavlov, public domain), ported for the 7z reader.
//!
//! The `xz` crate decodes LZMA as liblzma 5.2.5 does. On a sound stream the
//! two give the same bytes; on a damaged one they part ways, and a 7z reader
//! is judged by failing where 7-Zip fails and giving back what 7-Zip gives
//! back. Two differences the corruption corpus (`tests/data/mutations.txt`)
//! shows:
//! - a match running past the end of the output: liblzma refuses it whole,
//!   `LzmaDec` copies it up to the end and then calls the stream damaged --
//!   so the files it completes are judged by their CRCs rather than lost;
//! - an LZMA2 chunk's input: liblzma's range decoder reads on past the
//!   chunk's declared packed size, `Lzma2Dec` gives it that size and no more
//!   (`lzma2_dec.rs`).
//!
//! The port keeps `LzmaDec.c`'s structure, names and probability layout so
//! that the two read side by side, with two changes of representation:
//! - The dictionary is the output itself, a `Vec<u8>` whose length is
//!   `dicPos`. 7-Zip's is a ring at least the dictionary's size, which holds
//!   every byte a distance may reach -- a distance is checked against the
//!   bytes since the last dictionary reset (`processedPos`, then
//!   `checkDicSize`) -- so no decision differs.
//! - Reads of the input and of the dictionary are checked. Where `LzmaDec.c`
//!   relies on its caller for room, a read out of range here is its "cannot
//!   happen" error, `SZ_ERROR_FAIL`.

// The arithmetic is `LzmaDec.c`'s. The range coder's is on `u32` and wraps
// where the C code relies on wrapping (`wrapping_*`); every other sum is of
// a probability (below 2^11), a length (below 2^10), a symbol (below 2^9)
// or an index into a buffer this module checks before using.
#![allow(clippy::arithmetic_side_effects)]

use alloc::vec;
use alloc::vec::Vec;

/// `kTopValue`
const TOP_VALUE: u32 = 1 << 24;
/// `kNumBitModelTotalBits`
const NUM_BIT_MODEL_TOTAL_BITS: u32 = 11;
/// `kBitModelTotal`
const BIT_MODEL_TOTAL: u32 = 1 << NUM_BIT_MODEL_TOTAL_BITS;
/// `kNumMoveBits`
const NUM_MOVE_BITS: u32 = 5;
/// `RC_INIT_SIZE`
const RC_INIT_SIZE: usize = 5;
/// `LZMA_REQUIRED_INPUT_MAX`: the most input one symbol and the
/// normalisation after it can take.
pub(crate) const REQUIRED_INPUT_MAX: usize = 20;

/// `kNumPosBitsMax`
const NUM_POS_BITS_MAX: u32 = 4;
/// `kLenNumLowBits`
const LEN_NUM_LOW_BITS: u32 = 3;
/// `kLenNumLowSymbols`
const LEN_NUM_LOW_SYMBOLS: u32 = 1 << LEN_NUM_LOW_BITS;
/// `kLenNumHighBits`
const LEN_NUM_HIGH_BITS: u32 = 8;
/// `kLenNumHighSymbols`
const LEN_NUM_HIGH_SYMBOLS: u32 = 1 << LEN_NUM_HIGH_BITS;
/// `LenLow`
const LEN_LOW: usize = 0;
/// `LenHigh`
const LEN_HIGH: usize = LEN_LOW + 2 * (16 << LEN_NUM_LOW_BITS);
/// `kNumLenProbs`
const NUM_LEN_PROBS: usize = LEN_HIGH + LEN_NUM_HIGH_SYMBOLS as usize;
/// `LenChoice`
const LEN_CHOICE: usize = LEN_LOW;
/// `LenChoice2`
const LEN_CHOICE2: usize = LEN_LOW + (1 << LEN_NUM_LOW_BITS);

/// `kNumStates`
const NUM_STATES: u32 = 12;
/// `kNumStates2`
const NUM_STATES2: usize = 16;
/// `kNumLitStates`
const NUM_LIT_STATES: u32 = 7;
/// `kStartPosModelIndex`
const START_POS_MODEL_INDEX: u32 = 4;
/// `kEndPosModelIndex`
const END_POS_MODEL_INDEX: u32 = 14;
/// `kNumFullDistances`
const NUM_FULL_DISTANCES: usize = 1 << (END_POS_MODEL_INDEX >> 1);
/// `kNumPosSlotBits`
const NUM_POS_SLOT_BITS: u32 = 6;
/// `kNumLenToPosStates`
const NUM_LEN_TO_POS_STATES: u32 = 4;
/// `kNumAlignBits`
const NUM_ALIGN_BITS: u32 = 4;
/// `kAlignTableSize`
const ALIGN_TABLE_SIZE: usize = 1 << NUM_ALIGN_BITS;
/// `kMatchMinLen`
const MATCH_MIN_LEN: u32 = 2;
/// `kMatchSpecLenStart`: `remainLen` after the end marker; one more is
/// "start the range coder", two more "and reset the state".
pub(crate) const MATCH_SPEC_LEN_START: u32 =
    MATCH_MIN_LEN + LEN_NUM_LOW_SYMBOLS * 2 + LEN_NUM_HIGH_SYMBOLS;
/// `kMatchSpecLen_Error_Data`
const MATCH_SPEC_LEN_ERROR_DATA: u32 = 1 << 9;
/// `kMatchSpecLen_Error_Fail`
const MATCH_SPEC_LEN_ERROR_FAIL: u32 = MATCH_SPEC_LEN_ERROR_DATA - 1;

// The probabilities, at `LzmaDec.c`'s offsets from `probs_1664` plus 1664,
// which makes them offsets from the start of the array.
/// `SpecPos`
const SPEC_POS: usize = 0;
/// `IsRep0Long`
const IS_REP0_LONG: usize = SPEC_POS + NUM_FULL_DISTANCES;
/// `RepLenCoder`
const REP_LEN_CODER: usize = IS_REP0_LONG + (NUM_STATES2 << NUM_POS_BITS_MAX);
/// `LenCoder`
const LEN_CODER: usize = REP_LEN_CODER + NUM_LEN_PROBS;
/// `IsMatch`
const IS_MATCH: usize = LEN_CODER + NUM_LEN_PROBS;
/// `Align`
const ALIGN: usize = IS_MATCH + (NUM_STATES2 << NUM_POS_BITS_MAX);
/// `IsRep`
const IS_REP: usize = ALIGN + ALIGN_TABLE_SIZE;
/// `IsRepG0`
const IS_REP_G0: usize = IS_REP + NUM_STATES as usize;
/// `IsRepG1`
const IS_REP_G1: usize = IS_REP_G0 + NUM_STATES as usize;
/// `IsRepG2`
const IS_REP_G2: usize = IS_REP_G1 + NUM_STATES as usize;
/// `PosSlot`
const POS_SLOT: usize = IS_REP_G2 + NUM_STATES as usize;
/// `Literal`
const LITERAL: usize = POS_SLOT + ((NUM_LEN_TO_POS_STATES as usize) << NUM_POS_SLOT_BITS);
/// `NUM_BASE_PROBS`
const NUM_BASE_PROBS: usize = LITERAL;
/// `LZMA_LIT_SIZE`
const LIT_SIZE: usize = 0x300;

// `LzmaDec.c` stops compiling if its layout is not this.
const _: () = assert!(ALIGN == 1664 && NUM_BASE_PROBS == 1984);

/// `LZMA_DIC_MIN`
const DIC_MIN: u32 = 1 << 12;

/// `kRange0`
const RANGE0: u32 = 0xFFFF_FFFF;
/// `kBound0`
const BOUND0: u32 = (RANGE0 >> NUM_BIT_MODEL_TOTAL_BITS) << (NUM_BIT_MODEL_TOTAL_BITS - 1);
/// `kBadRepCode`: an initial `code` from which the first symbol would be a
/// repeated match, impossible before any output.
const BAD_REP_CODE: u32 =
    BOUND0 + (((RANGE0 - BOUND0) >> NUM_BIT_MODEL_TOTAL_BITS) << (NUM_BIT_MODEL_TOTAL_BITS - 1));
const _: () = assert!(BAD_REP_CODE == 0xC000_0000 - 0x400);

/// `ELzmaFinishMode`
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Finish {
    /// `LZMA_FINISH_ANY`: stop at the limit, whatever comes next.
    Any,
    /// `LZMA_FINISH_END`: the stream must end at the limit.
    End,
}

/// `ELzmaStatus`
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Status {
    /// `LZMA_STATUS_NOT_SPECIFIED`
    NotSpecified,
    /// `LZMA_STATUS_FINISHED_WITH_MARK`
    FinishedWithMark,
    /// `LZMA_STATUS_NOT_FINISHED`
    NotFinished,
    /// `LZMA_STATUS_NEEDS_MORE_INPUT`
    NeedsMoreInput,
    /// `LZMA_STATUS_MAYBE_FINISHED_WITHOUT_MARK`
    MaybeFinishedWithoutMark,
}

/// A decoder's error: `SZ_ERROR_DATA` or `SZ_ERROR_FAIL`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Fault {
    /// `SZ_ERROR_DATA`: the stream is damaged.
    Data,
    /// `SZ_ERROR_FAIL`: a state the code cannot reach, which 7-Zip reports
    /// as an internal error.
    Fail,
}

/// What one `*_DecodeToDic` call did: `*srcLen`, `*status` and its `SRes`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Step {
    pub(crate) consumed: usize,
    pub(crate) status: Status,
    pub(crate) fault: Option<Fault>,
}

impl Step {
    const fn ok(consumed: usize, status: Status) -> Self {
        Self {
            consumed,
            status,
            fault: None,
        }
    }

    const fn fault(consumed: usize, status: Status, fault: Fault) -> Self {
        Self {
            consumed,
            status,
            fault: Some(fault),
        }
    }
}

/// `CLzmaProps`
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Props {
    pub(crate) lc: u32,
    pub(crate) lp: u32,
    pub(crate) pb: u32,
    pub(crate) dic_size: u32,
}

impl Props {
    /// `LzmaProps_Decode`: the five property bytes (more are ignored);
    /// `None` is `SZ_ERROR_UNSUPPORTED`.
    pub(crate) fn decode(data: &[u8]) -> Option<Self> {
        let (&d, rest) = data.split_first()?;
        let dic: [u8; 4] = rest.get(..4)?.try_into().ok()?;
        let dic_size = u32::from_le_bytes(dic).max(DIC_MIN);
        if d >= 9 * 5 * 5 {
            return None;
        }
        Some(Self {
            lc: u32::from(d % 9),
            lp: u32::from(d / 9 % 5),
            pb: u32::from(d / 9 / 5),
            dic_size,
        })
    }

    /// `LzmaProps_GetNumProbs`
    fn num_probs(self) -> usize {
        NUM_BASE_PROBS + (LIT_SIZE << (self.lc + self.lp))
    }
}

/// What the next symbol is, by `LzmaDec_TryDummy` (`ELzmaDummy` without
/// `DUMMY_INPUT_EOF`, which is `None` where it is returned).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Dummy {
    Lit,
    Match,
    Rep,
}

/// A probability after a 0 or a 1, as `UPDATE_0` and `UPDATE_1` make it.
/// Every probability is below `BIT_MODEL_TOTAL` (2^11), so neither
/// truncates.
#[allow(clippy::cast_possible_truncation)]
const fn prob16(v: u32) -> u16 {
    v as u16
}

/// The byte `rep` back from the end of `out` -- `dic[dicPos - rep]` -- or
/// `SZ_ERROR_FAIL` where there is none.
fn back(out: &[u8], rep: u32) -> Result<u8, Fault> {
    let rep = usize::try_from(rep).map_err(|_| Fault::Fail)?;
    out.len()
        .checked_sub(rep)
        .and_then(|i| out.get(i))
        .copied()
        .ok_or(Fault::Fail)
}

/// Appends `len` bytes copied from `rep` back, overlapping where `rep` is
/// shorter than `len`.
fn copy_back(out: &mut Vec<u8>, rep: u32, len: usize) -> Result<(), Fault> {
    let rep = usize::try_from(rep).map_err(|_| Fault::Fail)?;
    let from = out.len().checked_sub(rep).ok_or(Fault::Fail)?;
    if rep >= len {
        out.extend_from_within(from..from + len);
    } else {
        out.reserve(len);
        for i in from..from + len {
            let b = *out.get(i).ok_or(Fault::Fail)?;
            out.push(b);
        }
    }
    Ok(())
}

/// `CLzmaDec`, with the dictionary passed to each call as the output.
#[derive(Clone, Debug)]
pub(crate) struct LzmaDec {
    pub(crate) prop: Props,
    probs: Vec<u16>,
    range: u32,
    code: u32,
    /// The bytes since the last dictionary reset, modulo 2^32.
    pub(crate) processed_pos: u32,
    /// Zero until `processed_pos` has reached the dictionary's size, then
    /// that size: how far back a distance may reach.
    pub(crate) check_dic_size: u32,
    reps: [u32; 4],
    state: u32,
    /// The bytes of a match still to copy, or one of the `kMatchSpecLen*`
    /// states.
    pub(crate) remain_len: u32,
    temp_buf_size: usize,
    temp_buf: [u8; REQUIRED_INPUT_MAX],
}

impl LzmaDec {
    /// `LzmaDec_Allocate` (or `..._AllocateProbs`) for `prop`, then
    /// `LzmaDec_Init`.
    pub(crate) fn new(prop: Props) -> Self {
        let mut dec = Self {
            prop,
            probs: vec![0; prop.num_probs()],
            range: 0,
            code: 0,
            processed_pos: 0,
            check_dic_size: 0,
            reps: [0; 4],
            state: 0,
            remain_len: 0,
            temp_buf_size: 0,
            temp_buf: [0; REQUIRED_INPUT_MAX],
        };
        dec.init_dic_and_state(true, true);
        dec
    }

    /// `LzmaDec_InitDicAndState`
    pub(crate) fn init_dic_and_state(&mut self, init_dic: bool, init_state: bool) {
        self.remain_len = MATCH_SPEC_LEN_START + 1;
        self.temp_buf_size = 0;
        if init_dic {
            self.processed_pos = 0;
            self.check_dic_size = 0;
            self.remain_len = MATCH_SPEC_LEN_START + 2;
        }
        if init_state {
            self.remain_len = MATCH_SPEC_LEN_START + 2;
        }
    }

    /// `LzmaDec_UpdateWithUncompressed` (in `Lzma2Dec.c`): an LZMA2
    /// uncompressed chunk's bytes, into the dictionary.
    pub(crate) fn update_with_uncompressed(&mut self, out: &mut Vec<u8>, src: &[u8]) {
        out.extend_from_slice(src);
        let size = u32::try_from(src.len()).unwrap_or(u32::MAX);
        if self.check_dic_size == 0 && self.prop.dic_size.wrapping_sub(self.processed_pos) <= size {
            self.check_dic_size = self.prop.dic_size;
        }
        self.processed_pos = self.processed_pos.wrapping_add(size);
    }

    /// `LzmaDec_DecodeToDic`: decodes `src` onto `out` up to `dic_limit`
    /// bytes in all.
    pub(crate) fn decode_to_dic(
        &mut self,
        out: &mut Vec<u8>,
        dic_limit: usize,
        src: &[u8],
        finish: Finish,
    ) -> Step {
        match self.decode_to_dic_inner(out, dic_limit, src, finish) {
            Ok(step) => step,
            Err(consumed) => {
                self.remain_len = MATCH_SPEC_LEN_ERROR_FAIL;
                Step::fault(consumed, Status::NotSpecified, Fault::Fail)
            }
        }
    }

    /// `LzmaDec_DecodeToDic`; `Err` is the `SZ_ERROR_FAIL` exit, with the
    /// input taken so far.
    #[allow(clippy::too_many_lines)]
    fn decode_to_dic_inner(
        &mut self,
        out: &mut Vec<u8>,
        dic_limit: usize,
        src: &[u8],
        finish: Finish,
    ) -> Result<Step, usize> {
        let mut in_size = src.len();
        let mut consumed = 0usize;
        // `src` as the C code advances it.
        let mut s = 0usize;

        if self.remain_len > MATCH_SPEC_LEN_START {
            if self.remain_len > MATCH_SPEC_LEN_START + 2 {
                let fault = if self.remain_len == MATCH_SPEC_LEN_ERROR_FAIL {
                    Fault::Fail
                } else {
                    Fault::Data
                };
                return Ok(Step::fault(consumed, Status::NotSpecified, fault));
            }
            while in_size > 0 && self.temp_buf_size < RC_INIT_SIZE {
                let b = *src.get(s).ok_or(consumed)?;
                *self.temp_buf.get_mut(self.temp_buf_size).ok_or(consumed)? = b;
                self.temp_buf_size += 1;
                s += 1;
                consumed += 1;
                in_size -= 1;
            }
            if self.temp_buf_size != 0 && self.temp_buf[0] != 0 {
                return Ok(Step::fault(consumed, Status::NotSpecified, Fault::Data));
            }
            if self.temp_buf_size < RC_INIT_SIZE {
                return Ok(Step::ok(consumed, Status::NeedsMoreInput));
            }
            self.code = u32::from_be_bytes([
                self.temp_buf[1],
                self.temp_buf[2],
                self.temp_buf[3],
                self.temp_buf[4],
            ]);
            if self.check_dic_size == 0 && self.processed_pos == 0 && self.code >= BAD_REP_CODE {
                return Ok(Step::fault(consumed, Status::NotSpecified, Fault::Data));
            }
            self.range = 0xFFFF_FFFF;
            self.temp_buf_size = 0;
            if self.remain_len > MATCH_SPEC_LEN_START + 1 {
                let n = self.prop.num_probs();
                let probs = self.probs.get_mut(..n).ok_or(consumed)?;
                probs.fill(prob16(BIT_MODEL_TOTAL >> 1));
                self.reps = [1; 4];
                self.state = 0;
            }
            self.remain_len = 0;
        }

        loop {
            if self.remain_len == MATCH_SPEC_LEN_START {
                if self.code != 0 {
                    return Ok(Step::fault(consumed, Status::NotSpecified, Fault::Data));
                }
                return Ok(Step::ok(consumed, Status::FinishedWithMark));
            }

            self.write_rem(out, dic_limit).map_err(|_| consumed)?;

            let mut check_end_mark_now = false;
            if out.len() >= dic_limit {
                if self.remain_len == 0 && self.code == 0 {
                    return Ok(Step::ok(consumed, Status::MaybeFinishedWithoutMark));
                }
                if finish == Finish::Any {
                    return Ok(Step::ok(consumed, Status::NotFinished));
                }
                if self.remain_len != 0 {
                    // RETURN_NOT_FINISHED_FOR_FINISH
                    return Ok(Step::fault(consumed, Status::NotFinished, Fault::Data));
                }
                check_end_mark_now = true;
            }

            if self.temp_buf_size == 0 {
                let rest = src.get(s..s + in_size).ok_or(consumed)?;
                let buf_limit;
                let mut dummy_processed = None;
                if in_size < REQUIRED_INPUT_MAX || check_end_mark_now {
                    match self.try_dummy(out, rest).map_err(|_| consumed)? {
                        None => {
                            if in_size >= REQUIRED_INPUT_MAX {
                                return Err(consumed);
                            }
                            consumed += in_size;
                            self.temp_buf_size = in_size;
                            self.temp_buf
                                .get_mut(..in_size)
                                .ok_or(consumed)?
                                .copy_from_slice(rest);
                            return Ok(Step::ok(consumed, Status::NeedsMoreInput));
                        }
                        Some((res, processed)) => {
                            if processed > REQUIRED_INPUT_MAX {
                                return Err(consumed);
                            }
                            dummy_processed = Some(processed);
                            if check_end_mark_now && res != Dummy::Match {
                                consumed += processed;
                                self.temp_buf_size = processed;
                                self.temp_buf
                                    .get_mut(..processed)
                                    .ok_or(consumed)?
                                    .copy_from_slice(rest.get(..processed).ok_or(consumed)?);
                                // RETURN_NOT_FINISHED_FOR_FINISH
                                return Ok(Step::fault(consumed, Status::NotFinished, Fault::Data));
                            }
                            // One symbol only.
                            buf_limit = 0;
                        }
                    }
                } else {
                    buf_limit = in_size - REQUIRED_INPUT_MAX;
                }

                let (data_error, processed) = self
                    .decode_real2(out, dic_limit, rest, buf_limit)
                    .map_err(|_| consumed)?;
                match dummy_processed {
                    None if processed > in_size => return Err(consumed),
                    Some(d) if d != processed => return Err(consumed),
                    _ => {}
                }
                s += processed;
                in_size -= processed;
                consumed += processed;
                if data_error {
                    self.remain_len = MATCH_SPEC_LEN_ERROR_DATA;
                    return Ok(Step::fault(consumed, Status::NotSpecified, Fault::Data));
                }
                continue;
            }

            // Some input is waiting in `temp_buf`, too little for a symbol.
            let mut rem = self.temp_buf_size;
            let mut ahead = 0usize;
            while rem < REQUIRED_INPUT_MAX && ahead < in_size {
                *self.temp_buf.get_mut(rem).ok_or(consumed)? =
                    *src.get(s + ahead).ok_or(consumed)?;
                rem += 1;
                ahead += 1;
            }
            let mut dummy_processed = None;
            if rem < REQUIRED_INPUT_MAX || check_end_mark_now {
                let temp = self.temp_buf;
                let have = temp.get(..rem).ok_or(consumed)?;
                match self.try_dummy(out, have).map_err(|_| consumed)? {
                    None => {
                        if rem >= REQUIRED_INPUT_MAX {
                            return Err(consumed);
                        }
                        self.temp_buf_size = rem;
                        consumed += ahead;
                        return Ok(Step::ok(consumed, Status::NeedsMoreInput));
                    }
                    Some((res, processed)) => {
                        if processed < self.temp_buf_size {
                            return Err(consumed);
                        }
                        dummy_processed = Some(processed);
                        if check_end_mark_now && res != Dummy::Match {
                            consumed += processed - self.temp_buf_size;
                            self.temp_buf_size = processed;
                            // RETURN_NOT_FINISHED_FOR_FINISH
                            return Ok(Step::fault(consumed, Status::NotFinished, Fault::Data));
                        }
                    }
                }
            }

            // One symbol from `temp_buf`.
            let temp = self.temp_buf;
            let have = temp.get(..rem).ok_or(consumed)?;
            let (data_error, processed) = self
                .decode_real2(out, dic_limit, have, 0)
                .map_err(|_| consumed)?;
            let held = self.temp_buf_size;
            match dummy_processed {
                None if processed > REQUIRED_INPUT_MAX || processed < held => return Err(consumed),
                Some(d) if d != processed => return Err(consumed),
                _ => {}
            }
            let processed = processed - held;
            s += processed;
            in_size -= processed;
            consumed += processed;
            self.temp_buf_size = 0;
            if data_error {
                self.remain_len = MATCH_SPEC_LEN_ERROR_DATA;
                return Ok(Step::fault(consumed, Status::NotSpecified, Fault::Data));
            }
        }
    }

    /// `LzmaDec_WriteRem`: the rest of a match the last call stopped in, as
    /// far as `limit`.
    fn write_rem(&mut self, out: &mut Vec<u8>, limit: usize) -> Result<(), Fault> {
        let mut len = self.remain_len;
        if len == 0 {
            return Ok(());
        }
        let rem = limit.saturating_sub(out.len());
        if rem < len as usize {
            len = u32::try_from(rem).map_err(|_| Fault::Fail)?;
            if len == 0 {
                return Ok(());
            }
        }
        if self.check_dic_size == 0 && self.prop.dic_size.wrapping_sub(self.processed_pos) <= len {
            self.check_dic_size = self.prop.dic_size;
        }
        self.processed_pos = self.processed_pos.wrapping_add(len);
        self.remain_len -= len;
        copy_back(out, self.reps[0], len as usize)
    }

    /// `LzmaDec_DecodeReal2`: keeps the first call that reaches the
    /// dictionary's size from passing it, so that `check_dic_size` is set
    /// exactly there.
    fn decode_real2(
        &mut self,
        out: &mut Vec<u8>,
        mut limit: usize,
        input: &[u8],
        buf_limit: usize,
    ) -> Result<(bool, usize), Fault> {
        if self.check_dic_size == 0 {
            let rem = self.prop.dic_size.wrapping_sub(self.processed_pos) as usize;
            if limit.saturating_sub(out.len()) > rem {
                limit = out.len() + rem;
            }
        }
        let res = self.decode_real(out, limit, input, buf_limit);
        if self.check_dic_size == 0 && self.processed_pos >= self.prop.dic_size {
            self.check_dic_size = self.prop.dic_size;
        }
        res
    }

    /// `LZMA_DECODE_REAL` (`LzmaDec_DecodeReal_3`): decodes symbols from
    /// `input` onto `out` while `out` is short of `limit` and the input
    /// position is below `buf_limit` -- the first symbol in any case -- then
    /// normalises the range coder. Returns whether a match reached outside
    /// the dictionary (`SZ_ERROR_DATA`) and the input position.
    #[allow(clippy::too_many_lines)]
    fn decode_real(
        &mut self,
        out: &mut Vec<u8>,
        limit: usize,
        input: &[u8],
        buf_limit: usize,
    ) -> Result<(bool, usize), Fault> {
        let pb_mask = (1u32 << self.prop.pb) - 1;
        let lc = self.prop.lc;
        let lp_mask = (0x100u32 << self.prop.lp) - (0x100u32 >> lc);
        let mut state = self.state;
        let [mut rep0, mut rep1, mut rep2, mut rep3] = self.reps;
        let mut processed_pos = self.processed_pos;
        let check_dic_size = self.check_dic_size;
        let mut len: u32 = 0;
        let mut buf = 0usize;
        let mut range = self.range;
        let mut code = self.code;
        let probs = &mut self.probs[..];

        macro_rules! normalize {
            () => {
                if range < TOP_VALUE {
                    let b = *input.get(buf).ok_or(Fault::Fail)?;
                    range <<= 8;
                    code = (code << 8) | u32::from(b);
                    buf += 1;
                }
            };
        }
        // IF_BIT_0 with UPDATE_0 / UPDATE_1: whether the bit is 1.
        macro_rules! bit {
            ($i:expr) => {{
                let p = probs.get_mut($i).ok_or(Fault::Fail)?;
                let ttt = u32::from(*p);
                normalize!();
                let bound = (range >> NUM_BIT_MODEL_TOTAL_BITS) * ttt;
                if code < bound {
                    range = bound;
                    *p = prob16(ttt + ((BIT_MODEL_TOTAL - ttt) >> NUM_MOVE_BITS));
                    false
                } else {
                    range -= bound;
                    code -= bound;
                    *p = prob16(ttt - (ttt >> NUM_MOVE_BITS));
                    true
                }
            }};
        }

        'main: loop {
            let pos_state = ((processed_pos & pb_mask) << 4) as usize;
            let st = state as usize;
            'symbol: {
                if !bit!(IS_MATCH + pos_state + st) {
                    let mut prob = LITERAL;
                    if processed_pos != 0 || check_dic_size != 0 {
                        let prev = u32::from(*out.last().ok_or(Fault::Fail)?);
                        prob += 3
                            * ((((processed_pos << 8).wrapping_add(prev) & lp_mask) << lc)
                                as usize);
                    }
                    processed_pos = processed_pos.wrapping_add(1);
                    let mut symbol: u32 = 1;
                    if state < NUM_LIT_STATES {
                        state -= if state < 4 { state } else { 3 };
                        for _ in 0..8 {
                            let b = bit!(prob + symbol as usize);
                            symbol = (symbol << 1) | u32::from(b);
                        }
                    } else {
                        let mut match_byte = u32::from(back(out, rep0)?);
                        let mut offs: u32 = 0x100;
                        state -= if state < 10 { 3 } else { 6 };
                        for _ in 0..8 {
                            match_byte <<= 1;
                            let bit = offs;
                            offs &= match_byte;
                            if bit!(prob + (offs + bit + symbol) as usize) {
                                symbol = (symbol << 1) + 1;
                            } else {
                                symbol <<= 1;
                                offs ^= bit;
                            }
                        }
                    }
                    out.push(symbol.to_le_bytes()[0]);
                    break 'symbol;
                }

                let len_coder;
                if bit!(IS_REP + st) {
                    if bit!(IS_REP_G0 + st) {
                        let distance;
                        if bit!(IS_REP_G1 + st) {
                            if bit!(IS_REP_G2 + st) {
                                distance = rep3;
                                rep3 = rep2;
                            } else {
                                distance = rep2;
                            }
                            rep2 = rep1;
                        } else {
                            distance = rep1;
                        }
                        rep1 = rep0;
                        rep0 = distance;
                    } else if !bit!(IS_REP0_LONG + pos_state + st) {
                        // A "short rep": one byte from rep0. Before any
                        // output it would be refused by `BAD_REP_CODE`.
                        let b = back(out, rep0)?;
                        out.push(b);
                        processed_pos = processed_pos.wrapping_add(1);
                        state = if state < NUM_LIT_STATES { 9 } else { 11 };
                        break 'symbol;
                    }
                    state = if state < NUM_LIT_STATES { 8 } else { 11 };
                    len_coder = REP_LEN_CODER;
                } else {
                    state += NUM_STATES;
                    len_coder = LEN_CODER;
                }

                if bit!(len_coder + LEN_CHOICE) {
                    if bit!(len_coder + LEN_CHOICE2) {
                        let base = len_coder + LEN_HIGH;
                        len = 1;
                        while len < LEN_NUM_HIGH_SYMBOLS {
                            let b = bit!(base + len as usize);
                            len = (len << 1) | u32::from(b);
                        }
                        len -= LEN_NUM_HIGH_SYMBOLS;
                        len += LEN_NUM_LOW_SYMBOLS * 2;
                    } else {
                        let base = len_coder + LEN_LOW + pos_state + (1 << LEN_NUM_LOW_BITS);
                        len = 1;
                        for _ in 0..3 {
                            let b = bit!(base + len as usize);
                            len = (len << 1) | u32::from(b);
                        }
                    }
                } else {
                    let base = len_coder + LEN_LOW + pos_state;
                    len = 1;
                    for _ in 0..3 {
                        let b = bit!(base + len as usize);
                        len = (len << 1) | u32::from(b);
                    }
                    len -= 8;
                }

                if state >= NUM_STATES {
                    let base = POS_SLOT
                        + ((len.min(NUM_LEN_TO_POS_STATES - 1) as usize) << NUM_POS_SLOT_BITS);
                    let mut distance: u32 = 1;
                    for _ in 0..NUM_POS_SLOT_BITS {
                        let b = bit!(base + distance as usize);
                        distance = (distance << 1) | u32::from(b);
                    }
                    distance -= 1 << NUM_POS_SLOT_BITS;
                    if distance >= START_POS_MODEL_INDEX {
                        let pos_slot = distance;
                        let mut num_direct_bits = (distance >> 1) - 1;
                        distance = 2 | (distance & 1);
                        if pos_slot < END_POS_MODEL_INDEX {
                            distance <<= num_direct_bits;
                            let mut m: u32 = 1;
                            distance += 1;
                            loop {
                                // REV_BIT_VAR
                                if bit!(SPEC_POS + distance as usize) {
                                    m += m;
                                    distance += m;
                                } else {
                                    distance += m;
                                    m += m;
                                }
                                num_direct_bits -= 1;
                                if num_direct_bits == 0 {
                                    break;
                                }
                            }
                            distance -= m;
                        } else {
                            num_direct_bits -= NUM_ALIGN_BITS;
                            loop {
                                normalize!();
                                range >>= 1;
                                code = code.wrapping_sub(range);
                                let t = 0u32.wrapping_sub(code >> 31);
                                distance = (distance << 1).wrapping_add(t.wrapping_add(1));
                                code = code.wrapping_add(range & t);
                                num_direct_bits -= 1;
                                if num_direct_bits == 0 {
                                    break;
                                }
                            }
                            distance <<= NUM_ALIGN_BITS;
                            let mut i: u32 = 1;
                            // REV_BIT_CONST with 1, 2, 4, then REV_BIT_LAST
                            // with 8.
                            i += if bit!(ALIGN + i as usize) { 2 } else { 1 };
                            i += if bit!(ALIGN + i as usize) { 4 } else { 2 };
                            i += if bit!(ALIGN + i as usize) { 8 } else { 4 };
                            if !bit!(ALIGN + i as usize) {
                                i -= 8;
                            }
                            distance |= i;
                            if distance == 0xFFFF_FFFF {
                                len = MATCH_SPEC_LEN_START;
                                state -= NUM_STATES;
                                break 'main;
                            }
                        }
                    }

                    rep3 = rep2;
                    rep2 = rep1;
                    rep1 = rep0;
                    rep0 = distance + 1;
                    state = if state < NUM_STATES + NUM_LIT_STATES {
                        NUM_LIT_STATES
                    } else {
                        NUM_LIT_STATES + 3
                    };
                    let reach = if check_dic_size == 0 {
                        processed_pos
                    } else {
                        check_dic_size
                    };
                    if distance >= reach {
                        len += MATCH_SPEC_LEN_ERROR_DATA + MATCH_MIN_LEN;
                        break 'main;
                    }
                }

                len += MATCH_MIN_LEN;
                let rem = limit.saturating_sub(out.len());
                if rem == 0 {
                    // The caller looked ahead for an end marker and found
                    // a match: it is copied by the next call, if any.
                    break 'main;
                }
                let cur_len = rem.min(len as usize);
                let cur = u32::try_from(cur_len).map_err(|_| Fault::Fail)?;
                processed_pos = processed_pos.wrapping_add(cur);
                len -= cur;
                copy_back(out, rep0, cur_len)?;
            }
            if !(out.len() < limit && buf < buf_limit) {
                break;
            }
        }
        normalize!();

        self.range = range;
        self.code = code;
        self.remain_len = len;
        self.processed_pos = processed_pos;
        self.reps = [rep0, rep1, rep2, rep3];
        self.state = state;
        Ok((len >= MATCH_SPEC_LEN_ERROR_DATA, buf))
    }

    /// `LzmaDec_TryDummy`: whether the next symbol, and the normalisation
    /// after it, can be decoded from `input`, changing nothing. `None` is
    /// `DUMMY_INPUT_EOF`; otherwise the symbol's kind and the input it
    /// takes.
    #[allow(clippy::too_many_lines)]
    fn try_dummy(&self, out: &[u8], input: &[u8]) -> Result<Option<(Dummy, usize)>, Fault> {
        let mut range = self.range;
        let mut code = self.code;
        let mut buf = 0usize;
        let probs = &self.probs[..];
        let state = self.state as usize;
        let pos_state = ((self.processed_pos & ((1u32 << self.prop.pb) - 1)) << 4) as usize;

        macro_rules! normalize_check {
            () => {
                if range < TOP_VALUE {
                    let Some(&b) = input.get(buf) else {
                        return Ok(None);
                    };
                    range <<= 8;
                    code = (code << 8) | u32::from(b);
                    buf += 1;
                }
            };
        }
        // IF_BIT_0_CHECK with UPDATE_0_CHECK / UPDATE_1_CHECK.
        macro_rules! bit_check {
            ($i:expr) => {{
                let ttt = u32::from(*probs.get($i).ok_or(Fault::Fail)?);
                normalize_check!();
                let bound = (range >> NUM_BIT_MODEL_TOTAL_BITS) * ttt;
                if code < bound {
                    range = bound;
                    false
                } else {
                    range -= bound;
                    code -= bound;
                    true
                }
            }};
        }

        let res;
        'symbol: {
            if !bit_check!(IS_MATCH + pos_state + state) {
                let mut prob = LITERAL;
                if self.check_dic_size != 0 || self.processed_pos != 0 {
                    let prev = u32::from(*out.last().ok_or(Fault::Fail)?);
                    let lp_bits = self.processed_pos & ((1u32 << self.prop.lp) - 1);
                    prob += LIT_SIZE
                        * (((lp_bits << self.prop.lc) + (prev >> (8 - self.prop.lc))) as usize);
                }
                let mut symbol: u32 = 1;
                if (state as u32) < NUM_LIT_STATES {
                    while symbol < 0x100 {
                        let b = bit_check!(prob + symbol as usize);
                        symbol = (symbol << 1) | u32::from(b);
                    }
                } else {
                    let mut match_byte = u32::from(back(out, self.reps[0])?);
                    let mut offs: u32 = 0x100;
                    while symbol < 0x100 {
                        match_byte <<= 1;
                        let bit = offs;
                        offs &= match_byte;
                        if bit_check!(prob + (offs + bit + symbol) as usize) {
                            symbol = (symbol << 1) + 1;
                        } else {
                            symbol <<= 1;
                            offs ^= bit;
                        }
                    }
                }
                res = Dummy::Lit;
                break 'symbol;
            }

            let after;
            let len_coder;
            if bit_check!(IS_REP + state) {
                res = Dummy::Rep;
                if bit_check!(IS_REP_G0 + state) {
                    if bit_check!(IS_REP_G1 + state) {
                        bit_check!(IS_REP_G2 + state);
                    }
                } else if !bit_check!(IS_REP0_LONG + pos_state + state) {
                    break 'symbol;
                }
                after = NUM_STATES;
                len_coder = REP_LEN_CODER;
            } else {
                after = 0;
                len_coder = LEN_CODER;
                res = Dummy::Match;
            }

            let (base, offset, limit);
            if bit_check!(len_coder + LEN_CHOICE) {
                if bit_check!(len_coder + LEN_CHOICE2) {
                    base = len_coder + LEN_HIGH;
                    offset = LEN_NUM_LOW_SYMBOLS * 2;
                    limit = LEN_NUM_HIGH_SYMBOLS;
                } else {
                    base = len_coder + LEN_LOW + pos_state + (1 << LEN_NUM_LOW_BITS);
                    offset = LEN_NUM_LOW_SYMBOLS;
                    limit = 1 << LEN_NUM_LOW_BITS;
                }
            } else {
                base = len_coder + LEN_LOW + pos_state;
                offset = 0;
                limit = 1 << LEN_NUM_LOW_BITS;
            }
            let mut len: u32 = 1;
            while len < limit {
                let b = bit_check!(base + len as usize);
                len = (len << 1) | u32::from(b);
            }
            len = len - limit + offset;

            if after < 4 {
                let base =
                    POS_SLOT + ((len.min(NUM_LEN_TO_POS_STATES - 1) as usize) << NUM_POS_SLOT_BITS);
                let mut pos_slot: u32 = 1;
                while pos_slot < 1 << NUM_POS_SLOT_BITS {
                    let b = bit_check!(base + pos_slot as usize);
                    pos_slot = (pos_slot << 1) | u32::from(b);
                }
                pos_slot -= 1 << NUM_POS_SLOT_BITS;
                if pos_slot >= START_POS_MODEL_INDEX {
                    let mut num_direct_bits = (pos_slot >> 1) - 1;
                    let prob;
                    if pos_slot < END_POS_MODEL_INDEX {
                        prob = SPEC_POS + ((2 | (pos_slot & 1)) << num_direct_bits) as usize;
                    } else {
                        num_direct_bits -= NUM_ALIGN_BITS;
                        loop {
                            normalize_check!();
                            range >>= 1;
                            let keep = (code.wrapping_sub(range) >> 31).wrapping_sub(1);
                            code = code.wrapping_sub(range & keep);
                            num_direct_bits -= 1;
                            if num_direct_bits == 0 {
                                break;
                            }
                        }
                        prob = ALIGN;
                        num_direct_bits = NUM_ALIGN_BITS;
                    }
                    let mut i: u32 = 1;
                    let mut m: u32 = 1;
                    loop {
                        // REV_BIT_CHECK
                        if bit_check!(prob + i as usize) {
                            m += m;
                            i += m;
                        } else {
                            i += m;
                            m += m;
                        }
                        num_direct_bits -= 1;
                        if num_direct_bits == 0 {
                            break;
                        }
                    }
                }
            }
        }
        // The final NORMALIZE_CHECK: only whether its byte is there counts.
        if range < TOP_VALUE {
            if buf >= input.len() {
                return Ok(None);
            }
            buf += 1;
        }
        Ok(Some((res, buf)))
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )]

    use super::*;

    /// A range encoder writing the symbols `LzmaDec` reads, with lc, lp and
    /// pb of 0: to build streams no real encoder writes. It normalises after
    /// each bit and flushes five bytes, as 7-Zip's `LzmaEnc` does.
    struct Enc {
        low: u64,
        range: u32,
        cache: u8,
        cache_size: u64,
        out: Vec<u8>,
        probs: Vec<u16>,
        state: u32,
    }

    impl Enc {
        fn new() -> Self {
            Self {
                low: 0,
                range: 0xFFFF_FFFF,
                cache: 0,
                cache_size: 1,
                out: Vec::new(),
                probs: vec![1024; NUM_BASE_PROBS + LIT_SIZE],
                state: 0,
            }
        }

        fn shift_low(&mut self) {
            if self.low < 0xFF00_0000 || self.low >= 1 << 32 {
                let carry = (self.low >> 32) as u8;
                let mut temp = self.cache;
                loop {
                    self.out.push(temp.wrapping_add(carry));
                    temp = 0xFF;
                    self.cache_size -= 1;
                    if self.cache_size == 0 {
                        break;
                    }
                }
                self.cache = (self.low >> 24) as u8;
            }
            self.cache_size += 1;
            self.low = (self.low & 0x00FF_FFFF) << 8;
        }

        fn bit(&mut self, i: usize, b: bool) {
            let p = u32::from(self.probs[i]);
            let bound = (self.range >> NUM_BIT_MODEL_TOTAL_BITS) * p;
            if b {
                self.low += u64::from(bound);
                self.range -= bound;
                self.probs[i] = prob16(p - (p >> NUM_MOVE_BITS));
            } else {
                self.range = bound;
                self.probs[i] = prob16(p + ((BIT_MODEL_TOTAL - p) >> NUM_MOVE_BITS));
            }
            while self.range < TOP_VALUE {
                self.range <<= 8;
                self.shift_low();
            }
        }

        /// `value`'s low `bits` bits through the bit tree at `base`.
        fn tree(&mut self, base: usize, bits: u32, value: u32) {
            let mut m = 1usize;
            for i in (0..bits).rev() {
                let b = (value >> i) & 1 == 1;
                self.bit(base + m, b);
                m = (m << 1) | usize::from(b);
            }
        }

        /// A literal, after literals only.
        fn literal(&mut self, byte: u8) {
            assert!(self.state < NUM_LIT_STATES);
            self.bit(IS_MATCH + self.state as usize, false);
            self.tree(LITERAL, 8, u32::from(byte));
            self.state -= if self.state < 4 { self.state } else { 3 };
        }

        /// A new match of `len` 2 to 9 at a `distance` below 4, which is
        /// its own distance slot.
        fn short_match(&mut self, distance: u32, len: u32) {
            self.bit(IS_MATCH + self.state as usize, true);
            self.bit(IS_REP + self.state as usize, false);
            self.bit(LEN_CODER + LEN_CHOICE, false);
            self.tree(LEN_CODER + LEN_LOW, 3, len - 2);
            let len_state = (len - 2).min(NUM_LEN_TO_POS_STATES - 1) as usize;
            self.tree(POS_SLOT + (len_state << NUM_POS_SLOT_BITS), 6, distance);
            self.state = if self.state < NUM_LIT_STATES { 7 } else { 10 };
        }

        /// `value`'s low `bits` bits as direct bits, high bit first.
        fn direct(&mut self, value: u32, bits: u32) {
            for i in (0..bits).rev() {
                self.range >>= 1;
                if (value >> i) & 1 == 1 {
                    self.low += u64::from(self.range);
                }
                while self.range < TOP_VALUE {
                    self.range <<= 8;
                    self.shift_low();
                }
            }
        }

        /// A new match of length 2 at a `distance` of 128 or more, whose
        /// low bits are direct and aligned.
        fn far_match(&mut self, distance: u32) {
            assert!(distance >= 128);
            self.bit(IS_MATCH + self.state as usize, true);
            self.bit(IS_REP + self.state as usize, false);
            self.bit(LEN_CODER + LEN_CHOICE, false);
            self.tree(LEN_CODER + LEN_LOW, 3, 0);
            let n = distance.ilog2();
            let slot = 2 * n + ((distance >> (n - 1)) & 1);
            self.tree(POS_SLOT, 6, slot);
            let num_direct_bits = (slot >> 1) - 1;
            let low = distance - ((2 | (slot & 1)) << num_direct_bits);
            self.direct(low >> NUM_ALIGN_BITS, num_direct_bits - NUM_ALIGN_BITS);
            let mut m = 1usize;
            for i in 0..NUM_ALIGN_BITS {
                let b = (low >> i) & 1 == 1;
                self.bit(ALIGN + m, b);
                m = (m << 1) | usize::from(b);
            }
            self.state = if self.state < NUM_LIT_STATES { 7 } else { 10 };
        }

        fn finish(mut self) -> Vec<u8> {
            for _ in 0..5 {
                self.shift_low();
            }
            self.out
        }
    }

    const PLAIN: Props = Props {
        lc: 0,
        lp: 0,
        pb: 0,
        dic_size: 1 << 16,
    };

    /// "abc", then a match of `len` at `distance`.
    fn abc_then(distance: u32, len: u32) -> Vec<u8> {
        let mut e = Enc::new();
        for &b in b"abc" {
            e.literal(b);
        }
        e.short_match(distance, len);
        e.finish()
    }

    #[test]
    fn the_hand_made_streams_are_what_they_say() {
        let input = abc_then(2, 2);
        let (out, step) = decode(PLAIN, &input, 5, Finish::End);
        assert_eq!(out, b"abcab");
        assert_eq!(
            step,
            Step::ok(input.len(), Status::MaybeFinishedWithoutMark)
        );
    }

    /// A distance is checked against the bytes decoded so far: the nearest
    /// one too far is damage (not "cannot happen"), and nothing of the match
    /// is written.
    #[test]
    fn a_match_may_not_reach_before_the_first_byte() {
        let input = abc_then(3, 2);
        let (out, step) = decode(PLAIN, &input, 5, Finish::End);
        assert_eq!(out, b"abc");
        assert_eq!(step.fault, Some(Fault::Data));
        assert_eq!(step.status, Status::NotSpecified);
    }

    /// A match running past the limit is copied up to it, as 7-Zip does
    /// (liblzma refuses it whole); with FINISH_END that is damage, and with
    /// FINISH_ANY the rest comes out at the next call.
    #[test]
    fn a_match_running_past_the_limit_is_copied_to_it() {
        let input = abc_then(2, 9);
        let (out, step) = decode(PLAIN, &input, 5, Finish::End);
        assert_eq!(out, b"abcab");
        assert_eq!(step.fault, Some(Fault::Data));
        assert_eq!(step.status, Status::NotFinished);

        let mut dec = LzmaDec::new(PLAIN);
        let mut out = Vec::new();
        let first = dec.decode_to_dic(&mut out, 5, &input, Finish::Any);
        assert_eq!(first.fault, None);
        assert_eq!(first.status, Status::NotFinished);
        assert_eq!(out, b"abcab");
        let rest = dec.decode_to_dic(&mut out, 12, &input[first.consumed..], Finish::End);
        assert_eq!(out, b"abcabcabcabc");
        assert_eq!(
            rest,
            Step::ok(
                input.len() - first.consumed,
                Status::MaybeFinishedWithoutMark
            )
        );
    }

    /// Once the dictionary's size has been decoded, a distance may reach
    /// back that far and no farther -- though more has been decoded. The
    /// call that reaches the size stops there so that the rule starts
    /// exactly at it, even when one call decodes past it.
    #[test]
    fn a_distance_past_the_dictionary_is_damage_once_it_is_full() {
        let props = Props {
            dic_size: DIC_MIN,
            ..PLAIN
        };
        let n = DIC_MIN as usize + 4;
        let literals: Vec<u8> = (0..n).map(|i| (i % 251) as u8).collect();
        for (distance, sound) in [(DIC_MIN - 1, true), (DIC_MIN, false)] {
            let mut e = Enc::new();
            for &b in &literals {
                e.literal(b);
            }
            e.far_match(distance);
            let input = e.finish();
            let (out, step) = decode(props, &input, n + 2, Finish::End);
            assert_eq!(out[..n], literals[..], "{distance}");
            if sound {
                assert_eq!(out.len(), n + 2, "{distance}");
                assert_eq!(step.status, Status::MaybeFinishedWithoutMark);
            } else {
                assert_eq!(out.len(), n, "{distance}");
                assert_eq!(step.fault, Some(Fault::Data), "{distance}");
            }
        }
    }

    /// After the end marker the range coder's `code` must be zero; a stream
    /// whose last byte is wrong still reaches its marker, and is damaged.
    #[test]
    fn the_end_marker_must_finish_the_range_coder() {
        let data = sample();
        let (props, input) = stream(&data);
        let last = input.len() - 1;
        let mut reached = 0;
        for v in 0..=255u8 {
            if v == input[last] {
                continue;
            }
            let mut bad = input.clone();
            bad[last] = v;
            let (out, step) = decode(props, &bad, data.len(), Finish::End);
            if out == data {
                // The data and the marker decoded as they were: only the
                // finished range coder can be wrong.
                reached += 1;
                assert_eq!(step.fault, Some(Fault::Data), "last byte {v:02x}");
            }
        }
        assert!(reached > 0, "no last byte left the marker in place");
    }

    /// An LZMA stream (`.lzma` without its header) of `data`, by liblzma
    /// through the `xz` crate.
    fn stream(data: &[u8]) -> (Props, Vec<u8>) {
        let lzma = xz::compress_lzma(data, &xz::LzmaOptions::preset(xz::Preset::DEFAULT)).unwrap();
        let props = Props::decode(&lzma[..5]).unwrap();
        (props, lzma[13..].to_vec())
    }

    /// Decodes `input` whole to `size` bytes, as one call.
    fn decode(props: Props, input: &[u8], size: usize, finish: Finish) -> (Vec<u8>, Step) {
        let mut dec = LzmaDec::new(props);
        let mut out = Vec::new();
        let step = dec.decode_to_dic(&mut out, size, input, finish);
        (out, step)
    }

    fn sample() -> Vec<u8> {
        let mut v = Vec::new();
        for i in 0..20_000u32 {
            v.extend_from_slice(b"the quick brown fox ");
            v.push((i.wrapping_mul(2_654_435_761) >> 24) as u8);
        }
        v
    }

    #[test]
    fn a_sound_stream_decodes_and_ends_with_its_marker() {
        let data = sample();
        let (props, input) = stream(&data);
        // `.lzma` from `xz` ends with an end marker after the data.
        let (out, step) = decode(props, &input, data.len(), Finish::End);
        assert_eq!(out, data);
        assert_eq!(step, Step::ok(input.len(), Status::FinishedWithMark));
    }

    #[test]
    fn any_split_of_the_input_decodes_the_same() {
        let data = sample();
        let (props, input) = stream(&data);
        for piece in [1usize, 2, 3, 7, 19, 20, 21, 64, 1000] {
            let mut dec = LzmaDec::new(props);
            let mut out = Vec::new();
            let mut used = 0;
            let mut last = None;
            while used < input.len() {
                let end = (used + piece).min(input.len());
                let step = dec.decode_to_dic(&mut out, data.len(), &input[used..end], Finish::End);
                assert_eq!(step.fault, None, "piece {piece}");
                used += step.consumed;
                last = Some(step.status);
                if step.status == Status::FinishedWithMark {
                    break;
                }
                assert_eq!(step.consumed, end - (used - step.consumed), "piece {piece}");
            }
            assert_eq!(out, data, "piece {piece}");
            assert_eq!(used, input.len(), "piece {piece}");
            assert_eq!(last, Some(Status::FinishedWithMark), "piece {piece}");
        }
    }

    #[test]
    fn any_split_of_the_output_decodes_the_same() {
        let data = sample();
        let (props, input) = stream(&data);
        for step_size in [1usize, 2, 5, 273, 4096] {
            let mut dec = LzmaDec::new(props);
            let mut out = Vec::new();
            let mut used = 0;
            loop {
                let limit = (out.len() + step_size).min(data.len());
                let finish = if limit == data.len() {
                    Finish::End
                } else {
                    Finish::Any
                };
                let step = dec.decode_to_dic(&mut out, limit, &input[used..], finish);
                assert_eq!(step.fault, None, "step {step_size}");
                used += step.consumed;
                if step.status == Status::FinishedWithMark {
                    break;
                }
                assert_eq!(step.status, Status::NotFinished, "step {step_size}");
            }
            assert_eq!(out, data, "step {step_size}");
            assert_eq!(used, input.len());
        }
    }

    #[test]
    fn the_first_symbol_may_not_be_a_repeated_match() {
        let data = sample();
        let (props, mut input) = stream(&data);
        // The four code bytes at kBadRepCode and above.
        input[1..5].copy_from_slice(&0xC000_0000u32.to_be_bytes());
        let (out, step) = decode(props, &input, data.len(), Finish::End);
        assert!(out.is_empty());
        assert_eq!(step.fault, Some(Fault::Data));
        input[1..5].copy_from_slice(&(BAD_REP_CODE - 1).to_be_bytes());
        let (_, step) = decode(props, &input, data.len(), Finish::End);
        assert_ne!(step.consumed, 5, "decoding went on past the start");
    }

    #[test]
    fn the_first_byte_must_be_zero() {
        let data = sample();
        let (props, mut input) = stream(&data);
        input[0] = 1;
        let (out, step) = decode(props, &input, data.len(), Finish::End);
        assert!(out.is_empty());
        assert_eq!(step, Step::fault(5, Status::NotSpecified, Fault::Data));
    }

    #[test]
    fn short_input_asks_for_more_and_keeps_what_it_decoded() {
        let data = sample();
        let (props, input) = stream(&data);
        let cut = input.len() / 2;
        let (out, step) = decode(props, &input[..cut], data.len(), Finish::End);
        assert_eq!(step, Step::ok(cut, Status::NeedsMoreInput));
        assert!(!out.is_empty() && out.len() < data.len());
        assert_eq!(out[..], data[..out.len()]);
    }

    #[test]
    fn a_stream_cut_at_its_size_is_maybe_finished_and_the_rest_is_unread() {
        let data = sample();
        let (props, input) = stream(&data);
        // Stopping short of the end with FINISH_ANY: not finished.
        let (out, step) = decode(props, &input, 1000, Finish::Any);
        assert_eq!(out[..], data[..1000]);
        assert_eq!(step.status, Status::NotFinished);
        // With FINISH_END there, the next symbol is no end marker: damage.
        let (out, step) = decode(props, &input, 1000, Finish::End);
        assert_eq!(out[..], data[..1000]);
        assert_eq!(step.fault, Some(Fault::Data));
        assert_eq!(step.status, Status::NotFinished);
    }
}
