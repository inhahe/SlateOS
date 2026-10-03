//! Tiles, partitions and blocks: everything after a frame's headers.
//!
//! A frame is cut into tiles, each coded independently with its own bool
//! decoder; a tile into 64x64 superblocks; a superblock, by a recursive
//! partition, into blocks. Each block carries its mode information --
//! segment, skip flag, transform size, prediction modes, references and
//! motion vectors -- then, unless it is skipped, its coefficients. This module
//! reads all of it and reconstructs each block as it goes: intra blocks
//! transform block by transform block, each predicted from the pixels just
//! decoded; inter blocks predicted whole from the reference frames, then their
//! residual added.
//!
//! The structure is libvpx's: `decode_tiles`, `decode_partition` and
//! `decode_block` from `vp9_decodeframe.c`, and the mode information readers
//! of `vp9_decodemv.c`, including its motion vector prediction
//! (`dec_find_mv_refs`). The contexts each decision is coded with are
//! `vp9_pred_common.c`'s.
//!
//! Tiles of one tile row are independent -- nothing in one reads anything in
//! another -- so they are decoded one after another here where libvpx
//! interleaves them a superblock row at a time; the result is the same.
//!
//! Translated into Rust from libvpx v1.17.0's `vp9/decoder/vp9_decodeframe.c`,
//! `vp9/decoder/vp9_decodemv.c`, `vp9/common/vp9_pred_common.c`,
//! `vp9_pred_common.h`, `vp9_mvref_common.h`, `vp9_blockd.c`, `vp9_blockd.h`,
//! `vp9_onyxc_int.h` and `vp9_entropymv.c` (copyright the WebM project
//! authors), used under libvpx's BSD licence and patent grant
//! (`licenses/libvpx-LICENSE`, `licenses/libvpx-PATENTS`).

#![allow(
    clippy::indexing_slicing,
    reason = "indices are into fixed-size arrays and bounded by construction: planes by plane.min(2), coefficient probabilities by tx.min(3) and 0/1 plane and reference types, sub-blocks below 4, references below 2, search positions below 8, candidate lists at most index 1; positions read from the stream go through get"
)]
#![allow(
    clippy::arithmetic_side_effects,
    reason = "positions are mode-info coordinates bounded by the frame (at most 8192 by 8192) and small block arithmetic; values from the stream are bounded by the trees and literals they are read with, and motion vectors wrap as libvpx's 16-bit fields do"
)]

use std::sync::Arc;

use crate::Error;
use crate::boolread::BoolReader;
use crate::common::{
    ALTREF_FRAME, BLOCK_4X4, BLOCK_4X8, BLOCK_8X4, BLOCK_8X8, BLOCK_64X64, BLOCK_INVALID,
    BlockSize, CLASS0_SIZE, COMPOUND_REFERENCE, DC_PRED, DCT_DCT, GOLDEN_FRAME, INTER_MODE_TREE,
    INTRA_FRAME, INTRA_MODE_TO_TX_TYPE, INTRA_MODE_TREE, InterpFilter, LAST_FRAME, MAX_REF_FRAMES,
    MI_MASK, MV_CLASS_TREE, MV_FP_TREE, MV_JOINT_TREE, MV_LOW, MV_UPP, Mv, NEARESTMV, NEARMV,
    NEWMV, NO_REF_FRAME, PARTITION_HORZ, PARTITION_NONE, PARTITION_PLOFFSET, PARTITION_SPLIT,
    PARTITION_TREE, PARTITION_VERT, PredictionMode, REFERENCE_MODE_SELECT, RefFrame, ReferenceMode,
    SEG_LVL_REF_FRAME, SEG_LVL_SKIP, SEGMENT_TREE, SWITCHABLE, SWITCHABLE_FILTERS,
    SWITCHABLE_INTERP_TREE, TX_4X4, TX_8X8, TX_16X16, TX_32X32, TX_MODE_SELECT, TxMode, TxSize,
    ZEROMV,
};
use crate::detokenize::{self, BlockCounts, Scan};
use crate::frame::{AnyFrame, FrameBuf, Pixel};
use crate::header::{self, Segmentation};
use crate::idct;
use crate::inter::{self, McScratch, ScaleFactors};
use crate::intra::{self, Edges};
use crate::probs::{Counts, FrameContext, MvComponentCounts, MvCounts};
use crate::tables;

// --- What a frame is decoded with ----------------------------------------------------

/// Everything about the frame that the blocks need: its headers' results.
#[derive(Clone, Debug)]
pub(crate) struct FrameInfo {
    pub mi_cols: usize,
    pub mi_rows: usize,
    pub bit_depth: u8,
    pub ss_x: u8,
    pub ss_y: u8,
    /// A key frame or an intra-only frame.
    pub intra_only: bool,
    pub lossless: bool,
    pub tx_mode: TxMode,
    pub interp_filter: InterpFilter,
    pub allow_high_precision_mv: bool,
    pub reference_mode: ReferenceMode,
    pub comp_fixed_ref: RefFrame,
    pub comp_var_ref: [RefFrame; 2],
    pub ref_frame_sign_bias: [bool; MAX_REF_FRAMES],
    pub log2_tile_cols: u32,
    pub log2_tile_rows: u32,
    pub seg: Segmentation,
    /// Per segment: Y DC, Y AC, UV DC, UV AC.
    pub dequant: [[i16; 4]; 8],
    /// Whether to count symbols for adaptation (not frame-parallel).
    pub count: bool,
}

/// A reference frame, as the current frame sees it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct RefInfo<'a> {
    pub frame: &'a Arc<AnyFrame>,
    pub sf: ScaleFactors,
}

/// What one 8x8 cell leaves for the next frame's motion vector prediction:
/// libvpx's `MV_REF`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct MvRef {
    pub mv: [Mv; 2],
    pub ref_frame: [RefFrame; 2],
}

impl Default for MvRef {
    fn default() -> Self {
        Self {
            mv: [Mv::ZERO; 2],
            ref_frame: [INTRA_FRAME, NO_REF_FRAME],
        }
    }
}

/// A sub-8x8 block's mode and motion vectors: libvpx's `b_mode_info`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Bmi {
    pub mode: PredictionMode,
    pub mv: [Mv; 2],
}

/// One block's mode information: libvpx's `MODE_INFO`, with where it is.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ModeInfo {
    pub sb_type: BlockSize,
    pub mode: PredictionMode,
    pub tx_size: TxSize,
    pub skip: bool,
    pub segment_id: u8,
    pub seg_id_predicted: bool,
    pub uv_mode: PredictionMode,
    pub interp_filter: InterpFilter,
    pub ref_frame: [RefFrame; 2],
    pub mv: [Mv; 2],
    pub bmi: [Bmi; 4],
    /// The block's top-left cell and its size in cells.
    pub mi_row: usize,
    pub mi_col: usize,
    pub bw: usize,
    pub bh: usize,
}

impl ModeInfo {
    /// libvpx's `is_inter_block`.
    pub fn is_inter(&self) -> bool {
        self.ref_frame[0] > INTRA_FRAME
    }

    /// libvpx's `has_second_ref`.
    pub fn has_second_ref(&self) -> bool {
        self.ref_frame[1] > INTRA_FRAME
    }

    /// The luma mode of sub-block `block`: libvpx's `get_y_mode`.
    fn y_mode(&self, block: usize) -> PredictionMode {
        if self.sb_type < BLOCK_8X8 {
            self.bmi.get(block).map_or(self.mode, |b| b.mode)
        } else {
            self.mode
        }
    }
}

/// Which block covers each 8x8 cell of the frame: libvpx's
/// `mi_grid_visible`, as indices into the frame's blocks.
#[derive(Clone, Debug)]
pub(crate) struct MiGrid {
    pub mi_cols: usize,
    pub mi_rows: usize,
    cells: Vec<u32>,
    pub blocks: Vec<ModeInfo>,
}

/// A cell no block has covered yet.
const NO_BLOCK: u32 = u32::MAX;

impl MiGrid {
    fn new(mi_cols: usize, mi_rows: usize) -> Self {
        Self {
            mi_cols,
            mi_rows,
            cells: vec![NO_BLOCK; mi_cols * mi_rows],
            blocks: Vec::new(),
        }
    }

    /// The block covering the cell, if it has been decoded.
    pub fn at(&self, mi_row: usize, mi_col: usize) -> Option<&ModeInfo> {
        if mi_row >= self.mi_rows || mi_col >= self.mi_cols {
            return None;
        }
        let idx = *self.cells.get(mi_row * self.mi_cols + mi_col)?;
        self.blocks.get(idx as usize)
    }

    fn index_at(&self, mi_row: usize, mi_col: usize) -> Option<usize> {
        if mi_row >= self.mi_rows || mi_col >= self.mi_cols {
            return None;
        }
        self.cells
            .get(mi_row * self.mi_cols + mi_col)
            .copied()
            .filter(|&i| i != NO_BLOCK)
            .map(|i| i as usize)
    }
}

/// What decoding the tiles left: the blocks, for the loop filter, and where
/// the frame's data ended.
pub(crate) struct Decoded {
    pub mi: MiGrid,
    /// The first byte of the tile data the last tile's decoder did not use:
    /// libvpx's `vpx_reader_find_end`.
    pub end_of_data: usize,
}

// --- Tiles --------------------------------------------------------------------------

/// A tile's extent in mode-info units: libvpx's `TileInfo`.
#[derive(Clone, Copy, Debug)]
struct TileInfo {
    mi_col_start: usize,
    mi_col_end: usize,
}

/// Split the tile data into one buffer per tile: libvpx's `get_tile_buffers`.
/// Every tile but the last starts with its size, big-endian; the last runs
/// to the end.
fn tile_buffers(data: &[u8], rows: usize, cols: usize) -> Result<Vec<(usize, &[u8])>, Error> {
    let mut out = Vec::with_capacity(rows * cols);
    let mut pos = 0usize;
    for r in 0..rows {
        for c in 0..cols {
            let last = r + 1 == rows && c + 1 == cols;
            let size = if last {
                data.len().saturating_sub(pos)
            } else {
                let bytes = data
                    .get(pos..pos + 4)
                    .ok_or(Error::Corrupt("truncated packet or corrupt tile length"))?;
                pos += 4;
                u32::from_be_bytes(bytes.try_into().unwrap_or([0; 4])) as usize
            };
            let buf = data
                .get(pos..)
                .and_then(|d| d.get(..size))
                .ok_or(Error::Corrupt("truncated packet or corrupt tile size"))?;
            if size == 0 {
                return Err(Error::Corrupt("truncated packet or corrupt tile length"));
            }
            out.push((pos, buf));
            pos += size;
        }
    }
    Ok(out)
}

