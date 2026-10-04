//! Cyclic refresh: libvpx's realtime adaptive quantisation (`--aq-mode=3`).
//!
//! A video call or a screen recording is mostly still, and a still block
//! coded at a coarse quantiser stays coarse forever: nothing ever recodes
//! it. Cyclic refresh walks a band of superblocks across the frame, a few
//! percent at a time, and codes the still blocks in the band at a finer
//! quantiser -- through segmentation, VP9's per-block quantiser offsets --
//! so the picture sharpens over a cycle of frames while the bitrate barely
//! moves. The rate control knows: its model of a frame's size weighs the
//! refreshed blocks at their own quantiser.
//!
//! Before an inter frame the band is chosen (`setup`, `update_map`): the
//! superblocks whose blocks are due -- marked clean long enough ago, last
//! coded at a coarse quantiser or moving -- go to segment 1. As each block
//! is decided its segment is settled (`update_segment`): kept in segment 1,
//! or raised to segment 2 for a large still block, or dropped to segment 0
//! where the block moves a lot, is intra, or is skipped. After the frame,
//! what was refreshed is counted for the rate control, and whether the
//! frame was still enough to become the golden frame is checked.
//!
//! Key frames refresh nothing.
//!
//! Translated into Rust from libvpx v1.17.0's
//! `vp9/encoder/vp9_aq_cyclicrefresh.c` and `vp9_aq_cyclicrefresh.h`
//! (copyright the WebM project authors), used under libvpx's BSD licence
//! and patent grant (`licenses/libvpx-LICENSE`, `licenses/libvpx-PATENTS`).

#![allow(
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    reason = "libvpx's arithmetic: block counts bounded by the frame's 8x8 cells, quantiser indices in 0..=255, doubles truncated where libvpx truncates them"
)]

use crate::block::MiGrid;
use crate::common::{BLOCK_16X16, BlockSize, Mv, SEG_LVL_ALT_Q};
use crate::enc::cpi::Cpi;
use crate::enc::partition::NoiseLevel;
use crate::enc::ratectrl::{INTER_FRAME, bits_per_mb, estimate_bits_at_q, qindex_to_q};
use crate::header::MAXQ;
use crate::tables;

/// The cyclic refresh's segments: libvpx's `CR_SEGMENT_ID_BASE`,
/// `CR_SEGMENT_ID_BOOST1` and `CR_SEGMENT_ID_BOOST2`.
pub(crate) const CR_SEGMENT_ID_BASE: u8 = 0;
pub(crate) const CR_SEGMENT_ID_BOOST1: u8 = 1;
pub(crate) const CR_SEGMENT_ID_BOOST2: u8 = 2;
/// libvpx's `CR_MAX_RATE_TARGET_RATIO`.
const CR_MAX_RATE_TARGET_RATIO: f64 = 4.0;

/// Whether a segment is one of the refresh's boosted ones: libvpx's
/// `cyclic_refresh_segment_id_boosted`.
pub(crate) fn segment_boosted(segment_id: u8) -> bool {
    segment_id == CR_SEGMENT_ID_BOOST1 || segment_id == CR_SEGMENT_ID_BOOST2
}

