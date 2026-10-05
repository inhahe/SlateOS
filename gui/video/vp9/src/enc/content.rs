//! What libvpx's realtime encoder learns from the source pictures alone,
//! before it decides anything: how much each superblock changed since the
//! last picture (its content state), whether a scene changed, how noisy
//! the source is, and where there is skin.
//!
//! Each steers decisions: a superblock that barely changed keeps the last
//! frame's partition and skips intra modes; one that changed a lot is
//! searched harder; noise raises the partitioning's thresholds; skin is
//! refreshed by the cyclic refresh and kept in smaller blocks.
//!
//! Translated into Rust from libvpx v1.17.0's `vp9/encoder/vp9_encodeframe.c`
//! (`avg_source_sad`), `vp9_skin_detection.c` (`vp9_compute_skin_block`,
//! `vp9_compute_skin_sb`), `vpx_dsp/skin_detection.c` (`vpx_skin_pixel`),
//! `vp9_ratectrl.c` (`vp9_scene_detection_onepass`) and
//! `vp9_noise_estimate.c` (copyright the WebM project authors), used under
//! libvpx's BSD licence and patent grant (`licenses/libvpx-LICENSE`,
//! `licenses/libvpx-PATENTS`).

#![allow(
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    reason = "sums of 8-bit samples over 64x64 blocks and cell counts of one frame, in libvpx's integer widths"
)]
#![allow(
    clippy::indexing_slicing,
    reason = "the only indices are the skin model's five entries and the noise histogram's twenty bins, each within its loop's bounds; picture samples are read through get"
)]

use crate::enc::partition::{ContentState, NoiseLevel};
use crate::enc::variance;
use crate::frame::Plane;

/// A 64x64 block's sum of absolute differences between this picture and the
/// last, and its content state: libvpx's `avg_source_sad`. `y`, `x` are the
/// superblock's luma position; both planes repeat their edges out to whole
/// superblocks. Returns the sum and the state. (libvpx also notes a sum of
/// zero, `zero_temp_sad_source`, which only its screen-content mode reads.)
pub(crate) fn avg_source_sad(
    src: &Plane<u8>,
    last: &Plane<u8>,
    x: usize,
    y: usize,
) -> (u64, ContentState) {
    let s = src.data.get(y * src.stride + x..).unwrap_or(&[]);
    let l = last.data.get(y * last.stride + x..).unwrap_or(&[]);
    let tmp_sad = u64::from(variance::sad(s, src.stride, l, last.stride, 64, 64));
    let (tmp_variance, tmp_sse) = variance::variance(s, src.stride, l, last.stride, 64, 64);
    let low_sumdiff = tmp_sse.wrapping_sub(tmp_variance) < 25;
    let mut state = match (tmp_sad < 10_000, low_sumdiff) {
        (true, true) => ContentState::LowSadLowSumdiff,
        (true, false) => ContentState::LowSadHighSumdiff,
        (false, true) => ContentState::HighSadLowSumdiff,
        (false, false) => ContentState::HighSadHighSumdiff,
    };
    // A large lighting change (CBR, not screen content).
    if tmp_variance < (tmp_sse >> 3) && tmp_sse.wrapping_sub(tmp_variance) > 10_000 {
        state = ContentState::LowVarHighSumdiff;
    } else if tmp_sad > 20_000 {
        state = ContentState::VeryHighSad;
    }
    (tmp_sad, state)
}

/// libvpx's `skin_mean`, `skin_inv_cov` and `skin_threshold`: a fixed-point
/// skin colour model.
const SKIN_MEAN: [[i32; 2]; 5] = [
    [7463, 9614],
    [6400, 10240],
    [7040, 10240],
    [8320, 9280],
    [6800, 9614],
];
const SKIN_INV_COV: [i32; 4] = [4107, 1663, 1663, 2157];
const SKIN_THRESHOLD: [i32; 6] = [1_570_636, 1_400_000, 800_000, 800_000, 800_000, 800_000];

