//! The loop filter: smoothing across block and transform edges after a frame
//! is reconstructed, before it is shown or predicted from.
//!
//! Each 8x8 cell's left and top edges are filtered if they are a block's or a
//! transform's edge, with a filter as wide as the transform behind it -- 4,
//! 8 or 16 pixels -- at a strength chosen by the block's segment, reference
//! and mode ([`crate::header::LoopFilterParams::levels`]) and thresholds by
//! the frame's sharpness ([`crate::header::LoopFilterParams::limits`]). A
//! filter only acts where the edge looks like a coding artefact: flat on
//! both sides and a small step across.
//!
//! libvpx decides the edges with bit masks per 64x64 superblock, built from
//! each block as it decodes (`vp9_build_mask`), trimmed at the frame's edges
//! (`vp9_adjust_mask`), then filters superblock by superblock in raster
//! order, each plane's vertical edges and then its horizontal ones. Filters
//! overlap, so that order decides pixels, and it is followed exactly here --
//! including its quirks, such as a double-width 16-pixel filter using the
//! first half's thresholds for both halves. libvpx delays filtering a
//! superblock row until the next has decoded; filtering the whole frame
//! after it decodes, as here, gives the same result, since no block reads a
//! pixel the filter of a row it has not reached would change.
//!
//! libvpx's high-bit-depth filters are its 8-bit ones with thresholds and
//! clamps scaled by `bit depth - 8`, so one generic copy serves both.
//!
//! Translated into Rust from libvpx v1.17.0's `vp9/common/vp9_loopfilter.c`
//! and `vpx_dsp/loopfilter.c` (copyright the WebM project authors), used under
//! libvpx's BSD licence and patent grant (`licenses/libvpx-LICENSE`,
//! `licenses/libvpx-PATENTS`).

#![allow(
    clippy::arithmetic_side_effects,
    reason = "filter sums are at most 16 samples of 12 bits; positions are within allocated planes; mask shifts are below 64"
)]
#![allow(
    clippy::indexing_slicing,
    reason = "mask tables are indexed by block and transform sizes already bounded to their tables' lengths, and per-superblock arrays by positions below 64; pixels are reached through checked helpers"
)]

use crate::block::{Decoded, MiGrid, ModeInfo, uv_tx_size};
use crate::common::{BLOCK_SIZES, BlockSize, MI_BLOCK_SIZE, TX_4X4, TX_8X8, TX_16X16, TX_32X32};
use crate::frame::{AnyFrame, FrameBuf, Pixel};
use crate::header::{FilterLimits, LevelTable, LimitTable, MODE_LF_LUT};
use crate::tables;

// --- Masks: libvpx's tables ---------------------------------------------------------------

const LEFT_64X64_TXFORM_MASK: [u64; 4] = [
    0xffff_ffff_ffff_ffff,
    0xffff_ffff_ffff_ffff,
    0x5555_5555_5555_5555,
    0x1111_1111_1111_1111,
];
const ABOVE_64X64_TXFORM_MASK: [u64; 4] = [
    0xffff_ffff_ffff_ffff,
    0xffff_ffff_ffff_ffff,
    0x00ff_00ff_00ff_00ff,
    0x0000_00ff_0000_00ff,
];
const LEFT_PREDICTION_MASK: [u64; BLOCK_SIZES] = [
    0x0000_0000_0000_0001,
    0x0000_0000_0000_0001,
    0x0000_0000_0000_0001,
    0x0000_0000_0000_0001,
    0x0000_0000_0000_0101,
    0x0000_0000_0000_0001,
    0x0000_0000_0000_0101,
    0x0000_0000_0101_0101,
    0x0000_0000_0000_0101,
    0x0000_0000_0101_0101,
    0x0101_0101_0101_0101,
    0x0000_0000_0101_0101,
    0x0101_0101_0101_0101,
];
const ABOVE_PREDICTION_MASK: [u64; BLOCK_SIZES] = [
    0x0000_0000_0000_0001,
    0x0000_0000_0000_0001,
    0x0000_0000_0000_0001,
    0x0000_0000_0000_0001,
    0x0000_0000_0000_0001,
    0x0000_0000_0000_0003,
    0x0000_0000_0000_0003,
    0x0000_0000_0000_0003,
    0x0000_0000_0000_000f,
    0x0000_0000_0000_000f,
    0x0000_0000_0000_000f,
    0x0000_0000_0000_00ff,
    0x0000_0000_0000_00ff,
];
const SIZE_MASK: [u64; BLOCK_SIZES] = [
    0x0000_0000_0000_0001,
    0x0000_0000_0000_0001,
    0x0000_0000_0000_0001,
    0x0000_0000_0000_0001,
    0x0000_0000_0000_0101,
    0x0000_0000_0000_0003,
    0x0000_0000_0000_0303,
    0x0000_0000_0303_0303,
    0x0000_0000_0000_0f0f,
    0x0000_0000_0f0f_0f0f,
    0x0f0f_0f0f_0f0f_0f0f,
    0x0000_0000_ffff_ffff,
    0xffff_ffff_ffff_ffff,
];
const LEFT_BORDER: u64 = 0x1111_1111_1111_1111;
const ABOVE_BORDER: u64 = 0x0000_00ff_0000_00ff;

const LEFT_64X64_TXFORM_MASK_UV: [u16; 4] = [0xffff, 0xffff, 0x5555, 0x1111];
const ABOVE_64X64_TXFORM_MASK_UV: [u16; 4] = [0xffff, 0xffff, 0x0f0f, 0x000f];
const LEFT_PREDICTION_MASK_UV: [u16; BLOCK_SIZES] = [
    0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0011, 0x0001, 0x0011, 0x1111, 0x0011,
    0x1111,
];
const ABOVE_PREDICTION_MASK_UV: [u16; BLOCK_SIZES] = [
    0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0003, 0x0003, 0x0003, 0x000f,
    0x000f,
];
const SIZE_MASK_UV: [u16; BLOCK_SIZES] = [
    0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0001, 0x0011, 0x0003, 0x0033, 0x3333, 0x00ff,
    0xffff,
];
const LEFT_BORDER_UV: u16 = 0x1111;
const ABOVE_BORDER_UV: u16 = 0x000f;

/// Which 8x8 cells of a superblock begin a 16x16 area: libvpx's
/// `first_block_in_16x16`, as a test.
fn first_block_in_16x16(row: usize, col: usize) -> bool {
    row & 1 == 0 && col & 1 == 0
}

/// One superblock's edges: libvpx's `LOOP_FILTER_MASK`. Bit `col + 8 * row`
/// of a luma mask is the 8x8 cell at (row, col); bit `col + 4 * row` of a
/// chroma mask the 4:2:0 chroma cell. Indexed by transform size.
#[derive(Clone, Copy, Debug)]
struct Lfm {
    left_y: [u64; 4],
    above_y: [u64; 4],
    int_4x4_y: u64,
    left_uv: [u16; 4],
    above_uv: [u16; 4],
    int_4x4_uv: u16,
    lfl_y: [u8; 64],
}

impl Default for Lfm {
    fn default() -> Self {
        Self {
            left_y: [0; 4],
            above_y: [0; 4],
            int_4x4_y: 0,
            left_uv: [0; 4],
            above_uv: [0; 4],
            int_4x4_uv: 0,
            lfl_y: [0; 64],
        }
    }
}

/// A block's filter level: libvpx's `get_filter_level`.
fn filter_level(levels: &LevelTable, mi: &ModeInfo) -> u8 {
    let mode_class = usize::from(MODE_LF_LUT.get(usize::from(mi.mode)).copied().unwrap_or(0));
    levels
        .get(usize::from(mi.segment_id))
        .and_then(|s| s.get(mi.ref_frame[0].clamp(0, 3) as usize))
        .and_then(|r| r.get(mode_class))
        .copied()
        .unwrap_or(0)
}

