//! Encoding a frame's blocks: walking each tile's superblocks through their
//! partitions, and coding each block -- predicting it, transforming and
//! quantising what prediction missed, reconstructing what the decoder will
//! see, and turning the levels into tokens.
//!
//! What to do at each step -- how to partition, which modes -- is asked of a
//! [`Decide`]; this module carries the decisions out exactly as libvpx's
//! encoder does, so that what it reconstructs is, pixel for pixel, what a
//! decoder will make of the stream. Intra blocks for now: key frames.
//!
//! Translated into Rust from libvpx v1.17.0's `vp9/encoder/vp9_encodeframe.c`
//! (`encode_superblock`, `encode_b_rt`, `encode_sb_rt`, `encode_tiles`),
//! `vp9_encodemb.c` (`vp9_encode_block_intra`,
//! `vp9_encode_intra_block_plane`), `vp9_tokenize.c` (`vp9_tokenize_sb`)
//! and `vp9/common/vp9_blockd.c` (`vp9_foreach_transformed_block_in_plane`,
//! `vp9_set_contexts`) (copyright the WebM project authors), used under
//! libvpx's BSD licence and patent grant (`licenses/libvpx-LICENSE`,
//! `licenses/libvpx-PATENTS`).

#![allow(
    clippy::arithmetic_side_effects,
    reason = "positions are mode-info and pixel coordinates bounded by the frame (at most 65536 a side), transform-block indices below 256, and residuals of 8-bit samples; counts wrap as libvpx's unsigned ones do"
)]
#![allow(
    clippy::indexing_slicing,
    reason = "the only indices are planes, clamped with min(2) into three-element arrays; everything taken from a buffer goes through get"
)]

use crate::block::{MiGrid, ModeInfo, uv_tx_size};
use crate::common::{
    BLOCK_8X8, BLOCK_64X64, BLOCK_INVALID, BlockSize, DCT_DCT, INTRA_FRAME, INTRA_MODE_TO_TX_TYPE,
    NO_REF_FRAME, PARTITION_HORZ, PARTITION_NONE, PARTITION_SPLIT, PARTITION_VERT, Partition,
    PredictionMode, SEG_LVL_SKIP, TX_4X4, TX_8X8, TX_16X16, TX_32X32, TX_MODE_SELECT, TxSize,
    TxType,
};
use crate::context;
use crate::detokenize::{Scan, scan_for};
use crate::enc::bitstream::{EncCounts, FrameHeader};
use crate::enc::fdct;
use crate::enc::quantize::{Quants, quantize_b};
use crate::enc::tokenize::{TokenExtra, TokenSink, tokenize_b};
use crate::frame::FrameBuf;
use crate::header::{self, Segmentation};
use crate::idct;
use crate::intra::{self, Edges};
use crate::tables;

/// A block's intra modes, as a [`Decide`] chooses them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct BlockModes {
    /// The luma mode of a block 8x8 or larger.
    pub mode: PredictionMode,
    /// The luma modes of a block smaller than 8x8, by 4x4 quarter (raster
    /// order). A 4x8 block uses quarters 0 and 1, an 8x4 block 0 and 2;
    /// the others repeat them.
    pub sub_modes: [PredictionMode; 4],
    pub uv_mode: PredictionMode,
    /// The transform size, if the frame lets blocks choose it
    /// (`TX_MODE_SELECT`); otherwise the frame's mode decides.
    pub tx_size: TxSize,
}

/// What an encoder decides: how each superblock is partitioned and how each
/// block is predicted. libvpx's choices -- variance-based partitioning, the
/// realtime mode search -- are implementations of this; so is a test's
/// random choice, which exercises everything the stream can say.
pub(crate) trait Decide {
    /// The partition of the square block `bsize` at (`mi_row`, `mi_col`).
    /// Where the block crosses the frame's bottom edge only `PARTITION_HORZ`
    /// and `PARTITION_SPLIT` can be coded, across the right edge only
    /// `PARTITION_VERT` and `PARTITION_SPLIT`, across both only
    /// `PARTITION_SPLIT`; any other answer there is taken as a split.
    fn partition(
        &mut self,
        f: &FrameEncoder<'_>,
        mi_row: usize,
        mi_col: usize,
        bsize: BlockSize,
    ) -> Partition;

    /// The modes of block `bsize` at (`mi_row`, `mi_col`), the blocks before
    /// it in coding order already reconstructed.
    fn modes(
        &mut self,
        f: &FrameEncoder<'_>,
        mi_row: usize,
        mi_col: usize,
        bsize: BlockSize,
    ) -> BlockModes;
}

