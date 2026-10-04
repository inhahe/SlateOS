//! The encoder: pictures in, compressed frames out.
//!
//! This is libvpx's realtime frame loop being ported: `vp9_cx_iface.c`'s
//! `encoder_encode` (timestamps), `vp9_encoder.c`'s
//! `vp9_get_compressed_data`, `Pass0Encode`, `encode_frame_to_data_rate`
//! and `encode_without_recode_loop` -- the frame rate tracked from the
//! timestamps, the rate control's target and quantiser, the cyclic refresh
//! set up, the blocks encoded and reconstructed (`encodeframe`), the
//! reconstruction loop-filtered at the level libvpx's realtime speeds pick
//! from the quantiser (`vp9_pick_filter_level`) and its edges extended for
//! the next frame's prediction, the frame written (`bitstream`), the
//! reference slots refreshed and the rate control told what it cost.
//!
//! Key frames take libvpx's own decisions. Inter frames are coded -- the
//! references, their vectors and the inter frame's headers -- but their
//! decisions are still a caller's (the tests'); until the encoder makes its
//! own, [`Encoder::encode`] codes every frame as a key frame. What a decoder
//! makes of each frame is exactly [`Encoder::reconstruction`]: the tests
//! decode every frame written and compare.
//!
//! Translated in part into Rust from libvpx v1.17.0's `vp9/vp9_cx_iface.c`,
//! `vp9/encoder/vp9_encoder.c`, `vp9_picklpf.c`, `vp9_extend.c`,
//! `vp9_segmentation.c`, `vpx_scale/generic/yv12extend.c` and
//! `vp9/common/vp9_entropymode.c` (copyright the WebM project authors), used
//! under libvpx's BSD licence and patent grant (`licenses/libvpx-LICENSE`,
//! `licenses/libvpx-PATENTS`).

#![allow(
    clippy::arithmetic_side_effects,
    reason = "timestamps are frame counts times the timebase's ticks, far inside i64; the raw-rate cap is computed in f64 and the plane checks bound every size by the frame (at most 65536 a side)"
)]

use std::sync::Arc;

use crate::Error;
use crate::block::MvRef;
use crate::common::{
    ALLOW_16X16, MAX_REF_FRAMES, ONLY_4X4, REF_FRAMES, SWITCHABLE, TX_MODE_SELECT, TxMode,
};
use crate::decoder::{Picture, PlaneView};
use crate::enc::bitstream::{self, CoefUpdates, Frame, FrameHeader, UpdateSearch};
use crate::enc::cpi::{Cpi, Oxcf, quantizer_to_qindex};
use crate::enc::encodeframe::{Decide, FrameEncoder, InterFrame};
use crate::enc::nonrd::RtDecisions;
use crate::enc::rd::{RdFrame, compute_rd_mult};
use crate::enc::tokenize;
use crate::frame::{AnyFrame, Buffers, FrameBuf, Pixel};
use crate::header::{self, Quantization, Segmentation};
use crate::loopfilter;
use crate::probs::{self, FrameContext};

/// libvpx's `HIGH_PRECISION_MV_QTHRESH`: inter frames below this quantiser
/// index may code eighth-pixel vectors.
const HIGH_PRECISION_MV_QTHRESH: i32 = 200;

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

/// The frame-level choices to code a frame with, instead of the rate
/// control's and the cyclic refresh's: a test's, or libvpx's own for a frame
/// being replayed.
#[derive(Clone, Copy, Debug)]
pub(crate) struct FixedFrame {
    pub base_qindex: i32,
    pub filter_level: u8,
    /// Whether segmentation is on, and its features: the map's coding is
    /// chosen when the frame is written.
    pub seg: Segmentation,
}

