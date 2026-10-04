//! libvpx's realtime decisions, as a [`Decide`]: the non-RD path
//! (`encode_nonrd_sb_row`) that its speeds 5 and up take.
//!
//! Each superblock starts with what the source says about it: skin
//! (`vp9_compute_skin_sb`) and how much it changed since the last picture
//! (`avg_source_sad`). Then it is partitioned by variance
//! (`choose_partitioning`): on a key frame against a flat grey, on an inter
//! frame against its prediction from the last frame -- or, where it barely
//! changed, kept whole or given the last frame's partition again. The
//! partition is walked as `nonrd_use_partition` walks it, and each block's
//! modes picked by `vp9_pick_intra_mode` (a key frame's) or
//! `vp9_pick_inter_mode`, the cyclic refresh then settling its segment
//! (`update_state_rt`).
//!
//! An inter frame of 352x288 pixels or fewer is partitioned by search
//! instead (`nonrd_pick_partition`, [`Learned`]): each square block of the
//! superblock tried whole and cut in four as a small network allows
//! (`mlpart`), every candidate's modes picked, the cheaper kept -- and only
//! then is the superblock coded, from what the search kept.
//!
//! What carries from frame to frame -- the adaptive mode thresholds (each
//! tile's own), the partitions to copy, the skin map, how long each
//! superblock has been still, and the contents of libvpx's mode-information
//! buffers, which its decisions read stale -- is [`RtState`]. Everything in
//! it but the thresholds and one register is kept per cell or superblock,
//! and a block reads and writes only its own tile column's, so tile columns
//! coded on threads of their own each take a copy and
//! [`RtState::absorb_columns`] puts their columns back.
//!
//! Translated into Rust from libvpx v1.17.0's `vp9/encoder/vp9_encodeframe.c`
//! (copyright the WebM project authors), used under libvpx's BSD licence and
//! patent grant (`licenses/libvpx-LICENSE`, `licenses/libvpx-PATENTS`).

#![allow(
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    reason = "cell, superblock and pixel coordinates bounded by the frame (at most 65536 a side), and libvpx's integer arithmetic"
)]
#![allow(
    clippy::indexing_slicing,
    reason = "indices are cells and superblocks of the frame the state was sized for, block sizes below 13 and reference frames below 4"
)]

use crate::block::ModeInfo;
use crate::common::{
    BILINEAR, BLOCK_4X4, BLOCK_8X8, BLOCK_32X32, BLOCK_64X64, BLOCK_SIZES, BlockSize, INTRA_MODES,
    LAST_FRAME, Mv, PARTITION_CONTEXTS, PARTITION_NONE, PARTITION_SPLIT, PARTITION_TREE,
    PARTITION_TYPES, Partition,
};
use crate::enc::aq_cyclicrefresh::{CR_SEGMENT_ID_BASE, CyclicRefresh, segment_boosted};
use crate::enc::content;
use crate::enc::encodeframe::{BlockModes, Decide, FrameEncoder};
use crate::enc::mcomp::{self, MvLimits, Search};
use crate::enc::mlpart;
use crate::enc::partition::{self, ContentState, InterSb, NoiseLevel, SbPartition};
use crate::enc::pickinter::{self, SbState, SearchFrame};
use crate::enc::rd::{
    MAX_MODES, RD_THRESH_INIT_FACT, RDDIV_BITS, cost_tokens, kf_y_mode_costs, rdcost,
};
use crate::frame::FrameBuf;
use crate::tables;

/// What one 8x8 cell of libvpx's mode-information buffer last held, as far
/// as any decision reads it: the block size `set_block_size` wrote there,
/// the vector a block left, and whether the block coded at the cell coded
/// nothing -- which the mode search never writes, so a partition search's
/// unfinished blocks keep the cell's old one (see `Learned`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct StaleMi {
    pub sb_type: BlockSize,
    pub mv: Mv,
    pub skip: bool,
}

/// What libvpx's realtime decisions keep from frame to frame.
#[derive(Clone, Debug)]
pub(crate) struct RtState {
    mi_rows: usize,
    mi_cols: usize,
    sb_cols: usize,
    /// Each tile's adaptive mode thresholds, by block size: libvpx's
    /// `tile_data->thresh_freq_fact`, which every tile keeps for itself from
    /// frame to frame. See [`RtState::thresholds`].
    thresh_freq_fact: Vec<[[i32; MAX_MODES]; BLOCK_SIZES]>,
    /// The last partition each cell had, for copying (`prev_partition`),
    /// and per superblock its segment, its low-variance flags and how many
    /// frames in a row it was copied (`prev_segment_id`,
    /// `prev_variance_low`, `copied_frame_cnt`).
    prev_partition: Vec<BlockSize>,
    prev_segment_id: Vec<u8>,
    prev_variance_low: Vec<[bool; 25]>,
    copied_frame_cnt: Vec<u8>,
    /// Per superblock, for how many frames in a row its source changed
    /// little (`content_state_sb_fd`).
    content_state_sb_fd: Vec<u8>,
    /// Per 8x8 cell, whether its 16x16 block is skin (`skin_map`).
    skin_map: Vec<bool>,
    /// libvpx's two mode-information buffers, which alternate frame by
    /// frame: the copy `set_low_temp_var_flag` reads a vector from was last
    /// written two frames ago, or earlier.
    stale_mi: [Vec<StaleMi>; 2],
    cur_mi: usize,
    /// libvpx's `x->last_sb_high_content`: kept from the last superblock
    /// that set it, across superblocks and frames.
    last_sb_high_content: i32,
}

impl RtState {
    /// The state of a new encoder of `mi_rows` x `mi_cols` cells, as
    /// libvpx's allocations leave it.
    pub(crate) fn new(mi_rows: usize, mi_cols: usize) -> Self {
        let sb_cols = mi_cols.div_ceil(8);
        let sbs = sb_cols * mi_rows.div_ceil(8);
        let cells = mi_rows * mi_cols;
        Self {
            mi_rows,
            mi_cols,
            sb_cols,
            thresh_freq_fact: Vec::new(),
            prev_partition: vec![BLOCK_4X4; cells],
            prev_segment_id: vec![0; sbs],
            prev_variance_low: vec![[false; 25]; sbs],
            copied_frame_cnt: vec![0; sbs],
            content_state_sb_fd: vec![0; sbs],
            skin_map: vec![false; cells],
            stale_mi: [
                vec![StaleMi::default(); cells],
                vec![StaleMi::default(); cells],
            ],
            cur_mi: 0,
            last_sb_high_content: 0,
        }
    }

    fn sb_index(&self, mi_row: usize, mi_col: usize) -> usize {
        (mi_row >> 3) * self.sb_cols + (mi_col >> 3)
    }

