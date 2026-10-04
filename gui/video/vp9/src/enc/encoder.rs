//! The encoder: pictures in, compressed frames out.
//!
//! This is libvpx's realtime frame loop being ported: `vp9_cx_iface.c`'s
//! `encoder_encode` (timestamps), `vp9_encoder.c`'s
//! `vp9_get_compressed_data`, `Pass0Encode`, `encode_frame_to_data_rate`
//! and `encode_without_recode_loop` -- the frame rate tracked from the
//! timestamps, the rate control's target and quantiser, the cyclic refresh
//! set up, the blocks encoded and reconstructed (`encodeframe`), the
//! reconstruction loop-filtered at the level libvpx's realtime speeds pick
//! from the quantiser (`vp9_pick_filter_level`), the frame written
//! (`bitstream`) and the rate control told what it cost.
//!
//! Every frame is a key frame for now, and the block decisions are fixed
//! ones; libvpx's mode decisions and inter frames come next. What a decoder
//! makes of each frame is exactly [`Encoder::reconstruction`]: the tests
//! decode every frame written and compare.
//!
//! Translated in part into Rust from libvpx v1.17.0's `vp9/vp9_cx_iface.c`,
//! `vp9/encoder/vp9_encoder.c`, `vp9_picklpf.c`, `vp9_extend.c`,
//! `vp9_segmentation.c` and `vp9/common/vp9_entropymode.c` (copyright the
//! WebM project authors), used under libvpx's BSD licence and patent grant
//! (`licenses/libvpx-LICENSE`, `licenses/libvpx-PATENTS`).

#![allow(
    clippy::arithmetic_side_effects,
    reason = "timestamps are frame counts times the timebase's ticks, far inside i64; the raw-rate cap is computed in f64 and the plane checks bound every size by the frame (at most 65536 a side)"
)]

use std::sync::Arc;

use crate::Error;
use crate::common::{ALLOW_16X16, ONLY_4X4, TX_MODE_SELECT};
use crate::decoder::{Picture, PlaneView};
use crate::enc::bitstream::{self, CoefUpdates, FrameHeader, KeyFrame, UpdateSearch};
use crate::enc::cpi::{Cpi, Oxcf, quantizer_to_qindex};
use crate::enc::encodeframe::{Decide, FixedDecisions, FrameEncoder};
use crate::enc::tokenize;
use crate::frame::{AnyFrame, Buffers, FrameBuf};
use crate::header::{self, Quantization};
use crate::loopfilter;
use crate::probs::{self, FrameContext};

/// What to encode and how: libvpx's `vpx_codec_enc_cfg_t`, the fields its
/// realtime constant-bitrate mode reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EncoderConfig {
    /// The pictures' size in pixels, 1 to 65536 each way.
    pub width: u32,
    pub height: u32,
    /// Frames per second, as numerator and denominator.
    pub fps: (u32, u32),
    /// The bitrate to hold, in kilobits per second.
    pub bitrate_kbps: u32,
    /// The quantiser's bounds on libvpx's 0-to-63 scale (`vpxenc`'s
    /// `--min-q` and `--max-q`). Both 0 is lossless.
    pub min_quantizer: u8,
    pub max_quantizer: u8,
    /// How far, in percent, a frame's target may move below and above the
    /// average to steer the buffer back to its optimum.
    pub undershoot_pct: u8,
    pub overshoot_pct: u8,
    /// The decoder's buffer, in milliseconds of the bitrate: its size, how
    /// full it starts, and how full the rate control keeps it.
    pub buffer_ms: u32,
    pub buffer_initial_ms: u32,
    pub buffer_optimal_ms: u32,
    /// The most frames between key frames.
    pub keyframe_max_distance: u32,
}