/// Decode every tile of the frame into `frame`: libvpx's `decode_tiles`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn decode_tiles(
    info: &FrameInfo,
    fc: &FrameContext,
    refs: &[Option<RefInfo<'_>>],
    prev_mvs: Option<&[MvRef]>,
    last_seg_map: &[u8],
    cur_seg_map: &mut [u8],
    cur_mvs: &mut [MvRef],
    counts: &mut Counts,
    data: &[u8],
    frame: &mut AnyFrame,
) -> Result<Decoded, Error> {
    match frame {
        AnyFrame::Eight(f) => decode_tiles_t(
            info,
            fc,
            refs,
            prev_mvs,
            last_seg_map,
            cur_seg_map,
            cur_mvs,
            counts,
            data,
            f,
        ),
        AnyFrame::High(f) => decode_tiles_t(
            info,
            fc,
            refs,
            prev_mvs,
            last_seg_map,
            cur_seg_map,
            cur_mvs,
            counts,
            data,
            f,
        ),
    }
}

#[allow(clippy::too_many_arguments)]
fn decode_tiles_t<P: Pixel>(
    info: &FrameInfo,
    fc: &FrameContext,
    refs: &[Option<RefInfo<'_>>],
    prev_mvs: Option<&[MvRef]>,
    last_seg_map: &[u8],
    cur_seg_map: &mut [u8],
    cur_mvs: &mut [MvRef],
    counts: &mut Counts,
    data: &[u8],
    frame: &mut FrameBuf<P>,
) -> Result<Decoded, Error> {
    let mut ref_frames: [Option<(&FrameBuf<P>, ScaleFactors)>; 3] = [None, None, None];
    if !info.intra_only {
        for (slot, r) in ref_frames.iter_mut().zip(refs) {
            if let Some(r) = r {
                let f = P::from_any(r.frame).ok_or(Error::Corrupt(
                    "a reference frame has an incompatible colour format",
                ))?;
                *slot = Some((f, r.sf));
            }
        }
    }
    let tile_cols = 1usize << info.log2_tile_cols;
    let tile_rows = 1usize << info.log2_tile_rows;
    let buffers = tile_buffers(data, tile_rows, tile_cols)?;
    let aligned_cols = (info.mi_cols + 7) & !7;
    let mut d = Dec {
        info,
        fc,
        refs: ref_frames,
        prev_mvs,
        last_seg_map,
        cur_seg_map,
        cur_mvs,
        counts: if info.count { Some(counts) } else { None },
        mi: MiGrid::new(info.mi_cols, info.mi_rows),
        frame,
        above_ctx: core::array::from_fn(|_| vec![0u8; 2 * aligned_cols]),
        above_seg: vec![0u8; aligned_cols],
        left_ctx: [[0; 16]; 3],
        left_seg: [0; 8],
        token_cache: [0; 1024],
        dqcoeff: vec![0; 32 * 32],
        mc: McScratch::new(),
        intra_out: [[0; 32]; 32],
        tile: TileInfo {
            mi_col_start: 0,
            mi_col_end: 0,
        },
    };
    let mut end_of_data = 0usize;
    for tile_row in 0..tile_rows {
        let mi_row_start =
            header::tile_offset(tile_row as u32, info.mi_rows as u32, info.log2_tile_rows) as usize;
        let mi_row_end = header::tile_offset(
            tile_row as u32 + 1,
            info.mi_rows as u32,
            info.log2_tile_rows,
        ) as usize;
        for tile_col in 0..tile_cols {
            let (start, buf) = *buffers
                .get(tile_row * tile_cols + tile_col)
                .ok_or(Error::Corrupt("a tile is missing"))?;
            d.tile = TileInfo {
                mi_col_start: header::tile_offset(
                    tile_col as u32,
                    info.mi_cols as u32,
                    info.log2_tile_cols,
                ) as usize,
                mi_col_end: header::tile_offset(
                    tile_col as u32 + 1,
                    info.mi_cols as u32,
                    info.log2_tile_cols,
                ) as usize,
            };
            let mut r = BoolReader::new(buf)?;
            let mut mi_row = mi_row_start;
            while mi_row < mi_row_end {
                d.left_ctx = [[0; 16]; 3];
                d.left_seg = [0; 8];
                let mut mi_col = d.tile.mi_col_start;
                while mi_col < d.tile.mi_col_end {
                    d.decode_partition(&mut r, mi_row, mi_col, BLOCK_64X64, 4)?;
                    mi_col += 8;
                }
                if r.has_error() {
                    return Err(Error::Corrupt("failed to decode tile data"));
                }
                mi_row += 8;
            }
            end_of_data = start + r.find_end();
        }
    }
    Ok(Decoded {
        mi: d.mi,
        end_of_data,
    })
}

// --- The decoder of one frame's blocks ------------------------------------------------

/// The state one frame's block decoding works on: libvpx's `VP9_COMMON` and
/// one tile's `TileWorkerData` and `MACROBLOCKD`, cut down to what blocks use.
struct Dec<'a, P: Pixel> {
    info: &'a FrameInfo,
    fc: &'a FrameContext,
    refs: [Option<(&'a FrameBuf<P>, ScaleFactors)>; 3],
    prev_mvs: Option<&'a [MvRef]>,
    last_seg_map: &'a [u8],
    cur_seg_map: &'a mut [u8],
    cur_mvs: &'a mut [MvRef],
    counts: Option<&'a mut Counts>,
    mi: MiGrid,
    frame: &'a mut FrameBuf<P>,
    /// Per plane: whether each 4x4 column above has nonzero coefficients.
    above_ctx: [Vec<u8>; 3],
    /// Partition contexts of each 8x8 column.
    above_seg: Vec<u8>,
    left_ctx: [[u8; 16]; 3],
    left_seg: [u8; 8],
    token_cache: [u8; 1024],
    dqcoeff: Vec<i32>,
    /// Inter prediction's scratch space, reused from block to block.
    mc: McScratch<P>,
    /// Intra prediction's scratch block, likewise.
    intra_out: intra::Prediction,
    tile: TileInfo,
}

/// Where a block is, as libvpx's `MACROBLOCKD` describes it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct BlockPos {
    pub mi_row: usize,
    pub mi_col: usize,
    /// Size in mode-info units.
    pub bw: usize,
    pub bh: usize,
    /// log2 of the width in 4x4 units: libvpx's `bwl`.
    pub bwl: u32,
    /// Distances to the frame's edges in eighth pixels: libvpx's
    /// `mb_to_*_edge`. Negative right and bottom distances mean the block
    /// reaches past the frame.
    pub mb_to_left_edge: i32,
    pub mb_to_right_edge: i32,
    pub mb_to_top_edge: i32,
    pub mb_to_bottom_edge: i32,
}

impl<P: Pixel> Dec<'_, P> {
    fn counts(&mut self) -> Option<&mut Counts> {
        self.counts.as_deref_mut()
    }

    // --- Partitions ---------------------------------------------------------------

    /// libvpx's `dec_partition_plane_context`.
    fn partition_context(&self, mi_row: usize, mi_col: usize, bsl: usize) -> usize {
        let above = usize::from(self.above_seg.get(mi_col).copied().unwrap_or(0) >> bsl) & 1;
        let left = usize::from(
            self.left_seg
                .get(mi_row & MI_MASK as usize)
                .copied()
                .unwrap_or(0)
                >> bsl,
        ) & 1;
        (left * 2 + above) + bsl * PARTITION_PLOFFSET
    }

    /// libvpx's `read_partition`.
    fn read_partition(
        &mut self,
        r: &mut BoolReader<'_>,
        mi_row: usize,
        mi_col: usize,
        has_rows: bool,
        has_cols: bool,
        bsl: usize,
    ) -> u8 {
        let ctx = self.partition_context(mi_row, mi_col, bsl);
        let probs = if self.info.intra_only {
            tables::KF_PARTITION_PROBS.get(ctx).copied()
        } else {
            self.fc.partition_prob.get(ctx).copied()
        }
        .unwrap_or([128; 3]);
        let p = if has_rows && has_cols {
            r.read_tree(&PARTITION_TREE, &probs) as u8
        } else if !has_rows && has_cols {
            if r.read_bool(probs[1]) {
                PARTITION_SPLIT
            } else {
                PARTITION_HORZ
            }
        } else if has_rows && !has_cols {
            if r.read_bool(probs[2]) {
                PARTITION_SPLIT
            } else {
                PARTITION_VERT
            }
        } else {
            PARTITION_SPLIT
        };
        if let Some(c) = self.counts()
            && let Some(slot) = c
                .partition
                .get_mut(ctx)
                .and_then(|s| s.get_mut(usize::from(p)))
        {
            *slot = slot.wrapping_add(1);
        }
        p
    }

    /// libvpx's `decode_partition`.
    fn decode_partition(
        &mut self,
        r: &mut BoolReader<'_>,
        mi_row: usize,
        mi_col: usize,
        bsize: BlockSize,
        n4x4_l2: u32,
    ) -> Result<(), Error> {
        let n8x8_l2 = n4x4_l2 - 1;
        let num_8x8_wh = 1usize << n8x8_l2;
        let hbs = num_8x8_wh >> 1;
        if mi_row >= self.info.mi_rows || mi_col >= self.info.mi_cols {
            return Ok(());
        }
        let has_rows = mi_row + hbs < self.info.mi_rows;
        let has_cols = mi_col + hbs < self.info.mi_cols;
        let partition =
            self.read_partition(r, mi_row, mi_col, has_rows, has_cols, n8x8_l2 as usize);
        let subsize = tables::SUBSIZE
            .get(usize::from(partition))
            .and_then(|s| s.get(usize::from(bsize)))
            .copied()
            .unwrap_or(BLOCK_INVALID);
        if hbs == 0 {
            // An 8x8 block, possibly cut into 4x4 parts: their dimensions.
            let wl = 1 >> u32::from(partition & PARTITION_VERT != 0);
            let hl = 1 >> u32::from(partition & PARTITION_HORZ != 0);
            self.decode_block(r, mi_row, mi_col, subsize, 1, 1, (wl, hl))?;
        } else {
            match partition {
                PARTITION_NONE => {
                    self.decode_block(r, mi_row, mi_col, subsize, n4x4_l2, n4x4_l2, (1, 1))?;
                }
                PARTITION_HORZ => {
                    self.decode_block(r, mi_row, mi_col, subsize, n4x4_l2, n8x8_l2, (1, 1))?;
                    if has_rows {
                        self.decode_block(
                            r,
                            mi_row + hbs,
                            mi_col,
                            subsize,
                            n4x4_l2,
                            n8x8_l2,
                            (1, 1),
                        )?;
                    }
                }
                PARTITION_VERT => {
                    self.decode_block(r, mi_row, mi_col, subsize, n8x8_l2, n4x4_l2, (1, 1))?;
                    if has_cols {
                        self.decode_block(
                            r,
                            mi_row,
                            mi_col + hbs,
                            subsize,
                            n8x8_l2,
                            n4x4_l2,
                            (1, 1),
                        )?;
                    }
                }
                _ => {
                    self.decode_partition(r, mi_row, mi_col, subsize, n8x8_l2)?;
                    self.decode_partition(r, mi_row, mi_col + hbs, subsize, n8x8_l2)?;
                    self.decode_partition(r, mi_row + hbs, mi_col, subsize, n8x8_l2)?;
                    self.decode_partition(r, mi_row + hbs, mi_col + hbs, subsize, n8x8_l2)?;
                }
            }
        }
        // Update the partition context.
        if bsize >= BLOCK_8X8 && (bsize == BLOCK_8X8 || partition != PARTITION_SPLIT) {
            let [above, left] = tables::PARTITION_CONTEXT_LOOKUP
                .get(usize::from(subsize))
                .copied()
                .unwrap_or([0, 0]);
            for a in self.above_seg.iter_mut().skip(mi_col).take(num_8x8_wh) {
                *a = above;
            }
            for l in self.left_seg.iter_mut().skip(mi_row & 7).take(num_8x8_wh) {
                *l = left;
            }
        }
        Ok(())
    }

    // --- Blocks ---------------------------------------------------------------------

    /// libvpx's `decode_block`. `bmode` is the sub-8x8 partition's
    /// dimensions (log2 of 4x4 units per part), libvpx's `bmode_blocks_wl`
    /// and `_hl`.
    fn decode_block(
        &mut self,
        r: &mut BoolReader<'_>,
        mi_row: usize,
        mi_col: usize,
        bsize: BlockSize,
        bwl: u32,
        bhl: u32,
        bmode: (u32, u32),
    ) -> Result<(), Error> {
        let info = self.info;
        let bw = 1usize << (bwl - 1);
        let bh = 1usize << (bhl - 1);
        let x_mis = bw.min(info.mi_cols - mi_col);
        let y_mis = bh.min(info.mi_rows - mi_row);
        let pos = BlockPos {
            mi_row,
            mi_col,
            bw,
            bh,
            bwl,
            mb_to_left_edge: -((mi_col * 64) as i32),
            mb_to_right_edge: (info.mi_cols as i32 - bw as i32 - mi_col as i32) * 64,
            mb_to_top_edge: -((mi_row * 64) as i32),
            mb_to_bottom_edge: (info.mi_rows as i32 - bh as i32 - mi_row as i32) * 64,
        };
        if bsize >= BLOCK_8X8
            && (info.ss_x != 0 || info.ss_y != 0)
            && tables::SS_SIZE
                .get(usize::from(bsize))
                .and_then(|s| s.get(usize::from(info.ss_x)))
                .and_then(|s| s.get(usize::from(info.ss_y)))
                .is_none_or(|&b| b == BLOCK_INVALID)
        {
            return Err(Error::Corrupt("invalid block size"));
        }

        let above = if mi_row != 0 {
            self.mi.index_at(mi_row - 1, mi_col)
        } else {
            None
        };
        let left = if mi_col > self.tile.mi_col_start {
            self.mi.index_at(mi_row, mi_col - 1)
        } else {
            None
        };
        let mut mi = ModeInfo {
            sb_type: bsize,
            mi_row,
            mi_col,
            bw,
            bh,
            ..ModeInfo::default()
        };
        self.read_mode_info(r, &mut mi, &pos, above, left, x_mis, y_mis, bmode)?;

        // Record the block for the cells it covers.
        let idx =
            u32::try_from(self.mi.blocks.len()).map_err(|_| Error::Corrupt("too many blocks"))?;
        self.mi.blocks.push(mi);
        for y in 0..y_mis {
            let row = (mi_row + y) * info.mi_cols + mi_col;
            if let Some(cells) = self.mi.cells.get_mut(row..row + x_mis) {
                cells.fill(idx);
            }
        }

        if mi.skip {
            self.reset_skip_context(&pos);
        }
        let skip_lf = if mi.is_inter() {
            inter::build_inter_predictors_sb(
                self.frame,
                &self.refs,
                &mi,
                &pos,
                info.bit_depth,
                &mut self.mc,
            )?;
            if mi.skip {
                false
            } else {
                let eobtotal = self.reconstruct(r, &mi, &pos, false);
                bsize >= BLOCK_8X8 && eobtotal == 0
            }
        } else {
            self.reconstruct(r, &mi, &pos, true);
            false
        };
        if skip_lf && let Some(b) = self.mi.blocks.get_mut(idx as usize) {
            // No residual anywhere: the loop filter treats it as skipped.
            b.skip = true;
        }
        if r.has_error() {
            return Err(Error::Corrupt("failed to decode tile data"));
        }
        Ok(())
    }

    /// libvpx's `dec_reset_skip_context`.
    fn reset_skip_context(&mut self, pos: &BlockPos) {
        let (ssx, ssy) = (u32::from(self.info.ss_x), u32::from(self.info.ss_y));
        for plane in 0..3usize {
            let (sx, sy) = if plane == 0 { (0, 0) } else { (ssx, ssy) };
            let n4_w = (pos.bw << 1) >> sx;
            let n4_h = (pos.bh << 1) >> sy;
            let a0 = (pos.mi_col * 2) >> sx;
            let l0 = ((pos.mi_row * 2) & 15) >> sy;
            if let Some(a) = self.above_ctx.get_mut(plane) {
                for v in a.iter_mut().skip(a0).take(n4_w) {
                    *v = 0;
                }
            }
            if let Some(l) = self.left_ctx.get_mut(plane) {
                for v in l.iter_mut().skip(l0).take(n4_h) {
                    *v = 0;
                }
            }
        }
    }

    /// Predict (intra) and add the residual of every transform block in the
    /// frame: libvpx's loops over `predict_and_reconstruct_intra_block` and
    /// `reconstruct_inter_block`. Returns the total end-of-block count.
    fn reconstruct(
        &mut self,
        r: &mut BoolReader<'_>,
        mi: &ModeInfo,
        pos: &BlockPos,
        intra: bool,
    ) -> usize {
        let info = self.info;
        let mut eobtotal = 0usize;
        for plane in 0..3usize {
            let (sx, sy) = if plane == 0 {
                (0, 0)
            } else {
                (u32::from(info.ss_x), u32::from(info.ss_y))
            };
            let tx_size = if plane == 0 {
                mi.tx_size
            } else {
                uv_tx_size(mi.sb_type, mi.tx_size, info.ss_x, info.ss_y)
            };
            let n4_w = ((pos.bw << 1) >> sx) as i32;
            let n4_h = ((pos.bh << 1) >> sy) as i32;
            let max_blocks_wide = n4_w
                + if pos.mb_to_right_edge >= 0 {
                    0
                } else {
                    pos.mb_to_right_edge >> (5 + sx)
                };
            let max_blocks_high = n4_h
                + if pos.mb_to_bottom_edge >= 0 {
                    0
                } else {
                    pos.mb_to_bottom_edge >> (5 + sy)
                };
            let ctx_limits = (
                if pos.mb_to_right_edge >= 0 {
                    0
                } else {
                    max_blocks_wide
                },
                if pos.mb_to_bottom_edge >= 0 {
                    0
                } else {
                    max_blocks_high
                },
            );
            let step = 1i32 << tx_size;
            let mut row = 0;
            while row < max_blocks_high {
                let mut col = 0;
                while col < max_blocks_wide {
                    eobtotal += self.transform_block(
                        r,
                        mi,
                        pos,
                        plane,
                        row as usize,
                        col as usize,
                        tx_size,
                        intra,
                        ctx_limits,
                    );
                    col += step;
                }
                row += step;
            }
        }
        eobtotal
    }

    /// One transform block: intra prediction if `intra`, then its tokens and
    /// inverse transform. Returns its end of block.
    #[allow(clippy::too_many_arguments)]
    fn transform_block(
        &mut self,
        r: &mut BoolReader<'_>,
        mi: &ModeInfo,
        pos: &BlockPos,
        plane: usize,
        row: usize,
        col: usize,
        tx_size: TxSize,
        intra: bool,
        ctx_limits: (i32, i32),
    ) -> usize {
        let info = self.info;
        let (sx, sy) = if plane == 0 {
            (0, 0)
        } else {
            (u32::from(info.ss_x), u32::from(info.ss_y))
        };
        // The block's top-left pixel in this plane.
        let x0 = ((pos.mi_col * 8) >> sx) + 4 * col;
        let y0 = ((pos.mi_row * 8) >> sy) + 4 * row;
        let lossless = info.lossless;
        let bd = info.bit_depth;

        let mut mode = if plane == 0 { mi.mode } else { mi.uv_mode };
        if intra {
            if mi.sb_type < BLOCK_8X8 && plane == 0 {
                mode = mi.bmi.get((row << 1) + col).map_or(mode, |b| b.mode);
            }
            // vp9_predict_intra_block.
            let n4_wl = pos.bwl - sx;
            let bw4 = 1usize << n4_wl;
            let txw = 1usize << tx_size;
            // libvpx's have_top and have_left: inside the block, or a
            // neighbour above (any row but the frame's first) or to the
            // left within the tile.
            let have_top = row != 0 || pos.mi_row != 0;
            let have_left = col != 0 || pos.mi_col > self.tile.mi_col_start;
            let p = &mut self.frame.planes[plane.min(2)];
            let edges = Edges {
                have_top,
                have_left,
                have_right: col + txw < bw4,
                past_right: pos.mb_to_right_edge < 0,
                past_bottom: pos.mb_to_bottom_edge < 0,
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
                bd,
                &mut self.intra_out,
            );
        }
        if intra && mi.skip {
            return 0;
        }

        // The coefficients.
        let tx_type = if !intra || plane != 0 || lossless {
            DCT_DCT
        } else {
            INTRA_MODE_TO_TX_TYPE
                .get(usize::from(mode))
                .copied()
                .unwrap_or(DCT_DCT)
        };
        let scan: Scan = if !intra || plane != 0 || lossless {
            detokenize::scan_for(tx_size, DCT_DCT)
        } else {
            detokenize::scan_for(tx_size, tx_type)
        };
        let seg = usize::from(mi.segment_id).min(7);
        let dq = info.dequant.get(seg).copied().unwrap_or([0; 4]);
        let dq = if plane == 0 {
            [dq[0], dq[1]]
        } else {
            [dq[2], dq[3]]
        };
        let eob = self.decode_block_tokens(r, mi, plane, &scan, col, row, tx_size, dq, ctx_limits);
        if eob > 0 {
            let p = &mut self.frame.planes[plane.min(2)];
            let start = y0 * p.stride + x0;
            if let Some(dst) = p.data.get_mut(start..) {
                idct::inverse_transform_add(
                    tx_size,
                    tx_type,
                    lossless,
                    eob,
                    &self.dqcoeff,
                    dst,
                    p.stride,
                    bd,
                );
            }
            detokenize::clear_coefs(&mut self.dqcoeff, &scan, eob);
        }
        eob
    }

    /// One transform block's tokens, with the above and left contexts it is
    /// coded with and leaves behind: libvpx's `vp9_decode_block_tokens`.
    #[allow(clippy::too_many_arguments)]
    fn decode_block_tokens(
        &mut self,
        r: &mut BoolReader<'_>,
        mi: &ModeInfo,
        plane: usize,
        scan: &Scan,
        x: usize,
        y: usize,
        tx_size: TxSize,
        dq: [i16; 2],
        ctx_limits: (i32, i32),
    ) -> usize {
        let info = self.info;
        let (sx, sy) = if plane == 0 {
            (0, 0)
        } else {
            (u32::from(info.ss_x), u32::from(info.ss_y))
        };
        let pos_a = ((mi.mi_col * 2) >> sx) + x;
        let pos_l = (((mi.mi_row * 2) & 15) >> sy) + y;
        let n = 1usize << tx_size;
        let any = |s: &[u8]| s.iter().any(|&v| v != 0);
        let above = self
            .above_ctx
            .get(plane)
            .and_then(|a| a.get(pos_a..pos_a + n))
            .is_some_and(any);
        let left = self
            .left_ctx
            .get(plane)
            .and_then(|l| l.get(pos_l..pos_l + n))
            .is_some_and(any);
        let ctx = usize::from(above) + usize::from(left);

        let plane_type = usize::from(plane > 0);
        let ref_type = usize::from(mi.is_inter());
        let tx = usize::from(tx_size).min(3);
        let probs = &self.fc.coef_probs[tx][plane_type][ref_type];
        let counts = match self.counts.as_deref_mut() {
            Some(c) => Some(BlockCounts {
                tokens: &mut c.coef[tx][plane_type][ref_type],
                eob_branch: &mut c.eob_branch[tx][plane_type][ref_type],
            }),
            None => None,
        };
        let eob = detokenize::decode_coefs(
            r,
            probs,
            counts,
            tx_size,
            dq,
            ctx,
            scan,
            info.bit_depth,
            &mut self.dqcoeff,
            &mut self.token_cache,
        );

        // The contexts it leaves: 1 if anything was coded, for the 4x4
        // columns and rows inside the frame; 0 past its edge.
        let flag = u8::from(eob > 0);
        let (max_w, max_h) = ctx_limits;
        let inside = |limit: i32, start: usize| -> usize {
            if limit == 0 {
                n
            } else {
                (limit - start as i32).clamp(0, n as i32) as usize
            }
        };
        let keep_a = inside(max_w, x);
        let keep_l = inside(max_h, y);
        if let Some(a) = self
            .above_ctx
            .get_mut(plane)
            .and_then(|a| a.get_mut(pos_a..pos_a + n))
        {
            for (i, v) in a.iter_mut().enumerate() {
                *v = if i < keep_a { flag } else { 0 };
            }
        }
        if let Some(l) = self
            .left_ctx
            .get_mut(plane)
            .and_then(|l| l.get_mut(pos_l..pos_l + n))
        {
            for (i, v) in l.iter_mut().enumerate() {
                *v = if i < keep_l { flag } else { 0 };
            }
        }
        eob
    }

    // --- Mode information -----------------------------------------------------------------

    /// libvpx's `vp9_read_mode_info`.
    #[allow(clippy::too_many_arguments)]
    fn read_mode_info(
        &mut self,
        r: &mut BoolReader<'_>,
        mi: &mut ModeInfo,
        pos: &BlockPos,
        above: Option<usize>,
        left: Option<usize>,
        x_mis: usize,
        y_mis: usize,
        bmode: (u32, u32),
    ) -> Result<(), Error> {
        if self.info.intra_only {
            self.read_intra_frame_mode_info(r, mi, above, left, x_mis, y_mis);
            return Ok(());
        }
        self.read_inter_frame_mode_info(r, mi, pos, above, left, x_mis, y_mis, bmode)?;
        let mv_ref = MvRef {
            mv: mi.mv,
            ref_frame: mi.ref_frame,
        };
        let cols = self.info.mi_cols;
        for y in 0..y_mis {
            let row = (pos.mi_row + y) * cols + pos.mi_col;
            if let Some(cells) = self.cur_mvs.get_mut(row..row + x_mis) {
                cells.fill(mv_ref);
            }
        }
        Ok(())
    }

    fn block(&self, idx: Option<usize>) -> Option<&ModeInfo> {
        idx.and_then(|i| self.mi.blocks.get(i))
    }

    /// libvpx's `read_intra_frame_mode_info`: a key frame's or intra-only
    /// frame's block.
    fn read_intra_frame_mode_info(
        &mut self,
        r: &mut BoolReader<'_>,
        mi: &mut ModeInfo,
        above: Option<usize>,
        left: Option<usize>,
        x_mis: usize,
        y_mis: usize,
    ) {
        mi.segment_id = self.read_intra_segment_id(r, mi.mi_row, mi.mi_col, x_mis, y_mis);
        mi.skip = self.read_skip(r, mi.segment_id, above, left);
        mi.tx_size = self.read_tx_size(r, mi.sb_type, true, above, left);
        mi.ref_frame = [INTRA_FRAME, NO_REF_FRAME];

        let above_mi = self.block(above).copied();
        let left_mi = self.block(left).copied();
        let read = |r: &mut BoolReader<'_>, mi: &ModeInfo, b: usize| -> PredictionMode {
            let a = above_block_mode(mi, above_mi.as_ref(), b);
            let l = left_block_mode(mi, left_mi.as_ref(), b);
            let probs = tables::KF_Y_MODE_PROB
                .get(usize::from(a))
                .and_then(|p| p.get(usize::from(l)))
                .copied()
                .unwrap_or([128; 9]);
            r.read_tree(&INTRA_MODE_TREE, &probs) as PredictionMode
        };
        match mi.sb_type {
            BLOCK_4X4 => {
                for i in 0..4 {
                    let m = read(r, mi, i);
                    mi.bmi[i].mode = m;
                }
                mi.mode = mi.bmi[3].mode;
            }
            BLOCK_4X8 => {
                let m0 = read(r, mi, 0);
                mi.bmi[0].mode = m0;
                mi.bmi[2].mode = m0;
                let m1 = read(r, mi, 1);
                mi.bmi[1].mode = m1;
                mi.bmi[3].mode = m1;
                mi.mode = m1;
            }
            BLOCK_8X4 => {
                let m0 = read(r, mi, 0);
                mi.bmi[0].mode = m0;
                mi.bmi[1].mode = m0;
                let m2 = read(r, mi, 2);
                mi.bmi[2].mode = m2;
                mi.bmi[3].mode = m2;
                mi.mode = m2;
            }
            _ => mi.mode = read(r, mi, 0),
        }
        let probs = tables::KF_UV_MODE_PROB
            .get(usize::from(mi.mode))
            .copied()
            .unwrap_or([128; 9]);
        mi.uv_mode = r.read_tree(&INTRA_MODE_TREE, &probs) as PredictionMode;
    }

    /// libvpx's `read_intra_segment_id`.
    fn read_intra_segment_id(
        &mut self,
        r: &mut BoolReader<'_>,
        mi_row: usize,
        mi_col: usize,
        x_mis: usize,
        y_mis: usize,
    ) -> u8 {
        let seg = &self.info.seg;
        if !seg.enabled {
            return 0;
        }
        if !seg.update_map {
            self.copy_segment_id(mi_row, mi_col, x_mis, y_mis);
            return 0;
        }
        let id = r.read_tree(&SEGMENT_TREE, &seg.tree_probs) as u8;
        self.set_segment_id(mi_row, mi_col, x_mis, y_mis, id);
        id
    }

    /// libvpx's `copy_segment_id`.
    fn copy_segment_id(&mut self, mi_row: usize, mi_col: usize, x_mis: usize, y_mis: usize) {
        let cols = self.info.mi_cols;
        for y in 0..y_mis {
            let start = (mi_row + y) * cols + mi_col;
            for i in start..start + x_mis {
                let v = self.last_seg_map.get(i).copied().unwrap_or(0);
                if let Some(c) = self.cur_seg_map.get_mut(i) {
                    *c = v;
                }
            }
        }
    }

    /// libvpx's `set_segment_id`.
    fn set_segment_id(&mut self, mi_row: usize, mi_col: usize, x_mis: usize, y_mis: usize, id: u8) {
        let cols = self.info.mi_cols;
        for y in 0..y_mis {
            let start = (mi_row + y) * cols + mi_col;
            if let Some(cells) = self.cur_seg_map.get_mut(start..start + x_mis) {
                cells.fill(id);
            }
        }
    }

    /// libvpx's `read_inter_segment_id`.
    fn read_inter_segment_id(
        &mut self,
        r: &mut BoolReader<'_>,
        mi: &mut ModeInfo,
        above: Option<usize>,
        left: Option<usize>,
        x_mis: usize,
        y_mis: usize,
    ) -> u8 {
        let seg = self.info.seg;
        if !seg.enabled {
            return 0;
        }
        let cols = self.info.mi_cols;
        // dec_get_segment_id: the smallest id the last frame had under the
        // block.
        let mut predicted = u8::MAX;
        for y in 0..y_mis {
            let start = (mi.mi_row + y) * cols + mi.mi_col;
            for i in start..start + x_mis {
                predicted = predicted.min(self.last_seg_map.get(i).copied().unwrap_or(0));
            }
        }
        let predicted = predicted.min(7);
        if !seg.update_map {
            self.copy_segment_id(mi.mi_row, mi.mi_col, x_mis, y_mis);
            return predicted;
        }
        let id = if seg.temporal_update {
            // vp9_get_pred_context_seg_id.
            let a = self.block(above).is_some_and(|m| m.seg_id_predicted);
            let l = self.block(left).is_some_and(|m| m.seg_id_predicted);
            let prob = seg
                .pred_probs
                .get(usize::from(a) + usize::from(l))
                .copied()
                .unwrap_or(128);
            mi.seg_id_predicted = r.read_bool(prob);
            if mi.seg_id_predicted {
                predicted
            } else {
                r.read_tree(&SEGMENT_TREE, &seg.tree_probs) as u8
            }
        } else {
            r.read_tree(&SEGMENT_TREE, &seg.tree_probs) as u8
        };
        self.set_segment_id(mi.mi_row, mi.mi_col, x_mis, y_mis, id);
        id
    }

    /// libvpx's `read_skip`.
    fn read_skip(
        &mut self,
        r: &mut BoolReader<'_>,
        segment_id: u8,
        above: Option<usize>,
        left: Option<usize>,
    ) -> bool {
        if self.info.seg.feature_active(segment_id, SEG_LVL_SKIP) {
            return true;
        }
        // vp9_get_skip_context.
        let ctx = usize::from(self.block(above).is_some_and(|m| m.skip))
            + usize::from(self.block(left).is_some_and(|m| m.skip));
        let prob = self.fc.skip_probs.get(ctx).copied().unwrap_or(128);
        let skip = r.read_bool(prob);
        if let Some(c) = self.counts()
            && let Some(slot) = c
                .skip
                .get_mut(ctx)
                .and_then(|s| s.get_mut(usize::from(skip)))
        {
            *slot = slot.wrapping_add(1);
        }
        skip
    }

    /// libvpx's `read_tx_size` and `read_selected_tx_size`.
    fn read_tx_size(
        &mut self,
        r: &mut BoolReader<'_>,
        bsize: BlockSize,
        allow_select: bool,
        above: Option<usize>,
        left: Option<usize>,
    ) -> TxSize {
        let tx_mode = self.info.tx_mode;
        let max_tx = tables::MAX_TXSIZE
            .get(usize::from(bsize))
            .copied()
            .unwrap_or(TX_4X4);
        if !(allow_select && tx_mode == TX_MODE_SELECT && bsize >= BLOCK_8X8) {
            let biggest = tables::TX_MODE_TO_BIGGEST_TX_SIZE
                .get(usize::from(tx_mode))
                .copied()
                .unwrap_or(TX_4X4);
            return max_tx.min(biggest);
        }
        // get_tx_size_context.
        let a = self.block(above);
        let l = self.block(left);
        let mut above_ctx = a.map_or(max_tx, |m| if m.skip { max_tx } else { m.tx_size });
        let mut left_ctx = l.map_or(max_tx, |m| if m.skip { max_tx } else { m.tx_size });
        if l.is_none() {
            left_ctx = above_ctx;
        }
        if a.is_none() {
            above_ctx = left_ctx;
        }
        let ctx = usize::from(above_ctx + left_ctx > max_tx);
        let p = &self.fc.tx_probs;
        let probs: &[u8] = match max_tx {
            TX_8X8 => p.p8x8.get(ctx).map_or(&[], |v| v.as_slice()),
            TX_16X16 => p.p16x16.get(ctx).map_or(&[], |v| v.as_slice()),
            _ => p.p32x32.get(ctx).map_or(&[], |v| v.as_slice()),
        };
        let prob = |i: usize| probs.get(i).copied().unwrap_or(128);
        let mut tx = r.read(prob(0)) as TxSize;
        if tx != TX_4X4 && max_tx >= TX_16X16 {
            tx += r.read(prob(1)) as TxSize;
            if tx != TX_8X8 && max_tx >= TX_32X32 {
                tx += r.read(prob(2)) as TxSize;
            }
        }
        if let Some(c) = self.counts() {
            let t = &mut c.tx;
            let slot = match max_tx {
                TX_8X8 => t.p8x8.get_mut(ctx).and_then(|s| s.get_mut(usize::from(tx))),
                TX_16X16 => t
                    .p16x16
                    .get_mut(ctx)
                    .and_then(|s| s.get_mut(usize::from(tx))),
                _ => t
                    .p32x32
                    .get_mut(ctx)
                    .and_then(|s| s.get_mut(usize::from(tx))),
            };
            if let Some(slot) = slot {
                *slot = slot.wrapping_add(1);
            }
        }
        tx
    }

    /// libvpx's `read_inter_frame_mode_info`.
    #[allow(clippy::too_many_arguments)]
    fn read_inter_frame_mode_info(
        &mut self,
        r: &mut BoolReader<'_>,
        mi: &mut ModeInfo,
        pos: &BlockPos,
        above: Option<usize>,
        left: Option<usize>,
        x_mis: usize,
        y_mis: usize,
        bmode: (u32, u32),
    ) -> Result<(), Error> {
        mi.segment_id = self.read_inter_segment_id(r, mi, above, left, x_mis, y_mis);
        mi.skip = self.read_skip(r, mi.segment_id, above, left);
        let inter_block = self.read_is_inter_block(r, mi.segment_id, above, left);
        mi.tx_size = self.read_tx_size(r, mi.sb_type, !mi.skip || !inter_block, above, left);
        if inter_block {
            self.read_inter_block_mode_info(r, mi, pos, above, left, bmode)
        } else {
            self.read_intra_block_mode_info(r, mi);
            Ok(())
        }
    }

    /// libvpx's `read_is_inter_block`.
    fn read_is_inter_block(
        &mut self,
        r: &mut BoolReader<'_>,
        segment_id: u8,
        above: Option<usize>,
        left: Option<usize>,
    ) -> bool {
        let seg = &self.info.seg;
        if seg.feature_active(segment_id, SEG_LVL_REF_FRAME) {
            return seg.data(segment_id, SEG_LVL_REF_FRAME) != i32::from(INTRA_FRAME);
        }
        // get_intra_inter_context.
        let a = self.block(above);
        let l = self.block(left);
        let ctx = match (a, l) {
            (Some(a), Some(l)) => {
                let (ai, li) = (!a.is_inter(), !l.is_inter());
                if ai && li { 3 } else { usize::from(ai || li) }
            }
            (Some(e), None) | (None, Some(e)) => 2 * usize::from(!e.is_inter()),
            (None, None) => 0,
        };
        let prob = self.fc.intra_inter_prob.get(ctx).copied().unwrap_or(128);
        let is_inter = r.read_bool(prob);
        if let Some(c) = self.counts()
            && let Some(slot) = c
                .intra_inter
                .get_mut(ctx)
                .and_then(|s| s.get_mut(usize::from(is_inter)))
        {
            *slot = slot.wrapping_add(1);
        }
        is_inter
    }

    /// One luma mode with the inter frame's probabilities, counted:
    /// libvpx's `read_intra_mode_y`.
    fn read_intra_mode_y(&mut self, r: &mut BoolReader<'_>, size_group: usize) -> PredictionMode {
        let probs = self
            .fc
            .y_mode_prob
            .get(size_group)
            .copied()
            .unwrap_or([128; 9]);
        let m = r.read_tree(&INTRA_MODE_TREE, &probs) as PredictionMode;
        if let Some(c) = self.counts()
            && let Some(slot) = c
                .y_mode
                .get_mut(size_group)
                .and_then(|s| s.get_mut(usize::from(m)))
        {
            *slot = slot.wrapping_add(1);
        }
        m
    }

    /// libvpx's `read_intra_block_mode_info`: an intra block in an inter
    /// frame.
    fn read_intra_block_mode_info(&mut self, r: &mut BoolReader<'_>, mi: &mut ModeInfo) {
        match mi.sb_type {
            BLOCK_4X4 => {
                for i in 0..4 {
                    mi.bmi[i].mode = self.read_intra_mode_y(r, 0);
                }
                mi.mode = mi.bmi[3].mode;
            }
            BLOCK_4X8 => {
                let m0 = self.read_intra_mode_y(r, 0);
                mi.bmi[0].mode = m0;
                mi.bmi[2].mode = m0;
                let m1 = self.read_intra_mode_y(r, 0);
                mi.bmi[1].mode = m1;
                mi.bmi[3].mode = m1;
                mi.mode = m1;
            }
            BLOCK_8X4 => {
                let m0 = self.read_intra_mode_y(r, 0);
                mi.bmi[0].mode = m0;
                mi.bmi[1].mode = m0;
                let m2 = self.read_intra_mode_y(r, 0);
                mi.bmi[2].mode = m2;
                mi.bmi[3].mode = m2;
                mi.mode = m2;
            }
            b => {
                let group =
                    usize::from(tables::SIZE_GROUP.get(usize::from(b)).copied().unwrap_or(0));
                mi.mode = self.read_intra_mode_y(r, group);
            }
        }
        // read_intra_mode_uv.
        let probs = self
            .fc
            .uv_mode_prob
            .get(usize::from(mi.mode))
            .copied()
            .unwrap_or([128; 9]);
        mi.uv_mode = r.read_tree(&INTRA_MODE_TREE, &probs) as PredictionMode;
        if let Some(c) = self.counts()
            && let Some(slot) = c
                .uv_mode
                .get_mut(usize::from(mi.mode))
                .and_then(|s| s.get_mut(usize::from(mi.uv_mode)))
        {
            *slot = slot.wrapping_add(1);
        }
        // So that the filter context needs no inter check.
        mi.interp_filter = SWITCHABLE_FILTERS as InterpFilter;
        mi.ref_frame = [INTRA_FRAME, NO_REF_FRAME];
    }

    /// libvpx's `read_ref_frames`.
    fn read_ref_frames(
        &mut self,
        r: &mut BoolReader<'_>,
        segment_id: u8,
        above: Option<usize>,
        left: Option<usize>,
    ) -> [RefFrame; 2] {
        let info = self.info;
        if info.seg.feature_active(segment_id, SEG_LVL_REF_FRAME) {
            return [
                info.seg.data(segment_id, SEG_LVL_REF_FRAME) as RefFrame,
                NO_REF_FRAME,
            ];
        }
        let a = self.block(above).copied();
        let l = self.block(left).copied();
        // read_block_reference_mode.
        let mode = if info.reference_mode == REFERENCE_MODE_SELECT {
            let ctx = reference_mode_context(info, a.as_ref(), l.as_ref());
            let prob = self.fc.comp_inter_prob.get(ctx).copied().unwrap_or(128);
            let m = r.read(prob) as ReferenceMode;
            if let Some(c) = self.counts()
                && let Some(slot) = c
                    .comp_inter
                    .get_mut(ctx)
                    .and_then(|s| s.get_mut(usize::from(m)))
            {
                *slot = slot.wrapping_add(1);
            }
            m
        } else {
            info.reference_mode
        };
        if mode == COMPOUND_REFERENCE {
            let idx = usize::from(
                info.ref_frame_sign_bias
                    .get(info.comp_fixed_ref as usize)
                    .copied()
                    .unwrap_or(false),
            );
            let ctx = comp_ref_context(info, a.as_ref(), l.as_ref());
            let prob = self.fc.comp_ref_prob.get(ctx).copied().unwrap_or(128);
            let bit = r.read(prob) as usize;
            if let Some(c) = self.counts()
                && let Some(slot) = c.comp_ref.get_mut(ctx).and_then(|s| s.get_mut(bit))
            {
                *slot = slot.wrapping_add(1);
            }
            let mut refs = [NO_REF_FRAME; 2];
            refs[idx] = info.comp_fixed_ref;
            refs[1 - idx] = info.comp_var_ref[bit.min(1)];
            refs
        } else {
            let ctx0 = single_ref_p1_context(a.as_ref(), l.as_ref());
            let prob0 = self.fc.single_ref_prob.get(ctx0).map_or(128, |p| p[0]);
            let bit0 = r.read(prob0) as usize;
            if let Some(c) = self.counts()
                && let Some(slot) = c.single_ref.get_mut(ctx0).and_then(|s| s[0].get_mut(bit0))
            {
                *slot = slot.wrapping_add(1);
            }
            let first = if bit0 == 1 {
                let ctx1 = single_ref_p2_context(a.as_ref(), l.as_ref());
                let prob1 = self.fc.single_ref_prob.get(ctx1).map_or(128, |p| p[1]);
                let bit1 = r.read(prob1) as usize;
                if let Some(c) = self.counts()
                    && let Some(slot) = c.single_ref.get_mut(ctx1).and_then(|s| s[1].get_mut(bit1))
                {
                    *slot = slot.wrapping_add(1);
                }
                if bit1 == 1 {
                    ALTREF_FRAME
                } else {
                    GOLDEN_FRAME
                }
            } else {
                LAST_FRAME
            };
            [first, NO_REF_FRAME]
        }
    }

    /// libvpx's `read_inter_mode`.
    fn read_inter_mode(&mut self, r: &mut BoolReader<'_>, ctx: usize) -> PredictionMode {
        let probs = self
            .fc
            .inter_mode_probs
            .get(ctx)
            .copied()
            .unwrap_or([128; 3]);
        let m = r.read_tree(&INTER_MODE_TREE, &probs) as usize;
        if let Some(c) = self.counts()
            && let Some(slot) = c.inter_mode.get_mut(ctx).and_then(|s| s.get_mut(m))
        {
            *slot = slot.wrapping_add(1);
        }
        NEARESTMV + m as PredictionMode
    }

    /// libvpx's `read_switchable_interp_filter`.
    fn read_switchable_interp_filter(
        &mut self,
        r: &mut BoolReader<'_>,
        above: Option<usize>,
        left: Option<usize>,
    ) -> InterpFilter {
        // get_pred_context_switchable_interp.
        let sw = SWITCHABLE_FILTERS as InterpFilter;
        let left_type = self.block(left).map_or(sw, |m| m.interp_filter);
        let above_type = self.block(above).map_or(sw, |m| m.interp_filter);
        let ctx = usize::from(if left_type == above_type {
            left_type
        } else if left_type == sw {
            above_type
        } else if above_type == sw {
            left_type
        } else {
            sw
        });
        let probs = self
            .fc
            .switchable_interp_prob
            .get(ctx)
            .copied()
            .unwrap_or([128; 2]);
        let t = r.read_tree(&SWITCHABLE_INTERP_TREE, &probs) as usize;
        if let Some(c) = self.counts()
            && let Some(slot) = c.switchable_interp.get_mut(ctx).and_then(|s| s.get_mut(t))
        {
            *slot = slot.wrapping_add(1);
        }
        t as InterpFilter
    }

    /// libvpx's `read_inter_block_mode_info`.
    fn read_inter_block_mode_info(
        &mut self,
        r: &mut BoolReader<'_>,
        mi: &mut ModeInfo,
        pos: &BlockPos,
        above: Option<usize>,
        left: Option<usize>,
        bmode: (u32, u32),
    ) -> Result<(), Error> {
        let info = self.info;
        let bsize = mi.sb_type;
        let allow_hp = info.allow_high_precision_mv;
        let mut best_ref_mvs = [Mv::ZERO; 2];
        mi.ref_frame = self.read_ref_frames(r, mi.segment_id, above, left);
        let is_compound = mi.has_second_ref();
        let search = mv_ref_blocks(bsize);
        let inter_mode_ctx = self.mode_context(&search, pos);

        if info.seg.feature_active(mi.segment_id, SEG_LVL_SKIP) {
            mi.mode = ZEROMV;
            if bsize < BLOCK_8X8 {
                return Err(Error::Unsupported(
                    "invalid use of the skip segment feature on small blocks",
                ));
            }
        } else if bsize >= BLOCK_8X8 {
            mi.mode = self.read_inter_mode(r, inter_mode_ctx);
        }

        mi.interp_filter = if info.interp_filter == SWITCHABLE {
            self.read_switchable_interp_filter(r, above, left)
        } else {
            info.interp_filter
        };

        let refs = usize::from(is_compound) + 1;
        if bsize < BLOCK_8X8 {
            let num_4x4_w = 1usize << bmode.0;
            let num_4x4_h = 1usize << bmode.1;
            let mut got_mv_refs_for_new = false;
            let mut best_sub8x8 = [Mv::ZERO, Mv::INVALID];
            let mut b_mode = ZEROMV;
            let mut idy = 0;
            while idy < 2 {
                let mut idx = 0;
                while idx < 2 {
                    let j = idy * 2 + idx;
                    b_mode = self.read_inter_mode(r, inter_mode_ctx);
                    if b_mode == NEARESTMV || b_mode == NEARMV {
                        for (rf, best) in best_sub8x8.iter_mut().enumerate().take(refs) {
                            *best = self.append_sub8x8_mvs_for_idx(mi, &search, pos, b_mode, j, rf);
                        }
                    } else if b_mode == NEWMV && !got_mv_refs_for_new {
                        for (rf, best) in best_ref_mvs.iter_mut().enumerate().take(refs) {
                            let frame = mi.ref_frame[rf];
                            let (list, _) = self.find_mv_refs(mi, pos, NEWMV, frame, &search, None);
                            *best = lower_mv_precision(list[0], allow_hp);
                            got_mv_refs_for_new = true;
                        }
                    }
                    let mut mvs = [Mv::ZERO; 2];
                    if !self.assign_mv(
                        r,
                        b_mode,
                        &mut mvs,
                        &best_ref_mvs,
                        &best_sub8x8,
                        is_compound,
                        allow_hp,
                    ) {
                        return Err(Error::Corrupt("an invalid motion vector"));
                    }
                    mi.bmi[j].mv = mvs;
                    if num_4x4_h == 2 {
                        mi.bmi[j + 2] = mi.bmi[j];
                    }
                    if num_4x4_w == 2 {
                        mi.bmi[j + 1] = mi.bmi[j];
                    }
                    idx += num_4x4_w;
                }
                idy += num_4x4_h;
            }
            mi.mode = b_mode;
            mi.mv = mi.bmi[3].mv;
        } else {
            if mi.mode != ZEROMV {
                for rf in 0..refs {
                    let frame = mi.ref_frame[rf];
                    let (list, count) = self.find_mv_refs(mi, pos, mi.mode, frame, &search, None);
                    best_ref_mvs[rf] =
                        lower_mv_precision(list[count.saturating_sub(1).min(1)], allow_hp);
                }
            }
            let mut mvs = [Mv::ZERO; 2];
            let ok = self.assign_mv(
                r,
                mi.mode,
                &mut mvs,
                &best_ref_mvs,
                &best_ref_mvs,
                is_compound,
                allow_hp,
            );
            mi.mv = mvs;
            if !ok {
                return Err(Error::Corrupt("an invalid motion vector"));
            }
        }
        Ok(())
    }

    /// libvpx's `assign_mv`. False for an invalid vector or mode.
    #[allow(clippy::too_many_arguments)]
    fn assign_mv(
        &mut self,
        r: &mut BoolReader<'_>,
        mode: PredictionMode,
        mv: &mut [Mv; 2],
        ref_mv: &[Mv; 2],
        near_nearest: &[Mv; 2],
        is_compound: bool,
        allow_hp: bool,
    ) -> bool {
        match mode {
            NEWMV => {
                let mut ok = true;
                for i in 0..=usize::from(is_compound) {
                    mv[i] = self.read_mv(r, ref_mv[i], allow_hp);
                    ok = ok && is_mv_valid(mv[i]);
                }
                ok
            }
            NEARMV | NEARESTMV => {
                *mv = *near_nearest;
                true
            }
            ZEROMV => {
                *mv = [Mv::ZERO; 2];
                true
            }
            _ => false,
        }
    }

    /// libvpx's `read_mv`: a difference from `reference`, counted.
    fn read_mv(&mut self, r: &mut BoolReader<'_>, reference: Mv, allow_hp: bool) -> Mv {
        let ctx = &self.fc.nmvc;
        let joint = r.read_tree(&MV_JOINT_TREE, &ctx.joints) as usize;
        let use_hp = allow_hp && use_mv_hp(reference);
        let mut diff = Mv::ZERO;
        // MV_JOINT_HZVNZ (2) and HNZVNZ (3) have a row; HNZVZ (1) and 3 a
        // column.
        if joint == 2 || joint == 3 {
            diff.row = read_mv_component(r, &ctx.comps[0], use_hp) as i16;
        }
        if joint == 1 || joint == 3 {
            diff.col = read_mv_component(r, &ctx.comps[1], use_hp) as i16;
        }
        if let Some(c) = self.counts() {
            inc_mv(diff, &mut c.mv);
        }
        Mv {
            row: (i32::from(reference.row) + i32::from(diff.row)) as i16,
            col: (i32::from(reference.col) + i32::from(diff.col)) as i16,
        }
    }

    // --- Motion vector prediction -----------------------------------------------------

    /// libvpx's `get_mode_context`: the inter mode context from the two
    /// nearest neighbours' modes.
    fn mode_context(&self, search: &[[i8; 2]; 8], pos: &BlockPos) -> usize {
        let mut counter = 0usize;
        for p in search.iter().take(2) {
            if let Some(c) = self.candidate(pos, *p) {
                counter += usize::from(
                    tables::MODE_2_COUNTER
                        .get(usize::from(c.mode))
                        .copied()
                        .unwrap_or(0),
                );
            }
        }
        usize::from(
            tables::COUNTER_TO_CONTEXT
                .get(counter)
                .copied()
                .unwrap_or(0),
        )
    }

    /// The block at a search offset (row, column) from the current one, if
    /// it is inside the tile: libvpx's `is_inside` and `xd->mi[...]`.
    fn candidate(&self, pos: &BlockPos, p: [i8; 2]) -> Option<&ModeInfo> {
        let row = pos.mi_row as i64 + i64::from(p[0]);
        let col = pos.mi_col as i64 + i64::from(p[1]);
        if row < 0
            || col < self.tile.mi_col_start as i64
            || row >= self.info.mi_rows as i64
            || col >= self.tile.mi_col_end as i64
        {
            return None;
        }
        self.mi.at(row as usize, col as usize)
    }

    /// libvpx's `dec_find_mv_refs`: up to two candidate vectors for
    /// `ref_frame`, clamped. `block` is the sub-8x8 block's index when
    /// predicting for one, which makes the two nearest candidates their
    /// sub-blocks. Returns the list and how many of it count.
    fn find_mv_refs(
        &self,
        mi: &ModeInfo,
        pos: &BlockPos,
        mode: PredictionMode,
        ref_frame: RefFrame,
        search: &[[i8; 2]; 8],
        block: Option<usize>,
    ) -> ([Mv; 2], usize) {
        let _ = mi;
        let sign_bias = &self.info.ref_frame_sign_bias;
        let bias = |f: RefFrame| sign_bias.get(f.max(0) as usize).copied().unwrap_or(false);
        let mut list = [Mv::ZERO; 2];
        let mut count = 0usize;
        let mut different_ref_found = false;
        let early_break = mode != NEARMV;
        let prev = self
            .prev_mvs
            .and_then(|p| p.get(pos.mi_row * self.info.mi_cols + pos.mi_col));

        // ADD_MV_REF_LIST_EB: returns true when the search is done.
        let add = |mv: Mv, list: &mut [Mv; 2], count: &mut usize| -> bool {
            if *count > 0 {
                if mv != list[0] {
                    list[*count] = mv;
                    *count += 1;
                    return true;
                }
                false
            } else {
                list[0] = mv;
                *count = 1;
                early_break
            }
        };

        let done = 'search: {
            let mut i = 0;
            if let Some(b) = block {
                // The nearest two: sub-block vectors where they have them.
                while i < 2 {
                    let p = search[i];
                    if let Some(c) = self.candidate(pos, p) {
                        different_ref_found = true;
                        let sub = |which: usize| -> Mv {
                            if c.sb_type < BLOCK_8X8 {
                                let k = tables::IDX_N_COLUMN_TO_SUBBLOCK
                                    .get(b)
                                    .and_then(|v| v.get(usize::from(p[1] == 0)))
                                    .copied()
                                    .unwrap_or(0);
                                c.bmi
                                    .get(usize::from(k))
                                    .map_or(c.mv[which], |bm| bm.mv[which])
                            } else {
                                c.mv[which]
                            }
                        };
                        if c.ref_frame[0] == ref_frame {
                            if add(sub(0), &mut list, &mut count) {
                                break 'search true;
                            }
                        } else if c.ref_frame[1] == ref_frame && add(sub(1), &mut list, &mut count)
                        {
                            break 'search true;
                        }
                    }
                    i += 1;
                }
            }
            // The rest of the neighbours.
            while i < search.len() {
                if let Some(c) = self.candidate(pos, search[i]) {
                    different_ref_found = true;
                    if c.ref_frame[0] == ref_frame {
                        if add(c.mv[0], &mut list, &mut count) {
                            break 'search true;
                        }
                    } else if c.ref_frame[1] == ref_frame && add(c.mv[1], &mut list, &mut count) {
                        break 'search true;
                    }
                }
                i += 1;
            }
            // The last frame's vectors at this block.
            if let Some(p) = prev {
                if p.ref_frame[0] == ref_frame {
                    if add(p.mv[0], &mut list, &mut count) {
                        break 'search true;
                    }
                } else if p.ref_frame[1] == ref_frame && add(p.mv[1], &mut list, &mut count) {
                    break 'search true;
                }
            }
            // Neighbours with other references, sign-corrected.
            if different_ref_found {
                for &s in search {
                    if let Some(c) = self.candidate(pos, s) {
                        if !c.is_inter() {
                            continue;
                        }
                        let scaled = |which: usize| -> Mv {
                            let mv = c.mv[which];
                            if bias(c.ref_frame[which]) != bias(ref_frame) {
                                mv.negated()
                            } else {
                                mv
                            }
                        };
                        if c.ref_frame[0] != ref_frame && add(scaled(0), &mut list, &mut count) {
                            break 'search true;
                        }
                        if c.has_second_ref()
                            && c.ref_frame[1] != ref_frame
                            && c.mv[1] != c.mv[0]
                            && add(scaled(1), &mut list, &mut count)
                        {
                            break 'search true;
                        }
                    }
                }
            }
            // The last frame's, with other references.
            if let Some(p) = prev {
                if p.ref_frame[0] != ref_frame && p.ref_frame[0] > INTRA_FRAME {
                    let mut mv = p.mv[0];
                    if bias(p.ref_frame[0]) != bias(ref_frame) {
                        mv = mv.negated();
                    }
                    if add(mv, &mut list, &mut count) {
                        break 'search true;
                    }
                }
                if p.ref_frame[1] > INTRA_FRAME && p.ref_frame[1] != ref_frame && p.mv[1] != p.mv[0]
                {
                    let mut mv = p.mv[1];
                    if bias(p.ref_frame[1]) != bias(ref_frame) {
                        mv = mv.negated();
                    }
                    if add(mv, &mut list, &mut count) {
                        break 'search true;
                    }
                }
            }
            false
        };
        if !done {
            count = if mode == NEARMV { 2 } else { 1 };
        }
        for mv in list.iter_mut().take(count) {
            *mv = clamp_mv_ref(*mv, pos);
        }
        (list, count)
    }

    /// libvpx's `append_sub8x8_mvs_for_idx`: the nearest (or near) vector
    /// for sub-block `block` of the current block.
    fn append_sub8x8_mvs_for_idx(
        &self,
        mi: &ModeInfo,
        search: &[[i8; 2]; 8],
        pos: &BlockPos,
        b_mode: PredictionMode,
        block: usize,
        rf: usize,
    ) -> Mv {
        let frame = mi.ref_frame[rf];
        let bmi = &mi.bmi;
        match block {
            0 => {
                let (list, count) = self.find_mv_refs(mi, pos, b_mode, frame, search, Some(block));
                list[count.saturating_sub(1).min(1)]
            }
            1 | 2 => {
                if b_mode == NEARESTMV {
                    bmi[0].mv[rf]
                } else {
                    let (list, _) = self.find_mv_refs(mi, pos, b_mode, frame, search, Some(block));
                    list.iter()
                        .copied()
                        .find(|&m| m != bmi[0].mv[rf])
                        .unwrap_or(Mv::ZERO)
                }
            }
            _ => {
                if b_mode == NEARESTMV {
                    bmi[2].mv[rf]
                } else if bmi[2].mv[rf] != bmi[1].mv[rf] {
                    bmi[1].mv[rf]
                } else if bmi[2].mv[rf] != bmi[0].mv[rf] {
                    bmi[0].mv[rf]
                } else {
                    let (list, _) = self.find_mv_refs(mi, pos, b_mode, frame, search, Some(block));
                    list.iter()
                        .copied()
                        .find(|&m| m != bmi[2].mv[rf])
                        .unwrap_or(Mv::ZERO)
                }
            }
        }
    }
}

