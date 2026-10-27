//! Macroblocks coded with context-adaptive variable-length codes
//! (`h264_cavlc.c`): all of Baseline and Extended profile, and Main and
//! High streams that do not use CABAC.
//!
//! A macroblock's type, its prediction -- intra modes, or reference
//! indices and vector differences -- and its coded block pattern are
//! Exp-Golomb codes. Each 4x4 block's coefficients are then a run of
//! codes: how many coefficients there are and how many end in ones (a
//! table picked by how many the blocks above and to the left had), the
//! levels, how many zeros lie among them, and the run of zeros before each.
//! An 8x8 transform block is coded as four interleaved 4x4 runs.
//!
//! The bits are read unchecked here, as FFmpeg reads them in this file
//! (`UNCHECKED_BITSTREAM_READER`): a damaged slice may read past its end
//! into the padding before its macroblock is refused.

#![allow(
    clippy::arithmetic_side_effects,
    reason = "FFmpeg's int arithmetic on levels, vectors and QPs, wrapping where it wraps"
)]
#![allow(
    clippy::indexing_slicing,
    reason = "cache indices are scan8's; table indices are codes the tables were built for, \
              counts the code tables bound, and block indices under 48"
)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    reason = "FFmpeg's int, unsigned, int8 and int16 conversions, made where it makes them"
)]

use std::sync::LazyLock;

use crate::decoder::{Context, DecodeError, SliceContext};
use crate::mbtype::{
    B_MB_TYPE_INFO, B_SUB_MB_TYPE_INFO, CHROMA_DC_BLOCK_INDEX, CHROMA_DC_SCAN, CHROMA422_DC_SCAN,
    GOLOMB_TO_INTER_CBP, GOLOMB_TO_INTER_CBP_GRAY, GOLOMB_TO_INTRA4X4_CBP,
    GOLOMB_TO_INTRA4X4_CBP_GRAY, I_MB_TYPE_INFO, LUMA_DC_BLOCK_INDEX, MB_TYPE_8X8DCT, MB_TYPE_16X8,
    MB_TYPE_16X16, MB_TYPE_INTERLACED, P_MB_TYPE_INFO, P_SUB_MB_TYPE_INFO, SCAN8, is_8x8dct,
    is_16x8, is_16x16, is_dir, is_direct, is_inter, is_interlaced, is_intra, is_intra_pcm,
    is_intra4x4, is_intra16x16, is_ref0, is_sub_4x8, is_sub_8x4, is_sub_8x8,
};
use crate::mvpred::{LIST_NOT_USED, PART_NOT_AVAILABLE, fill, tab};
use crate::picture::table_offset;
use crate::pred::{DC_128_PRED8X8, check_intra_pred_mode, check_intra4x4_pred_mode};
use crate::scan::{Scan, ScanTables};
use crate::slice::{TYPE_B, TYPE_I, TYPE_P, TYPE_SI};
use ffbits::{Bits, Vlc, VlcError, bits::av_log2};

/// How many bits each table is looked up by (`*_VLC_BITS`).
const LEVEL_TAB_BITS: u32 = 8;
const CHROMA_DC_COEFF_TOKEN_VLC_BITS: u32 = 8;
const CHROMA422_DC_COEFF_TOKEN_VLC_BITS: u32 = 13;
const COEFF_TOKEN_VLC_BITS: u32 = 8;
const TOTAL_ZEROS_VLC_BITS: u32 = 9;
const CHROMA_DC_TOTAL_ZEROS_VLC_BITS: u32 = 3;
const CHROMA422_DC_TOTAL_ZEROS_VLC_BITS: u32 = 5;
const RUN_VLC_BITS: u32 = 3;
const RUN7_VLC_BITS: u32 = 6;

/// Which of the four coefficient-token tables a predicted count reads
/// (`coeff_token_table_index`).
const COEFF_TOKEN_TABLE_INDEX: [usize; 17] = [0, 0, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 3, 3, 3, 3, 3];

/// A macroblock's samples by chroma format, I_PCM's size at 1 bit a sample
/// (`ff_h264_mb_sizes`).
const MB_SIZES: [i32; 4] = [256, 384, 512, 768];

/// The code tables, built once (`ff_h264_decode_init_vlc`).
struct Tables {
    chroma_dc_coeff_token: Vlc,
    chroma422_dc_coeff_token: Vlc,
    /// By predicted count range: 0-1, 2-3, 4-7, 8 and up.
    coeff_token: Vec<Vlc>,
    /// By coefficient count, from 1.
    total_zeros: Vec<Vlc>,
    chroma_dc_total_zeros: Vec<Vlc>,
    chroma422_dc_total_zeros: Vec<Vlc>,
    /// By zeros left, from 1 to 6; `run7` for 7 and more.
    run: Vec<Vlc>,
    run7: Vlc,
    /// A level's value and length for each next 8 bits, by suffix length;
    /// 100 plus the prefix where the code is longer
    /// (`cavlc_level_tab`).
    level: [[[i8; 2]; 1 << LEVEL_TAB_BITS]; 7],
}

/// FFmpeg's tables as `Vlc::new` takes them: lengths and codes, the
/// symbol of each code its index.
fn vlc(bits: u32, lens: &[u8], codes: &[u8]) -> Result<Vlc, VlcError> {
    let codes: Vec<u32> = codes.iter().map(|&c| u32::from(c)).collect();
    Vlc::new(bits, lens, &codes, None)
}

