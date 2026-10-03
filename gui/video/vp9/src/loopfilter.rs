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
//! On several threads, superblock rows are filtered as libvpx's
//! `vp9_loop_filter_frame_mt` does them: a wavefront, each row a superblock
//! or two behind the row above, which gives the single thread's pixels (see
//! "Filtering on threads" below).
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
use std::sync::mpsc::{Receiver, Sender};

use crate::frame::{AnyBuffers, AnyFrame, Buffers, FrameBuf, Pixel, Plane};
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
/// superblock row -- on up to `threads` threads, libvpx's
/// `vp9_loop_filter_frame_mt`, when the frame has rows enough to share.
pub(crate) fn filter_frame(
    frame: &mut AnyFrame,
    decoded: &Decoded,
    levels: &LevelTable,
    limits: &LimitTable,
    threads: usize,
    scratch: &mut AnyBuffers,
) {
    match frame {
        AnyFrame::Eight(f) => {
            filter_frame_t(f, &decoded.mi, levels, limits, threads, &mut scratch.eight);
        }
        AnyFrame::High(f) => {
            filter_frame_t(f, &decoded.mi, levels, limits, threads, &mut scratch.high);
        }
    }
}

fn filter_frame_t<P: Pixel>(
    frame: &mut FrameBuf<P>,
    mi: &MiGrid,
    levels: &LevelTable,
    limits: &LimitTable,
    threads: usize,
    scratch: &mut Buffers<P>,
) {
    let plan = Plan::new(frame, mi, levels, limits);
    // Two rows a thread at least: fewer, and starting the threads costs more
    // than they save.
    let workers = threads.min(plan.sb_rows / 2);
    if workers > 1 && filter_rows_threaded(frame, &plan, workers, scratch).is_some() {
        return;
    }
    let [y, u, v] = &mut frame.planes;
    let mut rows = [Rows::whole(y), Rows::whole(u), Rows::whole(v)];
    for sb_row in 0..plan.sb_rows {
        for sb_col in 0..plan.sb_cols {
            plan.filter_sb(&mut rows, sb_row, sb_col);
        }
    }
}

/// A plane's rows from `row0` on, `stride` samples apart: the whole plane,
/// or a thread's copy of a band of superblock rows and the rows above it.
struct Rows<'b, P> {
    data: &'b mut [P],
    row0: usize,
    stride: usize,
}

impl<'b, P> Rows<'b, P> {
    fn whole(p: &'b mut Plane<P>) -> Self {
        Self {
            stride: p.stride,
            data: &mut p.data,
            row0: 0,
        }
    }
}

/// What filtering any superblock of a frame needs: every superblock's edge
/// masks, made up front; the blocks, which chroma planes subsampled one way
/// only are filtered from; and the thresholds.
struct Plan<'a> {
    mi: &'a MiGrid,
    levels: &'a LevelTable,
    limits: &'a LimitTable,
    lfms: Vec<Lfm>,
    sb_rows: usize,
    sb_cols: usize,
    path: Path,
    ss: (usize, usize),
    /// `bit depth - 8`.
    shift: u32,
}

impl<'a> Plan<'a> {
    fn new<P: Pixel>(
        frame: &FrameBuf<P>,
        mi: &'a MiGrid,
        levels: &'a LevelTable,
        limits: &'a LimitTable,
    ) -> Self {
        let (mi_rows, mi_cols) = (mi.mi_rows, mi.mi_cols);
        let sb_cols = mi_cols.div_ceil(8);
        let sb_rows = mi_rows.div_ceil(8);
        let mut lfms = vec![Lfm::default(); sb_cols * sb_rows];
        for b in &mi.blocks {
            if let Some(lfm) = lfms.get_mut((b.mi_row >> 3) * sb_cols + (b.mi_col >> 3)) {
                build_mask(lfm, levels, b);
            }
        }
        for (i, lfm) in lfms.iter_mut().enumerate() {
            let (sb_row, sb_col) = (i / sb_cols.max(1), i % sb_cols.max(1));
            adjust_mask(lfm, sb_row * 8, sb_col * 8, mi_rows, mi_cols);
        }
        let path = match (frame.ss_x, frame.ss_y) {
            (1, 1) => Path::Ss11,
            (0, 0) => Path::Ss00,
            _ => Path::NonSs11,
        };
        Self {
            mi,
            levels,
            limits,
            lfms,
            sb_rows,
            sb_cols,
            path,
            ss: (usize::from(frame.ss_x), usize::from(frame.ss_y)),
            shift: u32::from(frame.bit_depth.clamp(8, 12) - 8),
        }
    }