// --- Contexts and small helpers ----------------------------------------------------------

/// The chroma transform size of a block: libvpx's `uv_txsize_lookup`.
pub(crate) fn uv_tx_size(bsize: BlockSize, tx_size: TxSize, ss_x: u8, ss_y: u8) -> TxSize {
    tables::UV_TXSIZE
        .get(usize::from(bsize))
        .and_then(|t| t.get(usize::from(tx_size)))
        .and_then(|t| t.get(usize::from(ss_x)))
        .and_then(|t| t.get(usize::from(ss_y)))
        .copied()
        .unwrap_or(TX_4X4)
}

/// libvpx's `vp9_above_block_mode`.
fn above_block_mode(cur: &ModeInfo, above: Option<&ModeInfo>, b: usize) -> PredictionMode {
    if b == 0 || b == 1 {
        match above {
            Some(a) if !a.is_inter() => a.y_mode(b + 2),
            _ => DC_PRED,
        }
    } else {
        cur.bmi.get(b - 2).map_or(DC_PRED, |m| m.mode)
    }
}

/// libvpx's `vp9_left_block_mode`.
fn left_block_mode(cur: &ModeInfo, left: Option<&ModeInfo>, b: usize) -> PredictionMode {
    if b == 0 || b == 2 {
        match left {
            Some(l) if !l.is_inter() => l.y_mode(b + 1),
            _ => DC_PRED,
        }
    } else {
        cur.bmi.get(b - 1).map_or(DC_PRED, |m| m.mode)
    }
}

