//! Macroblocks coded with CABAC (`h264_cabac.c`): Main and High profile
//! streams that set `entropy_coding_mode_flag`.
//!
//! Every syntax element is a string of bins read through the arithmetic
//! decoder (`cabac`), each bin with the context state its position and its
//! neighbours pick -- 1024 of them, set up at the slice's start from its QP
//! (`init_cabac_states`) -- or with no context at all (bypass bins, for
//! signs and the long tails of large values). The elements are FFmpeg's in
//! FFmpeg's order: a skip flag, the field flag of an MBAFF pair, the
//! macroblock type, its prediction (intra modes, or references and vector
//! differences), the coded block pattern, the 8x8-transform flag, the QP
//! change, and for each coded block a coded-block flag, a significance map
//! and the levels.
//!
//! What the macroblock layer around it does is shared with CAVLC
//! (`mvpred`, `direct`, `pred`): the caches are filled with CABAC's extra
//! state (vector differences, direct flags, neighbouring coded block
//! patterns), and the motion written back with its vector differences.

#![allow(
    clippy::arithmetic_side_effects,
    reason = "FFmpeg's int arithmetic on context indices, levels, vectors and QPs, wrapping \
              where it wraps"
)]
#![allow(
    clippy::indexing_slicing,
    reason = "context indices are FFmpeg's offsets into the 1024 states, cache indices scan8's, \
              table indices codes the tables were built for"
)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    reason = "FFmpeg's int, unsigned, int8 and int16 conversions, made where it makes them"
)]

use crate::cabac::{CONTEXT_INIT_I, CONTEXT_INIT_PB, Cabac, LAST_COEFF_FLAG_OFFSET_8X8, table};
use crate::decoder::{Context, DecodeError, SliceContext};
use crate::mbtype::{
    B_MB_TYPE_INFO, B_SUB_MB_TYPE_INFO, CHROMA_DC_BLOCK_INDEX, CHROMA_DC_SCAN, CHROMA422_DC_SCAN,
    I_MB_TYPE_INFO, LUMA_DC_BLOCK_INDEX, MB_TYPE_8X8DCT, MB_TYPE_16X8, MB_TYPE_16X16,
    MB_TYPE_DIRECT2, MB_TYPE_INTERLACED, MB_TYPE_INTRA_PCM, MB_TYPE_INTRA16X16, MB_TYPE_SKIP,
    P_MB_TYPE_INFO, P_SUB_MB_TYPE_INFO, SCAN8, is_8x8dct, is_16x8, is_16x16, is_dir, is_direct,
    is_inter, is_interlaced, is_intra, is_intra_pcm, is_intra4x4, is_intra16x16, is_skip,
    is_sub_4x8, is_sub_8x4, is_sub_8x8,
};
use crate::mvpred::{LIST_NOT_USED, PART_NOT_AVAILABLE, fill, tab};
use crate::picture::table_offset;
use crate::pred::{DC_128_PRED8X8, check_intra_pred_mode, check_intra4x4_pred_mode};
use crate::scan::{Scan, ScanTables};
use crate::slice::{TYPE_B, TYPE_I, TYPE_P, TYPE_SI};

/// A macroblock's samples by chroma format, I_PCM's size in samples
/// (`ff_h264_mb_sizes`).
const MB_SIZES: [i32; 4] = [256, 384, 512, 768];

/// The data a slice is read from: the buffer and where it starts in its
/// aligned allocation (`Bits::origin`).
#[derive(Clone, Copy)]
pub(crate) struct Data<'a> {
    pub(crate) buf: &'a [u8],
    pub(crate) origin: usize,
}

/// One block's coefficients as `decode_cabac_residual_internal` is asked
/// for them.
#[derive(Clone, Copy)]
struct Block<'a> {
    /// The block category, 0 to 13: picks every context.
    cat: usize,
    /// Its index: a 4x4 block's, or a DC block's from
    /// `LUMA_DC_BLOCK_INDEX` / `CHROMA_DC_BLOCK_INDEX` on.
    n: usize,
    scan: Scan<'a>,
    /// Dequantization factors; none for a DC block, kept as read.
    qmul: Option<&'a [u32]>,
    max_coeff: usize,
    is_dc: bool,
    chroma422: bool,
}

/// `significant_coeff_flag_offset`, by field and category.
const SIGNIFICANT_COEFF_FLAG_OFFSET: [[usize; 14]; 2] = [
    [
        105, 120, 134, 149, 152, 402, 484, 499, 513, 660, 528, 543, 557, 718,
    ],
    [
        277, 292, 306, 321, 324, 436, 776, 791, 805, 675, 820, 835, 849, 733,
    ],
];
/// `last_coeff_flag_offset`.
const LAST_COEFF_FLAG_OFFSET: [[usize; 14]; 2] = [
    [
        166, 181, 195, 210, 213, 417, 572, 587, 601, 690, 616, 631, 645, 748,
    ],
    [
        338, 353, 367, 382, 385, 451, 864, 879, 893, 699, 908, 923, 937, 757,
    ],
];
/// `coeff_abs_level_m1_offset`.
const COEFF_ABS_LEVEL_M1_OFFSET: [usize; 14] = [
    227, 237, 247, 257, 266, 426, 952, 962, 972, 708, 982, 992, 1002, 766,
];
/// `significant_coeff_flag_offset_8x8`, by field.
#[rustfmt::skip]
const SIGNIFICANT_COEFF_FLAG_OFFSET_8X8: [[u8; 63]; 2] = [
    [ 0, 1, 2, 3, 4, 5, 5, 4, 4, 3, 3, 4, 4, 4, 5, 5,
      4, 4, 4, 4, 3, 3, 6, 7, 7, 7, 8, 9,10, 9, 8, 7,
      7, 6,11,12,13,11, 6, 7, 8, 9,14,10, 9, 8, 6,11,
     12,13,11, 6, 9,14,10, 9,11,12,13,11,14,10,12 ],
    [ 0, 1, 1, 2, 2, 3, 3, 4, 5, 6, 7, 7, 7, 8, 4, 5,
      6, 9,10,10, 8,11,12,11, 9, 9,10,10, 8,11,12,11,
      9, 9,10,10, 8,11,12,11, 9, 9,10,10, 8,13,13, 9,
      9,10,10, 8,13,13, 9, 9,10,10,14,14,14,14,14 ],
];
/// `sig_coeff_offset_dc`: a 4:2:2 chroma DC block's contexts.
const SIG_COEFF_OFFSET_DC: [usize; 7] = [0, 0, 1, 1, 2, 2, 2];
/// `coeff_abs_level1_ctx`: the context of a level's first bin, by node.
const COEFF_ABS_LEVEL1_CTX: [usize; 8] = [1, 2, 3, 4, 0, 0, 0, 0];
/// `coeff_abs_levelgt1_ctx`: of its further bins, by node; the second row
/// for a 4:2:2 chroma DC block.
const COEFF_ABS_LEVELGT1_CTX: [[usize; 8]; 2] =
    [[5, 5, 5, 5, 6, 7, 8, 9], [5, 5, 5, 5, 6, 7, 8, 8]];
/// `coeff_abs_level_transition`: the next node after a level of one, and
/// after a larger one.
const COEFF_ABS_LEVEL_TRANSITION: [[usize; 8]; 2] =
    [[1, 2, 3, 3, 4, 5, 6, 7], [4, 4, 4, 4, 5, 6, 7, 7]];
