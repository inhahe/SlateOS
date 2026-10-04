//! The compressor's state: libvpx's `VP9_COMP`, with its `VP9_COMMON`
//! (`Common`) and `VP9EncoderConfig` (`Oxcf`), the fields the realtime path
//! reads and writes.
//!
//! libvpx's encoder is one large structure that every part of it reaches
//! into -- the rate control reads the frame type, the cyclic refresh reads
//! the rate control, the mode search reads both -- and the port keeps that
//! shape: rate control, cyclic refresh and the frame loop are `impl Cpi`
//! blocks in modules of their own, so that `cpi->rc.x` in libvpx is
//! `self.rc.x` here and a function ports line for line.
//!
//! Translated into Rust from libvpx v1.17.0's `vp9/encoder/vp9_encoder.h`,
//! `vp9_encoder.c`, `vp9/common/vp9_onyxc_int.h` and `vp9/vp9_cx_iface.c`
//! (copyright the WebM project authors), used under libvpx's BSD licence and
//! patent grant (`licenses/libvpx-LICENSE`, `licenses/libvpx-PATENTS`).

use crate::enc::aq_cyclicrefresh::CyclicRefresh;
use crate::enc::bitstream::LoopFilterState;
use crate::enc::content::NoiseEstimate;
use crate::enc::mcomp::MvCosts;
use crate::enc::nonrd::RtState;
use crate::enc::quantize::{Deltas, Quants};
use crate::enc::ratectrl::RateControl;
use crate::enc::rd::ModeCosts;
use crate::header::{Quantization, Segmentation};
use crate::probs::FrameContext;

/// How many probability contexts a stream may save.
pub(crate) const FRAME_CONTEXTS: usize = 4;

/// libvpx's `vp9_quantizer_to_qindex` table: the 0-to-63 quantiser scale
/// `vpxenc`'s `--min-q` and `--max-q` take, as quantiser indices.
const QUANTIZER_TO_QINDEX: [u8; 64] = [
    0, 4, 8, 12, 16, 20, 24, 28, 32, 36, 40, 44, 48, 52, 56, 60, 64, 68, 72, 76, 80, 84, 88, 92,
    96, 100, 104, 108, 112, 116, 120, 124, 128, 132, 136, 140, 144, 148, 152, 156, 160, 164, 168,
    172, 176, 180, 184, 188, 192, 196, 200, 204, 208, 212, 216, 220, 224, 228, 232, 236, 240, 244,
    249, 255,
];

/// A quantiser on the 0-to-63 scale as an index: libvpx's
/// `vp9_quantizer_to_qindex`.
pub(crate) fn quantizer_to_qindex(quantizer: u8) -> i32 {
    i32::from(
        QUANTIZER_TO_QINDEX
            .get(usize::from(quantizer.min(63)))
            .copied()
            .unwrap_or(255),
    )
}

/// The encoder's configuration as libvpx keeps it: `VP9EncoderConfig`, the
/// fields the realtime path reads, filled as `vp9_cx_iface.c`'s
/// `set_encoder_config` fills them from `vpxenc`'s options.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Oxcf {
    pub width: u32,
    pub height: u32,
    /// Frames per second the timebase implies: libvpx's `init_framerate`.
    pub init_framerate: f64,
    /// The timebase, seconds per unit: numerator and denominator.
    pub timebase: (u32, u32),
    /// The rate the pictures come at, frames per second as numerator and
    /// denominator: `vpxenc`'s `--fps`, from which it stamps each picture.
    pub fps: (u32, u32),
    /// Bits per second.
    pub target_bandwidth: i64,
    pub starting_buffer_level_ms: i64,
    pub optimal_buffer_level_ms: i64,
    pub maximum_buffer_size_ms: i64,
    /// Quantiser indices.
    pub best_allowed_q: i32,
    pub worst_allowed_q: i32,
    pub under_shoot_pct: i32,
    pub over_shoot_pct: i32,
    pub rc_max_intra_bitrate_pct: i32,
    pub rc_max_inter_bitrate_pct: i32,
    pub gf_cbr_boost_pct: i32,
    /// The longest gap between key frames: libvpx's `key_freq`.
    pub key_freq: i32,
    pub auto_key: bool,
    pub two_pass_vbrmax_section: i32,
    pub min_gf_interval: i32,
    pub max_gf_interval: i32,
    pub speed: i32,
    /// Cyclic refresh adaptive quantisation (libvpx's `aq_mode` 3).
    pub cyclic_refresh: bool,
    pub screen_content: bool,
    pub drop_frames_water_mark: i32,
    pub frame_parallel_decoding_mode: bool,
    pub error_resilient_mode: bool,
    pub tile_columns: u32,
    pub tile_rows: u32,
}