/// libvpx's `vp9_get_reference_mode_context`.
fn reference_mode_context(info: &FrameInfo, a: Option<&ModeInfo>, l: Option<&ModeInfo>) -> usize {
    let fixed = info.comp_fixed_ref;
    match (a, l) {
        (Some(a), Some(l)) => {
            if !a.has_second_ref() && !l.has_second_ref() {
                usize::from((a.ref_frame[0] == fixed) ^ (l.ref_frame[0] == fixed))
            } else if !a.has_second_ref() {
                2 + usize::from(a.ref_frame[0] == fixed || !a.is_inter())
            } else if !l.has_second_ref() {
                2 + usize::from(l.ref_frame[0] == fixed || !l.is_inter())
            } else {
                4
            }
        }
        (Some(e), None) | (None, Some(e)) => {
            if e.has_second_ref() {
                3
            } else {
                usize::from(e.ref_frame[0] == fixed)
            }
        }
        (None, None) => 1,
    }
}

/// libvpx's `vp9_get_pred_context_comp_ref_p`.
fn comp_ref_context(info: &FrameInfo, a: Option<&ModeInfo>, l: Option<&ModeInfo>) -> usize {
    let fix_ref_idx = usize::from(
        info.ref_frame_sign_bias
            .get(info.comp_fixed_ref.max(0) as usize)
            .copied()
            .unwrap_or(false),
    );
    let var_ref_idx = 1 - fix_ref_idx;
    let var1 = info.comp_var_ref[1];
    let var0 = info.comp_var_ref[0];
    let fixed = info.comp_fixed_ref;
    match (a, l) {
        (Some(a), Some(l)) => {
            let (ai, li) = (!a.is_inter(), !l.is_inter());
            if ai && li {
                2
            } else if ai || li {
                let e = if ai { l } else { a };
                if e.has_second_ref() {
                    1 + 2 * usize::from(e.ref_frame[var_ref_idx] != var1)
                } else {
                    1 + 2 * usize::from(e.ref_frame[0] != var1)
                }
            } else {
                let l_sg = !l.has_second_ref();
                let a_sg = !a.has_second_ref();
                let vrfa = if a_sg {
                    a.ref_frame[0]
                } else {
                    a.ref_frame[var_ref_idx]
                };
                let vrfl = if l_sg {
                    l.ref_frame[0]
                } else {
                    l.ref_frame[var_ref_idx]
                };
                if vrfa == vrfl && var1 == vrfa {
                    0
                } else if l_sg && a_sg {
                    if (vrfa == fixed && vrfl == var0) || (vrfl == fixed && vrfa == var0) {
                        4
                    } else if vrfa == vrfl {
                        3
                    } else {
                        1
                    }
                } else if l_sg || a_sg {
                    let vrfc = if l_sg { vrfa } else { vrfl };
                    let rfs = if a_sg { vrfa } else { vrfl };
                    if vrfc == var1 && rfs != var1 {
                        1
                    } else if rfs == var1 && vrfc != var1 {
                        2
                    } else {
                        4
                    }
                } else if vrfa == vrfl {
                    4
                } else {
                    2
                }
            }
        }
        (Some(e), None) | (None, Some(e)) => {
            if !e.is_inter() {
                2
            } else if e.has_second_ref() {
                4 * usize::from(e.ref_frame[var_ref_idx] != var1)
            } else {
                3 * usize::from(e.ref_frame[0] != var1)
            }
        }
        (None, None) => 2,
    }
}

