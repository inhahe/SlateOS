//! Rate control: how many bits each frame may spend, and the quantiser
//! expected to spend them -- libvpx's one-pass constant-bitrate (CBR)
//! control, the one its realtime mode runs.
//!
//! A leaky bucket models the decoder's buffer: each frame adds the average
//! frame's share of the bitrate and takes away what the frame really cost.
//! Each frame's target follows the bucket -- spend less when it runs low,
//! more when it is full -- and the quantiser is the one a model of bits per
//! macroblock against quantiser predicts will meet the target. After each
//! frame the model is corrected by how far the real size missed the
//! prediction, more gently once it has been wrong both ways.
//!
//! The arithmetic is libvpx's to the bit, floating point where libvpx's is:
//! IEEE doubles in the same order of operations give the same results, and
//! the one transcendental, a `log10` in the correction's damping, is only
//! ever truncated to an integer percentage.
//!
//! Translated into Rust from libvpx v1.17.0's `vp9/encoder/vp9_ratectrl.c`,
//! `vp9_ratectrl.h` and `vp9_encoder.c` (`vp9_set_rc_buffer_sizes`,
//! `vp9_new_framerate`, `adjust_frame_rate`) (copyright the WebM project
//! authors), used under libvpx's BSD licence and patent grant
//! (`licenses/libvpx-LICENSE`, `licenses/libvpx-PATENTS`).

#![allow(
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    reason = "libvpx's rate arithmetic: bit counts in i64, buffer levels bounded by the configured bitrate times seconds, and doubles truncated to int where libvpx truncates them"
)]

use crate::enc::cpi::{Cpi, Oxcf};
use crate::header::ac_quant;

/// The bits a frame costs at the least: libvpx's `FRAME_OVERHEAD_BITS`.
pub(crate) const FRAME_OVERHEAD_BITS: i32 = 200;
/// Bits per macroblock are kept scaled by 2^9: libvpx's `BPER_MB_NORMBITS`.
const BPER_MB_NORMBITS: u32 = 9;
const MIN_BPB_FACTOR: f64 = 0.005;
const MAX_BPB_FACTOR: f64 = 50.0;
/// The largest rate a frame may take, per macroblock and at 1080p and below.
const MAX_MB_RATE: i32 = 250;
const MAXRATE_1080P: i32 = 4_000_000;
pub(crate) const DEFAULT_KF_BOOST: i32 = 2000;
pub(crate) const DEFAULT_GF_BOOST: i32 = 2000;
const MIN_GF_INTERVAL: i32 = 4;
const MAX_GF_INTERVAL: i32 = 16;
const MAX_STATIC_GF_GROUP_LENGTH: i32 = 250;
/// Boost bounds for the key-frame and golden-frame active quality.
const KF_LOW: i32 = 300;
const KF_HIGH: i32 = 4800;
const GF_LOW: i32 = 400;
const GF_HIGH: i32 = 2000;
/// The quantiser index range.
pub(crate) const QINDEX_RANGE: usize = 256;

/// Rate factor levels: which correction factor a frame's model uses.
const INTER_NORMAL: usize = 0;
const GF_ARF_STD: usize = 3;
const KF_STD: usize = 4;
const RATE_FACTOR_LEVELS: usize = 5;

/// libvpx's `KEY_FRAME` and `INTER_FRAME`, as indices.
pub(crate) const KEY_FRAME: usize = 0;
pub(crate) const INTER_FRAME: usize = 1;

/// A quantiser index as the "real" quantiser libvpx's models use: the AC
/// step over 4, for 8-bit streams. libvpx's `vp9_convert_qindex_to_q`.
pub(crate) fn qindex_to_q(qindex: i32) -> f64 {
    f64::from(ac_quant(qindex, 0, 8)) / 4.0
}

/// The bits a macroblock is expected to cost at `qindex`, scaled by 2^9:
/// libvpx's `vp9_rc_bits_per_mb`.
pub(crate) fn bits_per_mb(key_frame: bool, qindex: i32, correction_factor: f64) -> i32 {
    let q = qindex_to_q(qindex);
    let mut enumerator: i32 = if key_frame { 2_700_000 } else { 1_800_000 };
    // libvpx: `enumerator += (int)(enumerator * q) >> 12`.
    enumerator += (f64::from(enumerator) * q) as i32 >> 12;
    (f64::from(enumerator) * correction_factor / q) as i32
}

/// The bits a frame of `mbs` macroblocks is expected to cost at `q`:
/// libvpx's `vp9_estimate_bits_at_q`.
pub(crate) fn estimate_bits_at_q(key_frame: bool, q: i32, mbs: i32, correction_factor: f64) -> i32 {
    let bpm = bits_per_mb(key_frame, q, correction_factor);
    let bits = (u64::try_from(bpm)
        .unwrap_or(0)
        .wrapping_mul(u64::try_from(mbs).unwrap_or(0))
        >> BPER_MB_NORMBITS) as i32;
    FRAME_OVERHEAD_BITS.max(bits)
}

