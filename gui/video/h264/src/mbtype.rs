//! Macroblock types and the tables that map coded values onto them
//! (`MB_TYPE_*` in `mpegutils.h` and `h264_parse.h`, the predicates in
//! `h264dec.h`, and `h264data.c`'s tables), with `scan8`, the layout of the
//! per-macroblock caches.
//!
//! FFmpeg keeps a macroblock's type as a set of flags: its partitioning,
//! which lists each partition predicts from, whether it is intra, skipped,
//! interlaced, direct, or transformed 8x8. The tables turn a slice type's
//! coded `mb_type` into those flags.

pub(crate) const MB_TYPE_INTRA4X4: u32 = 1 << 0;
pub(crate) const MB_TYPE_INTRA16X16: u32 = 1 << 1;
pub(crate) const MB_TYPE_INTRA_PCM: u32 = 1 << 2;
pub(crate) const MB_TYPE_16X16: u32 = 1 << 3;
pub(crate) const MB_TYPE_16X8: u32 = 1 << 4;
pub(crate) const MB_TYPE_8X16: u32 = 1 << 5;
pub(crate) const MB_TYPE_8X8: u32 = 1 << 6;
pub(crate) const MB_TYPE_INTERLACED: u32 = 1 << 7;
pub(crate) const MB_TYPE_DIRECT2: u32 = 1 << 8;
/// `MB_TYPE_REF0`: an 8x8 P macroblock whose partitions all use reference
/// 0 (`MB_TYPE_CODEC_SPECIFIC`).
pub(crate) const MB_TYPE_REF0: u32 = 1 << 9;
pub(crate) const MB_TYPE_P0L0: u32 = 1 << 12;
pub(crate) const MB_TYPE_P1L0: u32 = 1 << 13;
pub(crate) const MB_TYPE_P0L1: u32 = 1 << 14;
pub(crate) const MB_TYPE_P1L1: u32 = 1 << 15;
pub(crate) const MB_TYPE_L0: u32 = MB_TYPE_P0L0 | MB_TYPE_P1L0;
pub(crate) const MB_TYPE_L1: u32 = MB_TYPE_P0L1 | MB_TYPE_P1L1;
pub(crate) const MB_TYPE_L0L1: u32 = MB_TYPE_L0 | MB_TYPE_L1;
pub(crate) const MB_TYPE_SKIP: u32 = 1 << 17;
pub(crate) const MB_TYPE_8X8DCT: u32 = 0x0100_0000;

pub(crate) fn is_intra4x4(a: u32) -> bool {
    a & MB_TYPE_INTRA4X4 != 0
}
pub(crate) fn is_intra16x16(a: u32) -> bool {
    a & MB_TYPE_INTRA16X16 != 0
}
pub(crate) fn is_intra_pcm(a: u32) -> bool {
    a & MB_TYPE_INTRA_PCM != 0
}
pub(crate) fn is_intra(a: u32) -> bool {
    a & 7 != 0
}
pub(crate) fn is_inter(a: u32) -> bool {
    a & (MB_TYPE_16X16 | MB_TYPE_16X8 | MB_TYPE_8X16 | MB_TYPE_8X8) != 0
}
pub(crate) fn is_skip(a: u32) -> bool {
    a & MB_TYPE_SKIP != 0
}
pub(crate) fn is_interlaced(a: u32) -> bool {
    a & MB_TYPE_INTERLACED != 0
}
pub(crate) fn is_direct(a: u32) -> bool {
    a & MB_TYPE_DIRECT2 != 0
}
pub(crate) fn is_16x16(a: u32) -> bool {
    a & MB_TYPE_16X16 != 0
}
pub(crate) fn is_16x8(a: u32) -> bool {
    a & MB_TYPE_16X8 != 0
}
pub(crate) fn is_8x16(a: u32) -> bool {
    a & MB_TYPE_8X16 != 0
}
pub(crate) fn is_8x8(a: u32) -> bool {
    a & MB_TYPE_8X8 != 0
}
pub(crate) fn is_ref0(a: u32) -> bool {
    a & MB_TYPE_REF0 != 0
}
pub(crate) fn is_8x8dct(a: u32) -> bool {
    a & MB_TYPE_8X8DCT != 0
}
/// A sub-macroblock's 8x8 partition, reusing the 16x16 flag (`IS_SUB_8X8`),
/// and so on for the smaller ones.
pub(crate) fn is_sub_8x8(a: u32) -> bool {
    a & MB_TYPE_16X16 != 0
}
pub(crate) fn is_sub_8x4(a: u32) -> bool {
    a & MB_TYPE_16X8 != 0
}
pub(crate) fn is_sub_4x8(a: u32) -> bool {
    a & MB_TYPE_8X16 != 0
}
pub(crate) fn is_sub_4x4(a: u32) -> bool {
    a & MB_TYPE_8X8 != 0
}
/// Whether partition `part` predicts from `list` (`IS_DIR`).
pub(crate) fn is_dir(a: u32, part: u32, list: u32) -> bool {
    a & (MB_TYPE_P0L0 << (part | (list << 1))) != 0
}
/// Whether the macroblock uses `list` at all (`USES_LIST`).
pub(crate) fn uses_list(a: u32, list: u32) -> bool {
    a & ((MB_TYPE_P0L0 | MB_TYPE_P1L0) << (list << 1)) != 0
}