/// libvpx's `CYCLIC_REFRESH`: the refresh's parameters and its maps.
#[derive(Clone, Debug)]
pub(crate) struct CyclicRefresh {
    /// The share of blocks refreshed per frame, in percent.
    pub percent_refresh: i32,
    /// The largest quantiser drop, as a percentage of the frame's.
    pub max_qdelta_perc: i32,
    /// Where the band starts: the superblock index the next frame's search
    /// for blocks due begins at.
    pub sb_index: usize,
    /// How long a refreshed block waits beyond the cycle before it may be
    /// refreshed again.
    pub time_for_refresh: i32,
    pub target_num_seg_blocks: i32,
    pub actual_num_seg1_blocks: i32,
    pub actual_num_seg2_blocks: i32,
    /// The rate-distortion multiplier of segment 1's quantiser.
    pub rdmult: i32,
    /// Per 8x8 cell: 1 not a candidate, 0 a candidate to refresh, negative
    /// refreshed that many frames ago (counting up to 0).
    pub map: Vec<i8>,
    /// Per 8x8 cell, the quantiser it was last coded at.
    pub last_coded_q_map: Vec<u8>,
    /// Per 8x8 cell, this frame's segment: libvpx's `cpi->segmentation_map`.
    pub seg_map: Vec<u8>,
    /// A block coding more than this rate (times 2^10), or more than this
    /// distortion while moving, is not refreshed.
    pub thresh_rate_sb: i64,
    pub thresh_dist_sb: i64,
    /// Motion beyond this many eighth pixels is "moving".
    pub motion_thresh: i32,
    pub rate_ratio_qdelta: f64,
    /// Segment 2's boost over segment 1's rate ratio, in tenths.
    pub rate_boost_fac: i32,
    /// A running average of the share of low-motion blocks.
    pub low_content_avg: f64,
    pub qindex_delta: [i32; 3],
    pub reduce_refresh: bool,
    pub weight_segment: f64,
    pub apply_cyclic_refresh: bool,
    pub counter_encode_maxq_scene_change: i32,
    pub skip_flat_static_blocks: bool,
    pub content_mode: bool,
}

impl CyclicRefresh {
    /// libvpx's `vp9_cyclic_refresh_alloc` for a frame of `cells` 8x8
    /// cells.
    pub(crate) fn new(cells: usize) -> Self {
        Self {
            percent_refresh: 0,
            max_qdelta_perc: 0,
            sb_index: 0,
            time_for_refresh: 0,
            target_num_seg_blocks: 0,
            actual_num_seg1_blocks: 0,
            actual_num_seg2_blocks: 0,
            rdmult: 0,
            map: vec![0; cells],
            last_coded_q_map: vec![MAXQ as u8; cells],
            seg_map: vec![0; cells],
            thresh_rate_sb: 0,
            thresh_dist_sb: 0,
            motion_thresh: 0,
            rate_ratio_qdelta: 0.0,
            rate_boost_fac: 0,
            low_content_avg: 0.0,
            qindex_delta: [0; 3],
            reduce_refresh: false,
            weight_segment: 0.0,
            apply_cyclic_refresh: false,
            counter_encode_maxq_scene_change: 0,
            skip_flat_static_blocks: false,
            content_mode: true,
        }
    }