/// Where a block is, as libvpx's `MACROBLOCKD` describes it: its size in
/// cells and its distances to the frame's edges in eighth pixels (negative
/// right and bottom distances reach past the frame).
#[derive(Clone, Copy, Debug)]
struct Placement {
    mi_row: usize,
    mi_col: usize,
    mb_to_right_edge: i32,
    mb_to_bottom_edge: i32,
}

/// A plane's coefficient buffers for one block of up to 64x64: libvpx's
/// `macroblock_plane` `qcoeff` and `eobs` and `macroblockd_plane`
/// `dqcoeff`, each transform block's at 16 times its index in 4x4 units.
struct PlaneCoeffs {
    qcoeff: Vec<i32>,
    dqcoeff: Vec<i32>,
    eobs: Vec<u16>,
}

impl PlaneCoeffs {
    fn new() -> Self {
        Self {
            qcoeff: vec![0; 64 * 64],
            dqcoeff: vec![0; 64 * 64],
            eobs: vec![0; 256],
        }
    }
}

/// Encodes one frame's blocks: libvpx's per-frame and per-tile encoder
/// state (`MACROBLOCK`, `MACROBLOCKD`, `TileDataEnc`, `FRAME_COUNTS`).
pub(crate) struct FrameEncoder<'a> {
    pub h: &'a FrameHeader,
    pub seg: &'a Segmentation,
    pub quants: &'a Quants,
    /// The picture being encoded, its edges repeated out to whole
    /// superblocks.
    pub src: &'a FrameBuf<u8>,
    /// What the decoder will reconstruct, block by block.
    pub recon: &'a mut FrameBuf<u8>,
    /// The blocks coded so far.
    pub mi: MiGrid,
    pub counts: EncCounts,
    /// Each tile's tokens, tile rows first.
    pub tile_tokens: Vec<Vec<TokenExtra>>,
    /// The tile being encoded: its first column of cells, and its tokens'
    /// index in `tile_tokens`.
    tile_mi_col_start: usize,
    tile_index: usize,
    /// Per plane, whether each 4x4 column above has nonzero coefficients:
    /// libvpx's `above_context`, the frame's width aligned to superblocks.
    above_ctx: [Vec<u8>; 3],
    /// Per plane, the same for the superblock's rows: `left_context`.
    left_ctx: [[u8; 16]; 3],
    coeffs: [PlaneCoeffs; 3],
    /// Scratch: one transform block's residual and coefficients, the
    /// intra predictor's output, the tokenizer's energy classes.
    diff: Vec<i16>,
    coeff: Vec<i32>,
    intra_out: intra::Prediction,
    token_cache: [u8; 1024],
}

impl<'a> FrameEncoder<'a> {
    /// A frame encoder writing to `recon`, every block yet to be coded.
    pub(crate) fn new(
        h: &'a FrameHeader,
        seg: &'a Segmentation,
        quants: &'a Quants,
        src: &'a FrameBuf<u8>,
        recon: &'a mut FrameBuf<u8>,
    ) -> Self {
        let aligned_cols = (h.mi_cols() + 7) & !7;
        let tiles = (1usize << h.log2_tile_cols) * (1usize << h.log2_tile_rows);
        Self {
            h,
            seg,
            quants,
            src,
            recon,
            mi: MiGrid::new(h.mi_cols(), h.mi_rows()),
            counts: EncCounts::default(),
            tile_tokens: vec![Vec::new(); tiles],
            tile_mi_col_start: 0,
            tile_index: 0,
            above_ctx: core::array::from_fn(|_| vec![0u8; 2 * aligned_cols]),
            left_ctx: [[0; 16]; 3],
            coeffs: core::array::from_fn(|_| PlaneCoeffs::new()),
            diff: vec![0; 32 * 32],
            coeff: vec![0; 32 * 32],
            intra_out: [[0; 32]; 32],
            token_cache: [0; 1024],
        }
    }

    /// What encoding left: the blocks, the counts and each tile's tokens.
    pub(crate) fn finish(self) -> (MiGrid, EncCounts, Vec<Vec<TokenExtra>>) {
        (self.mi, self.counts, self.tile_tokens)
    }