impl Tables {
    fn new() -> Result<Self, VlcError> {
        let rows = |bits, lens: &[[u8; 16]], codes: &[[u8; 16]], n: usize, count: usize| {
            (0..n)
                .map(|i| vlc(bits, &lens[i][..count], &codes[i][..count]))
                .collect::<Result<Vec<_>, _>>()
        };
        Ok(Self {
            chroma_dc_coeff_token: vlc(
                CHROMA_DC_COEFF_TOKEN_VLC_BITS,
                &CHROMA_DC_COEFF_TOKEN_LEN,
                &CHROMA_DC_COEFF_TOKEN_BITS,
            )?,
            chroma422_dc_coeff_token: vlc(
                CHROMA422_DC_COEFF_TOKEN_VLC_BITS,
                &CHROMA422_DC_COEFF_TOKEN_LEN,
                &CHROMA422_DC_COEFF_TOKEN_BITS,
            )?,
            coeff_token: (0..4)
                .map(|i| {
                    vlc(
                        COEFF_TOKEN_VLC_BITS,
                        &COEFF_TOKEN_LEN[i],
                        &COEFF_TOKEN_BITS[i],
                    )
                })
                .collect::<Result<Vec<_>, _>>()?,
            total_zeros: rows(
                TOTAL_ZEROS_VLC_BITS,
                &TOTAL_ZEROS_LEN,
                &TOTAL_ZEROS_BITS,
                15,
                16,
            )?,
            chroma_dc_total_zeros: (0..3)
                .map(|i| {
                    vlc(
                        CHROMA_DC_TOTAL_ZEROS_VLC_BITS,
                        &CHROMA_DC_TOTAL_ZEROS_LEN[i],
                        &CHROMA_DC_TOTAL_ZEROS_BITS[i],
                    )
                })
                .collect::<Result<Vec<_>, _>>()?,
            chroma422_dc_total_zeros: (0..7)
                .map(|i| {
                    vlc(
                        CHROMA422_DC_TOTAL_ZEROS_VLC_BITS,
                        &CHROMA422_DC_TOTAL_ZEROS_LEN[i],
                        &CHROMA422_DC_TOTAL_ZEROS_BITS[i],
                    )
                })
                .collect::<Result<Vec<_>, _>>()?,
            run: rows(RUN_VLC_BITS, &RUN_LEN, &RUN_BITS, 6, 7)?,
            run7: vlc(RUN7_VLC_BITS, &RUN_LEN[6], &RUN_BITS[6])?,
            level: level_table(),
        })
    }
}

/// `init_cavlc_level_tab`: for each suffix length and each next 8 bits, a
/// level whose prefix and suffix fit in them, or the prefix to finish
/// reading by hand.
fn level_table() -> [[[i8; 2]; 1 << LEVEL_TAB_BITS]; 7] {
    let mut tab = [[[0i8; 2]; 1 << LEVEL_TAB_BITS]; 7];
    let bits = LEVEL_TAB_BITS as i32;
    for (suffix_length, row) in tab.iter_mut().enumerate() {
        let suffix_length = suffix_length as i32;
        for (i, entry) in row.iter_mut().enumerate() {
            let i = i as u32;
            let prefix = bits - av_log2(2 * i);
            *entry = if prefix + 1 + suffix_length <= bits {
                let level_code = (prefix << suffix_length)
                    + (i >> (av_log2(i) - suffix_length)) as i32
                    - (1 << suffix_length);
                let mask = -(level_code & 1);
                let level = (((2 + level_code) >> 1) ^ mask) - mask;
                [level as i8, (prefix + 1 + suffix_length) as i8]
            } else if prefix < bits {
                [(prefix + 100) as i8, (prefix + 1) as i8]
            } else {
                [(bits + 100) as i8, bits as i8]
            };
        }
    }
    tab
}

static TABLES: LazyLock<Result<Tables, VlcError>> = LazyLock::new(Tables::new);

/// The predicted coefficient count of block `n`: the mean of the left and
/// top blocks', rounded up, or the one of them that is there -- an absent
/// one counts 64 (`pred_non_zero_count`).
fn pred_non_zero_count(nnz: &[u8; 120], n: usize) -> usize {
    let index8 = SCAN8[n];
    let left = u32::from(nnz[index8 - 1]);
    let top = u32::from(nnz[index8 - 8]);
    let mut i = left + top;
    if i < 64 {
        i = (i + 1) >> 1;
    }
    (i & 31) as usize
}