/// libvpx's `vp9_get_pred_context_single_ref_p1`.
fn single_ref_p1_context(a: Option<&ModeInfo>, l: Option<&ModeInfo>) -> usize {
    let last = |f: RefFrame| f == LAST_FRAME;
    match (a, l) {
        (Some(a), Some(l)) => {
            let (ai, li) = (!a.is_inter(), !l.is_inter());
            if ai && li {
                2
            } else if ai || li {
                let e = if ai { l } else { a };
                if e.has_second_ref() {
                    1 + usize::from(last(e.ref_frame[0]) || last(e.ref_frame[1]))
                } else {
                    4 * usize::from(last(e.ref_frame[0]))
                }
            } else {
                let (a2, l2) = (a.has_second_ref(), l.has_second_ref());
                let (a0, a1, l0, l1) = (
                    a.ref_frame[0],
                    a.ref_frame[1],
                    l.ref_frame[0],
                    l.ref_frame[1],
                );
                if a2 && l2 {
                    1 + usize::from(last(a0) || last(a1) || last(l0) || last(l1))
                } else if a2 || l2 {
                    let rfs = if a2 { l0 } else { a0 };
                    let (crf1, crf2) = if a2 { (a0, a1) } else { (l0, l1) };
                    if last(rfs) {
                        3 + usize::from(last(crf1) || last(crf2))
                    } else {
                        usize::from(last(crf1) || last(crf2))
                    }
                } else {
                    2 * usize::from(last(a0)) + 2 * usize::from(last(l0))
                }
            }
        }
        (Some(e), None) | (None, Some(e)) => {
            if !e.is_inter() {
                2
            } else if e.has_second_ref() {
                1 + usize::from(last(e.ref_frame[0]) || last(e.ref_frame[1]))
            } else {
                4 * usize::from(last(e.ref_frame[0]))
            }
        }
        (None, None) => 2,
    }
}