/// Add a block's edges to its superblock's masks: libvpx's `vp9_build_mask`.
fn build_mask(lfm: &mut Lfm, levels: &LevelTable, mi: &ModeInfo) {
    let bsize = usize::from(mi.sb_type).min(12);
    let tx_y = usize::from(mi.tx_size).min(3);
    let level = filter_level(levels, mi);
    // libvpx's masks are built for 4:2:0 chroma whatever the subsampling.
    let tx_uv = usize::from(uv_tx_size(mi.sb_type, mi.tx_size, 1, 1)).min(3);
    let row_in_sb = mi.mi_row & 7;
    let col_in_sb = mi.mi_col & 7;
    let shift_y = col_in_sb + (row_in_sb << 3);
    let shift_uv = (col_in_sb >> 1) + ((row_in_sb >> 1) << 2);
    let build_uv = first_block_in_16x16(row_in_sb, col_in_sb);

    if level == 0 {
        return;
    }
    for r in 0..mi.bh {
        let start = shift_y + r * 8;
        for v in lfm.lfl_y.iter_mut().skip(start).take(mi.bw) {
            *v = level;
        }
    }

    lfm.above_y[tx_y] |= ABOVE_PREDICTION_MASK[bsize] << shift_y;
    lfm.left_y[tx_y] |= LEFT_PREDICTION_MASK[bsize] << shift_y;
    if build_uv {
        lfm.above_uv[tx_uv] |= ABOVE_PREDICTION_MASK_UV[bsize] << shift_uv;
        lfm.left_uv[tx_uv] |= LEFT_PREDICTION_MASK_UV[bsize] << shift_uv;
    }

    // A skipped inter block has no transform edges inside it.
    if mi.skip && mi.is_inter() {
        return;
    }

    lfm.above_y[tx_y] |= (SIZE_MASK[bsize] & ABOVE_64X64_TXFORM_MASK[tx_y]) << shift_y;
    lfm.left_y[tx_y] |= (SIZE_MASK[bsize] & LEFT_64X64_TXFORM_MASK[tx_y]) << shift_y;
    if build_uv {
        lfm.above_uv[tx_uv] |=
            (SIZE_MASK_UV[bsize] & ABOVE_64X64_TXFORM_MASK_UV[tx_uv]) << shift_uv;
        lfm.left_uv[tx_uv] |= (SIZE_MASK_UV[bsize] & LEFT_64X64_TXFORM_MASK_UV[tx_uv]) << shift_uv;
    }
    if tx_y == usize::from(TX_4X4) {
        lfm.int_4x4_y |= SIZE_MASK[bsize] << shift_y;
    }
    if build_uv && tx_uv == usize::from(TX_4X4) {
        lfm.int_4x4_uv |= SIZE_MASK_UV[bsize] << shift_uv;
    }
}

/// Fix a superblock's masks for the frame's edges: libvpx's
/// `vp9_adjust_mask`.
fn adjust_mask(lfm: &mut Lfm, mi_row: usize, mi_col: usize, mi_rows: usize, mi_cols: usize) {
    let (t16, t8, t4, t32) = (
        usize::from(TX_16X16),
        usize::from(TX_8X8),
        usize::from(TX_4X4),
        usize::from(TX_32X32),
    );
    // 32x32 transforms filter like 16x16 ones.
    lfm.left_y[t16] |= lfm.left_y[t32];
    lfm.above_y[t16] |= lfm.above_y[t32];
    lfm.left_uv[t16] |= lfm.left_uv[t32];
    lfm.above_uv[t16] |= lfm.above_uv[t32];

    // At least an 8-tap filter on every 32x32 edge, even for 4x4 transforms.
    lfm.left_y[t8] |= lfm.left_y[t4] & LEFT_BORDER;
    lfm.left_y[t4] &= !LEFT_BORDER;
    lfm.above_y[t8] |= lfm.above_y[t4] & ABOVE_BORDER;
    lfm.above_y[t4] &= !ABOVE_BORDER;
    lfm.left_uv[t8] |= lfm.left_uv[t4] & LEFT_BORDER_UV;
    lfm.left_uv[t4] &= !LEFT_BORDER_UV;
    lfm.above_uv[t8] |= lfm.above_uv[t4] & ABOVE_BORDER_UV;
    lfm.above_uv[t4] &= !ABOVE_BORDER_UV;

    if mi_row + MI_BLOCK_SIZE as usize > mi_rows {
        let rows = (mi_rows - mi_row) as u64;
        let mask_y = (1u64 << (rows << 3)) - 1;
        let mask_uv = ((1u32 << (((rows + 1) >> 1) << 2)) - 1) as u16;
        for i in 0..t32 {
            lfm.left_y[i] &= mask_y;
            lfm.above_y[i] &= mask_y;
            lfm.left_uv[i] &= mask_uv;
            lfm.above_uv[i] &= mask_uv;
        }
        lfm.int_4x4_y &= mask_y;
        lfm.int_4x4_uv &= mask_uv;
        // No wide filter on the last chroma row.
        if rows == 1 {
            lfm.above_uv[t8] |= lfm.above_uv[t16];
            lfm.above_uv[t16] = 0;
        }
        if rows == 5 {
            lfm.above_uv[t8] |= lfm.above_uv[t16] & 0xff00;
            lfm.above_uv[t16] &= !(lfm.above_uv[t16] & 0xff00);
        }
    }

    if mi_col + MI_BLOCK_SIZE as usize > mi_cols {
        let columns = (mi_cols - mi_col) as u64;
        let mask_y = ((1u64 << columns) - 1) * 0x0101_0101_0101_0101;
        let mask_uv = (((1u32 << ((columns + 1) >> 1)) - 1) * 0x1111) as u16;
        // Internal edges stop one column sooner.
        let mask_uv_int = (((1u32 << (columns >> 1)) - 1) * 0x1111) as u16;
        for i in 0..t32 {
            lfm.left_y[i] &= mask_y;
            lfm.above_y[i] &= mask_y;
            lfm.left_uv[i] &= mask_uv;
            lfm.above_uv[i] &= mask_uv;
        }
        lfm.int_4x4_y &= mask_y;
        lfm.int_4x4_uv &= mask_uv_int;
        // No wide filter on the last chroma column.
        if columns == 1 {
            lfm.left_uv[t8] |= lfm.left_uv[t16];
            lfm.left_uv[t16] = 0;
        }
        if columns == 5 {
            lfm.left_uv[t8] |= lfm.left_uv[t16] & 0xcccc;
            lfm.left_uv[t16] &= !(lfm.left_uv[t16] & 0xcccc);
        }
    }

    // Nothing is filtered on the frame's left edge.
    if mi_col == 0 {
        for i in 0..t32 {
            lfm.left_y[i] &= 0xfefe_fefe_fefe_fefe;
            lfm.left_uv[i] &= 0xeeee;
        }
    }
}

// --- Filtering a frame -------------------------------------------------------------------

/// Filter a decoded frame: libvpx's `loop_filter_rows` over every
/// superblock row.
pub(crate) fn filter_frame(
    frame: &mut AnyFrame,
    decoded: &Decoded,
    levels: &LevelTable,
    limits: &LimitTable,
) {
    match frame {
        AnyFrame::Eight(f) => filter_frame_t(f, &decoded.mi, levels, limits),
        AnyFrame::High(f) => filter_frame_t(f, &decoded.mi, levels, limits),
    }
}