/// The tables relating a frame's worst quantiser to its best: libvpx's
/// `vp9_rc_init_minq_luts`, 8-bit, less its two-pass `inter` table. Each is
/// a third-order polynomial fit, evaluated as libvpx evaluates it.
#[derive(Clone, Debug)]
pub(crate) struct MinqLuts {
    pub kf_low_motion: [i32; QINDEX_RANGE],
    pub kf_high_motion: [i32; QINDEX_RANGE],
    pub arfgf_low_motion: [i32; QINDEX_RANGE],
    pub arfgf_high_motion: [i32; QINDEX_RANGE],
    pub rtc: [i32; QINDEX_RANGE],
}

/// libvpx's `get_minq_index`.
fn minq_index(maxq: f64, x3: f64, x2: f64, x1: f64) -> i32 {
    let minqtarget = (((x3 * maxq + x2) * maxq + x1) * maxq).min(maxq);
    // The step from q 2.0 down to lossless (q 1.0).
    if minqtarget <= 2.0 {
        return 0;
    }
    (0..QINDEX_RANGE as i32)
        .find(|&i| minqtarget <= qindex_to_q(i))
        .unwrap_or(QINDEX_RANGE as i32 - 1)
}

impl MinqLuts {
    pub(crate) fn new() -> Self {
        let table = |x3: f64, x2: f64, x1: f64| {
            core::array::from_fn(|i| minq_index(qindex_to_q(i as i32), x3, x2, x1))
        };
        Self {
            kf_low_motion: table(0.000_001, -0.0004, 0.150),
            kf_high_motion: table(0.000_002_1, -0.001_25, 0.45),
            arfgf_low_motion: table(0.000_001_5, -0.0009, 0.30),
            arfgf_high_motion: table(0.000_002_1, -0.001_25, 0.55),
            rtc: table(0.000_002_71, -0.001_13, 0.70),
        }
    }
}

/// libvpx's `get_active_quality`.
fn active_quality(
    q: i32,
    gfu_boost: i32,
    low: i32,
    high: i32,
    low_minq: &[i32; QINDEX_RANGE],
    high_minq: &[i32; QINDEX_RANGE],
) -> i32 {
    let at = |t: &[i32; QINDEX_RANGE]| t.get(q.clamp(0, 255) as usize).copied().unwrap_or(0);
    if gfu_boost > high {
        at(low_minq)
    } else if gfu_boost < low {
        at(high_minq)
    } else {
        let gap = high - low;
        let offset = high - gfu_boost;
        let qdiff = at(high_minq) - at(low_minq);
        let adjustment = (offset * qdiff + (gap >> 1)) / gap;
        at(low_minq) + adjustment
    }
}

/// libvpx's `round`, half away from zero, saturated to i32 as libvpx's
/// `saturate_cast_double_to_int` does.
fn round_to_int(v: f64) -> i32 {
    let r = v.round();
    if r >= f64::from(i32::MAX) {
        i32::MAX
    } else if r <= f64::from(i32::MIN) {
        i32::MIN
    } else {
        r as i32
    }
}

/// The rate control's state: libvpx's `RATE_CONTROL`, the fields one-pass
/// CBR reads and writes.
#[derive(Clone, Debug)]
pub(crate) struct RateControl {
    pub minq: MinqLuts,
    pub avg_frame_bandwidth: i32,
    pub max_frame_bandwidth: i32,
    pub this_frame_target: i32,
    pub projected_frame_size: i32,
    pub last_q: [i32; 2],
    pub avg_frame_qindex: [i32; 2],
    pub last_boosted_qindex: i32,
    pub buffer_level: i64,
    pub bits_off_target: i64,
    pub starting_buffer_level: i64,
    pub optimal_buffer_level: i64,
    pub maximum_buffer_size: i64,
    pub avg_frame_low_motion: i32,
    pub frames_to_key: i32,
    pub frames_since_key: i32,
    pub frames_since_golden: i32,
    pub frames_till_gf_update_due: i32,
    pub kf_boost: i32,
    pub gfu_boost: i32,
    pub min_gf_interval: i32,
    pub max_gf_interval: i32,
    pub baseline_gf_interval: i32,
    pub static_scene_max_gf_interval: i32,
    pub rate_correction_factors: [f64; RATE_FACTOR_LEVELS],
    pub damped_adjustment: [bool; RATE_FACTOR_LEVELS],
    pub q_1_frame: i32,
    pub q_2_frame: i32,
    pub rc_1_frame: i32,
    pub rc_2_frame: i32,
    pub worst_quality: i32,
    pub best_quality: i32,
    pub this_key_frame_forced: bool,
    pub high_source_sad: bool,
    pub reset_high_source_sad: bool,
    pub force_max_q: bool,
}

/// libvpx's `vp9_rc_get_default_min_gf_interval`.
fn default_min_gf_interval(width: u32, height: u32, framerate: f64) -> i32 {
    // No constraint is needed below 4K at 20 frames a second.
    let factor_safe = 3840.0 * 2160.0 * 20.0;
    let factor = f64::from(width) * f64::from(height) * framerate;
    let default_interval = round_to_int(framerate * 0.125).clamp(MIN_GF_INTERVAL, MAX_GF_INTERVAL);
    if factor <= factor_safe {
        default_interval
    } else {
        default_interval.max(round_to_int(
            f64::from(MIN_GF_INTERVAL) * factor / factor_safe,
        ))
    }
}

/// libvpx's `vp9_rc_get_default_max_gf_interval`.
fn default_max_gf_interval(framerate: f64, min_gf_interval: i32) -> i32 {
    let mut interval = MAX_GF_INTERVAL.min(round_to_int(framerate * 0.75));
    interval += interval & 1;
    interval.max(min_gf_interval)
}