/// One block's coefficients (`decode_residual`): at most `max_coeff` of
/// them, dequantized by `qmul` -- the factors from the block's row on, as
/// [`crate::ps::Pps::dequant4_from`] gives them -- or, for a DC block (`n`
/// from `LUMA_DC_BLOCK_INDEX` on), kept as they are; and stored at the
/// scan's positions in `block`, which runs on past the block to the
/// macroblock's end, as `int16` unless `wide`. The count goes into `nnz`
/// for the blocks after to predict from.
#[allow(
    clippy::too_many_arguments,
    reason = "FFmpeg's signature, the context split up"
)]
#[allow(
    clippy::too_many_lines,
    reason = "one function in FFmpeg, kept whole to read against it"
)]
fn decode_residual(
    t: &Tables,
    gb: &mut Bits<'_>,
    nnz: &mut [u8; 120],
    block: &mut [i32],
    n: usize,
    scan: Scan<'_>,
    qmul: Option<&[u32]>,
    max_coeff: usize,
    wide: bool,
) -> Result<(), DecodeError> {
    let coeff_token = if max_coeff <= 8 {
        if max_coeff == 4 {
            t.chroma_dc_coeff_token.read(gb, 1)
        } else {
            t.chroma422_dc_coeff_token.read(gb, 1)
        }
    } else {
        let predicted = pred_non_zero_count(
            nnz,
            if n >= LUMA_DC_BLOCK_INDEX {
                (n - LUMA_DC_BLOCK_INDEX) * 16
            } else {
                n
            },
        );
        t.coeff_token[COEFF_TOKEN_TABLE_INDEX[predicted]].read(gb, 2)
    };
    let total_coeff = coeff_token >> 2;
    // An invalid code reads -1, stored as FFmpeg's uint8 stores it.
    nnz[SCAN8[n]] = total_coeff as u8;
    if total_coeff == 0 {
        return Ok(());
    }
    if total_coeff as u32 > max_coeff as u32 {
        return Err(DecodeError);
    }
    let total_coeff = total_coeff as usize;
    let trailing_ones = (coeff_token & 3) as usize;

    let mut level = [0i32; 16];
    let i = gb.show(3) as i32;
    gb.skip(trailing_ones as u32);
    level[0] = 1 - ((i & 4) >> 1);
    level[1] = 1 - (i & 2);
    level[2] = 1 - ((i & 1) << 1);

    if trailing_ones < total_coeff {
        let mut suffix_length = usize::from(total_coeff > 10 && trailing_ones < 3);
        let bitsi = gb.show(LEVEL_TAB_BITS) as usize;
        let mut level_code = i32::from(t.level[suffix_length][bitsi][0]);
        gb.skip(t.level[suffix_length][bitsi][1] as u32);
        if level_code >= 100 {
            let mut prefix = level_code - 100;
            if prefix == LEVEL_TAB_BITS as i32 {
                prefix += gb.level_prefix();
            }
            // The first coefficient's suffix length is 0 or 1.
            match prefix.cmp(&14) {
                std::cmp::Ordering::Less => {
                    level_code = if suffix_length != 0 {
                        (prefix << 1) + gb.bit() as i32
                    } else {
                        prefix
                    };
                }
                std::cmp::Ordering::Equal => {
                    level_code = if suffix_length != 0 {
                        (prefix << 1) + gb.bit() as i32
                    } else {
                        prefix + gb.bits(4) as i32
                    };
                }
                std::cmp::Ordering::Greater => {
                    level_code = 30;
                    if prefix >= 16 {
                        if prefix > 25 + 3 {
                            return Err(DecodeError);
                        }
                        level_code += (1 << (prefix - 3)) - 4096;
                    }
                    level_code += gb.bits((prefix - 3) as u32) as i32;
                }
            }
            if trailing_ones < 3 {
                level_code += 2;
            }
            suffix_length = 2;
            let mask = -(level_code & 1);
            level[trailing_ones] = (((2 + level_code) >> 1) ^ mask) - mask;
        } else {
            level_code += ((level_code >> 31) | 1) & -i32::from(trailing_ones < 3);
            suffix_length = 1 + usize::from((level_code as u32).wrapping_add(3) > 6);
            level[trailing_ones] = level_code;
        }

        // The rest have a suffix length of at least 1.
        const SUFFIX_LIMIT: [u32; 7] = [0, 3, 6, 12, 24, 48, i32::MAX as u32];
        for slot in &mut level[trailing_ones + 1..total_coeff] {
            let bitsi = gb.show(LEVEL_TAB_BITS) as usize;
            level_code = i32::from(t.level[suffix_length][bitsi][0]);
            gb.skip(t.level[suffix_length][bitsi][1] as u32);
            if level_code >= 100 {
                let mut prefix = level_code - 100;
                if prefix == LEVEL_TAB_BITS as i32 {
                    prefix += gb.level_prefix();
                }
                if prefix < 15 {
                    level_code = (prefix << suffix_length) + gb.bits(suffix_length as u32) as i32;
                } else {
                    level_code = 15 << suffix_length;
                    if prefix >= 16 {
                        if prefix > 25 + 3 {
                            return Err(DecodeError);
                        }
                        level_code += (1 << (prefix - 3)) - 4096;
                    }
                    level_code += gb.bits((prefix - 3) as u32) as i32;
                }
                let mask = -(level_code & 1);
                level_code = (((2 + level_code) >> 1) ^ mask) - mask;
            }
            *slot = level_code;
            let limit = SUFFIX_LIMIT[suffix_length];
            suffix_length += usize::from(limit.wrapping_add(level_code as u32) > 2 * limit);
        }
    }

    let mut zeros_left = if total_coeff == max_coeff {
        0
    } else if max_coeff <= 8 {
        if max_coeff == 4 {
            t.chroma_dc_total_zeros[total_coeff - 1].read(gb, 1)
        } else {
            t.chroma422_dc_total_zeros[total_coeff - 1].read(gb, 1)
        }
    } else {
        t.total_zeros[total_coeff - 1].read(gb, 1)
    };

    // STORE_BLOCK: from the last coefficient back, each a run of zeros
    // before the one after it. Zeros run out before the scan's start only
    // in a damaged stream, and the positions read there can lie past the
    // block's 16 -- in the macroblock's blocks after it, dequantized by the
    // QP rows after `qmul`'s (both slices run on), as FFmpeg stores them.
    // Past a luma DC block's 16 (`mb_luma_dc`), FFmpeg's stores land where
    // nothing reads them -- the row's unused half, or the next plane's row,
    // cleared before it is used -- and they are dropped here.
    let dc = n >= LUMA_DC_BLOCK_INDEX;
    let mut put = |at: isize, level: i32| -> Result<(), DecodeError> {
        let pos = scan.get(at)?;
        let v = if dc {
            level
        } else {
            let q = qmul.and_then(|q| q.get(pos)).copied().ok_or(DecodeError)?;
            ((level as u32).wrapping_mul(q).wrapping_add(32) as i32) >> 6
        };
        if let Some(slot) = block.get_mut(pos) {
            *slot = if wide { v } else { i32::from(v as i16) };
        } else if !dc {
            return Err(DecodeError);
        }
        Ok(())
    };
    let mut at = (zeros_left + total_coeff as i32 - 1) as isize;
    put(at, level[0])?;
    let mut i = 1;
    while i < total_coeff && zeros_left > 0 {
        let run_before = if zeros_left < 7 {
            t.run[zeros_left as usize - 1].read(gb, 1)
        } else {
            t.run7.read(gb, 2)
        };
        zeros_left -= run_before;
        at -= 1 + run_before as isize;
        put(at, level[i])?;
        i += 1;
    }
    while i < total_coeff {
        at -= 1;
        put(at, level[i])?;
        i += 1;
    }

    if zeros_left < 0 {
        return Err(DecodeError);
    }
    Ok(())
}

/// A reference index of a partition (`ref_count` choices): none to read
/// for one, a flipped bit for two, else an Exp-Golomb code under the count.
fn read_ref(gb: &mut Bits<'_>, ref_count: u32) -> Result<u32, DecodeError> {
    match ref_count {
        1 => Ok(0),
        2 => Ok(gb.bit() ^ 1),
        _ => {
            let v = gb.ue_31();
            if v >= ref_count {
                Err(DecodeError)
            } else {
                Ok(v)
            }
        }
    }
}