    /// Encode every tile: libvpx's `encode_tiles` and `vp9_encode_tile`, a
    /// superblock row at a time (`encode_nonrd_sb_row`). The above contexts
    /// start clear once, for the frame; the left ones at each superblock
    /// row.
    pub(crate) fn encode_tiles(&mut self, d: &mut dyn Decide) {
        let (mi_cols, mi_rows) = (self.h.mi_cols(), self.h.mi_rows());
        let (log2_cols, log2_rows) = (self.h.log2_tile_cols, self.h.log2_tile_rows);
        let offset = |i: usize, mis: usize, log2: u32| {
            header::tile_offset(
                u32::try_from(i).unwrap_or(u32::MAX),
                u32::try_from(mis).unwrap_or(u32::MAX),
                log2,
            ) as usize
        };
        for tile_row in 0..1usize << log2_rows {
            for tile_col in 0..1usize << log2_cols {
                self.tile_index = (tile_row << log2_cols) + tile_col;
                let col_start = offset(tile_col, mi_cols, log2_cols);
                let col_end = offset(tile_col + 1, mi_cols, log2_cols);
                self.tile_mi_col_start = col_start;
                let mut mi_row = offset(tile_row, mi_rows, log2_rows);
                let row_end = offset(tile_row + 1, mi_rows, log2_rows);
                while mi_row < row_end {
                    self.left_ctx = [[0; 16]; 3];
                    let mut mi_col = col_start;
                    while mi_col < col_end {
                        self.encode_partition(d, mi_row, mi_col, BLOCK_64X64);
                        mi_col += 8;
                    }
                    mi_row += 8;
                }
            }
        }
    }

    /// Walk one square block's partition, encoding each block it is cut
    /// into: libvpx's `encode_sb_rt`, with the partition asked of `d`.
    fn encode_partition(
        &mut self,
        d: &mut dyn Decide,
        mi_row: usize,
        mi_col: usize,
        bsize: BlockSize,
    ) {
        let (mi_cols, mi_rows) = (self.mi.mi_cols, self.mi.mi_rows);
        if mi_row >= mi_rows || mi_col >= mi_cols {
            return;
        }
        // Half the block, in cells: 4 for 64x64, 0 for 8x8.
        let hbs = usize::from(
            tables::NUM_8X8_WIDE
                .get(usize::from(bsize))
                .copied()
                .unwrap_or(1),
        ) >> 1;
        let has_rows = mi_row + hbs < mi_rows;
        let has_cols = mi_col + hbs < mi_cols;
        let mut partition = d.partition(self, mi_row, mi_col, bsize);
        let codable = match (has_rows, has_cols) {
            (true, true) => partition <= PARTITION_SPLIT,
            (false, true) => partition == PARTITION_HORZ || partition == PARTITION_SPLIT,
            (true, false) => partition == PARTITION_VERT || partition == PARTITION_SPLIT,
            (false, false) => partition == PARTITION_SPLIT,
        };
        if !codable {
            partition = PARTITION_SPLIT;
        }
        let subsize = tables::SUBSIZE
            .get(usize::from(partition))
            .and_then(|s| s.get(usize::from(bsize)))
            .copied()
            .unwrap_or(BLOCK_INVALID);
        if subsize == BLOCK_INVALID {
            debug_assert!(false, "no block size for that partition");
            return;
        }
        if hbs == 0 {
            // An 8x8 block, whole or in 4x4 parts: one block either way.
            self.encode_block(d, mi_row, mi_col, subsize);
            return;
        }
        match partition {
            PARTITION_NONE => self.encode_block(d, mi_row, mi_col, subsize),
            PARTITION_HORZ => {
                self.encode_block(d, mi_row, mi_col, subsize);
                if has_rows {
                    self.encode_block(d, mi_row + hbs, mi_col, subsize);
                }
            }
            PARTITION_VERT => {
                self.encode_block(d, mi_row, mi_col, subsize);
                if has_cols {
                    self.encode_block(d, mi_row, mi_col + hbs, subsize);
                }
            }
            _ => {
                self.encode_partition(d, mi_row, mi_col, subsize);
                self.encode_partition(d, mi_row, mi_col + hbs, subsize);
                self.encode_partition(d, mi_row + hbs, mi_col, subsize);
                self.encode_partition(d, mi_row + hbs, mi_col + hbs, subsize);
            }
        }
    }