impl RateControl {
    /// libvpx's `vp9_rc_init` for one-pass CBR. The buffer levels and the
    /// frame bandwidths it reads are set after it, by [`Cpi::new`], which
    /// runs `vp9_change_config`'s part first as libvpx does.
    pub(crate) fn new(oxcf: &Oxcf) -> Self {
        let min_gf_interval = if oxcf.min_gf_interval == 0 {
            default_min_gf_interval(oxcf.width, oxcf.height, oxcf.init_framerate)
        } else {
            oxcf.min_gf_interval
        };
        let max_gf_interval = if oxcf.max_gf_interval == 0 {
            default_max_gf_interval(oxcf.init_framerate, min_gf_interval)
        } else {
            oxcf.max_gf_interval
        };
        Self {
            minq: MinqLuts::new(),
            avg_frame_bandwidth: 0,
            max_frame_bandwidth: 0,
            this_frame_target: 0,
            projected_frame_size: 0,
            last_q: [oxcf.best_allowed_q, oxcf.worst_allowed_q],
            // One-pass CBR starts both averages at the worst quality.
            avg_frame_qindex: [oxcf.worst_allowed_q, oxcf.worst_allowed_q],
            last_boosted_qindex: 0,
            buffer_level: 0,
            bits_off_target: 0,
            starting_buffer_level: 0,
            optimal_buffer_level: 0,
            maximum_buffer_size: 0,
            avg_frame_low_motion: 0,
            frames_to_key: 0,
            // "Sensible default for first frame."
            frames_since_key: 8,
            frames_since_golden: 0,
            frames_till_gf_update_due: 0,
            kf_boost: 0,
            gfu_boost: 0,
            min_gf_interval,
            max_gf_interval,
            baseline_gf_interval: i32::midpoint(min_gf_interval, max_gf_interval),
            static_scene_max_gf_interval: MAX_STATIC_GF_GROUP_LENGTH,
            rate_correction_factors: [1.0; RATE_FACTOR_LEVELS],
            damped_adjustment: [false; RATE_FACTOR_LEVELS],
            q_1_frame: 0,
            q_2_frame: 0,
            rc_1_frame: 0,
            rc_2_frame: 0,
            worst_quality: oxcf.worst_allowed_q,
            best_quality: oxcf.best_allowed_q,
            this_key_frame_forced: false,
            high_source_sad: false,
            reset_high_source_sad: false,
            force_max_q: false,
        }
    }
}

impl Cpi {
    /// libvpx's `vp9_set_rc_buffer_sizes`.
    pub(crate) fn set_rc_buffer_sizes(&mut self) {
        let rc = &mut self.rc;
        let bandwidth = self.oxcf.target_bandwidth;
        let starting = self.oxcf.starting_buffer_level_ms;
        let optimal = self.oxcf.optimal_buffer_level_ms;
        let maximum = self.oxcf.maximum_buffer_size_ms;
        rc.starting_buffer_level = starting * bandwidth / 1000;
        rc.optimal_buffer_level = if optimal == 0 {
            bandwidth / 8
        } else {
            optimal * bandwidth / 1000
        };
        rc.maximum_buffer_size = if maximum == 0 {
            bandwidth / 8
        } else {
            maximum * bandwidth / 1000
        };
        rc.bits_off_target = rc.bits_off_target.min(rc.maximum_buffer_size);
        rc.buffer_level = rc.buffer_level.min(rc.maximum_buffer_size);
    }

    /// libvpx's `vp9_new_framerate` and `vp9_rc_update_framerate`.
    pub(crate) fn new_framerate(&mut self, framerate: f64) {
        self.framerate = if framerate < 0.1 { 30.0 } else { framerate };
        let rc = &mut self.rc;
        let oxcf = &self.oxcf;
        rc.avg_frame_bandwidth = round_to_int(oxcf.target_bandwidth as f64 / self.framerate);
        let vbr_max_bits =
            (i64::from(rc.avg_frame_bandwidth) * i64::from(oxcf.two_pass_vbrmax_section) / 100)
                .min(i64::from(i32::MAX));
        rc.max_frame_bandwidth = (self.common.mbs.saturating_mul(MAX_MB_RATE))
            .max(MAXRATE_1080P)
            .max(vbr_max_bits as i32);
        // vp9_rc_set_gf_interval_range, one-pass CBR, no target level.
        rc.max_gf_interval = oxcf.max_gf_interval;
        rc.min_gf_interval = oxcf.min_gf_interval;
        if rc.min_gf_interval == 0 {
            rc.min_gf_interval = default_min_gf_interval(oxcf.width, oxcf.height, self.framerate);
        }
        if rc.max_gf_interval == 0 {
            rc.max_gf_interval = default_max_gf_interval(self.framerate, rc.min_gf_interval);
        }
        rc.static_scene_max_gf_interval = MAX_STATIC_GF_GROUP_LENGTH;
        rc.max_gf_interval = rc.max_gf_interval.min(rc.static_scene_max_gf_interval);
        rc.min_gf_interval = rc.min_gf_interval.min(rc.max_gf_interval);
    }