fn filter_frame_t<P: Pixel>(
    frame: &mut FrameBuf<P>,
    mi: &MiGrid,
    levels: &LevelTable,
    limits: &LimitTable,
) {
    let (mi_rows, mi_cols) = (mi.mi_rows, mi.mi_cols);
    let sb_cols = mi_cols.div_ceil(8);
    let sb_rows = mi_rows.div_ceil(8);
    let mut lfms = vec![Lfm::default(); sb_cols * sb_rows];
    for b in &mi.blocks {
        if let Some(lfm) = lfms.get_mut((b.mi_row >> 3) * sb_cols + (b.mi_col >> 3)) {
            build_mask(lfm, levels, b);
        }
    }
    let path = match (frame.ss_x, frame.ss_y) {
        (1, 1) => Path::Ss11,
        (0, 0) => Path::Ss00,
        _ => Path::NonSs11,
    };
    let bd = frame.bit_depth;
    let shift = u32::from(bd.clamp(8, 12) - 8);
    for sb_row in 0..sb_rows {
        for sb_col in 0..sb_cols {
            let mi_row = sb_row * 8;
            let mi_col = sb_col * 8;
            let lfm = &mut lfms[sb_row * sb_cols + sb_col];
            adjust_mask(lfm, mi_row, mi_col, mi_rows, mi_cols);
            let lfm = *lfm;
            let ctx = Ctx {
                limits,
                shift,
                mi_row,
                mi_rows,
            };
            {
                let p = &mut frame.planes[0];
                let origin = mi_row * 8 * p.stride + mi_col * 8;
                filter_block_plane_ss00(&ctx, &mut p.data, origin, p.stride, &lfm);
            }
            for plane in 1..3 {
                let (ss_x, ss_y) = (usize::from(frame.ss_x), usize::from(frame.ss_y));
                let p = &mut frame.planes[plane];
                let origin = ((mi_row * 8) >> ss_y) * p.stride + ((mi_col * 8) >> ss_x);
                match path {
                    Path::Ss11 => {
                        filter_block_plane_ss11(&ctx, &mut p.data, origin, p.stride, &lfm);
                    }
                    Path::Ss00 => {
                        filter_block_plane_ss00(&ctx, &mut p.data, origin, p.stride, &lfm);
                    }
                    Path::NonSs11 => filter_block_plane_non420(
                        &ctx,
                        &mut p.data,
                        origin,
                        p.stride,
                        mi,
                        levels,
                        mi_col,
                        frame_ss(ss_x, ss_y),
                    ),
                }
            }
        }
    }
}

fn frame_ss(ss_x: usize, ss_y: usize) -> (u32, u32) {
    (ss_x as u32, ss_y as u32)
}

/// Which chroma filtering a frame uses: libvpx's `lf_path`.
#[derive(Clone, Copy)]
enum Path {
    Ss11,
    Ss00,
    NonSs11,
}

/// What every filter call of a superblock shares.
struct Ctx<'a> {
    limits: &'a LimitTable,
    /// `bit depth - 8`: how far thresholds and clamps scale.
    shift: u32,
    mi_row: usize,
    mi_rows: usize,
}

impl Ctx<'_> {
    fn lim(&self, level: u8) -> FilterLimits {
        self.limits
            .get(usize::from(level))
            .copied()
            .unwrap_or_default()
    }
}

/// libvpx's `vp9_filter_block_plane_ss00`: a full-resolution plane.
fn filter_block_plane_ss00<P: Pixel>(
    ctx: &Ctx<'_>,
    s: &mut [P],
    origin: usize,
    stride: usize,
    lfm: &Lfm,
) {
    // Vertical edges, two rows of cells at a time.
    let mut m16 = lfm.left_y[usize::from(TX_16X16)];
    let mut m8 = lfm.left_y[usize::from(TX_8X8)];
    let mut m4 = lfm.left_y[usize::from(TX_4X4)];
    let mut m4i = lfm.int_4x4_y;
    let mut r = 0;
    while r < 8 && ctx.mi_row + r < ctx.mi_rows {
        let at = origin + r * 8 * stride;
        filter_selectively_vert_row2(
            ctx,
            false,
            s,
            at,
            stride,
            m16 as u32,
            m8 as u32,
            m4 as u32,
            m4i as u32,
            &lfm.lfl_y[r << 3..],
        );
        m16 >>= 16;
        m8 >>= 16;
        m4 >>= 16;
        m4i >>= 16;
        r += 2;
    }
    // Horizontal edges.
    let mut m16 = lfm.above_y[usize::from(TX_16X16)];
    let mut m8 = lfm.above_y[usize::from(TX_8X8)];
    let mut m4 = lfm.above_y[usize::from(TX_4X4)];
    let mut m4i = lfm.int_4x4_y;
    let mut r = 0;
    while r < 8 && ctx.mi_row + r < ctx.mi_rows {
        let (a16, a8, a4) = if ctx.mi_row + r == 0 {
            (0, 0, 0)
        } else {
            ((m16 & 0xff) as u32, (m8 & 0xff) as u32, (m4 & 0xff) as u32)
        };
        let at = origin + r * 8 * stride;
        filter_selectively_horiz(
            ctx,
            s,
            at,
            stride,
            a16,
            a8,
            a4,
            (m4i & 0xff) as u32,
            &lfm.lfl_y[r << 3..],
        );
        m16 >>= 8;
        m8 >>= 8;
        m4 >>= 8;
        m4i >>= 8;
        r += 1;
    }
}

/// libvpx's `vp9_filter_block_plane_ss11`: a 4:2:0 chroma plane.
fn filter_block_plane_ss11<P: Pixel>(
    ctx: &Ctx<'_>,
    s: &mut [P],
    origin: usize,
    stride: usize,
    lfm: &Lfm,
) {
    let mut lfl_uv = [0u8; 16];
    let mut m16 = lfm.left_uv[usize::from(TX_16X16)];
    let mut m8 = lfm.left_uv[usize::from(TX_8X8)];
    let mut m4 = lfm.left_uv[usize::from(TX_4X4)];
    let mut m4i = lfm.int_4x4_uv;
    // Vertical edges, two rows of chroma cells at a time.
    let mut r = 0;
    while r < 8 && ctx.mi_row + r < ctx.mi_rows {
        for c in 0..4 {
            lfl_uv[(r << 1) + c] = lfm.lfl_y[(r << 3) + (c << 1)];
            lfl_uv[((r + 2) << 1) + c] = lfm.lfl_y[((r + 2) << 3) + (c << 1)];
        }
        let at = origin + (r >> 1) * 8 * stride;
        filter_selectively_vert_row2(
            ctx,
            true,
            s,
            at,
            stride,
            u32::from(m16),
            u32::from(m8),
            u32::from(m4),
            u32::from(m4i),
            &lfl_uv[r << 1..],
        );
        m16 >>= 8;
        m8 >>= 8;
        m4 >>= 8;
        m4i >>= 8;
        r += 4;
    }
    // Horizontal edges.
    let mut m16 = lfm.above_uv[usize::from(TX_16X16)];
    let mut m8 = lfm.above_uv[usize::from(TX_8X8)];
    let mut m4 = lfm.above_uv[usize::from(TX_4X4)];
    let mut m4i = lfm.int_4x4_uv;
    let mut r = 0;
    while r < 8 && ctx.mi_row + r < ctx.mi_rows {
        let skip_border_4x4_r = ctx.mi_row + r == ctx.mi_rows - 1;
        let m4i_r = if skip_border_4x4_r {
            0
        } else {
            u32::from(m4i & 0xf)
        };
        let (a16, a8, a4) = if ctx.mi_row + r == 0 {
            (0, 0, 0)
        } else {
            (
                u32::from(m16 & 0xf),
                u32::from(m8 & 0xf),
                u32::from(m4 & 0xf),
            )
        };
        let at = origin + (r >> 1) * 8 * stride;
        filter_selectively_horiz(ctx, s, at, stride, a16, a8, a4, m4i_r, &lfl_uv[r << 1..]);
        m16 >>= 4;
        m8 >>= 4;
        m4 >>= 4;
        m4i >>= 4;
        r += 2;
    }
}