    /// The blocks above and to the left of a cell, as the contexts see them:
    /// none off the top of the frame or left of the tile.
    fn neighbours(&self, mi_row: usize, mi_col: usize) -> (Option<usize>, Option<usize>) {
        let above = if mi_row > 0 {
            self.mi.index_at(mi_row - 1, mi_col)
        } else {
            None
        };
        let left = if mi_col > self.tile_mi_col_start {
            self.mi.index_at(mi_row, mi_col - 1)
        } else {
            None
        };
        (above, left)
    }

    /// Encode one block: libvpx's `encode_b_rt` -- record its modes
    /// (`update_state_rt`), code it (`encode_superblock`), and mark the end
    /// of its tokens.
    fn encode_block(&mut self, d: &mut dyn Decide, mi_row: usize, mi_col: usize, bsize: BlockSize) {
        let modes = d.modes(self, mi_row, mi_col, bsize);
        let h = self.h;
        let bw = usize::from(
            tables::NUM_8X8_WIDE
                .get(usize::from(bsize))
                .copied()
                .unwrap_or(1),
        );
        let bh = usize::from(
            tables::NUM_8X8_HIGH
                .get(usize::from(bsize))
                .copied()
                .unwrap_or(1),
        );
        let (mi_cols, mi_rows) = (self.mi.mi_cols, self.mi.mi_rows);
        let x_mis = bw.min(mi_cols - mi_col);
        let y_mis = bh.min(mi_rows - mi_row);
        let place = Placement {
            mi_row,
            mi_col,
            mb_to_right_edge: (mi_cols as i32 - bw as i32 - mi_col as i32) * 64,
            mb_to_bottom_edge: (mi_rows as i32 - bh as i32 - mi_row as i32) * 64,
        };

        let max_tx = tables::MAX_TXSIZE
            .get(usize::from(bsize))
            .copied()
            .unwrap_or(TX_4X4);
        let tx_size = if bsize < BLOCK_8X8 {
            TX_4X4
        } else if h.tx_mode == TX_MODE_SELECT {
            modes.tx_size.min(max_tx)
        } else {
            let biggest = tables::TX_MODE_TO_BIGGEST_TX_SIZE
                .get(usize::from(h.tx_mode))
                .copied()
                .unwrap_or(TX_4X4);
            max_tx.min(biggest)
        };
        let mut mi = ModeInfo {
            sb_type: bsize,
            mode: modes.mode,
            uv_mode: modes.uv_mode,
            tx_size,
            ref_frame: [INTRA_FRAME, NO_REF_FRAME],
            mi_row,
            mi_col,
            bw,
            bh,
            ..ModeInfo::default()
        };
        if bsize < BLOCK_8X8 {
            // A 4x8 block's two modes are quarters 0 and 1, repeated below;
            // an 8x4 block's, quarters 0 and 2, repeated to the right.
            let s = modes.sub_modes;
            let quarters = match bsize {
                crate::common::BLOCK_4X8 => [s[0], s[1], s[0], s[1]],
                crate::common::BLOCK_8X4 => [s[0], s[0], s[2], s[2]],
                _ => s,
            };
            for (b, &m) in mi.bmi.iter_mut().zip(&quarters) {
                b.mode = m;
            }
            mi.mode = quarters[3];
        }

        let (above, left) = self.neighbours(mi_row, mi_col);
        let Ok(idx) = u32::try_from(self.mi.blocks.len()) else {
            return;
        };
        self.mi.blocks.push(mi);
        for y in 0..y_mis {
            self.mi.cover(mi_row + y, mi_col, x_mis, idx);
        }

        // encode_superblock, intra: code every plane, then tokenize.
        let seg_skip = self.seg.feature_active(mi.segment_id, SEG_LVL_SKIP);
        let block8 = bsize.max(BLOCK_8X8);
        let mut skip = true;
        for plane in 0..3 {
            skip &= !self.encode_intra_plane(&mi, &place, block8, plane);
        }
        mi.skip = skip;
        if let Some(b) = self.mi.blocks.get_mut(idx as usize) {
            b.skip = skip;
        }
        // sum_intra_stats.
        let c = &mut self.counts.counts;
        if bsize < BLOCK_8X8 {
            let w4 = usize::from(
                tables::NUM_4X4_WIDE
                    .get(usize::from(bsize))
                    .copied()
                    .unwrap_or(1),
            );
            let h4 = usize::from(
                tables::NUM_4X4_HIGH
                    .get(usize::from(bsize))
                    .copied()
                    .unwrap_or(1),
            );
            for idy in (0..2).step_by(h4.max(1)) {
                for idx in (0..2).step_by(w4.max(1)) {
                    let m = mi.bmi.get(idy * 2 + idx).map_or(0, |b| usize::from(b.mode));
                    if let Some(n) = c.y_mode.get_mut(0).and_then(|y| y.get_mut(m)) {
                        *n = n.wrapping_add(1);
                    }
                }
            }
        } else {
            let group = usize::from(
                tables::SIZE_GROUP
                    .get(usize::from(bsize))
                    .copied()
                    .unwrap_or(0),
            );
            if let Some(n) = c
                .y_mode
                .get_mut(group)
                .and_then(|y| y.get_mut(usize::from(mi.mode)))
            {
                *n = n.wrapping_add(1);
            }
        }
        if let Some(n) = c
            .uv_mode
            .get_mut(usize::from(mi.mode))
            .and_then(|u| u.get_mut(usize::from(mi.uv_mode)))
        {
            *n = n.wrapping_add(1);
        }

        let a = above.and_then(|i| self.mi.blocks.get(i)).copied();
        let l = left.and_then(|i| self.mi.blocks.get(i)).copied();
        self.tokenize_sb(&mi, &place, block8, seg_skip, a.as_ref(), l.as_ref());

        // The transform size's counts.
        let tx = &mut self.counts;
        if h.tx_mode == TX_MODE_SELECT && bsize >= BLOCK_8X8 {
            let ctx = context::tx_size_context(a.as_ref(), l.as_ref(), max_tx);
            let t = &mut tx.counts.tx;
            let slot = match max_tx {
                TX_8X8 => t
                    .p8x8
                    .get_mut(ctx)
                    .and_then(|s| s.get_mut(usize::from(tx_size))),
                TX_16X16 => t
                    .p16x16
                    .get_mut(ctx)
                    .and_then(|s| s.get_mut(usize::from(tx_size))),
                TX_32X32 => t
                    .p32x32
                    .get_mut(ctx)
                    .and_then(|s| s.get_mut(usize::from(tx_size))),
                _ => None,
            };
            if let Some(n) = slot {
                *n = n.wrapping_add(1);
            }
        }
        let uv_tx = uv_tx_size(bsize, tx_size, h.ss_x, h.ss_y);
        for t in [tx_size, uv_tx] {
            if let Some(n) = tx.tx_totals.get_mut(usize::from(t)) {
                *n = n.wrapping_add(1);
            }
        }

        if let Some(tokens) = self.tile_tokens.get_mut(self.tile_index) {
            tokens.push(TokenExtra::END_OF_BLOCK_TOKENS);
        }
    }