    /// Filter superblock (`sb_row`, `sb_col`): Y, then U, then V, each
    /// plane's vertical edges and then its horizontal ones.
    fn filter_sb<P: Pixel>(&self, rows: &mut [Rows<'_, P>; 3], sb_row: usize, sb_col: usize) {
        let Some(lfm) = self.lfms.get(sb_row * self.sb_cols + sb_col) else {
            return;
        };
        let (mi_row, mi_col) = (sb_row * 8, sb_col * 8);
        let ctx = Ctx {
            limits: self.limits,
            shift: self.shift,
            mi_row,
            mi_rows: self.mi.mi_rows,
        };
        let (ss_x, ss_y) = self.ss;
        for (plane, r) in rows.iter_mut().enumerate() {
            let (sx, sy) = if plane == 0 { (0, 0) } else { (ss_x, ss_y) };
            let Some(y) = ((mi_row * 8) >> sy).checked_sub(r.row0) else {
                continue;
            };
            let origin = y * r.stride + ((mi_col * 8) >> sx);
            match (plane, self.path) {
                (0, _) | (_, Path::Ss00) => {
                    filter_block_plane_ss00(&ctx, r.data, origin, r.stride, lfm);
                }
                (_, Path::Ss11) => filter_block_plane_ss11(&ctx, r.data, origin, r.stride, lfm),
                (_, Path::NonSs11) => filter_block_plane_non420(
                    &ctx,
                    r.data,
                    origin,
                    r.stride,
                    self.mi,
                    self.levels,
                    mi_col,
                    frame_ss(ss_x, ss_y),
                ),
            }
        }
    }
}

// --- Filtering on threads: libvpx's wavefront ------------------------------------------------
//
// Superblock row r's filters reach eight rows up into row r - 1 (its top
// edges), and row r - 1's reach a superblock's width to the right (the next
// superblock's left edge). So row r may filter superblock c once row r - 1
// has finished superblock c + 1 -- libvpx's `lf_sync`, the order a single
// thread gives, bit for bit.
//
// Here each thread takes every `workers`-th row, and filters a private copy
// of it: the band's own rows, with the band above's last eight rows on top.
// Those arrive a superblock at a time down a channel from the thread above,
// as each becomes final: after it has finished the superblock to the right.
// The thread below filters its top edges into its copy of them. Once every
// thread is done, the bands are written back, and then the rows above each,
// which hold the last word on them. Nothing is shared but by message, and the
// frame is not touched until the end -- so anything going wrong, a thread
// not starting among them, leaves it to be filtered on one thread instead.

/// How many rows of the band above a band's top edges read: the 16-wide
/// filter's `p7..p0`.
const TAIL: usize = 8;

/// A band's last rows under one superblock, final: on its way down to the
/// thread filtering the band below.
struct Tail<P> {
    sb_row: usize,
    sb_col: usize,
    plane: usize,
    rows: Vec<P>,
}

/// A band of superblock rows as a thread filtered it: per plane, the rows
/// above it (but for the first band) and then its own.
struct Band<P> {
    sb_row: usize,
    planes: [Vec<P>; 3],
}

/// Per plane: its stride, a band's height and a superblock's width.
type Geometry = [(usize, usize, usize); 3];

/// Filter every row of the frame on `workers` threads. `None`, with the
/// frame untouched, if they could not all run.
fn filter_rows_threaded<P: Pixel>(
    frame: &mut FrameBuf<P>,
    plan: &Plan<'_>,
    workers: usize,
    scratch: &mut Buffers<P>,
) -> Option<()> {
    let (ss_x, ss_y) = plan.ss;
    let geometry: Geometry = core::array::from_fn(|plane| {
        let (sx, sy) = if plane == 0 { (0, 0) } else { (ss_x, ss_y) };
        (frame.planes[plane].stride, 64 >> sy, 64 >> sx)
    });
    let bands: Vec<Band<P>> = {
        let frame: &FrameBuf<P> = frame;
        // Worker t sends down channel t to worker t + 1, the next row's.
        let mut senders = Vec::with_capacity(workers);
        let mut receivers = Vec::with_capacity(workers);
        for _ in 0..workers {
            let (tx, rx) = std::sync::mpsc::channel::<Tail<P>>();
            senders.push(Some(tx));
            receivers.push(Some(rx));
        }
        // Each thread's empty buffers for its bands' copies, from the pool.
        let mut empties: Vec<Vec<[Vec<P>; 3]>> = (0..workers)
            .map(|t| {
                (t..plan.sb_rows)
                    .step_by(workers)
                    .map(|_| [scratch.rows(), scratch.rows(), scratch.rows()])
                    .collect()
            })
            .collect();
        // `Err` if a thread could not start; otherwise each thread's bands,
        // `None` from a thread that lost step.
        let outcomes: Result<Vec<Option<Vec<Band<P>>>>, ()> = std::thread::scope(|scope| {
            let mut handles = Vec::with_capacity(workers);
            for t in 0..workers {
                let mine = empties.get_mut(t).map(core::mem::take).unwrap_or_default();
                let tx = senders.get_mut(t).and_then(Option::take);
                let rx = receivers
                    .get_mut((t + workers - 1) % workers)
                    .and_then(Option::take);
                let started = match (tx, rx) {
                    (Some(tx), Some(rx)) => {
                        let work = move || {
                            filter_band_rows(frame, plan, geometry, (t, workers), mine, (&tx, &rx))
                        };
                        std::thread::Builder::new().spawn_scoped(scope, work).ok()
                    }
                    _ => None,
                };
                let Some(h) = started else {
                    // A thread that cannot start dropped its channel ends;
                    // dropping those no thread took too disconnects every
                    // thread waiting on a row above, so all of them stop.
                    senders.clear();
                    receivers.clear();
                    return Err(());
                };
                handles.push(h);
            }
            Ok(handles
                .into_iter()
                .map(|h| h.join().ok().flatten())
                .collect())
        });
        // A thread that could not start leaves the frame to one thread: the
        // machine's limits, not a fault.
        let outcomes = outcomes.ok()?;
        let mut bands = Vec::with_capacity(plan.sb_rows);
        for outcome in outcomes {
            let Some(rows) = outcome else {
                // The threads' steps depend on the frame's size alone, never
                // on what it holds, so this is a bug in them: one thread
                // still gives the right pixels, but tests must hear of it.
                debug_assert!(false, "the loop filter's threads lost step");
                return None;
            };
            bands.extend(rows);
        }
        bands
    };
    // Every band whole before the frame is touched: falling back to one
    // thread must find the frame as it was.
    let whole = bands.len() == plan.sb_rows
        && bands.iter().all(|band| {
            let apron = if band.sb_row > 0 { TAIL } else { 0 };
            band.planes.iter().zip(&frame.planes).zip(&geometry).all(
                |((data, plane), &(stride, band_h, _))| {
                    data.len() == (apron + band_h) * stride
                        && (band.sb_row + 1) * band_h * stride <= plane.data.len()
                },
            )
        });
    if !whole {
        debug_assert!(false, "a loop filter thread's band is the wrong size");
        return None;
    }
    // The bands' own rows, then the rows above each, which the band below
    // filtered last.
    for band in &bands {
        let apron = if band.sb_row > 0 { TAIL } else { 0 };
        for ((plane, data), &(stride, band_h, _)) in
            frame.planes.iter_mut().zip(&band.planes).zip(&geometry)
        {
            let top = band.sb_row * band_h * stride;
            let own = data.get(apron * stride..)?;
            plane
                .data
                .get_mut(top..top + own.len())?
                .copy_from_slice(own);
        }
    }
    for band in bands.iter().filter(|b| b.sb_row > 0) {
        for ((plane, data), &(stride, band_h, _)) in
            frame.planes.iter_mut().zip(&band.planes).zip(&geometry)
        {
            let top = (band.sb_row * band_h - TAIL) * stride;
            let above = data.get(..TAIL * stride)?;
            plane
                .data
                .get_mut(top..top + above.len())?
                .copy_from_slice(above);
        }
    }
    for band in bands {
        for rows in band.planes {
            scratch.give_rows(rows);
        }
    }
    Some(())
}

/// One thread's share: rows `t`, `t + workers`, ... `None` if the channel
/// from the thread above broke, or the frame's geometry is not what it should
/// be.
fn filter_band_rows<P: Pixel>(
    frame: &FrameBuf<P>,
    plan: &Plan<'_>,
    geometry: Geometry,
    (t, workers): (usize, usize),
    mut empties: Vec<[Vec<P>; 3]>,
    (tx, rx): (&Sender<Tail<P>>, &Receiver<Tail<P>>),
) -> Option<Vec<Band<P>>> {
    let mut out = Vec::new();
    for sb_row in (t..plan.sb_rows).step_by(workers) {
        let apron = if sb_row > 0 { TAIL } else { 0 };
        let mut planes: [Vec<P>; 3] = empties.pop().unwrap_or_default();
        for ((copy, plane), &(stride, band_h, _)) in
            planes.iter_mut().zip(&frame.planes).zip(&geometry)
        {
            let top = sb_row * band_h * stride;
            let own = plane.data.get(top..top + band_h * stride)?;
            copy.reserve_exact((apron + band_h) * stride);
            copy.resize(apron * stride, P::default());
            copy.extend_from_slice(own);
        }
        let below = sb_row + 1 < plan.sb_rows;
        for sb_col in 0..plan.sb_cols {
            if sb_row > 0 {
                for (plane, (copy, &(stride, _, sb_w))) in
                    planes.iter_mut().zip(&geometry).enumerate()
                {
                    let tail = rx.recv().ok()?;
                    if (tail.sb_row, tail.sb_col, tail.plane) != (sb_row - 1, sb_col, plane) {
                        return None;
                    }
                    for (r, src) in tail.rows.chunks_exact(sb_w).enumerate() {
                        let at = r * stride + sb_col * sb_w;
                        copy.get_mut(at..at + sb_w)?.copy_from_slice(src);
                    }
                }
            }
            {
                let [y, u, v] = &mut planes;
                let row0 = |plane: usize| {
                    let band_h = geometry[plane].1;
                    sb_row * band_h - apron
                };
                let mut rows = [
                    Rows {
                        data: y,
                        row0: row0(0),
                        stride: geometry[0].0,
                    },
                    Rows {
                        data: u,
                        row0: row0(1),
                        stride: geometry[1].0,
                    },
                    Rows {
                        data: v,
                        row0: row0(2),
                        stride: geometry[2].0,
                    },
                ];
                plan.filter_sb(&mut rows, sb_row, sb_col);
            }
            // The superblock to the left is final now: nothing to its right
            // reaches back past this one's left edge.
            if below && sb_col > 0 {
                send_tails(&planes, geometry, apron, sb_row, sb_col - 1, tx)?;
            }
        }
        if below {
            send_tails(
                &planes,
                geometry,
                apron,
                sb_row,
                plan.sb_cols.checked_sub(1)?,
                tx,
            )?;
        }
        out.push(Band { sb_row, planes });
    }
    Some(out)
}

/// Send band `sb_row`'s last rows under superblock `sb_col`, every plane,
/// down to the thread filtering the band below.
fn send_tails<P: Pixel>(
    planes: &[Vec<P>; 3],
    geometry: Geometry,
    apron: usize,
    sb_row: usize,
    sb_col: usize,
    tx: &Sender<Tail<P>>,
) -> Option<()> {
    for (plane, (data, &(stride, band_h, sb_w))) in planes.iter().zip(&geometry).enumerate() {
        let mut rows = Vec::with_capacity(TAIL * sb_w);
        for r in apron + band_h - TAIL..apron + band_h {
            let at = r * stride + sb_col * sb_w;
            rows.extend_from_slice(data.get(at..at + sb_w)?);
        }
        tx.send(Tail {
            sb_row,
            sb_col,
            plane,
            rows,
        })
        .ok()?;
    }
    Some(())
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

/// One sample of each of eight lines across an edge.
///
/// Sixteen bits hold every value the filters compute, at every bit depth:
/// samples are at most 12 bits, the narrow filter's arithmetic at most
/// 3 * 4095 + 2048, the 7-tap sums 8 * 4095 + 4. Only the 15-tap sums at
/// 12 bits (16 * 4095 + 8) do not fit; they are never negative, so they are
/// taken unsigned. Eight 16-bit lanes are one SSE2 register.
type Lane = [i16; 8];

/// `K` samples across an edge for eight lines: `v[k][j]` is sample `k` of
/// line `j`, in order across the edge -- `p7` to `p0` then `q0` to `q7` for
/// the 16-wide filter, `p3` to `q3` for the others.
type Lanes<const K: usize> = [Lane; K];

/// One filter call's thresholds, scaled to the bit depth.
#[derive(Clone, Copy)]
struct Thresholds {
    limit: i16,
    blimit: i16,
    hev: i16,
    /// The flatness threshold: 1, scaled.
    flat: i16,
    /// 128, scaled: what makes a sample signed for the narrow filter.
    off: i16,
}

impl Thresholds {
    fn new(l: &FilterLimits, shift: u32) -> Self {
        Self {
            limit: i16::from(l.lim) << shift,
            blimit: i16::from(l.mblim) << shift,
            hev: i16::from(l.hev_thr) << shift,
            flat: 1 << shift,
            off: 128 << shift,
        }
    }
}

/// A lane mask, all ones where `ok`: what a comparison of vectors gives.
#[inline(always)]
fn ones(ok: bool) -> i16 {
    -i16::from(ok)
}

/// Each lane of `a` where the mask `m` is set, else of `b`.
#[inline(always)]
fn select(m: &Lane, a: &Lane, b: &Lane) -> Lane {
    let mut out = [0; 8];
    for j in 0..8 {
        out[j] = (m[j] & a[j]) | (!m[j] & b[j]);
    }
    out
}

/// libvpx's `filter_mask`: which lines are filtered at all.
#[inline(always)]
fn filter_mask(t: &Thresholds, [p3, p2, p1, p0, q0, q1, q2, q3]: &[Lane; 8]) -> Lane {
    let mut m = [0; 8];
    for j in 0..8 {
        let ok = ((p3[j] - p2[j]).abs() <= t.limit)
            & ((p2[j] - p1[j]).abs() <= t.limit)
            & ((p1[j] - p0[j]).abs() <= t.limit)
            & ((q1[j] - q0[j]).abs() <= t.limit)
            & ((q2[j] - q1[j]).abs() <= t.limit)
            & ((q3[j] - q2[j]).abs() <= t.limit)
            & ((p0[j] - q0[j]).abs() * 2 + ((p1[j] - q1[j]).abs() >> 1) <= t.blimit);
        m[j] = ones(ok);
    }
    m
}

/// libvpx's `flat_mask4`: `p3..p1` within the flatness threshold of `p0`,
/// and `q1..q3` of `q0`. Given the outer samples in place of the inner
/// ones, the larger part of `flat_mask5`.
#[inline(always)]
fn flat_mask4(t: &Thresholds, [p3, p2, p1, p0, q0, q1, q2, q3]: &[Lane; 8]) -> Lane {
    let mut m = [0; 8];
    for j in 0..8 {
        let ok = ((p1[j] - p0[j]).abs() <= t.flat)
            & ((q1[j] - q0[j]).abs() <= t.flat)
            & ((p2[j] - p0[j]).abs() <= t.flat)
            & ((q2[j] - q0[j]).abs() <= t.flat)
            & ((p3[j] - p0[j]).abs() <= t.flat)
            & ((q3[j] - q0[j]).abs() <= t.flat);
        m[j] = ones(ok);
    }
    m
}

/// libvpx's `filter4` on `p1 p0 q0 q1`, as new values for them; where the
/// mask is off, the values it was given.
#[inline(always)]
fn filter4(t: &Thresholds, mask: &Lane, [p1, p0, q0, q1]: [&Lane; 4]) -> [Lane; 4] {
    let (lo, hi) = (-t.off, t.off - 1);
    let mut out = [[0; 8]; 4];
    for j in 0..8 {
        let sc = |x: i16| x.clamp(lo, hi);
        let (ps1, ps0, qs0, qs1) = (p1[j] - t.off, p0[j] - t.off, q0[j] - t.off, q1[j] - t.off);
        let hev = ones(((p1[j] - p0[j]).abs() > t.hev) | ((q1[j] - q0[j]).abs() > t.hev));
        // The outer taps only where the edge varies a lot.
        let f = sc(ps1 - qs1) & hev;
        let f = sc(f + 3 * (qs0 - ps0)) & mask[j];
        // One side rounded up, the other down.
        let f1 = sc(f + 4) >> 3;
        let f2 = sc(f + 3) >> 3;
        let outer = ((f1 + 1) >> 1) & !hev;
        out[0][j] = sc(ps1 + outer) + t.off;
        out[1][j] = sc(ps0 + f2) + t.off;
        out[2][j] = sc(qs0 - f1) + t.off;
        out[3][j] = sc(qs1 - outer) + t.off;
    }
    out
}

/// The flat 7-tap filter of libvpx's `filter8`: new `p2..q2`.
#[inline(always)]
fn flat8([p3, p2, p1, p0, q0, q1, q2, q3]: &[Lane; 8]) -> [Lane; 6] {
    let mut out = [[0; 8]; 6];
    for j in 0..8 {
        let r = |x: i16| (x + 4) >> 3;
        let [p3, p2, p1, p0, q0, q1, q2, q3] =
            [p3[j], p2[j], p1[j], p0[j], q0[j], q1[j], q2[j], q3[j]];
        out[0][j] = r(3 * p3 + 2 * p2 + p1 + p0 + q0);
        out[1][j] = r(2 * p3 + p2 + 2 * p1 + p0 + q0 + q1);
        out[2][j] = r(p3 + p2 + p1 + 2 * p0 + q0 + q1 + q2);
        out[3][j] = r(p2 + p1 + p0 + 2 * q0 + q1 + q2 + q3);
        out[4][j] = r(p1 + p0 + q0 + 2 * q1 + q2 + 2 * q3);
        out[5][j] = r(p0 + q0 + q1 + 2 * q2 + 3 * q3);
    }
    out
}

/// The flat 15-tap filter of libvpx's `filter16`: new `p6..q6`. Output `i`
/// is the fifteen samples centred on it -- the end samples repeated past the
/// line's ends -- plus itself once more, so a running sum computes them
/// all. The sums are unsigned (see [`Lane`]); added and taken away in turn,
/// they wrap modulo 2^16 to the exact value.
#[inline(always)]
fn flat16(s: &Lanes<16>) -> [Lane; 14] {
    let u = |k: usize, j: usize| s[k][j] as u16;
    let mut sum = [0u16; 8];
    for (j, sum) in sum.iter_mut().enumerate() {
        // Output 1's window: p7 seven times, then p6 to q0.
        *sum = 7 * u(0, j);
        for k in 1..=8 {
            *sum += u(k, j);
        }
    }
    let mut out = [[0; 8]; 14];
    for i in 1..15usize {
        let (add, take) = ((i + 8).min(15), i.saturating_sub(7));
        for j in 0..8 {
            out[i - 1][j] = (sum[j].wrapping_add(u(i, j)).wrapping_add(8) >> 4) as i16;
            sum[j] = sum[j].wrapping_add(u(add, j)).wrapping_sub(u(take, j));
        }
    }
    out
}

/// Whether any lane of a mask is set.
#[inline(always)]
fn any(m: &Lane) -> bool {
    m.iter().fold(0, |a, &x| a | x) != 0
}

/// How far either side of an edge a filter changed samples: 2 (`p1..q1`),
/// 3 (`p2..q2`) or 7 (`p6..q6`).
type Reach = usize;

/// libvpx's `vpx_lpf_*_4`, eight lines at once: `p3..q3` in, `p1..q1` out.
/// Like each of these, returns how far it changed samples, if it changed
/// any -- skipping, as libvpx's SIMD does, what no lane needs.
#[inline(always)]
fn lanes4(v: &mut Lanes<8>, t: &Thresholds) -> Option<Reach> {
    let mask = filter_mask(t, v);
    if !any(&mask) {
        return None;
    }
    let f4 = filter4(t, &mask, [&v[2], &v[3], &v[4], &v[5]]);
    v[2..6].copy_from_slice(&f4);
    Some(2)
}

/// libvpx's `vpx_lpf_*_8`, eight lines at once: `p3..q3` in, `p2..q2` out.
#[inline(always)]
fn lanes8(v: &mut Lanes<8>, t: &Thresholds) -> Option<Reach> {
    let mask = filter_mask(t, v);
    if !any(&mask) {
        return None;
    }
    let flat = select(&mask, &flat_mask4(t, v), &[0; 8]);
    let f4 = filter4(t, &mask, [&v[2], &v[3], &v[4], &v[5]]);
    if !any(&flat) {
        v[2..6].copy_from_slice(&f4);
        return Some(2);
    }
    let f8 = flat8(v);
    // Sample by sample, every index a constant: as a loop, the compiler
    // rebuilt each lane vector a sample at a time.
    let [_, p2, _, _, _, _, q2, _] = *v;
    let [n1, n0, m0, m1] = f4;
    v[1] = select(&flat, &f8[0], &p2);
    v[2] = select(&flat, &f8[1], &n1);
    v[3] = select(&flat, &f8[2], &n0);
    v[4] = select(&flat, &f8[3], &m0);
    v[5] = select(&flat, &f8[4], &m1);
    v[6] = select(&flat, &f8[5], &q2);
    Some(3)
}

/// libvpx's `vpx_lpf_*_16`, eight lines at once: `p7..q7` in, `p6..q6` out.
#[inline(always)]
fn lanes16(v: &mut Lanes<16>, t: &Thresholds) -> Option<Reach> {
    let inner: [Lane; 8] = [v[4], v[5], v[6], v[7], v[8], v[9], v[10], v[11]];
    let mask = filter_mask(t, &inner);
    if !any(&mask) {
        return None;
    }
    let flat = select(&mask, &flat_mask4(t, &inner), &[0; 8]);
    let f4 = filter4(t, &mask, [&v[6], &v[7], &v[8], &v[9]]);
    if !any(&flat) {
        v[6..10].copy_from_slice(&f4);
        return Some(2);
    }
    // libvpx's flat_mask5: p7..p4 and q4..q7 against p0 and q0 too.
    let outer = flat_mask4(t, &[v[1], v[2], v[3], v[7], v[8], v[12], v[13], v[14]]);
    let mut flat2 = [0; 8];
    for j in 0..8 {
        let ends = ((v[0][j] - v[7][j]).abs() <= t.flat) & ((v[15][j] - v[8][j]).abs() <= t.flat);
        flat2[j] = flat[j] & outer[j] & ones(ends);
    }
    let f8 = flat8(&inner);
    // Sample by sample, every index a constant (see `lanes8`): the narrow
    // filter's p1..q1, then the 7-tap filter's p2..q2 where flat.
    let [n1, n0, m0, m1] = f4;
    let eight = [
        select(&flat, &f8[0], &v[5]),
        select(&flat, &f8[1], &n1),
        select(&flat, &f8[2], &n0),
        select(&flat, &f8[3], &m0),
        select(&flat, &f8[4], &m1),
        select(&flat, &f8[5], &v[10]),
    ];
    if !any(&flat2) {
        v[5..11].copy_from_slice(&eight);
        return Some(3);
    }
    let f16 = flat16(v);
    // And the 15-tap filter's p6..q6 where flat2.
    let [e2, e1, e0, d0, d1, d2] = eight;
    v[1] = select(&flat2, &f16[0], &v[1]);
    v[2] = select(&flat2, &f16[1], &v[2]);
    v[3] = select(&flat2, &f16[2], &v[3]);
    v[4] = select(&flat2, &f16[3], &v[4]);
    v[5] = select(&flat2, &f16[4], &e2);
    v[6] = select(&flat2, &f16[5], &e1);
    v[7] = select(&flat2, &f16[6], &e0);
    v[8] = select(&flat2, &f16[7], &d0);
    v[9] = select(&flat2, &f16[8], &d1);
    v[10] = select(&flat2, &f16[9], &d2);
    v[11] = select(&flat2, &f16[10], &v[11]);
    v[12] = select(&flat2, &f16[11], &v[12]);
    v[13] = select(&flat2, &f16[12], &v[13]);
    v[14] = select(&flat2, &f16[13], &v[14]);
    Some(7)
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
            *x = p.int() as i16;
        }
    }
    Some(v)
}