/// libvpx's `vp9_filter_block_plane_non420`: a chroma plane subsampled one
/// way only, filtered from the blocks themselves rather than the masks.
#[allow(clippy::too_many_arguments)]
fn filter_block_plane_non420<P: Pixel>(
    ctx: &Ctx<'_>,
    s: &mut [P],
    origin: usize,
    stride: usize,
    mi: &MiGrid,
    levels: &LevelTable,
    mi_col: usize,
    (ss_x, ss_y): (u32, u32),
) {
    let mi_row = ctx.mi_row;
    let row_step = 1usize << ss_y;
    let col_step = 1usize << ss_x;
    let mut mask_16x16 = [0u32; 8];
    let mut mask_8x8 = [0u32; 8];
    let mut mask_4x4 = [0u32; 8];
    let mut mask_4x4_int = [0u32; 8];
    let mut lfl = [0u8; 64];
    let num_4x4 =
        |t: &[u8; 13], b: BlockSize| usize::from(t.get(usize::from(b)).copied().unwrap_or(1));

    let mut r = 0;
    while r < 8 && mi_row + r < ctx.mi_rows {
        let (mut m16_c, mut m8_c, mut m4_c) = (0u32, 0u32, 0u32);
        let mut c = 0;
        while c < 8 && mi_col + c < mi.mi_cols {
            let Some(b) = mi.at(mi_row + r, mi_col + c) else {
                c += col_step;
                continue;
            };
            let sb_type = b.sb_type;
            let skip_this = b.skip && b.is_inter();
            let block_edge_left = if num_4x4(&tables::NUM_4X4_WIDE, sb_type) > 1 {
                c & (num_4x4(&tables::NUM_8X8_WIDE, sb_type).max(1) - 1) == 0
            } else {
                true
            };
            let skip_this_c = skip_this && !block_edge_left;
            let block_edge_above = if num_4x4(&tables::NUM_4X4_HIGH, sb_type) > 1 {
                r & (num_4x4(&tables::NUM_8X8_HIGH, sb_type).max(1) - 1) == 0
            } else {
                true
            };
            let skip_this_r = skip_this && !block_edge_above;
            let tx_size = uv_tx_size(sb_type, b.tx_size, ss_x as u8, ss_y as u8);
            let skip_border_4x4_c = ss_x != 0 && mi_col + c == mi.mi_cols - 1;
            let skip_border_4x4_r = ss_y != 0 && mi_row + r == ctx.mi_rows - 1;
            let level = filter_level(levels, b);
            lfl[(r << 3) + (c >> ss_x)] = level;
            if level != 0 {
                let bit = 1u32 << (c >> ss_x);
                if tx_size == TX_32X32 {
                    if !skip_this_c && ((c >> ss_x) & 3) == 0 {
                        if skip_border_4x4_c {
                            m8_c |= bit;
                        } else {
                            m16_c |= bit;
                        }
                    }
                    if !skip_this_r && ((r >> ss_y) & 3) == 0 {
                        if skip_border_4x4_r {
                            mask_8x8[r] |= bit;
                        } else {
                            mask_16x16[r] |= bit;
                        }
                    }
                } else if tx_size == TX_16X16 {
                    if !skip_this_c && ((c >> ss_x) & 1) == 0 {
                        if skip_border_4x4_c {
                            m8_c |= bit;
                        } else {
                            m16_c |= bit;
                        }
                    }
                    if !skip_this_r && ((r >> ss_y) & 1) == 0 {
                        if skip_border_4x4_r {
                            mask_8x8[r] |= bit;
                        } else {
                            mask_16x16[r] |= bit;
                        }
                    }
                } else {
                    // An 8-tap filter on every 32x32 edge.
                    if !skip_this_c {
                        if tx_size == TX_8X8 || ((c >> ss_x) & 3) == 0 {
                            m8_c |= bit;
                        } else {
                            m4_c |= bit;
                        }
                    }
                    if !skip_this_r {
                        if tx_size == TX_8X8 || ((r >> ss_y) & 3) == 0 {
                            mask_8x8[r] |= bit;
                        } else {
                            mask_4x4[r] |= bit;
                        }
                    }
                    if !skip_this && tx_size < TX_8X8 && !skip_border_4x4_c {
                        mask_4x4_int[r] |= bit;
                    }
                }
            }
            c += col_step;
        }
        // Nothing on the frame's left edge.
        let border_mask = if mi_col == 0 { !1u32 } else { !0u32 };
        let at = origin + (r >> ss_y) * 8 * stride;
        filter_selectively_vert(
            ctx,
            s,
            at,
            stride,
            m16_c & border_mask,
            m8_c & border_mask,
            m4_c & border_mask,
            mask_4x4_int[r],
            &lfl[r << 3..],
        );
        r += row_step;
    }

    let mut r = 0;
    while r < 8 && mi_row + r < ctx.mi_rows {
        let skip_border_4x4_r = ss_y != 0 && mi_row + r == ctx.mi_rows - 1;
        let m4i_r = if skip_border_4x4_r {
            0
        } else {
            mask_4x4_int[r]
        };
        let (a16, a8, a4) = if mi_row + r == 0 {
            (0, 0, 0)
        } else {
            (mask_16x16[r], mask_8x8[r], mask_4x4[r])
        };
        let at = origin + (r >> ss_y) * 8 * stride;
        filter_selectively_horiz(ctx, s, at, stride, a16, a8, a4, m4i_r, &lfl[r << 3..]);
        r += row_step;
    }
}

/// libvpx's `filter_selectively_vert_row2`: the vertical edges of two rows
/// of cells -- eight pixel rows apart -- column by column.
#[allow(clippy::too_many_arguments)]
fn filter_selectively_vert_row2<P: Pixel>(
    ctx: &Ctx<'_>,
    subsampled: bool,
    s: &mut [P],
    origin: usize,
    stride: usize,
    mut m16: u32,
    mut m8: u32,
    mut m4: u32,
    mut m4i: u32,
    lfl: &[u8],
) {
    let cutoff = if subsampled { 0xff } else { 0xffff };
    let forward = if subsampled { 4 } else { 8 };
    let dual_one = 1u32 | (1 << forward);
    let mut at = origin;
    let mut lfl_i = 0usize;
    let mut mask = (m16 | m8 | m4 | m4i) & cutoff;
    while mask != 0 {
        if mask & dual_one != 0 {
            let l0 = ctx.lim(lfl.get(lfl_i).copied().unwrap_or(0));
            let l1 = ctx.lim(lfl.get(lfl_i + forward).copied().unwrap_or(0));
            let ss = [at, at + 8 * stride];
            let lims = [l0, l1];
            if m16 & dual_one != 0 {
                if m16 & dual_one == dual_one {
                    // Both rows, with the first's thresholds: libvpx's
                    // vpx_lpf_vertical_16_dual.
                    lpf_vertical_16(ctx, s, ss[0], stride, &l0, 16);
                } else {
                    let k = usize::from(m16 & 1 == 0);
                    lpf_vertical_16(ctx, s, ss[k], stride, &lims[k], 8);
                }
            }
            if m8 & dual_one != 0 {
                for k in 0..2 {
                    if m8 & (1 << (k * forward)) != 0 {
                        lpf_vertical_8(ctx, s, ss[k], stride, &lims[k]);
                    }
                }
            }
            if m4 & dual_one != 0 {
                for k in 0..2 {
                    if m4 & (1 << (k * forward)) != 0 {
                        lpf_vertical_4(ctx, s, ss[k], stride, &lims[k]);
                    }
                }
            }
            if m4i & dual_one != 0 {
                for k in 0..2 {
                    if m4i & (1 << (k * forward)) != 0 {
                        lpf_vertical_4(ctx, s, ss[k] + 4, stride, &lims[k]);
                    }
                }
            }
        }
        at += 8;
        lfl_i += 1;
        m16 >>= 1;
        m8 >>= 1;
        m4 >>= 1;
        m4i >>= 1;
        mask = (mask & !dual_one) >> 1;
    }
}