    /// The transform blocks of one plane of a block, in coding order, as
    /// (index in 4x4 units, row and column in 4x4 units): libvpx's
    /// `vp9_foreach_transformed_block_in_plane`, which skips those wholly
    /// past the frame's edge.
    fn transform_blocks(
        &self,
        place: &Placement,
        block8: BlockSize,
        plane: usize,
        tx_size: TxSize,
    ) -> Vec<(usize, usize, usize)> {
        let (sx, sy) = self.subsampling(plane);
        let plane_bsize = plane_block_size(block8, sx, sy);
        let n4_w = i32::from(
            tables::NUM_4X4_WIDE
                .get(usize::from(plane_bsize))
                .copied()
                .unwrap_or(1),
        );
        let n4_h = i32::from(
            tables::NUM_4X4_HIGH
                .get(usize::from(plane_bsize))
                .copied()
                .unwrap_or(1),
        );
        let max_wide = n4_w
            + if place.mb_to_right_edge >= 0 {
                0
            } else {
                place.mb_to_right_edge >> (5 + sx)
            };
        let max_high = n4_h
            + if place.mb_to_bottom_edge >= 0 {
                0
            } else {
                place.mb_to_bottom_edge >> (5 + sy)
            };
        let step = 1i32 << (tx_size << 1);
        let extra_step = ((n4_w - max_wide) >> tx_size) * step;
        let mut out = Vec::new();
        let mut i = 0i32;
        let mut r = 0;
        while r < max_high {
            let mut c = 0;
            while c < max_wide {
                out.push((i.max(0) as usize, r.max(0) as usize, c.max(0) as usize));
                i += step;
                c += 1 << tx_size;
            }
            i += extra_step;
            r += 1 << tx_size;
        }
        out
    }

    fn subsampling(&self, plane: usize) -> (u32, u32) {
        if plane == 0 {
            (0, 0)
        } else {
            (u32::from(self.h.ss_x), u32::from(self.h.ss_y))
        }
    }