impl EncoderConfig {
    /// libvpx's realtime settings, as the reference encode the port is
    /// checked against gives them to `vpxenc` (`--rt --cpu-used=8
    /// --end-usage=cbr --min-q=2 --max-q=52 --undershoot-pct=50
    /// --overshoot-pct=50 --buf-sz=1000 --buf-initial-sz=500
    /// --buf-optimal-sz=600 --kf-max-dist=9999`), at 30 frames a second.
    #[must_use]
    pub fn realtime(width: u32, height: u32, bitrate_kbps: u32) -> Self {
        Self {
            width,
            height,
            fps: (30, 1),
            bitrate_kbps,
            min_quantizer: 2,
            max_quantizer: 52,
            undershoot_pct: 50,
            overshoot_pct: 50,
            buffer_ms: 1000,
            buffer_initial_ms: 500,
            buffer_optimal_ms: 600,
            keyframe_max_distance: 9999,
        }
    }

    /// The configuration as libvpx keeps it: `vp9_cx_iface.c`'s
    /// `set_encoder_config` for `vpxenc`'s realtime options.
    pub(crate) fn oxcf(&self) -> Oxcf {
        let (fps_num, fps_den) = self.fps;
        // The timebase is one frame: libvpx's g_timebase = 1 / fps.
        let mut init_framerate = f64::from(fps_num) / f64::from(fps_den);
        if init_framerate > 180.0 {
            init_framerate = 30.0;
        }
        // Capped at the raw rate or 1000 Mbit/s, whichever is less.
        let raw_target_rate =
            (f64::from(self.width) * f64::from(self.height) * 8.0 * 3.0 * init_framerate / 1000.0)
                as u64;
        let kbps = u64::from(self.bitrate_kbps)
            .min(raw_target_rate)
            .min(1_000_000);
        Oxcf {
            width: self.width,
            height: self.height,
            init_framerate,
            timebase: (fps_den, fps_num),
            target_bandwidth: 1000 * i64::try_from(kbps).unwrap_or(1_000_000),
            starting_buffer_level_ms: i64::from(self.buffer_initial_ms),
            optimal_buffer_level_ms: i64::from(self.buffer_optimal_ms),
            maximum_buffer_size_ms: i64::from(self.buffer_ms),
            best_allowed_q: quantizer_to_qindex(self.min_quantizer),
            worst_allowed_q: quantizer_to_qindex(self.max_quantizer),
            under_shoot_pct: i32::from(self.undershoot_pct),
            over_shoot_pct: i32::from(self.overshoot_pct),
            rc_max_intra_bitrate_pct: 0,
            rc_max_inter_bitrate_pct: 0,
            gf_cbr_boost_pct: 0,
            key_freq: i32::try_from(self.keyframe_max_distance).unwrap_or(i32::MAX),
            // libvpx's VPX_KF_AUTO with a minimum distance of 0.
            auto_key: self.keyframe_max_distance != 0,
            two_pass_vbrmin_section: 0,
            two_pass_vbrmax_section: 2000,
            min_gf_interval: 0,
            max_gf_interval: 0,
            speed: 8,
            cyclic_refresh: true,
            screen_content: false,
            drop_frames_water_mark: 0,
            frame_parallel_decoding_mode: true,
            error_resilient_mode: false,
            tile_columns: 0,
            tile_rows: 0,
        }
    }
}

/// How a frame is coded, beyond what the configuration says: for tests that
/// reach parts of the stream the encoder's own decisions do not yet use.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct FrameOptions {
    /// Whether blocks choose their own transform size (`TX_MODE_SELECT`).
    pub tx_select: bool,
}