/// Write back the samples within `R` of a horizontal edge.
#[inline(always)]
fn store_h<P: Pixel, const K: usize, const R: usize>(
    s: &mut [P],
    at: usize,
    stride: usize,
    v: &Lanes<K>,
) {
    let Some(top) = at.checked_sub(K / 2 * stride) else {
        return;
    };
    for k in K / 2 - R..K / 2 + R {
        let Some(row) = s.get_mut(top + k * stride..).and_then(|r| r.get_mut(..8)) else {
            continue;
        };
        for (p, &x) in row.iter_mut().zip(&v[k]) {
            *p = P::from_int(i32::from(x));
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
            lane[j] = p.int() as i16;
        }
    }
    Some(v)
}

/// Write back the samples within `R` of a vertical edge.
#[inline(always)]
fn store_v<P: Pixel, const K: usize, const R: usize>(
    s: &mut [P],
    at: usize,
    stride: usize,
    v: &Lanes<K>,
) {
    let Some(left) = at.checked_sub(K / 2) else {
        return;
    };
    for j in 0..8 {
        let Some(line) = s.get_mut(left + j * stride..).and_then(|r| r.get_mut(..K)) else {
            continue;
        };
        for k in K / 2 - R..K / 2 + R {
            line[k] = P::from_int(i32::from(v[k][j]));
        }
    }
}