/// `base_ctx` of `get_cabac_cbf_ctx`: each category's coded-block flags.
const CBF_BASE_CTX: [usize; 14] = [
    85, 89, 93, 97, 101, 1012, 460, 464, 468, 1016, 472, 476, 480, 1020,
];

/// The coded-block flag's context (`get_cabac_cbf_ctx`): whether the
/// blocks to the left and above had coefficients.
fn cbf_ctx(sl: &SliceContext, cat: usize, idx: usize, is_dc: bool) -> usize {
    let (nza, nzb) = if is_dc {
        if cat == 3 {
            let i = idx - CHROMA_DC_BLOCK_INDEX;
            ((sl.left_cbp >> (6 + i)) & 1, (sl.top_cbp >> (6 + i)) & 1)
        } else {
            let i = idx - LUMA_DC_BLOCK_INDEX;
            (sl.left_cbp & (0x100 << i), sl.top_cbp & (0x100 << i))
        }
    } else {
        (
            i32::from(sl.non_zero_count_cache[SCAN8[idx] - 1]),
            i32::from(sl.non_zero_count_cache[SCAN8[idx] - 8]),
        )
    };
    let mut ctx = 0;
    if nza > 0 {
        ctx += 1;
    }
    if nzb > 0 {
        ctx += 2;
    }
    CBF_BASE_CTX[cat] + ctx
}

/// A block's coefficients, once its coded-block flag has said it has some
/// (`decode_cabac_residual_internal`): the significance map, then the
/// levels from the last coefficient back, dequantized unless DC, stored at
/// the scan's positions in `block` -- as `int16` unless `wide`. Answers how
/// many there were.
#[allow(
    clippy::too_many_arguments,
    reason = "the parts of the slice context it writes, borrowed apart"
)]
fn residual(
    c: &mut Cabac,
    buf: &[u8],
    states: &mut [u8; 1024],
    nnz: &mut [u8; 120],
    block: &mut [i32],
    b: &Block<'_>,
    field: bool,
    wide: bool,
) -> Result<usize, DecodeError> {
    let f = usize::from(field);
    let sig_base = SIGNIFICANT_COEFF_FLAG_OFFSET[f][b.cat];
    let last_base = LAST_COEFF_FLAG_OFFSET[f][b.cat];
    let abs_base = COEFF_ABS_LEVEL_M1_OFFSET[b.cat];

    let mut index = [0usize; 64];
    let mut coeff_count = 0;
    // DECODE_SIGNIFICANCE: each position's flag, and after a significant
    // one whether it was the last; the final position needs no flag.
    let mut significance =
        |coefs: usize, sig_off: &dyn Fn(usize) -> usize, last_off: &dyn Fn(usize) -> usize| {
            let mut last = 0;
            while last < coefs {
                if c.get(buf, &mut states[sig_base + sig_off(last)]) != 0 {
                    index[coeff_count] = last;
                    coeff_count += 1;
                    if c.get(buf, &mut states[last_base + last_off(last)]) != 0 {
                        last = b.max_coeff;
                        break;
                    }
                }
                last += 1;
            }
            if last == b.max_coeff - 1 {
                index[coeff_count] = last;
                coeff_count += 1;
            }
        };
    if !b.is_dc && b.max_coeff == 64 {
        let offsets = &SIGNIFICANT_COEFF_FLAG_OFFSET_8X8[f];
        significance(63, &|last| usize::from(offsets[last]), &|last| {
            table(LAST_COEFF_FLAG_OFFSET_8X8 + last as i32) as usize
        });
    } else if b.is_dc && b.chroma422 {
        significance(7, &|last| SIG_COEFF_OFFSET_DC[last], &|last| {
            SIG_COEFF_OFFSET_DC[last]
        });
    } else {
        significance(b.max_coeff - 1, &|last| last, &|last| last);
    }

    if b.is_dc {
        nnz[SCAN8[b.n]] = coeff_count as u8;
    } else if b.max_coeff == 64 {
        fill(nnz, SCAN8[b.n], 2, 2, coeff_count as u8);
    } else {
        nnz[SCAN8[b.n]] = coeff_count as u8;
    }
    let count = coeff_count;

    // STORE_BLOCK: from the last coefficient back.
    let mut node_ctx = 0;
    while coeff_count > 0 {
        coeff_count -= 1;
        let j = b.scan.get(index[coeff_count] as isize)?;
        let value = if c.get(buf, &mut states[abs_base + COEFF_ABS_LEVEL1_CTX[node_ctx]]) == 0 {
            node_ctx = COEFF_ABS_LEVEL_TRANSITION[0][node_ctx];
            if b.is_dc {
                c.bypass_sign(buf, -1)
            } else {
                let q = b.qmul.and_then(|q| q.get(j)).copied().ok_or(DecodeError)?;
                c.bypass_sign(buf, (q as i32).wrapping_neg())
                    .wrapping_add(32)
                    >> 6
            }
        } else {
            let ctx =
                abs_base + COEFF_ABS_LEVELGT1_CTX[usize::from(b.is_dc && b.chroma422)][node_ctx];
            node_ctx = COEFF_ABS_LEVEL_TRANSITION[1][node_ctx];
            let mut coeff_abs: u32 = 2;
            while coeff_abs < 15 && c.get(buf, &mut states[ctx]) != 0 {
                coeff_abs += 1;
            }
            if coeff_abs >= 15 {
                let mut escape = 0;
                while c.bypass(buf) != 0 && escape < 16 + 7 {
                    escape += 1;
                }
                coeff_abs = 1;
                while escape > 0 {
                    escape -= 1;
                    coeff_abs = coeff_abs
                        .wrapping_add(coeff_abs)
                        .wrapping_add(c.bypass(buf) as u32);
                }
                coeff_abs = coeff_abs.wrapping_add(14);
            }
            let signed = c.bypass_sign(buf, (coeff_abs as i32).wrapping_neg());
            if b.is_dc {
                signed
            } else {
                let q = b.qmul.and_then(|q| q.get(j)).copied().ok_or(DecodeError)?;
                ((signed as u32).wrapping_mul(q).wrapping_add(32) as i32) >> 6
            }
        };
        let slot = block.get_mut(j).ok_or(DecodeError)?;
        *slot = if wide { value } else { i32::from(value as i16) };
    }
    Ok(count)
}

impl Context {
    /// The context states at the slice's start, from its QP and
    /// `cabac_init_idc` (`ff_h264_init_cabac_states`).
    pub(crate) fn init_cabac_states(&self, sl: &mut SliceContext) -> Result<(), DecodeError> {
        let sps = self.ps.sps.as_deref().ok_or(DecodeError)?;
        let slice_qp = (sl.hdr.qscale - 6 * (sps.bit_depth_luma - 8)).clamp(0, 51);
        let tab = if sl.hdr.slice_type_nos == TYPE_I {
            &CONTEXT_INIT_I
        } else {
            CONTEXT_INIT_PB
                .get(usize::try_from(sl.hdr.cabac_init_idc).map_err(|_| DecodeError)?)
                .ok_or(DecodeError)?
        };
        for (state, &[m, n]) in sl.cabac_state.iter_mut().zip(tab.iter()) {
            let mut pre = 2 * (((i32::from(m) * slice_qp) >> 4) + i32::from(n)) - 127;
            pre ^= pre >> 31;
            if pre > 124 {
                pre = 124 + (pre & 1);
            }
            *state = pre as u8;
        }
        Ok(())
    }