/// libvpx's `filter_selectively_vert`: one row of cells' vertical edges.
#[allow(clippy::too_many_arguments)]
fn filter_selectively_vert<P: Pixel>(
    ctx: &Ctx<'_>,
    s: &mut [P],
    origin: usize,
    stride: usize,
    mut m16: u32,
    mut m8: u32,
    mut m4: u32,
    mut m4i: u32,
    lfl: &[u8],
) {
    let mut at = origin;
    let mut lfl_i = 0usize;
    let mut mask = m16 | m8 | m4 | m4i;
    while mask != 0 {
        let lim = ctx.lim(lfl.get(lfl_i).copied().unwrap_or(0));
        if mask & 1 != 0 {
            if m16 & 1 != 0 {
                lpf_vertical_16(ctx, s, at, stride, &lim, 8);
            } else if m8 & 1 != 0 {
                lpf_vertical_8(ctx, s, at, stride, &lim);
            } else if m4 & 1 != 0 {
                lpf_vertical_4(ctx, s, at, stride, &lim);
            }
        }
        if m4i & 1 != 0 {
            lpf_vertical_4(ctx, s, at + 4, stride, &lim);
        }
        at += 8;
        lfl_i += 1;
        m16 >>= 1;
        m8 >>= 1;
        m4 >>= 1;
        m4i >>= 1;
        mask >>= 1;
    }
}

/// libvpx's `filter_selectively_horiz`: one row of cells' horizontal
/// edges, column by column, two at a time where libvpx pairs them.
#[allow(clippy::too_many_arguments)]
fn filter_selectively_horiz<P: Pixel>(
    ctx: &Ctx<'_>,
    s: &mut [P],
    origin: usize,
    stride: usize,
    mut m16: u32,
    mut m8: u32,
    mut m4: u32,
    mut m4i: u32,
    lfl: &[u8],
) {
    let mut at = origin;
    let mut lfl_i = 0usize;
    let mut mask = m16 | m8 | m4 | m4i;
    while mask != 0 {
        let mut count = 1u32;
        if mask & 1 != 0 {
            let lfi = ctx.lim(lfl.get(lfl_i).copied().unwrap_or(0));
            let lfin = || ctx.lim(lfl.get(lfl_i + 1).copied().unwrap_or(0));
            let internal = |s: &mut [P], lfi: &FilterLimits, lfin: &FilterLimits, m4i: u32| {
                if m4i & 3 == 3 {
                    lpf_horizontal_4(ctx, s, at + 4 * stride, stride, lfi, 8);
                    lpf_horizontal_4(ctx, s, at + 8 + 4 * stride, stride, lfin, 8);
                } else if m4i & 1 != 0 {
                    lpf_horizontal_4(ctx, s, at + 4 * stride, stride, lfi, 8);
                } else if m4i & 2 != 0 {
                    lpf_horizontal_4(ctx, s, at + 8 + 4 * stride, stride, lfin, 8);
                }
            };
            if m16 & 1 != 0 {
                if m16 & 3 == 3 {
                    // Two columns with the first's thresholds: libvpx's
                    // vpx_lpf_horizontal_16_dual.
                    lpf_horizontal_16(ctx, s, at, stride, &lfi, 16);
                    count = 2;
                } else {
                    lpf_horizontal_16(ctx, s, at, stride, &lfi, 8);
                }
            } else if m8 & 1 != 0 {
                if m8 & 3 == 3 {
                    let n = lfin();
                    lpf_horizontal_8(ctx, s, at, stride, &lfi);
                    lpf_horizontal_8(ctx, s, at + 8, stride, &n);
                    internal(s, &lfi, &n, m4i);
                    count = 2;
                } else {
                    lpf_horizontal_8(ctx, s, at, stride, &lfi);
                    if m4i & 1 != 0 {
                        lpf_horizontal_4(ctx, s, at + 4 * stride, stride, &lfi, 8);
                    }
                }
            } else if m4 & 1 != 0 {
                if m4 & 3 == 3 {
                    let n = lfin();
                    lpf_horizontal_4(ctx, s, at, stride, &lfi, 8);
                    lpf_horizontal_4(ctx, s, at + 8, stride, &n, 8);
                    internal(s, &lfi, &n, m4i);
                    count = 2;
                } else {
                    lpf_horizontal_4(ctx, s, at, stride, &lfi, 8);
                    if m4i & 1 != 0 {
                        lpf_horizontal_4(ctx, s, at + 4 * stride, stride, &lfi, 8);
                    }
                }
            } else {
                lpf_horizontal_4(ctx, s, at + 4 * stride, stride, &lfi, 8);
            }
        }
        at += 8 * count as usize;
        lfl_i += count as usize;
        m16 >>= count;
        m8 >>= count;
        m4 >>= count;
        m4i >>= count;
        mask >>= count;
    }
}

// --- The filters: libvpx's vpx_dsp/loopfilter.c ---------------------------------------------
//
// libvpx's C filters one line across an edge at a time, deciding per line
// whether to filter and how widely. Here eight lines go together: every
// decision is taken for all eight and applied as a choice per lane, which
// the compiler turns into vector operations. A lane whose mask is off writes
// back what it read, so the results are the C's.

/// Eight lines across an edge, side by side: `v[k][j]` is sample `k` of line
/// `j`, in order across the edge -- `p7` to `p0` then `q0` to `q7` for the
/// 16-wide filter, `p3` to `q3` for the others.
type Lanes<const K: usize> = [[i32; 8]; K];

/// One filter call's thresholds, scaled to the bit depth.
#[derive(Clone, Copy)]
struct Thresholds {
    limit: i32,
    blimit: i32,
    hev: i32,
    /// The flatness threshold: 1, scaled.
    flat: i32,
    /// 128, scaled: what makes a sample signed for the narrow filter.
    off: i32,
}

impl Thresholds {
    fn new(l: &FilterLimits, shift: u32) -> Self {
        Self {
            limit: i32::from(l.lim) << shift,
            blimit: i32::from(l.mblim) << shift,
            hev: i32::from(l.hev_thr) << shift,
            flat: 1 << shift,
            off: 128 << shift,
        }
    }
}

/// libvpx's `filter_mask`: whether a line is filtered at all.
#[inline(always)]
fn filter_mask(t: &Thresholds, [p3, p2, p1, p0, q0, q1, q2, q3]: [i32; 8]) -> bool {
    (p3 - p2).abs() <= t.limit
        && (p2 - p1).abs() <= t.limit
        && (p1 - p0).abs() <= t.limit
        && (q1 - q0).abs() <= t.limit
        && (q2 - q1).abs() <= t.limit
        && (q3 - q2).abs() <= t.limit
        && (p0 - q0).abs() * 2 + (p1 - q1).abs() / 2 <= t.blimit
}

/// libvpx's `flat_mask4`: `p3..p1` within the flatness threshold of `p0`,
/// and `q1..q3` of `q0`. Given the outer samples in place of the inner
/// ones, the larger part of `flat_mask5`.
#[inline(always)]
fn flat_mask4(t: &Thresholds, [p3, p2, p1, p0, q0, q1, q2, q3]: [i32; 8]) -> bool {
    (p1 - p0).abs() <= t.flat
        && (q1 - q0).abs() <= t.flat
        && (p2 - p0).abs() <= t.flat
        && (q2 - q0).abs() <= t.flat
        && (p3 - p0).abs() <= t.flat
        && (q3 - q0).abs() <= t.flat
}

/// libvpx's `filter4` on `p1 p0 q0 q1`, as new values for them; with the
/// mask off, the values it was given.
#[inline(always)]
fn filter4(t: &Thresholds, mask: bool, [p1, p0, q0, q1]: [i32; 4]) -> [i32; 4] {
    let (lo, hi) = (-t.off, t.off - 1);
    let sc = |x: i32| x.clamp(lo, hi);
    let (ps1, ps0, qs0, qs1) = (p1 - t.off, p0 - t.off, q0 - t.off, q1 - t.off);
    let hev = (p1 - p0).abs() > t.hev || (q1 - q0).abs() > t.hev;
    // The outer taps only where the edge varies a lot.
    let f = if hev { sc(ps1 - qs1) } else { 0 };
    let f = if mask { sc(f + 3 * (qs0 - ps0)) } else { 0 };
    // One side rounded up, the other down.
    let f1 = sc(f + 4) >> 3;
    let f2 = sc(f + 3) >> 3;
    let outer = if hev { 0 } else { (f1 + 1) >> 1 };
    [
        sc(ps1 + outer) + t.off,
        sc(ps0 + f2) + t.off,
        sc(qs0 - f1) + t.off,
        sc(qs1 - outer) + t.off,
    ]
}