/// libvpx's `vpx_evaluate_skin_color_difference`: the Mahalanobis distance
/// of a (Cb, Cr) from model `idx`, in its 32-bit integer arithmetic.
fn skin_color_difference(cb: i32, cr: i32, idx: usize) -> i32 {
    let cb_q6 = cb << 6;
    let cr_q6 = cr << 6;
    let [mb, mr] = SKIN_MEAN[idx];
    let cb_diff_q12 = (cb_q6 - mb).wrapping_mul(cb_q6 - mb);
    let cbcr_diff_q12 = (cb_q6 - mb).wrapping_mul(cr_q6 - mr);
    let cr_diff_q12 = (cr_q6 - mr).wrapping_mul(cr_q6 - mr);
    let cb_diff_q2 = cb_diff_q12.wrapping_add(1 << 9) >> 10;
    let cbcr_diff_q2 = cbcr_diff_q12.wrapping_add(1 << 9) >> 10;
    let cr_diff_q2 = cr_diff_q12.wrapping_add(1 << 9) >> 10;
    SKIN_INV_COV[0]
        .wrapping_mul(cb_diff_q2)
        .wrapping_add(SKIN_INV_COV[1].wrapping_mul(cbcr_diff_q2))
        .wrapping_add(SKIN_INV_COV[2].wrapping_mul(cbcr_diff_q2))
        .wrapping_add(SKIN_INV_COV[3].wrapping_mul(cr_diff_q2))
}

/// Whether a pixel is skin-coloured: libvpx's `vpx_skin_pixel` (its model 1).
pub(crate) fn skin_pixel(y: i32, cb: i32, cr: i32, motion: bool) -> bool {
    if !(40..=220).contains(&y) {
        return false;
    }
    if cb == 128 && cr == 128 {
        return false;
    }
    if cb > 150 && cr < 110 {
        return false;
    }
    for i in 0..5 {
        let diff = skin_color_difference(cb, cr, i);
        let t = SKIN_THRESHOLD[i + 1];
        if diff < t {
            // Too dark for this close a match, or too far for a still one.
            return !((y < 60 && diff > 3 * (t >> 2)) || (!motion && diff > (t >> 1)));
        }
        if diff > (t << 3) {
            return false;
        }
    }
    false
}

/// Whether a block is skin, judged by its centre pixel: libvpx's
/// `vp9_compute_skin_block` for a block `bw` x `bh` whose luma starts at
/// `y_at` and chroma at `uv_at` (rows `stride`, `uv_stride` apart).
pub(crate) fn skin_block(
    y: &[u8],
    stride: usize,
    u: &[u8],
    v: &[u8],
    uv_stride: usize,
    bw: usize,
    bh: usize,
    consec_zeromv: i32,
) -> bool {
    if consec_zeromv > 60 {
        return false;
    }
    let (ys, xs) = (bh >> 1, bw >> 1);
    let (uys, uxs) = (ys >> 1, xs >> 1);
    let at = |p: &[u8], i: usize| i32::from(p.get(i).copied().unwrap_or(0));
    let motion = consec_zeromv <= 25;
    skin_pixel(
        at(y, ys * stride + xs),
        at(u, uys * uv_stride + uxs),
        at(v, uys * uv_stride + uxs),
        motion,
    )
}