    /// The slice table's entry for `xy`: the slice that macroblock is in.
    fn slice_at(&self, xy: i32) -> i32 {
        i32::from(self.slice_table[tab(table_offset(self.mb_stride), xy)])
    }

    /// The field flag of an MBAFF pair (`decode_cabac_field_decoding_flag`).
    fn cabac_field_decoding_flag(&self, sl: &mut SliceContext, d: Data<'_>) -> i32 {
        let mbb_xy = sl.mb_xy - 2 * self.mb_stride;
        let mut ctx = (sl.hdr.mb_field_decoding_flag & i32::from(sl.mb_x != 0)) as usize;
        ctx += ((self.cur_type(mbb_xy) >> 7) & u32::from(self.slice_at(mbb_xy) == sl.slice_num))
            as usize;
        sl.cabac.get(d.buf, &mut sl.cabac_state[70 + ctx])
    }

    /// An intra macroblock's type, 0 (I_4x4) to 25 (I_PCM)
    /// (`decode_cabac_intra_mb_type`), its bins from `ctx_base` on.
    fn cabac_intra_mb_type(
        sl: &mut SliceContext,
        d: Data<'_>,
        ctx_base: usize,
        intra_slice: bool,
    ) -> usize {
        let mut state = ctx_base;
        if intra_slice {
            let mut ctx = 0;
            if sl.left_type[0] & (MB_TYPE_INTRA16X16 | MB_TYPE_INTRA_PCM) != 0 {
                ctx += 1;
            }
            if sl.top_type & (MB_TYPE_INTRA16X16 | MB_TYPE_INTRA_PCM) != 0 {
                ctx += 1;
            }
            if sl.cabac.get(d.buf, &mut sl.cabac_state[state + ctx]) == 0 {
                return 0;
            }
            state += 2;
        } else if sl.cabac.get(d.buf, &mut sl.cabac_state[state]) == 0 {
            return 0;
        }
        if sl.cabac.terminate(d.buf) != 0 {
            return 25;
        }
        let is = usize::from(intra_slice);
        let mut mb_type = 1;
        mb_type += 12 * sl.cabac.get(d.buf, &mut sl.cabac_state[state + 1]) as usize;
        if sl.cabac.get(d.buf, &mut sl.cabac_state[state + 2]) != 0 {
            mb_type += 4 + 4 * sl.cabac.get(d.buf, &mut sl.cabac_state[state + 2 + is]) as usize;
        }
        mb_type += 2 * sl.cabac.get(d.buf, &mut sl.cabac_state[state + 3 + is]) as usize;
        mb_type += sl.cabac.get(d.buf, &mut sl.cabac_state[state + 3 + 2 * is]) as usize;
        mb_type
    }

    /// The skip flag of macroblock (`mb_x`, `mb_y`) (`decode_cabac_mb_skip`).
    fn cabac_mb_skip(&self, sl: &mut SliceContext, d: Data<'_>, mb_x: i32, mb_y: i32) -> i32 {
        let stride = self.mb_stride;
        let (mba_xy, mbb_xy);
        if self.mb_aff_frame {
            let mb_xy = mb_x + (mb_y & !1) * stride;
            let mut a = mb_xy - 1;
            let field = sl.hdr.mb_field_decoding_flag != 0;
            if mb_y & 1 != 0
                && self.slice_at(a) == sl.slice_num
                && field == is_interlaced(self.cur_type(a))
            {
                a += stride;
            }
            mba_xy = a;
            mbb_xy = if field {
                let mut b = mb_xy - stride;
                if mb_y & 1 == 0
                    && self.slice_at(b) == sl.slice_num
                    && is_interlaced(self.cur_type(b))
                {
                    b -= stride;
                }
                b
            } else {
                mb_x + (mb_y - 1) * stride
            };
        } else {
            let mb_xy = sl.mb_xy;
            mba_xy = mb_xy - 1;
            let field_picture = i32::from(self.picture_structure != crate::slice::PICT_FRAME);
            mbb_xy = mb_xy - (stride << field_picture);
        }
        let mut ctx = 0;
        if self.slice_at(mba_xy) == sl.slice_num && !is_skip(self.cur_type(mba_xy)) {
            ctx += 1;
        }
        if self.slice_at(mbb_xy) == sl.slice_num && !is_skip(self.cur_type(mbb_xy)) {
            ctx += 1;
        }
        if sl.hdr.slice_type_nos == TYPE_B {
            ctx += 13;
        }
        sl.cabac.get(d.buf, &mut sl.cabac_state[11 + ctx])
    }

    /// An intra 4x4 or 8x8 block's mode: the predicted one, or another
    /// (`decode_cabac_mb_intra4x4_pred_mode`).
    fn cabac_intra4x4_pred_mode(sl: &mut SliceContext, d: Data<'_>, pred_mode: i32) -> i32 {
        if sl.cabac.get(d.buf, &mut sl.cabac_state[68]) != 0 {
            return pred_mode;
        }
        let mut mode = sl.cabac.get(d.buf, &mut sl.cabac_state[69]);
        mode += 2 * sl.cabac.get(d.buf, &mut sl.cabac_state[69]);
        mode += 4 * sl.cabac.get(d.buf, &mut sl.cabac_state[69]);
        mode + i32::from(mode >= pred_mode)
    }

    /// The chroma prediction mode as coded (`decode_cabac_mb_chroma_pre_mode`).
    fn cabac_chroma_pred_mode(&self, sl: &mut SliceContext, d: Data<'_>) -> i32 {
        let mba_xy = sl.left_mb_xy[0];
        let mbb_xy = sl.top_mb_xy;
        let mut ctx = 0;
        // An intra neighbour of either kind has its mode in the table; an
        // inter one has 0 there.
        if sl.left_type[0] != 0 && self.chroma_pred_mode_table[mba_xy as usize] != 0 {
            ctx += 1;
        }
        if sl.top_type != 0 && self.chroma_pred_mode_table[mbb_xy as usize] != 0 {
            ctx += 1;
        }
        if sl.cabac.get(d.buf, &mut sl.cabac_state[64 + ctx]) == 0 {
            return 0;
        }
        if sl.cabac.get(d.buf, &mut sl.cabac_state[64 + 3]) == 0 {
            return 1;
        }
        if sl.cabac.get(d.buf, &mut sl.cabac_state[64 + 3]) == 0 {
            2
        } else {
            3
        }
    }

    /// The luma half of the coded block pattern (`decode_cabac_mb_cbp_luma`).
    fn cabac_cbp_luma(sl: &mut SliceContext, d: Data<'_>) -> i32 {
        let cbp_a = sl.left_cbp;
        let cbp_b = sl.top_cbp;
        let nz = |v: i32| i32::from(v == 0) as usize;
        let mut cbp = 0;
        let ctx = nz(cbp_a & 0x02) + 2 * nz(cbp_b & 0x04);
        cbp += sl.cabac.get(d.buf, &mut sl.cabac_state[73 + ctx]);
        let ctx = nz(cbp & 0x01) + 2 * nz(cbp_b & 0x08);
        cbp += sl.cabac.get(d.buf, &mut sl.cabac_state[73 + ctx]) << 1;
        let ctx = nz(cbp_a & 0x08) + 2 * nz(cbp & 0x01);
        cbp += sl.cabac.get(d.buf, &mut sl.cabac_state[73 + ctx]) << 2;
        let ctx = nz(cbp & 0x04) + 2 * nz(cbp & 0x02);
        cbp += sl.cabac.get(d.buf, &mut sl.cabac_state[73 + ctx]) << 3;
        cbp
    }

