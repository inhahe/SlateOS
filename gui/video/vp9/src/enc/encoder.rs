//! The encoder: pictures in, compressed frames out.
//!
//! This is libvpx's realtime frame loop: `vp9_cx_iface.c`'s
//! `encoder_encode` (timestamps), `vp9_encoder.c`'s
//! `vp9_get_compressed_data`, `Pass0Encode`, `encode_frame_to_data_rate`
//! and `encode_without_recode_loop`, in libvpx's order -- the frame rate
//! tracked from the timestamps; scene detection and the noise estimate from
//! the source; the rate control's target and quantiser; the cyclic refresh
//! set up; the blocks decided (`nonrd`) and encoded and reconstructed
//! (`encodeframe`); the reconstruction loop-filtered at the level libvpx's
//! realtime speeds pick from the quantiser (`vp9_pick_filter_level`) and its
//! edges extended for the next frame's prediction; the frame written
//! (`bitstream`); the reference slots refreshed and the rate control told
//! what it cost.
//!
//! Every frame takes libvpx's own decisions, as `vpxenc --rt --cpu-used=8`
//! makes them: a key frame first, inter frames after it, `vpxenc`'s byte for
//! byte at every size -- above 352x288 partitioned by variance, at and below
//! it by libvpx's learned search. A picture cut into tile columns
//! ([`EncoderConfig::tile_columns`]) is coded a column to a thread
//! (`encode_columns_threaded`, design-decisions §1342), and the frames are
//! the same on any number of threads. Tests may hand the frame loop
//! decisions of their own instead, which run on one thread. What a decoder
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

use std::sync::{Arc, Mutex, PoisonError};

use crate::Error;
use crate::block::MiGrid;
use crate::block::MvRef;
use crate::common::{
    ALLOW_16X16, MAX_REF_FRAMES, ONLY_4X4, REF_FRAMES, SWITCHABLE, TX_MODE_SELECT, TxMode,
};
use crate::common::{BLOCK_SIZES, LAST_FRAME};
use crate::decoder::{Picture, PlaneView};
use crate::enc::aq_cyclicrefresh::CyclicRefresh;
use crate::enc::bitstream::{self, CoefUpdates, Frame, FrameHeader, UpdateSearch};
use crate::enc::content;
use crate::enc::cpi::{Cpi, Oxcf, quantizer_to_qindex};
use crate::enc::encodeframe::{
    ColumnEncoded, Decide, Encoded, FrameEncoder, InterFrame, column_strip, merge_columns,
    tile_span,
};
use crate::enc::mcomp::LumaRef;
use crate::enc::nonrd::{InterDecisions, InterSettings, RtDecisions, RtState};
use crate::enc::partition::NoiseLevel;
use crate::enc::pickinter::{ALT_FLAG, GOLD_FLAG, LAST_FLAG, SearchFrame};
use crate::enc::quantize::Quants;
use crate::enc::rd::{MAX_MODES, RdFrame, compute_rd_mult};
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
    /// The unit of the pictures' timestamps, in seconds (numerator,
    /// denominator): `vpxenc`'s `--timebase`. Each picture is stamped with
    /// its start in these units, rounded down, as `vpxenc` stamps it -- so a
    /// coarse unit makes the frames' durations uneven, and the rate control
    /// follows them. `vpxenc`'s, given a frame rate, is a millisecond.
    pub timebase: (u32, u32),
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
    /// How many tile columns each picture is cut into, as a power of two:
    /// `vpxenc`'s `--tile-columns` (0 for one column, 1 for two, 2 for
    /// four), held to what the width allows -- a tile column is 256 to 4096
    /// pixels wide, so 1280 pixels make at most four. Tile columns are coded
    /// independently of each other, so the encoder codes them on threads of
    /// their own ([`Encoder::set_threads`]), and the stream is the same on
    /// any number of threads; each costs a little compression, mostly where
    /// a column's first blocks cannot see their left neighbours (0.3% for
    /// four at 1280x720). The encoder's only parallelism: a picture of one
    /// column encodes on one thread.
    pub tile_columns: u8,
    /// The colour space written into every key frame's header, which tells
    /// a decoder how to turn the pictures back into RGB: libvpx's
    /// `vpx_color_space_t` (`vpxenc --color-space`) -- 0 unknown, `vpxenc`'s
    /// default; 1 BT.601; 2 BT.709; 3 SMPTE 170M; 4 SMPTE 240M; 5 BT.2020.
    /// Pictures from [`crate::rgb::argb_to_yuv420`] are BT.601
    /// ([`crate::rgb::COLOR_SPACE`]). Left unknown, a decoder guesses -- and
    /// players guess BT.709 for pictures 1280 wide or more, shifting their
    /// colours. sRGB (7) needs 4:4:4, which this encoder does not code, and 6
    /// is reserved: both are refused.
    pub color_space: u8,
}

impl EncoderConfig {
    /// libvpx's realtime settings, as `vpxenc` takes them for the reference
    /// encodes the port is checked against (`--rt --cpu-used=8
    /// --end-usage=cbr --min-q=2 --max-q=52 --undershoot-pct=50
    /// --overshoot-pct=50 --buf-sz=1000 --buf-initial-sz=500
    /// --buf-optimal-sz=600 --kf-max-dist=9999`), at 30 frames a second, in
    /// as many tile columns as the width allows -- `vpxenc`'s default
    /// (`--tile-columns=6`), so that a picture 512 pixels wide or wider
    /// encodes on several threads.
    #[must_use]
    pub fn realtime(width: u32, height: u32, bitrate_kbps: u32) -> Self {
        Self {
            width,
            height,
            fps: (30, 1),
            timebase: (1, 1000),
            bitrate_kbps,
            min_quantizer: 2,
            max_quantizer: 52,
            undershoot_pct: 50,
            overshoot_pct: 50,
            buffer_ms: 1000,
            buffer_initial_ms: 500,
            buffer_optimal_ms: 600,
            keyframe_max_distance: 9999,
            tile_columns: 6,
            color_space: 0,
        }
    }