    /// Take in what a tile column coded apart -- on a thread of its own, from
    /// a clone of this state -- left: its cells' and superblocks' entries
    /// (the column is the cells `col0..col1`, whole superblocks but at the
    /// frame's edge), the thresholds of its `tiles`, and, if it is the
    /// frame's last column, `x->last_sb_high_content` as its last
    /// superblock left it -- where a single thread, coding the columns in
    /// order, would leave it. A column reads and writes no other column's
    /// entries, so nothing else of `part` differs from this state.
    pub(crate) fn absorb_columns(
        &mut self,
        part: &RtState,
        col0: usize,
        col1: usize,
        tiles: &[usize],
        last: bool,
    ) {
        let cols = self.mi_cols.max(1);
        copy_columns(
            &mut self.prev_partition,
            &part.prev_partition,
            cols,
            col0,
            col1,
        );
        copy_columns(&mut self.skin_map, &part.skin_map, cols, col0, col1);
        for (d, s) in self.stale_mi.iter_mut().zip(&part.stale_mi) {
            copy_columns(d, s, cols, col0, col1);
        }
        let (sbs, s0, s1) = (self.sb_cols.max(1), col0 / 8, col1.div_ceil(8));
        copy_columns(
            &mut self.prev_segment_id,
            &part.prev_segment_id,
            sbs,
            s0,
            s1,
        );
        copy_columns(
            &mut self.prev_variance_low,
            &part.prev_variance_low,
            sbs,
            s0,
            s1,
        );
        copy_columns(
            &mut self.copied_frame_cnt,
            &part.copied_frame_cnt,
            sbs,
            s0,
            s1,
        );
        copy_columns(
            &mut self.content_state_sb_fd,
            &part.content_state_sb_fd,
            sbs,
            s0,
            s1,
        );
        for &t in tiles {
            if let Some(th) = part.thresh_freq_fact.get(t) {
                *self.tile_thresholds(t) = *th;
            }
        }
        if last {
            self.last_sb_high_content = part.last_sb_high_content;
        }
    }

    /// Tile `tile`'s adaptive mode thresholds, by block size. A tile's start
    /// at libvpx's initial factor the first time it is coded, as
    /// `vp9_init_tile_data` sets them when it allocates the tiles' data.
    fn tile_thresholds(&mut self, tile: usize) -> &mut [[i32; MAX_MODES]; BLOCK_SIZES] {
        while self.thresh_freq_fact.len() <= tile {
            self.thresh_freq_fact
                .push([[RD_THRESH_INIT_FACT; MAX_MODES]; BLOCK_SIZES]);
        }
        &mut self.thresh_freq_fact[tile]
    }

    /// Tile `tile`'s adaptive mode thresholds for blocks of `bsize`.
    fn thresholds(&mut self, tile: usize, bsize: BlockSize) -> &mut [i32; MAX_MODES] {
        &mut self.tile_thresholds(tile)[usize::from(bsize)]
    }

    /// After a shown frame: libvpx's `vp9_swap_mi_and_prev_mi`.
    pub(crate) fn swap_mi(&mut self) {
        self.cur_mi ^= 1;
    }

    /// A key frame clears the buffer the next frame will use: libvpx's
    /// `vp9_setup_past_independence` clearing `prev_mip`.
    pub(crate) fn clear_prev_mi(&mut self) {
        self.stale_mi[self.cur_mi ^ 1].fill(StaleMi::default());
    }

    fn mi_at(&mut self, mi_row: usize, mi_col: usize) -> Option<&mut StaleMi> {
        if mi_row < self.mi_rows && mi_col < self.mi_cols {
            self.stale_mi[self.cur_mi].get_mut(mi_row * self.mi_cols + mi_col)
        } else {
            None
        }
    }

    /// Apply `set_block_size` calls in order; returns the cell the last one
    /// inside the frame wrote (where libvpx's `xd->mi` is left), if any.
    fn apply_set_calls(&mut self, calls: &[(usize, usize, BlockSize)]) -> Option<(usize, usize)> {
        let mut last = None;
        for &(r, c, bs) in calls {
            if let Some(m) = self.mi_at(r, c) {
                m.sb_type = bs;
                last = Some((r, c));
            }
        }
        last
    }
}

/// Copy columns `c0..c1` of every row of `src`, a row-major array `width`
/// to a row, into `dst`, laid out alike.
pub(crate) fn copy_columns<T: Copy>(dst: &mut [T], src: &[T], width: usize, c0: usize, c1: usize) {
    for (d, s) in dst
        .chunks_exact_mut(width.max(1))
        .zip(src.chunks_exact(width.max(1)))
    {
        if let (Some(d), Some(s)) = (d.get_mut(c0..c1.min(width)), s.get(c0..c1.min(width))) {
            d.copy_from_slice(s);
        }
    }
}

/// The lowest segment over a block's cells in `map`: libvpx's
/// `get_segment_id`.
fn get_segment_id(
    map: &[u8],
    bsize: BlockSize,
    mi_row: usize,
    mi_col: usize,
    mi_rows: usize,
    mi_cols: usize,
) -> u8 {
    let bw = usize::from(tables::NUM_8X8_WIDE[usize::from(bsize)]);
    let bh = usize::from(tables::NUM_8X8_HIGH[usize::from(bsize)]);
    let xmis = (mi_cols - mi_col).min(bw);
    let ymis = (mi_rows - mi_row).min(bh);
    let mut id = 8u8;
    for y in 0..ymis {
        for x in 0..xmis {
            id = id.min(
                map.get((mi_row + y) * mi_cols + mi_col + x)
                    .copied()
                    .unwrap_or(0),
            );
        }
    }
    id.min(7)
}

/// The blocks a partition search kept under `node` (block `bsize` at
/// (`mi_row`, `mi_col`)), in coding order: libvpx's `encode_sb_rt` walk.
fn collect_leaves(
    node: &SearchNode,
    mi_row: usize,
    mi_col: usize,
    bsize: BlockSize,
    mi_rows: usize,
    mi_cols: usize,
    out: &mut Vec<(usize, usize, BlockSize, Candidate)>,
) {
    if mi_row >= mi_rows || mi_col >= mi_cols {
        return;
    }
    match node.chosen {
        Some(PARTITION_NONE) => {
            if let Some(c) = node.whole {
                out.push((mi_row, mi_col, bsize, c));
            }
        }
        Some(PARTITION_SPLIT) => {
            let ms = usize::from(tables::NUM_8X8_WIDE[usize::from(bsize)]) / 2;
            let subsize = tables::SUBSIZE[usize::from(PARTITION_SPLIT)][usize::from(bsize)];
            for (i, quarter) in node.quarters.iter().enumerate() {
                let (r, c) = (mi_row + (i >> 1) * ms, mi_col + (i & 1) * ms);
                collect_leaves(quarter, r, c, subsize, mi_rows, mi_cols, out);
            }
        }
        _ => {}
    }
}

/// The partition `nonrd_use_partition` walks at block `bsize` of a
/// superblock: the size chosen where the block starts.
fn walk_partition(
    sb: Option<&SbPartition>,
    mi_row: usize,
    mi_col: usize,
    bsize: BlockSize,
) -> Partition {
    let subsize = if bsize >= BLOCK_8X8 {
        sb.map_or(BLOCK_4X4, |sb| sb.size_at(mi_row, mi_col))
    } else {
        BLOCK_4X4
    };
    let bsl = usize::from(
        tables::B_WIDTH_LOG2
            .get(usize::from(bsize))
            .copied()
            .unwrap_or(0),
    );
    tables::PARTITION_LOOKUP
        .get(bsl)
        .and_then(|p| p.get(usize::from(subsize)))
        .copied()
        .unwrap_or(PARTITION_NONE)
}