/// libvpx's `vp9_get_pred_context_single_ref_p2`.
fn single_ref_p2_context(a: Option<&ModeInfo>, l: Option<&ModeInfo>) -> usize {
    let golden = |f: RefFrame| f == GOLDEN_FRAME;
    let last = |f: RefFrame| f == LAST_FRAME;
    match (a, l) {
        (Some(a), Some(l)) => {
            let (ai, li) = (!a.is_inter(), !l.is_inter());
            if ai && li {
                2
            } else if ai || li {
                let e = if ai { l } else { a };
                if e.has_second_ref() {
                    1 + 2 * usize::from(golden(e.ref_frame[0]) || golden(e.ref_frame[1]))
                } else if last(e.ref_frame[0]) {
                    3
                } else {
                    4 * usize::from(golden(e.ref_frame[0]))
                }
            } else {
                let (a2, l2) = (a.has_second_ref(), l.has_second_ref());
                let (a0, a1, l0, l1) = (
                    a.ref_frame[0],
                    a.ref_frame[1],
                    l.ref_frame[0],
                    l.ref_frame[1],
                );
                if a2 && l2 {
                    if a0 == l0 && a1 == l1 {
                        3 * usize::from(golden(a0) || golden(a1) || golden(l0) || golden(l1))
                    } else {
                        2
                    }
                } else if a2 || l2 {
                    let rfs = if a2 { l0 } else { a0 };
                    let (crf1, crf2) = if a2 { (a0, a1) } else { (l0, l1) };
                    if golden(rfs) {
                        3 + usize::from(golden(crf1) || golden(crf2))
                    } else if rfs == ALTREF_FRAME {
                        usize::from(golden(crf1) || golden(crf2))
                    } else {
                        1 + 2 * usize::from(golden(crf1) || golden(crf2))
                    }
                } else if last(a0) && last(l0) {
                    3
                } else if last(a0) || last(l0) {
                    let edge0 = if last(a0) { l0 } else { a0 };
                    4 * usize::from(golden(edge0))
                } else {
                    2 * usize::from(golden(a0)) + 2 * usize::from(golden(l0))
                }
            }
        }
        (Some(e), None) | (None, Some(e)) => {
            if !e.is_inter() || (last(e.ref_frame[0]) && !e.has_second_ref()) {
                2
            } else if !e.has_second_ref() {
                4 * usize::from(golden(e.ref_frame[0]))
            } else {
                3 * usize::from(golden(e.ref_frame[0]) || golden(e.ref_frame[1]))
            }
        }
        (None, None) => 2,
    }
}