    /// Track the frame rate from the timestamps: libvpx's
    /// `adjust_frame_rate`. `ts_start` and `ts_end` are in 1/10,000,000 s.
    pub(crate) fn adjust_frame_rate(&mut self, ts_start: i64, ts_end: i64) {
        if ts_start < self.first_time_stamp_ever {
            self.first_time_stamp_ever = ts_start;
            self.last_end_time_stamp_seen = ts_start;
        }
        let (this_duration, step) = if ts_start == self.first_time_stamp_ever {
            (ts_end - ts_start, 1)
        } else {
            let last_duration = self.last_end_time_stamp_seen - self.last_time_stamp_seen;
            let this_duration = ts_end - self.last_end_time_stamp_seen;
            // A step update if the duration changes by 10%.
            let step = if last_duration != 0 {
                (this_duration - last_duration) * 10 / last_duration
            } else {
                0
            };
            (this_duration, step)
        };
        if this_duration != 0 {
            if step != 0 {
                self.new_framerate(10_000_000.0 / this_duration as f64);
            } else {
                // Average this frame's rate into the last second's.
                let interval = ((ts_end - self.first_time_stamp_ever) as f64).min(10_000_000.0);
                let mut avg_duration = 10_000_000.0 / self.framerate;
                avg_duration *= interval - avg_duration + this_duration as f64;
                avg_duration /= interval;
                self.new_framerate(10_000_000.0 / avg_duration);
            }
        }
        self.last_time_stamp_seen = ts_start;
        self.last_end_time_stamp_seen = ts_end;
    }

    /// libvpx's `vp9_rc_get_one_pass_cbr_params`: is this a key frame, does
    /// it refresh the golden frame, and how many bits may it spend.
    pub(crate) fn rc_get_one_pass_cbr_params(&mut self) {
        let cm = &mut self.common;
        let rc = &mut self.rc;
        // libvpx also starts a key frame when the deadline mode changes,
        // which a realtime encoder's never does.
        if cm.current_video_frame == 0
            || self.force_key_frame
            || (self.oxcf.auto_key && rc.frames_to_key == 0)
        {
            cm.key_frame = true;
            rc.frames_to_key = self.oxcf.key_freq;
            rc.kf_boost = DEFAULT_KF_BOOST;
        } else {
            cm.key_frame = false;
        }
        if rc.frames_till_gf_update_due == 0 {
            if self.oxcf.cyclic_refresh {
                self.cyclic_refresh_set_golden_update();
            } else {
                self.rc.baseline_gf_interval =
                    i32::midpoint(self.rc.min_gf_interval, self.rc.max_gf_interval);
            }
            let rc = &mut self.rc;
            rc.frames_till_gf_update_due = rc.baseline_gf_interval.min(rc.frames_to_key);
            self.refresh_golden_frame = true;
            rc.gfu_boost = DEFAULT_GF_BOOST;
        }
        // Cyclic refresh's parameters change here, before the frame's
        // quantiser is chosen.
        if self.oxcf.cyclic_refresh {
            self.cyclic_refresh_update_parameters();
        }
        let target = if self.common.frame_is_intra_only() {
            self.calc_iframe_target_size_one_pass_cbr()
        } else {
            self.calc_pframe_target_size_one_pass_cbr()
        };
        self.rc_set_frame_target(target);
        if self.common.show_frame {
            // vp9_update_buffer_level_preencode.
            let rc = &mut self.rc;
            rc.bits_off_target += i64::from(rc.avg_frame_bandwidth);
            rc.bits_off_target = rc.bits_off_target.min(rc.maximum_buffer_size);
            rc.buffer_level = rc.bits_off_target;
        }
    }

    /// libvpx's `vp9_calc_pframe_target_size_one_pass_cbr`.
    fn calc_pframe_target_size_one_pass_cbr(&self) -> i32 {
        let oxcf = &self.oxcf;
        let rc = &self.rc;
        let diff = rc.optimal_buffer_level - rc.buffer_level;
        let one_pct_bits = 1 + rc.optimal_buffer_level / 100;
        let min_frame_target = (rc.avg_frame_bandwidth >> 4).max(FRAME_OVERHEAD_BITS);
        let mut target: i64 = if oxcf.gf_cbr_boost_pct != 0 && rc.baseline_gf_interval != i32::MAX {
            let af_ratio_pct = i64::from(oxcf.gf_cbr_boost_pct) + 100;
            let interval = i64::from(rc.baseline_gf_interval);
            let den = interval * 100 + af_ratio_pct - 100;
            if self.refresh_golden_frame {
                i64::from(rc.avg_frame_bandwidth) * interval * af_ratio_pct / den
            } else {
                i64::from(rc.avg_frame_bandwidth) * interval * 100 / den
            }
        } else {
            i64::from(rc.avg_frame_bandwidth)
        };
        if diff > 0 {
            // Lower the target when the buffer runs below its optimum.
            let pct_low = (diff / one_pct_bits).min(i64::from(oxcf.under_shoot_pct));
            target -= target * pct_low / 200;
        } else if diff < 0 {
            let pct_high = (-diff / one_pct_bits).min(i64::from(oxcf.over_shoot_pct));
            target += target * pct_high / 200;
        }
        if oxcf.rc_max_inter_bitrate_pct != 0 {
            let max_rate =
                i64::from(rc.avg_frame_bandwidth) * i64::from(oxcf.rc_max_inter_bitrate_pct) / 100;
            target = target.min(max_rate);
        }
        min_frame_target.max(target.min(i64::from(i32::MAX)) as i32)
    }