/// The flat 7-tap filter of libvpx's `filter8`: new `p2..q2`.
#[inline(always)]
fn flat8([p3, p2, p1, p0, q0, q1, q2, q3]: [i32; 8]) -> [i32; 6] {
    let r = |x: i32| (x + 4) >> 3;
    [
        r(3 * p3 + 2 * p2 + p1 + p0 + q0),
        r(2 * p3 + p2 + 2 * p1 + p0 + q0 + q1),
        r(p3 + p2 + p1 + 2 * p0 + q0 + q1 + q2),
        r(p2 + p1 + p0 + 2 * q0 + q1 + q2 + q3),
        r(p1 + p0 + q0 + 2 * q1 + q2 + 2 * q3),
        r(p0 + q0 + q1 + 2 * q2 + 3 * q3),
    ]
}

/// The flat 15-tap filter of libvpx's `filter16`: new `p6..q6`. Output `i`
/// is the fifteen samples centred on it -- the end samples repeated past the
/// line's ends -- plus itself once more, so a running sum computes them all.
#[inline(always)]
fn flat16(s: &[i32; 16]) -> [i32; 14] {
    let at = |m: isize| s[m.clamp(0, 15) as usize];
    let mut sum: i32 = (-6..=8).map(at).sum();
    let mut out = [0; 14];
    for (i, o) in (1..15isize).zip(out.iter_mut()) {
        *o = (sum + at(i) + 8) >> 4;
        sum += at(i + 8) - at(i - 7);
    }
    out
}

/// libvpx's `vpx_lpf_*_4`, eight lines at once: `p3..q3` in, `p1..q1` out.
#[inline(always)]
fn lanes4(v: &mut Lanes<8>, t: &Thresholds) {
    let mut out = *v;
    for j in 0..8 {
        let s: [i32; 8] = core::array::from_fn(|k| v[k][j]);
        let f4 = filter4(t, filter_mask(t, s), [s[2], s[3], s[4], s[5]]);
        for (k, &x) in (2..6).zip(&f4) {
            out[k][j] = x;
        }
    }
    *v = out;
}

/// libvpx's `vpx_lpf_*_8`, eight lines at once: `p3..q3` in, `p2..q2` out.
#[inline(always)]
fn lanes8(v: &mut Lanes<8>, t: &Thresholds) {
    let mut out = *v;
    for j in 0..8 {
        let s: [i32; 8] = core::array::from_fn(|k| v[k][j]);
        let mask = filter_mask(t, s);
        let flat = mask && flat_mask4(t, s);
        let f4 = filter4(t, mask, [s[2], s[3], s[4], s[5]]);
        let f8 = flat8(s);
        for k in 1..7 {
            let narrow = if (2..6).contains(&k) { f4[k - 2] } else { s[k] };
            out[k][j] = if flat { f8[k - 1] } else { narrow };
        }
    }
    *v = out;
}

/// libvpx's `vpx_lpf_*_16`, eight lines at once: `p7..q7` in, `p6..q6` out.
#[inline(always)]
fn lanes16(v: &mut Lanes<16>, t: &Thresholds) {
    let mut out = *v;
    for j in 0..8 {
        let s: [i32; 16] = core::array::from_fn(|k| v[k][j]);
        let inner: [i32; 8] = core::array::from_fn(|k| s[k + 4]);
        let mask = filter_mask(t, inner);
        let flat = mask && flat_mask4(t, inner);
        // libvpx's flat_mask5: p7..p4 and q4..q7 against p0 and q0 too.
        let outer = [s[1], s[2], s[3], s[7], s[8], s[12], s[13], s[14]];
        let flat2 = flat
            && flat_mask4(t, outer)
            && (s[0] - s[7]).abs() <= t.flat
            && (s[15] - s[8]).abs() <= t.flat;
        let f4 = filter4(t, mask, [s[6], s[7], s[8], s[9]]);
        let f8 = flat8(inner);
        let f16 = flat16(&s);
        for k in 1..15 {
            let narrow = if (6..10).contains(&k) {
                f4[k - 6]
            } else {
                s[k]
            };
            let eight = if (5..11).contains(&k) {
                f8[k - 5]
            } else {
                s[k]
            };
            out[k][j] = if flat2 {
                f16[k - 1]
            } else if flat {
                eight
            } else {
                narrow
            };
        }
    }
    *v = out;
}

/// The eight lines across a horizontal edge at `at` (the first sample below
/// it), `K / 2` rows above and below: column `j` is line `j`. `None` if a row
/// leaves the plane, which the geometry never asks for.
#[inline(always)]
fn load_h<P: Pixel, const K: usize>(s: &[P], at: usize, stride: usize) -> Option<Lanes<K>> {
    let top = at.checked_sub(K / 2 * stride)?;
    let mut v = [[0; 8]; K];
    for (k, lane) in v.iter_mut().enumerate() {
        let row = s.get(top + k * stride..)?.get(..8)?;
        for (x, &p) in lane.iter_mut().zip(row) {
            *x = p.int();
        }
    }
    Some(v)
}

/// Write samples `from..to` of each line back across a horizontal edge.
#[inline(always)]
fn store_h<P: Pixel, const K: usize>(
    s: &mut [P],
    at: usize,
    stride: usize,
    v: &Lanes<K>,
    (from, to): (usize, usize),
) {
    let Some(top) = at.checked_sub(K / 2 * stride) else {
        return;
    };
    for (k, lane) in v.iter().enumerate().take(to).skip(from) {
        let Some(row) = s.get_mut(top + k * stride..).and_then(|r| r.get_mut(..8)) else {
            continue;
        };
        for (p, &x) in row.iter_mut().zip(lane) {
            *p = P::from_int(x);
        }
    }
}

/// The eight lines across a vertical edge at `at` (the first sample right of
/// it): rows, `K / 2` samples either side. `None` if one leaves the plane.
#[inline(always)]
fn load_v<P: Pixel, const K: usize>(s: &[P], at: usize, stride: usize) -> Option<Lanes<K>> {
    let left = at.checked_sub(K / 2)?;
    let mut v = [[0; 8]; K];
    for j in 0..8 {
        let line = s.get(left + j * stride..)?.get(..K)?;
        for (lane, &p) in v.iter_mut().zip(line) {
            lane[j] = p.int();
        }
    }
    Some(v)
}

/// Write samples `from..to` of each line back across a vertical edge.
#[inline(always)]
fn store_v<P: Pixel, const K: usize>(
    s: &mut [P],
    at: usize,
    stride: usize,
    v: &Lanes<K>,
    (from, to): (usize, usize),
) {
    let Some(left) = at.checked_sub(K / 2) else {
        return;
    };
    for j in 0..8 {
        let Some(line) = s.get_mut(left + j * stride..).and_then(|r| r.get_mut(..K)) else {
            continue;
        };
        for (p, lane) in line.iter_mut().zip(v).take(to).skip(from) {
            *p = P::from_int(lane[j]);
        }
    }
}

/// Run `filter` on `count` lines (a multiple of eight) across an edge at
/// `at`: a vertical edge if `across` is 1 (the lines are rows, `along` the
/// stride), else a horizontal one (`across` the stride, the lines columns).
/// `written` is the range of samples the filter may change.
#[allow(clippy::too_many_arguments)]
#[inline(always)]
fn lpf<P: Pixel, const K: usize>(
    s: &mut [P],
    at: usize,
    across: usize,
    along: usize,
    count: usize,
    written: (usize, usize),
    t: &Thresholds,
    filter: impl Fn(&mut Lanes<K>, &Thresholds),
) {
    for g in 0..count / 8 {
        let at = at + g * 8 * along;
        if across == 1 {
            if let Some(mut v) = load_v::<P, K>(s, at, along) {
                filter(&mut v, t);
                store_v(s, at, along, &v, written);
            }
        } else if let Some(mut v) = load_h::<P, K>(s, at, across) {
            filter(&mut v, t);
            store_h(s, at, across, &v, written);
        }
    }
}