/// libvpx's realtime decisions for a frame.
pub(crate) enum RtDecisions<'a> {
    Key(KeyDecisions<'a>),
    Inter(Box<InterDecisions<'a>>),
}

/// A key frame's.
pub(crate) struct KeyDecisions<'a> {
    /// The rate-distortion multiplier: libvpx's `RDMULT`.
    rdmult: i32,
    /// The variance partitioning's thresholds: libvpx's `vbp_thresholds`.
    thresholds: [i64; 4],
    y_mode_costs: Box<[[[i32; INTRA_MODES]; INTRA_MODES]; INTRA_MODES]>,
    sb: Option<SbPartition>,
    state: &'a mut RtState,
    /// The last source picture, if any: a key frame still measures how
    /// much each superblock changed, for the frames after it.
    last_src: Option<&'a FrameBuf<u8>>,
}

impl<'a> RtDecisions<'a> {
    /// The decisions for a key frame whose multiplier is `rdmult` and whose
    /// luma AC step is `y_ac_dequant`: libvpx's `vp9_initialize_rd_consts`
    /// and `vp9_set_variance_partition_thresholds`.
    pub(crate) fn key_frame(
        rdmult: i32,
        y_ac_dequant: i32,
        state: &'a mut RtState,
        last_src: Option<&'a FrameBuf<u8>>,
    ) -> Self {
        Self::Key(KeyDecisions {
            rdmult,
            thresholds: partition::key_frame_thresholds(y_ac_dequant),
            y_mode_costs: kf_y_mode_costs(),
            sb: None,
            state,
            last_src,
        })
    }
}

/// An inter frame's settings for its decisions, fixed for the frame.
#[derive(Clone, Copy)]
pub(crate) struct InterSettings {
    pub base_qindex: i32,
    /// The partitioning's whole-superblock and copy thresholds:
    /// `vbp_threshold_sad` and `vbp_threshold_copy`.
    pub vbp_threshold_sad: i64,
    pub vbp_threshold_copy: i64,
    /// The key frame's 8x8 threshold libvpx leaves in place.
    pub vbp_threshold_8x8: i64,
    /// The noise level the partitioning's thresholds read, if libvpx
    /// estimates noise (`vp9_noise_estimate_extract_level`).
    pub noise_extracted: Option<NoiseLevel>,
    pub avg_inter_qindex: i32,
    /// The partition copy (`copy_partition_flag`) and its limit.
    pub copy_partition: bool,
    pub max_copied_frame: u8,
    pub frames_since_key: i32,
    pub use_skin_detection: bool,
    /// The source's change per superblock is measured (`use_source_sad`).
    pub use_source_sad: bool,
    /// The encode runs cyclic refresh (libvpx's `aq_mode ==
    /// CYCLIC_REFRESH_AQ`), whether or not this frame codes segments: see
    /// [`SearchFrame::cyclic_refresh`](crate::enc::pickinter::SearchFrame).
    pub cyclic_refresh: bool,
    /// Partition by search, trimmed by a network, rather than by variance:
    /// libvpx's speed 8 for inter frames of 352x288 pixels or fewer
    /// (`ML_BASED_PARTITION`).
    pub learned_partition: bool,
}

/// A block the learned partitioning tried whole: libvpx's
/// `PICK_MODE_CONTEXT` after `nonrd_pick_sb_modes`, kept for the coding.
#[derive(Clone, Copy, Debug)]
struct Candidate {
    picked: pickinter::Picked,
}

/// One block of a superblock's partition search: libvpx's `PC_TREE` node.
#[derive(Debug, Default)]
struct SearchNode {
    /// `PARTITION_NONE` or `PARTITION_SPLIT`, whichever the search kept;
    /// `None` if neither came in under its budget.
    chosen: Option<Partition>,
    /// What trying the block whole found.
    whole: Option<Candidate>,
    /// Its quarters, if it was cut.
    quarters: Vec<SearchNode>,
}

/// libvpx's `RD_COST` as the partition search adds it up.
#[derive(Clone, Copy, Debug)]
struct SearchCost {
    rate: i32,
    dist: i64,
    rdcost: i64,
}

/// The learned partitioning's state (`InterSettings::learned_partition`):
/// libvpx's `nonrd_pick_partition`, which searches all of a superblock
/// before coding any of it. What the search reads that the variance path
/// never did is libvpx's state between blocks, which here carries from one
/// block's search to another's and to the coding -- so it is kept as libvpx
/// keeps it.
pub(crate) struct Learned {
    /// The superblock's luma predicted from the last frame at its estimated
    /// vector, 64x64, rows 64 apart: libvpx's `x->est_pred`
    /// (`get_estimated_pred`), which the network's features measure.
    est_pred: Vec<u8>,
    /// The blocks the search kept, in coding order, for the coding.
    leaves: Vec<(usize, usize, BlockSize, Candidate)>,
    /// libvpx's `x->skip` between blocks. Each mode search leaves its
    /// verdict there and each block's coding the block's own, and the
    /// cyclic refresh reads it before the coding sets it -- so a
    /// superblock's first block is judged by the search's last candidate,
    /// and every other by the block coded before it.
    x_skip: bool,
    /// libvpx's `x->rdmult` between blocks: the last mode search's (a
    /// boosted segment's own), with which the search prices a split before
    /// searching it.
    rdmult: i32,
    /// libvpx's `cpi->partition_cost`, by context and partition: from the
    /// frame's probabilities, every frame that searches partitions.
    partition_cost: [[i32; PARTITION_TYPES]; PARTITION_CONTEXTS],
    /// The frame's luma DC quantiser step: the network's first feature.
    dc_q: i32,
}

impl Learned {
    /// The state for a frame whose luma DC step is `dc_q`.
    pub(crate) fn new(dc_q: i32) -> Self {
        Self {
            est_pred: vec![0; 64 * 64],
            leaves: Vec::new(),
            x_skip: false,
            rdmult: 0,
            partition_cost: [[0; PARTITION_TYPES]; PARTITION_CONTEXTS],
            dc_q,
        }
    }
}

/// An inter frame's.
pub(crate) struct InterDecisions<'a> {
    pub search: SearchFrame<'a>,
    pub settings: InterSettings,
    pub state: &'a mut RtState,
    pub cr: &'a mut CyclicRefresh,
    pub consec_zero_mv: &'a [u8],
    /// The last source picture, its edges repeated to whole superblocks.
    pub last_src: &'a FrameBuf<u8>,
    /// The superblock's state, and its partition.
    pub sb: SbState,
    pub part: Option<SbPartition>,
    /// The learned partitioning's, where it runs.
    pub learned: Learned,
}

impl<'a> RtDecisions<'a> {
    /// The decisions for an inter frame.
    pub(crate) fn inter(d: InterDecisions<'a>) -> Self {
        Self::Inter(Box::new(d))
    }
}