    /// The segment of a block decided at (`mi_row`, `mi_col`): libvpx's
    /// `vp9_cyclic_refresh_update_segment`. `segment_id` is the block's
    /// segment as the band set it; `inter`, `mv`, `rate` and `dist` describe
    /// the mode the search chose, `skip` whether it codes nothing, and
    /// `is_skin` (asked only when the block is not otherwise refreshed, and
    /// is 16x16 or smaller) whether it is skin. Updates the refresh map and
    /// the frame's segment map for the block's cells, and returns its
    /// segment.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn update_segment(
        &mut self,
        segment_id: u8,
        inter: bool,
        mv: Mv,
        rate: i64,
        dist: i64,
        skip: bool,
        bsize: BlockSize,
        mi_row: usize,
        mi_col: usize,
        mi_rows: usize,
        mi_cols: usize,
        is_skin: &mut dyn FnMut() -> bool,
    ) -> u8 {
        let n = |t: &[u8; 13]| usize::from(t.get(usize::from(bsize)).copied().unwrap_or(1));
        let (bw, bh) = (n(&tables::NUM_8X8_WIDE), n(&tables::NUM_8X8_HIGH));
        let xmis = (mi_cols - mi_col).min(bw);
        let ymis = (mi_rows - mi_row).min(bh);
        let block_index = mi_row * mi_cols + mi_col;
        // candidate_refresh_aq.
        let (r, c) = (i32::from(mv.row), i32::from(mv.col));
        let t = self.motion_thresh;
        let mut refresh =
            if dist > self.thresh_dist_sb && (r > t || r < -t || c > t || c < -t || !inter) {
                CR_SEGMENT_ID_BASE
            } else if bsize >= BLOCK_16X16
                && rate < self.thresh_rate_sb
                && inter
                && mv == Mv::ZERO
                && self.rate_boost_fac > 10
            {
                CR_SEGMENT_ID_BOOST2
            } else {
                CR_SEGMENT_ID_BOOST1
            };
        if refresh == CR_SEGMENT_ID_BASE && bsize <= BLOCK_16X16 && is_skin() {
            refresh = CR_SEGMENT_ID_BOOST1;
        }
        let mut segment_id = segment_id;
        if segment_boosted(segment_id) {
            segment_id = if skip { CR_SEGMENT_ID_BASE } else { refresh };
        }
        let old = self.map.get(block_index).copied().unwrap_or(0);
        let new_map_value = if segment_boosted(segment_id) {
            -(self.time_for_refresh as i8)
        } else if refresh != CR_SEGMENT_ID_BASE {
            if old == 1 { 0 } else { old }
        } else {
            1
        };
        for y in 0..ymis {
            let start = block_index + y * mi_cols;
            if let Some(cells) = self.map.get_mut(start..start + xmis) {
                cells.fill(new_map_value);
            }
            if let Some(cells) = self.seg_map.get_mut(start..start + xmis) {
                cells.fill(segment_id);
            }
        }
        segment_id
    }

    /// After a frame: the quantiser each coded block's cells were last coded
    /// at, libvpx's `vp9_cyclic_refresh_update_sb_postencode` for every
    /// block of the frame.
    pub(crate) fn update_sb_postencode(&mut self, mi: &MiGrid, base_qindex: i32) {
        let cols = mi.mi_cols;
        for b in &mi.blocks {
            if b.segment_id > CR_SEGMENT_ID_BOOST2 {
                continue;
            }
            let delta = self
                .qindex_delta
                .get(usize::from(b.segment_id))
                .copied()
                .unwrap_or(0);
            let q = (base_qindex + delta).clamp(0, MAXQ) as u8;
            let xmis = b.bw.min(cols - b.mi_col);
            let ymis = b.bh.min(mi.mi_rows - b.mi_row);
            for y in 0..ymis {
                let start = (b.mi_row + y) * cols + b.mi_col;
                if let Some(cells) = self.last_coded_q_map.get_mut(start..start + xmis) {
                    for cell in cells {
                        *cell = if !b.is_inter() || !b.skip {
                            q
                        } else {
                            q.min(*cell)
                        };
                    }
                }
            }
        }
    }
}

impl Cpi {
    /// The golden frame's update interval: a multiple of the refresh
    /// period, within a limit. libvpx's
    /// `vp9_cyclic_refresh_set_golden_update`, CBR.
    pub(crate) fn cyclic_refresh_set_golden_update(&mut self) {
        let cr = &self.cr;
        let rc = &mut self.rc;
        rc.baseline_gf_interval = if cr.percent_refresh > 0 {
            (4 * (100 / cr.percent_refresh)).min(40)
        } else {
            40
        };
        if rc.avg_frame_low_motion < 50 && rc.frames_since_key > 40 && cr.content_mode {
            rc.baseline_gf_interval = 10;
        }
    }