impl Context {
    /// A plane's luma (or 4:4:4 chroma) residual (`decode_luma_residual`):
    /// for an Intra16x16 macroblock the DC block then each AC block; else
    /// each 8x8 the coded block pattern names, as four 4x4 blocks or one
    /// 8x8. What answers is the 8x8 pattern the counts give, which CAVLC
    /// 4:4:4 keeps for the loop filter -- with FFmpeg's quirk of or-ing
    /// each 4x4 block's count, not whether it is zero.
    #[allow(clippy::too_many_arguments, reason = "FFmpeg's signature")]
    fn decode_luma_residual(
        &self,
        t: &Tables,
        sl: &mut SliceContext,
        gb: &mut Bits<'_>,
        scan: usize,
        scan8x8: usize,
        mb_type: u32,
        cbp: i32,
        p: usize,
    ) -> Result<i32, DecodeError> {
        let pps = self.ps.pps.as_deref().ok_or(DecodeError)?;
        let wide = self.pixel_shift != 0;
        let qscale = if p == 0 {
            sl.hdr.qscale
        } else {
            sl.hdr.chroma_qp[p - 1]
        } as usize;
        let tables = &self.scans.all[..];
        let at = |start: usize| Scan { tables, start };

        if is_intra16x16(mb_type) {
            sl.mb_luma_dc[p] = [0; 16];
            decode_residual(
                t,
                gb,
                &mut sl.non_zero_count_cache,
                &mut sl.mb_luma_dc[p],
                LUMA_DC_BLOCK_INDEX + p,
                at(scan),
                None,
                16,
                wide,
            )?;
            if cbp & 15 == 0 {
                fill(&mut sl.non_zero_count_cache, SCAN8[p * 16], 4, 4, 0);
                return Ok(0);
            }
            let qmul = pps.dequant4_from(p, qscale).ok_or(DecodeError)?;
            for i8x8 in 0..4 {
                for i4x4 in 0..4 {
                    let index = i4x4 + 4 * i8x8 + p * 16;
                    decode_residual(
                        t,
                        gb,
                        &mut sl.non_zero_count_cache,
                        &mut sl.mb[16 * index..],
                        index,
                        at(scan + 1),
                        Some(qmul),
                        15,
                        wide,
                    )?;
                }
            }
            return Ok(0xF);
        }

        let cqm = if is_intra(mb_type) { 0 } else { 3 } + p;
        let mut new_cbp = 0;
        for i8x8 in 0..4 {
            let corner = SCAN8[4 * i8x8 + p * 16];
            if cbp & (1 << i8x8) == 0 {
                for k in [0, 1, 8, 9] {
                    sl.non_zero_count_cache[corner + k] = 0;
                }
                continue;
            }
            if is_8x8dct(mb_type) {
                let qmul = pps.dequant8(cqm, qscale).ok_or(DecodeError)?;
                let buf = 64 * i8x8 + 256 * p;
                for i4x4 in 0..4 {
                    let index = i4x4 + 4 * i8x8 + p * 16;
                    decode_residual(
                        t,
                        gb,
                        &mut sl.non_zero_count_cache,
                        &mut sl.mb[buf..],
                        index,
                        at(scan8x8 + 16 * i4x4),
                        Some(qmul),
                        16,
                        wide,
                    )?;
                }
                let nnz = &mut sl.non_zero_count_cache;
                nnz[corner] = nnz[corner]
                    .wrapping_add(nnz[corner + 1])
                    .wrapping_add(nnz[corner + 8])
                    .wrapping_add(nnz[corner + 9]);
                new_cbp |= i32::from(nnz[corner] != 0) << i8x8;
            } else {
                let qmul = pps.dequant4_from(cqm, qscale).ok_or(DecodeError)?;
                for i4x4 in 0..4 {
                    let index = i4x4 + 4 * i8x8 + p * 16;
                    decode_residual(
                        t,
                        gb,
                        &mut sl.non_zero_count_cache,
                        &mut sl.mb[16 * index..],
                        index,
                        at(scan),
                        Some(qmul),
                        16,
                        wide,
                    )?;
                    new_cbp |= i32::from(sl.non_zero_count_cache[SCAN8[index]]) << i8x8;
                }
            }
        }
        Ok(new_cbp)
    }