impl Oxcf {
    /// Whether every frame is lossless: libvpx's `is_lossless_requested`
    /// (both quantiser bounds 0).
    pub(crate) fn lossless(&self) -> bool {
        self.best_allowed_q == 0 && self.worst_allowed_q == 0
    }
}

/// What is common to encoder and decoder about the frame being coded and
/// the stream so far: libvpx's `VP9_COMMON`, the encoder's fields.
#[derive(Clone, Debug)]
pub(crate) struct Common {
    pub width: u32,
    pub height: u32,
    pub mi_cols: usize,
    pub mi_rows: usize,
    /// 16x16 macroblocks, rounding up: libvpx's `MBs`.
    pub mbs: i32,
    pub key_frame: bool,
    pub last_key_frame: bool,
    pub intra_only: bool,
    pub show_frame: bool,
    pub current_video_frame: u32,
    /// Whether an inter frame's vectors may be eighth-pixel.
    pub allow_high_precision_mv: bool,
    pub quant: Quantization,
    pub seg: Segmentation,
    pub lf: LoopFilterState,
    pub error_resilient_mode: bool,
    pub frame_parallel_decoding_mode: bool,
    pub refresh_frame_context: bool,
    pub reset_frame_context: u8,
    pub frame_context_idx: usize,
    pub frame_contexts: [FrameContext; FRAME_CONTEXTS],
    pub log2_tile_cols: u32,
    pub log2_tile_rows: u32,
}

impl Common {
    /// libvpx's `frame_is_intra_only`.
    pub(crate) fn frame_is_intra_only(&self) -> bool {
        self.key_frame || self.intra_only
    }
}

/// The compressor: libvpx's `VP9_COMP`.
#[derive(Debug)]
pub(crate) struct Cpi {
    pub oxcf: Oxcf,
    pub common: Common,
    pub rc: RateControl,
    pub cr: CyclicRefresh,
    pub quants: Quants,
    /// Frames per second, as measured from the timestamps.
    pub framerate: f64,
    /// Which references the frame refreshes, and which slots they are in:
    /// libvpx's `refresh_*_frame` and `lst_fb_idx`, `gld_fb_idx`,
    /// `alt_fb_idx` (0, 1 and 2 for a one-pass encode).
    pub refresh_last_frame: bool,
    pub refresh_golden_frame: bool,
    pub refresh_alt_ref_frame: bool,
    pub lst_fb_idx: usize,
    pub gld_fb_idx: usize,
    pub alt_fb_idx: usize,
    /// The last frame's size and whether it was shown: whether its vectors
    /// may predict this frame's (libvpx's `cm->last_width`, `last_height`
    /// and `last_show_frame`).
    pub last_width: u32,
    pub last_height: u32,
    pub last_show_frame: bool,
    /// Whether the caller forces this frame to be a key frame: libvpx's
    /// `FRAMEFLAGS_KEY` in `frame_flags`.
    pub force_key_frame: bool,
    /// libvpx's timestamps, in 1/10,000,000 s: the first ever, and the
    /// last frame's start and end (`adjust_frame_rate`).
    pub first_time_stamp_ever: i64,
    pub last_time_stamp_seen: i64,
    pub last_end_time_stamp_seen: i64,
    /// The source's estimated noise: libvpx's `noise_estimate`.
    pub noise: NoiseEstimate,
    /// Per 8x8 cell, for how many frames in a row its block was still
    /// against the last frame: libvpx's `consec_zero_mv`.
    pub consec_zero_mv: Vec<u8>,
    /// Which references the frame may use: libvpx's `ref_frame_flags`.
    pub ref_frame_flags: u8,
    /// The last frame mostly predicted from other frames, so this one's
    /// search predicts intra modes from the source (`sf.skip_encode_frame`).
    pub skip_encode_frame: bool,
    /// What vectors and modes cost the searches, rebuilt on libvpx's
    /// schedule.
    pub mv_costs: MvCosts,
    pub mode_costs: ModeCosts,
    /// The realtime decisions' state from frame to frame.
    pub rt: RtState,
    /// How many frames were encoded: libvpx's `num_encoded_top_layer`.
    pub frames_encoded: u32,
    /// libvpx's `use_skin_detection`, set from the first frame on.
    pub use_skin_detection: bool,
}