    /// The refresh's parameters for this frame, set before its quantiser is
    /// chosen: libvpx's `vp9_cyclic_refresh_update_parameters`, CBR, one
    /// spatial and temporal layer. The noise estimate is the last frame's.
    pub(crate) fn cyclic_refresh_update_parameters(&mut self) {
        let rc = &self.rc;
        let cm = &self.common;
        let noisy = self.noise.enabled && self.noise.level >= NoiseLevel::Medium;
        let cr = &mut self.cr;
        let num8x8bl = cm.mbs << 2;
        let thresh_low_motion = 20;
        let qp_thresh = (if self.oxcf.screen_content { 35 } else { 20 }).min(rc.best_quality << 1);
        let qp_max_thresh = (117 * MAXQ) >> 7;
        cr.apply_cyclic_refresh = true;
        if cm.frame_is_intra_only()
            || self.oxcf.lossless()
            || rc.avg_frame_qindex[INTER_FRAME] < qp_thresh
            || (cr.content_mode
                && rc.avg_frame_low_motion < thresh_low_motion
                && rc.frames_since_key > 40)
            || (rc.avg_frame_qindex[INTER_FRAME] > qp_max_thresh && rc.frames_since_key > 20)
        {
            cr.apply_cyclic_refresh = false;
            return;
        }
        cr.percent_refresh = if cr.reduce_refresh { 5 } else { 10 };
        cr.max_qdelta_perc = 60;
        cr.time_for_refresh = 0;
        cr.motion_thresh = 32;
        cr.rate_boost_fac = 15;
        // A larger quantiser drop for the first few refresh cycles after a
        // key frame.
        if cr.percent_refresh > 0 && rc.frames_since_key < 4 * (100 / cr.percent_refresh) {
            cr.rate_ratio_qdelta = 3.0;
        } else {
            cr.rate_ratio_qdelta = 2.0;
            if cr.content_mode && noisy {
                // A smaller drop for a noisy source.
                cr.rate_ratio_qdelta = 1.7;
                cr.rate_boost_fac = 13;
            }
        }
        if self.oxcf.screen_content {
            cr.skip_flat_static_blocks = true;
            cr.percent_refresh = if cr.skip_flat_static_blocks { 5 } else { 10 };
            if cr.content_mode && cr.counter_encode_maxq_scene_change < 30 {
                cr.percent_refresh = if cr.skip_flat_static_blocks { 10 } else { 15 };
            }
            cr.rate_ratio_qdelta = 2.0;
            cr.rate_boost_fac = 10;
        }
        // Small pictures.
        if u64::from(cm.width) * u64::from(cm.height) <= 352 * 288 {
            if rc.avg_frame_bandwidth < 3000 {
                cr.motion_thresh = 64;
                cr.rate_boost_fac = 13;
            } else {
                cr.max_qdelta_perc = 70;
                cr.rate_ratio_qdelta = cr.rate_ratio_qdelta.max(2.5);
            }
        }
        // The weight of the refreshed segment in the frame's rate: the
        // average of this frame's target and the last frame's actual, or the
        // target if that is smaller.
        let cells = i32::try_from(cm.mi_rows * cm.mi_cols).unwrap_or(i32::MAX);
        let target_refresh = cr.percent_refresh * cells / 100;
        let weight_segment_target = f64::from(target_refresh) / f64::from(num8x8bl);
        let mut weight_segment = f64::from(
            (target_refresh + cr.actual_num_seg1_blocks + cr.actual_num_seg2_blocks) >> 1,
        ) / f64::from(num8x8bl);
        if weight_segment_target < 7.0 * weight_segment / 8.0 {
            weight_segment = weight_segment_target;
        }
        if self.oxcf.screen_content {
            weight_segment = f64::from(cr.actual_num_seg1_blocks + cr.actual_num_seg2_blocks)
                / f64::from(num8x8bl);
        }
        cr.weight_segment = weight_segment;
        if !cr.content_mode {
            cr.actual_num_seg1_blocks = cr.percent_refresh * cells / 100;
            cr.actual_num_seg2_blocks = 0;
            cr.weight_segment = f64::from(cr.actual_num_seg1_blocks) / f64::from(num8x8bl);
        }
    }