/// A VP9 encoder: 8-bit 4:2:0 pictures in, one compressed frame each out,
/// at the bitrate the configuration asks for.
///
/// ```
/// let (w, h) = (64u32, 48u32);
/// let y = vec![128u8; (w * h) as usize];
/// let uv = vec![128u8; ((w / 2) * (h / 2)) as usize];
/// let mut encoder = vp9::Encoder::new(vp9::EncoderConfig::realtime(w, h, 200))?;
/// let frame = encoder.encode([
///     vp9::PlaneView { data: &y, stride: w as usize, width: w as usize, height: h as usize },
///     vp9::PlaneView { data: &uv, stride: (w / 2) as usize, width: (w / 2) as usize, height: (h / 2) as usize },
///     vp9::PlaneView { data: &uv, stride: (w / 2) as usize, width: (w / 2) as usize, height: (h / 2) as usize },
/// ])?;
/// let mut decoder = vp9::Decoder::new();
/// assert!(decoder.decode(&frame)?.is_some());
/// # Ok::<(), vp9::Error>(())
/// ```
#[derive(Debug)]
pub struct Encoder {
    cpi: Cpi,
    /// The frames encoded so far: the next one's timestamp, in frames.
    frames: i64,
    /// The last frame's reconstruction: what a decoder shows for it.
    last: Option<Arc<AnyFrame>>,
    /// The loop filter's buffers, kept from frame to frame.
    scratch: Buffers<u8>,
    threads: usize,
    /// What the last frame counted, folded as the decoder counts.
    #[cfg(test)]
    last_counts: crate::probs::Counts,
}

impl Encoder {
    /// An encoder for `config`.
    ///
    /// # Errors
    ///
    /// [`Error::Unsupported`] if a dimension is 0 or above 65536, the frame
    /// rate has a zero term, the bitrate is zero, or the quantiser bounds
    /// are out of order or past 63.
    pub fn new(config: EncoderConfig) -> Result<Self, Error> {
        let max = crate::frame::MAX_DIMENSION;
        if config.width == 0 || config.height == 0 || config.width > max || config.height > max {
            return Err(Error::Unsupported("a frame dimension is out of range"));
        }
        if config.fps.0 == 0 || config.fps.1 == 0 {
            return Err(Error::Unsupported("the frame rate has a zero term"));
        }
        if config.bitrate_kbps == 0 {
            return Err(Error::Unsupported("the bitrate is zero"));
        }
        if config.min_quantizer > config.max_quantizer || config.max_quantizer > 63 {
            return Err(Error::Unsupported("the quantiser bounds are out of range"));
        }
        Ok(Self::with_oxcf(config.oxcf()))
    }

    /// An encoder for a configuration as libvpx keeps it.
    pub(crate) fn with_oxcf(oxcf: Oxcf) -> Self {
        Self {
            cpi: Cpi::new(oxcf),
            frames: 0,
            last: None,
            scratch: Buffers::default(),
            threads: std::thread::available_parallelism().map_or(1, usize::from),
            #[cfg(test)]
            last_counts: crate::probs::Counts::default(),
        }
    }

    /// What a decoder shows for the last frame encoded, if any.
    #[must_use]
    pub fn reconstruction(&self) -> Option<Picture> {
        self.last.clone().map(Picture::from_frame)
    }