impl Decide for RtDecisions<'_> {
    fn partition(
        &mut self,
        f: &mut FrameEncoder<'_>,
        mi_row: usize,
        mi_col: usize,
        bsize: BlockSize,
    ) -> Partition {
        match self {
            Self::Key(k) => {
                if bsize == BLOCK_64X64
                    && let Some(last) = k.last_src
                {
                    // avg_source_sad's count of still frames.
                    let (sad, _) = content::avg_source_sad(
                        &f.src.planes[0],
                        &last.planes[0],
                        mi_col * 8,
                        mi_row * 8,
                    );
                    let sbi = k.state.sb_index(mi_row, mi_col);
                    let fd = &mut k.state.content_state_sb_fd[sbi];
                    *fd = if sad < 12_000 {
                        fd.saturating_add(1)
                    } else {
                        0
                    };
                }
                if bsize == BLOCK_64X64 {
                    let frame = partition::Frame {
                        luma: &f.src.planes[0],
                        mi_rows: f.mi.mi_rows,
                        mi_cols: f.mi.mi_cols,
                    };
                    k.sb = Some(partition::choose_key_frame_partitioning(
                        &frame,
                        mi_row,
                        mi_col,
                        k.thresholds,
                    ));
                }
                walk_partition(k.sb.as_ref(), mi_row, mi_col, bsize)
            }
            Self::Inter(d) => {
                if bsize == BLOCK_64X64 {
                    d.superblock(f, mi_row, mi_col);
                }
                walk_partition(d.part.as_ref(), mi_row, mi_col, bsize)
            }
        }
    }

    fn modes(
        &mut self,
        f: &mut FrameEncoder<'_>,
        mi_row: usize,
        mi_col: usize,
        bsize: BlockSize,
    ) -> BlockModes {
        match self {
            Self::Key(k) => {
                let modes = f.pick_intra_mode(mi_row, mi_col, bsize, k.rdmult, &k.y_mode_costs);
                if let Some(m) = k.state.mi_at(mi_row, mi_col) {
                    m.sb_type = bsize;
                    m.mv = Mv::INVALID;
                }
                modes
            }
            Self::Inter(d) if d.settings.learned_partition => {
                d.searched_block(f, mi_row, mi_col, bsize)
            }
            Self::Inter(d) => d.block(f, mi_row, mi_col, bsize),
        }
    }

    /// The block's coding leaves its verdict on coding nothing in the
    /// buffer cell where it starts, as libvpx's `encode_superblock` writes
    /// `mi->skip`.
    fn encoded(&mut self, mi_row: usize, mi_col: usize, _bsize: BlockSize, skip: bool) {
        let state = match self {
            Self::Key(k) => &mut *k.state,
            Self::Inter(d) => &mut *d.state,
        };
        if let Some(m) = state.mi_at(mi_row, mi_col) {
            m.skip = skip;
        }
    }
}