    /// The quantiser change for a segment, `rate_factor` times the rate,
    /// within the largest drop: libvpx's `compute_deltaq`.
    fn cyclic_refresh_compute_deltaq(&self, q: i32, rate_factor: f64) -> i32 {
        let cr = &self.cr;
        let deltaq = self.compute_qdelta_by_rate(self.common.key_frame, q, rate_factor);
        if -deltaq > cr.max_qdelta_perc * q / 100 {
            -cr.max_qdelta_perc * q / 100
        } else {
            deltaq
        }
    }

    /// After a frame: its size as the model predicts it, the refreshed
    /// blocks at their own quantisers. libvpx's
    /// `vp9_cyclic_refresh_estimate_bits_at_q`.
    pub(crate) fn cyclic_refresh_estimate_bits_at_q(&self, correction_factor: f64) -> i32 {
        let cm = &self.common;
        let cr = &self.cr;
        let mbs = cm.mbs;
        let num8x8bl = f64::from(mbs << 2);
        let weight_segment1 = f64::from(cr.actual_num_seg1_blocks) / num8x8bl;
        let weight_segment2 = f64::from(cr.actual_num_seg2_blocks) / num8x8bl;
        let key = cm.key_frame;
        let q = cm.quant.base_qindex;
        let bits = |q: i32| f64::from(estimate_bits_at_q(key, q, mbs, correction_factor));
        ((1.0 - weight_segment1 - weight_segment2) * bits(q)
            + weight_segment1 * bits(q + cr.qindex_delta[1])
            + weight_segment2 * bits(q + cr.qindex_delta[2]))
        .round() as i32
    }

    /// Before a frame: the bits a macroblock costs at index `i`, the refresh
    /// band's share at its lower quantiser. libvpx's
    /// `vp9_cyclic_refresh_rc_bits_per_mb`.
    pub(crate) fn cyclic_refresh_rc_bits_per_mb(&self, i: i32, correction_factor: f64) -> i32 {
        let cr = &self.cr;
        let key = self.common.key_frame;
        let deltaq = if self.oxcf.speed < 8 {
            self.cyclic_refresh_compute_deltaq(i, cr.rate_ratio_qdelta)
        } else {
            -(cr.max_qdelta_perc * i) / 200
        };
        ((1.0 - cr.weight_segment) * f64::from(bits_per_mb(key, i, correction_factor))
            + cr.weight_segment * f64::from(bits_per_mb(key, i + deltaq, correction_factor)))
        .round() as i32
    }

    /// For screen content: hold the frame's quantiser to at most 8 below
    /// the last frame's while the refresh runs. libvpx's
    /// `vp9_cyclic_refresh_limit_q`.
    pub(crate) fn cyclic_refresh_limit_q(&self, q: i32) -> i32 {
        if self.cr.percent_refresh > 0 && self.rc.q_1_frame - q > 8 {
            self.rc.q_1_frame - 8
        } else {
            q
        }
    }