    /// The chroma half (`decode_cabac_mb_cbp_chroma`): 0, 1 or 2.
    fn cabac_cbp_chroma(sl: &mut SliceContext, d: Data<'_>) -> i32 {
        let cbp_a = (sl.left_cbp >> 4) & 0x03;
        let cbp_b = (sl.top_cbp >> 4) & 0x03;
        let mut ctx = 0;
        if cbp_a > 0 {
            ctx += 1;
        }
        if cbp_b > 0 {
            ctx += 2;
        }
        if sl.cabac.get(d.buf, &mut sl.cabac_state[77 + ctx]) == 0 {
            return 0;
        }
        let mut ctx = 4;
        if cbp_a == 2 {
            ctx += 1;
        }
        if cbp_b == 2 {
            ctx += 2;
        }
        1 + sl.cabac.get(d.buf, &mut sl.cabac_state[77 + ctx])
    }

    /// A P 8x8 partition's sub-type code (`decode_cabac_p_mb_sub_type`).
    fn cabac_p_sub_type(sl: &mut SliceContext, d: Data<'_>) -> usize {
        if sl.cabac.get(d.buf, &mut sl.cabac_state[21]) != 0 {
            return 0;
        }
        if sl.cabac.get(d.buf, &mut sl.cabac_state[22]) == 0 {
            return 1;
        }
        if sl.cabac.get(d.buf, &mut sl.cabac_state[23]) != 0 {
            return 2;
        }
        3
    }

    /// A B 8x8 partition's sub-type code (`decode_cabac_b_mb_sub_type`).
    fn cabac_b_sub_type(sl: &mut SliceContext, d: Data<'_>) -> usize {
        if sl.cabac.get(d.buf, &mut sl.cabac_state[36]) == 0 {
            return 0;
        }
        if sl.cabac.get(d.buf, &mut sl.cabac_state[37]) == 0 {
            return 1 + sl.cabac.get(d.buf, &mut sl.cabac_state[39]) as usize;
        }
        let mut kind = 3;
        if sl.cabac.get(d.buf, &mut sl.cabac_state[38]) != 0 {
            if sl.cabac.get(d.buf, &mut sl.cabac_state[39]) != 0 {
                return 11 + sl.cabac.get(d.buf, &mut sl.cabac_state[39]) as usize;
            }
            kind += 4;
        }
        kind += 2 * sl.cabac.get(d.buf, &mut sl.cabac_state[39]) as usize;
        kind += sl.cabac.get(d.buf, &mut sl.cabac_state[39]) as usize;
        kind
    }

    /// A partition's reference index (`decode_cabac_mb_ref`): -1 past 32.
    fn cabac_mb_ref(sl: &mut SliceContext, d: Data<'_>, list: usize, n: usize) -> i32 {
        let refa = i32::from(sl.ref_cache[list][SCAN8[n] - 1]);
        let refb = i32::from(sl.ref_cache[list][SCAN8[n] - 8]);
        let mut r = 0;
        let mut ctx = 0;
        let direct = (MB_TYPE_DIRECT2 >> 1) as u8;
        if sl.hdr.slice_type_nos == TYPE_B {
            if refa > 0 && sl.direct_cache[SCAN8[n] - 1] & direct == 0 {
                ctx += 1;
            }
            if refb > 0 && sl.direct_cache[SCAN8[n] - 8] & direct == 0 {
                ctx += 2;
            }
        } else {
            if refa > 0 {
                ctx += 1;
            }
            if refb > 0 {
                ctx += 2;
            }
        }
        while sl.cabac.get(d.buf, &mut sl.cabac_state[54 + ctx]) != 0 {
            r += 1;
            ctx = (ctx >> 2) + 4;
            if r >= 32 {
                return -1;
            }
        }
        r
    }

    /// One component of a vector difference, and its magnitude capped at
    /// 70 for the next one's context (`decode_cabac_mb_mvd`); `None` for
    /// an escape longer than any vector.
    fn cabac_mvd(
        sl: &mut SliceContext,
        d: Data<'_>,
        ctxbase: usize,
        amvd: i32,
    ) -> Option<(i32, u8)> {
        let ctx = ctxbase + 2 - usize::from(amvd < 3) - usize::from(amvd < 33);
        if sl.cabac.get(d.buf, &mut sl.cabac_state[ctx]) == 0 {
            return Some((0, 0));
        }
        let mut mvd: i32 = 1;
        let mut ctx = ctxbase + 3;
        while mvd < 9 && sl.cabac.get(d.buf, &mut sl.cabac_state[ctx]) != 0 {
            if mvd < 4 {
                ctx += 1;
            }
            mvd += 1;
        }
        let mvda;
        if mvd >= 9 {
            let mut k = 3;
            while sl.cabac.bypass(d.buf) != 0 {
                mvd = mvd.wrapping_add(1 << k);
                k += 1;
                if k > 24 {
                    return None;
                }
            }
            while k > 0 {
                k -= 1;
                mvd = mvd.wrapping_add(sl.cabac.bypass(d.buf) << k);
            }
            mvda = if mvd < 70 { mvd } else { 70 };
        } else {
            mvda = mvd;
        }
        Some((sl.cabac.bypass_sign(d.buf, mvd.wrapping_neg()), mvda as u8))
    }

    /// `DECODE_CABAC_MB_MVD`: block `n`'s vector difference, added to the
    /// predicted vector; with its magnitudes.
    fn cabac_mb_mvd(
        sl: &mut SliceContext,
        d: Data<'_>,
        list: usize,
        n: usize,
        (mx, my): (i32, i32),
    ) -> Result<(i32, i32, u8, u8), DecodeError> {
        let c = &sl.mvd_cache[list];
        let amvd0 = i32::from(c[SCAN8[n] - 1][0]) + i32::from(c[SCAN8[n] - 8][0]);
        let amvd1 = i32::from(c[SCAN8[n] - 1][1]) + i32::from(c[SCAN8[n] - 8][1]);
        let x = Self::cabac_mvd(sl, d, 40, amvd0);
        let y = Self::cabac_mvd(sl, d, 47, amvd1);
        let ((mxd, mpx), (myd, mpy)) = x.zip(y).ok_or(DecodeError)?;
        Ok((mx.wrapping_add(mxd), my.wrapping_add(myd), mpx, mpy))
    }

    /// A DC block's coefficients behind its coded-block flag
    /// (`decode_cabac_residual_dc`, `_dc_422`), and its coded-block bit.
    fn cabac_residual_dc(
        &mut self,
        sl: &mut SliceContext,
        d: Data<'_>,
        block: Block<'_>,
        at: DcAt,
    ) -> Result<(), DecodeError> {
        let ctx = cbf_ctx(sl, block.cat, block.n, true);
        if sl.cabac.get(d.buf, &mut sl.cabac_state[ctx]) == 0 {
            sl.non_zero_count_cache[SCAN8[block.n]] = 0;
            return Ok(());
        }
        let field = sl.hdr.mb_field_decoding_flag != 0;
        let wide = self.pixel_shift != 0;
        let target: &mut [i32] = match at {
            DcAt::Luma(p) => &mut sl.mb_luma_dc[p],
            DcAt::Mb(start) => &mut sl.mb[start..],
        };
        residual(
            &mut sl.cabac,
            d.buf,
            &mut sl.cabac_state,
            &mut sl.non_zero_count_cache,
            target,
            &block,
            field,
            wide,
        )?;
        let mb_xy = sl.mb_xy as usize;
        if block.cat == 3 {
            self.cbp_table[mb_xy] |= (0x40 << (block.n - CHROMA_DC_BLOCK_INDEX)) as u16;
        } else {
            self.cbp_table[mb_xy] |= (0x100 << (block.n - LUMA_DC_BLOCK_INDEX)) as u16;
        }
        Ok(())
    }