/// The block index of luma DC, and the first of chroma DC's two
/// (`LUMA_DC_BLOCK_INDEX`, `CHROMA_DC_BLOCK_INDEX`).
pub(crate) const LUMA_DC_BLOCK_INDEX: usize = 48;
pub(crate) const CHROMA_DC_BLOCK_INDEX: usize = 49;

/// Where each 4x4 block's entry sits in the caches, which are 8 wide with
/// the neighbours' entries above and to the left (`scan8`): 16 luma
/// blocks, then 16 Cb and 16 Cr (4:4:4 needs them all), then the DC
/// entries for luma, Cb and Cr.
#[rustfmt::skip]
#[allow(
    clippy::identity_op,
    clippy::erasing_op,
    reason = "FFmpeg's table as written: a column plus a row times the cache width"
)]
pub(crate) const SCAN8: [usize; 16 * 3 + 3] = [
    4 +  1 * 8, 5 +  1 * 8, 4 +  2 * 8, 5 +  2 * 8,
    6 +  1 * 8, 7 +  1 * 8, 6 +  2 * 8, 7 +  2 * 8,
    4 +  3 * 8, 5 +  3 * 8, 4 +  4 * 8, 5 +  4 * 8,
    6 +  3 * 8, 7 +  3 * 8, 6 +  4 * 8, 7 +  4 * 8,
    4 +  6 * 8, 5 +  6 * 8, 4 +  7 * 8, 5 +  7 * 8,
    6 +  6 * 8, 7 +  6 * 8, 6 +  7 * 8, 7 +  7 * 8,
    4 +  8 * 8, 5 +  8 * 8, 4 +  9 * 8, 5 +  9 * 8,
    6 +  8 * 8, 7 +  8 * 8, 6 +  9 * 8, 7 +  9 * 8,
    4 + 11 * 8, 5 + 11 * 8, 4 + 12 * 8, 5 + 12 * 8,
    6 + 11 * 8, 7 + 11 * 8, 6 + 12 * 8, 7 + 12 * 8,
    4 + 13 * 8, 5 + 13 * 8, 4 + 14 * 8, 5 + 14 * 8,
    6 + 13 * 8, 7 + 13 * 8, 6 + 14 * 8, 7 + 14 * 8,
    0 +  0 * 8, 0 +  5 * 8, 0 + 10 * 8,
];

/// An intra macroblock type: its flags, its 16x16 prediction mode and its
/// coded block pattern (`IMbInfo`); 255 where the type has none.
#[derive(Clone, Copy, Debug)]
pub(crate) struct IMbInfo {
    pub(crate) kind: u32,
    pub(crate) pred_mode: u8,
    pub(crate) cbp: u8,
}

/// An inter macroblock or sub-macroblock type: its flags and how many
/// partitions it has (`PMbInfo`).
#[derive(Clone, Copy, Debug)]
pub(crate) struct PMbInfo {
    pub(crate) kind: u32,
    pub(crate) partition_count: u8,
}

