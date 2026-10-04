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
//! Key frames refresh nothing; this holds what rate control needs from the
//! first frame on. The band and its map, the per-block decisions and the
//! statistics after each frame come with inter frames.
//!
//! Translated into Rust from libvpx v1.17.0's
//! `vp9/encoder/vp9_aq_cyclicrefresh.c` and `vp9_aq_cyclicrefresh.h`
//! (copyright the WebM project authors), used under libvpx's BSD licence
//! and patent grant (`licenses/libvpx-LICENSE`, `licenses/libvpx-PATENTS`).

#![allow(
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    reason = "libvpx's arithmetic: block counts bounded by the frame's 8x8 cells, quantiser indices in 0..=255, doubles truncated where libvpx truncates them"
)]

use crate::enc::cpi::Cpi;
use crate::enc::ratectrl::{INTER_FRAME, bits_per_mb, estimate_bits_at_q};
use crate::header::MAXQ;

/// libvpx's `CYCLIC_REFRESH`: the refresh's parameters and its maps.
#[derive(Clone, Debug)]
pub(crate) struct CyclicRefresh {
    /// The share of blocks refreshed per frame, in percent.
    pub percent_refresh: i32,
    /// The largest quantiser drop, as a percentage of the frame's.
    pub max_qdelta_perc: i32,
    /// Where the band starts, as a superblock index.
    pub sb_index: usize,
    pub time_for_refresh: i32,
    pub actual_num_seg1_blocks: i32,
    pub actual_num_seg2_blocks: i32,
    /// Per 8x8 cell: the quantiser it was last coded at.
    pub last_coded_q_map: Vec<u8>,
    pub motion_thresh: i16,
    pub rate_ratio_qdelta: f64,
    pub rate_boost_fac: i32,
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
    /// libvpx's `vp9_cyclic_refresh_alloc`.
    pub(crate) fn new(mi_rows: usize, mi_cols: usize) -> Self {
        let cells = mi_rows * mi_cols;
        Self {
            percent_refresh: 0,
            max_qdelta_perc: 0,
            sb_index: 0,
            time_for_refresh: 0,
            actual_num_seg1_blocks: 0,
            actual_num_seg2_blocks: 0,
            last_coded_q_map: vec![MAXQ as u8; cells],
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
    /// spatial and temporal layer.
    pub(crate) fn cyclic_refresh_update_parameters(&mut self) {
        let rc = &self.rc;
        let cm = &self.common;
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
            // libvpx lowers it for noisy sources when it estimates noise;
            // the encoder's noise estimate comes with inter frames.
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
    /// `vp9_cyclic_refresh_setup`. On a key frame -- the only frames coded
    /// so far -- the refresh is off: segmentation disabled, the maps
    /// cleared, the band back at the start.
    pub(crate) fn cyclic_refresh_setup(&mut self) {
        let scene_change_detected = self.rc.high_source_sad;
        if self.common.current_video_frame == 0 {
            self.cr.low_content_avg = 0.0;
        }
        debug_assert!(
            !self.cr.apply_cyclic_refresh || scene_change_detected,
            "the refresh's inter-frame band comes with inter frames"
        );
        // vp9_disable_segmentation (and libvpx clears its segmentation map,
        // which a key frame does not code).
        self.common.seg.enabled = false;
        if self.common.key_frame || scene_change_detected {
            self.cr.last_coded_q_map.fill(MAXQ as u8);
            self.cr.sb_index = 0;
            self.cr.reduce_refresh = false;
            self.cr.counter_encode_maxq_scene_change = 0;
        }
    }
}
