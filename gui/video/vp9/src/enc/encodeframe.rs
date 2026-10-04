//! Encoding a frame's blocks: walking each tile's superblocks through their
//! partitions, and coding each block -- predicting it, transforming and
//! quantising what prediction missed, reconstructing what the decoder will
//! see, and turning the levels into tokens.
//!
//! What to do at each step -- how to partition, which modes, which reference
//! and vector -- is asked of a [`Decide`]; this module carries the decisions
//! out exactly as libvpx's encoder does, so that what it reconstructs is,
//! pixel for pixel, what a decoder will make of the stream. Intra blocks are
//! predicted transform block by transform block from the pixels just
//! reconstructed; inter blocks whole, from the reference frames, with the
//! decoder's own inter prediction, then their residual quantised with the
//! fast quantiser libvpx's realtime path uses.
//!
//! Translated into Rust from libvpx v1.17.0's `vp9/encoder/vp9_encodeframe.c`
//! (`encode_superblock`, `update_state_rt`, `update_stats`, `encode_b_rt`,
//! `encode_sb_rt`, `nonrd_use_partition`, `encode_tiles`), `vp9_encodemb.c`
//! (`vp9_encode_sb`, `vp9_xform_quant_fp`, `vp9_encode_block_intra`,
//! `vp9_encode_intra_block_plane`), `vp9_encodemv.c` (`vp9_update_mv_count`),
//! `vp9_tokenize.c` (`vp9_tokenize_sb`) and `vp9/common/vp9_blockd.c`
//! (`vp9_foreach_transformed_block_in_plane`, `vp9_set_contexts`),
//! `vp9_mvref_common.c` (`vp9_find_best_ref_mvs`) and `vp9_onyxc_int.h`
//! (`partition_plane_context`, `update_partition_context`) (copyright the
//! WebM project authors), used under libvpx's BSD licence and patent grant
//! (`licenses/libvpx-LICENSE`, `licenses/libvpx-PATENTS`).

#![allow(
    clippy::arithmetic_side_effects,
    reason = "positions are mode-info and pixel coordinates bounded by the frame (at most 65536 a side), transform-block indices below 256, and residuals of 8-bit samples; counts wrap as libvpx's unsigned ones do"
)]
#![allow(
    clippy::indexing_slicing,
    reason = "the only indices are planes, clamped with min(2) into three-element arrays, sub-blocks below 4 into four-element ones, and contexts computed in range by the context functions; everything taken from a buffer goes through get"
)]

use crate::block::{
    BlockPos, Bmi, MiGrid, ModeInfo, MvPredictor, MvRef, inc_mv, lower_mv_precision, mv_ref_blocks,
    uv_tx_size,
};
use crate::common::{
    BLOCK_8X8, BLOCK_64X64, BLOCK_INVALID, BlockSize, DC_PRED, DCT_DCT, GOLDEN_FRAME, INTRA_FRAME,
    INTRA_MODE_TO_TX_TYPE, InterpFilter, LAST_FRAME, MAX_REF_FRAMES, MI_MASK, Mv, NEARESTMV,
    NEARMV, NEWMV, NO_REF_FRAME, PARTITION_HORZ, PARTITION_NONE, PARTITION_PLOFFSET,
    PARTITION_SPLIT, PARTITION_VERT, Partition, PredictionMode, RefFrame, SEG_LVL_REF_FRAME,
    SEG_LVL_SKIP, SWITCHABLE, SWITCHABLE_FILTERS, TX_4X4, TX_8X8, TX_16X16, TX_32X32,
    TX_MODE_SELECT, TxSize, TxType, ZEROMV,
};
use crate::context;
use crate::detokenize::{Scan, scan_for};
use crate::enc::bitstream::{EncCounts, FrameHeader};
use crate::enc::fdct;
use crate::enc::quantize::{Quants, quantize_b, quantize_fp};
use crate::enc::tokenize::{TokenExtra, TokenSink, tokenize_b};
use crate::frame::FrameBuf;
use crate::header::{self, Segmentation};
use crate::idct;
use crate::inter::{self, McScratch, ScaleFactors};
use crate::intra::{self, Edges};
use crate::probs::FrameContext;
use crate::tables;

/// A block's inter prediction, as a [`Decide`] chooses it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct InterModes {
    /// `LAST_FRAME`, `GOLDEN_FRAME` or `ALTREF_FRAME`.
    pub ref_frame: RefFrame,
    /// `NEARESTMV`, `NEARMV`, `ZEROMV` or `NEWMV`.
    pub mode: PredictionMode,
    /// The vector, for `NEWMV`; the other modes' come from the neighbours
    /// ([`FrameEncoder::mv_refs`]). A component must be even wherever the
    /// vector it is coded against does not allow eighth pixels
    /// ([`MvRefs::usehp`]).
    pub mv: Mv,
    /// The interpolation filter, when the frame lets blocks choose.
    pub interp_filter: InterpFilter,
    /// A block smaller than 8x8: each 4x4 quarter's mode and (for
    /// `NEWMV`) vector, in raster order -- a 4x8 block's quarters 0 and 1,
    /// an 8x4 block's 0 and 2, as `sub_modes`. `mode` and `mv` are then
    /// unused.
    pub sub: [(PredictionMode, Mv); 4],
}

/// A block's modes, as a [`Decide`] chooses them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct BlockModes {
    /// The luma mode of an intra block 8x8 or larger.
    pub mode: PredictionMode,
    /// The luma modes of an intra block smaller than 8x8, by 4x4 quarter
    /// (raster order). A 4x8 block uses quarters 0 and 1, an 8x4 block 0 and
    /// 2; the others repeat them.
    pub sub_modes: [PredictionMode; 4],
    pub uv_mode: PredictionMode,
    /// The transform size, if the frame lets blocks choose it
    /// (`TX_MODE_SELECT`); otherwise the frame's mode decides.
    pub tx_size: TxSize,
    /// Inter prediction, for a block 8x8 or larger in an inter frame; `None`
    /// is an intra block.
    pub inter: Option<InterModes>,
    /// The block's segment, when the frame has segmentation.
    pub segment_id: u8,
    /// Code no residual at all: libvpx's `x->skip`, the mode search's
    /// encode breakout. Inter blocks only.
    pub skip: bool,
    /// Code no luma residual: libvpx's `skip_txfm[0] == SKIP_TXFM_AC_DC`,
    /// which the mode search sets where it judges every luma coefficient
    /// would quantise to zero. Inter blocks of segment 0 only, as
    /// `update_state_rt` applies it.
    pub skip_y: bool,
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
        f: &mut FrameEncoder<'_>,
        mi_row: usize,
        mi_col: usize,
        bsize: BlockSize,
    ) -> Partition;

    /// The modes of block `bsize` at (`mi_row`, `mi_col`), the blocks before
    /// it in coding order already reconstructed. A search may predict into
    /// the reconstruction, as libvpx's does: the block's coding then
    /// predicts over it.
    fn modes(
        &mut self,
        f: &mut FrameEncoder<'_>,
        mi_row: usize,
        mi_col: usize,
        bsize: BlockSize,
    ) -> BlockModes;
}