/// Run `filter` on `count` lines (a multiple of eight) across an edge at
/// `at`: a vertical edge if `across` is 1 (the lines are rows, `along` the
/// stride), else a horizontal one (`across` the stride, the lines columns).
/// Writes back the samples the filter says it changed.
#[inline(always)]
fn lpf<P: Pixel, const K: usize>(
    s: &mut [P],
    at: usize,
    (across, along): (usize, usize),
    count: usize,
    t: &Thresholds,
    filter: impl Fn(&mut Lanes<K>, &Thresholds) -> Option<Reach>,
) {
    for g in 0..count / 8 {
        let at = at + g * 8 * along;
        if across == 1 {
            let Some(mut v) = load_v::<P, K>(s, at, along) else {
                continue;
            };
            match filter(&mut v, t) {
                Some(2) => store_v::<P, K, 2>(s, at, along, &v),
                Some(3) => store_v::<P, K, 3>(s, at, along, &v),
                Some(7) => store_v::<P, K, 7>(s, at, along, &v),
                _ => {}
            }
        } else {
            let Some(mut v) = load_h::<P, K>(s, at, across) else {
                continue;
            };
            match filter(&mut v, t) {
                Some(2) => store_h::<P, K, 2>(s, at, across, &v),
                Some(3) => store_h::<P, K, 3>(s, at, across, &v),
                Some(7) => store_h::<P, K, 7>(s, at, across, &v),
                _ => {}
            }
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
    lpf::<P, 8>(s, at, (across, along), count, &t, lanes4);
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
    lpf::<P, 8>(s, at, (across, along), 8, &t, lanes8);
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
    lpf::<P, 16>(s, at, (across, along), count, &t, lanes16);
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
        use crate::header::FilterLimits;

        /// libvpx's thresholds for one call, in the C's own ints.
        pub struct Thresholds {
            limit: i32,
            blimit: i32,
            hev: i32,
            pub flat: i32,
            off: i32,
        }

        impl Thresholds {
            pub fn new(l: &FilterLimits, shift: u32) -> Self {
                Self {
                    limit: i32::from(l.lim) << shift,
                    blimit: i32::from(l.mblim) << shift,
                    hev: i32::from(l.hev_thr) << shift,
                    flat: 1 << shift,
                    off: 128 << shift,
                }
            }
        }

        pub fn mask(t: &Thresholds, s: &[i32]) -> bool {
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
        pub fn flat(t: &Thresholds, [p3, p2, p1, p0, q0, q1, q2, q3]: [i32; 8]) -> bool {
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
            at + j + i * stride - k / 2 * stride
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
                let t = reference::Thresholds::new(&l, shift);
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
        let t = reference::Thresholds::new(&limits(40, 0), 0);
        let mut seen = [0u32; 4];
        for _ in 0..2000 {
            let s: [i32; 16] = line(&mut rng, 16, 8).try_into().unwrap();
            let inner: [i32; 8] = core::array::from_fn(|k| s[k + 4]);
            let mask = reference::mask(&t, &inner);
            let flat = mask && reference::flat(&t, inner);
            let outer = [s[1], s[2], s[3], s[7], s[8], s[12], s[13], s[14]];
            let flat2 = flat
                && reference::flat(&t, outer)
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