/// The skin map of one superblock at 16x16: libvpx's `vp9_compute_skin_sb`
/// for `BLOCK_16X16`, isolated blocks then smoothed away. `consec_zero_mv`
/// and `skin_map` are per 8x8 cell, `mi_cols` to a row. libvpx skips the
/// frame's first row and column of blocks without moving its source
/// pointers, so in a superblock at column 0 each block reads the pixels 16
/// to its left; that is reproduced.
#[allow(clippy::too_many_arguments)]
pub(crate) fn skin_sb(
    src: [&Plane<u8>; 3],
    consec_zero_mv: &[u8],
    skin_map: &mut [bool],
    mi_rows: usize,
    mi_cols: usize,
    mi_row: usize,
    mi_col: usize,
) {
    let [py, pu, pv] = src;
    let (ys, uvs) = (py.stride, pu.stride);
    let fac = 2usize;
    let mi_row_limit = (mi_row + 8).min(mi_rows.saturating_sub(2));
    let mi_col_limit = (mi_col + 8).min(mi_cols.saturating_sub(2));
    // libvpx's moving pointers, as offsets into each plane.
    let mut y_off = ys * (mi_row << 3) + (mi_col << 3);
    let mut uv_off = uvs * (mi_row << 2) + (mi_col << 2);
    let czm = |i: usize| i32::from(consec_zero_mv.get(i).copied().unwrap_or(0));
    let mut i = mi_row;
    while i < mi_row_limit {
        let mut num_bl = 0usize;
        let mut j = mi_col;
        while j < mi_col_limit {
            let bl = i * mi_cols + j;
            if i == 0 || j == 0 {
                j += fac;
                continue;
            }
            let consec = czm(bl)
                .min(czm(bl + 1))
                .min(czm(bl + mi_cols))
                .min(czm(bl + mi_cols + 1));
            let is_skin = skin_block(
                py.data.get(y_off..).unwrap_or(&[]),
                ys,
                pu.data.get(uv_off..).unwrap_or(&[]),
                pv.data.get(uv_off..).unwrap_or(&[]),
                uvs,
                16,
                16,
                consec,
            );
            if let Some(s) = skin_map.get_mut(bl) {
                *s = is_skin;
            }
            num_bl += 1;
            y_off += 16;
            uv_off += 8;
            j += fac;
        }
        y_off = (y_off + (ys << 4)).wrapping_sub(num_bl << 4);
        uv_off = (uv_off + (uvs << 3)).wrapping_sub(num_bl << 3);
        i += fac;
    }
    // Remove isolated skin blocks and fill isolated non-skin ones.
    let mut i = mi_row;
    while i < mi_row_limit {
        let mut j = mi_col;
        while j < mi_col_limit {
            let bl = i * mi_cols + j;
            let corner = (i == mi_row || i + fac == mi_row_limit)
                && (j == mi_col || j + fac == mi_col_limit);
            if !corner {
                let border = i == mi_row
                    || i + fac == mi_row_limit
                    || j == mi_col
                    || j + fac == mi_col_limit;
                let non_skin_threshold = if border { 5 } else { 8 };
                let mut num_neighbor = 0;
                for di in [-2isize, 0, 2] {
                    for dj in [-2isize, 0, 2] {
                        let (ni, nj) = (i as isize + di, j as isize + dj);
                        if ni >= mi_row as isize
                            && ni < mi_row_limit as isize
                            && nj >= mi_col as isize
                            && nj < mi_col_limit as isize
                            && skin_map
                                .get(ni as usize * mi_cols + nj as usize)
                                .copied()
                                .unwrap_or(false)
                        {
                            num_neighbor += 1;
                        }
                    }
                }
                if let Some(s) = skin_map.get_mut(bl) {
                    if *s && num_neighbor < 2 {
                        *s = false;
                    }
                    if !*s && num_neighbor == non_skin_threshold {
                        *s = true;
                    }
                }
            }
            j += fac;
        }
        i += fac;
    }
}

/// Whether a superblock is mostly skin, so it is split and searched as
/// skin: libvpx's `skin_sb_split`, away from the frame's border.
pub(crate) fn skin_sb_split(
    skin_map: &[bool],
    mi_rows: usize,
    mi_cols: usize,
    mi_row: usize,
    mi_col: usize,
    low_res: bool,
) -> bool {
    if low_res || !(mi_col >= 8 && mi_col + 8 < mi_cols && mi_row >= 8 && mi_row + 8 < mi_rows) {
        return false;
    }
    let xmis = (mi_cols - mi_col).min(8);
    let ymis = (mi_rows - mi_row).min(8);
    let (mut skin, mut non) = (0, 0);
    'rows: for i in (0..ymis).step_by(2) {
        for j in (0..xmis).step_by(2) {
            let is_skin = skin_map
                .get((mi_row + i) * mi_cols + mi_col + j)
                .copied()
                .unwrap_or(false);
            if is_skin {
                skin += 1;
            } else {
                non += 1;
            }
            if non > 3 {
                break 'rows;
            }
        }
    }
    skin > 12
}

/// libvpx's scene detection's running state (`rc->avg_source_sad[0]`).
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct SceneState {
    pub avg_source_sad: u64,
}

/// Whether this picture starts a new scene: libvpx's
/// `vp9_scene_detection_onepass` for a single-layer realtime encode without
/// lag. The sums of differences of a checkerboard of interior 64x64 blocks
/// against the last picture are averaged; a jump far above their running
/// average (and enough blocks that changed) is a scene change.
/// Returns (high_source_sad, high_num_blocks_with_motion).
pub(crate) fn scene_detection(
    src: &Plane<u8>,
    last: &Plane<u8>,
    mi_rows: usize,
    mi_cols: usize,
    frames_since_key: i32,
    state: &mut SceneState,
) -> (bool, bool) {
    let min_thresh: u64 = 65_000;
    let thresh = 8.0f32;
    let sb_cols = mi_cols.div_ceil(8);
    let sb_rows = mi_rows.div_ceil(8);
    let mut avg_sad: u64 = 0;
    let mut num_samples: u64 = 0;
    let mut num_zero_temp_sad: u64 = 0;
    for r in 0..sb_rows {
        for c in 0..sb_cols {
            if r > 0
                && c > 0
                && r + 1 < sb_rows
                && c + 1 < sb_cols
                && ((r % 2 == 0 && c % 2 == 0) || (r % 2 != 0 && c % 2 != 0))
            {
                let (x, y) = (c * 64, r * 64);
                let s = src.data.get(y * src.stride + x..).unwrap_or(&[]);
                let l = last.data.get(y * last.stride + x..).unwrap_or(&[]);
                let tmp = u64::from(variance::sad(s, src.stride, l, last.stride, 64, 64));
                avg_sad += tmp;
                num_samples += 1;
                if tmp == 0 {
                    num_zero_temp_sad += 1;
                }
            }
        }
    }
    if num_samples > 0 {
        avg_sad /= num_samples;
    }
    // libvpx: (unsigned int)(avg_source_sad[0] * thresh), the product in
    // single precision.
    let scaled = (state.avg_source_sad as f32 * thresh) as u32;
    let high = avg_sad > min_thresh.max(u64::from(scaled))
        && frames_since_key > 2
        && num_zero_temp_sad < 3 * (num_samples >> 2);
    state.avg_source_sad = (3 * state.avg_source_sad + avg_sad) >> 2;
    let motion = num_zero_temp_sad < ((3 * num_samples) >> 2);
    (high, motion)
}