impl InterDecisions<'_> {
    /// A superblock's start: `encode_nonrd_sb_row`'s setup, then
    /// `choose_partitioning`.
    fn superblock(&mut self, f: &mut FrameEncoder<'_>, mi_row: usize, mi_col: usize) {
        let (mi_rows, mi_cols) = (f.mi.mi_rows, f.mi.mi_cols);
        let st = &self.settings;
        if st.use_skin_detection {
            let src = [&f.src.planes[0], &f.src.planes[1], &f.src.planes[2]];
            content::skin_sb(
                src,
                self.consec_zero_mv,
                &mut self.state.skin_map,
                mi_rows,
                mi_cols,
                mi_row,
                mi_col,
            );
        }
        self.sb = SbState {
            last_sb_high_content: self.state.last_sb_high_content,
            ..SbState::default()
        };
        let sbi = self.state.sb_index(mi_row, mi_col);
        if st.use_source_sad {
            let (sad, state) = content::avg_source_sad(
                &f.src.planes[0],
                &self.last_src.planes[0],
                mi_col * 8,
                mi_row * 8,
            );
            self.sb.content_state = state;
            let fd = &mut self.state.content_state_sb_fd[sbi];
            *fd = if sad < 12_000 {
                fd.saturating_add(1)
            } else {
                0
            };
        }
        if self.settings.learned_partition {
            self.learned_superblock(f, mi_row, mi_col);
        } else {
            self.choose_partitioning(f, mi_row, mi_col);
        }
    }

    /// A superblock of the learned partitioning: libvpx's
    /// `get_estimated_pred`, then `nonrd_pick_partition` over the whole
    /// superblock, its choices shown to the blocks searched after them and
    /// kept for the coding.
    fn learned_superblock(&mut self, f: &mut FrameEncoder<'_>, mi_row: usize, mi_col: usize) {
        if self.learned.partition_cost == [[0; PARTITION_TYPES]; PARTITION_CONTEXTS] {
            // The frame's first superblock: vp9_initialize_rd_consts.
            for (costs, probs) in self
                .learned
                .partition_cost
                .iter_mut()
                .zip(&f.fc.partition_prob)
            {
                cost_tokens(costs, probs, &PARTITION_TREE);
            }
        }
        self.estimated_pred(f, mi_row, mi_col);
        let mark = f.shown_mark();
        let mut root = SearchNode::default();
        self.pick_partition(f, mi_row, mi_col, BLOCK_64X64, i64::MAX, &mut root);
        f.forget_shown(mark, mi_row, mi_col);
        // The blocks kept, in coding order, and the partition they make.
        let mut leaves = Vec::new();
        collect_leaves(
            &root,
            mi_row,
            mi_col,
            BLOCK_64X64,
            f.mi.mi_rows,
            f.mi.mi_cols,
            &mut leaves,
        );
        let mut part = partition::empty_superblock(mi_row, mi_col);
        for &(r, c, bs, _) in &leaves {
            part.set_size(r, c, bs);
        }
        self.part = Some(part);
        self.learned.leaves = leaves;
    }

    /// libvpx's `get_estimated_pred` for an inter frame at speed 8: the
    /// superblock's motion estimated by integral projections, and its luma
    /// predicted from the last frame there with the bilinear filter.
    fn estimated_pred(&mut self, f: &mut FrameEncoder<'_>, mi_row: usize, mi_col: usize) {
        let (mi_rows, mi_cols) = (f.mi.mi_rows, f.mi.mi_cols);
        // set_offsets: the frame's multiplier.
        self.learned.rdmult = self.search.rdmult;
        let bsize = BLOCK_32X32
            + if mi_col + 4 < mi_cols { 2 } else { 0 }
            + if mi_row + 4 < mi_rows { 1 } else { 0 };
        let (bw, bh) = (
            4usize << tables::B_WIDTH_LOG2[usize::from(bsize)],
            4usize << tables::B_HEIGHT_LOG2[usize::from(bsize)],
        );
        let (y_sad, mv) = match self.search.luma[0] {
            Some(last) => {
                let src = &f.src.planes[0];
                let search = Search {
                    src: src
                        .data
                        .get(mi_row * 8 * src.stride + mi_col * 8..)
                        .unwrap_or(&[]),
                    src_stride: src.stride,
                    pre: last,
                    x: (mi_col * 8) as i32,
                    y: (mi_row * 8) as i32,
                    w: bw,
                    h: bh,
                };
                let limits = MvLimits::for_block(mi_row, mi_col, 8, 8, mi_rows, mi_cols);
                mcomp::int_pro_motion_estimation(
                    &search,
                    u32::from(tables::B_WIDTH_LOG2[usize::from(bsize)]),
                    u32::from(tables::B_HEIGHT_LOG2[usize::from(bsize)]),
                    &limits,
                    Mv::ZERO,
                )
            }
            None => (u32::MAX, Mv::ZERO),
        };
        self.sb.sb_use_mv_part = true;
        self.sb.sb_mvcol_part = i32::from(mv.col);
        self.sb.sb_mvrow_part = i32::from(mv.row);
        self.sb.pred_mv[LAST_FRAME as usize] = Some(mv);
        if let Some(m) = self.state.mi_at(mi_row, mi_col) {
            m.sb_type = BLOCK_64X64;
            m.mv = mv;
        }
        #[cfg(test)]
        crate::enc::trace::line(|| {
            format!(
                "G {mi_row} {mi_col} cs={} ysad={y_sad} mv={},{}",
                self.sb.content_state as i32, mv.row, mv.col
            )
        });
        #[cfg(not(test))]
        let _ = y_sad;
        // The superblock predicted from the last frame: its luma into
        // est_pred, its chroma into the reconstruction as libvpx's leaves
        // it, the reconstruction's luma as it was.
        let mi = ModeInfo {
            sb_type: BLOCK_64X64,
            ref_frame: [LAST_FRAME, crate::common::NO_REF_FRAME],
            mv: [mv, Mv::ZERO],
            interp_filter: BILINEAR,
            ..ModeInfo::default()
        };
        let (x0, y0) = (mi_col * 8, mi_row * 8);
        let mut saved = vec![0u8; 64 * 64];
        for (row, out) in saved.chunks_exact_mut(64).enumerate() {
            let (line, _) = f.recon_at(0, x0, y0 + row);
            let n = line.len().min(64);
            out[..n].copy_from_slice(&line[..n]);
        }
        f.predict_inter(mi_row, mi_col, BLOCK_64X64, &mi, 0..3);
        for (row, (est, old)) in self
            .learned
            .est_pred
            .chunks_exact_mut(64)
            .zip(saved.chunks_exact(64))
            .enumerate()
        {
            let (line, _) = f.recon_at_mut(0, x0, y0 + row);
            let n = line.len().min(64);
            est[..n].copy_from_slice(&line[..n]);
            line[..n].copy_from_slice(&old[..n]);
        }
    }

    /// libvpx's `nonrd_pick_partition` at speed 8 for the square block
    /// `bsize` at (`mi_row`, `mi_col`): tried whole and cut in four, as the
    /// edges and the network allow, the cheaper kept and shown to the
    /// blocks searched after it -- or `None` where neither comes in under
    /// `best_rd`.
    ///
    /// At speed 8 the search is square only (`use_square_partition_only`),
    /// from 64x64 down to 8x8 (`x->max_partition_size`,
    /// `min_partition_size`), so a block is never tried in halves: where it
    /// crosses the picture's edge, only cut.
    fn pick_partition(
        &mut self,
        f: &mut FrameEncoder<'_>,
        mi_row: usize,
        mi_col: usize,
        bsize: BlockSize,
        best_rd: i64,
        node: &mut SearchNode,
    ) -> Option<SearchCost> {
        let (mi_rows, mi_cols) = (f.mi.mi_rows, f.mi.mi_cols);
        let ms = usize::from(tables::NUM_8X8_WIDE[usize::from(bsize)]) / 2;
        let mut whole_allowed = mi_row + ms < mi_rows && mi_col + ms < mi_cols;
        let mut do_split = bsize > BLOCK_8X8;
        if whole_allowed
            && do_split
            && let Some((verdict, score)) = mlpart::predict(
                &f.src.planes[0],
                &self.learned.est_pred,
                mi_row,
                mi_col,
                bsize,
                self.learned.dc_q,
            )
        {
            #[cfg(test)]
            crate::enc::trace::line(|| format!("Q {mi_row} {mi_col} bs={bsize} score={score}"));
            #[cfg(not(test))]
            let _ = score;
            match verdict {
                mlpart::Verdict::Whole => do_split = false,
                mlpart::Verdict::Split => whole_allowed = false,
                mlpart::Verdict::Both => {}
            }
        }
        let rd = |rdmult: i32, rate: i32, dist: i64| rdcost(rdmult, RDDIV_BITS, rate, dist);
        let mut best = SearchCost {
            rate: i32::MAX,
            dist: i64::MAX,
            rdcost: best_rd,
        };
        node.chosen = None;
        if whole_allowed {
            // ctx->pred_pixel_ready: a block that will not be searched in
            // quarters keeps its winner's prediction (see search_candidate).
            let candidate = self.search_candidate(f, mi_row, mi_col, bsize, !do_split);
            node.whole = Some(candidate);
            let p = candidate.picked;
            if p.rate != i32::MAX {
                let pl = f.partition_context(mi_row, mi_col, bsize);
                let rate = p.rate + self.learned.partition_cost[pl][usize::from(PARTITION_NONE)];
                let this = SearchCost {
                    rate,
                    dist: p.dist,
                    rdcost: rd(self.learned.rdmult, rate, p.dist),
                };
                if this.rdcost < best.rdcost {
                    best = this;
                    node.chosen = Some(PARTITION_NONE);
                }
            }
        }
        // store_pred_mv: what the quarters each start from.
        let pred_mv = self.sb.pred_mv;
        if do_split {
            let pl = f.partition_context(mi_row, mi_col, bsize);
            let rate = self.learned.partition_cost[pl][usize::from(PARTITION_SPLIT)];
            let mut sum = SearchCost {
                rate,
                dist: 0,
                rdcost: rd(self.learned.rdmult, rate, 0),
            };
            let subsize = tables::SUBSIZE[usize::from(PARTITION_SPLIT)][usize::from(bsize)];
            node.quarters = (0..4).map(|_| SearchNode::default()).collect();
            for (i, quarter) in node.quarters.iter_mut().enumerate() {
                if sum.rdcost >= best.rdcost {
                    break;
                }
                let (r, c) = (mi_row + (i >> 1) * ms, mi_col + (i & 1) * ms);
                if r >= mi_rows || c >= mi_cols {
                    continue;
                }
                // load_pred_mv.
                self.sb.pred_mv = pred_mv;
                match self.pick_partition(f, r, c, subsize, best.rdcost - sum.rdcost, quarter) {
                    None => {
                        sum = SearchCost {
                            rate: i32::MAX,
                            dist: i64::MAX,
                            rdcost: i64::MAX,
                        };
                    }
                    Some(this) => {
                        sum.rate += this.rate;
                        sum.dist += this.dist;
                        sum.rdcost += this.rdcost;
                    }
                }
            }
            if sum.rdcost < best.rdcost {
                best = sum;
                node.chosen = Some(PARTITION_SPLIT);
            }
        }
        if best.rate == i32::MAX {
            #[cfg(test)]
            crate::enc::trace::line(|| format!("X {mi_row} {mi_col} bs={bsize} none"));
            return None;
        }
        #[cfg(test)]
        crate::enc::trace::line(|| {
            format!(
                "X {mi_row} {mi_col} bs={bsize} part={} rate={} dist={} rd={}",
                node.chosen.unwrap_or(PARTITION_NONE),
                best.rate,
                best.dist,
                best.rdcost
            )
        });
        // fill_mode_info_sb: kept whole, the block shows its modes, and the
        // verdict on coding nothing its buffer cell held before -- the mode
        // search never writes one. Cut, its quarters showed themselves.
        if node.chosen == Some(PARTITION_NONE)
            && let Some(whole) = node.whole
        {
            let skip = self.state.mi_at(mi_row, mi_col).is_some_and(|m| m.skip);
            f.show(&whole.picked.modes, mi_row, mi_col, bsize, skip);
        }
        Some(best)
    }

    /// One block tried whole by the partition search: libvpx's
    /// `nonrd_pick_sb_modes` -- the block's segment as the map has it, its
    /// modes searched, and libvpx's registers left as the search leaves
    /// them.
    ///
    /// What the search leaves in the reconstruction matters here as it
    /// never does when each block is coded straight after its search: the
    /// blocks searched next predict intra from it. libvpx's mode search
    /// predicts into the reconstruction as this one does, and so leaves the
    /// last prediction it made -- except where it keeps its predictions
    /// aside to reuse them (`reuse_inter_pred`, on when `pred_pixel_ready`:
    /// the block will not be searched in quarters), and then copies its
    /// winner back if an inter mode won.
    fn search_candidate(
        &mut self,
        f: &mut FrameEncoder<'_>,
        mi_row: usize,
        mi_col: usize,
        bsize: BlockSize,
        pred_pixel_ready: bool,
    ) -> Candidate {
        let (mi_rows, mi_cols) = (f.mi.mi_rows, f.mi.mi_cols);
        let segment_id = if f.seg.enabled {
            get_segment_id(&self.cr.seg_map, bsize, mi_row, mi_col, mi_rows, mi_cols)
        } else {
            0
        };
        let tile = f.tile_index();
        let picked = pickinter::pick_inter_mode(
            f,
            &self.search,
            &mut self.sb,
            self.state.thresholds(tile, bsize),
            mi_row,
            mi_col,
            bsize,
            segment_id,
        );
        let modes = picked.modes;
        let mv = modes.inter.map_or(Mv::INVALID, |i| i.mv);
        #[cfg(test)]
        crate::enc::trace::line(|| {
            let (r, filt) = modes
                .inter
                .map_or((0, 3), |i| (i.ref_frame, i.interp_filter));
            format!(
                "B {mi_row} {mi_col} bs={bsize} seg={segment_id} m={} r={r} mv={},{} f={filt} tx={} sk={} skt={} rate={} dist={}",
                modes.mode,
                mv.row,
                mv.col,
                modes.tx_size,
                u8::from(modes.skip),
                picked.skip_txfm,
                picked.rate,
                picked.dist
            )
        });
        if pred_pixel_ready && let Some(inter) = modes.inter {
            let mi = ModeInfo {
                sb_type: bsize,
                ref_frame: [inter.ref_frame, crate::common::NO_REF_FRAME],
                mv: [inter.mv, Mv::ZERO],
                interp_filter: inter.interp_filter,
                ..ModeInfo::default()
            };
            f.predict_inter(mi_row, mi_col, bsize, &mi, 0..1);
        }
        if let Some(m) = self.state.mi_at(mi_row, mi_col) {
            m.sb_type = bsize;
            m.mv = mv;
        }
        self.learned.x_skip = modes.skip;
        self.learned.rdmult = if self.settings.cyclic_refresh && segment_boosted(segment_id) {
            self.search.cr_rdmult
        } else {
            self.search.rdmult
        };
        Candidate { picked }
    }

    /// A block of the learned partitioning, coded: what the search kept
    /// for it, its segment settled by the cyclic refresh as libvpx's
    /// `update_state_rt` settles it -- with `x->skip` as the block before it
    /// left it.
    fn searched_block(
        &mut self,
        f: &mut FrameEncoder<'_>,
        mi_row: usize,
        mi_col: usize,
        bsize: BlockSize,
    ) -> BlockModes {
        let (mi_rows, mi_cols) = (f.mi.mi_rows, f.mi.mi_cols);
        let Some(candidate) = self
            .learned
            .leaves
            .iter()
            .find(|l| l.0 == mi_row && l.1 == mi_col && l.2 == bsize)
            .map(|l| l.3)
        else {
            debug_assert!(
                false,
                "the coding asked for a block the search did not keep"
            );
            return self.block(f, mi_row, mi_col, bsize);
        };
        let picked = candidate.picked;
        let mut modes = picked.modes;
        let mv = modes.inter.map_or(Mv::INVALID, |i| i.mv);
        if f.seg.enabled && self.settings.cyclic_refresh {
            let use_skin = self.settings.use_skin_detection;
            let src = f.src;
            let mut is_skin = || {
                use_skin && {
                    let (y, u, v) = (&src.planes[0], &src.planes[1], &src.planes[2]);
                    let (x0, y0) = (mi_col * 8, mi_row * 8);
                    let bw = 4usize << tables::B_WIDTH_LOG2[usize::from(bsize)];
                    let bh = 4usize << tables::B_HEIGHT_LOG2[usize::from(bsize)];
                    content::skin_block(
                        y.data.get(y0 * y.stride + x0..).unwrap_or(&[]),
                        y.stride,
                        u.data
                            .get((y0 >> 1) * u.stride + (x0 >> 1)..)
                            .unwrap_or(&[]),
                        v.data
                            .get((y0 >> 1) * v.stride + (x0 >> 1)..)
                            .unwrap_or(&[]),
                        u.stride,
                        bw,
                        bh,
                        0,
                    )
                }
            };
            modes.segment_id = self.cr.update_segment(
                modes.segment_id,
                modes.inter.is_some(),
                mv,
                i64::from(picked.rate),
                picked.dist,
                self.learned.x_skip,
                bsize,
                mi_row,
                mi_col,
                mi_rows,
                mi_cols,
                &mut is_skin,
            );
            #[cfg(test)]
            crate::enc::trace::line(|| format!("U {mi_row} {mi_col} seg={}", modes.segment_id));
        }
        self.learned.x_skip = picked.modes.skip;
        if let Some(m) = self.state.mi_at(mi_row, mi_col) {
            m.sb_type = bsize;
            m.mv = mv;
        }
        modes
    }

    /// The copy of the last frame's partition, if libvpx copies here:
    /// `copy_partitioning`.
    fn copy_partitioning(&mut self, mi_row: usize, mi_col: usize, segment_id: u8) -> bool {
        let sbi = self.state.sb_index(mi_row, mi_col);
        let st = &self.settings;
        if !(st.frames_since_key > 1
            && segment_id == CR_SEGMENT_ID_BASE
            && self.state.prev_segment_id[sbi] == CR_SEGMENT_ID_BASE
            && self.state.copied_frame_cnt[sbi] < st.max_copied_frame)
        {
            return false;
        }
        let (mi_rows, mi_cols) = (self.state.mi_rows, self.state.mi_cols);
        let prev = &self.state.prev_partition;
        let (part, calls) = partition::copy_partition(mi_row, mi_col, mi_rows, mi_cols, &|r, c| {
            prev.get(r * mi_cols + c).copied().unwrap_or(BLOCK_4X4)
        });
        self.state.copied_frame_cnt[sbi] += 1;
        self.sb.variance_low = self.state.prev_variance_low[sbi];
        self.state.apply_set_calls(&calls);
        self.part = Some(part);
        true
    }

    /// Store this frame's partition for the next frame's copy:
    /// `update_prev_partition`.
    fn update_prev_partition(&mut self, mi_row: usize, mi_col: usize, segment_id: u8) {
        let (mi_rows, mi_cols) = (self.state.mi_rows, self.state.mi_cols);
        let sbi = self.state.sb_index(mi_row, mi_col);
        if let Some(part) = self.part {
            let prev = &mut self.state.prev_partition;
            partition::store_partition(
                mi_row,
                mi_col,
                mi_rows,
                mi_cols,
                BLOCK_64X64,
                &|r, c| part.size_at(r, c),
                &mut |r, c, bs| {
                    if let Some(p) = prev.get_mut(r * mi_cols + c) {
                        *p = bs;
                    }
                },
            );
        }
        self.state.prev_segment_id[sbi] = segment_id;
        self.state.prev_variance_low[sbi] = self.sb.variance_low;
        self.state.copied_frame_cnt[sbi] = 0;
    }

    /// Whether each chroma plane differs from its prediction enough to be
    /// weighed in the mode search: libvpx's `chroma_check`.
    fn chroma_check(
        &mut self,
        f: &FrameEncoder<'_>,
        mi_row: usize,
        mi_col: usize,
        bsize: BlockSize,
        y_sad: u32,
    ) {
        let uv_bsize = tables::SS_SIZE[usize::from(bsize)][1][1];
        let w = 4usize << tables::B_WIDTH_LOG2[usize::from(uv_bsize)];
        let h = 4usize << tables::B_HEIGHT_LOG2[usize::from(uv_bsize)];
        for i in 1..=2usize {
            let s = &f.src.planes[i];
            let (x, y) = (mi_col * 4, mi_row * 4);
            let (pred, pred_stride) = f.recon_at(i, x, y);
            let uv_sad = crate::enc::variance::sad(
                s.data.get(y * s.stride + x..).unwrap_or(&[]),
                s.stride,
                pred,
                pred_stride,
                w,
                h,
            );
            self.sb.color_sensitivity[i - 1] = uv_sad > (y_sad >> 2);
        }
    }

    /// libvpx's `choose_partitioning` for an inter frame at speed 8.
    #[allow(clippy::too_many_lines)]
    fn choose_partitioning(&mut self, f: &mut FrameEncoder<'_>, mi_row: usize, mi_col: usize) {
        let (mi_rows, mi_cols) = (f.mi.mi_rows, f.mi.mi_cols);
        let segment_id = if f.seg.enabled {
            get_segment_id(
                &self.cr.seg_map,
                BLOCK_64X64,
                mi_row,
                mi_col,
                mi_rows,
                mi_cols,
            )
        } else {
            0
        };
        let sbi = self.state.sb_index(mi_row, mi_col);
        self.sb.variance_low = [false; 25];
        let content_state = self.sb.content_state;
        let force_64_split = self.search.scene_change;
        if self.settings.use_source_sad {
            self.sb.skip_low_source_sad = matches!(
                content_state,
                ContentState::LowSadLowSumdiff | ContentState::LowSadHighSumdiff
            );
            self.sb.lowvar_highsumdiff = content_state == ContentState::LowVarHighSumdiff;
            self.state.last_sb_high_content = i32::from(self.state.content_state_sb_fd[sbi]);
            self.sb.last_sb_high_content = self.state.last_sb_high_content;
            if self.sb.skip_low_source_sad
                && self.settings.copy_partition
                && !force_64_split
                && self.copy_partitioning(mi_row, mi_col, segment_id)
            {
                self.sb.sb_use_mv_part = true;
                self.trace_sb(mi_row, mi_col, segment_id, u32::MAX, 'C');
                return;
            }
        }
        // The thresholds, a boosted segment's at its own quantiser.
        let q = if self.settings.cyclic_refresh && segment_boosted(segment_id) {
            f.seg.qindex(segment_id, self.settings.base_qindex)
        } else {
            self.settings.base_qindex
        };
        let y_ac = i32::from(f.quants.get(0, usize::try_from(q).unwrap_or(0)).dequant[1]);
        let thresholds = partition::inter_thresholds(
            y_ac,
            1,
            self.settings.noise_extracted,
            content_state,
            self.search.frame_width,
            self.search.frame_height,
            8,
            false,
            self.settings.avg_inter_qindex,
            self.settings.vbp_threshold_8x8,
        );
        // The superblock's sum of differences against the last frame, at
        // zero motion -- or, where it changed a lot, at the integral
        // projections' estimate, which its blocks may then reuse.
        let bsize = BLOCK_32X32
            + if mi_col + 4 < mi_cols { 2 } else { 0 }
            + if mi_row + 4 < mi_rows { 1 } else { 0 };
        let (bw, bh) = (
            4usize << tables::B_WIDTH_LOG2[usize::from(bsize)],
            4usize << tables::B_HEIGHT_LOG2[usize::from(bsize)],
        );
        let low_res = self.search.frame_width <= 352 && self.search.frame_height <= 288;
        let mut sb_mv = Mv::ZERO;
        let y_sad = match self.search.luma[0] {
            Some(last) => {
                let src = &f.src.planes[0];
                let search = Search {
                    src: src
                        .data
                        .get(mi_row * 8 * src.stride + mi_col * 8..)
                        .unwrap_or(&[]),
                    src_stride: src.stride,
                    pre: last,
                    x: (mi_col * 8) as i32,
                    y: (mi_row * 8) as i32,
                    w: bw,
                    h: bh,
                };
                if !low_res && content_state != ContentState::VeryHighSad {
                    search.sad(0, 0)
                } else {
                    let limits = MvLimits::for_block(mi_row, mi_col, 8, 8, mi_rows, mi_cols);
                    let (sad, mv) = mcomp::int_pro_motion_estimation(
                        &search,
                        u32::from(tables::B_WIDTH_LOG2[usize::from(bsize)]),
                        u32::from(tables::B_HEIGHT_LOG2[usize::from(bsize)]),
                        &limits,
                        Mv::ZERO,
                    );
                    self.sb.sb_use_mv_part = true;
                    self.sb.sb_mvcol_part = i32::from(mv.col);
                    self.sb.sb_mvrow_part = i32::from(mv.row);
                    sb_mv = mv;
                    sad
                }
            }
            None => u32::MAX,
        };
        let y_sad_last = y_sad;
        self.sb.pred_mv[1] = Some(sb_mv);
        if let Some(m) = self.state.mi_at(mi_row, mi_col) {
            m.sb_type = BLOCK_64X64;
            m.mv = sb_mv;
        }
        // The superblock predicted from the last frame, every plane.
        let mi = ModeInfo {
            sb_type: BLOCK_64X64,
            ref_frame: [LAST_FRAME, crate::common::NO_REF_FRAME],
            mv: [sb_mv, Mv::ZERO],
            interp_filter: BILINEAR,
            ..ModeInfo::default()
        };
        f.predict_inter(mi_row, mi_col, BLOCK_64X64, &mi, 0..3);
        let mut force_split_64 = force_64_split;
        if self.settings.use_skin_detection {
            self.sb.sb_is_skin = content::skin_sb_split(
                &self.state.skin_map,
                mi_rows,
                mi_cols,
                mi_row,
                mi_col,
                low_res,
            );
            force_split_64 |= self.sb.sb_is_skin;
        }
        // A superblock that barely changed stays whole.
        if segment_id == CR_SEGMENT_ID_BASE
            && i64::from(y_sad) < self.settings.vbp_threshold_sad
            && mi_col + 4 < mi_cols
            && mi_row + 4 < mi_rows
        {
            self.part = Some(partition::whole_superblock(mi_row, mi_col));
            self.state.apply_set_calls(&[(mi_row, mi_col, BLOCK_64X64)]);
            self.sb.variance_low[0] = true;
            self.chroma_check(f, mi_row, mi_col, bsize, y_sad);
            if self.settings.copy_partition {
                self.update_prev_partition(mi_row, mi_col, segment_id);
            }
            self.trace_sb(mi_row, mi_col, segment_id, y_sad, 'E');
            return;
        }
        // One that changed little takes the last frame's partition.
        if self.settings.copy_partition
            && i64::from(y_sad_last) < self.settings.vbp_threshold_copy
            && self.copy_partitioning(mi_row, mi_col, segment_id)
        {
            self.chroma_check(f, mi_row, mi_col, bsize, y_sad);
            self.trace_sb(mi_row, mi_col, segment_id, y_sad, 'P');
            return;
        }
        let (pred, pred_x0) = f.recon_plane(0);
        let ip = partition::choose_inter_partitioning(&InterSb {
            src: &f.src.planes[0],
            pred,
            pred_x0,
            mi_rows,
            mi_cols,
            mi_row,
            mi_col,
            thresholds,
            force_split_64,
            noise_level: self.settings.noise_extracted.unwrap_or(NoiseLevel::Low),
        });
        let last = self
            .state
            .apply_set_calls(&ip.set_calls)
            .unwrap_or((mi_row, mi_col));
        self.part = Some(ip.part);
        if self.settings.copy_partition {
            self.update_prev_partition(mi_row, mi_col, segment_id);
        }
        if self.search.short_circuit_low_temp_var != 0 {
            let stale = self
                .state
                .mi_at(last.0, last.1)
                .copied()
                .unwrap_or_default();
            #[cfg(test)]
            crate::enc::trace::line(|| {
                format!(
                    "L {mi_row} {mi_col} mv={},{} sbt={} rfp=1",
                    stale.mv.row, stale.mv.col, stale.sb_type
                )
            });
            self.sb.variance_low = partition::low_temp_var_flags(
                &ip,
                thresholds,
                (stale.sb_type, stale.mv),
                self.search.short_circuit_low_temp_var,
                self.search.frame_width > 640,
                mi_rows,
                mi_cols,
            );
        }
        self.chroma_check(f, mi_row, mi_col, bsize, y_sad);
        self.trace_sb(mi_row, mi_col, segment_id, y_sad, 'V');
    }

    /// The trace line libvpx's instrumented build writes per superblock.
    #[cfg_attr(not(test), allow(unused_variables, clippy::unused_self))]
    fn trace_sb(&self, mi_row: usize, mi_col: usize, segment_id: u8, y_sad: u32, path: char) {
        #[cfg(test)]
        crate::enc::trace::line(|| {
            let sb = &self.sb;
            let vl = sb
                .variance_low
                .iter()
                .enumerate()
                .fold(0u32, |a, (i, &v)| a | (u32::from(v) << i));
            let pmv = sb.pred_mv[1].unwrap_or(Mv {
                row: i16::MAX,
                col: i16::MAX,
            });
            format!(
                "S {mi_row} {mi_col} seg={segment_id} cs={} lshc={} ysad={y_sad} p={path} vl={vl:x} col={}{} skin={} smp={},{},{} pmv={},{}",
                sb.content_state as i32,
                sb.last_sb_high_content,
                u8::from(sb.color_sensitivity[0]),
                u8::from(sb.color_sensitivity[1]),
                u8::from(sb.sb_is_skin),
                u8::from(sb.sb_use_mv_part),
                sb.sb_mvrow_part,
                sb.sb_mvcol_part,
                pmv.row,
                pmv.col
            )
        });
    }

    /// A block's decisions: `nonrd_pick_sb_modes` and `update_state_rt`'s
    /// segment.
    fn block(
        &mut self,
        f: &mut FrameEncoder<'_>,
        mi_row: usize,
        mi_col: usize,
        bsize: BlockSize,
    ) -> BlockModes {
        let (mi_rows, mi_cols) = (f.mi.mi_rows, f.mi.mi_cols);
        let segment_id = if f.seg.enabled {
            get_segment_id(&self.cr.seg_map, bsize, mi_row, mi_col, mi_rows, mi_cols)
        } else {
            0
        };
        let tile = f.tile_index();
        let picked = pickinter::pick_inter_mode(
            f,
            &self.search,
            &mut self.sb,
            self.state.thresholds(tile, bsize),
            mi_row,
            mi_col,
            bsize,
            segment_id,
        );
        let mut modes = picked.modes;
        let mv = modes.inter.map_or(Mv::INVALID, |i| i.mv);
        #[cfg(test)]
        crate::enc::trace::line(|| {
            let (r, filt) = modes
                .inter
                .map_or((0, 3), |i| (i.ref_frame, i.interp_filter));
            format!(
                "B {mi_row} {mi_col} bs={bsize} seg={segment_id} m={} r={r} mv={},{} f={filt} tx={} sk={} skt={} rate={} dist={}",
                modes.mode,
                mv.row,
                mv.col,
                modes.tx_size,
                u8::from(modes.skip),
                picked.skip_txfm,
                picked.rate,
                picked.dist
            )
        });
        if f.seg.enabled && self.settings.cyclic_refresh {
            let use_skin = self.settings.use_skin_detection;
            let src = f.src;
            let mut is_skin = || {
                use_skin && {
                    let (y, u, v) = (&src.planes[0], &src.planes[1], &src.planes[2]);
                    let (x0, y0) = (mi_col * 8, mi_row * 8);
                    let bw = 4usize << tables::B_WIDTH_LOG2[usize::from(bsize)];
                    let bh = 4usize << tables::B_HEIGHT_LOG2[usize::from(bsize)];
                    content::skin_block(
                        y.data.get(y0 * y.stride + x0..).unwrap_or(&[]),
                        y.stride,
                        u.data
                            .get((y0 >> 1) * u.stride + (x0 >> 1)..)
                            .unwrap_or(&[]),
                        v.data
                            .get((y0 >> 1) * v.stride + (x0 >> 1)..)
                            .unwrap_or(&[]),
                        u.stride,
                        bw,
                        bh,
                        0,
                    )
                }
            };
            modes.segment_id = self.cr.update_segment(
                segment_id,
                modes.inter.is_some(),
                mv,
                i64::from(picked.rate),
                picked.dist,
                modes.skip,
                bsize,
                mi_row,
                mi_col,
                mi_rows,
                mi_cols,
                &mut is_skin,
            );
            #[cfg(test)]
            crate::enc::trace::line(|| format!("U {mi_row} {mi_col} seg={}", modes.segment_id));
        }
        if let Some(m) = self.state.mi_at(mi_row, mi_col) {
            m.sb_type = bsize;
            m.mv = mv;
        }
        modes
    }
}