/// libvpx's `vpx_lpf_*_4` along `count` pixels of an edge at `at`.
fn lpf_4<P: Pixel>(
    ctx: &Ctx<'_>,
    s: &mut [P],
    at: usize,
    across: usize,
    along: usize,
    l: &FilterLimits,
    count: usize,
) {
    let t = Thresholds::new(l, ctx.shift);
    lpf::<P, 8>(s, at, across, along, count, (2, 6), &t, lanes4);
}

/// libvpx's `vpx_lpf_*_8` along 8 pixels of an edge at `at`.
fn lpf_8<P: Pixel>(
    ctx: &Ctx<'_>,
    s: &mut [P],
    at: usize,
    across: usize,
    along: usize,
    l: &FilterLimits,
) {
    let t = Thresholds::new(l, ctx.shift);
    lpf::<P, 8>(s, at, across, along, 8, (1, 7), &t, lanes8);
}

/// libvpx's `mb_lpf_*_edge_w`: the 16-wide filter along `count` pixels.
fn lpf_16<P: Pixel>(
    ctx: &Ctx<'_>,
    s: &mut [P],
    at: usize,
    across: usize,
    along: usize,
    l: &FilterLimits,
    count: usize,
) {
    let t = Thresholds::new(l, ctx.shift);
    lpf::<P, 16>(s, at, across, along, count, (1, 15), &t, lanes16);
}

fn lpf_vertical_4<P: Pixel>(
    ctx: &Ctx<'_>,
    s: &mut [P],
    at: usize,
    stride: usize,
    l: &FilterLimits,
) {
    lpf_4(ctx, s, at, 1, stride, l, 8);
}

fn lpf_vertical_8<P: Pixel>(
    ctx: &Ctx<'_>,
    s: &mut [P],
    at: usize,
    stride: usize,
    l: &FilterLimits,
) {
    lpf_8(ctx, s, at, 1, stride, l);
}

fn lpf_vertical_16<P: Pixel>(
    ctx: &Ctx<'_>,
    s: &mut [P],
    at: usize,
    stride: usize,
    l: &FilterLimits,
    rows: usize,
) {
    lpf_16(ctx, s, at, 1, stride, l, rows);
}

fn lpf_horizontal_4<P: Pixel>(
    ctx: &Ctx<'_>,
    s: &mut [P],
    at: usize,
    stride: usize,
    l: &FilterLimits,
    cols: usize,
) {
    lpf_4(ctx, s, at, stride, 1, l, cols);
}

fn lpf_horizontal_8<P: Pixel>(
    ctx: &Ctx<'_>,
    s: &mut [P],
    at: usize,
    stride: usize,
    l: &FilterLimits,
) {
    lpf_8(ctx, s, at, stride, 1, l);
}