    /// Set up the refresh for the frame: libvpx's
    /// `vp9_cyclic_refresh_setup`. Where the refresh is off (a key frame, a
    /// scene change, quantisers out of its range) segmentation is disabled
    /// and the map cleared; else the two boosted segments' quantisers are
    /// set and the band chosen (`update_map`). `consec_zero_mv` counts each
    /// cell's frames of zero motion.
    pub(crate) fn cyclic_refresh_setup(
        &mut self,
        consec_zero_mv: &[u8],
        rd_frame: crate::enc::rd::RdFrame,
    ) {
        let scene_change_detected = self.rc.high_source_sad;
        if self.common.current_video_frame == 0 {
            self.cr.low_content_avg = 0.0;
        }
        if !self.cr.apply_cyclic_refresh || scene_change_detected {
            self.cr.seg_map.fill(0);
            let seg = &mut self.common.seg;
            seg.enabled = false;
            seg.update_map = false;
            seg.update_data = false;
            if self.common.key_frame || scene_change_detected {
                self.cr.last_coded_q_map.fill(MAXQ as u8);
                self.cr.sb_index = 0;
                self.cr.reduce_refresh = false;
                self.cr.counter_encode_maxq_scene_change = 0;
            }
            return;
        }
        let base_qindex = self.common.quant.base_qindex;
        let q = qindex_to_q(base_qindex);
        self.cr.counter_encode_maxq_scene_change += 1;
        self.cr.thresh_rate_sb = (i64::from(self.rc.sb64_target_rate) << 8) << 2;
        self.cr.thresh_dist_sb = ((q * q) as i64) << 2;
        // vp9_enable_segmentation, vp9_clearall_segfeatures, delta coding.
        let qindex_delta1 =
            self.cyclic_refresh_compute_deltaq(base_qindex, self.cr.rate_ratio_qdelta);
        let ratio2 = CR_MAX_RATE_TARGET_RATIO
            .min(0.1 * f64::from(self.cr.rate_boost_fac) * self.cr.rate_ratio_qdelta);
        let qindex_delta2 = self.cyclic_refresh_compute_deltaq(base_qindex, ratio2);
        let seg = &mut self.common.seg;
        seg.enabled = true;
        seg.update_map = true;
        seg.update_data = true;
        seg.clear_all_features();
        seg.abs_delta = false;
        self.cr.qindex_delta[1] = qindex_delta1;
        let qindex2 = (base_qindex + qindex_delta1).clamp(0, MAXQ);
        self.cr.rdmult = crate::enc::rd::compute_rd_mult(qindex2, rd_frame);
        seg.set_feature(CR_SEGMENT_ID_BOOST1, SEG_LVL_ALT_Q, qindex_delta1);
        self.cr.qindex_delta[2] = qindex_delta2;
        seg.set_feature(CR_SEGMENT_ID_BOOST2, SEG_LVL_ALT_Q, qindex_delta2);
        self.cyclic_refresh_update_map(consec_zero_mv);
    }