    /// The configuration as libvpx keeps it: `vp9_cx_iface.c`'s
    /// `set_encoder_config` for `vpxenc`'s realtime options.
    pub(crate) fn oxcf(&self) -> Oxcf {
        // The frame rate libvpx assumes before it has seen a timestamp:
        // the timebase's units per second, or 30 if that is implausible.
        let (tb_num, tb_den) = self.timebase;
        let mut init_framerate = f64::from(tb_den) / f64::from(tb_num.max(1));
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
            timebase: self.timebase,
            fps: self.fps,
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
            tile_columns: u32::from(self.tile_columns),
            tile_rows: 0,
            color_space: self.color_space,
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
    /// When the picture is shown, from and until, in the timebase's units:
    /// [`Encoder::encode_timed`]'s. `None` is a frame after the last at the
    /// configured rate, as `vpxenc` stamps them.
    pub stamp: Option<(u64, u64)>,
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
    /// When the last picture's showing ended, in libvpx's ten-million-a-
    /// second ticks: the next must end later.
    last_end: Option<i64>,
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
    /// Each reference slot's luma with its edges repeated outward, for the
    /// motion searches.
    ref_luma: [Option<Arc<LumaRef>>; REF_FRAMES],
    /// The last source picture, edges repeated to whole superblocks:
    /// libvpx's `Last_Source`.
    last_src: Option<FrameBuf<u8>>,
    /// The key frame's 8x8 partitioning threshold, which libvpx's inter
    /// frames leave in place (`vbp_thresholds[3]`).
    vbp_threshold_8x8: i64,
    /// The variance partitioning's thresholds as the last frame that set
    /// them left them (`vbp_thresholds`, `vbp_threshold_sad`,
    /// `vbp_threshold_copy`): a frame partitioned by search sets none, and
    /// libvpx's state keeps the old ones.
    frame_vbp: ([i64; 4], i64, i64),
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
    /// rate or the timebase has a zero term, the bitrate is zero, the
    /// quantiser bounds are out of order or past 63, or the colour space is
    /// one this encoder does not code (above 5).
    pub fn new(config: EncoderConfig) -> Result<Self, Error> {
        let max = crate::frame::MAX_DIMENSION;
        if config.width == 0 || config.height == 0 || config.width > max || config.height > max {
            return Err(Error::Unsupported("a frame dimension is out of range"));
        }
        if config.fps.0 == 0 || config.fps.1 == 0 {
            return Err(Error::Unsupported("the frame rate has a zero term"));
        }
        if config.timebase.0 == 0 || config.timebase.1 == 0 {
            return Err(Error::Unsupported("the timebase has a zero term"));
        }
        if config.bitrate_kbps == 0 {
            return Err(Error::Unsupported("the bitrate is zero"));
        }
        if config.min_quantizer > config.max_quantizer || config.max_quantizer > 63 {
            return Err(Error::Unsupported("the quantiser bounds are out of range"));
        }
        if config.color_space > 5 {
            return Err(Error::Unsupported(
                "a colour space this encoder does not code (sRGB needs 4:4:4; 6 is reserved)",
            ));
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
            last_end: None,
            ref_frame_map: Default::default(),
            last: None,
            prev_mvs: vec![MvRef::default(); cells],
            last_seg_map: vec![0; cells],
            ref_luma: Default::default(),
            last_src: None,
            vbp_threshold_8x8: 0,
            frame_vbp: ([0; 4], 0, 0),
            scratch: Buffers::default(),
            threads: std::thread::available_parallelism().map_or(1, usize::from),
            #[cfg(test)]
            last_counts: crate::probs::Counts::default(),
            #[cfg(test)]
            last_mi: None,
        }
    }

    /// Encode on at most `threads` threads (at least one) from the next
    /// picture on. A new encoder uses as many as the machine has cores.
    ///
    /// A frame's tile columns are what encode in parallel
    /// ([`EncoderConfig::tile_columns`]), so a picture of one tile column
    /// encodes on one thread whatever this says. The frames are the same
    /// however many threads make them.
    pub fn set_threads(&mut self, threads: usize) {
        self.threads = threads.max(1);
    }

    /// How many threads the encoder may use.
    #[must_use]
    pub fn threads(&self) -> usize {
        self.threads
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

    /// [`Encoder::encode`] for a picture shown from `start` until `end`, in
    /// units of the configured [`EncoderConfig::timebase`] -- the timestamp
    /// and duration `vpx_codec_encode` takes -- rather than a frame after the
    /// last at the configured rate: for pictures that come when they come, as
    /// a screen capture's do. The rate control follows the frame rate the
    /// times make, sharing the bitrate out by each picture's duration.
    ///
    /// # Errors
    ///
    /// [`Error::Unsupported`] as for [`Encoder::encode`], or if `end` is not
    /// after `start`, or not after the last picture's end.
    pub fn encode_timed(
        &mut self,
        planes: [PlaneView<'_, u8>; 3],
        start: u64,
        end: u64,
    ) -> Result<Vec<u8>, Error> {
        let opts = FrameOptions {
            stamp: Some((start, end)),
            ..FrameOptions::default()
        };
        self.encode_frame(planes, None, opts)
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

        // vpxenc's stamps: each picture's start in the timebase's units,
        // rounded down; then encoder_encode's conversion to 1/10,000,000 s.
        let (tb_num, tb_den) = self.cpi.oxcf.timebase;
        let (fps_num, fps_den) = self.cpi.oxcf.fps;
        let pts = |n: i64| -> i64 {
            (i64::from(tb_den)
                .saturating_mul(n)
                .saturating_mul(i64::from(fps_den))
                / i64::from(tb_num).max(1))
                / i64::from(fps_num).max(1)
        };
        let ticks = |p: i64| -> i64 {
            let num = i64::from(tb_num) * 10_000_000;
            let den = i64::from(tb_den);
            let g = gcd(num, den).max(1);
            p.saturating_mul(num / g) / (den / g).max(1)
        };
        let (ts_start, ts_end) = match opts.stamp {
            Some((start, end)) => {
                let at = |t: u64| ticks(i64::try_from(t).unwrap_or(i64::MAX));
                (at(start), at(end))
            }
            None => (ticks(pts(self.frames)), ticks(pts(self.frames + 1))),
        };
        // libvpx's frame rate is the durations' reciprocal: a picture timed
        // by its caller must last, and end after the last one did. (A
        // coarse timebase may give vpxenc's own stamps no duration at all,
        // which libvpx's rate control passes over, and so does this.)
        if opts.stamp.is_some()
            && (ts_end <= ts_start || self.last_end.is_some_and(|last| ts_end <= last))
        {
            return Err(Error::Unsupported(
                "a picture's time is not after the last picture's",
            ));
        }
        self.last_end = Some(ts_end);
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
        // A caller deciding blocks itself may ask for intra-only frames;
        // otherwise the rate control decides, as libvpx's does.
        cpi.force_key_frame = d.is_some() && !opts.inter;

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
        let (mi_rows, mi_cols) = (cpi.common.mi_rows, cpi.common.mi_cols);

        // encode_without_recode_loop: what the source says. The last
        // source picture (Last_Source) exists from the second frame on.
        let last_src = self
            .last_src
            .as_ref()
            .filter(|l| l.width == width && l.height == height);
        if intra_only {
            cpi.consec_zero_mv.fill(0);
        }
        cpi.rc.high_source_sad = false;
        if let Some(last) = last_src {
            // vp9_scene_detection_onepass (realtime, CBR, speed 5 and up).
            let mut scene = content::SceneState {
                avg_source_sad: cpi.rc.avg_source_sad,
            };
            let (high, motion) = content::scene_detection(
                &src.planes[0],
                &last.planes[0],
                mi_rows,
                mi_cols,
                cpi.rc.frames_since_key,
                &mut scene,
            );
            cpi.rc.avg_source_sad = scene.avg_source_sad;
            cpi.rc.high_source_sad = high;
            cpi.rc.high_num_blocks_with_motion = motion;
            if !cpi.oxcf.screen_content {
                cpi.scene_change_rate_reset();
            }
        }
        // vp9_update_noise_estimate: realtime CBR with cyclic refresh at
        // 640x360 and up.
        let noise_enable = cpi.oxcf.cyclic_refresh
            && cpi.oxcf.speed >= 5
            && !cpi.oxcf.screen_content
            && u64::from(width) * u64::from(height) >= 640 * 360;
        let (frame_counter, encoded) = (cpi.common.current_video_frame, cpi.frames_encoded);
        let (fsk, afl) = (cpi.rc.frames_since_key, cpi.rc.avg_frame_low_motion);
        let use_skin = cpi.use_skin_detection;
        let scene_change = cpi.rc.high_source_sad;
        cpi.noise.update(
            noise_enable,
            frame_counter,
            [&src.planes[0], &src.planes[1], &src.planes[2]],
            last_src.map(|l| &l.planes[0]),
            width,
            height,
            mi_rows,
            mi_cols,
            &cpi.consec_zero_mv,
            scene_change,
            use_skin,
            encoded,
            fsk,
            afl,
        );
        // The speed features that change frame to frame: speed 8 short-
        // circuits low-variance blocks harder unless the source is noisy.
        let short_circuit_low_temp_var = if cpi.noise.enabled
            && width >= 1280
            && height >= 720
            && cpi.noise.extract_level() >= NoiseLevel::Medium
        {
            2
        } else {
            3
        };

        // The quantiser, and the frame set up.
        let (q, _bottom, _top) = cpi.rc_pick_q_and_bounds_one_pass_cbr();
        let mut q = if cpi.rc.force_max_q {
            cpi.rc.force_max_q = false;
            cpi.rc.worst_quality
        } else {
            q
        };
        if let Some(f) = opts.fixed {
            q = f.base_qindex.clamp(0, 255);
        }
        // vp9_set_high_precision_mv: always at the frame's start, then by
        // the quantiser on an inter frame (set_size_dependent_vars).
        cpi.common.allow_high_precision_mv = intra_only || q < HIGH_PRECISION_MV_QTHRESH;
        cpi.mv_costs
            .set_high_precision(cpi.common.allow_high_precision_mv);
        // Skin detection, from here on (8-bit, speed 5 and up, CBR, video,
        // cyclic refresh).
        cpi.use_skin_detection =
            cpi.oxcf.speed >= 5 && !cpi.oxcf.screen_content && cpi.oxcf.cyclic_refresh;
        // vp9_set_quantizer.
        cpi.common.quant = Quantization {
            base_qindex: q,
            ..Quantization::default()
        };
        cpi.setup_frame();
        if key_frame {
            cpi.rt.clear_prev_mi();
        }
        // A scene change at a low quantiser is coded at a high one
        // (FAST_DETECTION_MAXQ, inter frames).
        if !intra_only
            && opts.fixed.is_none()
            && cpi.rc.high_source_sad
            && let Some(nq) = cpi.encodedframe_overshoot_fast(q)
        {
            q = nq;
            cpi.common.quant.base_qindex = q;
        }
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
            let rd_frame = rd_frame_of(cpi);
            let czm = core::mem::take(&mut cpi.consec_zero_mv);
            cpi.cyclic_refresh_setup(&czm, rd_frame);
            cpi.consec_zero_mv = czm;
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
        let ref_luma: [Option<Arc<LumaRef>>; 3] =
            [lst, gld, alt].map(|i| self.ref_luma.get(i).cloned().flatten());
        let same_size = |r: &Option<Arc<AnyFrame>>| {
            r.as_ref()
                .is_some_and(|f| f.width() == width && f.height() == height)
        };
        let cm = &cpi.common;
        let mut h = FrameHeader {
            profile: 0,
            bit_depth: 8,
            color_space: cpi.oxcf.color_space,
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
            // Set once the frame is coded: the cyclic refresh may cancel a
            // golden refresh.
            refresh_frame_flags: 0,
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
        // vp9_initialize_rd_consts: the multiplier, and on libvpx's schedule
        // (key frames, and every eighth frame from the first) the mode and
        // vector costs from this frame's probabilities.
        let rd_frame = rd_frame_of(cpi);
        let rdmult = compute_rd_mult(q + cm.quant.y_dc_delta_q, rd_frame);
        if key_frame || (cm.current_video_frame & 7) == 1 {
            cpi.mode_costs.fill(&fc);
            if !intra_only {
                cpi.mv_costs.build(&fc.nmvc, cm.allow_high_precision_mv);
                cpi.mode_costs.build_inter_mode_cost(&fc);
            }
        }
        // encode_frame_internal: no golden reference just after a golden
        // refresh.
        if !key_frame && cpi.rc.frames_since_golden == 0 {
            cpi.ref_frame_flags &= !GOLD_FLAG;
        }
        let costing = fc.clone();
        let ref_bufs: [Option<&FrameBuf<u8>>; 3] = [0, 1, 2].map(|i| {
            refs.get(i)
                .and_then(Option::as_deref)
                .and_then(<u8 as Pixel>::from_any)
        });
        // encode_frame_internal's use_prev_frame_mvs.
        let cm = &cpi.common;
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
        // set_block_thresholds: each segment's mode thresholds.
        let thresh_mult = crate::enc::rd::thresh_mult(true);
        let threshes: Box<[[[i32; MAX_MODES]; BLOCK_SIZES]; 8]> =
            Box::new(core::array::from_fn(|s| {
                let qi = cpi.common.seg.qindex(s as u8, q) + cpi.common.quant.y_dc_delta_q;
                crate::enc::rd::block_thresholds(qi.clamp(0, 255), &thresh_mult)
            }));
        let y_ac_q = i32::from(cpi.quants.get(0, usize::try_from(q).unwrap_or(0)).dequant[1]);
        // sf->nonrd_use_ml_partition: speed 8 partitions inter frames of
        // 352x288 pixels or fewer by search, and sets no variance
        // thresholds for them (vp9_set_variance_partition_thresholds
        // returns early).
        let learned_partition = !intra_only && u64::from(width) * u64::from(height) <= 352 * 288;
        let frame_vbp = if learned_partition {
            self.frame_vbp
        } else if intra_only {
            (
                crate::enc::partition::key_frame_thresholds(y_ac_q),
                0i64,
                0i64,
            )
        } else {
            let thresholds = crate::enc::partition::inter_thresholds(
                y_ac_q,
                1,
                cpi.noise.enabled.then(|| cpi.noise.extract_level()),
                crate::enc::partition::ContentState::Invalid,
                width,
                height,
                8,
                false,
                cpi.rc.avg_frame_qindex[crate::enc::ratectrl::INTER_FRAME],
                self.vbp_threshold_8x8,
            );
            let (sad, copy) = if cpi.rc.high_source_sad {
                (0, 0)
            } else {
                let ac = i64::from(y_ac_q);
                let sad = if width <= 352 && height <= 288 {
                    10
                } else {
                    (ac << 1).max(1000)
                };
                let copy = if width <= 352 && height <= 288 {
                    4000
                } else if width <= 640 && height <= 360 {
                    8000
                } else {
                    (ac << 3).max(8000)
                };
                (sad, copy)
            };
            (thresholds, sad, copy)
        };
        self.frame_vbp = frame_vbp;
        #[cfg(test)]
        trace_frame(cpi, rdmult, short_circuit_low_temp_var, tx_mode, &frame_vbp);
        let ref_frame_flags = cpi.ref_frame_flags;
        let skip_encode_frame = cpi.skip_encode_frame;
        let vbp_threshold_8x8 = self.vbp_threshold_8x8;
        let cyclic_refresh_mode = cpi.oxcf.cyclic_refresh && opts.fixed.is_none();

        // The decisions: the caller's, or libvpx's.
        let Cpi {
            common,
            quants,
            cr,
            rt,
            mv_costs,
            mode_costs,
            consec_zero_mv,
            rc,
            noise,
            ..
        } = &mut *cpi;
        let (_, vbp_threshold_sad, vbp_threshold_copy) = frame_vbp;
        let decisions = FrameDecisions {
            intra_only,
            rdmult,
            y_ac_q,
            dc_q: i32::from(header::dc_quant(q, 0, 8)),
            search: SearchFrame {
                mv_costs,
                mode_costs,
                threshes: &threshes,
                luma: [0, 1, 2].map(|i| ref_luma.get(i).and_then(Option::as_deref)),
                ref_frame_flags,
                frames_since_golden: rc.frames_since_golden,
                avg_frame_low_motion: rc.avg_frame_low_motion,
                scene_change: rc.high_source_sad,
                high_num_blocks_with_motion: rc.high_num_blocks_with_motion,
                noise_enabled: noise.enabled,
                noise_level: noise.level,
                rdmult,
                cr_rdmult: cr.rdmult,
                // The configured mode, not whether this frame codes
                // segments: a scene cut codes none, and libvpx still sizes
                // its transforms as under cyclic refresh. A caller's fixed
                // segments are not the refresh's.
                cyclic_refresh: cyclic_refresh_mode,
                current_video_frame: common.current_video_frame,
                skip_encode_frame,
                short_circuit_low_temp_var,
                rd_frame,
                adaptive_rd_thresh: crate::enc::pickinter::speed8_adaptive_rd_thresh(width, height),
                max_partition_size: if learned_partition {
                    crate::common::BLOCK_64X64
                } else {
                    crate::common::BLOCK_32X32
                },
                base_qindex: q,
                frame_width: width,
                frame_height: height,
            },
            settings: InterSettings {
                base_qindex: q,
                vbp_threshold_sad,
                vbp_threshold_copy,
                vbp_threshold_8x8,
                noise_extracted: noise.enabled.then(|| noise.extract_level()),
                avg_inter_qindex: rc.avg_frame_qindex[crate::enc::ratectrl::INTER_FRAME],
                copy_partition: true,
                max_copied_frame: 4,
                frames_since_key: rc.frames_since_key,
                use_skin_detection: true,
                use_source_sad: last_src.is_some(),
                cyclic_refresh: cyclic_refresh_mode,
                learned_partition,
            },
            consec_zero_mv,
            last_src,
            src: &src,
        };
        // The blocks: the caller's decisions, on one thread; libvpx's, each
        // tile column on a thread of its own where there are several of
        // both (a frame of one tile row: the encoder makes no others).
        let workers = self.threads.min(1usize << h.log2_tile_cols);
        let mut encoded = match d {
            Some(d) => {
                let mut fe =
                    FrameEncoder::new(&h, &costing, &common.seg, quants, &src, &mut recon, inter);
                fe.encode_tiles(d);
                fe.finish()
            }
            None if workers > 1 && h.log2_tile_rows == 0 => {
                let frame = FrameShared {
                    h: &h,
                    fc: &costing,
                    seg: &common.seg,
                    quants,
                    src: &src,
                    inter,
                    decisions,
                };
                encode_columns_threaded(&frame, &mut recon, rt, cr, workers)?
            }
            None => {
                let mut own = decisions.on(rt, cr);
                let mut fe =
                    FrameEncoder::new(&h, &costing, &common.seg, quants, &src, &mut recon, inter);
                fe.encode_tiles(&mut own);
                fe.finish()
            }
        };
        // fix_interp_filter: a frame of one filter codes it once.
        h.interp_filter = bitstream::fix_interp_filter(SWITCHABLE, &encoded.counts.counts);
        // sf.skip_encode_frame for the next frame: this one predicted
        // mostly from other frames.
        let ii = &encoded.counts.counts.intra_inter;
        let (intra_count, inter_count) = ii.iter().fold((0u32, 0u32), |(a, b), c| {
            (a.wrapping_add(c[0]), b.wrapping_add(c[1]))
        });
        let cpi = &mut self.cpi;
        cpi.skip_encode_frame = !key_frame && (intra_count << 2) < inter_count;
        // The cyclic refresh's account of the frame, which may cancel a
        // golden refresh due now.
        if cpi.oxcf.cyclic_refresh && cpi.common.seg.enabled && !intra_only && cpi.cr.content_mode {
            cpi.cyclic_refresh_postencode(&encoded.mi);
        }
        h.refresh_frame_flags = cpi.refresh_mask();
        if key_frame {
            // The 8x8 threshold an inter frame leaves in place.
            self.vbp_threshold_8x8 = crate::enc::partition::key_frame_thresholds(i32::from(
                cpi.quants.get(0, usize::try_from(q).unwrap_or(0)).dequant[1],
            ))[3];
        }

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
        // vp9_update_reference_frames, each with its padded luma for the
        // next frames' searches.
        let luma = Arc::new(LumaRef::new(&recon.planes[0]));
        let frame = Arc::new(AnyFrame::Eight(recon));
        let mut refresh = |slot: usize| {
            if let Some(s) = self.ref_frame_map.get_mut(slot) {
                *s = Some(Arc::clone(&frame));
            }
            if let Some(s) = self.ref_luma.get_mut(slot) {
                *s = Some(Arc::clone(&luma));
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
        // get_ref_frame_flags: the next frame's references, those not the
        // same picture as LAST.
        let same = |a: usize, b: usize| match (self.ref_frame_map.get(a), self.ref_frame_map.get(b))
        {
            (Some(Some(x)), Some(Some(y))) => Arc::ptr_eq(x, y),
            (Some(None), Some(None)) => true,
            _ => false,
        };
        let mut flags = LAST_FLAG | GOLD_FLAG | ALT_FLAG;
        if same(gld, lst) {
            flags &= !GOLD_FLAG;
        }
        if same(alt, lst) || same(gld, alt) {
            flags &= !ALT_FLAG;
        }
        cpi.ref_frame_flags = flags;
        cpi.rc_postencode_update(u64::try_from(out.len()).unwrap_or(u64::MAX));
        if !intra_only {
            cpi.compute_frame_low_motion(&encoded.mi);
        }
        // What each block's coding updates in libvpx as it goes, done here
        // for the frame: the quantisers the cells were coded at
        // (vp9_cyclic_refresh_update_sb_postencode) and the cells' runs of
        // zero motion (update_zeromv_cnt).
        if cpi.oxcf.cyclic_refresh && cpi.common.seg.enabled && cpi.cr.content_mode {
            let base = cpi.common.quant.base_qindex;
            cpi.cr.update_sb_postencode(&encoded.mi, base);
        }
        update_zeromv_cnt(&mut cpi.consec_zero_mv, &encoded.mi);
        #[cfg(test)]
        crate::enc::trace::line(|| {
            format!(
                "E {} size={} seg1={} seg2={} lca={} rgf={} alm={} bl={}",
                cpi.common.current_video_frame,
                out.len(),
                cpi.cr.actual_num_seg1_blocks,
                cpi.cr.actual_num_seg2_blocks,
                cpi.cr.low_content_avg,
                u8::from(cpi.refresh_golden_frame),
                cpi.rc.avg_frame_low_motion,
                cpi.rc.buffer_level
            )
        });
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
            cpi.rt.swap_mi();
        }
        cpi.frames_encoded = cpi.frames_encoded.wrapping_add(1);
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
        self.last_src = Some(src);
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

/// What kind of frame the rate-distortion multipliers are for: libvpx's
/// `vp9_compute_rd_mult_based_on_qindex`'s branches (no alt-ref source in
/// a one-pass encode).
fn rd_frame_of(cpi: &Cpi) -> RdFrame {
    if cpi.common.key_frame {
        RdFrame::Key
    } else if cpi.refresh_golden_frame || cpi.refresh_alt_ref_frame {
        RdFrame::GoldenOrAltRef
    } else {
        RdFrame::Inter
    }
}

/// Each cell's run of frames of near-zero motion from the last frame,
/// after a frame: libvpx's `update_zeromv_cnt` for every block (blocks of
/// the cyclic refresh's three segments only).
fn update_zeromv_cnt(consec_zero_mv: &mut [u8], mi: &MiGrid) {
    let cols = mi.mi_cols;
    for b in &mi.blocks {
        if !(b.is_inter() && b.ref_frame[0] == LAST_FRAME && b.segment_id <= 2) {
            continue;
        }
        let small = |v: i16| i32::from(v).abs() < 8;
        let still = small(b.mv[0].row) && small(b.mv[0].col);
        let xmis = b.bw.min(cols - b.mi_col);
        let ymis = b.bh.min(mi.mi_rows - b.mi_row);
        for y in 0..ymis {
            let start = (b.mi_row + y) * cols + b.mi_col;
            if let Some(cells) = consec_zero_mv.get_mut(start..start + xmis) {
                for c in cells {
                    *c = if still { c.saturating_add(1) } else { 0 };
                }
            }
        }
    }
}

/// The trace line libvpx's instrumented build writes per frame, before its
/// blocks are coded.
#[cfg(test)]
fn trace_frame(cpi: &Cpi, rdmult: i32, scl: i32, tx_mode: TxMode, vbp: &([i64; 4], i64, i64)) {
    crate::enc::trace::line(|| {
        let cm = &cpi.common;
        let cr = &cpi.cr;
        let q = cm.quant.base_qindex;
        format!(
            "F {} q={q} kf={} rdm={rdmult} epb={} spb={} rff={} fsg={} ne={},{},{} hss={} scl={scl} sef={} seg={} crd={},{} crrd={} trs={} tds={} sbi={} tfg={} vbp={},{},{},{} vsad={} vcopy={} alm={} hp={} ifl=4 txm={tx_mode}",
            cm.current_video_frame,
            u8::from(cm.key_frame),
            crate::enc::rd::error_per_bit(rdmult),
            crate::enc::rd::sad_per_bit16(q),
            cpi.ref_frame_flags,
            cpi.rc.frames_since_golden,
            u8::from(cpi.noise.enabled),
            cpi.noise.value,
            cpi.noise.level as i32,
            u8::from(cpi.rc.high_source_sad),
            u8::from(cpi.skip_encode_frame),
            u8::from(cm.seg.enabled),
            cr.qindex_delta[1],
            cr.qindex_delta[2],
            cr.rdmult,
            cr.thresh_rate_sb,
            cr.thresh_dist_sb,
            cr.sb_index,
            cpi.rc.frames_till_gf_update_due,
            vbp.0[0],
            vbp.0[1],
            vbp.0[2],
            vbp.0[3],
            vbp.1,
            vbp.2,
            cpi.rc.avg_frame_low_motion,
            u8::from(cm.allow_high_precision_mv)
        )
    });
}

/// libvpx's decisions for one frame, but for the state they keep from block
/// to block and frame to frame, which they are given: the encoder's own, or
/// a tile column's clone of it.
#[derive(Clone, Copy)]
struct FrameDecisions<'a> {
    intra_only: bool,
    rdmult: i32,
    y_ac_q: i32,
    /// The frame's luma DC step, which the learned partitioning reads.
    dc_q: i32,
    search: SearchFrame<'a>,
    settings: InterSettings,
    consec_zero_mv: &'a [u8],
    last_src: Option<&'a FrameBuf<u8>>,
    src: &'a FrameBuf<u8>,
}

impl<'a> FrameDecisions<'a> {
    /// The decisions, keeping `state` and the cyclic refresh `cr`.
    fn on<'b>(&self, state: &'b mut RtState, cr: &'b mut CyclicRefresh) -> RtDecisions<'b>
    where
        'a: 'b,
    {
        if self.intra_only {
            RtDecisions::key_frame(self.rdmult, self.y_ac_q, state, self.last_src)
        } else {
            RtDecisions::inter(InterDecisions {
                search: self.search,
                settings: self.settings,
                state,
                cr,
                consec_zero_mv: self.consec_zero_mv,
                last_src: self.last_src.unwrap_or(self.src),
                sb: crate::enc::pickinter::SbState::default(),
                part: None,
                learned: crate::enc::nonrd::Learned::new(self.dc_q),
            })
        }
    }
}

/// What every tile column of a frame reads while the columns are coded on
/// threads of their own.
struct FrameShared<'a> {
    h: &'a FrameHeader,
    fc: &'a FrameContext,
    seg: &'a Segmentation,
    quants: &'a Quants,
    src: &'a FrameBuf<u8>,
    inter: Option<InterFrame<'a>>,
    decisions: FrameDecisions<'a>,
}

/// One tile column coded on a thread of its own: its strip of the
/// reconstruction and the luma pixel it starts at, its clones of the state
/// the decisions keep, and, once coded, what its encoder left.
struct ColumnJob {
    tile_col: usize,
    x0: usize,
    strip: FrameBuf<u8>,
    rt: RtState,
    cr: CyclicRefresh,
    encoded: Option<Encoded>,
}

/// The frame's blocks, each tile column on a thread of its own, `workers`
/// at once -- libvpx's `vp9_encode_tiles_mt` -- each from its own clone of
/// the state libvpx's decisions keep (no column reads another's), then put
/// together in column order and the state taken back: the frame one thread
/// codes, bit for bit.
///
/// A column's thread that panics -- a bug, which would panic on one thread
/// too -- panics this thread with the same payload.
///
/// # Errors
///
/// [`Error::Unsupported`] if a strip cannot be allocated.
fn encode_columns_threaded(
    frame: &FrameShared<'_>,
    recon: &mut FrameBuf<u8>,
    rt: &mut RtState,
    cr: &mut CyclicRefresh,
    workers: usize,
) -> Result<Encoded, Error> {
    let h = frame.h;
    let tile_cols = 1usize << h.log2_tile_cols;
    let plane_w: [usize; 3] = recon.planes.each_ref().map(|p| p.width);
    // Columns dealt out in turn: hand i takes columns i, i + workers, ...
    // Each hand is one thread's; its mutex only carries it across the
    // thread boundary, and is never contended.
    let mut dealt: Vec<Vec<ColumnJob>> = (0..workers.max(1)).map(|_| Vec::new()).collect();
    for tile_col in 0..tile_cols {
        let (x0, strip) = column_strip(recon, h, tile_col)?;
        if let Some(hand) = dealt.get_mut(tile_col % workers.max(1)) {
            hand.push(ColumnJob {
                tile_col,
                x0,
                strip,
                rt: rt.clone(),
                cr: cr.clone(),
                encoded: None,
            });
        }
    }
    let hands: Vec<Mutex<Vec<ColumnJob>>> = dealt.into_iter().map(Mutex::new).collect();
    let code_hand = |hand: &Mutex<Vec<ColumnJob>>| {
        // Poisoned only by a panic on this very hand, which is re-raised
        // below before the hand is read again.
        let mut hand = hand.lock().unwrap_or_else(PoisonError::into_inner);
        for job in hand.iter_mut() {
            let mut d = frame.decisions.on(&mut job.rt, &mut job.cr);
            let mut fe = FrameEncoder::for_tile_column(
                h,
                frame.fc,
                frame.seg,
                frame.quants,
                frame.src,
                &mut job.strip,
                frame.inter,
                job.tile_col,
                plane_w,
            );
            fe.encode_tile_column(&mut d, job.tile_col);
            job.encoded = Some(fe.finish());
        }
    };
    std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(workers);
        let mut here = vec![0];
        for (i, hand) in hands.iter().enumerate().skip(1) {
            match std::thread::Builder::new().spawn_scoped(scope, move || code_hand(hand)) {
                Ok(handle) => handles.push(handle),
                // A thread that cannot be made leaves its hand to this one.
                Err(_) => here.push(i),
            }
        }
        // This thread takes the first hand, and any left over.
        for i in here {
            if let Some(hand) = hands.get(i) {
                code_hand(hand);
            }
        }
        for handle in handles {
            if let Err(payload) = handle.join() {
                std::panic::resume_unwind(payload);
            }
        }
    });
    let mut columns = Vec::with_capacity(tile_cols);
    for hand in hands {
        for job in hand.into_inner().unwrap_or_else(PoisonError::into_inner) {
            let (col0, col1) = tile_span(job.tile_col, h.mi_cols(), h.log2_tile_cols);
            let tiles: Vec<usize> = (0..1usize << h.log2_tile_rows)
                .map(|r| r * tile_cols + job.tile_col)
                .collect();
            rt.absorb_columns(&job.rt, col0, col1, &tiles, job.tile_col + 1 == tile_cols);
            cr.absorb_columns(&job.cr, h.mi_cols(), col0, col1);
            // Every hand was coded, or its panic re-raised above.
            let encoded = job
                .encoded
                .ok_or(Error::Unsupported("a tile column was not coded"))?;
            columns.push(ColumnEncoded {
                tile_col: job.tile_col,
                x0: job.x0,
                strip: job.strip,
                encoded,
            });
        }
    }
    merge_columns(recon, h, columns)
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

    /// The colour space the configuration names is the one every key frame
    /// declares, and so the one a decoder reports; spaces this encoder does
    /// not code are refused rather than written.
    #[test]
    fn the_colour_space_reaches_the_decoder() {
        let pic = picture(64, 48, 3);
        for space in [0u8, 1, 2, 5] {
            let mut enc = Encoder::new(EncoderConfig {
                color_space: space,
                ..EncoderConfig::realtime(64, 48, 300)
            })
            .unwrap();
            let mut dec = crate::Decoder::new();
            for _ in 0..3 {
                let frame = enc.encode(views(&pic, 64, 48)).unwrap();
                let Some(shown) = dec.decode(&frame).unwrap() else {
                    panic!("colour space {space}: no picture");
                };
                assert_eq!(shown.color(), (space, false), "colour space {space}");
            }
        }
        for space in [6u8, 7, 8] {
            assert!(
                Encoder::new(EncoderConfig {
                    color_space: space,
                    ..EncoderConfig::realtime(64, 48, 300)
                })
                .is_err(),
                "colour space {space}"
            );
        }
    }

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
                stamp: None,
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
        for timebase in [(0, 1000), (1, 0)] {
            assert!(Encoder::new(EncoderConfig { timebase, ..config }).is_err());
        }
        assert!(
            Encoder::new(EncoderConfig {
                timebase: (1, 30),
                ..config
            })
            .is_ok()
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
        // The rate control's state after it, as libvpx leaves it. The first
        // frame lasts 33 ms by vpxenc's millisecond stamps (0 to 1000 / 30,
        // rounded down): 30.3 frames a second, 33,000 bits each -- libvpx's
        // buffer after the frame is 500,000 + 33,000 less the frame's bits.
        let rc = &enc.cpi.rc;
        assert_eq!(rc.avg_frame_bandwidth, 33_000);
        assert_eq!(rc.buffer_level, 500_000 + 33_000 - 8 * frame.len() as i64);
        assert_eq!(rc.this_frame_target, 250_000);
        assert_eq!(rc.last_q[crate::enc::ratectrl::KEY_FRAME], 161);
        assert_eq!(rc.baseline_gf_interval, 40);
        assert_eq!(rc.frames_till_gf_update_due, 39);
    }

    /// The timebase sets the frames' durations, as `vpxenc`'s stamps do: in
    /// milliseconds the first of 30 frames a second lasts 33 ms, so its
    /// share of 1000 kbit/s is 33,000 bits; in thirtieths of a second it
    /// lasts one tick, which libvpx's ten-million-a-second clock holds as
    /// 333,333, and its share is 33,333.
    #[test]
    fn the_timebase_sets_a_frames_share() {
        let (w, h) = (64usize, 64usize);
        let pic = [
            vec![90u8; w * h],
            vec![128u8; w * h / 4],
            vec![128u8; w * h / 4],
        ];
        for (timebase, share) in [((1, 1000), 33_000), ((1, 30), 33_333)] {
            let mut enc = Encoder::new(EncoderConfig {
                timebase,
                ..EncoderConfig::realtime(64, 64, 1000)
            })
            .unwrap();
            enc.encode(views(&pic, w, h)).unwrap();
            assert_eq!(enc.cpi.rc.avg_frame_bandwidth, share, "{timebase:?}");
        }
    }

    /// A picture timed by its caller is coded as one stamped by the frame
    /// rate: times equal to `vpxenc`'s give its very frames.
    #[test]
    fn timed_pictures_at_vpxencs_times_are_its_frames() {
        let (w, h) = (96usize, 64usize);
        let mut by_rate = Encoder::new(EncoderConfig::realtime(96, 64, 300)).unwrap();
        let mut timed = Encoder::new(EncoderConfig::realtime(96, 64, 300)).unwrap();
        // vpxenc's stamps at 30 frames a second, in milliseconds.
        let pts = |n: u64| n * 1000 / 30;
        for n in 0..12u64 {
            let pic = picture_moved(w, h, 3, (n * 2) as usize);
            let a = by_rate.encode(views(&pic, w, h)).unwrap();
            let b = timed
                .encode_timed(views(&pic, w, h), pts(n), pts(n + 1))
                .unwrap();
            assert_eq!(a, b, "frame {n}");
        }
    }

    /// The rate control follows the times: pictures a tenth of a second
    /// apart are ten a second, each given three times the bits of one of
    /// thirty a second; and times that do not move on are refused.
    #[test]
    fn timed_pictures_set_the_frame_rate() {
        let (w, h) = (64usize, 64usize);
        let pic = picture_moved(w, h, 5, 0);
        let mut enc = Encoder::new(EncoderConfig::realtime(64, 64, 300)).unwrap();
        for n in 0..4u64 {
            enc.encode_timed(views(&pic, w, h), n * 100, (n + 1) * 100)
                .unwrap();
        }
        assert!(
            (enc.cpi.framerate - 10.0).abs() < 1e-6,
            "{} frames a second",
            enc.cpi.framerate
        );
        assert_eq!(enc.cpi.rc.avg_frame_bandwidth, 30_000);
        // A picture that lasts nothing, or ends before the last one did.
        assert!(enc.encode_timed(views(&pic, w, h), 500, 500).is_err());
        assert!(enc.encode_timed(views(&pic, w, h), 350, 400).is_err());
        assert!(enc.encode_timed(views(&pic, w, h), 400, 450).is_ok());
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