/// Where a block is, as libvpx's `MACROBLOCKD` describes it: its size in
/// cells and its distances to the frame's edges in eighth pixels (negative
/// right and bottom distances reach past the frame).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Placement {
    pub mi_row: usize,
    pub mi_col: usize,
    pub mb_to_right_edge: i32,
    pub mb_to_bottom_edge: i32,
}

/// What an inter frame's blocks are coded with beyond the header: the
/// references, the last frame's vectors and the frame's vector and filter
/// settings -- libvpx's `cm->frame_refs`, `prev_frame->mvs`,
/// `ref_frame_sign_bias`, `allow_high_precision_mv` and `interp_filter`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct InterFrame<'a> {
    /// `LAST_FRAME`, `GOLDEN_FRAME` and `ALTREF_FRAME`: frames of this
    /// frame's size, edges extended (see `Encoder`'s `extend_frame`).
    pub refs: [Option<&'a FrameBuf<u8>>; 3],
    /// The last frame's vectors, where they may be used: libvpx's
    /// `use_prev_frame_mvs`.
    pub prev_mvs: Option<&'a [MvRef]>,
    pub sign_bias: [bool; MAX_REF_FRAMES],
    pub allow_hp: bool,
    /// `SWITCHABLE`, or the one filter every block uses.
    pub interp_filter: InterpFilter,
}

/// A block's predicted vectors for one reference: libvpx's
/// `frame_mv[NEARESTMV]` and `frame_mv[NEARMV]` as `vp9_find_best_ref_mvs`
/// leaves them, the inter mode context `vp9_find_mv_refs` counts, and
/// whether a new vector coded against the nearest may use eighth pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct MvRefs {
    pub nearest: Mv,
    pub near: Mv,
    pub mode_context: u8,
    pub usehp: bool,
}

/// What a block's bitstream needs that its mode information does not hold:
/// libvpx's `MB_MODE_INFO_EXT` -- the inter mode context, and per reference
/// the vector a new one is coded against.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct BlockExt {
    pub mode_context: u8,
    pub best_mv: [Mv; 2],
}

/// The partition contexts of a tile's walk: per 8x8 column of the frame and
/// per row of the superblock row, which block sizes ended there. libvpx's
/// `above_seg_context` and `left_seg_context`.
#[derive(Clone, Debug)]
pub(crate) struct PartitionContexts {
    above: Vec<u8>,
    left: [u8; 8],
}

impl PartitionContexts {
    /// Clear contexts for a frame `mi_cols` cells wide.
    pub(crate) fn new(mi_cols: usize) -> Self {
        Self {
            above: vec![0; (mi_cols + 7) & !7],
            left: [0; 8],
        }
    }

    /// A superblock row begins: the left contexts clear.
    pub(crate) fn new_row(&mut self) {
        self.left = [0; 8];
    }

    /// libvpx's `partition_plane_context` for the square block `bsize`.
    pub(crate) fn context(&self, mi_row: usize, mi_col: usize, bsize: BlockSize) -> usize {
        // mi_width_log2_lookup, which for a square is its 4x4 log2 less one.
        let bsl = usize::from(
            tables::B_WIDTH_LOG2
                .get(usize::from(bsize))
                .copied()
                .unwrap_or(1),
        )
        .saturating_sub(1);
        let above = usize::from(self.above.get(mi_col).copied().unwrap_or(0) >> bsl) & 1;
        let left = usize::from(
            self.left
                .get(mi_row & MI_MASK as usize)
                .copied()
                .unwrap_or(0)
                >> bsl,
        ) & 1;
        (left * 2 + above) + bsl * PARTITION_PLOFFSET
    }