    /// libvpx's `vp9_calc_iframe_target_size_one_pass_cbr`, with
    /// `vp9_rc_clamp_iframe_target_size`.
    fn calc_iframe_target_size_one_pass_cbr(&self) -> i32 {
        let rc = &self.rc;
        let target: i64 = if self.common.current_video_frame == 0 {
            rc.starting_buffer_level / 2
        } else {
            let framerate = self.framerate;
            let mut kf_boost = 32.max(round_to_int(2.0 * framerate - 16.0));
            if f64::from(rc.frames_since_key) < framerate / 2.0 {
                kf_boost = round_to_int(
                    f64::from(kf_boost) * f64::from(rc.frames_since_key) / (framerate / 2.0),
                );
            }
            (i64::from(16 + kf_boost) * i64::from(rc.avg_frame_bandwidth)) >> 4
        };
        let mut target = target.min(i64::from(i32::MAX)) as i32;
        if self.oxcf.rc_max_intra_bitrate_pct != 0 {
            let max_rate = i64::from(rc.avg_frame_bandwidth)
                * i64::from(self.oxcf.rc_max_intra_bitrate_pct)
                / 100;
            target = (i64::from(target).min(max_rate)) as i32;
        }
        target.min(rc.max_frame_bandwidth)
    }

    /// libvpx's `vp9_rc_set_frame_target`, without dynamic resizing. (Its
    /// per-superblock rate is what cyclic refresh's band reads, and comes
    /// with it.)
    fn rc_set_frame_target(&mut self, target: i32) {
        self.rc.this_frame_target = target;
    }

    /// The rate correction factor this frame's model uses: libvpx's
    /// `get_rate_correction_factor`, one pass, unscaled.
    fn rate_correction_factor(&self) -> f64 {
        let rc = &self.rc;
        // libvpx's test is also `rc_mode != VPX_CBR`, false here.
        let rcf = if self.common.frame_is_intra_only() {
            rc.rate_correction_factors[KF_STD]
        } else if (self.refresh_alt_ref_frame || self.refresh_golden_frame)
            && self.oxcf.gf_cbr_boost_pct > 100
        {
            rc.rate_correction_factors[GF_ARF_STD]
        } else {
            rc.rate_correction_factors[INTER_NORMAL]
        };
        rcf.clamp(MIN_BPB_FACTOR, MAX_BPB_FACTOR)
    }

    /// libvpx's `set_rate_correction_factor`.
    fn set_rate_correction_factor(&mut self, factor: f64) {
        let factor = factor.clamp(MIN_BPB_FACTOR, MAX_BPB_FACTOR);
        let level = if self.common.frame_is_intra_only() {
            KF_STD
        } else if (self.refresh_alt_ref_frame || self.refresh_golden_frame)
            && self.oxcf.gf_cbr_boost_pct > 100
        {
            GF_ARF_STD
        } else {
            INTER_NORMAL
        };
        if let Some(f) = self.rc.rate_correction_factors.get_mut(level) {
            *f = factor;
        }
    }

    /// Correct the model by how far the frame's real size missed its
    /// prediction: libvpx's `vp9_rc_update_rate_correction_factors`.
    fn rc_update_rate_correction_factors(&mut self) {
        let cm = &self.common;
        let mut rate_correction_factor = self.rate_correction_factor();
        let projected_size_based_on_q = if self.oxcf.cyclic_refresh && cm.seg.enabled {
            self.cyclic_refresh_estimate_bits_at_q(rate_correction_factor)
        } else {
            let key = cm.intra_only || cm.key_frame;
            estimate_bits_at_q(key, cm.quant.base_qindex, cm.mbs, rate_correction_factor)
        };
        let mut correction_factor = 100;
        if projected_size_based_on_q > FRAME_OVERHEAD_BITS {
            correction_factor = (100 * i64::from(self.rc.projected_frame_size)
                / i64::from(projected_size_based_on_q)) as i32;
        }
        // One pass has one rate factor level for its damping:
        // gf_group.rf_level[0], which is INTER_NORMAL.
        let rc = &mut self.rc;
        let adjustment_limit = if rc.damped_adjustment[INTER_NORMAL] {
            0.25 + 0.5 * (0.01 * f64::from(correction_factor)).log10().abs().min(1.0)
        } else {
            rc.damped_adjustment[INTER_NORMAL] = true;
            1.0
        };
        rc.q_2_frame = rc.q_1_frame;
        rc.q_1_frame = cm.quant.base_qindex;
        rc.rc_2_frame = rc.rc_1_frame;
        rc.rc_1_frame = if correction_factor > 110 {
            -1
        } else if correction_factor < 90 {
            1
        } else {
            0
        };
        // No oscillation detection after a massive overshoot.
        if rc.rc_1_frame == -1 && rc.rc_2_frame == 1 && correction_factor > 1000 {
            rc.rc_2_frame = 0;
        }
        if correction_factor > 102 {
            correction_factor =
                (100.0 + f64::from(correction_factor - 100) * adjustment_limit) as i32;
            rate_correction_factor = rate_correction_factor * f64::from(correction_factor) / 100.0;
            rate_correction_factor = rate_correction_factor.min(MAX_BPB_FACTOR);
        } else if correction_factor < 99 {
            correction_factor =
                (100.0 - f64::from(100 - correction_factor) * adjustment_limit) as i32;
            rate_correction_factor = rate_correction_factor * f64::from(correction_factor) / 100.0;
            rate_correction_factor = rate_correction_factor.max(MIN_BPB_FACTOR);
        }
        self.set_rate_correction_factor(rate_correction_factor);
    }