const fn i(kind: u32, pred_mode: u8, cbp: u8) -> IMbInfo {
    IMbInfo {
        kind,
        pred_mode,
        cbp,
    }
}

const fn p(kind: u32, partition_count: u8) -> PMbInfo {
    PMbInfo {
        kind,
        partition_count,
    }
}

const I16: u32 = MB_TYPE_INTRA16X16;

/// `ff_h264_i_mb_type_info`.
#[rustfmt::skip]
pub(crate) const I_MB_TYPE_INFO: [IMbInfo; 26] = [
    i(MB_TYPE_INTRA4X4, 255, 255),
    i(I16, 2, 0), i(I16, 1, 0), i(I16, 0, 0), i(I16, 3, 0),
    i(I16, 2, 16), i(I16, 1, 16), i(I16, 0, 16), i(I16, 3, 16),
    i(I16, 2, 32), i(I16, 1, 32), i(I16, 0, 32), i(I16, 3, 32),
    i(I16, 2, 15), i(I16, 1, 15), i(I16, 0, 15), i(I16, 3, 15),
    i(I16, 2, 15 + 16), i(I16, 1, 15 + 16), i(I16, 0, 15 + 16), i(I16, 3, 15 + 16),
    i(I16, 2, 15 + 32), i(I16, 1, 15 + 32), i(I16, 0, 15 + 32), i(I16, 3, 15 + 32),
    i(MB_TYPE_INTRA_PCM, 255, 255),
];

const L0: u32 = MB_TYPE_P0L0;
const L0_2: u32 = MB_TYPE_P0L0 | MB_TYPE_P1L0;

/// `ff_h264_p_mb_type_info`.
pub(crate) const P_MB_TYPE_INFO: [PMbInfo; 5] = [
    p(MB_TYPE_16X16 | L0, 1),
    p(MB_TYPE_16X8 | L0_2, 2),
    p(MB_TYPE_8X16 | L0_2, 2),
    p(MB_TYPE_8X8 | L0_2, 4),
    p(MB_TYPE_8X8 | L0_2 | MB_TYPE_REF0, 4),
];

/// `ff_h264_p_sub_mb_type_info`.
pub(crate) const P_SUB_MB_TYPE_INFO: [PMbInfo; 4] = [
    p(MB_TYPE_16X16 | L0, 1),
    p(MB_TYPE_16X8 | L0, 2),
    p(MB_TYPE_8X16 | L0, 2),
    p(MB_TYPE_8X8 | L0, 4),
];

const P0L0: u32 = MB_TYPE_P0L0;
const P1L0: u32 = MB_TYPE_P1L0;
const P0L1: u32 = MB_TYPE_P0L1;
const P1L1: u32 = MB_TYPE_P1L1;

/// `ff_h264_b_mb_type_info`.
#[rustfmt::skip]
pub(crate) const B_MB_TYPE_INFO: [PMbInfo; 23] = [
    p(MB_TYPE_DIRECT2 | MB_TYPE_L0L1, 1),
    p(MB_TYPE_16X16 | P0L0, 1),
    p(MB_TYPE_16X16 | P0L1, 1),
    p(MB_TYPE_16X16 | P0L0 | P0L1, 1),
    p(MB_TYPE_16X8 | P0L0 | P1L0, 2),
    p(MB_TYPE_8X16 | P0L0 | P1L0, 2),
    p(MB_TYPE_16X8 | P0L1 | P1L1, 2),
    p(MB_TYPE_8X16 | P0L1 | P1L1, 2),
    p(MB_TYPE_16X8 | P0L0 | P1L1, 2),
    p(MB_TYPE_8X16 | P0L0 | P1L1, 2),
    p(MB_TYPE_16X8 | P0L1 | P1L0, 2),
    p(MB_TYPE_8X16 | P0L1 | P1L0, 2),
    p(MB_TYPE_16X8 | P0L0 | P1L0 | P1L1, 2),
    p(MB_TYPE_8X16 | P0L0 | P1L0 | P1L1, 2),
    p(MB_TYPE_16X8 | P0L1 | P1L0 | P1L1, 2),
    p(MB_TYPE_8X16 | P0L1 | P1L0 | P1L1, 2),
    p(MB_TYPE_16X8 | P0L0 | P0L1 | P1L0, 2),
    p(MB_TYPE_8X16 | P0L0 | P0L1 | P1L0, 2),
    p(MB_TYPE_16X8 | P0L0 | P0L1 | P1L1, 2),
    p(MB_TYPE_8X16 | P0L0 | P0L1 | P1L1, 2),
    p(MB_TYPE_16X8 | P0L0 | P0L1 | P1L0 | P1L1, 2),
    p(MB_TYPE_8X16 | P0L0 | P0L1 | P1L0 | P1L1, 2),
    p(MB_TYPE_8X8 | P0L0 | P0L1 | P1L0 | P1L1, 4),
];