impl Cpi {
    /// A compressor at the start of a stream: libvpx's `vp9_create_compressor`,
    /// `init_config` and `vp9_change_config`, the parts the realtime path
    /// depends on.
    pub(crate) fn new(oxcf: Oxcf) -> Self {
        let mi_cols = (oxcf.width as usize).div_ceil(8);
        let mi_rows = (oxcf.height as usize).div_ceil(8);
        let mb_cols = mi_cols.div_ceil(2);
        let mb_rows = mi_rows.div_ceil(2);
        let mbs = i32::try_from(mb_cols.saturating_mul(mb_rows)).unwrap_or(i32::MAX);
        let (min_log2, max_log2) =
            crate::header::tile_n_bits(u32::try_from(mi_cols).unwrap_or(u32::MAX));
        let sb64_rows = mi_rows.div_ceil(8);
        let max_log2_rows = if sb64_rows >= 4 {
            2
        } else if sb64_rows >= 2 {
            1
        } else {
            0
        };
        let common = Common {
            width: oxcf.width,
            height: oxcf.height,
            mi_cols,
            mi_rows,
            mbs,
            key_frame: true,
            last_key_frame: true,
            intra_only: false,
            show_frame: true,
            current_video_frame: 0,
            allow_high_precision_mv: false,
            quant: Quantization::default(),
            seg: Segmentation::default(),
            lf: LoopFilterState::default(),
            error_resilient_mode: oxcf.error_resilient_mode,
            frame_parallel_decoding_mode: oxcf.frame_parallel_decoding_mode,
            refresh_frame_context: true,
            reset_frame_context: 0,
            frame_context_idx: 0,
            frame_contexts: core::array::from_fn(|_| FrameContext::defaults()),
            // set_tile_limits.
            log2_tile_cols: oxcf.tile_columns.clamp(min_log2, max_log2),
            log2_tile_rows: oxcf.tile_rows.min(max_log2_rows),
        };
        let mut cpi = Self {
            oxcf,
            common,
            rc: RateControl::new(&oxcf),
            cr: CyclicRefresh::new(mi_rows.saturating_mul(mi_cols)),
            quants: Quants::new(Deltas::default(), 0),
            framerate: oxcf.init_framerate,
            refresh_last_frame: true,
            refresh_golden_frame: false,
            refresh_alt_ref_frame: false,
            lst_fb_idx: 0,
            gld_fb_idx: 1,
            alt_fb_idx: 2,
            last_width: 0,
            last_height: 0,
            last_show_frame: false,
            force_key_frame: false,
            first_time_stamp_ever: i64::MAX,
            last_time_stamp_seen: 0,
            last_end_time_stamp_seen: 0,
            noise: NoiseEstimate::new(oxcf.width, oxcf.height),
            consec_zero_mv: vec![0; mi_rows.saturating_mul(mi_cols)],
            ref_frame_flags: 0,
            skip_encode_frame: false,
            mv_costs: MvCosts::new(),
            mode_costs: ModeCosts::default(),
            rt: RtState::new(mi_rows, mi_cols),
            frames_encoded: 0,
            use_skin_detection: false,
        };
        // vp9_change_config: the buffer sizes, the frame rate's share of the
        // bitrate, the quality bounds.
        cpi.set_rc_buffer_sizes();
        cpi.new_framerate(oxcf.init_framerate);
        cpi.rc.worst_quality = oxcf.worst_allowed_q;
        cpi.rc.best_quality = oxcf.best_allowed_q;
        // vp9_rc_init reads the buffer sizes vp9_change_config set first.
        cpi.rc.buffer_level = cpi.rc.starting_buffer_level;
        cpi.rc.bits_off_target = cpi.rc.starting_buffer_level;
        cpi
    }
}