    /// A plane's transform size and the transform and scan of one of its
    /// transform blocks: libvpx's `get_tx_type_4x4`, `get_tx_type` and
    /// `get_scan`.
    fn transform_of(
        &self,
        mi: &ModeInfo,
        plane: usize,
        block: usize,
        tx_size: TxSize,
    ) -> (PredictionMode, TxType, Scan) {
        let mode = if plane != 0 {
            mi.uv_mode
        } else if tx_size == TX_4X4 {
            mi.y_mode(block)
        } else {
            mi.mode
        };
        let tx_type = if plane != 0 || self.h.lossless() || tx_size == TX_32X32 {
            DCT_DCT
        } else {
            INTRA_MODE_TO_TX_TYPE
                .get(usize::from(mode))
                .copied()
                .unwrap_or(DCT_DCT)
        };
        (mode, tx_type, scan_for(tx_size, tx_type))
    }

    /// Code one plane of an intra block, transform block by transform
    /// block: libvpx's `vp9_encode_intra_block_plane` and
    /// `vp9_encode_block_intra`. Returns whether any coefficient is nonzero.
    fn encode_intra_plane(
        &mut self,
        mi: &ModeInfo,
        place: &Placement,
        block8: BlockSize,
        plane: usize,
    ) -> bool {
        let h = self.h;
        let (sx, sy) = self.subsampling(plane);
        let tx_size = if plane == 0 {
            mi.tx_size
        } else {
            uv_tx_size(mi.sb_type, mi.tx_size, h.ss_x, h.ss_y)
        };
        let plane_bsize = plane_block_size(block8, sx, sy);
        let bwl = u32::from(
            tables::B_WIDTH_LOG2
                .get(usize::from(plane_bsize))
                .copied()
                .unwrap_or(0),
        );
        let qindex = self.seg.qindex(mi.segment_id, h.quant.base_qindex);
        let qs = self.quants.get(plane, usize::try_from(qindex).unwrap_or(0));
        let lossless = h.lossless();
        let n = 4usize << tx_size;
        let txw = 1usize << tx_size;
        let mut any = false;
        for (block, row, col) in self.transform_blocks(place, block8, plane, tx_size) {
            let (mode, tx_type, scan) = self.transform_of(mi, plane, block, tx_size);
            let x0 = ((place.mi_col * 8) >> sx) + 4 * col;
            let y0 = ((place.mi_row * 8) >> sy) + 4 * row;
            let p = &mut self.recon.planes[plane.min(2)];
            let edges = Edges {
                have_top: row != 0 || place.mi_row != 0,
                have_left: col != 0 || place.mi_col > self.tile_mi_col_start,
                have_right: col + txw < (1usize << bwl),
                past_right: place.mb_to_right_edge < 0,
                past_bottom: place.mb_to_bottom_edge < 0,
                frame_width: p.width,
                frame_height: p.height,
            };
            intra::predict(
                &mut p.data,
                p.stride,
                x0,
                y0,
                mode,
                tx_size,
                &edges,
                8,
                &mut self.intra_out,
            );
            // vpx_subtract_block.
            let s = &self.src.planes[plane.min(2)];
            for r in 0..n {
                let (srow, prow) = (
                    s.data.get((y0 + r) * s.stride + x0..).unwrap_or(&[]),
                    p.data.get((y0 + r) * p.stride + x0..).unwrap_or(&[]),
                );
                let drow = self.diff.get_mut(r * n..(r + 1) * n).unwrap_or(&mut []);
                for ((d, &a), &b) in drow.iter_mut().zip(srow).zip(prow) {
                    *d = i16::from(a) - i16::from(b);
                }
            }
            forward_transform(&self.diff, n, tx_size, tx_type, lossless, &mut self.coeff);
            let start = block * 16;
            let pc = &mut self.coeffs[plane.min(2)];
            let (Some(qcoeff), Some(dqcoeff)) = (
                pc.qcoeff.get_mut(start..start + n * n),
                pc.dqcoeff.get_mut(start..start + n * n),
            ) else {
                continue;
            };
            let eob = quantize_b(
                &self.coeff,
                n * n,
                &qs,
                qcoeff,
                dqcoeff,
                scan.scan,
                tx_size == TX_32X32,
            );
            if let Some(e) = pc.eobs.get_mut(block) {
                *e = u16::try_from(eob).unwrap_or(u16::MAX);
            }
            if eob > 0 {
                any = true;
                if let Some(dst) = p.data.get_mut(y0 * p.stride + x0..) {
                    idct::inverse_transform_add(
                        tx_size, tx_type, lossless, eob, dqcoeff, dst, p.stride, 8,
                    );
                }
            }
        }
        any
    }