    /// A 4x4 or 8x8 block's coefficients behind its coded-block flag
    /// (`decode_cabac_residual_nondc`), at `start` in the macroblock's
    /// coefficients. An 8x8 luma block of a non-4:4:4 stream has no flag.
    fn cabac_residual_nondc(
        &self,
        sl: &mut SliceContext,
        d: Data<'_>,
        block: Block<'_>,
        start: usize,
    ) -> Result<(), DecodeError> {
        if block.cat != 5 || self.chroma_format_idc == 3 {
            let ctx = cbf_ctx(sl, block.cat, block.n, false);
            if sl.cabac.get(d.buf, &mut sl.cabac_state[ctx]) == 0 {
                if block.max_coeff == 64 {
                    fill(&mut sl.non_zero_count_cache, SCAN8[block.n], 2, 2, 0);
                } else {
                    sl.non_zero_count_cache[SCAN8[block.n]] = 0;
                }
                return Ok(());
            }
        }
        let field = sl.hdr.mb_field_decoding_flag != 0;
        let wide = self.pixel_shift != 0;
        residual(
            &mut sl.cabac,
            d.buf,
            &mut sl.cabac_state,
            &mut sl.non_zero_count_cache,
            &mut sl.mb[start..],
            &block,
            field,
            wide,
        )?;
        Ok(())
    }

    /// Plane `p`'s luma-like coefficients (`decode_cabac_luma_residual`):
    /// an intra 16x16 macroblock's DC and AC blocks, or each coded 8x8's
    /// four 4x4 blocks or its one 8x8.
    #[allow(clippy::too_many_arguments, reason = "FFmpeg's signature")]
    fn cabac_luma_residual(
        &mut self,
        sl: &mut SliceContext,
        d: Data<'_>,
        scan: usize,
        scan8x8: usize,
        mb_type: u32,
        cbp: i32,
        p: usize,
    ) -> Result<(), DecodeError> {
        const CTX_CAT: [[usize; 3]; 4] = [[0, 6, 10], [1, 7, 11], [2, 8, 12], [5, 9, 13]];
        let pps = self.ps.pps.clone().ok_or(DecodeError)?;
        let qscale = if p == 0 {
            sl.hdr.qscale
        } else {
            sl.hdr.chroma_qp[p - 1]
        };
        let qscale = usize::try_from(qscale).map_err(|_| DecodeError)?;
        let scans = self.scans.all;
        let scan_at = |start: usize| Scan {
            tables: &scans,
            start,
        };
        if is_intra16x16(mb_type) {
            sl.mb_luma_dc[p] = [0; 16];
            let dc = Block {
                cat: CTX_CAT[0][p],
                n: LUMA_DC_BLOCK_INDEX + p,
                scan: scan_at(scan),
                qmul: None,
                max_coeff: 16,
                is_dc: true,
                chroma422: false,
            };
            self.cabac_residual_dc(sl, d, dc, DcAt::Luma(p))?;
            if cbp & 15 != 0 {
                let qmul = pps.dequant4(p, qscale).ok_or(DecodeError)?;
                for i4x4 in 0..16 {
                    let index = 16 * p + i4x4;
                    let ac = Block {
                        cat: CTX_CAT[1][p],
                        n: index,
                        scan: scan_at(scan + 1),
                        qmul: Some(qmul),
                        max_coeff: 15,
                        is_dc: false,
                        chroma422: false,
                    };
                    self.cabac_residual_nondc(sl, d, ac, 16 * index)?;
                }
            } else {
                fill(&mut sl.non_zero_count_cache, SCAN8[16 * p], 4, 4, 0);
            }
        } else {
            let cqm = if is_intra(mb_type) { 0 } else { 3 } + p;
            for i8x8 in 0..4 {
                if cbp & (1 << i8x8) != 0 {
                    if is_8x8dct(mb_type) {
                        let index = 16 * p + 4 * i8x8;
                        let qmul = pps.dequant8(cqm, qscale).ok_or(DecodeError)?;
                        let b8 = Block {
                            cat: CTX_CAT[3][p],
                            n: index,
                            scan: scan_at(scan8x8),
                            qmul: Some(qmul),
                            max_coeff: 64,
                            is_dc: false,
                            chroma422: false,
                        };
                        self.cabac_residual_nondc(sl, d, b8, 16 * index)?;
                    } else {
                        let qmul = pps.dequant4(cqm, qscale).ok_or(DecodeError)?;
                        for i4x4 in 0..4 {
                            let index = 16 * p + 4 * i8x8 + i4x4;
                            let b4 = Block {
                                cat: CTX_CAT[2][p],
                                n: index,
                                scan: scan_at(scan),
                                qmul: Some(qmul),
                                max_coeff: 16,
                                is_dc: false,
                                chroma422: false,
                            };
                            self.cabac_residual_nondc(sl, d, b4, 16 * index)?;
                        }
                    }
                } else {
                    fill(
                        &mut sl.non_zero_count_cache,
                        SCAN8[4 * i8x8 + 16 * p],
                        2,
                        2,
                        0,
                    );
                }
            }
        }
        Ok(())
    }