    /// Encode the next picture: its Y, U and V planes, chroma at half the
    /// width and height (rounded up). Pictures are a frame apart at the
    /// configured rate. Returns the compressed frame, which a
    /// [`crate::Decoder`] decodes to [`Encoder::reconstruction`].
    ///
    /// # Errors
    ///
    /// [`Error::Unsupported`] if a plane is not the configured size, or its
    /// data is too short for its stride.
    pub fn encode(&mut self, planes: [PlaneView<'_, u8>; 3]) -> Result<Vec<u8>, Error> {
        self.encode_frame(planes, &mut FixedDecisions, FrameOptions::default())
    }

    /// [`Encoder::encode`] with the block decisions `d` makes, coded as
    /// `opts` says.
    pub(crate) fn encode_frame(
        &mut self,
        planes: [PlaneView<'_, u8>; 3],
        d: &mut dyn Decide,
        opts: FrameOptions,
    ) -> Result<Vec<u8>, Error> {
        let (width, height) = (self.cpi.oxcf.width, self.cpi.oxcf.height);
        let src = copy_and_extend(&planes, width, height)?;
        let mut recon = FrameBuf::<u8>::new(width, height, 1, 1, 8)?;

        // encoder_encode: the frame's start and end in 1/10,000,000 s, as
        // libvpx converts a timestamp from the timebase.
        let (tb_num, tb_den) = self.cpi.oxcf.timebase;
        let ticks = |n: i64| -> i64 {
            let num = i64::from(tb_num) * 10_000_000;
            let den = i64::from(tb_den);
            let g = gcd(num, den).max(1);
            n.saturating_mul(num / g) / (den / g).max(1)
        };
        let (ts_start, ts_end) = (ticks(self.frames), ticks(self.frames + 1));
        self.frames += 1;

        let cpi = &mut self.cpi;
        // vp9_get_compressed_data.
        cpi.common.reset_frame_context = 0;
        cpi.common.refresh_frame_context = true;
        cpi.refresh_last_frame = true;
        cpi.refresh_golden_frame = false;
        cpi.refresh_alt_ref_frame = false;
        cpi.common.show_frame = true;
        cpi.common.intra_only = false;
        cpi.adjust_frame_rate(ts_start, ts_end);
        // Inter frames come later: every frame is a key frame.
        cpi.force_key_frame = true;

        // Pass0Encode.
        cpi.rc_get_one_pass_cbr_params();

        // encode_frame_to_data_rate.
        cpi.common.lf.params.mode_ref_delta_update = false;
        if cpi.common.frame_is_intra_only() {
            // vp9_reset_segment_features.
            let seg = &mut cpi.common.seg;
            seg.enabled = false;
            seg.update_map = false;
            seg.update_data = false;
            seg.tree_probs = [255; 7];
            seg.clear_all_features();
            cpi.rc.source_alt_ref_active = false;
            cpi.common.error_resilient_mode = cpi.oxcf.error_resilient_mode;
            cpi.common.frame_parallel_decoding_mode = cpi.oxcf.frame_parallel_decoding_mode;
            if cpi.common.error_resilient_mode {
                cpi.common.frame_parallel_decoding_mode = true;
                cpi.common.reset_frame_context = 0;
                cpi.common.refresh_frame_context = false;
            }
        }

        // encode_without_recode_loop: the quantiser, the frame set up.
        let (q, _bottom, _top) = cpi.rc_pick_q_and_bounds_one_pass_cbr();
        let q = if cpi.rc.force_max_q {
            cpi.rc.force_max_q = false;
            cpi.rc.worst_quality
        } else {
            q
        };
        // vp9_set_quantizer.
        cpi.common.quant = Quantization {
            base_qindex: q,
            ..Quantization::default()
        };
        cpi.setup_frame();
        if cpi.oxcf.cyclic_refresh {
            cpi.cyclic_refresh_setup();
        }

        // vp9_encode_frame.
        let tx_mode = if cpi.common.quant.lossless() {
            ONLY_4X4
        } else if opts.tx_select {
            TX_MODE_SELECT
        } else {
            // select_tx_mode: a key frame at the realtime speeds.
            ALLOW_16X16
        };
        let h = FrameHeader {
            profile: 0,
            bit_depth: 8,
            color_space: 0,
            full_range: false,
            ss_x: 1,
            ss_y: 1,
            width,
            height,
            render_width: width,
            render_height: height,
            show_frame: cpi.common.show_frame,
            error_resilient_mode: cpi.common.error_resilient_mode,
            refresh_frame_context: cpi.common.refresh_frame_context,
            frame_parallel_decoding_mode: cpi.common.frame_parallel_decoding_mode,
            frame_context_idx: u8::try_from(cpi.common.frame_context_idx).unwrap_or(0),
            quant: cpi.common.quant,
            log2_tile_cols: cpi.common.log2_tile_cols,
            log2_tile_rows: cpi.common.log2_tile_rows,
            tx_mode,
        };
        let mut fc = FrameContext::defaults();
        let mut fe = FrameEncoder::new(&h, &cpi.common.seg, &cpi.quants, &src, &mut recon);
        fe.encode_tiles(d);
        let (mi, mut counts, tile_tokens) = fe.finish();

        // loopfilter_frame.
        if cpi.common.key_frame {
            cpi.refresh_last_frame = true;
        }
        let lf = &mut cpi.common.lf.params;
        lf.sharpness_level = 0;
        lf.filter_level = pick_filter_level_from_q(q, cpi.common.key_frame);
        if lf.filter_level != 0 {
            loopfilter::filter_frame_t(
                &mut recon,
                &mi,
                &lf.levels(&cpi.common.seg),
                &lf.limits(),
                self.threads,
                &mut self.scratch,
            );
        }

        let out = bitstream::pack_key_frame(KeyFrame {
            header: h,
            lf: &mut cpi.common.lf,
            seg: &cpi.common.seg,
            fc: &mut fc,
            counts: &counts,
            // libvpx's realtime speeds on a key frame.
            search: UpdateSearch {
                coef_updates: CoefUpdates::TwoLoop,
                coef_step: 4,
                tx_8x8_only: false,
            },
            mi: &mi,
            tile_tokens: &tile_tokens,
        });

        // What the frame leaves for the next: adapted probabilities (unless
        // frame-parallel), the saved context, the rate control's account.
        tokenize::model_counts(&counts.coef, &mut counts.counts.coef);
        let cm = &mut cpi.common;
        if !cm.error_resilient_mode && !cm.frame_parallel_decoding_mode {
            let pre = cm
                .frame_contexts
                .get(cm.frame_context_idx)
                .cloned()
                .unwrap_or_else(FrameContext::defaults);
            probs::adapt_coef_probs(&mut fc, &pre, &counts.counts, true, false);
        }
        cm.last_key_frame = cm.key_frame;
        cpi.rc_postencode_update(u64::try_from(out.len()).unwrap_or(u64::MAX));
        let cm = &mut cpi.common;
        cm.seg.update_map = false;
        cm.seg.update_data = false;
        cm.lf.params.mode_ref_delta_update = false;
        if cm.show_frame {
            cm.current_video_frame = cm.current_video_frame.wrapping_add(1);
        }
        if cm.refresh_frame_context
            && let Some(slot) = cm.frame_contexts.get_mut(cm.frame_context_idx)
        {
            *slot = fc;
        }
        #[cfg(test)]
        {
            self.last_counts = counts.counts.clone();
        }
        self.last = Some(Arc::new(AnyFrame::Eight(recon)));
        Ok(out)
    }

    /// The quantiser index the last frame was coded at.
    #[cfg(test)]
    pub(crate) fn last_qindex(&self) -> i32 {
        self.cpi.common.quant.base_qindex
    }
}

/// The greatest common divisor, for reducing the timebase as libvpx's
/// `reduce_ratio` does.
fn gcd(a: i64, b: i64) -> i64 {
    let (mut a, mut b) = (a.abs(), b.abs());
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

impl Cpi {
    /// libvpx's `setup_frame`: a key frame (or an error-resilient one)
    /// forgets everything earlier frames set up; another restores the
    /// context it codes with.
    fn setup_frame(&mut self) {
        if self.common.frame_is_intra_only() || self.common.error_resilient_mode {
            self.setup_past_independence();
        }
        if self.common.key_frame {
            self.refresh_golden_frame = true;
            self.refresh_alt_ref_frame = true;
        }
    }

    /// libvpx's `vp9_setup_past_independence`: segmentation features and
    /// loop filter deltas back to their defaults, every saved probability
    /// context (on a key frame) to the defaults.
    fn setup_past_independence(&mut self) {
        let cm = &mut self.common;
        cm.seg.clear_all_features();
        cm.seg.abs_delta = false;
        cm.lf.last_ref_deltas = [0; 4];
        cm.lf.last_mode_deltas = [0; 2];
        cm.lf.params.set_default_deltas();
        let defaults = FrameContext::defaults();
        if cm.key_frame || cm.error_resilient_mode || cm.reset_frame_context == 3 {
            for c in &mut cm.frame_contexts {
                *c = defaults.clone();
            }
        } else if cm.reset_frame_context == 2
            && let Some(c) = cm.frame_contexts.get_mut(cm.frame_context_idx)
        {
            *c = defaults;
        }
        cm.frame_context_idx = 0;
    }
}

/// The loop filter level libvpx's realtime speeds use: a line fitted to the
/// levels its search finds, from the quantiser's AC step, four lower on key
/// frames. libvpx's `vp9_pick_filter_level` with `LPF_PICK_FROM_Q`, 8-bit,
/// one pass.
#[allow(
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the AC step is at most 1828, so the fit is far inside i32, and it is clamped to 0..=63 before narrowing"
)]
pub(crate) fn pick_filter_level_from_q(base_qindex: i32, key_frame: bool) -> u8 {
    let q = i32::from(header::ac_quant(base_qindex, 0, 8));
    let mut guess = (q * 20723 + 1_015_158 + (1 << 17)) >> 18;
    if key_frame {
        guess -= 4;
    }
    guess.clamp(0, header::MAX_LOOP_FILTER) as u8
}

/// The source picture in a frame buffer, its edges repeated out to whole
/// superblocks: libvpx's `vp9_copy_and_extend_frame`, which the encoder's
/// transforms read past the picture's edge through.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "plane sizes are at most 65536 and are checked against the data before any row is read"
)]
fn copy_and_extend(
    planes: &[PlaneView<'_, u8>; 3],
    width: u32,
    height: u32,
) -> Result<FrameBuf<u8>, Error> {
    let mut f = FrameBuf::<u8>::new(width, height, 1, 1, 8)?;
    for (dst, src) in f.planes.iter_mut().zip(planes) {
        let (w, h) = (dst.crop_width, dst.crop_height);
        if src.width != w || src.height != h || src.stride < w {
            return Err(Error::Unsupported("a plane is not the configured size"));
        }
        let need = (h - 1) * src.stride + w;
        if src.data.len() < need {
            return Err(Error::Unsupported(
                "a plane's data is too short for its stride",
            ));
        }
        let stride = dst.stride;
        for y in 0..dst.alloc_height {
            let row = src.data.get(y.min(h - 1) * src.stride..).unwrap_or(&[]);
            let out = dst
                .data
                .get_mut(y * stride..(y + 1) * stride)
                .unwrap_or(&mut []);
            let (inside, past) = out.split_at_mut(w.min(stride));
            for (d, &s) in inside.iter_mut().zip(row) {
                *d = s;
            }
            let last = inside.last().copied().unwrap_or(0);
            past.fill(last);
        }
    }
    Ok(f)
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        clippy::cast_possible_truncation,
        reason = "a test: a failure should be loud"
    )]

    use super::*;
    use crate::common::{BlockSize, Partition};
    use crate::enc::encodeframe::BlockModes;

    /// A configuration whose quantiser is pinned: both bounds the same.
    fn fixed_q(w: u32, h: u32, quantizer: u8) -> EncoderConfig {
        EncoderConfig {
            min_quantizer: quantizer,
            max_quantizer: quantizer,
            ..EncoderConfig::realtime(w, h, 1000)
        }
    }

    struct Lcg(u32);

    impl Lcg {
        fn next(&mut self) -> u32 {
            self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            self.0 >> 8
        }
    }

    /// Decisions drawn at random: every partition, every intra mode, every
    /// transform size, wherever the stream allows them.
    struct RandomDecisions(Lcg);

    impl Decide for RandomDecisions {
        fn partition(
            &mut self,
            _f: &FrameEncoder<'_>,
            _r: usize,
            _c: usize,
            _b: BlockSize,
        ) -> Partition {
            (self.0.next() % 4) as Partition
        }

        fn modes(
            &mut self,
            _f: &FrameEncoder<'_>,
            _r: usize,
            _c: usize,
            bsize: BlockSize,
        ) -> BlockModes {
            let mut m = || (self.0.next() % 10) as u8;
            let modes = BlockModes {
                mode: m(),
                sub_modes: [m(), m(), m(), m()],
                uv_mode: m(),
                tx_size: (self.0.next() % 4) as u8,
            };
            debug_assert!(bsize <= crate::common::BLOCK_64X64);
            modes
        }
    }

    /// A picture with something of everything in it, in six regions: flat
    /// (which codes to nothing and is skipped), smooth gradients, edges,
    /// noise, and all three together.
    fn picture(w: usize, h: usize, seed: u32) -> [Vec<u8>; 3] {
        let mut rng = Lcg(seed);
        let (cw, ch) = (w.div_ceil(2), h.div_ceil(2));
        let mut plane = |pw: usize, ph: usize| {
            let mut v = vec![0u8; pw * ph];
            for y in 0..ph {
                for x in 0..pw {
                    let gradient = (x * 255 / pw.max(1) + y * 97 / ph.max(1)) as u32;
                    let edge = if (x / 7 + y / 5) % 3 == 0 { 60 } else { 0 };
                    let noise = rng.next() % 24;
                    let value = match (x * 3 / pw.max(1), y * 2 / ph.max(1)) {
                        (0, 0) => gradient + edge + noise,
                        (1, 0) => 77,
                        (2, 0) => gradient,
                        (0, _) => 200,
                        (1, _) => 128 + noise,
                        _ => 90 + edge,
                    };
                    v[y * pw + x] = (value % 256) as u8;
                }
            }
            v
        };
        [plane(w, h), plane(cw, ch), plane(cw, ch)]
    }

    fn views(p: &[Vec<u8>; 3], w: usize, h: usize) -> [PlaneView<'_, u8>; 3] {
        let (cw, ch) = (w.div_ceil(2), h.div_ceil(2));
        [
            PlaneView {
                data: &p[0],
                stride: w,
                width: w,
                height: h,
            },
            PlaneView {
                data: &p[1],
                stride: cw,
                width: cw,
                height: ch,
            },
            PlaneView {
                data: &p[2],
                stride: cw,
                width: cw,
                height: ch,
            },
        ]
    }

    /// Every plane of two pictures, sample for sample.
    fn same(a: &Picture, b: &Picture) -> bool {
        (0..3).all(|i| {
            let (pa, pb) = (a.plane8(i).unwrap(), b.plane8(i).unwrap());
            pa.width == pb.width
                && pa.height == pb.height
                && (0..pa.height).all(|y| {
                    pa.data[y * pa.stride..][..pa.width] == pb.data[y * pb.stride..][..pb.width]
                })
        })
    }

    /// What the decoder makes of each frame is what the encoder
    /// reconstructed: at every size, quantiser and transform mode, with
    /// partitions, modes and transform sizes drawn at random.
    #[test]
    fn the_decoder_sees_what_the_encoder_reconstructed() {
        let sizes = [
            (64, 64),
            (100, 60),
            (17, 9),
            (8, 8),
            (1, 1),
            (130, 70),
            (72, 200),
        ];
        let mut seed = 1;
        for &(w, h) in &sizes {
            for q in [0u8, 1, 10, 30, 63] {
                for tx_select in [false, true] {
                    seed += 1;
                    // Frame-parallel off on every other frame, so the
                    // decoder counts what it reads.
                    let frame_parallel = seed % 2 == 0;
                    let pic = picture(w, h, seed);
                    let mut enc = Encoder::with_oxcf(Oxcf {
                        frame_parallel_decoding_mode: frame_parallel,
                        ..fixed_q(w as u32, h as u32, q).oxcf()
                    });
                    let mut d = RandomDecisions(Lcg(seed * 7919));
                    let opts = FrameOptions { tx_select };
                    let frame = enc.encode_frame(views(&pic, w, h), &mut d, opts).unwrap();
                    let mut dec = crate::Decoder::new();
                    let shown = dec
                        .decode(&frame)
                        .unwrap_or_else(|e| panic!("{w}x{h} q{q} select {tx_select}: {e}"))
                        .unwrap();
                    let recon = enc.reconstruction().unwrap();
                    assert!(same(&shown, &recon), "{w}x{h} q{q} select {tx_select}");
                    if !frame_parallel {
                        // What the encoder counted is what the decoder
                        // counted, wherever a key frame counts.
                        let (e, dc) = (&enc.last_counts, dec.last_counts());
                        let at = format!("{w}x{h} q{q} select {tx_select}");
                        assert_eq!(e.coef, dc.coef, "coefficients, {at}");
                        assert_eq!(e.eob_branch, dc.eob_branch, "end of block, {at}");
                        assert_eq!(e.skip, dc.skip, "skip, {at}");
                        assert_eq!(e.tx, dc.tx, "transform size, {at}");
                    }
                    if q == 0 {
                        // Lossless: the source itself.
                        let y = recon.plane8(0).unwrap();
                        for row in 0..h {
                            assert_eq!(y.data[row * y.stride..][..w], pic[0][row * w..][..w]);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn a_plane_of_the_wrong_size_is_refused() {
        let pic = picture(32, 32, 5);
        let mut enc = Encoder::new(EncoderConfig::realtime(32, 30, 100)).unwrap();
        assert!(enc.encode(views(&pic, 32, 32)).is_err());
        let config = EncoderConfig::realtime(32, 30, 100);
        assert!(Encoder::new(EncoderConfig::realtime(0, 30, 100)).is_err());
        assert!(
            Encoder::new(EncoderConfig {
                fps: (0, 1),
                ..config
            })
            .is_err()
        );
        assert!(
            Encoder::new(EncoderConfig {
                bitrate_kbps: 0,
                ..config
            })
            .is_err()
        );
        let backwards = EncoderConfig {
            min_quantizer: 9,
            max_quantizer: 8,
            ..config
        };
        assert!(Encoder::new(backwards).is_err());
    }

    /// The first frame of libvpx's reference encode -- 1280x720 at 1000
    /// kbit/s, the realtime settings -- is a key frame at quantiser index
    /// 161 with loop filter level 24, whatever the picture: its target is
    /// half the buffer's starting level, and the model's quantiser for that
    /// depends on nothing else.
    #[test]
    fn the_first_frame_gets_libvpxs_quantiser() {
        let (w, h) = (1280usize, 720usize);
        let pic = [
            vec![90u8; w * h],
            vec![128u8; w * h / 4],
            vec![128u8; w * h / 4],
        ];
        let mut enc = Encoder::new(EncoderConfig::realtime(1280, 720, 1000)).unwrap();
        let frame = enc.encode(views(&pic, w, h)).unwrap();
        assert_eq!(enc.last_qindex(), 161);
        assert_eq!(enc.cpi.common.lf.params.filter_level, 24);
        // What the header says.
        let info = crate::peek_stream_info(&frame).unwrap();
        assert_eq!((info.width, info.height), (1280, 720));
        assert!(crate::Decoder::new().decode(&frame).unwrap().is_some());
        // The rate control's state after it, as libvpx leaves it.
        let rc = &enc.cpi.rc;
        assert_eq!(rc.avg_frame_bandwidth, 33_333);
        assert_eq!(rc.this_frame_target, 250_000);
        assert_eq!(rc.last_q[crate::enc::ratectrl::KEY_FRAME], 161);
        assert_eq!(rc.baseline_gf_interval, 40);
        assert_eq!(rc.frames_till_gf_update_due, 39);
    }

    /// libvpx's fit at its ends: q index 0 (AC step 4) and 255 (1828).
    #[test]
    fn the_filter_level_follows_libvpxs_fit() {
        // (4 * 20723 + 1015158 + 131072) >> 18 = 4.
        assert_eq!(pick_filter_level_from_q(0, false), 4);
        assert_eq!(pick_filter_level_from_q(0, true), 0);
        // (1828 * 20723 + 1015158 + 131072) >> 18 = 148, clamped.
        assert_eq!(pick_filter_level_from_q(255, false), 63);
    }
}
