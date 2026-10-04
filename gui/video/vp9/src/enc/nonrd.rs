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
//! What carries from frame to frame -- the adaptive mode thresholds, the
//! partitions to copy, the skin map, how long each superblock has been
//! still, and the contents of libvpx's mode-information buffers, which one
//! of its decisions reads stale -- is [`RtState`].
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
    LAST_FRAME, Mv, PARTITION_NONE, Partition,
};
use crate::enc::aq_cyclicrefresh::{CR_SEGMENT_ID_BASE, CyclicRefresh, segment_boosted};
use crate::enc::content;
use crate::enc::encodeframe::{BlockModes, Decide, FrameEncoder};
use crate::enc::mcomp::{self, MvLimits, Search};
use crate::enc::partition::{self, ContentState, InterSb, NoiseLevel, SbPartition};
use crate::enc::pickinter::{self, SbState, SearchFrame};
use crate::enc::rd::{MAX_MODES, RD_THRESH_INIT_FACT, kf_y_mode_costs};
use crate::frame::FrameBuf;
use crate::tables;

/// What one 8x8 cell of libvpx's mode-information buffer last held, as far
/// as any decision reads it: the block size `set_block_size` wrote there
/// and the vector a block left.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct StaleMi {
    pub sb_type: BlockSize,
    pub mv: Mv,
}

/// What libvpx's realtime decisions keep from frame to frame.
#[derive(Clone, Debug)]
pub(crate) struct RtState {
    mi_rows: usize,
    mi_cols: usize,
    sb_cols: usize,
    /// The tile's adaptive mode thresholds, by block size: libvpx's
    /// `tile_data->thresh_freq_fact` (one tile).
    pub thresh_freq_fact: Box<[[i32; MAX_MODES]; BLOCK_SIZES]>,
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
            thresh_freq_fact: Box::new([[RD_THRESH_INIT_FACT; MAX_MODES]; BLOCK_SIZES]),
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
    /// The cyclic refresh is setting segments this frame.
    pub cyclic_refresh: bool,
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
                    let (sad, _, _) = content::avg_source_sad(
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
                    *m = StaleMi {
                        sb_type: bsize,
                        mv: Mv::INVALID,
                    };
                }
                modes
            }
            Self::Inter(d) => d.block(f, mi_row, mi_col, bsize),
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
            let (sad, state, zero) = content::avg_source_sad(
                &f.src.planes[0],
                &self.last_src.planes[0],
                mi_col * 8,
                mi_row * 8,
            );
            self.sb.content_state = state;
            self.sb.zero_temp_sad_source = zero;
            let fd = &mut self.state.content_state_sb_fd[sbi];
            *fd = if sad < 12_000 {
                fd.saturating_add(1)
            } else {
                0
            };
        }
        self.choose_partitioning(f, mi_row, mi_col);
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
            let p = &f.recon.planes[i];
            let (x, y) = (mi_col * 4, mi_row * 4);
            let uv_sad = crate::enc::variance::sad(
                s.data.get(y * s.stride + x..).unwrap_or(&[]),
                s.stride,
                p.data.get(y * p.stride + x..).unwrap_or(&[]),
                p.stride,
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
            *m = StaleMi {
                sb_type: BLOCK_64X64,
                mv: sb_mv,
            };
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
        let ip = partition::choose_inter_partitioning(&InterSb {
            src: &f.src.planes[0],
            pred: &f.recon.planes[0],
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
        let picked = pickinter::pick_inter_mode(
            f,
            &self.search,
            &mut self.sb,
            &mut self.state.thresh_freq_fact[usize::from(bsize)],
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
            *m = StaleMi { sb_type: bsize, mv };
        }
        modes
    }
}