/// The neighbours a block size searches for motion vectors, as (row,
/// column) offsets: libvpx's `mv_ref_blocks`.
fn mv_ref_blocks(bsize: BlockSize) -> [[i8; 2]; 8] {
    tables::MV_REF_BLOCKS
        .get(usize::from(bsize))
        .copied()
        .unwrap_or([
            [-1, 0],
            [0, -1],
            [-1, -1],
            [-2, 0],
            [0, -2],
            [-2, -1],
            [-1, -2],
            [-2, -2],
        ])
}

/// libvpx's `clamp_mv_ref`: a candidate kept within 16 pixels (in eighth
/// pixels) of the frame around the block.
fn clamp_mv_ref(mv: Mv, pos: &BlockPos) -> Mv {
    const MV_BORDER: i32 = 16 << 3;
    let clamp = |v: i16, lo: i32, hi: i32| i32::from(v).clamp(lo, hi.max(lo)) as i16;
    Mv {
        col: clamp(
            mv.col,
            pos.mb_to_left_edge - MV_BORDER,
            pos.mb_to_right_edge + MV_BORDER,
        ),
        row: clamp(
            mv.row,
            pos.mb_to_top_edge - MV_BORDER,
            pos.mb_to_bottom_edge + MV_BORDER,
        ),
    }
}