/// libvpx's `NOISE_ESTIMATE`, for a realtime CBR encode with cyclic
/// refresh at 640x360 or more (where it is enabled).
#[derive(Clone, Copy, Debug)]
pub(crate) struct NoiseEstimate {
    pub enabled: bool,
    pub level: NoiseLevel,
    pub value: i32,
    count: i32,
    thresh: i32,
    adapt_thresh: i32,
    num_frames_estimate: i32,
    last_w: u32,
    last_h: u32,
}

/// libvpx's `MAX_VAR_HIST_BINS`.
const MAX_VAR_HIST_BINS: usize = 20;

impl NoiseEstimate {
    /// libvpx's `vp9_noise_estimate_init`.
    pub(crate) fn new(width: u32, height: u32) -> Self {
        let pels = u64::from(width) * u64::from(height);
        let thresh = if pels >= 1920 * 1080 {
            200
        } else if pels >= 1280 * 720 {
            140
        } else if pels >= 640 * 360 {
            115
        } else {
            90
        };
        Self {
            enabled: false,
            level: if pels < 1280 * 720 {
                NoiseLevel::LowLow
            } else {
                NoiseLevel::Low
            },
            value: 0,
            count: 0,
            thresh,
            adapt_thresh: (3 * thresh) >> 1,
            num_frames_estimate: 15,
            last_w: 0,
            last_h: 0,
        }
    }

    /// The level the current value says: libvpx's
    /// `vp9_noise_estimate_extract_level`.
    pub(crate) fn extract_level(&self) -> NoiseLevel {
        if self.value > (self.thresh << 1) {
            NoiseLevel::High
        } else if self.value > self.thresh {
            NoiseLevel::Medium
        } else if self.value > (self.thresh >> 1) {
            NoiseLevel::Low
        } else {
            NoiseLevel::LowLow
        }
    }