/// How a frame is coded, beyond what the configuration says: for tests that
/// reach parts of the stream the encoder's own decisions do not yet use.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct FrameOptions {
    /// The transform mode, instead of libvpx's realtime choice.
    pub tx_mode: Option<TxMode>,
    /// Let the rate control choose inter frames, rather than coding every
    /// frame as a key frame: only a caller that decides inter blocks may.
    pub inter: bool,
    pub fixed: Option<FixedFrame>,
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
    /// What each of the eight reference slots holds: libvpx's
    /// `ref_frame_map`. Reconstructions, loop-filtered, their edges
    /// extended.
    ref_frame_map: [Option<Arc<AnyFrame>>; REF_FRAMES],
    /// The last frame's reconstruction: what a decoder shows for it.
    last: Option<Arc<AnyFrame>>,
    /// The vectors the last frame left, per 8x8 cell: libvpx's
    /// `prev_frame->mvs`.
    prev_mvs: Vec<MvRef>,
    /// The segment map an inter frame's may be predicted from: libvpx's
    /// `last_frame_seg_map`.
    last_seg_map: Vec<u8>,
    /// The loop filter's buffers, kept from frame to frame.
    scratch: Buffers<u8>,
    threads: usize,
    /// What the last frame counted, folded as the decoder counts.
    #[cfg(test)]
    last_counts: crate::probs::Counts,
    /// The last frame's blocks.
    #[cfg(test)]
    last_mi: Option<crate::block::MiGrid>,
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
        let cpi = Cpi::new(oxcf);
        let cells = cpi.common.mi_rows * cpi.common.mi_cols;
        Self {
            cpi,
            frames: 0,
            ref_frame_map: Default::default(),
            last: None,
            prev_mvs: vec![MvRef::default(); cells],
            last_seg_map: vec![0; cells],
            scratch: Buffers::default(),
            threads: std::thread::available_parallelism().map_or(1, usize::from),
            #[cfg(test)]
            last_counts: crate::probs::Counts::default(),
            #[cfg(test)]
            last_mi: None,
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
        self.encode_frame(planes, None, FrameOptions::default())
    }

    /// [`Encoder::encode`] with the block decisions `d` makes, or libvpx's
    /// realtime ones, coded as `opts` says.
    #[allow(
        clippy::too_many_lines,
        reason = "libvpx's encode_frame_to_data_rate, kept in its order so it can be read against it"
    )]
    pub(crate) fn encode_frame(
        &mut self,
        planes: [PlaneView<'_, u8>; 3],
        d: Option<&mut dyn Decide>,
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
        // Until the encoder decides inter blocks itself, a frame is a key
        // frame unless the caller decides them.
        cpi.force_key_frame = !opts.inter;

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
            // (libvpx also clears its alt-ref bookkeeping here, which only a
            // lagged encode -- one with alt-ref frames -- reads.)
            cpi.common.error_resilient_mode = cpi.oxcf.error_resilient_mode;
            cpi.common.frame_parallel_decoding_mode = cpi.oxcf.frame_parallel_decoding_mode;
            if cpi.common.error_resilient_mode {
                cpi.common.frame_parallel_decoding_mode = true;
                cpi.common.reset_frame_context = 0;
                cpi.common.refresh_frame_context = false;
            }
        }
        let key_frame = cpi.common.key_frame;
        let intra_only = cpi.common.frame_is_intra_only();

        // encode_without_recode_loop: the quantiser, the frame set up.
        let (q, _bottom, _top) = cpi.rc_pick_q_and_bounds_one_pass_cbr();
        let q = if cpi.rc.force_max_q {
            cpi.rc.force_max_q = false;
            cpi.rc.worst_quality
        } else {
            q
        };
        let q = opts.fixed.map_or(q, |f| f.base_qindex.clamp(0, 255));
        // vp9_set_high_precision_mv: always at the frame's start, then by
        // the quantiser on an inter frame (set_size_dependent_vars).
        cpi.common.allow_high_precision_mv = intra_only || q < HIGH_PRECISION_MV_QTHRESH;
        // vp9_set_quantizer.
        cpi.common.quant = Quantization {
            base_qindex: q,
            ..Quantization::default()
        };
        cpi.setup_frame();
        if let Some(f) = opts.fixed {
            let seg = &mut cpi.common.seg;
            *seg = Segmentation {
                tree_probs: [255; 7],
                pred_probs: [255; 3],
                ..f.seg
            };
            if intra_only {
                seg.temporal_update = false;
            }
        } else if cpi.oxcf.cyclic_refresh {
            cpi.cyclic_refresh_setup();
        }

        // vp9_encode_frame: the transform mode (select_tx_mode), the
        // frame's filter (restore_encode_params' default, SWITCHABLE at the
        // realtime speeds) and its references.
        let tx_mode = if cpi.common.quant.lossless() {
            ONLY_4X4
        } else if let Some(t) = opts.tx_mode {
            t
        } else if key_frame {
            ALLOW_16X16
        } else {
            TX_MODE_SELECT
        };
        let (lst, gld, alt) = (cpi.lst_fb_idx, cpi.gld_fb_idx, cpi.alt_fb_idx);
        let refs: [Option<Arc<AnyFrame>>; 3] =
            [lst, gld, alt].map(|i| self.ref_frame_map.get(i).cloned().flatten());
        let same_size = |r: &Option<Arc<AnyFrame>>| {
            r.as_ref()
                .is_some_and(|f| f.width() == width && f.height() == height)
        };
        let cm = &cpi.common;
        let mut h = FrameHeader {
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
            key_frame,
            show_frame: cm.show_frame,
            error_resilient_mode: cm.error_resilient_mode,
            reset_frame_context: cm.reset_frame_context,
            refresh_frame_context: cm.refresh_frame_context,
            frame_parallel_decoding_mode: cm.frame_parallel_decoding_mode,
            frame_context_idx: u8::try_from(cm.frame_context_idx).unwrap_or(0),
            refresh_frame_flags: cpi.refresh_mask(),
            ref_slots: [lst, gld, alt].map(|i| u8::try_from(i).unwrap_or(0)),
            ref_sign_bias: [false; 3],
            ref_same_size: [
                same_size(&refs[0]),
                same_size(&refs[1]),
                same_size(&refs[2]),
            ],
            allow_high_precision_mv: cm.allow_high_precision_mv,
            interp_filter: SWITCHABLE,
            quant: cm.quant,
            log2_tile_cols: cm.log2_tile_cols,
            log2_tile_rows: cm.log2_tile_rows,
            tx_mode,
        };
        // The probabilities the frame codes with: setup_frame's cm->fc, the
        // defaults on a key frame.
        let mut fc = cm
            .frame_contexts
            .get(cm.frame_context_idx)
            .cloned()
            .unwrap_or_else(FrameContext::defaults);
        // vp9_initialize_rd_consts and the variance partitioning's
        // thresholds, for the realtime decisions.
        let mut rt = RtDecisions::key_frame(
            compute_rd_mult(q + cm.quant.y_dc_delta_q, RdFrame::Key),
            i32::from(cpi.quants.get(0, usize::try_from(q).unwrap_or(0)).dequant[1]),
        );
        let d: &mut dyn Decide = match d {
            Some(d) => d,
            None => &mut rt,
        };
        let costing = fc.clone();
        let ref_bufs: [Option<&FrameBuf<u8>>; 3] = [0, 1, 2].map(|i| {
            refs.get(i)
                .and_then(Option::as_deref)
                .and_then(<u8 as Pixel>::from_any)
        });
        // encode_frame_internal's use_prev_frame_mvs.
        let use_prev_frame_mvs = !cm.error_resilient_mode
            && width == cpi.last_width
            && height == cpi.last_height
            && !cm.intra_only
            && cpi.last_show_frame;
        let inter = (!intra_only).then(|| InterFrame {
            refs: ref_bufs,
            prev_mvs: use_prev_frame_mvs.then_some(self.prev_mvs.as_slice()),
            sign_bias: [false; MAX_REF_FRAMES],
            allow_hp: h.allow_high_precision_mv,
            interp_filter: SWITCHABLE,
        });
        let mut fe = FrameEncoder::new(&h, &costing, &cm.seg, &cpi.quants, &src, &mut recon, inter);
        fe.encode_tiles(d);
        let mut encoded = fe.finish();
        // fix_interp_filter: a frame of one filter codes it once.
        h.interp_filter = bitstream::fix_interp_filter(SWITCHABLE, &encoded.counts.counts);

        // loopfilter_frame: the level, the filter, and the edges extended
        // for the next frame's prediction.
        let cm = &mut cpi.common;
        let lf = &mut cm.lf.params;
        lf.sharpness_level = 0;
        lf.filter_level = opts.fixed.map_or_else(
            || {
                // The cyclic refresh's lower quantisers want a lighter
                // filter (CBR, aq-mode 3, not screen content).
                let refreshing = cpi.oxcf.cyclic_refresh
                    && cm.seg.enabled
                    && !key_frame
                    && !cpi.oxcf.screen_content
                    && (q < 200 || u64::from(width) * u64::from(height) > 320 * 240);
                pick_filter_level_from_q(q, key_frame, refreshing)
            },
            |f| f.filter_level,
        );
        if lf.filter_level != 0 {
            loopfilter::filter_frame_t(
                &mut recon,
                &encoded.mi,
                &lf.levels(&cm.seg),
                &lf.limits(),
                self.threads,
                &mut self.scratch,
            );
        }
        extend_frame(&mut recon);

        let search = if key_frame {
            UpdateSearch {
                coef_updates: CoefUpdates::TwoLoop,
                coef_step: 4,
                tx_8x8_only: false,
            }
        } else {
            // The realtime speeds' inter frames: one pass over the updates,
            // and the transform search tries 8x8 and smaller only.
            UpdateSearch {
                coef_updates: CoefUpdates::OneLoopReduced,
                coef_step: 4,
                tx_8x8_only: true,
            }
        };
        let out = bitstream::pack_frame(Frame {
            header: h,
            lf: &mut cm.lf,
            seg: &mut cm.seg,
            fc: &mut fc,
            counts: &encoded.counts,
            search,
            mi: &mut encoded.mi,
            ext: &encoded.ext,
            last_seg_map: &self.last_seg_map,
            tile_tokens: &encoded.tile_tokens,
        });

        // update_reference_segmentation_map.
        if cm.seg.update_map {
            let cols = cm.mi_cols;
            for (i, cell) in self.last_seg_map.iter_mut().enumerate() {
                *cell = encoded
                    .mi
                    .at(i / cols.max(1), i % cols.max(1))
                    .map_or(0, |b| b.segment_id);
            }
        }
        // vp9_update_reference_frames.
        let frame = Arc::new(AnyFrame::Eight(recon));
        let mut refresh = |slot: usize| {
            if let Some(s) = self.ref_frame_map.get_mut(slot) {
                *s = Some(Arc::clone(&frame));
            }
        };
        if key_frame {
            refresh(gld);
            refresh(alt);
        } else {
            if cpi.refresh_alt_ref_frame {
                refresh(alt);
            }
            if cpi.refresh_golden_frame {
                refresh(gld);
            }
        }
        if cpi.refresh_last_frame {
            refresh(lst);
        }

        // What the frame leaves for the next: adapted probabilities (unless
        // frame-parallel), the saved context, the rate control's account.
        let counts = &mut encoded.counts;
        tokenize::model_counts(&counts.coef, &mut counts.counts.coef);
        let cm = &mut cpi.common;
        if !cm.error_resilient_mode && !cm.frame_parallel_decoding_mode {
            let pre = cm
                .frame_contexts
                .get(cm.frame_context_idx)
                .cloned()
                .unwrap_or_else(FrameContext::defaults);
            probs::adapt_coef_probs(&mut fc, &pre, &counts.counts, intra_only, cm.last_key_frame);
            if !intra_only {
                probs::adapt_mode_probs(
                    &mut fc,
                    &pre,
                    &counts.counts,
                    h.interp_filter == SWITCHABLE,
                    tx_mode == TX_MODE_SELECT,
                );
                probs::adapt_mv_probs(&mut fc, &pre, &counts.counts, h.allow_high_precision_mv);
            }
        }
        cm.last_key_frame = key_frame;
        cpi.rc_postencode_update(u64::try_from(out.len()).unwrap_or(u64::MAX));
        let cm = &mut cpi.common;
        cm.seg.update_map = false;
        cm.seg.update_data = false;
        cm.lf.params.mode_ref_delta_update = false;
        cpi.last_width = width;
        cpi.last_height = height;
        cpi.last_show_frame = cm.show_frame;
        self.prev_mvs = encoded.mvs;
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
            self.last_counts = encoded.counts.counts.clone();
            self.last_mi = Some(encoded.mi.clone());
        }
        self.last = Some(frame);
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
    /// forgets everything earlier frames set up; another codes with the
    /// context it saved (the realtime encoder's: context 0, alt-ref frames
    /// aside).
    fn setup_frame(&mut self) {
        if self.common.frame_is_intra_only() || self.common.error_resilient_mode {
            self.setup_past_independence();
        } else {
            self.common.frame_context_idx = usize::from(self.refresh_alt_ref_frame);
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

    /// Which slots the frame refreshes: libvpx's `vp9_get_refresh_mask`, one
    /// pass with no alt-ref.
    fn refresh_mask(&self) -> u8 {
        let bit = |on: bool, slot: usize| if on { 1u8 << (slot & 7) } else { 0 };
        bit(self.refresh_last_frame, self.lst_fb_idx)
            | bit(self.refresh_golden_frame, self.gld_fb_idx)
            | bit(self.refresh_alt_ref_frame, self.alt_fb_idx)
    }
}

/// The loop filter level libvpx's realtime speeds use: a line fitted to the
/// levels its search finds, from the quantiser's AC step; five eighths of
/// it where the cyclic refresh is coding blocks at lower quantisers
/// (`refreshing`), four lower on key frames. libvpx's
/// `vp9_pick_filter_level` with `LPF_PICK_FROM_Q`, 8-bit, one pass.
#[allow(
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the AC step is at most 1828, so the fit is far inside i32, and it is clamped to 0..=63 before narrowing"
)]
pub(crate) fn pick_filter_level_from_q(base_qindex: i32, key_frame: bool, refreshing: bool) -> u8 {
    let q = i32::from(header::ac_quant(base_qindex, 0, 8));
    let mut guess = (q * 20723 + 1_015_158 + (1 << 17)) >> 18;
    if refreshing {
        guess = (5 * guess) >> 3;
    }
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

/// Repeat a reconstruction's edge pixels out over the rest of its buffer:
/// libvpx's `vpx_extend_frame_inner_borders` after the loop filter, from
/// the picture's own (cropped) edges. A block reaching past the picture
/// predicts from these, in libvpx's encoder as in this one; a decoder makes
/// different pixels there, which no shown pixel depends on.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "plane sizes are at most 65536, and every row is taken with get"
)]
fn extend_frame(f: &mut FrameBuf<u8>) {
    for p in &mut f.planes {
        let (w, h, stride) = (p.crop_width, p.crop_height, p.stride);
        if w == 0 || h == 0 {
            continue;
        }
        for y in 0..h {
            if let Some(row) = p.data.get_mut(y * stride..(y + 1) * stride) {
                let edge = row.get(w - 1).copied().unwrap_or(0);
                if let Some(past) = row.get_mut(w..) {
                    past.fill(edge);
                }
            }
        }
        let (above, below) = p.data.split_at_mut(h * stride);
        if let Some(last) = above.get((h - 1) * stride..) {
            for row in below.chunks_exact_mut(stride) {
                row.copy_from_slice(last);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_possible_wrap,
        reason = "a test: a failure should be loud"
    )]

    use super::*;
    use crate::block::ModeInfo;
    use crate::common::{
        ALLOW_8X8, ALLOW_32X32, ALTREF_FRAME, BlockSize, GOLDEN_FRAME, LAST_FRAME, Mv, NEARESTMV,
        Partition, SEG_LVL_ALT_LF, SEG_LVL_ALT_Q, SEG_LVL_REF_FRAME, SEG_LVL_SKIP, TX_MODE_SELECT,
    };
    use crate::enc::encodeframe::{BlockModes, InterModes};

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

        fn below(&mut self, n: u32) -> u32 {
            self.next() % n.max(1)
        }
    }

    /// Decisions drawn at random: every partition, every intra mode, every
    /// transform size, wherever the stream allows them -- and on an inter
    /// frame every reference, inter mode, filter, vector and segment, and
    /// blocks skipped whole or in luma.
    struct RandomDecisions {
        rng: Lcg,
        /// How likely a block in an inter frame is inter, out of 8.
        inter_odds: u32,
        /// Segments to choose from: 1 is segmentation off.
        segments: u32,
        /// One filter for the whole frame, so the header codes it once.
        one_filter: Option<u8>,
        /// New vectors at eighth pixels even where their coding cannot
        /// carry them, which the encoder must bring to what it can.
        any_precision: bool,
    }

    impl RandomDecisions {
        fn new(seed: u32) -> Self {
            Self {
                rng: Lcg(seed),
                inter_odds: 6,
                segments: 1,
                one_filter: None,
                any_precision: false,
            }
        }
    }

    impl Decide for RandomDecisions {
        fn partition(
            &mut self,
            _f: &mut FrameEncoder<'_>,
            _r: usize,
            _c: usize,
            _b: BlockSize,
        ) -> Partition {
            self.rng.below(4) as Partition
        }

        fn modes(
            &mut self,
            f: &mut FrameEncoder<'_>,
            mi_row: usize,
            mi_col: usize,
            bsize: BlockSize,
        ) -> BlockModes {
            let rng = &mut self.rng;
            let mut m = || (rng.next() % 10) as u8;
            let mut modes = BlockModes {
                mode: m(),
                sub_modes: [m(), m(), m(), m()],
                uv_mode: m(),
                tx_size: (self.rng.next() % 4) as u8,
                segment_id: self.rng.below(self.segments) as u8,
                ..BlockModes::default()
            };
            if f.inter.is_some() && self.rng.below(8) < self.inter_odds {
                let mut ref_frame =
                    [LAST_FRAME, GOLDEN_FRAME, ALTREF_FRAME][self.rng.below(3) as usize];
                // A segment that names a reference overrides the choice: the
                // vectors are for the one it names.
                let id = modes.segment_id;
                if f.seg.enabled && f.seg.feature_active(id, SEG_LVL_REF_FRAME) {
                    let r = f.seg.data(id, SEG_LVL_REF_FRAME) as i8;
                    if r > 0 {
                        ref_frame = r;
                    }
                }
                let refs = f.mv_refs(mi_row, mi_col, bsize, ref_frame);
                // New vectors near the predicted one, or far from it, at the
                // precision their coding allows (or not, to see the encoder
                // bring them to it).
                let any_precision = self.any_precision;
                let new_mv = |rng: &mut Lcg| {
                    let reach = if rng.below(4) == 0 { 2000 } else { 64 };
                    let step = if refs.usehp || any_precision { 1 } else { 2 };
                    let mut off = || (rng.below(2 * reach + 1) as i32 - reach as i32) * step;
                    let clamp = |v: i32| v.clamp(-16_000, 16_000) as i16;
                    Mv {
                        row: clamp(i32::from(refs.nearest.row) + off()),
                        col: clamp(i32::from(refs.nearest.col) + off()),
                    }
                };
                let mut sub = [(NEARESTMV, Mv::ZERO); 4];
                for s in &mut sub {
                    *s = (NEARESTMV + self.rng.below(4) as u8, new_mv(&mut self.rng));
                }
                modes.inter = Some(InterModes {
                    ref_frame,
                    mode: NEARESTMV + self.rng.below(4) as u8,
                    mv: new_mv(&mut self.rng),
                    interp_filter: self.one_filter.unwrap_or_else(|| self.rng.below(3) as u8),
                    sub,
                });
                modes.skip = self.rng.below(5) == 0;
                modes.skip_y = self.rng.below(4) == 0;
            }
            modes
        }
    }

    /// A picture with something of everything in it, in six regions: flat
    /// (which codes to nothing and is skipped), smooth gradients, edges,
    /// noise, and all three together. `shift` moves the content, as motion
    /// would.
    fn picture_moved(w: usize, h: usize, seed: u32, shift: usize) -> [Vec<u8>; 3] {
        let mut rng = Lcg(seed);
        let (cw, ch) = (w.div_ceil(2), h.div_ceil(2));
        let mut plane = |pw: usize, ph: usize| {
            let mut v = vec![0u8; pw * ph];
            for y in 0..ph {
                for x in 0..pw {
                    let (sx, sy) = (x + shift, y + shift / 2);
                    let gradient = (sx * 255 / pw.max(1) + sy * 97 / ph.max(1)) as u32;
                    let edge = if (sx / 7 + sy / 5) % 3 == 0 { 60 } else { 0 };
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

    fn picture(w: usize, h: usize, seed: u32) -> [Vec<u8>; 3] {
        picture_moved(w, h, seed, 0)
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
                    let mut d = RandomDecisions::new(seed * 7919);
                    let opts = FrameOptions {
                        tx_mode: tx_select.then_some(TX_MODE_SELECT),
                        ..FrameOptions::default()
                    };
                    let frame = enc
                        .encode_frame(views(&pic, w, h), Some(&mut d), opts)
                        .unwrap();
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
                        assert_eq!(e.partition, dc.partition, "partition, {at}");
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

    /// A segmentation with every feature libvpx's syntax has, drawn at
    /// random across `segments` segments.
    fn random_seg(rng: &mut Lcg, segments: u32) -> Segmentation {
        let mut seg = Segmentation {
            enabled: segments > 1,
            update_map: segments > 1,
            update_data: segments > 1,
            ..Segmentation::default()
        };
        if segments <= 1 {
            return seg;
        }
        for id in 0..segments as u8 {
            if rng.below(2) == 0 {
                seg.set_feature(id, SEG_LVL_ALT_Q, rng.below(121) as i32 - 60);
            }
            if rng.below(3) == 0 {
                seg.set_feature(id, SEG_LVL_ALT_LF, rng.below(31) as i32 - 15);
            }
            // A segment that names its reference, or skips every block,
            // is rare in libvpx's own streams and costs a decoder nothing
            // to get wrong unless it is tested.
            let mut inter_ref = false;
            if rng.below(6) == 0 {
                let r = rng.below(4) as i32;
                inter_ref = r > 0;
                seg.set_feature(id, SEG_LVL_REF_FRAME, r);
            }
            // A segment that names an inter reference and skips cannot
            // hold a block below 8x8, which the random partitions make.
            if !inter_ref && rng.below(8) == 0 {
                seg.set_feature(id, SEG_LVL_SKIP, 0);
            }
        }
        seg
    }

    /// Inter frames decode to what the encoder reconstructed, and the
    /// encoder counts what the decoder counts: a key frame then inter
    /// frames of moving pictures, at sizes that cross the superblock and
    /// 8x8 grids, quantisers on both sides of eighth-pixel vectors, every
    /// transform mode, segmentation off and on with every feature, the
    /// filter switchable and fixed -- with every decision drawn at random.
    #[test]
    fn inter_frames_decode_to_the_encoders_reconstruction() {
        let sizes = [(64, 64), (100, 60), (130, 70), (17, 9), (200, 136)];
        let mut seed = 11u32;
        for &(w, h) in &sizes {
            for run in 0..4u32 {
                seed += 1;
                inter_stream(w, h, seed, run, 5);
            }
        }
    }

    /// The same as [`inter_frames_decode_to_the_encoders_reconstruction`],
    /// two hundred streams of eight frames at each size: a soak for the
    /// rarer paths, run by hand.
    #[test]
    #[ignore = "a long soak: cargo test -p vp9 --release --lib inter_streams_soak -- --ignored"]
    fn inter_streams_soak() {
        let sizes = [(64, 64), (100, 60), (130, 70), (17, 9), (200, 136)];
        let mut seed = 11u32;
        for &(w, h) in &sizes {
            for run in 0..200u32 {
                seed += 1;
                inter_stream(w, h, seed, run, 8);
            }
        }
    }

    /// The first cell where two frames' blocks differ, as the encoder and
    /// the decoder hold them.
    fn first_block_difference(e: &crate::block::MiGrid, d: &crate::block::MiGrid) -> String {
        for r in 0..e.mi_rows {
            for c in 0..e.mi_cols {
                let (Some(a), Some(b)) = (e.at(r, c), d.at(r, c)) else {
                    return format!("cell ({r}, {c}) is missing a block");
                };
                // What both sides record: an inter block's vectors (its
                // sub-blocks' modes the decoder does not keep), an intra
                // block's modes.
                let key = |m: &ModeInfo| {
                    let sub = m.sb_type < crate::common::BLOCK_8X8;
                    (
                        (m.sb_type, m.ref_frame[0], m.skip, m.tx_size, m.segment_id),
                        if m.is_inter() {
                            (
                                m.mode,
                                m.mv[0],
                                sub.then(|| m.bmi.map(|b| b.mv[0])),
                                m.interp_filter,
                            )
                        } else {
                            (m.mode, Mv::ZERO, None, 0)
                        },
                        (!m.is_inter()).then(|| (m.uv_mode, sub.then(|| m.bmi.map(|b| b.mode)))),
                    )
                };
                if key(a) != key(b) {
                    return format!(
                        "cell ({r}, {c}): encoder {a:?}
  decoder {b:?}"
                    );
                }
            }
        }
        "the blocks agree".to_string()
    }

    /// The first sample where two pictures differ: its plane and position.
    fn first_sample_difference(a: &Picture, b: &Picture) -> (usize, usize, usize) {
        for i in 0..3 {
            let (pa, pb) = (a.plane8(i).unwrap(), b.plane8(i).unwrap());
            for y in 0..pa.height {
                for x in 0..pa.width {
                    if pa.data[y * pa.stride + x] != pb.data[y * pb.stride + x] {
                        return (i, x, y);
                    }
                }
            }
        }
        (3, 0, 0)
    }

    /// One stream of `frames` frames, a key frame then inter frames of a
    /// moving picture, every frame-level choice and every decision drawn at
    /// random from `seed`: each frame decodes to the encoder's
    /// reconstruction, and (every other run, which is not frame-parallel)
    /// the encoder counts what the decoder counts.
    fn inter_stream(w: usize, h: usize, seed: u32, run: u32, frames: u32) {
        let tx_modes = [None, Some(ONLY_4X4), Some(ALLOW_8X8), Some(ALLOW_32X32)];
        let mut rng = Lcg(seed.wrapping_mul(2_654_435_761));
        let frame_parallel = run % 2 == 1;
        let mut enc = Encoder::with_oxcf(Oxcf {
            frame_parallel_decoding_mode: frame_parallel,
            ..EncoderConfig::realtime(w as u32, h as u32, 1000).oxcf()
        });
        let mut dec = crate::Decoder::new();
        for n in 0..frames {
            let pic = picture_moved(w, h, seed + n, (n * 3) as usize);
            let segments = if n == 0 {
                1
            } else {
                [1, 3, 8][rng.below(3) as usize]
            };
            let seg = random_seg(&mut rng, segments);
            let q = [0, 40, 120, 199, 200, 255][rng.below(6) as usize];
            let fixed = FixedFrame {
                base_qindex: q,
                filter_level: rng.below(64) as u8,
                seg,
            };
            let mut d = RandomDecisions::new(seed * 31 + n);
            d.segments = segments;
            d.inter_odds = rng.below(9);
            d.one_filter = (rng.below(4) == 0).then(|| rng.below(3) as u8);
            d.any_precision = rng.below(4) == 0;
            let opts = FrameOptions {
                tx_mode: tx_modes[rng.below(4) as usize],
                inter: n > 0,
                fixed: Some(fixed),
            };
            let at = format!("{w}x{h} run {run} (seed {seed}) frame {n} q{q} seg {segments}");
            let frame = enc
                .encode_frame(views(&pic, w, h), Some(&mut d), opts)
                .unwrap();
            let shown = dec
                .decode(&frame)
                .unwrap_or_else(|e| {
                    panic!(
                        "{at}: {e} ({} bytes: {:02x?} .. {:02x?})",
                        frame.len(),
                        &frame[..frame.len().min(8)],
                        &frame[frame.len().saturating_sub(8)..]
                    )
                })
                .unwrap();
            let recon = enc.reconstruction().unwrap();
            if !same(&shown, &recon) {
                let (emi, dmi) = (enc.last_mi.as_ref().unwrap(), dec.last_mi().unwrap());
                let (plane, x, y) = first_sample_difference(&recon, &shown);
                let (r, c) = if plane == 0 {
                    (y / 8, x / 8)
                } else {
                    (y / 4, x / 4)
                };
                panic!(
                    "{at}: the pictures differ first in plane {plane} at ({x}, {y}), in cell ({r}, {c}):
  encoder {:?}
  decoder {:?}
 first block difference: {}",
                    emi.at(r, c),
                    dmi.at(r, c),
                    first_block_difference(emi, dmi)
                );
            }
            if !frame_parallel {
                let (e, dc) = (&enc.last_counts, dec.last_counts());
                assert_eq!(e.coef, dc.coef, "coefficients, {at}");
                assert_eq!(e.eob_branch, dc.eob_branch, "end of block, {at}");
                assert_eq!(e.skip, dc.skip, "skip, {at}");
                assert_eq!(e.tx, dc.tx, "transform size, {at}");
                assert_eq!(e.partition, dc.partition, "partition, {at}");
                if n > 0 {
                    assert_eq!(e.y_mode, dc.y_mode, "luma modes, {at}");
                    assert_eq!(e.uv_mode, dc.uv_mode, "chroma modes, {at}");
                    assert_eq!(e.intra_inter, dc.intra_inter, "intra or inter, {at}");
                    assert_eq!(e.single_ref, dc.single_ref, "references, {at}");
                    assert_eq!(e.inter_mode, dc.inter_mode, "inter modes, {at}");
                    assert_eq!(e.mv, dc.mv, "vectors, {at}");
                    // A frame whose blocks used one filter codes it in its
                    // header, and a decoder counts no filters.
                    if bitstream::fix_interp_filter(SWITCHABLE, e) == SWITCHABLE {
                        assert_eq!(e.switchable_interp, dc.switchable_interp, "filters, {at}");
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

    /// libvpx's fit at its ends: q index 0 (AC step 4) and 255 (1828); and
    /// the reference encode's second frame, an inter frame at index 200
    /// with the cyclic refresh on, filtered at 33.
    #[test]
    fn the_filter_level_follows_libvpxs_fit() {
        // (4 * 20723 + 1015158 + 131072) >> 18 = 4.
        assert_eq!(pick_filter_level_from_q(0, false, false), 4);
        assert_eq!(pick_filter_level_from_q(0, true, false), 0);
        // (1828 * 20723 + 1015158 + 131072) >> 18 = 148, clamped.
        assert_eq!(pick_filter_level_from_q(255, false, false), 63);
        assert_eq!(pick_filter_level_from_q(200, false, true), 33);
    }

    /// The edges of a reconstruction repeat out to its buffer's: the last
    /// column to the right, then the last row down.
    #[test]
    fn a_reconstructions_edges_are_extended() {
        let mut f = FrameBuf::<u8>::new(10, 6, 1, 1, 8).unwrap();
        let p = &mut f.planes[0];
        for y in 0..6 {
            for x in 0..10 {
                p.data[y * p.stride + x] = (y * 10 + x) as u8;
            }
        }
        extend_frame(&mut f);
        let p = &f.planes[0];
        assert_eq!(p.data[2 * p.stride + 40], 29, "right of row 2");
        assert_eq!(p.data[30 * p.stride + 3], 53, "below column 3");
        assert_eq!(p.data[30 * p.stride + 40], 59, "the corner");
    }
}