    /// The quantiser whose predicted rate is nearest the target: libvpx's
    /// `vp9_rc_regulate_q`, with `adjust_q_cbr`.
    fn rc_regulate_q(
        &self,
        target_bits_per_frame: i32,
        active_best: i32,
        active_worst: i32,
    ) -> i32 {
        let cm = &self.common;
        let mut q = active_worst;
        let mut last_error = i32::MAX;
        let correction_factor = self.rate_correction_factor();
        let target_bits_per_mb = ((u64::try_from(target_bits_per_frame).unwrap_or(0)
            << BPER_MB_NORMBITS)
            / u64::try_from(cm.mbs.max(1)).unwrap_or(1)) as i32;
        let mut i = active_best;
        loop {
            let bits_per_mb_at_this_q = if self.oxcf.cyclic_refresh
                && self.cr.apply_cyclic_refresh
                && (self.oxcf.gf_cbr_boost_pct == 0 || !self.refresh_golden_frame)
            {
                self.cyclic_refresh_rc_bits_per_mb(i, correction_factor)
            } else {
                let key = cm.intra_only || cm.key_frame;
                bits_per_mb(key, i, correction_factor)
            };
            let diff_bits = (i64::from(target_bits_per_mb) - i64::from(bits_per_mb_at_this_q))
                .clamp(-i64::from(i32::MAX), i64::from(i32::MAX))
                as i32;
            if bits_per_mb_at_this_q <= target_bits_per_mb {
                q = if diff_bits <= last_error { i } else { i - 1 };
                break;
            }
            last_error = -diff_bits;
            i += 1;
            if i > active_worst {
                break;
            }
        }
        self.adjust_q_cbr(q)
    }

    /// Keep q between the last two frames' when the rate has been
    /// oscillating about the target: libvpx's `adjust_q_cbr`.
    fn adjust_q_cbr(&self, q: i32) -> i32 {
        let rc = &self.rc;
        let mut q = q;
        if !rc.reset_high_source_sad
            && (self.oxcf.gf_cbr_boost_pct == 0
                || !(self.refresh_alt_ref_frame || self.refresh_golden_frame))
            && rc.rc_1_frame * rc.rc_2_frame == -1
            && rc.q_1_frame != rc.q_2_frame
        {
            let qclamp = q.clamp(
                rc.q_1_frame.min(rc.q_2_frame),
                rc.q_1_frame.max(rc.q_2_frame),
            );
            // After an overshoot, react faster to a q that must rise.
            q = if rc.rc_1_frame == -1 && q > qclamp {
                (q + qclamp) >> 1
            } else {
                qclamp
            };
        }
        if self.oxcf.screen_content && self.oxcf.cyclic_refresh {
            q = self.cyclic_refresh_limit_q(q);
        }
        q.min(rc.worst_quality).max(rc.best_quality)
    }

    /// libvpx's `calc_active_worst_quality_one_pass_cbr`: the worst
    /// quantiser this frame may use, following the buffer.
    fn calc_active_worst_quality_one_pass_cbr(&self) -> i32 {
        let cm = &self.common;
        let rc = &self.rc;
        let critical_level = rc.optimal_buffer_level >> 3;
        if cm.frame_is_intra_only() || rc.reset_high_source_sad || rc.force_max_q {
            return rc.worst_quality;
        }
        // One temporal layer: the key frame's quantiser weighs in for five
        // frames.
        let num_frames_weight_key = 5;
        let ambient_qp = if cm.current_video_frame < num_frames_weight_key {
            rc.avg_frame_qindex[INTER_FRAME].min(rc.avg_frame_qindex[KEY_FRAME])
        } else {
            rc.avg_frame_qindex[INTER_FRAME]
        };
        let mut active_worst_quality = rc.worst_quality.min((ambient_qp * 5) >> 2);
        if rc.buffer_level > rc.optimal_buffer_level {
            // Adjust down, by at most about 30% (less for screen content).
            let max_adjustment_down = if self.oxcf.screen_content {
                active_worst_quality >> 3
            } else {
                active_worst_quality / 3
            };
            if max_adjustment_down != 0 {
                let buff_lvl_step = (rc.maximum_buffer_size - rc.optimal_buffer_level)
                    / i64::from(max_adjustment_down);
                let adjustment = if buff_lvl_step != 0 {
                    ((rc.buffer_level - rc.optimal_buffer_level) / buff_lvl_step) as i32
                } else {
                    0
                };
                active_worst_quality -= adjustment;
            }
        } else if rc.buffer_level > critical_level {
            // Adjust up from the ambient quantiser.
            if critical_level != 0 {
                let buff_lvl_step = rc.optimal_buffer_level - critical_level;
                let adjustment = if buff_lvl_step != 0 {
                    (i64::from(rc.worst_quality - ambient_qp)
                        * (rc.optimal_buffer_level - rc.buffer_level)
                        / buff_lvl_step) as i32
                } else {
                    0
                };
                active_worst_quality = ambient_qp + adjustment;
            }
        } else {
            active_worst_quality = rc.worst_quality;
        }
        active_worst_quality
    }