    /// libvpx's `vp9_update_noise_estimate`: every eighth frame, the
    /// variance of steady background blocks between this picture and the
    /// last, binned; the busiest bin is the noise. `enable` is libvpx's
    /// `enable_noise_estimation`, `last` the last source picture if any,
    /// `encoded` how many frames were encoded before this one.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn update(
        &mut self,
        enable: bool,
        frame_counter: u32,
        src: [&Plane<u8>; 3],
        last: Option<&Plane<u8>>,
        width: u32,
        height: u32,
        mi_rows: usize,
        mi_cols: usize,
        consec_zero_mv: &[u8],
        scene_change: bool,
        use_skin_detection: bool,
        encoded: u32,
        frames_since_key: i32,
        avg_frame_low_motion: i32,
    ) {
        self.enabled = enable;
        let thresh_consec_zeromv = 6i32;
        let Some(last) = last else {
            return;
        };
        if !self.enabled
            || !frame_counter.is_multiple_of(8)
            || self.last_w != width
            || self.last_h != height
        {
            self.last_w = width;
            self.last_h = height;
            return;
        }
        let low_res = width <= 352 && height <= 288;
        if frame_counter > 60
            && encoded > 1
            && frames_since_key > 1
            && avg_frame_low_motion < if low_res { 60 } else { 40 }
        {
            // High motion: no noise estimate.
            self.level = NoiseLevel::LowLow;
            self.count = 0;
            self.num_frames_estimate = 10;
            return;
        }
        let czm = |i: usize| i32::from(consec_zero_mv.get(i).copied().unwrap_or(0));
        let cells = mi_rows * mi_cols;
        let num_low_motion = (0..cells)
            .filter(|&i| czm(i) > thresh_consec_zeromv)
            .count();
        let frame_low_motion = num_low_motion >= (3 * cells) >> 3;
        let mut hist = [0u32; MAX_VAR_HIST_BINS];
        let [py, pu, pv] = src;
        for mi_row in (0..mi_rows.saturating_sub(1)).step_by(4) {
            for mi_col in (0..mi_cols.saturating_sub(1)).step_by(4) {
                let bl = mi_row * mi_cols + mi_col;
                let consec = czm(bl)
                    .min(czm(bl + 1))
                    .min(czm(bl + mi_cols))
                    .min(czm(bl + mi_cols + 1));
                if !(frame_low_motion && consec > thresh_consec_zeromv && !scene_change) {
                    continue;
                }
                let (x, y) = (mi_col * 8, mi_row * 8);
                let ys = py.data.get(y * py.stride + x..).unwrap_or(&[]);
                let is_skin = use_skin_detection
                    && skin_block(
                        ys,
                        py.stride,
                        pu.data
                            .get((y >> 1) * pu.stride + (x >> 1)..)
                            .unwrap_or(&[]),
                        pv.data
                            .get((y >> 1) * pv.stride + (x >> 1)..)
                            .unwrap_or(&[]),
                        pu.stride,
                        16,
                        16,
                        consec,
                    );
                if !is_skin {
                    let ls = last.data.get(y * last.stride + x..).unwrap_or(&[]);
                    let (var, _) = variance::variance(ys, py.stride, ls, last.stride, 16, 16);
                    let hist_index = (var / 100) as usize;
                    if hist_index < MAX_VAR_HIST_BINS {
                        hist[hist_index] += 1;
                    } else if hist_index < 3 * (MAX_VAR_HIST_BINS >> 1) {
                        hist[MAX_VAR_HIST_BINS - 1] += 1;
                    }
                }
            }
        }
        self.last_w = width;
        self.last_h = height;
        // The histogram flattens toward zero as the scene darkens.
        if hist[0] > 10 && hist[MAX_VAR_HIST_BINS - 1] > hist[0] >> 2 {
            hist[0] = 0;
            hist[1] >>= 2;
            hist[2] >>= 2;
            hist[3] >>= 2;
            hist[4] >>= 1;
            hist[5] >>= 1;
            hist[6] = (3 * hist[6]) >> 1;
            hist[MAX_VAR_HIST_BINS - 1] >>= 1;
        }
        let mut max_bin = 0usize;
        let mut max_bin_count = 0u32;
        for b in 0..MAX_VAR_HIST_BINS {
            let avg = if b == 0 {
                (hist[0] + hist[1] + hist[2]) / 3
            } else if b == MAX_VAR_HIST_BINS - 1 {
                hist[b] >> 2
            } else if b == MAX_VAR_HIST_BINS - 2 {
                (hist[b - 1] + 2 * hist[b] + (hist[b + 1] >> 1) + 2) >> 2
            } else {
                (hist[b - 1] + 2 * hist[b] + hist[b + 1] + 2) >> 2
            };
            if avg > max_bin_count {
                max_bin_count = avg;
                max_bin = b;
            }
        }
        self.value = (3 * self.value + max_bin as i32 * 40) >> 2;
        if self.level < NoiseLevel::Medium && self.value > self.adapt_thresh {
            self.count = self.num_frames_estimate;
        } else {
            self.count += 1;
        }
        if self.count == self.num_frames_estimate {
            self.num_frames_estimate = 30;
            self.count = 0;
            self.level = self.extract_level();
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::indexing_slicing, reason = "a test: a failure should be loud")]

    use super::*;

    #[test]
    fn grey_and_extreme_pixels_are_not_skin() {
        assert!(!skin_pixel(100, 128, 128, true));
        assert!(!skin_pixel(30, 110, 150, true));
        assert!(!skin_pixel(100, 160, 100, true));
        // A typical skin tone in BT.601: Cb ~110, Cr ~150.
        assert!(skin_pixel(150, 113, 151, true));
    }

    #[test]
    fn the_noise_level_follows_its_thresholds() {
        let mut ne = NoiseEstimate::new(1280, 720);
        assert_eq!(ne.level, NoiseLevel::Low);
        ne.value = 141;
        assert_eq!(ne.extract_level(), NoiseLevel::Medium);
        ne.value = 281;
        assert_eq!(ne.extract_level(), NoiseLevel::High);
        ne.value = 70;
        assert_eq!(ne.extract_level(), NoiseLevel::LowLow);
    }
}