    /// libvpx's `update_partition_context`: `bsize` at (`mi_row`, `mi_col`)
    /// was cut into blocks of `subsize`.
    pub(crate) fn update(
        &mut self,
        mi_row: usize,
        mi_col: usize,
        subsize: BlockSize,
        bsize: BlockSize,
    ) {
        let [above, left] = tables::PARTITION_CONTEXT_LOOKUP
            .get(usize::from(subsize))
            .copied()
            .unwrap_or([0, 0]);
        let n = usize::from(
            tables::NUM_8X8_WIDE
                .get(usize::from(bsize))
                .copied()
                .unwrap_or(1),
        );
        for a in self.above.iter_mut().skip(mi_col).take(n) {
            *a = above;
        }
        for l in self.left.iter_mut().skip(mi_row & 7).take(n) {
            *l = left;
        }
    }
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

/// What encoding a frame's blocks left: the blocks and what their bitstream
/// needs, the counts, each tile's tokens, and the vectors for the next
/// frame.
pub(crate) struct Encoded {
    pub mi: MiGrid,
    /// Beside `mi.blocks`, block for block.
    pub ext: Vec<BlockExt>,
    pub counts: EncCounts,
    /// Each tile's tokens, tile rows first.
    pub tile_tokens: Vec<Vec<TokenExtra>>,
    /// Per 8x8 cell: libvpx's `cur_frame->mvs`.
    pub mvs: Vec<MvRef>,
}

/// Encodes one frame's blocks: libvpx's per-frame and per-tile encoder
/// state (`MACROBLOCK`, `MACROBLOCKD`, `TileDataEnc`, `FRAME_COUNTS`).
pub(crate) struct FrameEncoder<'a> {
    pub h: &'a FrameHeader,
    /// The probabilities the frame codes with, before its header's updates:
    /// what libvpx's mode search costs decisions with.
    pub fc: &'a FrameContext,
    pub seg: &'a Segmentation,
    pub quants: &'a Quants,
    /// The picture being encoded, its edges repeated out to whole
    /// superblocks.
    pub src: &'a FrameBuf<u8>,
    /// What the decoder will reconstruct, block by block.
    pub recon: &'a mut FrameBuf<u8>,
    /// An inter frame's references and settings; `None` on an intra-only
    /// frame.
    pub inter: Option<InterFrame<'a>>,
    /// The blocks coded so far.
    pub mi: MiGrid,
    ext: Vec<BlockExt>,
    pub counts: EncCounts,
    /// Each tile's tokens, tile rows first.
    pub tile_tokens: Vec<Vec<TokenExtra>>,
    mvs: Vec<MvRef>,
    /// The tile being encoded: its columns of cells, and its tokens' index
    /// in `tile_tokens`.
    tile_mi_col_start: usize,
    tile_mi_col_end: usize,
    tile_index: usize,
    /// The references as the inter predictor takes them.
    pred_refs: [Option<(&'a FrameBuf<u8>, ScaleFactors)>; 3],
    /// Per plane, whether each 4x4 column above has nonzero coefficients:
    /// libvpx's `above_context`, the frame's width aligned to superblocks.
    above_ctx: [Vec<u8>; 3],
    /// Per plane, the same for the superblock's rows: `left_context`.
    left_ctx: [[u8; 16]; 3],
    partition_ctx: PartitionContexts,
    coeffs: [PlaneCoeffs; 3],
    /// Scratch: one transform block's residual and coefficients, the
    /// predictors' output, the tokenizer's energy classes.
    diff: Vec<i16>,
    coeff: Vec<i32>,
    intra_out: intra::Prediction,
    mc: McScratch<u8>,
    token_cache: [u8; 1024],
}

impl<'a> FrameEncoder<'a> {
    /// A frame encoder writing to `recon`, every block yet to be coded.
    /// `inter` is `None` for an intra-only frame.
    pub(crate) fn new(
        h: &'a FrameHeader,
        fc: &'a FrameContext,
        seg: &'a Segmentation,
        quants: &'a Quants,
        src: &'a FrameBuf<u8>,
        recon: &'a mut FrameBuf<u8>,
        inter: Option<InterFrame<'a>>,
    ) -> Self {
        let aligned_cols = (h.mi_cols() + 7) & !7;
        let tiles = (1usize << h.log2_tile_cols) * (1usize << h.log2_tile_rows);
        let pred_refs = inter.map_or([None; 3], |f| {
            f.refs.map(|r| {
                r.map(|r| {
                    let sf = ScaleFactors::new(r.width, r.height, h.width, h.height);
                    (r, sf)
                })
            })
        });
        Self {
            h,
            fc,
            seg,
            quants,
            src,
            recon,
            inter,
            mi: MiGrid::new(h.mi_cols(), h.mi_rows()),
            ext: Vec::new(),
            counts: EncCounts::default(),
            tile_tokens: vec![Vec::new(); tiles],
            mvs: vec![MvRef::default(); h.mi_cols() * h.mi_rows()],
            tile_mi_col_start: 0,
            tile_mi_col_end: h.mi_cols(),
            tile_index: 0,
            pred_refs,
            above_ctx: core::array::from_fn(|_| vec![0u8; 2 * aligned_cols]),
            left_ctx: [[0; 16]; 3],
            partition_ctx: PartitionContexts::new(h.mi_cols()),
            coeffs: core::array::from_fn(|_| PlaneCoeffs::new()),
            diff: vec![0; 32 * 32],
            coeff: vec![0; 32 * 32],
            intra_out: [[0; 32]; 32],
            mc: McScratch::new(),
            token_cache: [0; 1024],
        }
    }

    /// What encoding left.
    pub(crate) fn finish(self) -> Encoded {
        Encoded {
            mi: self.mi,
            ext: self.ext,
            counts: self.counts,
            tile_tokens: self.tile_tokens,
            mvs: self.mvs,
        }
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
                self.tile_mi_col_end = col_end;
                let mut mi_row = offset(tile_row, mi_rows, log2_rows);
                let row_end = offset(tile_row + 1, mi_rows, log2_rows);
                while mi_row < row_end {
                    self.left_ctx = [[0; 16]; 3];
                    self.partition_ctx.new_row();
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
    /// into: libvpx's `nonrd_use_partition` (as `encode_sb_rt`), with the
    /// partition asked of `d` and counted.
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
        let ctx = self.partition_ctx.context(mi_row, mi_col, bsize);
        if let Some(n) = self
            .counts
            .counts
            .partition
            .get_mut(ctx)
            .and_then(|c| c.get_mut(usize::from(partition)))
        {
            *n = n.wrapping_add(1);
        }
        if hbs == 0 {
            // An 8x8 block, whole or in 4x4 parts: one block either way.
            self.encode_block(d, mi_row, mi_col, subsize);
        } else {
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
        if partition != PARTITION_SPLIT || hbs == 0 {
            self.partition_ctx.update(mi_row, mi_col, subsize, bsize);
        }
    }

    /// Where block `bsize` at (`mi_row`, `mi_col`) is: libvpx's
    /// `set_mi_row_col` edge distances.
    pub(crate) fn placement(&self, mi_row: usize, mi_col: usize, bsize: BlockSize) -> Placement {
        let n = |t: &[u8; 13]| i32::from(t.get(usize::from(bsize)).copied().unwrap_or(1));
        let (bw, bh) = (n(&tables::NUM_8X8_WIDE), n(&tables::NUM_8X8_HIGH));
        let (mi_cols, mi_rows) = (self.mi.mi_cols as i32, self.mi.mi_rows as i32);
        Placement {
            mi_row,
            mi_col,
            mb_to_right_edge: (mi_cols - bw - mi_col as i32) * 64,
            mb_to_bottom_edge: (mi_rows - bh - mi_row as i32) * 64,
        }
    }

    /// The block as the decoder's prediction describes it: libvpx's
    /// `MACROBLOCKD` edges and size, for blocks of 8x8 and smaller as an 8x8.
    pub(crate) fn block_pos(&self, mi_row: usize, mi_col: usize, bsize: BlockSize) -> BlockPos {
        let n =
            |t: &[u8; 13], b: BlockSize| usize::from(t.get(usize::from(b)).copied().unwrap_or(1));
        let (bw, bh) = (
            n(&tables::NUM_8X8_WIDE, bsize),
            n(&tables::NUM_8X8_HIGH, bsize),
        );
        let bwl = u32::from(
            tables::B_WIDTH_LOG2
                .get(usize::from(bsize.max(BLOCK_8X8)))
                .copied()
                .unwrap_or(1),
        );
        let (mi_cols, mi_rows) = (self.mi.mi_cols as i32, self.mi.mi_rows as i32);
        BlockPos {
            mi_row,
            mi_col,
            bw,
            bh,
            bwl,
            mb_to_left_edge: -((mi_col * 64) as i32),
            mb_to_right_edge: (mi_cols - bw as i32 - mi_col as i32) * 64,
            mb_to_top_edge: -((mi_row * 64) as i32),
            mb_to_bottom_edge: (mi_rows - bh as i32 - mi_row as i32) * 64,
        }
    }

    /// What motion vector prediction reads here: this frame's blocks so far
    /// in this tile, and the last frame's vectors.
    fn mv_predictor(&self) -> Option<MvPredictor<'_>> {
        let f = self.inter.as_ref()?;
        Some(MvPredictor {
            mi: &self.mi,
            mi_rows: self.mi.mi_rows,
            mi_cols: self.mi.mi_cols,
            tile_start: self.tile_mi_col_start,
            tile_end: self.tile_mi_col_end,
            prev_mvs: f.prev_mvs,
            sign_bias: &f.sign_bias,
        })
    }

    /// The predicted vectors of block `bsize` at (`mi_row`, `mi_col`) for
    /// `ref_frame`: libvpx's `vp9_find_mv_refs` and `vp9_find_best_ref_mvs`.
    /// Zero vectors and context 0 on an intra-only frame.
    pub(crate) fn mv_refs(
        &self,
        mi_row: usize,
        mi_col: usize,
        bsize: BlockSize,
        ref_frame: RefFrame,
    ) -> MvRefs {
        let none = MvRefs {
            nearest: Mv::ZERO,
            near: Mv::ZERO,
            mode_context: 0,
            usehp: false,
        };
        let (Some(p), Some(f)) = (self.mv_predictor(), self.inter.as_ref()) else {
            return none;
        };
        let pos = self.block_pos(mi_row, mi_col, bsize);
        let search = mv_ref_blocks(bsize);
        // NEARMV's search is the full one: both candidates, every one
        // clamped, as vp9_find_mv_refs gives them.
        let (list, _) = p.find_mv_refs(&pos, NEARMV, ref_frame, &search, None);
        let nearest = lower_mv_precision(list[0], f.allow_hp);
        MvRefs {
            nearest,
            near: lower_mv_precision(list[1], f.allow_hp),
            mode_context: u8::try_from(p.mode_context(&search, &pos)).unwrap_or(0),
            usehp: f.allow_hp && crate::block::use_mv_hp(nearest),
        }
    }

    /// Predict one intra transform block into the reconstruction: libvpx's
    /// `vp9_predict_intra_block`, with the neighbours it may read. Returns
    /// the block's top-left pixel in the plane.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn predict_intra(
        &mut self,
        place: &Placement,
        block8: BlockSize,
        plane: usize,
        row: usize,
        col: usize,
        tx_size: TxSize,
        mode: PredictionMode,
    ) -> (usize, usize) {
        let (sx, sy) = self.subsampling(plane);
        let plane_bsize = plane_block_size(block8, sx, sy);
        let bwl = u32::from(
            tables::B_WIDTH_LOG2
                .get(usize::from(plane_bsize))
                .copied()
                .unwrap_or(0),
        );
        let txw = 1usize << tx_size;
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
        (x0, y0)
    }

    /// Predict a luma transform block of the mode search's intra candidate
    /// into the reconstruction: [`Self::predict_intra`], with the
    /// neighbours read from the source picture where libvpx's search skips
    /// the encode (`x->skip_encode`) -- its estimate of what the
    /// reconstruction will be.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn predict_intra_search(
        &mut self,
        place: &Placement,
        bsize: BlockSize,
        row: usize,
        col: usize,
        tx_size: TxSize,
        mode: PredictionMode,
        from_source: bool,
    ) {
        if !from_source {
            self.predict_intra(place, bsize, 0, row, col, tx_size, mode);
            return;
        }
        let bwl = u32::from(
            tables::B_WIDTH_LOG2
                .get(usize::from(bsize))
                .copied()
                .unwrap_or(0),
        );
        let txw = 1usize << tx_size;
        let x0 = place.mi_col * 8 + 4 * col;
        let y0 = place.mi_row * 8 + 4 * row;
        let p = &mut self.recon.planes[0];
        let s = &self.src.planes[0];
        let edges = Edges {
            have_top: row != 0 || place.mi_row != 0,
            have_left: col != 0 || place.mi_col > self.tile_mi_col_start,
            have_right: col + txw < (1usize << bwl),
            past_right: place.mb_to_right_edge < 0,
            past_bottom: place.mb_to_bottom_edge < 0,
            frame_width: p.width,
            frame_height: p.height,
        };
        intra::predict_from(
            &s.data,
            s.stride,
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
    }

    /// Predict block `bsize` at (`mi_row`, `mi_col`) as `mi` says -- the
    /// planes in `planes` -- into the reconstruction: what libvpx's mode
    /// search builds with `vp9_build_inter_predictors_sby`, `_sbuv` and
    /// `_sbp`, and its partitioning with `_sb`. The block's own coding
    /// predicts it again.
    pub(crate) fn predict_inter(
        &mut self,
        mi_row: usize,
        mi_col: usize,
        bsize: BlockSize,
        mi: &ModeInfo,
        planes: core::ops::Range<usize>,
    ) {
        let pos = self.block_pos(mi_row, mi_col, bsize);
        let mut m = *mi;
        m.sb_type = bsize;
        m.mi_row = mi_row;
        m.mi_col = mi_col;
        m.bw = pos.bw;
        m.bh = pos.bh;
        let predicted = inter::build_inter_predictors(
            self.recon,
            0,
            &self.pred_refs,
            &m,
            &pos,
            8,
            &mut self.mc,
            planes,
        );
        // The searches name only references the frame was given; were one
        // missing, the search would read stale pixels and choose worse, but
        // the block's coding predicts again and could not code it.
        debug_assert!(predicted.is_ok(), "a search names a reference not given");
    }

    /// The blocks above and to the left of a cell, as the contexts see them:
    /// none off the top of the frame or left of the tile.
    pub(crate) fn neighbours(
        &self,
        mi_row: usize,
        mi_col: usize,
    ) -> (Option<usize>, Option<usize>) {
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

    /// The block's mode information, from what `modes` asked for and what
    /// the frame and the block's segment allow: an inter block only in an
    /// inter frame; the segment's reference, where it names one; zero motion
    /// where the segment skips, as the decoder infers it (and so no inter
    /// block below 8x8 there, which the stream cannot say). Also the block's
    /// vectors' context, for the bitstream.
    fn mode_info(
        &self,
        modes: &BlockModes,
        mi_row: usize,
        mi_col: usize,
        bsize: BlockSize,
    ) -> (ModeInfo, BlockExt) {
        let h = self.h;
        let seg = self.seg;
        let segment_id = if seg.enabled {
            modes.segment_id.min(7)
        } else {
            0
        };
        let mut inter = if self.inter.is_some() {
            modes.inter
        } else {
            None
        };
        let seg_skip = seg.feature_active(segment_id, SEG_LVL_SKIP);
        if self.inter.is_some() && seg.feature_active(segment_id, SEG_LVL_REF_FRAME) {
            let r = RefFrame::try_from(seg.data(segment_id, SEG_LVL_REF_FRAME)).unwrap_or(0);
            inter = if r == INTRA_FRAME {
                None
            } else {
                debug_assert!(
                    !(seg_skip && bsize < BLOCK_8X8),
                    "a segment that names an inter reference and skips cannot hold a block below 8x8"
                );
                Some(InterModes {
                    ref_frame: r,
                    ..inter.unwrap_or(InterModes {
                        ref_frame: r,
                        mode: ZEROMV,
                        mv: Mv::ZERO,
                        interp_filter: 0,
                        sub: [(ZEROMV, Mv::ZERO); 4],
                    })
                })
            };
        } else if seg_skip && bsize < BLOCK_8X8 {
            // A skipping segment's inter blocks are zero motion at 8x8 and
            // up; one below that is coded intra.
            inter = None;
        }
        if seg_skip && let Some(i) = inter.as_mut() {
            i.mode = ZEROMV;
        }

        let max_tx = tables::MAX_TXSIZE
            .get(usize::from(bsize))
            .copied()
            .unwrap_or(TX_4X4);
        let tx_size = if bsize < BLOCK_8X8 {
            TX_4X4
        } else if h.tx_mode == TX_MODE_SELECT {
            modes.tx_size.min(max_tx)
        } else {
            max_tx.min(biggest_tx(h))
        };
        let n = |t: &[u8; 13]| usize::from(t.get(usize::from(bsize)).copied().unwrap_or(1));
        let mut mi = ModeInfo {
            sb_type: bsize,
            mode: modes.mode,
            uv_mode: modes.uv_mode,
            tx_size,
            ref_frame: [INTRA_FRAME, NO_REF_FRAME],
            segment_id,
            mi_row,
            mi_col,
            bw: n(&tables::NUM_8X8_WIDE),
            bh: n(&tables::NUM_8X8_HIGH),
            ..ModeInfo::default()
        };
        let mut ext = BlockExt::default();
        match (inter, self.inter.as_ref()) {
            (Some(i), Some(f)) => {
                let refs = self.mv_refs(mi_row, mi_col, bsize, i.ref_frame);
                ext.mode_context = refs.mode_context;
                ext.best_mv[0] = refs.nearest;
                mi.ref_frame = [i.ref_frame, NO_REF_FRAME];
                mi.uv_mode = DC_PRED;
                mi.interp_filter = if f.interp_filter == SWITCHABLE {
                    i.interp_filter.min(SWITCHABLE_FILTERS as InterpFilter - 1)
                } else {
                    f.interp_filter
                };
                if bsize >= BLOCK_8X8 {
                    mi.mode = i.mode;
                    mi.mv[0] = match i.mode {
                        NEARESTMV => refs.nearest,
                        NEARMV => refs.near,
                        NEWMV => codable(i.mv, refs.usehp),
                        _ => Mv::ZERO,
                    };
                } else {
                    self.sub8x8_inter(&mut mi, &i, &refs, bsize);
                }
            }
            (_, Some(_)) => {
                // An intra block in an inter frame records the out-of-range
                // filter, as the decoder's read_intra_block_mode_info does,
                // so the filter contexts of the blocks after it agree.
                mi.interp_filter = SWITCHABLE_FILTERS as InterpFilter;
            }
            _ => {}
        }
        if bsize < BLOCK_8X8 && !mi.is_inter() {
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
        (mi, ext)
    }

    /// The modes and vectors of an inter block smaller than 8x8, coded
    /// sub-block by sub-block: libvpx's `vp9_append_sub8x8_mvs_for_idx` for
    /// the nearest and near vectors, each sub-block's from the sub-blocks
    /// before it. The block's own mode and vector are its last sub-block's.
    fn sub8x8_inter(&self, mi: &mut ModeInfo, i: &InterModes, refs: &MvRefs, bsize: BlockSize) {
        let Some(p) = self.mv_predictor() else {
            return;
        };
        let pos = self.block_pos(mi.mi_row, mi.mi_col, bsize);
        let search = mv_ref_blocks(bsize);
        let n = |t: &[u8; 13]| usize::from(t.get(usize::from(bsize)).copied().unwrap_or(1));
        let (w4, h4) = (n(&tables::NUM_4X4_WIDE), n(&tables::NUM_4X4_HIGH));
        for j in sub_blocks(bsize) {
            let (mode, new_mv) = i.sub[j];
            let mv = match mode {
                NEARESTMV | NEARMV => p.append_sub8x8_mvs_for_idx(mi, &search, &pos, mode, j, 0),
                NEWMV => codable(new_mv, refs.usehp),
                _ => Mv::ZERO,
            };
            let b = Bmi {
                mode,
                mv: [mv, Mv::ZERO],
            };
            mi.bmi[j] = b;
            if h4 == 2 {
                mi.bmi[j + 2] = b;
            }
            if w4 == 2 {
                mi.bmi[j + 1] = b;
            }
        }
        mi.mode = mi.bmi[3].mode;
        mi.mv = mi.bmi[3].mv;
    }

    /// Encode one block: libvpx's `encode_b_rt` -- record its modes
    /// (`update_state_rt`), code it (`encode_superblock`), count it
    /// (`update_stats`), and mark the end of its tokens.
    fn encode_block(&mut self, d: &mut dyn Decide, mi_row: usize, mi_col: usize, bsize: BlockSize) {
        let modes = d.modes(self, mi_row, mi_col, bsize);
        let h = self.h;
        let (mut mi, ext) = self.mode_info(&modes, mi_row, mi_col, bsize);
        let (mi_cols, mi_rows) = (self.mi.mi_cols, self.mi.mi_rows);
        let x_mis = mi.bw.min(mi_cols - mi_col);
        let y_mis = mi.bh.min(mi_rows - mi_row);
        let place = self.placement(mi_row, mi_col, bsize);

        let (above, left) = self.neighbours(mi_row, mi_col);
        let Ok(idx) = u32::try_from(self.mi.blocks.len()) else {
            return;
        };
        self.mi.blocks.push(mi);
        self.ext.push(ext);
        for y in 0..y_mis {
            self.mi.cover(mi_row + y, mi_col, x_mis, idx);
        }

        // encode_superblock: predict and code every plane, then tokenize.
        let seg_skip = self.seg.feature_active(mi.segment_id, SEG_LVL_SKIP);
        let block8 = bsize.max(BLOCK_8X8);
        let skip = if mi.is_inter() {
            let pos = self.block_pos(mi_row, mi_col, bsize);
            let predicted = inter::build_inter_predictors_sb(
                self.recon,
                0,
                &self.pred_refs,
                &mi,
                &pos,
                8,
                &mut self.mc,
            );
            debug_assert!(predicted.is_ok(), "a block names a reference not given");
            // vp9_encode_sb: nothing when the mode search skipped the
            // block; the luma transform skipped where it judged luma would
            // quantise to nothing (segment 0 and lossy only, as
            // update_state_rt resets it).
            if modes.skip || seg_skip {
                true
            } else {
                let skip_y = modes.skip_y && mi.segment_id == 0 && !h.lossless();
                !self.encode_inter_planes(&mi, &place, block8, skip_y)
            }
        } else {
            let mut skip = true;
            for plane in 0..3 {
                skip &= !self.encode_intra_plane(&mi, &place, block8, plane, !seg_skip);
            }
            skip
        };
        mi.skip = skip;
        let max_tx = tables::MAX_TXSIZE
            .get(usize::from(bsize))
            .copied()
            .unwrap_or(TX_4X4);
        if mi.is_inter() && (skip || seg_skip) {
            // A skipped inter block codes no transform size: it is the
            // largest the frame allows, as the decoder infers.
            mi.tx_size = max_tx.min(biggest_tx(h));
        }
        if let Some(b) = self.mi.blocks.get_mut(idx as usize) {
            *b = mi;
        }

        let a = above.and_then(|i| self.mi.blocks.get(i)).copied();
        let l = left.and_then(|i| self.mi.blocks.get(i)).copied();
        if mi.is_inter() {
            self.count_inter(&mi, &ext, a.as_ref(), l.as_ref());
        } else {
            self.count_intra(&mi);
        }
        if self.inter.is_some() {
            self.count_stats(&mi, &ext, a.as_ref(), l.as_ref());
        }
        self.tokenize_sb(&mi, &place, block8, seg_skip, a.as_ref(), l.as_ref());

        // The transform size's counts.
        let tx = &mut self.counts;
        if h.tx_mode == TX_MODE_SELECT
            && bsize >= BLOCK_8X8
            && !(mi.is_inter() && (mi.skip || seg_skip))
        {
            let ctx = context::tx_size_context(a.as_ref(), l.as_ref(), max_tx);
            let t = &mut tx.counts.tx;
            let slot = match max_tx {
                TX_8X8 => t
                    .p8x8
                    .get_mut(ctx)
                    .and_then(|s| s.get_mut(usize::from(mi.tx_size))),
                TX_16X16 => t
                    .p16x16
                    .get_mut(ctx)
                    .and_then(|s| s.get_mut(usize::from(mi.tx_size))),
                TX_32X32 => t
                    .p32x32
                    .get_mut(ctx)
                    .and_then(|s| s.get_mut(usize::from(mi.tx_size))),
                _ => None,
            };
            if let Some(n) = slot {
                *n = n.wrapping_add(1);
            }
        }
        let uv_tx = uv_tx_size(bsize, mi.tx_size, h.ss_x, h.ss_y);
        for t in [mi.tx_size, uv_tx] {
            if let Some(n) = tx.tx_totals.get_mut(usize::from(t)) {
                *n = n.wrapping_add(1);
            }
        }

        // update_state_rt: the vectors the next frame predicts from.
        let cols = self.mi.mi_cols;
        for y in 0..y_mis {
            let start = (mi_row + y) * cols + mi_col;
            if let Some(cells) = self.mvs.get_mut(start..start + x_mis) {
                cells.fill(MvRef {
                    mv: mi.mv,
                    ref_frame: mi.ref_frame,
                });
            }
        }

        if let Some(tokens) = self.tile_tokens.get_mut(self.tile_index) {
            tokens.push(TokenExtra::END_OF_BLOCK_TOKENS);
        }
    }

    /// An intra block's modes, counted: libvpx's `sum_intra_stats`.
    fn count_intra(&mut self, mi: &ModeInfo) {
        let c = &mut self.counts.counts;
        let bump = |n: Option<&mut u32>| {
            if let Some(n) = n {
                *n = n.wrapping_add(1);
            }
        };
        if mi.sb_type < BLOCK_8X8 {
            let w4 = usize::from(
                tables::NUM_4X4_WIDE
                    .get(usize::from(mi.sb_type))
                    .copied()
                    .unwrap_or(1),
            );
            let h4 = usize::from(
                tables::NUM_4X4_HIGH
                    .get(usize::from(mi.sb_type))
                    .copied()
                    .unwrap_or(1),
            );
            for idy in (0..2).step_by(h4.max(1)) {
                for idx in (0..2).step_by(w4.max(1)) {
                    let m = mi.bmi.get(idy * 2 + idx).map_or(0, |b| usize::from(b.mode));
                    bump(c.y_mode.get_mut(0).and_then(|y| y.get_mut(m)));
                }
            }
        } else {
            let group = usize::from(
                tables::SIZE_GROUP
                    .get(usize::from(mi.sb_type))
                    .copied()
                    .unwrap_or(0),
            );
            bump(
                c.y_mode
                    .get_mut(group)
                    .and_then(|y| y.get_mut(usize::from(mi.mode))),
            );
        }
        bump(
            c.uv_mode
                .get_mut(usize::from(mi.mode))
                .and_then(|u| u.get_mut(usize::from(mi.uv_mode))),
        );
    }

    /// An inter block's vector and filter, counted: libvpx's
    /// `vp9_update_mv_count` and the switchable filter count in
    /// `update_state_rt`.
    fn count_inter(
        &mut self,
        mi: &ModeInfo,
        ext: &BlockExt,
        above: Option<&ModeInfo>,
        left: Option<&ModeInfo>,
    ) {
        let switchable = self
            .inter
            .as_ref()
            .is_some_and(|f| f.interp_filter == SWITCHABLE);
        let c = &mut self.counts.counts;
        // vp9_update_mv_count: each new vector, against the block's best.
        let new_mvs: Vec<Mv> = if mi.sb_type < BLOCK_8X8 {
            sub_blocks(mi.sb_type)
                .into_iter()
                .filter(|&j| mi.bmi[j].mode == NEWMV)
                .map(|j| mi.bmi[j].mv[0])
                .collect()
        } else if mi.mode == NEWMV {
            vec![mi.mv[0]]
        } else {
            Vec::new()
        };
        for mv in new_mvs {
            let diff = Mv {
                row: mv.row.wrapping_sub(ext.best_mv[0].row),
                col: mv.col.wrapping_sub(ext.best_mv[0].col),
            };
            inc_mv(diff, &mut c.mv);
        }
        if switchable {
            let ctx = context::switchable_interp_context(above, left);
            if let Some(n) = c
                .switchable_interp
                .get_mut(ctx)
                .and_then(|s| s.get_mut(usize::from(mi.interp_filter)))
            {
                *n = n.wrapping_add(1);
            }
        }
    }

    /// An inter frame's block, counted: libvpx's `update_stats` -- intra or
    /// inter, the reference, and the inter mode.
    fn count_stats(
        &mut self,
        mi: &ModeInfo,
        ext: &BlockExt,
        above: Option<&ModeInfo>,
        left: Option<&ModeInfo>,
    ) {
        let seg = self.seg;
        let c = &mut self.counts.counts;
        let bump = |n: Option<&mut u32>| {
            if let Some(n) = n {
                *n = n.wrapping_add(1);
            }
        };
        if !seg.feature_active(mi.segment_id, SEG_LVL_REF_FRAME) {
            let ctx = context::intra_inter_context(above, left);
            bump(
                c.intra_inter
                    .get_mut(ctx)
                    .and_then(|s| s.get_mut(usize::from(mi.is_inter()))),
            );
            if mi.is_inter() {
                let r = mi.ref_frame[0];
                let p1 = context::single_ref_p1_context(above, left);
                bump(
                    c.single_ref
                        .get_mut(p1)
                        .and_then(|s| s[0].get_mut(usize::from(r != LAST_FRAME))),
                );
                if r != LAST_FRAME {
                    let p2 = context::single_ref_p2_context(above, left);
                    bump(
                        c.single_ref
                            .get_mut(p2)
                            .and_then(|s| s[1].get_mut(usize::from(r != GOLDEN_FRAME))),
                    );
                }
            }
        }
        if mi.is_inter() && !seg.feature_active(mi.segment_id, SEG_LVL_SKIP) {
            let modes: Vec<PredictionMode> = if mi.sb_type < BLOCK_8X8 {
                sub_blocks(mi.sb_type)
                    .into_iter()
                    .map(|j| mi.bmi[j].mode)
                    .collect()
            } else {
                vec![mi.mode]
            };
            for mode in modes {
                let offset = usize::from(mode.saturating_sub(NEARESTMV));
                bump(
                    c.inter_mode
                        .get_mut(usize::from(ext.mode_context))
                        .and_then(|s| s.get_mut(offset)),
                );
            }
        }
    }

    /// The transform blocks of one plane of a block, in coding order, as
    /// (index in 4x4 units, row and column in 4x4 units): libvpx's
    /// `vp9_foreach_transformed_block_in_plane`, which skips those wholly
    /// past the frame's edge.
    pub(crate) fn transform_blocks(
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

    /// A plane's transform and scan for one of its transform blocks: libvpx's
    /// `get_tx_type_4x4`, `get_tx_type` and `get_scan`. Only intra luma
    /// blocks below 32x32 take a hybrid transform.
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
        let tx_type = if plane != 0 || self.h.lossless() || tx_size == TX_32X32 || mi.is_inter() {
            DCT_DCT
        } else {
            INTRA_MODE_TO_TX_TYPE
                .get(usize::from(mode))
                .copied()
                .unwrap_or(DCT_DCT)
        };
        (mode, tx_type, scan_for(tx_size, tx_type))
    }

    /// Subtract the prediction (in the reconstruction) from the source over
    /// one `n` x `n` transform block at (`x0`, `y0`) of `plane`: libvpx's
    /// `vpx_subtract_block`, into `diff`.
    fn subtract(&mut self, plane: usize, x0: usize, y0: usize, n: usize) {
        let s = &self.src.planes[plane.min(2)];
        let p = &self.recon.planes[plane.min(2)];
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
    }

    /// Code one plane of an intra block, transform block by transform
    /// block: libvpx's `vp9_encode_intra_block_plane` and
    /// `vp9_encode_block_intra`. Without `residual` (a segment that skips)
    /// the blocks are only predicted. Returns whether any coefficient is
    /// nonzero.
    fn encode_intra_plane(
        &mut self,
        mi: &ModeInfo,
        place: &Placement,
        block8: BlockSize,
        plane: usize,
        residual: bool,
    ) -> bool {
        let h = self.h;
        let tx_size = if plane == 0 {
            mi.tx_size
        } else {
            uv_tx_size(mi.sb_type, mi.tx_size, h.ss_x, h.ss_y)
        };
        let qindex = self.seg.qindex(mi.segment_id, h.quant.base_qindex);
        let qs = self.quants.get(plane, usize::try_from(qindex).unwrap_or(0));
        let lossless = h.lossless();
        let n = 4usize << tx_size;
        let mut any = false;
        for (block, row, col) in self.transform_blocks(place, block8, plane, tx_size) {
            let (mode, tx_type, scan) = self.transform_of(mi, plane, block, tx_size);
            let (x0, y0) = self.predict_intra(place, block8, plane, row, col, tx_size, mode);
            if !residual {
                continue;
            }
            self.subtract(plane, x0, y0, n);
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
                let p = &mut self.recon.planes[plane.min(2)];
                if let Some(dst) = p.data.get_mut(y0 * p.stride + x0..) {
                    idct::inverse_transform_add(
                        tx_size, tx_type, lossless, eob, dqcoeff, dst, p.stride, 8,
                    );
                }
            }
        }
        any
    }

    /// Code an inter block's residual, plane by plane, over its prediction
    /// in the reconstruction: libvpx's `vp9_encode_sb` with the realtime
    /// path's `vp9_xform_quant_fp` -- the DCT, the fast quantiser. With
    /// `skip_y` the luma transform is skipped. Returns whether any
    /// coefficient is nonzero.
    fn encode_inter_planes(
        &mut self,
        mi: &ModeInfo,
        place: &Placement,
        block8: BlockSize,
        skip_y: bool,
    ) -> bool {
        let h = self.h;
        let qindex = self.seg.qindex(mi.segment_id, h.quant.base_qindex);
        let lossless = h.lossless();
        let mut any = false;
        for plane in 0..3 {
            let tx_size = if plane == 0 {
                mi.tx_size
            } else {
                uv_tx_size(mi.sb_type, mi.tx_size, h.ss_x, h.ss_y)
            };
            let qs = self.quants.get(plane, usize::try_from(qindex).unwrap_or(0));
            let n = 4usize << tx_size;
            let scan = scan_for(tx_size, DCT_DCT);
            let (sx, sy) = self.subsampling(plane);
            for (block, row, col) in self.transform_blocks(place, block8, plane, tx_size) {
                if plane == 0 && skip_y {
                    if let Some(e) = self.coeffs[0].eobs.get_mut(block) {
                        *e = 0;
                    }
                    continue;
                }
                let x0 = ((place.mi_col * 8) >> sx) + 4 * col;
                let y0 = ((place.mi_row * 8) >> sy) + 4 * row;
                self.subtract(plane, x0, y0, n);
                forward_transform(&self.diff, n, tx_size, DCT_DCT, lossless, &mut self.coeff);
                let start = block * 16;
                let pc = &mut self.coeffs[plane.min(2)];
                let (Some(qcoeff), Some(dqcoeff)) = (
                    pc.qcoeff.get_mut(start..start + n * n),
                    pc.dqcoeff.get_mut(start..start + n * n),
                ) else {
                    continue;
                };
                let eob = quantize_fp(
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
                    let p = &mut self.recon.planes[plane.min(2)];
                    if let Some(dst) = p.data.get_mut(y0 * p.stride + x0..) {
                        idct::inverse_transform_add(
                            tx_size, DCT_DCT, lossless, eob, dqcoeff, dst, p.stride, 8,
                        );
                    }
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
        let ref_type = usize::from(mi.is_inter());
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
                        ref_type,
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

/// A new vector as its coding can carry it: without eighth pixels
/// (`usehp` false) a difference from an even reference must be even, so an
/// odd component is moved toward zero as libvpx's `lower_mv_precision`
/// moves one. A search at the right precision never needs it; a decision
/// made for another reference -- one a segment overrides -- can.
fn codable(mv: Mv, usehp: bool) -> Mv {
    debug_assert!(
        crate::block::is_mv_valid(mv),
        "a new vector outside the range the stream can code"
    );
    if usehp {
        mv
    } else {
        lower_mv_precision(mv, false)
    }
}

/// The 4x4 sub-blocks a block smaller than 8x8 codes a mode for, in order:
/// libvpx's `idy`/`idx` loops over `num_4x4_blocks_*_lookup` -- all four of a
/// 4x4 block, 0 and 1 of a 4x8, 0 and 2 of an 8x4.
pub(crate) fn sub_blocks(bsize: BlockSize) -> Vec<usize> {
    let n = |t: &[u8; 13]| usize::from(t.get(usize::from(bsize)).copied().unwrap_or(1)).max(1);
    let (w4, h4) = (n(&tables::NUM_4X4_WIDE), n(&tables::NUM_4X4_HIGH));
    let mut out = Vec::with_capacity(4);
    for idy in (0..2).step_by(h4) {
        for idx in (0..2).step_by(w4) {
            out.push(idy * 2 + idx);
        }
    }
    out
}

/// The largest transform the frame's mode allows: libvpx's
/// `tx_mode_to_biggest_tx_size`.
fn biggest_tx(h: &FrameHeader) -> TxSize {
    tables::TX_MODE_TO_BIGGEST_TX_SIZE
        .get(usize::from(h.tx_mode))
        .copied()
        .unwrap_or(TX_4X4)
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
/// `vp9_encode_block_intra` and `vp9_xform_quant_fp` -- the Walsh-Hadamard
/// transform for lossless 4x4 blocks, the hybrid transforms where an intra
/// mode asks, and for 32x32 the reduced-precision DCT the realtime speeds use
/// (`use_lp32x32fdct`).
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