    /// libvpx's `vp9_compute_qdelta`.
    fn compute_qdelta(&self, qstart: f64, qtarget: f64) -> i32 {
        let rc = &self.rc;
        let mut start_index = rc.worst_quality;
        let mut target_index = rc.worst_quality;
        for i in rc.best_quality..rc.worst_quality {
            start_index = i;
            if qindex_to_q(i) >= qstart {
                break;
            }
        }
        for i in rc.best_quality..rc.worst_quality {
            target_index = i;
            if qindex_to_q(i) >= qtarget {
                break;
            }
        }
        target_index - start_index
    }

    /// The quantiser index change that scales the rate by
    /// `rate_target_ratio`: libvpx's `vp9_compute_qdelta_by_rate`.
    pub(crate) fn compute_qdelta_by_rate(
        &self,
        key_frame: bool,
        qindex: i32,
        rate_target_ratio: f64,
    ) -> i32 {
        let rc = &self.rc;
        let base_bits_per_mb = bits_per_mb(key_frame, qindex, 1.0);
        let target_bits_per_mb = (rate_target_ratio * f64::from(base_bits_per_mb)) as i32;
        let target_index = (rc.best_quality..rc.worst_quality)
            .find(|&i| bits_per_mb(key_frame, i, 1.0) <= target_bits_per_mb)
            .unwrap_or(rc.worst_quality);
        target_index - qindex
    }

    /// The frame's quantiser and the bounds it was chosen in: libvpx's
    /// `rc_pick_q_and_bounds_one_pass_cbr`. Returns (q, bottom, top).
    pub(crate) fn rc_pick_q_and_bounds_one_pass_cbr(&self) -> (i32, i32, i32) {
        let cm = &self.common;
        let rc = &self.rc;
        let mut active_worst_quality = self.calc_active_worst_quality_one_pass_cbr();
        let mut active_best_quality;
        let rtc = |q: i32| {
            rc.minq
                .rtc
                .get(q.clamp(0, 255) as usize)
                .copied()
                .unwrap_or(0)
        };
        if cm.frame_is_intra_only() {
            active_best_quality = rc.best_quality;
            if rc.this_key_frame_forced {
                let qindex = rc.last_boosted_qindex;
                let last_boosted_q = qindex_to_q(qindex);
                let delta_qindex = self.compute_qdelta(last_boosted_q, last_boosted_q * 0.75);
                active_best_quality = (qindex + delta_qindex).max(rc.best_quality);
            } else if cm.current_video_frame > 0 {
                // Not the first frame: the key frame's boost sets its best.
                let mut q_adj_factor = 1.0;
                active_best_quality = active_quality(
                    rc.avg_frame_qindex[KEY_FRAME],
                    rc.kf_boost,
                    KF_LOW,
                    KF_HIGH,
                    &rc.minq.kf_low_motion,
                    &rc.minq.kf_high_motion,
                );
                if u64::from(cm.width) * u64::from(cm.height) <= 352 * 288 {
                    q_adj_factor -= 0.25;
                }
                let q_val = qindex_to_q(active_best_quality);
                active_best_quality += self.compute_qdelta(q_val, q_val * q_adj_factor);
            }
        } else if self.oxcf.gf_cbr_boost_pct != 0
            && (self.refresh_golden_frame || self.refresh_alt_ref_frame)
        {
            let q = if rc.frames_since_key > 1
                && rc.avg_frame_qindex[INTER_FRAME] < active_worst_quality
            {
                rc.avg_frame_qindex[INTER_FRAME]
            } else {
                active_worst_quality
            };
            active_best_quality = active_quality(
                q,
                rc.gfu_boost,
                GF_LOW,
                GF_HIGH,
                &rc.minq.arfgf_low_motion,
                &rc.minq.arfgf_high_motion,
            );
        } else {
            // The lower of the active worst and the recent average.
            let average = if cm.current_video_frame > 1 {
                rc.avg_frame_qindex[INTER_FRAME]
            } else {
                rc.avg_frame_qindex[KEY_FRAME]
            };
            active_best_quality = if average < active_worst_quality {
                rtc(average)
            } else {
                rtc(active_worst_quality)
            };
        }
        active_best_quality = active_best_quality.clamp(rc.best_quality, rc.worst_quality);
        active_worst_quality = active_worst_quality.clamp(active_best_quality, rc.worst_quality);
        let mut top_index = active_worst_quality;
        let bottom_index = active_best_quality;
        let q = if cm.frame_is_intra_only() && rc.this_key_frame_forced {
            rc.last_boosted_qindex
                .clamp(rc.best_quality, rc.worst_quality)
        } else {
            let mut q = self.rc_regulate_q(
                rc.this_frame_target,
                active_best_quality,
                active_worst_quality,
            );
            if q > top_index {
                // Targeting the most a frame may take: let q exceed.
                if rc.this_frame_target >= rc.max_frame_bandwidth {
                    top_index = q;
                } else {
                    q = top_index;
                }
            }
            q
        };
        (q, bottom_index, top_index)
    }