    /// One macroblock read from a CABAC slice (`ff_h264_decode_mb_cabac`).
    #[allow(
        clippy::too_many_lines,
        reason = "one function in FFmpeg, kept whole to read against it"
    )]
    pub(crate) fn decode_mb_cabac(
        &mut self,
        sl: &mut SliceContext,
        d: Data<'_>,
    ) -> Result<(), DecodeError> {
        let pps = self.ps.pps.clone().ok_or(DecodeError)?;
        let sps = self.ps.sps.clone().ok_or(DecodeError)?;
        let mut dct8x8_allowed = pps.transform_8x8_mode != 0;
        let decode_chroma = sps.chroma_format_idc == 1 || sps.chroma_format_idc == 2;

        let mb_xy = sl.mb_x + sl.mb_y * self.mb_stride;
        sl.mb_xy = mb_xy;
        let mut cbp: i32 = 0;
        let cur = self.cur_pic_ptr.ok_or(DecodeError)?;
        let pic_at = tab(table_offset(self.pics[cur].mb_stride), mb_xy);

        if sl.hdr.slice_type_nos != TYPE_I {
            // A skipped macroblock needs the field flag of the one after.
            let skip = if self.mb_aff_frame && sl.mb_y & 1 == 1 && sl.prev_mb_skipped != 0 {
                sl.next_mb_skipped
            } else {
                self.cabac_mb_skip(sl, d, sl.mb_x, sl.mb_y)
            };
            if skip != 0 {
                if self.mb_aff_frame && sl.mb_y & 1 == 0 {
                    self.pics[cur].mb_type[pic_at] = MB_TYPE_SKIP;
                    sl.next_mb_skipped = self.cabac_mb_skip(sl, d, sl.mb_x, sl.mb_y + 1);
                    if sl.next_mb_skipped == 0 {
                        let field = self.cabac_field_decoding_flag(sl, d);
                        sl.hdr.mb_mbaff = field;
                        sl.hdr.mb_field_decoding_flag = field;
                    }
                }
                self.decode_mb_skip(sl, true);
                self.cbp_table[mb_xy as usize] = 0;
                self.chroma_pred_mode_table[mb_xy as usize] = 0;
                sl.hdr.last_qscale_diff = 0;
                return Ok(());
            }
        }
        if self.mb_aff_frame && sl.mb_y & 1 == 0 {
            let field = self.cabac_field_decoding_flag(sl, d);
            sl.hdr.mb_mbaff = field;
            sl.hdr.mb_field_decoding_flag = field;
        }

        sl.prev_mb_skipped = 0;

        self.fill_decode_neighbors(sl, sl.hdr.mb_field_decoding_flag.wrapping_neg() as u32);

        let mut partition_count = 0;
        let intra_code;
        let mut mb_type;
        if sl.hdr.slice_type_nos == TYPE_B {
            let mut ctx = 0;
            if !is_direct(sl.left_type[0].wrapping_sub(1)) {
                ctx += 1;
            }
            if !is_direct(sl.top_type.wrapping_sub(1)) {
                ctx += 1;
            }
            let code = if sl.cabac.get(d.buf, &mut sl.cabac_state[27 + ctx]) == 0 {
                Some(0)
            } else if sl.cabac.get(d.buf, &mut sl.cabac_state[27 + 3]) == 0 {
                Some(1 + sl.cabac.get(d.buf, &mut sl.cabac_state[27 + 5]) as usize)
            } else {
                let mut bits = sl.cabac.get(d.buf, &mut sl.cabac_state[27 + 4]) << 3;
                bits += sl.cabac.get(d.buf, &mut sl.cabac_state[27 + 5]) << 2;
                bits += sl.cabac.get(d.buf, &mut sl.cabac_state[27 + 5]) << 1;
                bits += sl.cabac.get(d.buf, &mut sl.cabac_state[27 + 5]);
                if bits < 8 {
                    Some(bits as usize + 3)
                } else if bits == 13 {
                    None
                } else if bits == 14 {
                    Some(11)
                } else if bits == 15 {
                    Some(22)
                } else {
                    bits = (bits << 1) + sl.cabac.get(d.buf, &mut sl.cabac_state[27 + 5]);
                    Some(bits as usize - 4)
                }
            };
            if let Some(code) = code {
                let info = B_MB_TYPE_INFO[code];
                partition_count = usize::from(info.partition_count);
                mb_type = info.kind;
                intra_code = None;
            } else {
                intra_code = Some(Self::cabac_intra_mb_type(sl, d, 32, false));
                mb_type = 0;
            }
        } else if sl.hdr.slice_type_nos == TYPE_P {
            if sl.cabac.get(d.buf, &mut sl.cabac_state[14]) == 0 {
                let code = if sl.cabac.get(d.buf, &mut sl.cabac_state[15]) == 0 {
                    3 * sl.cabac.get(d.buf, &mut sl.cabac_state[16]) as usize
                } else {
                    2 - sl.cabac.get(d.buf, &mut sl.cabac_state[17]) as usize
                };
                let info = P_MB_TYPE_INFO[code];
                partition_count = usize::from(info.partition_count);
                mb_type = info.kind;
                intra_code = None;
            } else {
                intra_code = Some(Self::cabac_intra_mb_type(sl, d, 17, false));
                mb_type = 0;
            }
        } else {
            let mut code = Self::cabac_intra_mb_type(sl, d, 3, true);
            if sl.hdr.slice_type == TYPE_SI && code != 0 {
                code -= 1;
            }
            intra_code = Some(code);
            mb_type = 0;
        }
        if let Some(code) = intra_code {
            // decode_intra_mb.
            partition_count = 0;
            let info = I_MB_TYPE_INFO[code];
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
            let mb_size = (MB_SIZES.get(chroma).ok_or(DecodeError)? * sps.bit_depth_luma) >> 3;
            // The bytes the arithmetic decoder has not yet used.
            let ptr = sl.cabac.unread();
            if sl.cabac.end - ptr < mb_size as isize {
                return Err(DecodeError);
            }
            sl.intra_pcm_pos = usize::try_from(ptr).map_err(|_| DecodeError)?;
            let ptr = ptr + mb_size as isize;
            let end = sl.cabac.end;
            sl.cabac.init(d.buf, d.origin, ptr, end - ptr)?;
            // Every block present, the loop filter's QP 0, every
            // coefficient there.
            self.cbp_table[mb_xy as usize] = 0xF7EF;
            self.chroma_pred_mode_table[mb_xy as usize] = 0;
            self.pics[cur].qscale_table[pic_at] = 0;
            self.non_zero_count[mb_xy as usize] = [16; 48];
            self.pics[cur].mb_type[pic_at] = mb_type;
            sl.hdr.last_qscale_diff = 0;
            return Ok(());
        }

        self.fill_decode_caches(sl, mb_type, true);

        let list_count = sl.hdr.list_count as usize;
        let mbaff = sl.hdr.mb_mbaff as u32;
        if is_intra(mb_type) {
            if is_intra4x4(mb_type) {
                if dct8x8_allowed
                    && sl.cabac.get(
                        d.buf,
                        &mut sl.cabac_state[399 + sl.neighbor_transform_size as usize],
                    ) != 0
                {
                    mb_type |= MB_TYPE_8X8DCT;
                    for i in (0..16).step_by(4) {
                        let pred = Self::pred_intra_mode(sl, i);
                        let mode = Self::cabac_intra4x4_pred_mode(sl, d, pred);
                        fill(&mut sl.intra4x4_pred_mode_cache, SCAN8[i], 2, 2, mode as i8);
                    }
                } else {
                    for i in 0..16 {
                        let pred = Self::pred_intra_mode(sl, i);
                        sl.intra4x4_pred_mode_cache[SCAN8[i]] =
                            Self::cabac_intra4x4_pred_mode(sl, d, pred) as i8;
                    }
                }
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
            if decode_chroma {
                let mode = self.cabac_chroma_pred_mode(sl, d);
                self.chroma_pred_mode_table[mb_xy as usize] = mode as u8;
                sl.chroma_pred_mode = check_intra_pred_mode(
                    sl.top_samples_available,
                    sl.left_samples_available,
                    mode as u32,
                    true,
                )?;
            } else {
                sl.chroma_pred_mode = DC_128_PRED8X8;
            }
        } else if partition_count == 4 {
            let mut sub_partition_count = [0usize; 4];
            let mut refs = [[0i32; 4]; 2];
            if sl.hdr.slice_type_nos == TYPE_B {
                for i in 0..4 {
                    let info = B_SUB_MB_TYPE_INFO[Self::cabac_b_sub_type(sl, d)];
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
                    for i in 0..4 {
                        let flag = ((sl.sub_mb_type[i] >> 1) & 0xFF) as u8;
                        fill(&mut sl.direct_cache, SCAN8[4 * i], 2, 2, flag);
                    }
                }
            } else {
                for i in 0..4 {
                    let info = P_SUB_MB_TYPE_INFO[Self::cabac_p_sub_type(sl, d)];
                    sub_partition_count[i] = usize::from(info.partition_count);
                    sl.sub_mb_type[i] = info.kind as u16;
                }
            }

            for list in 0..list_count {
                for i in 0..4 {
                    let sub = u32::from(sl.sub_mb_type[i]);
                    if is_direct(sub) {
                        continue;
                    }
                    refs[list][i] = if is_dir(sub, 0, list as u32) {
                        let rc = sl.hdr.ref_count[list] << mbaff;
                        if rc > 1 {
                            let r = Self::cabac_mb_ref(sl, d, list, 4 * i);
                            if r as u32 >= rc {
                                return Err(DecodeError);
                            }
                            r
                        } else {
                            0
                        }
                    } else {
                        -1
                    };
                    let corner = SCAN8[4 * i];
                    let r = refs[list][i] as i8;
                    sl.ref_cache[list][corner + 1] = r;
                    sl.ref_cache[list][corner + 8] = r;
                    sl.ref_cache[list][corner + 9] = r;
                }
            }

            if dct8x8_allowed {
                dct8x8_allowed = self.get_dct8x8_allowed(sl);
            }

            for list in 0..list_count {
                for i in 0..4 {
                    let corner = SCAN8[4 * i];
                    sl.ref_cache[list][corner] = sl.ref_cache[list][corner + 1];
                    let sub_mb_type = u32::from(sl.sub_mb_type[i]);
                    if is_direct(sub_mb_type) {
                        fill(&mut sl.mvd_cache[list], corner, 2, 2, [0, 0]);
                        continue;
                    }
                    if is_dir(sub_mb_type, 0, list as u32) {
                        let block_width = if sub_mb_type & (MB_TYPE_16X16 | MB_TYPE_16X8) != 0 {
                            2
                        } else {
                            1
                        };
                        for j in 0..sub_partition_count[i] {
                            let index = 4 * i + block_width * j;
                            let at = SCAN8[index];
                            let predicted = self.pred_motion(
                                sl,
                                index,
                                block_width,
                                list,
                                i32::from(sl.ref_cache[list][at]),
                            );
                            let (mx, my, mpx, mpy) =
                                Self::cabac_mb_mvd(sl, d, list, index, predicted)?;
                            let mv = [mx as i16, my as i16];
                            let mvd = [mpx, mpy];
                            let cache = &mut sl.mv_cache[list];
                            let dcache = &mut sl.mvd_cache[list];
                            if is_sub_8x8(sub_mb_type) {
                                for k in [1, 8, 9] {
                                    cache[at + k] = mv;
                                    dcache[at + k] = mvd;
                                }
                            } else if is_sub_8x4(sub_mb_type) {
                                cache[at + 1] = mv;
                                dcache[at + 1] = mvd;
                            } else if is_sub_4x8(sub_mb_type) {
                                cache[at + 8] = mv;
                                dcache[at + 8] = mvd;
                            }
                            cache[at] = mv;
                            dcache[at] = mvd;
                        }
                    } else {
                        fill(&mut sl.mv_cache[list], corner, 2, 2, [0, 0]);
                        fill(&mut sl.mvd_cache[list], corner, 2, 2, [0, 0]);
                    }
                }
            }
        } else if is_direct(mb_type) {
            self.pred_direct_motion(sl, &mut mb_type);
            fill(&mut sl.mvd_cache[0], SCAN8[0], 4, 4, [0, 0]);
            fill(&mut sl.mvd_cache[1], SCAN8[0], 4, 4, [0, 0]);
            dct8x8_allowed &= sps.direct_8x8_inference_flag != 0;
        } else if is_16x16(mb_type) {
            for list in 0..list_count {
                if is_dir(mb_type, 0, list as u32) {
                    let rc = sl.hdr.ref_count[list] << mbaff;
                    let r = if rc > 1 {
                        let r = Self::cabac_mb_ref(sl, d, list, 0);
                        if r as u32 >= rc {
                            return Err(DecodeError);
                        }
                        r
                    } else {
                        0
                    };
                    fill(&mut sl.ref_cache[list], SCAN8[0], 4, 4, r as i8);
                }
            }
            for list in 0..list_count {
                if is_dir(mb_type, 0, list as u32) {
                    let predicted =
                        self.pred_motion(sl, 0, 4, list, i32::from(sl.ref_cache[list][SCAN8[0]]));
                    let (mx, my, mpx, mpy) = Self::cabac_mb_mvd(sl, d, list, 0, predicted)?;
                    fill(&mut sl.mvd_cache[list], SCAN8[0], 4, 4, [mpx, mpy]);
                    fill(
                        &mut sl.mv_cache[list],
                        SCAN8[0],
                        4,
                        4,
                        [mx as i16, my as i16],
                    );
                }
            }
        } else if is_16x8(mb_type) {
            for list in 0..list_count {
                for i in 0..2 {
                    let val = if is_dir(mb_type, i as u32, list as u32) {
                        let rc = sl.hdr.ref_count[list] << mbaff;
                        if rc > 1 {
                            let r = Self::cabac_mb_ref(sl, d, list, 8 * i);
                            if r as u32 >= rc {
                                return Err(DecodeError);
                            }
                            r as i8
                        } else {
                            0
                        }
                    } else {
                        LIST_NOT_USED
                    };
                    fill(&mut sl.ref_cache[list], SCAN8[0] + 16 * i, 4, 2, val);
                }
            }
            for list in 0..list_count {
                for i in 0..2 {
                    if is_dir(mb_type, i as u32, list as u32) {
                        let r = i32::from(sl.ref_cache[list][SCAN8[0] + 16 * i]);
                        let predicted = self.pred_16x8_motion(sl, 8 * i, list, r);
                        let (mx, my, mpx, mpy) = Self::cabac_mb_mvd(sl, d, list, 8 * i, predicted)?;
                        fill(&mut sl.mvd_cache[list], SCAN8[0] + 16 * i, 4, 2, [mpx, mpy]);
                        fill(
                            &mut sl.mv_cache[list],
                            SCAN8[0] + 16 * i,
                            4,
                            2,
                            [mx as i16, my as i16],
                        );
                    } else {
                        fill(&mut sl.mvd_cache[list], SCAN8[0] + 16 * i, 4, 2, [0, 0]);
                        fill(&mut sl.mv_cache[list], SCAN8[0] + 16 * i, 4, 2, [0, 0]);
                    }
                }
            }
        } else {
            // 8x16.
            for list in 0..list_count {
                for i in 0..2 {
                    let val = if is_dir(mb_type, i as u32, list as u32) {
                        let rc = sl.hdr.ref_count[list] << mbaff;
                        if rc > 1 {
                            let r = Self::cabac_mb_ref(sl, d, list, 4 * i);
                            if r as u32 >= rc {
                                return Err(DecodeError);
                            }
                            r as i8
                        } else {
                            0
                        }
                    } else {
                        LIST_NOT_USED
                    };
                    fill(&mut sl.ref_cache[list], SCAN8[0] + 2 * i, 2, 4, val);
                }
            }
            for list in 0..list_count {
                for i in 0..2 {
                    if is_dir(mb_type, i as u32, list as u32) {
                        let r = i32::from(sl.ref_cache[list][SCAN8[0] + 2 * i]);
                        let predicted = self.pred_8x16_motion(sl, i * 4, list, r);
                        let (mx, my, mpx, mpy) = Self::cabac_mb_mvd(sl, d, list, 4 * i, predicted)?;
                        fill(&mut sl.mvd_cache[list], SCAN8[0] + 2 * i, 2, 4, [mpx, mpy]);
                        fill(
                            &mut sl.mv_cache[list],
                            SCAN8[0] + 2 * i,
                            2,
                            4,
                            [mx as i16, my as i16],
                        );
                    } else {
                        fill(&mut sl.mvd_cache[list], SCAN8[0] + 2 * i, 2, 4, [0, 0]);
                        fill(&mut sl.mv_cache[list], SCAN8[0] + 2 * i, 2, 4, [0, 0]);
                    }
                }
            }
        }

        if is_inter(mb_type) {
            self.chroma_pred_mode_table[mb_xy as usize] = 0;
            self.write_back_motion(sl, mb_type, true);
        }

        if !is_intra16x16(mb_type) {
            cbp = Self::cabac_cbp_luma(sl, d);
            if decode_chroma {
                cbp |= Self::cabac_cbp_chroma(sl, d) << 4;
            }
        } else if !decode_chroma && cbp > 15 {
            // Chroma coefficients in a stream with no chroma.
            return Err(DecodeError);
        }

        self.cbp_table[mb_xy as usize] = cbp as u16;
        sl.cbp = cbp;

        if dct8x8_allowed && cbp & 15 != 0 && !is_intra(mb_type) {
            let ctx = 399 + sl.neighbor_transform_size as usize;
            if sl.cabac.get(d.buf, &mut sl.cabac_state[ctx]) != 0 {
                mb_type |= MB_TYPE_8X8DCT;
            }
        }

        // The neighbours' 8x8 counts, which fill_decode_caches could not
        // know were wanted until the transform was read.
        if self.chroma_format_idc == 3 && is_8x8dct(mb_type) {
            let nnz = &mut sl.non_zero_count_cache;
            let old_x264 = (self.x264_build as u32) < 151;
            for i in 0..2 {
                let left = sl.left_type[i];
                if left != 0 && !is_8x8dct(left) {
                    let v = if old_x264 {
                        if is_intra(mb_type) { 64 } else { 0 }
                    } else if is_intra_pcm(left) {
                        64
                    } else {
                        0
                    };
                    for k in [1, 2, 6, 7, 11, 12] {
                        nnz[3 + 8 * k + 2 * 8 * i] = v;
                    }
                }
            }
            if sl.top_type != 0 && !is_8x8dct(sl.top_type) {
                let v = if old_x264 {
                    if is_intra(mb_type) { 0x40 } else { 0 }
                } else if is_intra_pcm(sl.top_type) {
                    0x40
                } else {
                    0
                };
                for row in [0, 5, 10] {
                    nnz[4 + 8 * row..4 + 8 * row + 4].fill(v);
                }
            }
        }
        self.pics[cur].mb_type[pic_at] = mb_type;

        if cbp != 0 || is_intra16x16(mb_type) {
            // decode_cabac_mb_dqp.
            let ctx = 60 + usize::from(sl.hdr.last_qscale_diff != 0);
            if sl.cabac.get(d.buf, &mut sl.cabac_state[ctx]) != 0 {
                let mut val = 1;
                let mut ctx = 2;
                let max_qp = 51 + 6 * (sps.bit_depth_luma - 8);
                while sl.cabac.get(d.buf, &mut sl.cabac_state[60 + ctx]) != 0 {
                    ctx = 3;
                    val += 1;
                    if val > 2 * max_qp {
                        return Err(DecodeError);
                    }
                }
                val = if val & 1 != 0 {
                    (val + 1) >> 1
                } else {
                    -((val + 1) >> 1)
                };
                sl.hdr.last_qscale_diff = val;
                sl.hdr.qscale += val;
                if sl.hdr.qscale as u32 > max_qp as u32 {
                    if sl.hdr.qscale < 0 {
                        sl.hdr.qscale += max_qp + 1;
                    } else {
                        sl.hdr.qscale -= max_qp + 1;
                    }
                }
                let qp = usize::try_from(sl.hdr.qscale).map_err(|_| DecodeError)?;
                sl.hdr.chroma_qp = [
                    i32::from(*pps.chroma_qp_table[0].get(qp).ok_or(DecodeError)?),
                    i32::from(*pps.chroma_qp_table[1].get(qp).ok_or(DecodeError)?),
                ];
            } else {
                sl.hdr.last_qscale_diff = 0;
            }

            let (scan, scan8x8) = ScanTables::cabac(is_interlaced(mb_type), sl.hdr.qscale == 0);
            self.cabac_luma_residual(sl, d, scan, scan8x8, mb_type, cbp, 0)?;
            if self.chroma_format_idc == 3 {
                self.cabac_luma_residual(sl, d, scan, scan8x8, mb_type, cbp, 1)?;
                self.cabac_luma_residual(sl, d, scan, scan8x8, mb_type, cbp, 2)?;
            } else {
                let chroma422 = self.chroma_format_idc == 2;
                let scans = self.scans.all;
                if cbp & 0x30 != 0 {
                    for c in 0..2 {
                        let dc = Block {
                            cat: 3,
                            n: CHROMA_DC_BLOCK_INDEX + c,
                            scan: Scan {
                                tables: if chroma422 {
                                    &CHROMA422_DC_SCAN
                                } else {
                                    &CHROMA_DC_SCAN
                                },
                                start: 0,
                            },
                            qmul: None,
                            max_coeff: if chroma422 { 8 } else { 4 },
                            is_dc: true,
                            chroma422,
                        };
                        self.cabac_residual_dc(sl, d, dc, DcAt::Mb(256 + 256 * c))?;
                    }
                }
                if cbp & 0x20 != 0 {
                    for c in 0..2 {
                        let list = c + 1 + if is_intra(mb_type) { 0 } else { 3 };
                        let qp = usize::try_from(sl.hdr.chroma_qp[c]).map_err(|_| DecodeError)?;
                        let qmul = pps.dequant4(list, qp).ok_or(DecodeError)?;
                        let mut block = 16 * (16 + 16 * c);
                        for i8x8 in 0..if chroma422 { 2 } else { 1 } {
                            for i in 0..4 {
                                let index = 16 + 16 * c + 8 * i8x8 + i;
                                let ac = Block {
                                    cat: 4,
                                    n: index,
                                    scan: Scan {
                                        tables: &scans,
                                        start: scan + 1,
                                    },
                                    qmul: Some(qmul),
                                    max_coeff: 15,
                                    is_dc: false,
                                    chroma422: false,
                                };
                                self.cabac_residual_nondc(sl, d, ac, block)?;
                                block += 16;
                            }
                        }
                    }
                } else {
                    fill(&mut sl.non_zero_count_cache, SCAN8[16], 4, 4, 0);
                    fill(&mut sl.non_zero_count_cache, SCAN8[32], 4, 4, 0);
                }
            }
        } else {
            fill(&mut sl.non_zero_count_cache, SCAN8[0], 4, 4, 0);
            fill(&mut sl.non_zero_count_cache, SCAN8[16], 4, 4, 0);
            fill(&mut sl.non_zero_count_cache, SCAN8[32], 4, 4, 0);
            sl.hdr.last_qscale_diff = 0;
        }

        self.pics[cur].qscale_table[pic_at] = sl.hdr.qscale as i8;
        self.write_back_non_zero_count(sl);
        Ok(())
    }
}

/// Where a DC block's coefficients go: a plane's luma DC array, or the
/// macroblock's coefficients from a position (chroma).
#[derive(Clone, Copy)]
enum DcAt {
    Luma(usize),
    Mb(usize),
}