    /// Tokenize the block, or clear its contexts if it is skipped: libvpx's
    /// `vp9_tokenize_sb`, which also counts the skip flag.
    fn tokenize_sb(
        &mut self,
        mi: &ModeInfo,
        place: &Placement,
        block8: BlockSize,
        seg_skip: bool,
        above: Option<&ModeInfo>,
        left: Option<&ModeInfo>,
    ) {
        let ctx = context::skip_context(above, left);
        let skip_counts = self.counts.counts.skip.get_mut(ctx);
        if mi.skip {
            if !seg_skip && let Some(s) = skip_counts {
                s[1] = s[1].wrapping_add(1);
            }
            self.reset_skip_context(place, block8);
            return;
        }
        if let Some(s) = skip_counts {
            s[0] = s[0].wrapping_add(1);
        }
        let h = self.h;
        for plane in 0..3 {
            let (sx, sy) = self.subsampling(plane);
            let tx_size = if plane == 0 {
                mi.tx_size
            } else {
                uv_tx_size(mi.sb_type, mi.tx_size, h.ss_x, h.ss_y)
            };
            let plane_bsize = plane_block_size(block8, sx, sy);
            let n4_w = i32::from(
                tables::NUM_4X4_WIDE
                    .get(usize::from(plane_bsize))
                    .copied()
                    .unwrap_or(1),
            );
            let n4_h = i32::from(
                tables::NUM_4X4_HIGH
                    .get(usize::from(plane_bsize))
                    .copied()
                    .unwrap_or(1),
            );
            let a0 = (place.mi_col * 2) >> sx;
            let l0 = ((place.mi_row * 2) & 15) >> sy;
            let tx = usize::from(tx_size.min(TX_32X32));
            for (block, row, col) in self.transform_blocks(place, block8, plane, tx_size) {
                let (_, _, scan) = self.transform_of(mi, plane, block, tx_size);
                let n = 1usize << tx_size;
                let any = |s: Option<&[u8]>| s.is_some_and(|s| s.iter().any(|&v| v != 0));
                let pt = usize::from(any(self.above_ctx[plane.min(2)].get(a0 + col..a0 + col + n)))
                    + usize::from(any(self.left_ctx[plane.min(2)].get(l0 + row..l0 + row + n)));
                let pc = &self.coeffs[plane.min(2)];
                let eob = usize::from(pc.eobs.get(block).copied().unwrap_or(0));
                let side = 16 << (tx_size << 1);
                let qcoeff = pc.qcoeff.get(block * 16..block * 16 + side).unwrap_or(&[]);
                if let (Some(tokens), Some(counts), Some(eob_branch)) = (
                    self.tile_tokens.get_mut(self.tile_index),
                    self.counts.coef.get_mut(tx),
                    self.counts.counts.eob_branch.get_mut(tx),
                ) {
                    let mut sink = TokenSink {
                        tokens,
                        counts,
                        eob_branch,
                        token_cache: &mut self.token_cache,
                    };
                    tokenize_b(
                        qcoeff,
                        eob,
                        tx_size,
                        usize::from(plane > 0),
                        0,
                        pt,
                        &scan,
                        &mut sink,
                    );
                }
                // vp9_set_contexts: the flag for the 4x4 columns and rows
                // inside the frame, 0 past its edge.
                let flag = u8::from(eob > 0);
                let keep = |edge: i32, n4: i32, start: usize, ss: u32| -> usize {
                    if flag != 0 && edge < 0 {
                        let blocks = n4 + (edge >> (5 + ss));
                        (blocks - start as i32).clamp(0, n as i32) as usize
                    } else {
                        n
                    }
                };
                let keep_a = keep(place.mb_to_right_edge, n4_w, col, sx);
                let keep_l = keep(place.mb_to_bottom_edge, n4_h, row, sy);
                if let Some(a) = self.above_ctx[plane.min(2)].get_mut(a0 + col..a0 + col + n) {
                    for (i, v) in a.iter_mut().enumerate() {
                        *v = if i < keep_a { flag } else { 0 };
                    }
                }
                if let Some(l) = self.left_ctx[plane.min(2)].get_mut(l0 + row..l0 + row + n) {
                    for (i, v) in l.iter_mut().enumerate() {
                        *v = if i < keep_l { flag } else { 0 };
                    }
                }
            }
        }
    }