/// `ff_h264_b_sub_mb_type_info`.
#[rustfmt::skip]
pub(crate) const B_SUB_MB_TYPE_INFO: [PMbInfo; 13] = [
    p(MB_TYPE_DIRECT2, 1),
    p(MB_TYPE_16X16 | P0L0, 1),
    p(MB_TYPE_16X16 | P0L1, 1),
    p(MB_TYPE_16X16 | P0L0 | P0L1, 1),
    p(MB_TYPE_16X8 | P0L0 | P1L0, 2),
    p(MB_TYPE_8X16 | P0L0 | P1L0, 2),
    p(MB_TYPE_16X8 | P0L1 | P1L1, 2),
    p(MB_TYPE_8X16 | P0L1 | P1L1, 2),
    p(MB_TYPE_16X8 | P0L0 | P0L1 | P1L0 | P1L1, 2),
    p(MB_TYPE_8X16 | P0L0 | P0L1 | P1L0 | P1L1, 2),
    p(MB_TYPE_8X8 | P0L0 | P1L0, 4),
    p(MB_TYPE_8X8 | P0L1 | P1L1, 4),
    p(MB_TYPE_8X8 | P0L0 | P0L1 | P1L0 | P1L1, 4),
];

/// `ff_h264_golomb_to_intra4x4_cbp`.
#[rustfmt::skip]
pub(crate) const GOLOMB_TO_INTRA4X4_CBP: [u8; 48] = [
    47, 31, 15, 0,  23, 27, 29, 30, 7,  11, 13, 14, 39, 43, 45, 46,
    16, 3,  5,  10, 12, 19, 21, 26, 28, 35, 37, 42, 44, 1,  2,  4,
    8,  17, 18, 20, 24, 6,  9,  22, 25, 32, 33, 34, 36, 40, 38, 41,
];

/// `ff_h264_golomb_to_inter_cbp`.
#[rustfmt::skip]
pub(crate) const GOLOMB_TO_INTER_CBP: [u8; 48] = [
    0,  16, 1,  2,  4,  8,  32, 3,  5,  10, 12, 15, 47, 7,  11, 13,
    14, 6,  9,  31, 35, 37, 42, 44, 33, 34, 36, 40, 39, 43, 45, 46,
    17, 18, 20, 24, 19, 21, 26, 28, 23, 27, 29, 30, 22, 25, 38, 41,
];

/// The same for monochrome, which has no chroma bits
/// (`golomb_to_intra4x4_cbp_gray`, `golomb_to_inter_cbp_gray` in
/// `h264_cavlc.c`).
pub(crate) const GOLOMB_TO_INTRA4X4_CBP_GRAY: [u8; 16] =
    [15, 0, 7, 11, 13, 14, 3, 5, 10, 12, 1, 2, 4, 8, 6, 9];
pub(crate) const GOLOMB_TO_INTER_CBP_GRAY: [u8; 16] =
    [0, 1, 2, 4, 8, 3, 5, 10, 12, 15, 7, 11, 13, 14, 6, 9];

/// Where chroma DC's coefficients go, 4:2:0 and 4:2:2
/// (`ff_h264_chroma_dc_scan`, `ff_h264_chroma422_dc_scan`).
pub(crate) const CHROMA_DC_SCAN: [u8; 4] = [0, 16, 32, 48];
pub(crate) const CHROMA422_DC_SCAN: [u8; 8] = [0, 32, 16, 64, 96, 48, 80, 112];