/// libvpx's `use_mv_hp`: whether a reference vector is small enough for
/// eighth-pixel precision.
fn use_mv_hp(mv: Mv) -> bool {
    i32::from(mv.row).abs() < 64 && i32::from(mv.col).abs() < 64
}

/// libvpx's `lower_mv_precision`: round odd components toward zero when
/// eighth-pixel precision is off.
fn lower_mv_precision(mv: Mv, allow_hp: bool) -> Mv {
    if allow_hp && use_mv_hp(mv) {
        return mv;
    }
    let lower = |v: i16| {
        if v & 1 != 0 {
            if v > 0 { v - 1 } else { v + 1 }
        } else {
            v
        }
    };
    Mv {
        row: lower(mv.row),
        col: lower(mv.col),
    }
}

/// libvpx's `is_mv_valid`.
fn is_mv_valid(mv: Mv) -> bool {
    let (r, c) = (i32::from(mv.row), i32::from(mv.col));
    r > MV_LOW && r < MV_UPP && c > MV_LOW && c < MV_UPP
}

/// libvpx's `read_mv_component`.
fn read_mv_component(
    r: &mut BoolReader<'_>,
    comp: &crate::probs::MvComponentProbs,
    use_hp: bool,
) -> i32 {
    let sign = r.read_bool(comp.sign);
    let mv_class = r.read_tree(&MV_CLASS_TREE, &comp.classes) as i32;
    let class0 = mv_class == 0;
    let (d, mut mag) = if class0 {
        (r.read(comp.class0[0]) as i32, 0)
    } else {
        let n = mv_class as usize + crate::common::CLASS0_BITS - 1;
        let mut d = 0i32;
        for i in 0..n {
            let p = comp.bits.get(i).copied().unwrap_or(128);
            d |= (r.read(p) as i32) << i;
        }
        (d, (CLASS0_SIZE as i32) << (mv_class + 2))
    };
    let fp_probs: &[u8] = if class0 {
        comp.class0_fp.get(d as usize).map_or(&[], |p| p.as_slice())
    } else {
        &comp.fp
    };
    let fr = r.read_tree(&MV_FP_TREE, fp_probs) as i32;
    let hp = if use_hp {
        r.read(if class0 { comp.class0_hp } else { comp.hp }) as i32
    } else {
        1
    };
    mag += ((d << 3) | (fr << 1) | hp) + 1;
    if sign { -mag } else { mag }
}

/// libvpx's `vp9_inc_mv`: count a decoded vector difference.
fn inc_mv(mv: Mv, counts: &mut MvCounts) {
    let joint = match (mv.row == 0, mv.col == 0) {
        (true, true) => 0,
        (true, false) => 1,
        (false, true) => 2,
        (false, false) => 3,
    };
    if let Some(j) = counts.joints.get_mut(joint) {
        *j = j.wrapping_add(1);
    }
    if joint == 2 || joint == 3 {
        inc_mv_component(i32::from(mv.row), &mut counts.comps[0]);
    }
    if joint == 1 || joint == 3 {
        inc_mv_component(i32::from(mv.col), &mut counts.comps[1]);
    }
}

/// libvpx's `inc_mv_component`, with `usehp` 1 as `vp9_inc_mv` passes it.
fn inc_mv_component(v: i32, c: &mut MvComponentCounts) {
    let bump = |x: &mut u32| *x = x.wrapping_add(1);
    let s = usize::from(v < 0);
    if let Some(x) = c.sign.get_mut(s) {
        bump(x);
    }
    let z = v.abs() - 1;
    // vp9_get_mv_class.
    let class = if z >= (CLASS0_SIZE as i32) * 4096 {
        10
    } else {
        i32::from(
            tables::LOG_IN_BASE_2
                .get((z >> 3) as usize)
                .copied()
                .unwrap_or(10),
        )
    };
    let base = if class == 0 {
        0
    } else {
        (CLASS0_SIZE as i32) << (class + 2)
    };
    let o = z - base;
    if let Some(x) = c.classes.get_mut(class as usize) {
        bump(x);
    }
    let d = (o >> 3) as usize;
    let f = ((o >> 1) & 3) as usize;
    let e = (o & 1) as usize;
    if class == 0 {
        if let Some(x) = c.class0.get_mut(d) {
            bump(x);
        }
        if let Some(x) = c.class0_fp.get_mut(d).and_then(|v| v.get_mut(f)) {
            bump(x);
        }
        if let Some(x) = c.class0_hp.get_mut(e) {
            bump(x);
        }
    } else {
        let bits = class as usize + crate::common::CLASS0_BITS - 1;
        for i in 0..bits {
            if let Some(x) = c.bits.get_mut(i).and_then(|v| v.get_mut((d >> i) & 1)) {
                bump(x);
            }
        }
        if let Some(x) = c.fp.get_mut(f) {
            bump(x);
        }
        if let Some(x) = c.hp.get_mut(e) {
            bump(x);
        }
    }
}