    /// One macroblock of a CAVLC slice (`ff_h264_decode_mb_cavlc`): a run of
    /// skipped ones first where the slice predicts, then its type, its
    /// prediction, its coded block pattern and its residual -- left in the
    /// slice context for reconstruction, and its type, QP, motion and
    /// counts in the picture's tables.
    #[allow(
        clippy::too_many_lines,
        reason = "one function in FFmpeg, kept whole to read against it"
    )]
    pub(crate) fn decode_mb_cavlc(
        &mut self,
        sl: &mut SliceContext,
        gb: &mut Bits<'_>,
    ) -> Result<(), DecodeError> {
        let t = TABLES.as_ref().map_err(|_| DecodeError)?;
        let pps = self.ps.pps.clone().ok_or(DecodeError)?;
        let sps = self.ps.sps.clone().ok_or(DecodeError)?;
        let mut dct8x8_allowed = pps.transform_8x8_mode != 0;
        let decode_chroma = sps.chroma_format_idc == 1 || sps.chroma_format_idc == 2;

        let mb_xy = sl.mb_x + sl.mb_y * self.mb_stride;
        sl.mb_xy = mb_xy;
        let mut cbp: i32 = 0;

        if sl.hdr.slice_type_nos != TYPE_I {
            if sl.mb_skip_run == -1 {
                let run = gb.ue_long();
                if run > self.mb_num as u32 {
                    return Err(DecodeError);
                }
                sl.mb_skip_run = run as i32;
            }
            let skipped = sl.mb_skip_run != 0;
            sl.mb_skip_run -= 1;
            if skipped {
                if self.mb_aff_frame && sl.mb_y & 1 == 0 && sl.mb_skip_run == 0 {
                    let field = gb.bit() as i32;
                    sl.hdr.mb_mbaff = field;
                    sl.hdr.mb_field_decoding_flag = field;
                }
                self.decode_mb_skip(sl, false);
                return Ok(());
            }
        }
        if self.mb_aff_frame && sl.mb_y & 1 == 0 {
            let field = gb.bit() as i32;
            sl.hdr.mb_mbaff = field;
            sl.hdr.mb_field_decoding_flag = field;
        }

        sl.prev_mb_skipped = 0;

        // An invalid code reads negative, as unsigned past every table.
        let mut code = gb.ue() as u32;
        let mut intra = true;
        let mut partition_count = 0;
        let mut mb_type = 0;
        if sl.hdr.slice_type_nos == TYPE_B {
            if code < 23 {
                let info = B_MB_TYPE_INFO[code as usize];
                (partition_count, mb_type, intra) = (info.partition_count, info.kind, false);
            } else {
                code -= 23;
            }
        } else if sl.hdr.slice_type_nos == TYPE_P {
            if code < 5 {
                let info = P_MB_TYPE_INFO[code as usize];
                (partition_count, mb_type, intra) = (info.partition_count, info.kind, false);
            } else {
                code -= 5;
            }
        } else if sl.hdr.slice_type == TYPE_SI && code != 0 {
            code -= 1;
        }
        if intra {
            let info = I_MB_TYPE_INFO.get(code as usize).ok_or(DecodeError)?;
            cbp = i32::from(info.cbp);
            sl.intra16x16_pred_mode = i32::from(info.pred_mode);
            mb_type = info.kind;
        }

        if sl.hdr.mb_field_decoding_flag != 0 {
            mb_type |= MB_TYPE_INTERLACED;
        }
        self.slice_table[tab(table_offset(self.mb_stride), mb_xy)] = sl.slice_num as u16;

        if is_intra_pcm(mb_type) {
            let chroma = usize::try_from(sps.chroma_format_idc).map_err(|_| DecodeError)?;
            let mb_size = MB_SIZES.get(chroma).ok_or(DecodeError)? * sps.bit_depth_luma;
            sl.intra_pcm_pos = gb.align();
            if gb.left() < mb_size {
                return Err(DecodeError);
            }
            gb.skip_long(mb_size);
            let cur = self.cur_pic_ptr.ok_or(DecodeError)?;
            let at = tab(table_offset(self.pics[cur].mb_stride), mb_xy);
            // The loop filter takes an I_PCM macroblock's QP as 0, and all
            // its coefficients as present.
            self.pics[cur].qscale_table[at] = 0;
            self.non_zero_count[mb_xy as usize] = [16; 48];
            self.pics[cur].mb_type[at] = mb_type;
            return Ok(());
        }

        self.fill_decode_neighbors(sl, mb_type);
        self.fill_decode_caches(sl, mb_type, false);

        let list_count = sl.hdr.list_count as usize;
        let mbaff = sl.hdr.mb_mbaff as u32;
        if is_intra(mb_type) {
            if is_intra4x4(mb_type) {
                let mut di = 1;
                if dct8x8_allowed && gb.bit() != 0 {
                    mb_type |= MB_TYPE_8X8DCT;
                    di = 4;
                }
                for i in (0..16).step_by(di) {
                    let mut mode = Self::pred_intra_mode(sl, i);
                    if gb.bit() == 0 {
                        let rem_mode = gb.bits(3) as i32;
                        mode = rem_mode + i32::from(rem_mode >= mode);
                    }
                    if di == 4 {
                        fill(&mut sl.intra4x4_pred_mode_cache, SCAN8[i], 2, 2, mode as i8);
                    } else {
                        sl.intra4x4_pred_mode_cache[SCAN8[i]] = mode as i8;
                    }
                }
                // The modes as coded are what the macroblocks after read,
                // not as the check below may change them.
                self.write_back_intra_pred_mode(sl);
                check_intra4x4_pred_mode(
                    &mut sl.intra4x4_pred_mode_cache,
                    sl.top_samples_available,
                    sl.left_samples_available,
                )?;
            } else {
                sl.intra16x16_pred_mode = check_intra_pred_mode(
                    sl.top_samples_available,
                    sl.left_samples_available,
                    sl.intra16x16_pred_mode as u32,
                    false,
                )?;
            }
            sl.chroma_pred_mode = if decode_chroma {
                let mode = gb.ue_31();
                check_intra_pred_mode(
                    sl.top_samples_available,
                    sl.left_samples_available,
                    mode,
                    true,
                )?
            } else {
                DC_128_PRED8X8
            };
        } else if partition_count == 4 {
            let mut sub_partition_count = [0usize; 4];
            let mut refs = [[0i32; 4]; 2];
            // The code is kept before it is checked, as FFmpeg keeps it: a
            // macroblock refused for it leaves it behind.
            if sl.hdr.slice_type_nos == TYPE_B {
                for i in 0..4 {
                    let code = gb.ue_31();
                    sl.sub_mb_type[i] = code as u16;
                    let info = B_SUB_MB_TYPE_INFO.get(code as usize).ok_or(DecodeError)?;
                    sub_partition_count[i] = usize::from(info.partition_count);
                    sl.sub_mb_type[i] = info.kind as u16;
                }
                let any = sl.sub_mb_type.iter().fold(0, |a, &s| a | s);
                if is_direct(u32::from(any)) {
                    self.pred_direct_motion(sl, &mut mb_type);
                    for list in 0..2 {
                        sl.ref_cache[list][SCAN8[4]] = PART_NOT_AVAILABLE;
                        sl.ref_cache[list][SCAN8[12]] = PART_NOT_AVAILABLE;
                    }
                }
            } else {
                for i in 0..4 {
                    let code = gb.ue_31();
                    sl.sub_mb_type[i] = code as u16;
                    let info = P_SUB_MB_TYPE_INFO.get(code as usize).ok_or(DecodeError)?;
                    sub_partition_count[i] = usize::from(info.partition_count);
                    sl.sub_mb_type[i] = info.kind as u16;
                }
            }

            for list in 0..list_count {
                let ref_count = if is_ref0(mb_type) {
                    1
                } else {
                    sl.hdr.ref_count[list] << mbaff
                };
                for i in 0..4 {
                    let sub = u32::from(sl.sub_mb_type[i]);
                    if is_direct(sub) {
                        continue;
                    }
                    refs[list][i] = if is_dir(sub, 0, list as u32) {
                        read_ref(gb, ref_count)? as i32
                    } else {
                        -1
                    };
                }
            }

            if dct8x8_allowed {
                dct8x8_allowed = self.get_dct8x8_allowed(sl);
            }

            for list in 0..list_count {
                for i in 0..4 {
                    let sub_mb_type = u32::from(sl.sub_mb_type[i]);
                    let corner = SCAN8[4 * i];
                    if is_direct(sub_mb_type) {
                        sl.ref_cache[list][corner] = sl.ref_cache[list][corner + 1];
                        continue;
                    }
                    let r = refs[list][i] as i8;
                    for k in [0, 1, 8, 9] {
                        sl.ref_cache[list][corner + k] = r;
                    }
                    if !is_dir(sub_mb_type, 0, list as u32) {
                        for k in [0, 1, 8, 9] {
                            sl.mv_cache[list][corner + k] = [0, 0];
                        }
                        continue;
                    }
                    let block_width = if sub_mb_type & (MB_TYPE_16X16 | MB_TYPE_16X8) != 0 {
                        2
                    } else {
                        1
                    };
                    for j in 0..sub_partition_count[i] {
                        let index = 4 * i + block_width * j;
                        let at = SCAN8[index];
                        let (mx, my) = self.pred_motion(
                            sl,
                            index,
                            block_width,
                            list,
                            i32::from(sl.ref_cache[list][at]),
                        );
                        let mx = mx.wrapping_add(gb.se());
                        let my = my.wrapping_add(gb.se());
                        let mv = [mx as i16, my as i16];
                        let cache = &mut sl.mv_cache[list];
                        if is_sub_8x8(sub_mb_type) {
                            cache[at + 1] = mv;
                            cache[at + 8] = mv;
                            cache[at + 9] = mv;
                        } else if is_sub_8x4(sub_mb_type) {
                            cache[at + 1] = mv;
                        } else if is_sub_4x8(sub_mb_type) {
                            cache[at + 8] = mv;
                        }
                        cache[at] = mv;
                    }
                }
            }
        } else if is_direct(mb_type) {
            self.pred_direct_motion(sl, &mut mb_type);
            dct8x8_allowed &= sps.direct_8x8_inference_flag != 0;
        } else if is_16x16(mb_type) {
            for list in 0..list_count {
                if is_dir(mb_type, 0, list as u32) {
                    let val = read_ref(gb, sl.hdr.ref_count[list] << mbaff)?;
                    fill(&mut sl.ref_cache[list], SCAN8[0], 4, 4, val as i8);
                }
            }
            for list in 0..list_count {
                if is_dir(mb_type, 0, list as u32) {
                    let r = i32::from(sl.ref_cache[list][SCAN8[0]]);
                    let (mx, my) = self.pred_motion(sl, 0, 4, list, r);
                    let mv = [
                        mx.wrapping_add(gb.se()) as i16,
                        my.wrapping_add(gb.se()) as i16,
                    ];
                    fill(&mut sl.mv_cache[list], SCAN8[0], 4, 4, mv);
                }
            }
        } else if is_16x8(mb_type) {
            for list in 0..list_count {
                for i in 0..2 {
                    let val = if is_dir(mb_type, i as u32, list as u32) {
                        read_ref(gb, sl.hdr.ref_count[list] << mbaff)? as i8
                    } else {
                        LIST_NOT_USED
                    };
                    fill(&mut sl.ref_cache[list], SCAN8[0] + 16 * i, 4, 2, val);
                }
            }
            for list in 0..list_count {
                for i in 0..2 {
                    let mv = if is_dir(mb_type, i as u32, list as u32) {
                        let r = i32::from(sl.ref_cache[list][SCAN8[0] + 16 * i]);
                        let (mx, my) = self.pred_16x8_motion(sl, 8 * i, list, r);
                        [
                            mx.wrapping_add(gb.se()) as i16,
                            my.wrapping_add(gb.se()) as i16,
                        ]
                    } else {
                        [0, 0]
                    };
                    fill(&mut sl.mv_cache[list], SCAN8[0] + 16 * i, 4, 2, mv);
                }
            }
        } else {
            // 8x16.
            for list in 0..list_count {
                for i in 0..2 {
                    let val = if is_dir(mb_type, i as u32, list as u32) {
                        read_ref(gb, sl.hdr.ref_count[list] << mbaff)? as i8
                    } else {
                        LIST_NOT_USED
                    };
                    fill(&mut sl.ref_cache[list], SCAN8[0] + 2 * i, 2, 4, val);
                }
            }
            for list in 0..list_count {
                for i in 0..2 {
                    let mv = if is_dir(mb_type, i as u32, list as u32) {
                        let r = i32::from(sl.ref_cache[list][SCAN8[0] + 2 * i]);
                        let (mx, my) = self.pred_8x16_motion(sl, i * 4, list, r);
                        [
                            mx.wrapping_add(gb.se()) as i16,
                            my.wrapping_add(gb.se()) as i16,
                        ]
                    } else {
                        [0, 0]
                    };
                    fill(&mut sl.mv_cache[list], SCAN8[0] + 2 * i, 2, 4, mv);
                }
            }
        }

        if is_inter(mb_type) {
            self.write_back_motion(sl, mb_type, false);
        }

        if !is_intra16x16(mb_type) {
            let code = gb.ue() as u32;
            let table: &[u8] = match (decode_chroma, is_intra4x4(mb_type)) {
                (true, true) => &GOLOMB_TO_INTRA4X4_CBP,
                (true, false) => &GOLOMB_TO_INTER_CBP,
                (false, true) => &GOLOMB_TO_INTRA4X4_CBP_GRAY,
                (false, false) => &GOLOMB_TO_INTER_CBP_GRAY,
            };
            cbp = i32::from(*table.get(code as usize).ok_or(DecodeError)?);
        } else if !decode_chroma && cbp > 15 {
            // Chroma coefficients in a stream with no chroma.
            return Err(DecodeError);
        }

        if dct8x8_allowed && cbp & 15 != 0 && !is_intra(mb_type) && gb.bit() != 0 {
            mb_type |= MB_TYPE_8X8DCT;
        }
        sl.cbp = cbp;
        self.cbp_table[mb_xy as usize] = cbp as u16;
        let cur = self.cur_pic_ptr.ok_or(DecodeError)?;
        let pic_at = tab(table_offset(self.pics[cur].mb_stride), mb_xy);
        self.pics[cur].mb_type[pic_at] = mb_type;

        let nnz =
            |sl: &mut SliceContext, n: usize| fill(&mut sl.non_zero_count_cache, SCAN8[n], 4, 4, 0);
        if cbp != 0 || is_intra16x16(mb_type) {
            let max_qp = 51 + 6 * (sps.bit_depth_luma - 8);
            let dquant = gb.se();
            sl.hdr.qscale = (sl.hdr.qscale as u32).wrapping_add(dquant as u32) as i32;
            if sl.hdr.qscale as u32 > max_qp as u32 {
                if sl.hdr.qscale < 0 {
                    sl.hdr.qscale += max_qp + 1;
                } else {
                    sl.hdr.qscale -= max_qp + 1;
                }
                if sl.hdr.qscale as u32 > max_qp as u32 {
                    sl.hdr.qscale = max_qp;
                    return Err(DecodeError);
                }
            }
            let qp = sl.hdr.qscale as usize;
            sl.hdr.chroma_qp = [
                i32::from(pps.chroma_qp_table[0][qp]),
                i32::from(pps.chroma_qp_table[1][qp]),
            ];

            let (scan, scan8x8) = ScanTables::cavlc(is_interlaced(mb_type), sl.hdr.qscale == 0);
            let luma = self.decode_luma_residual(t, sl, gb, scan, scan8x8, mb_type, cbp, 0)?;
            self.cbp_table[mb_xy as usize] |= (luma << 12) as u16;
            if sps.chroma_format_idc == 3 {
                self.decode_luma_residual(t, sl, gb, scan, scan8x8, mb_type, cbp, 1)?;
                self.decode_luma_residual(t, sl, gb, scan, scan8x8, mb_type, cbp, 2)?;
            } else {
                let num_c8x8 = sps.chroma_format_idc as usize;
                let wide = self.pixel_shift != 0;
                if cbp & 0x30 != 0 {
                    // Only a 4:2:2 DC block can run its zeros out before its
                    // scan's start. FFmpeg then reads whatever its linker put
                    // before `ff_h264_chroma422_dc_scan`, which cannot be
                    // followed, and the block is a decode error here.
                    let dc_scan: &[u8] = if sps.chroma_format_idc == 2 {
                        &CHROMA422_DC_SCAN
                    } else {
                        &CHROMA_DC_SCAN
                    };
                    for chroma_idx in 0..2 {
                        decode_residual(
                            t,
                            gb,
                            &mut sl.non_zero_count_cache,
                            &mut sl.mb[256 + 256 * chroma_idx..],
                            CHROMA_DC_BLOCK_INDEX + chroma_idx,
                            Scan {
                                tables: dc_scan,
                                start: 0,
                            },
                            None,
                            4 * num_c8x8,
                            wide,
                        )?;
                    }
                }
                if cbp & 0x20 != 0 {
                    for chroma_idx in 0..2 {
                        let list = chroma_idx + 1 + if is_intra(mb_type) { 0 } else { 3 };
                        let qmul = pps
                            .dequant4_from(list, sl.hdr.chroma_qp[chroma_idx] as usize)
                            .ok_or(DecodeError)?;
                        let mut block = 16 * (16 + 16 * chroma_idx);
                        for i8x8 in 0..num_c8x8 {
                            for i4x4 in 0..4 {
                                let index = 16 + 16 * chroma_idx + 8 * i8x8 + i4x4;
                                decode_residual(
                                    t,
                                    gb,
                                    &mut sl.non_zero_count_cache,
                                    &mut sl.mb[block..],
                                    index,
                                    Scan {
                                        tables: &self.scans.all,
                                        start: scan + 1,
                                    },
                                    Some(qmul),
                                    15,
                                    wide,
                                )?;
                                block += 16;
                            }
                        }
                    }
                } else {
                    nnz(sl, 16);
                    nnz(sl, 32);
                }
            }
        } else {
            nnz(sl, 0);
            nnz(sl, 16);
            nnz(sl, 32);
        }
        self.pics[cur].qscale_table[pic_at] = sl.hdr.qscale as i8;
        self.write_back_non_zero_count(sl);
        Ok(())
    }
}