    /// Clear a skipped block's entropy contexts: libvpx's
    /// `reset_skip_context`.
    fn reset_skip_context(&mut self, place: &Placement, block8: BlockSize) {
        for plane in 0..3 {
            let (sx, sy) = self.subsampling(plane);
            let plane_bsize = plane_block_size(block8, sx, sy);
            let n4_w = usize::from(
                tables::NUM_4X4_WIDE
                    .get(usize::from(plane_bsize))
                    .copied()
                    .unwrap_or(1),
            );
            let n4_h = usize::from(
                tables::NUM_4X4_HIGH
                    .get(usize::from(plane_bsize))
                    .copied()
                    .unwrap_or(1),
            );
            let a0 = (place.mi_col * 2) >> sx;
            let l0 = ((place.mi_row * 2) & 15) >> sy;
            for v in self.above_ctx[plane.min(2)].iter_mut().skip(a0).take(n4_w) {
                *v = 0;
            }
            for v in self.left_ctx[plane.min(2)].iter_mut().skip(l0).take(n4_h) {
                *v = 0;
            }
        }
    }
}

/// A block size as one plane sees it: libvpx's `get_plane_block_size`.
fn plane_block_size(bsize: BlockSize, sx: u32, sy: u32) -> BlockSize {
    tables::SS_SIZE
        .get(usize::from(bsize))
        .and_then(|s| s.get(sx as usize))
        .and_then(|s| s.get(sy as usize))
        .copied()
        .unwrap_or(BLOCK_INVALID)
}

/// The forward transform of an `n` x `n` residual: libvpx's choice in
/// `vp9_encode_block_intra` -- the Walsh-Hadamard transform for lossless
/// 4x4 blocks, the hybrid transforms where the mode asks, and for 32x32 the
/// reduced-precision DCT the realtime speeds use (`use_lp32x32fdct`).
fn forward_transform(
    diff: &[i16],
    n: usize,
    tx_size: TxSize,
    tx_type: TxType,
    lossless: bool,
    out: &mut [i32],
) {
    match tx_size {
        TX_4X4 => {
            let mut c = [0i32; 16];
            if lossless {
                fdct::fwht4x4(diff, n, &mut c);
            } else if tx_type == DCT_DCT {
                fdct::fdct4x4(diff, n, &mut c);
            } else {
                fdct::fht4x4(diff, n, &mut c, tx_type);
            }
            copy_into(out, &c);
        }
        TX_8X8 => {
            let mut c = [0i32; 64];
            fdct::fht8x8(diff, n, &mut c, tx_type);
            copy_into(out, &c);
        }
        TX_16X16 => {
            let mut c = [0i32; 256];
            fdct::fht16x16(diff, n, &mut c, tx_type);
            copy_into(out, &c);
        }
        _ => {
            let mut c = [0i32; 1024];
            fdct::fdct32x32(diff, n, &mut c, true);
            copy_into(out, &c);
        }
    }
}

fn copy_into(out: &mut [i32], c: &[i32]) {
    if let Some(o) = out.get_mut(..c.len()) {
        o.copy_from_slice(c);
    }
}

/// The skeleton's decisions: every superblock cut into 16x16 blocks (8x8
/// where a 16x16 one would cross the frame's edge), each predicted from its
/// neighbours' mean (`DC_PRED`). libvpx's realtime choices replace these.
pub(crate) struct FixedDecisions;

impl Decide for FixedDecisions {
    fn partition(
        &mut self,
        f: &FrameEncoder<'_>,
        mi_row: usize,
        mi_col: usize,
        bsize: BlockSize,
    ) -> Partition {
        let n = usize::from(
            tables::NUM_8X8_WIDE
                .get(usize::from(bsize))
                .copied()
                .unwrap_or(1),
        );
        let fits = mi_row + n <= f.mi.mi_rows && mi_col + n <= f.mi.mi_cols;
        if bsize <= crate::common::BLOCK_16X16 && fits || bsize == BLOCK_8X8 {
            PARTITION_NONE
        } else {
            PARTITION_SPLIT
        }
    }

    fn modes(
        &mut self,
        _f: &FrameEncoder<'_>,
        _mi_row: usize,
        _mi_col: usize,
        _bsize: BlockSize,
    ) -> BlockModes {
        BlockModes::default()
    }
}