    /// After a frame: the model's correction, the averages and the buffer.
    /// libvpx's `vp9_rc_postencode_update`, one pass, no alt-ref.
    pub(crate) fn rc_postencode_update(&mut self, bytes_used: u64) {
        let qindex = self.common.quant.base_qindex;
        self.rc.projected_frame_size = (bytes_used << 3).min(i32::MAX as u64) as i32;
        self.rc_update_rate_correction_factors();
        let intra_only = self.common.frame_is_intra_only();
        let rc = &mut self.rc;
        if intra_only {
            rc.last_q[KEY_FRAME] = qindex;
            rc.avg_frame_qindex[KEY_FRAME] = (3 * rc.avg_frame_qindex[KEY_FRAME] + qindex + 2) >> 2;
        } else if !(self.refresh_golden_frame || self.refresh_alt_ref_frame) {
            rc.last_q[INTER_FRAME] = qindex;
            rc.avg_frame_qindex[INTER_FRAME] =
                (3 * rc.avg_frame_qindex[INTER_FRAME] + qindex + 2) >> 2;
        }
        // The last boosted (key or golden) quantiser, or a lower one.
        if qindex < rc.last_boosted_qindex
            || self.common.key_frame
            || self.refresh_alt_ref_frame
            || self.refresh_golden_frame
        {
            rc.last_boosted_qindex = qindex;
        }
        // update_buffer_level_postencode.
        rc.bits_off_target -= i64::from(rc.projected_frame_size);
        rc.bits_off_target = rc.bits_off_target.min(rc.maximum_buffer_size);
        if self.oxcf.screen_content && self.oxcf.drop_frames_water_mark == 0 {
            rc.bits_off_target = rc.bits_off_target.max(-rc.maximum_buffer_size);
        }
        rc.buffer_level = rc.bits_off_target;
        // update_golden_frame_stats (no alt-ref in one-pass realtime).
        if self.refresh_golden_frame {
            rc.frames_since_golden = 0;
            if rc.frames_till_gf_update_due > 0 {
                rc.frames_till_gf_update_due -= 1;
            }
        } else if !self.refresh_alt_ref_frame {
            if rc.frames_till_gf_update_due > 0 {
                rc.frames_till_gf_update_due -= 1;
            }
            rc.frames_since_golden += 1;
        }
        if intra_only {
            rc.frames_since_key = 0;
        }
        if self.common.show_frame {
            rc.frames_since_key += 1;
            rc.frames_to_key -= 1;
        }
        if !intra_only {
            rc.reset_high_source_sad = false;
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        reason = "a test: a failure should be loud"
    )]

    use super::*;
    use crate::enc::encoder::EncoderConfig;

    /// A compressor whose frame has 512 macroblocks, so a frame's target in
    /// bits is its target per macroblock in the model's 2^-9 units.
    fn cpi_512_mbs() -> Cpi {
        let mut cpi = Cpi::new(EncoderConfig::realtime(512, 256, 1000).oxcf());
        assert_eq!(cpi.common.mbs, 512);
        cpi.common.key_frame = true;
        cpi
    }

    /// The model's quantiser is the one whose predicted rate is nearest the
    /// target: the first index at or under it, or the one before when that
    /// is closer. libvpx's `vp9_rc_regulate_q` both ways round.
    #[test]
    fn the_quantiser_is_the_one_nearest_the_target() {
        let cpi = cpi_512_mbs();
        let (best, worst) = (cpi.rc.best_quality, cpi.rc.worst_quality);
        let mut stepped_back = 0;
        for k in best + 1..worst {
            let at_k = bits_per_mb(true, k, 1.0);
            // Exactly the rate at k: k.
            assert_eq!(cpi.rc_regulate_q(at_k, best, worst), k, "at {k}");
            // Just under it: k + 1 is the first under the target, but k is
            // nearer -- unless k + 1 is as near.
            let target = at_k - 1;
            let under = bits_per_mb(true, k + 1, 1.0);
            let want = if target - under <= 1 { k + 1 } else { k };
            assert_eq!(cpi.rc_regulate_q(target, best, worst), want, "under {k}");
            stepped_back += usize::from(want == k);
        }
        assert!(
            stepped_back > 100,
            "the step back was tried {stepped_back} times"
        );
        // Out of range either way: the bounds.
        assert_eq!(cpi.rc_regulate_q(i32::MAX / 1024, best, worst), best);
        assert_eq!(cpi.rc_regulate_q(1, best, worst), worst);
    }

    #[test]
    fn bits_per_mb_falls_as_the_quantiser_rises() {
        for q in 1..255 {
            assert!(bits_per_mb(true, q + 1, 1.0) <= bits_per_mb(true, q, 1.0));
            assert!(bits_per_mb(false, q, 1.0) < bits_per_mb(true, q, 1.0));
        }
        // libvpx's model at q index 161 (AC step 311, q 77.75):
        // 2700000 + ((int)(2700000 * 77.75) >> 12) = 2751251, / 77.75 = 35385.
        assert_eq!(bits_per_mb(true, 161, 1.0), 35_385);
    }

    /// The minq tables' ends and a middle value, as libvpx's polynomial fit
    /// gives them.
    #[test]
    fn the_rtc_minq_table_follows_libvpxs_fit() {
        let luts = MinqLuts::new();
        assert_eq!(luts.rtc[0], 0);
        for q in 1..QINDEX_RANGE {
            assert!(luts.rtc[q] <= q as i32 && luts.rtc[q] >= luts.rtc[q - 1]);
        }
    }
}