// The code tables, as h264_cavlc.c has them, rows zero-filled as C fills
// them; generated from FFmpeg 8.1.3's source.
#[rustfmt::skip]
const CHROMA_DC_COEFF_TOKEN_LEN: [u8; 20] = [2, 0, 0, 0, 6, 1, 0, 0, 6, 6, 3, 0, 6, 7, 7, 6, 6, 8, 8, 7];
#[rustfmt::skip]
const CHROMA_DC_COEFF_TOKEN_BITS: [u8; 20] = [1, 0, 0, 0, 7, 1, 0, 0, 4, 6, 1, 0, 3, 3, 2, 5, 2, 3, 2, 0];
#[rustfmt::skip]
const CHROMA422_DC_COEFF_TOKEN_LEN: [u8; 36] = [1, 0, 0, 0, 7, 2, 0, 0, 7, 7, 3, 0, 9, 7, 7, 5, 9, 9, 7, 6, 10, 10, 9, 7, 11, 11, 10, 7, 12, 12, 11, 10, 13, 12, 12, 11];
#[rustfmt::skip]
const CHROMA422_DC_COEFF_TOKEN_BITS: [u8; 36] = [1, 0, 0, 0, 15, 1, 0, 0, 14, 13, 1, 0, 7, 12, 11, 1, 6, 5, 10, 1, 7, 6, 4, 9, 7, 6, 5, 8, 7, 6, 5, 4, 7, 5, 4, 4];
#[rustfmt::skip]
const COEFF_TOKEN_LEN: [[u8; 68]; 4] = [
    [1, 0, 0, 0, 6, 2, 0, 0, 8, 6, 3, 0, 9, 8, 7, 5, 10, 9, 8, 6, 11, 10, 9, 7, 13, 11, 10, 8, 13, 13, 11, 9, 13, 13, 13, 10, 14, 14, 13, 11, 14, 14, 14, 13, 15, 15, 14, 14, 15, 15, 15, 14, 16, 15, 15, 15, 16, 16, 16, 15, 16, 16, 16, 16, 16, 16, 16, 16],
    [2, 0, 0, 0, 6, 2, 0, 0, 6, 5, 3, 0, 7, 6, 6, 4, 8, 6, 6, 4, 8, 7, 7, 5, 9, 8, 8, 6, 11, 9, 9, 6, 11, 11, 11, 7, 12, 11, 11, 9, 12, 12, 12, 11, 12, 12, 12, 11, 13, 13, 13, 12, 13, 13, 13, 13, 13, 14, 13, 13, 14, 14, 14, 13, 14, 14, 14, 14],
    [4, 0, 0, 0, 6, 4, 0, 0, 6, 5, 4, 0, 6, 5, 5, 4, 7, 5, 5, 4, 7, 5, 5, 4, 7, 6, 6, 4, 7, 6, 6, 4, 8, 7, 7, 5, 8, 8, 7, 6, 9, 8, 8, 7, 9, 9, 8, 8, 9, 9, 9, 8, 10, 9, 9, 9, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10],
    [6, 0, 0, 0, 6, 6, 0, 0, 6, 6, 6, 0, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6],
];
#[rustfmt::skip]
const COEFF_TOKEN_BITS: [[u8; 68]; 4] = [
    [1, 0, 0, 0, 5, 1, 0, 0, 7, 4, 1, 0, 7, 6, 5, 3, 7, 6, 5, 3, 7, 6, 5, 4, 15, 6, 5, 4, 11, 14, 5, 4, 8, 10, 13, 4, 15, 14, 9, 4, 11, 10, 13, 12, 15, 14, 9, 12, 11, 10, 13, 8, 15, 1, 9, 12, 11, 14, 13, 8, 7, 10, 9, 12, 4, 6, 5, 8],
    [3, 0, 0, 0, 11, 2, 0, 0, 7, 7, 3, 0, 7, 10, 9, 5, 7, 6, 5, 4, 4, 6, 5, 6, 7, 6, 5, 8, 15, 6, 5, 4, 11, 14, 13, 4, 15, 10, 9, 4, 11, 14, 13, 12, 8, 10, 9, 8, 15, 14, 13, 12, 11, 10, 9, 12, 7, 11, 6, 8, 9, 8, 10, 1, 7, 6, 5, 4],
    [15, 0, 0, 0, 15, 14, 0, 0, 11, 15, 13, 0, 8, 12, 14, 12, 15, 10, 11, 11, 11, 8, 9, 10, 9, 14, 13, 9, 8, 10, 9, 8, 15, 14, 13, 13, 11, 14, 10, 12, 15, 10, 13, 12, 11, 14, 9, 12, 8, 10, 13, 8, 13, 7, 9, 12, 9, 12, 11, 10, 5, 8, 7, 6, 1, 4, 3, 2],
    [3, 0, 0, 0, 0, 1, 0, 0, 4, 5, 6, 0, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63],
];
#[rustfmt::skip]
const TOTAL_ZEROS_LEN: [[u8; 16]; 16] = [
    [1, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 9],
    [3, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 6, 6, 6, 6, 0],
    [4, 3, 3, 3, 4, 4, 3, 3, 4, 5, 5, 6, 5, 6, 0, 0],
    [5, 3, 4, 4, 3, 3, 3, 4, 3, 4, 5, 5, 5, 0, 0, 0],
    [4, 4, 4, 3, 3, 3, 3, 3, 4, 5, 4, 5, 0, 0, 0, 0],
    [6, 5, 3, 3, 3, 3, 3, 3, 4, 3, 6, 0, 0, 0, 0, 0],
    [6, 5, 3, 3, 3, 2, 3, 4, 3, 6, 0, 0, 0, 0, 0, 0],
    [6, 4, 5, 3, 2, 2, 3, 3, 6, 0, 0, 0, 0, 0, 0, 0],
    [6, 6, 4, 2, 2, 3, 2, 5, 0, 0, 0, 0, 0, 0, 0, 0],
    [5, 5, 3, 2, 2, 2, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [4, 4, 3, 3, 1, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [4, 4, 2, 1, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [3, 3, 1, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [2, 2, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
];
#[rustfmt::skip]
const TOTAL_ZEROS_BITS: [[u8; 16]; 16] = [
    [1, 3, 2, 3, 2, 3, 2, 3, 2, 3, 2, 3, 2, 3, 2, 1],
    [7, 6, 5, 4, 3, 5, 4, 3, 2, 3, 2, 3, 2, 1, 0, 0],
    [5, 7, 6, 5, 4, 3, 4, 3, 2, 3, 2, 1, 1, 0, 0, 0],
    [3, 7, 5, 4, 6, 5, 4, 3, 3, 2, 2, 1, 0, 0, 0, 0],
    [5, 4, 3, 7, 6, 5, 4, 3, 2, 1, 1, 0, 0, 0, 0, 0],
    [1, 1, 7, 6, 5, 4, 3, 2, 1, 1, 0, 0, 0, 0, 0, 0],
    [1, 1, 5, 4, 3, 3, 2, 1, 1, 0, 0, 0, 0, 0, 0, 0],
    [1, 1, 1, 3, 3, 2, 2, 1, 0, 0, 0, 0, 0, 0, 0, 0],
    [1, 0, 1, 3, 2, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0],
    [1, 0, 1, 3, 2, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [0, 1, 1, 2, 1, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [0, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [0, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [0, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
];
#[rustfmt::skip]
const CHROMA_DC_TOTAL_ZEROS_LEN: [[u8; 4]; 3] = [
    [1, 2, 3, 3],
    [1, 2, 2, 0],
    [1, 1, 0, 0],
];
#[rustfmt::skip]
const CHROMA_DC_TOTAL_ZEROS_BITS: [[u8; 4]; 3] = [
    [1, 1, 1, 0],
    [1, 1, 0, 0],
    [1, 0, 0, 0],
];
#[rustfmt::skip]
const CHROMA422_DC_TOTAL_ZEROS_LEN: [[u8; 8]; 7] = [
    [1, 3, 3, 4, 4, 4, 5, 5],
    [3, 2, 3, 3, 3, 3, 3, 0],
    [3, 3, 2, 2, 3, 3, 0, 0],
    [3, 2, 2, 2, 3, 0, 0, 0],
    [2, 2, 2, 2, 0, 0, 0, 0],
    [2, 2, 1, 0, 0, 0, 0, 0],
    [1, 1, 0, 0, 0, 0, 0, 0],
];
#[rustfmt::skip]
const CHROMA422_DC_TOTAL_ZEROS_BITS: [[u8; 8]; 7] = [
    [1, 2, 3, 2, 3, 1, 1, 0],
    [0, 1, 1, 4, 5, 6, 7, 0],
    [0, 1, 1, 2, 6, 7, 0, 0],
    [6, 0, 1, 2, 7, 0, 0, 0],
    [0, 1, 2, 3, 0, 0, 0, 0],
    [0, 1, 1, 0, 0, 0, 0, 0],
    [0, 1, 0, 0, 0, 0, 0, 0],
];
#[rustfmt::skip]
const RUN_LEN: [[u8; 16]; 7] = [
    [1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [1, 2, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [2, 2, 2, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [2, 2, 2, 3, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [2, 2, 3, 3, 3, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [2, 3, 3, 3, 3, 3, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [3, 3, 3, 3, 3, 3, 3, 4, 5, 6, 7, 8, 9, 10, 11, 0],
];
#[rustfmt::skip]
const RUN_BITS: [[u8; 16]; 7] = [
    [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [3, 2, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [3, 2, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [3, 2, 3, 2, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [3, 0, 1, 3, 2, 5, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [7, 6, 5, 4, 3, 2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0],
];

#[cfg(test)]
mod tests;