fn lpf_horizontal_16<P: Pixel>(
    ctx: &Ctx<'_>,
    s: &mut [P],
    at: usize,
    stride: usize,
    l: &FilterLimits,
    cols: usize,
) {
    lpf_16(ctx, s, at, stride, 1, l, cols);
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "a test: a failure should be loud")]

    use super::*;

    /// libvpx's `vpx_dsp/loopfilter.c`, one line at a time as its C does it:
    /// the reference the eight-lane filters are checked against.
    mod reference {
        use super::super::Thresholds;

        fn mask(t: &Thresholds, s: &[i32]) -> bool {
            let [p3, p2, p1, p0, q0, q1, q2, q3] = [s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]];
            !((p3 - p2).abs() > t.limit
                || (p2 - p1).abs() > t.limit
                || (p1 - p0).abs() > t.limit
                || (q1 - q0).abs() > t.limit
                || (q2 - q1).abs() > t.limit
                || (q3 - q2).abs() > t.limit
                || (p0 - q0).abs() * 2 + (p1 - q1).abs() / 2 > t.blimit)
        }

        /// `flat_mask4(thresh, p3, p2, p1, p0, q0, q1, q2, q3)`.
        fn flat(t: &Thresholds, [p3, p2, p1, p0, q0, q1, q2, q3]: [i32; 8]) -> bool {
            !((p1 - p0).abs() > t.flat
                || (q1 - q0).abs() > t.flat
                || (p2 - p0).abs() > t.flat
                || (q2 - q0).abs() > t.flat
                || (p3 - p0).abs() > t.flat
                || (q3 - q0).abs() > t.flat)
        }

        /// `filter4` on `s[0..4]` = `p1 p0 q0 q1`, in place.
        fn filter4(t: &Thresholds, mask: bool, s: &mut [i32]) {
            let sc = |x: i32| x.clamp(-t.off, t.off - 1);
            let (ps1, ps0, qs0, qs1) = (s[0] - t.off, s[1] - t.off, s[2] - t.off, s[3] - t.off);
            let hev = (s[0] - s[1]).abs() > t.hev || (s[3] - s[2]).abs() > t.hev;
            let filter = if hev { sc(ps1 - qs1) } else { 0 };
            let filter = if mask {
                sc(filter + 3 * (qs0 - ps0))
            } else {
                0
            };
            let filter1 = sc(filter + 4) >> 3;
            let filter2 = sc(filter + 3) >> 3;
            s[2] = sc(qs0 - filter1) + t.off;
            s[1] = sc(ps0 + filter2) + t.off;
            let filter = if hev { 0 } else { (filter1 + 1) >> 1 };
            s[3] = sc(qs1 - filter) + t.off;
            s[0] = sc(ps1 + filter) + t.off;
        }

        /// `filter8` on `s[0..8]` = `p3..q3`, in place.
        fn filter8(t: &Thresholds, mask: bool, flat: bool, s: &mut [i32]) {
            if flat && mask {
                let [p3, p2, p1, p0, q0, q1, q2, q3] =
                    [s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]];
                let r = |x: i32| (x + 4) >> 3;
                s[1] = r(p3 + p3 + p3 + 2 * p2 + p1 + p0 + q0);
                s[2] = r(p3 + p3 + p2 + 2 * p1 + p0 + q0 + q1);
                s[3] = r(p3 + p2 + p1 + 2 * p0 + q0 + q1 + q2);
                s[4] = r(p2 + p1 + p0 + 2 * q0 + q1 + q2 + q3);
                s[5] = r(p1 + p0 + q0 + 2 * q1 + q2 + q3 + q3);
                s[6] = r(p0 + q0 + q1 + 2 * q2 + q3 + q3 + q3);
            } else {
                filter4(t, mask, &mut s[2..6]);
            }
        }

        pub fn lpf4(t: &Thresholds, s: &mut [i32]) {
            let m = mask(t, s);
            filter4(t, m, &mut s[2..6]);
        }

        pub fn lpf8(t: &Thresholds, s: &mut [i32]) {
            let m = mask(t, s);
            let f = flat(t, [s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]]);
            filter8(t, m, f, s);
        }

        #[rustfmt::skip]
        pub fn lpf16(t: &Thresholds, s: &mut [i32]) {
            let m = mask(t, &s[4..12]);
            let f = flat(t, [s[4], s[5], s[6], s[7], s[8], s[9], s[10], s[11]]);
            // flat_mask5(1, p7, p6, p5, p4, p0, q0, q4, q5, q6, q7).
            let f2 = flat(t, [s[1], s[2], s[3], s[7], s[8], s[12], s[13], s[14]])
                && (s[0] - s[7]).abs() <= t.flat
                && (s[15] - s[8]).abs() <= t.flat;
            if f2 && f && m {
                let [p7, p6, p5, p4, p3, p2, p1, p0] = [s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]];
                let [q0, q1, q2, q3, q4, q5, q6, q7] = [s[8], s[9], s[10], s[11], s[12], s[13], s[14], s[15]];
                let r = |x: i32| (x + 8) >> 4;
                s[1] = r(p7 * 7 + p6 * 2 + p5 + p4 + p3 + p2 + p1 + p0 + q0);
                s[2] = r(p7 * 6 + p6 + p5 * 2 + p4 + p3 + p2 + p1 + p0 + q0 + q1);
                s[3] = r(p7 * 5 + p6 + p5 + p4 * 2 + p3 + p2 + p1 + p0 + q0 + q1 + q2);
                s[4] = r(p7 * 4 + p6 + p5 + p4 + p3 * 2 + p2 + p1 + p0 + q0 + q1 + q2 + q3);
                s[5] = r(p7 * 3 + p6 + p5 + p4 + p3 + p2 * 2 + p1 + p0 + q0 + q1 + q2 + q3 + q4);
                s[6] = r(p7 * 2 + p6 + p5 + p4 + p3 + p2 + p1 * 2 + p0 + q0 + q1 + q2 + q3 + q4 + q5);
                s[7] = r(p7 + p6 + p5 + p4 + p3 + p2 + p1 + p0 * 2 + q0 + q1 + q2 + q3 + q4 + q5 + q6);
                s[8] = r(p6 + p5 + p4 + p3 + p2 + p1 + p0 + q0 * 2 + q1 + q2 + q3 + q4 + q5 + q6 + q7);
                s[9] = r(p5 + p4 + p3 + p2 + p1 + p0 + q0 + q1 * 2 + q2 + q3 + q4 + q5 + q6 + q7 * 2);
                s[10] = r(p4 + p3 + p2 + p1 + p0 + q0 + q1 + q2 * 2 + q3 + q4 + q5 + q6 + q7 * 3);
                s[11] = r(p3 + p2 + p1 + p0 + q0 + q1 + q2 + q3 * 2 + q4 + q5 + q6 + q7 * 4);
                s[12] = r(p2 + p1 + p0 + q0 + q1 + q2 + q3 + q4 * 2 + q5 + q6 + q7 * 5);
                s[13] = r(p1 + p0 + q0 + q1 + q2 + q3 + q4 + q5 * 2 + q6 + q7 * 6);
                s[14] = r(p0 + q0 + q1 + q2 + q3 + q4 + q5 + q6 * 2 + q7 * 7);
            } else {
                filter8(t, m, f, &mut s[4..12]);
            }
        }
    }

    struct Lcg(u32);

    impl Lcg {
        fn next(&mut self) -> u32 {
            self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            self.0 >> 8
        }

        fn below(&mut self, n: u32) -> i32 {
            (self.next() % n) as i32
        }
    }

    /// A line of `n` samples at `bd` bits that exercises a filter's choices:
    /// noise (rarely filtered), or a flat run with a step across the edge and
    /// noise of 0, 1 or 3 (scaled) on top -- flat enough for the wide
    /// filters, or nearly.
    fn line(rng: &mut Lcg, n: usize, bd: u32) -> Vec<i32> {
        let max = (1 << bd) - 1;
        let shift = bd - 8;
        let base = rng.below(1 << bd);
        let kind = rng.below(4);
        if kind == 0 {
            return (0..n).map(|_| rng.below(1 << bd)).collect();
        }
        let noise = [0, 0, 1, 3][kind as usize] << shift;
        let step = (rng.below(41) - 20) << shift;
        (0..n)
            .map(|i| {
                let side = if i >= n / 2 { step } else { 0 };
                let wobble = if noise > 0 {
                    rng.below(2 * noise as u32 + 1) - noise
                } else {
                    0
                };
                (base + side + wobble).clamp(0, max)
            })
            .collect()
    }

    /// libvpx's thresholds for a filter level and sharpness
    /// (`update_sharpness` and `vp9_loop_filter_frame_init`).
    fn limits(level: u8, sharpness: u8) -> FilterLimits {
        let shifted = level >> (u8::from(sharpness > 0) + u8::from(sharpness > 4));
        let inside = if sharpness > 0 {
            shifted.min(9 - sharpness)
        } else {
            shifted
        }
        .max(1);
        FilterLimits {
            mblim: 2 * (level + 2) + inside,
            lim: inside,
            hev_thr: level >> 4,
        }
    }

    /// Where sample `i` of line `j` of an edge at `at` lies, `k` samples
    /// across.
    fn position(at: usize, stride: usize, vertical: bool, j: usize, i: usize, k: usize) -> usize {
        if vertical {
            at + j * stride + i - k / 2
        } else {
            at + j + (i - k / 2) * stride
        }
    }

    /// Every filter, both edge directions and all three bit depths, against
    /// the reference: each call on eight lines must leave each line as the
    /// reference leaves it.
    #[test]
    fn eight_lane_filters_match_libvpx_line_by_line() {
        let mut rng = Lcg(0x10f1_17e5);
        for bd in [8u32, 10, 12] {
            let shift = bd - 8;
            for trial in 0..600 {
                let level = rng.below(64) as u8;
                let l = limits(level, rng.below(8) as u8);
                let t = Thresholds::new(&l, shift);
                let table = [l; 64];
                let ctx = Ctx {
                    limits: &table,
                    shift,
                    mi_row: 0,
                    mi_rows: 8,
                };
                let width = [4usize, 8, 16][trial % 3];
                let vertical = (trial / 3) % 2 == 0;
                let k = if width == 16 { 16 } else { 8 };
                let lines: Vec<Vec<i32>> = (0..8).map(|_| line(&mut rng, k, bd)).collect();
                // A 32x32 plane with the edge at (16, 16).
                let stride = 32;
                let mut plane = vec![0u16; stride * 32];
                let at = 16 * stride + 16;
                for (j, ln) in lines.iter().enumerate() {
                    for (i, &v) in ln.iter().enumerate() {
                        plane[position(at, stride, vertical, j, i, k)] = v as u16;
                    }
                }
                let (across, along) = if vertical { (1, stride) } else { (stride, 1) };
                match width {
                    4 => lpf_4(&ctx, &mut plane, at, across, along, &l, 8),
                    8 => lpf_8(&ctx, &mut plane, at, across, along, &l),
                    _ => lpf_16(&ctx, &mut plane, at, across, along, &l, 8),
                }
                for (j, ln) in lines.iter().enumerate() {
                    let mut want = ln.clone();
                    match width {
                        4 => reference::lpf4(&t, &mut want),
                        8 => reference::lpf8(&t, &mut want),
                        _ => reference::lpf16(&t, &mut want),
                    }
                    let got: Vec<i32> = (0..k)
                        .map(|i| i32::from(plane[position(at, stride, vertical, j, i, k)]))
                        .collect();
                    assert_eq!(
                        got, want,
                        "{bd}-bit, {width}-wide, vertical {vertical}, level {level}, line {j}: {ln:?}"
                    );
                }
            }
        }
    }

    /// The lines that test draws reach every outcome of the 16-wide filter:
    /// the 15-tap, the 7-tap, the narrow filter and none.
    #[test]
    fn the_test_lines_reach_every_16_wide_outcome() {
        let mut rng = Lcg(0x10f1_17e5);
        let t = Thresholds::new(&limits(40, 0), 0);
        let mut seen = [0u32; 4];
        for _ in 0..2000 {
            let s: [i32; 16] = line(&mut rng, 16, 8).try_into().unwrap();
            let inner: [i32; 8] = core::array::from_fn(|k| s[k + 4]);
            let mask = filter_mask(&t, inner);
            let flat = mask && flat_mask4(&t, inner);
            let outer = [s[1], s[2], s[3], s[7], s[8], s[12], s[13], s[14]];
            let flat2 = flat
                && flat_mask4(&t, outer)
                && (s[0] - s[7]).abs() <= t.flat
                && (s[15] - s[8]).abs() <= t.flat;
            let outcome = if flat2 {
                0
            } else if flat {
                1
            } else if mask {
                2
            } else {
                3
            };
            seen[outcome] += 1;
        }
        assert!(seen.iter().all(|&n| n > 50), "{seen:?}");
    }
}