    /// Choose the band: libvpx's `cyclic_refresh_update_map`. From the
    /// superblock the last band ended at, each superblock at least half of
    /// whose cells are due (a candidate, and coarsely coded or moving) goes
    /// to segment 1, until the frame's share is found or the frame has been
    /// walked once.
    fn cyclic_refresh_update_map(&mut self, consec_zero_mv: &[u8]) {
        let cm = &self.common;
        let (mi_rows, mi_cols) = (cm.mi_rows, cm.mi_cols);
        let noisy = self.noise.enabled && self.noise.level >= NoiseLevel::Medium;
        let cr = &mut self.cr;
        cr.seg_map.fill(CR_SEGMENT_ID_BASE);
        let sb_cols = mi_cols.div_ceil(8);
        let sb_rows = mi_rows.div_ceil(8);
        let sbs_in_frame = sb_cols * sb_rows;
        let block_count = cr.percent_refresh * (mi_rows * mi_cols) as i32 / 100;
        let mut i = cr.sb_index.min(sbs_in_frame.saturating_sub(1));
        cr.target_num_seg_blocks = 0;
        let mut consec_zero_mv_thresh = if self.oxcf.screen_content { 0 } else { 100 };
        let base_qindex = cm.quant.base_qindex;
        let boost_q = |id: u8| cm.seg.qindex(id, base_qindex);
        let mut qindex_thresh = if self.oxcf.screen_content {
            boost_q(CR_SEGMENT_ID_BOOST2)
        } else {
            boost_q(CR_SEGMENT_ID_BOOST1)
        };
        if noisy && cr.content_mode {
            consec_zero_mv_thresh = 60;
            qindex_thresh = boost_q(CR_SEGMENT_ID_BOOST1).max(base_qindex);
        }
        let (mut count_sel, mut count_tot) = (0i32, 0i32);
        let start = i;
        loop {
            let sb_row = i / sb_cols;
            let sb_col = i - sb_row * sb_cols;
            let (mi_row, mi_col) = (sb_row * 8, sb_col * 8);
            let bl_index = mi_row * mi_cols + mi_col;
            let xmis = (mi_cols - mi_col).min(8);
            let ymis = (mi_rows - mi_row).min(8);
            let thresh_block = if noisy && (xmis <= 2 || ymis <= 2) {
                4
            } else {
                consec_zero_mv_thresh
            };
            let mut sum_map = 0usize;
            for y in 0..ymis {
                for x in 0..xmis {
                    let b2 = bl_index + y * mi_cols + x;
                    let m = cr.map.get(b2).copied().unwrap_or(0);
                    if m == 0 {
                        count_tot += 1;
                        let q = i32::from(cr.last_coded_q_map.get(b2).copied().unwrap_or(0));
                        let czm = i32::from(consec_zero_mv.get(b2).copied().unwrap_or(0));
                        if !cr.content_mode || q > qindex_thresh || czm < thresh_block {
                            sum_map += 1;
                            count_sel += 1;
                        }
                    } else if m < 0 {
                        if let Some(c) = cr.map.get_mut(b2) {
                            *c += 1;
                        }
                    }
                }
            }
            // A superblock goes to segment 1 whole, if half its cells are
            // due. (Flat static superblocks are left out for screen content
            // only, which this encoder does not code.)
            if sum_map >= xmis * ymis / 2 {
                for y in 0..ymis {
                    let s = bl_index + y * mi_cols;
                    if let Some(cells) = cr.seg_map.get_mut(s..s + xmis) {
                        cells.fill(CR_SEGMENT_ID_BOOST1);
                    }
                }
                cr.target_num_seg_blocks += (xmis * ymis) as i32;
            }
            i += 1;
            if i == sbs_in_frame {
                i = 0;
            }
            if !(cr.target_num_seg_blocks < block_count && i != start) {
                break;
            }
        }
        cr.sb_index = i;
        cr.reduce_refresh = !self.oxcf.screen_content && count_sel < (3 * count_tot) >> 2;
    }

    /// After a frame: how many cells each boosted segment coded, how much
    /// of the frame was still, and whether a golden frame refresh due now
    /// should be skipped because the frame moved too much. libvpx's
    /// `vp9_cyclic_refresh_postencode`, CBR, one layer.
    pub(crate) fn cyclic_refresh_postencode(&mut self, mi: &MiGrid) {
        let (mi_rows, mi_cols) = (mi.mi_rows, mi.mi_cols);
        let cr = &mut self.cr;
        cr.actual_num_seg1_blocks = 0;
        cr.actual_num_seg2_blocks = 0;
        let mut low_content_frame = 0i32;
        for row in 0..mi_rows {
            for col in 0..mi_cols {
                match cr.seg_map.get(row * mi_cols + col).copied() {
                    Some(CR_SEGMENT_ID_BOOST1) => cr.actual_num_seg1_blocks += 1,
                    Some(CR_SEGMENT_ID_BOOST2) => cr.actual_num_seg2_blocks += 1,
                    _ => {}
                }
                if let Some(b) = mi.at(row, col)
                    && b.is_inter()
                    && i32::from(b.mv[0].row).abs() < 16
                    && i32::from(b.mv[0].col).abs() < 16
                {
                    low_content_frame += 1;
                }
            }
        }
        if self.oxcf.gf_cbr_boost_pct != 0 {
            return;
        }
        let fraction_low = f64::from(low_content_frame) / (mi_rows * mi_cols) as f64;
        cr.low_content_avg = (fraction_low + 3.0 * cr.low_content_avg) / 4.0;
        let rc = &self.rc;
        if self.refresh_golden_frame && rc.frames_since_key > rc.frames_since_golden + 1 {
            if fraction_low < 0.65 || cr.low_content_avg < 0.6 {
                self.refresh_golden_frame = false;
            }
            cr.low_content_avg = fraction_low;
        }
    }
}
