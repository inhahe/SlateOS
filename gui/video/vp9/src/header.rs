//! The pieces of a frame header: the fields read with plain bits before the
//! entropy-coded data, and the probability updates read with the bool decoder
//! after them.
//!
//! The order the pieces are read in, and the decoder state each one changes,
//! is libvpx's `read_uncompressed_header` and `read_compressed_header`,
//! ported in `decoder.rs`; this module holds the parts that only read.
//!
//! Translated into Rust from libvpx v1.17.0's `vp9/decoder/vp9_decodeframe.c`,
//! `vp9/decoder/vp9_decoder.c`, `vp9/vp9_dx_iface.c`,
//! `vp9/common/vp9_seg_common.c`, `vp9_quant_common.c`, `vp9_tile_common.c`
//! and `vp9_pred_common.c` (copyright the WebM project authors), used under
//! libvpx's BSD licence and patent grant (`licenses/libvpx-LICENSE`,
//! `licenses/libvpx-PATENTS`).

use crate::Error;
use crate::bits::BitReader;
use crate::boolread::BoolReader;
use crate::common::{
    ALLOW_32X32, ALTREF_FRAME, BILINEAR, BLOCK_SIZE_GROUPS, CLASS0_SIZE, COEF_BANDS,
    COMP_INTER_CONTEXTS, COMPOUND_REFERENCE, EIGHTTAP, EIGHTTAP_SHARP, EIGHTTAP_SMOOTH,
    GOLDEN_FRAME, INTER_MODE_CONTEXTS, INTRA_INTER_CONTEXTS, INTRA_MODES, InterpFilter, LAST_FRAME,
    MAX_SEGMENTS, MB_MODE_COUNT, MI_BLOCK_SIZE_LOG2, MV_CLASSES, MV_FP_SIZE, MV_JOINTS,
    MV_OFFSET_BITS, PARTITION_CONTEXTS, PARTITION_TYPES, PLANE_TYPES, REF_CONTEXTS, REF_TYPES,
    REFERENCE_MODE_SELECT, RefFrame, ReferenceMode, SEG_LVL_ALT_LF, SEG_LVL_ALT_Q, SEG_LVL_MAX,
    SINGLE_REFERENCE, SWITCHABLE, SWITCHABLE_FILTER_CONTEXTS, SWITCHABLE_FILTERS, TX_MODE_SELECT,
    TX_SIZE_CONTEXTS, TxMode, UNCONSTRAINED_NODES, band_coeff_contexts,
};
use crate::probs::{FrameContext, diff_update_prob, update_mv_probs};
use crate::tables;

/// The first two bits of every frame.
const FRAME_MARKER: u32 = 2;
/// The three bytes after a key or intra-only frame's first fields.
const SYNC_CODE: [u32; 3] = [0x49, 0x83, 0x42];

/// Profiles: 0 is 8-bit 4:2:0, 1 adds other subsamplings, 2 and 3 are the
/// same at 10 and 12 bits.
pub const MAX_PROFILES: u8 = 4;

/// libvpx's `VPX_CS_SRGB`: the colour space whose chroma is never subsampled.
pub const CS_SRGB: u8 = 7;
/// libvpx's `VPX_CS_BT_601`: what a profile 0 intra-only frame assumes.
pub const CS_BT_601: u8 = 1;

/// The largest quantiser index.
pub const MAXQ: i32 = 255;
/// The largest loop filter level.
pub const MAX_LOOP_FILTER: i32 = 63;

/// How a frame's samples are coded: what a key frame (or an intra-only frame
/// above profile 0) declares and later frames inherit.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ColorConfig {
    /// 8, 10 or 12.
    pub bit_depth: u8,
    /// libvpx's `vpx_color_space_t`.
    pub color_space: u8,
    /// Full range rather than studio range.
    pub full_range: bool,
    /// Chroma halved horizontally.
    pub ss_x: u8,
    /// Chroma halved vertically.
    pub ss_y: u8,
}

/// Read a profile: libvpx's `vp9_read_profile`.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "three bits, so the profile is at most 4"
)]
pub fn read_profile(rb: &mut BitReader<'_>) -> u8 {
    let mut profile = rb.bit() | (rb.bit() << 1);
    if profile > 2 {
        profile += rb.bit();
    }
    profile as u8
}

/// Whether the next 24 bits are VP9's sync code: libvpx's
/// `vp9_read_sync_code`, which stops at the first byte that differs.
pub fn read_sync_code(rb: &mut BitReader<'_>) -> bool {
    SYNC_CODE.iter().all(|&b| rb.literal(8) == b)
}

/// A frame size: two 16-bit fields, each one less than the dimension.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "a 16-bit field plus one fits a u32"
)]
pub fn read_frame_size(rb: &mut BitReader<'_>) -> (u32, u32) {
    let w = rb.literal(16) + 1;
    let h = rb.literal(16) + 1;
    (w, h)
}

/// The colour format of a key frame or intra-only frame: libvpx's
/// `read_bitdepth_colorspace_sampling`.
pub fn read_color_config(rb: &mut BitReader<'_>, profile: u8) -> Result<ColorConfig, Error> {
    let bit_depth = if profile >= 2 {
        if rb.flag() { 12 } else { 10 }
    } else {
        8
    };
    let color_space = rb.literal(3) as u8;
    let mut c = ColorConfig {
        bit_depth,
        color_space,
        ..ColorConfig::default()
    };
    if color_space == CS_SRGB {
        c.full_range = true;
        if profile == 1 || profile == 3 {
            // sRGB means 4:4:4; nothing else is allowed.
            c.ss_x = 0;
            c.ss_y = 0;
            if rb.flag() {
                return Err(Error::Unsupported("a reserved bit is set"));
            }
        } else {
            return Err(Error::Unsupported("4:4:4 needs profile 1 or 3"));
        }
    } else {
        c.full_range = rb.flag();
        if profile == 1 || profile == 3 {
            c.ss_x = rb.bit() as u8;
            c.ss_y = rb.bit() as u8;
            if c.ss_x == 1 && c.ss_y == 1 {
                return Err(Error::Unsupported("4:2:0 is not allowed in profile 1 or 3"));
            }
            if rb.flag() {
                return Err(Error::Unsupported("a reserved bit is set"));
            }
        } else {
            c.ss_x = 1;
            c.ss_y = 1;
        }
    }
    Ok(c)
}

/// The frame-level interpolation filter: libvpx's `read_interp_filter`.
pub fn read_interp_filter(rb: &mut BitReader<'_>) -> InterpFilter {
    const LITERAL_TO_FILTER: [InterpFilter; 4] =
        [EIGHTTAP_SMOOTH, EIGHTTAP, EIGHTTAP_SHARP, BILINEAR];
    if rb.flag() {
        SWITCHABLE
    } else {
        LITERAL_TO_FILTER
            .get(rb.literal(2) as usize)
            .copied()
            .unwrap_or(EIGHTTAP)
    }
}

// --- Loop filter --------------------------------------------------------------

/// The loop filter's parameters: libvpx's `struct loopfilter`, less what it
/// derives from them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LoopFilterParams {
    /// 0 (off) to 63.
    pub filter_level: u8,
    /// 0 to 7.
    pub sharpness_level: u8,
    /// Whether the level is adjusted by reference frame and mode.
    pub mode_ref_delta_enabled: bool,
    /// Whether this frame's header changed the adjustments.
    pub mode_ref_delta_update: bool,
    /// The adjustment for intra, last, golden and altref blocks.
    pub ref_deltas: [i8; 4],
    /// The adjustment for zero-motion and other inter blocks.
    pub mode_deltas: [i8; 2],
}

impl LoopFilterParams {
    /// The deltas a key frame or error-resilient frame resets to: libvpx's
    /// `set_default_lf_deltas`.
    pub fn set_default_deltas(&mut self) {
        self.mode_ref_delta_enabled = true;
        self.mode_ref_delta_update = true;
        self.ref_deltas = [1, 0, -1, -1];
        self.mode_deltas = [0, 0];
    }

    /// Read the header's loop filter fields: libvpx's `setup_loopfilter`.
    /// Deltas the header leaves out keep their values from earlier frames.
    pub fn read(&mut self, rb: &mut BitReader<'_>) {
        self.filter_level = rb.literal(6) as u8;
        self.sharpness_level = rb.literal(3) as u8;
        self.mode_ref_delta_update = false;
        self.mode_ref_delta_enabled = rb.flag();
        if self.mode_ref_delta_enabled {
            self.mode_ref_delta_update = rb.flag();
            if self.mode_ref_delta_update {
                for d in &mut self.ref_deltas {
                    if rb.flag() {
                        *d = rb.signed_literal(6) as i8;
                    }
                }
                for d in &mut self.mode_deltas {
                    if rb.flag() {
                        *d = rb.signed_literal(6) as i8;
                    }
                }
            }
        }
    }

    /// The thresholds each filter level filters with at this frame's
    /// sharpness: libvpx's `update_sharpness`, with the high-variance
    /// thresholds `vp9_loop_filter_init` sets beside them.
    ///
    /// libvpx caches these and recomputes them when the sharpness changes;
    /// they depend on nothing else, so computing them for each frame gives
    /// the same numbers.
    #[allow(
        clippy::arithmetic_side_effects,
        reason = "lvl is at most 63 and sharpness at most 7, so every value fits a u8: mblim is at most 2 * 65 + 63"
    )]
    #[must_use]
    pub fn limits(&self) -> LimitTable {
        let sharpness = i32::from(self.sharpness_level);
        core::array::from_fn(|lvl| {
            let lvl = lvl as i32;
            let mut inside = lvl >> (i32::from(sharpness > 0) + i32::from(sharpness > 4));
            if sharpness > 0 && inside > 9 - sharpness {
                inside = 9 - sharpness;
            }
            inside = inside.max(1);
            FilterLimits {
                mblim: (2 * (lvl + 2) + inside) as u8,
                lim: inside as u8,
                hev_thr: (lvl >> 4) as u8,
            }
        })
    }

    /// The filter level of a block, by its segment, its reference frame
    /// (intra included) and whether it moves: libvpx's
    /// `vp9_loop_filter_frame_init`, the table `get_filter_level` reads.
    ///
    /// The second column of the intra row is never read -- an intra block's
    /// mode always selects the first -- and is left zero, as libvpx leaves
    /// it unwritten.
    #[must_use]
    pub fn levels(&self, seg: &Segmentation) -> LevelTable {
        let default = i32::from(self.filter_level);
        // Deltas count double for levels of 32 and up.
        let scale = 1i32 << (default >> 5);
        let clamp = |v: i32| v.clamp(0, MAX_LOOP_FILTER) as u8;
        let mut table = [[[0u8; MAX_MODE_LF_DELTAS]; MAX_REF_LF_DELTAS]; MAX_SEGMENTS];
        for (seg_id, levels) in (0u8..).zip(table.iter_mut()) {
            let mut lvl_seg = default;
            if seg.feature_active(seg_id, SEG_LVL_ALT_LF) {
                let data = seg.data(seg_id, SEG_LVL_ALT_LF);
                lvl_seg = clamp(if seg.abs_delta {
                    data
                } else {
                    default.saturating_add(data)
                })
                .into();
            }
            if !self.mode_ref_delta_enabled {
                *levels = [[clamp(lvl_seg); MAX_MODE_LF_DELTAS]; MAX_REF_LF_DELTAS];
                continue;
            }
            let delta = |d: i8| i32::from(d).saturating_mul(scale);
            if let (Some(intra), Some(&d)) = (levels.first_mut(), self.ref_deltas.first()) {
                intra[0] = clamp(lvl_seg.saturating_add(delta(d)));
            }
            for (by_mode, &ref_delta) in levels.iter_mut().zip(&self.ref_deltas).skip(1) {
                for (lvl, &mode_delta) in by_mode.iter_mut().zip(&self.mode_deltas) {
                    *lvl = clamp(
                        lvl_seg
                            .saturating_add(delta(ref_delta))
                            .saturating_add(delta(mode_delta)),
                    );
                }
            }
        }
        table
    }
}

/// How many reference-frame deltas there are: intra, last, golden, altref.
pub const MAX_REF_LF_DELTAS: usize = 4;
/// How many mode deltas there are: zero motion, and other inter modes.
pub const MAX_MODE_LF_DELTAS: usize = 2;

/// The thresholds one filter level filters an edge with: libvpx's
/// `loop_filter_thresh`, one value where libvpx repeats it across a SIMD
/// vector.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FilterLimits {
    /// The largest step across the edge that is still filtered.
    pub mblim: u8,
    /// The largest step between neighbours on either side.
    pub lim: u8,
    /// The step above which an edge has high variance and is filtered less.
    pub hev_thr: u8,
}

/// Thresholds for every filter level, 0 to 63.
pub type LimitTable = [FilterLimits; MAX_LOOP_FILTER as usize + 1];

/// Filter levels by segment, reference frame and mode class.
pub type LevelTable = [[[u8; MAX_MODE_LF_DELTAS]; MAX_REF_LF_DELTAS]; MAX_SEGMENTS];

/// The class of each mode for the loop filter's mode deltas: 1 for an inter
/// mode that moves, 0 for everything else. libvpx's `mode_lf_lut`.
pub const MODE_LF_LUT: [u8; MB_MODE_COUNT] = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 0, 1];

// --- Quantisers -----------------------------------------------------------------

/// The frame's quantiser index and its deltas: libvpx's `setup_quantization`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Quantization {
    pub base_qindex: i32,
    pub y_dc_delta_q: i32,
    pub uv_dc_delta_q: i32,
    pub uv_ac_delta_q: i32,
}

impl Quantization {
    /// Read the header's quantiser fields.
    pub fn read(rb: &mut BitReader<'_>) -> Self {
        let read_delta_q = |rb: &mut BitReader<'_>| {
            if rb.flag() { rb.signed_literal(4) } else { 0 }
        };
        let base_qindex = rb.literal(8) as i32;
        let y_dc_delta_q = read_delta_q(rb);
        let uv_dc_delta_q = read_delta_q(rb);
        let uv_ac_delta_q = read_delta_q(rb);
        Self {
            base_qindex,
            y_dc_delta_q,
            uv_dc_delta_q,
            uv_ac_delta_q,
        }
    }

    /// Whether the frame is lossless: every quantiser zero, so it codes with
    /// the Walsh-Hadamard transform.
    #[must_use]
    pub const fn lossless(&self) -> bool {
        self.base_qindex == 0
            && self.y_dc_delta_q == 0
            && self.uv_dc_delta_q == 0
            && self.uv_ac_delta_q == 0
    }
}

/// `qindex + delta` clamped to the quantiser range, as an index.
fn q_index(qindex: i32, delta: i32) -> usize {
    qindex.saturating_add(delta).clamp(0, MAXQ) as usize
}

/// The DC dequantiser: libvpx's `vp9_dc_quant`.
#[must_use]
pub fn dc_quant(qindex: i32, delta: i32, bit_depth: u8) -> i16 {
    let i = q_index(qindex, delta);
    let table = match bit_depth {
        10 => &tables::DC_QLOOKUP_10,
        12 => &tables::DC_QLOOKUP_12,
        _ => &tables::DC_QLOOKUP,
    };
    table.get(i).copied().unwrap_or(0)
}

/// The AC dequantiser: libvpx's `vp9_ac_quant`.
#[must_use]
pub fn ac_quant(qindex: i32, delta: i32, bit_depth: u8) -> i16 {
    let i = q_index(qindex, delta);
    let table = match bit_depth {
        10 => &tables::AC_QLOOKUP_10,
        12 => &tables::AC_QLOOKUP_12,
        _ => &tables::AC_QLOOKUP,
    };
    table.get(i).copied().unwrap_or(0)
}

// --- Segmentation -----------------------------------------------------------------

/// How many probabilities code a segment id.
pub const SEG_TREE_PROBS: usize = MAX_SEGMENTS - 1;
/// How many probabilities code a predicted-segment flag.
pub const PREDICTION_PROBS: usize = 3;

/// Which segment features take a sign: quantiser and loop filter.
const SEG_FEATURE_DATA_SIGNED: [bool; SEG_LVL_MAX] = [true, true, false, false];
/// The largest value of each segment feature.
const SEG_FEATURE_DATA_MAX: [i32; SEG_LVL_MAX] = [MAXQ, MAX_LOOP_FILTER, 3, 0];

/// A frame's segmentation: libvpx's `struct segmentation`. It persists from
/// frame to frame; each header says what it changes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Segmentation {
    pub enabled: bool,
    /// Whether this frame codes its blocks' segment ids.
    pub update_map: bool,
    /// Whether this frame's header changed the features.
    pub update_data: bool,
    /// Whether feature values replace the frame's, rather than adjust them.
    pub abs_delta: bool,
    /// Whether a segment id may be predicted from the previous frame's.
    pub temporal_update: bool,
    pub tree_probs: [u8; SEG_TREE_PROBS],
    pub pred_probs: [u8; PREDICTION_PROBS],
    pub feature_data: [[i16; SEG_LVL_MAX]; MAX_SEGMENTS],
    /// Per segment, a bit per enabled feature.
    pub feature_mask: [u8; MAX_SEGMENTS],
}

impl Segmentation {
    /// Disable every feature: libvpx's `vp9_clearall_segfeatures`.
    pub fn clear_all_features(&mut self) {
        self.feature_data = [[0; SEG_LVL_MAX]; MAX_SEGMENTS];
        self.feature_mask = [0; MAX_SEGMENTS];
    }

    /// Whether `feature` applies to `segment_id`: libvpx's
    /// `segfeature_active`.
    #[must_use]
    pub fn feature_active(&self, segment_id: u8, feature: usize) -> bool {
        self.enabled
            && self
                .feature_mask
                .get(usize::from(segment_id))
                .is_some_and(|m| m & (1 << feature) != 0)
    }

    /// A feature's value for a segment: libvpx's `get_segdata`.
    #[must_use]
    pub fn data(&self, segment_id: u8, feature: usize) -> i32 {
        self.feature_data
            .get(usize::from(segment_id))
            .and_then(|d| d.get(feature))
            .map_or(0, |&v| i32::from(v))
    }

    /// A segment's quantiser index: libvpx's `vp9_get_qindex`.
    #[must_use]
    pub fn qindex(&self, segment_id: u8, base_qindex: i32) -> i32 {
        if self.feature_active(segment_id, SEG_LVL_ALT_Q) {
            let data = self.data(segment_id, SEG_LVL_ALT_Q);
            let q = if self.abs_delta {
                data
            } else {
                base_qindex.saturating_add(data)
            };
            q.clamp(0, MAXQ)
        } else {
            base_qindex
        }
    }

    /// Read the header's segmentation fields: libvpx's `setup_segmentation`.
    pub fn read(&mut self, rb: &mut BitReader<'_>) {
        self.update_map = false;
        self.update_data = false;
        self.enabled = rb.flag();
        if !self.enabled {
            return;
        }
        let read_prob = |rb: &mut BitReader<'_>| {
            if rb.flag() { rb.literal(8) as u8 } else { 255 }
        };
        self.update_map = rb.flag();
        if self.update_map {
            for p in &mut self.tree_probs {
                *p = read_prob(rb);
            }
            self.temporal_update = rb.flag();
            if self.temporal_update {
                for p in &mut self.pred_probs {
                    *p = read_prob(rb);
                }
            } else {
                self.pred_probs = [255; PREDICTION_PROBS];
            }
        }
        self.update_data = rb.flag();
        if self.update_data {
            self.abs_delta = rb.flag();
            self.clear_all_features();
            for (mask, data) in self.feature_mask.iter_mut().zip(&mut self.feature_data) {
                for (j, (d, (&max, &signed))) in data
                    .iter_mut()
                    .zip(SEG_FEATURE_DATA_MAX.iter().zip(&SEG_FEATURE_DATA_SIGNED))
                    .enumerate()
                {
                    let mut value = 0;
                    if rb.flag() {
                        *mask |= 1 << j;
                        value = decode_unsigned_max(rb, max);
                        if signed && rb.flag() {
                            value = value.wrapping_neg();
                        }
                    }
                    *d = value as i16;
                }
            }
        }
    }
}

/// A value of as many bits as `max` needs, capped at `max`: libvpx's
/// `decode_unsigned_max`.
fn decode_unsigned_max(rb: &mut BitReader<'_>, max: i32) -> i32 {
    // get_unsigned_bits: the bits in max's binary representation.
    let bits = u32::BITS.saturating_sub(max.max(0).leading_zeros());
    (rb.literal(bits) as i32).min(max)
}

// --- Tiles ------------------------------------------------------------------------

/// The narrowest a tile column may be, in superblocks.
const MIN_TILE_WIDTH_B64: u32 = 4;
/// The widest, in superblocks.
const MAX_TILE_WIDTH_B64: u32 = 64;

/// Superblock columns spanned by `mi_cols` mode-info columns.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "mi_cols is at most 8192, so rounding it up cannot overflow"
)]
const fn sb64_cols(mi_cols: u32) -> u32 {
    (mi_cols + 7) >> MI_BLOCK_SIZE_LOG2
}

/// The range of tile-column counts (log2) a frame of `mi_cols` may use:
/// libvpx's `vp9_get_tile_n_bits`.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "the loops stop well before their shifts or counts can overflow: sb64 is at most 1024"
)]
#[must_use]
pub fn tile_n_bits(mi_cols: u32) -> (u32, u32) {
    let sb64 = sb64_cols(mi_cols);
    let mut min_log2 = 0;
    while (MAX_TILE_WIDTH_B64 << min_log2) < sb64 {
        min_log2 += 1;
    }
    let mut max_log2 = 1;
    while (sb64 >> max_log2) >= MIN_TILE_WIDTH_B64 {
        max_log2 += 1;
    }
    (min_log2, max_log2 - 1)
}

/// Read the tile counts: libvpx's `setup_tile_info`. Returns the log2 of the
/// tile columns and rows.
pub fn read_tile_info(rb: &mut BitReader<'_>, mi_cols: u32) -> Result<(u32, u32), Error> {
    let (min_log2, max_log2) = tile_n_bits(mi_cols);
    let mut log2_cols = min_log2;
    let mut max_ones = max_log2.saturating_sub(min_log2);
    while max_ones > 0 && rb.flag() {
        max_ones = max_ones.saturating_sub(1);
        log2_cols = log2_cols.saturating_add(1);
    }
    if log2_cols > 6 {
        return Err(Error::Corrupt("too many tile columns"));
    }
    let mut log2_rows = rb.bit();
    if log2_rows != 0 {
        log2_rows = log2_rows.saturating_add(rb.bit());
    }
    Ok((log2_cols, log2_rows))
}

/// Where tile `idx` of `1 << log2` starts, in mode-info units, across `mis`
/// of them: libvpx's `get_tile_offset`.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "idx is at most 64 and sb64 at most 1024, so the product fits"
)]
#[must_use]
pub fn tile_offset(idx: u32, mis: u32, log2: u32) -> u32 {
    let sb_cols = sb64_cols(mis);
    let offset = ((idx * sb_cols) >> log2) << MI_BLOCK_SIZE_LOG2;
    offset.min(mis)
}

// --- Superframes --------------------------------------------------------------------

/// The sizes of the frames a superframe packs together: libvpx's
/// `vp9_parse_superframe_index`. Empty when `data` has no index.
///
/// An index is a marker byte `0b110mmfff` (`mm` + 1 bytes per size, `fff` + 1
/// frames) at both ends of a run of little-endian sizes at the very end of
/// the packet.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "mag is at most 4 and frames at most 8, so the index is at most 34 bytes; start is below the length, which is at least 1"
)]
pub fn parse_superframe_index(data: &[u8]) -> Result<Vec<u32>, Error> {
    let Some(&marker) = data.last() else {
        return Ok(Vec::new());
    };
    if marker & 0xe0 != 0xc0 {
        return Ok(Vec::new());
    }
    let frames = usize::from(marker & 0x7) + 1;
    let mag = usize::from((marker >> 3) & 0x3) + 1;
    let index_sz = 2 + mag * frames;
    let Some(start) = data.len().checked_sub(index_sz) else {
        return Err(Error::Corrupt(
            "a superframe index is longer than its packet",
        ));
    };
    if data.get(start) != Some(&marker) {
        return Err(Error::Corrupt("a superframe index's markers differ"));
    }
    let sizes = data
        .get(start + 1..data.len() - 1)
        .unwrap_or(&[])
        .chunks_exact(mag)
        .map(|bytes| {
            bytes
                .iter()
                .rev()
                .fold(0u32, |acc, &b| (acc << 8) | u32::from(b))
        })
        .collect();
    Ok(sizes)
}

/// What the start of a frame says without decoding it: libvpx's
/// `decoder_peek_si_internal`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StreamInfo {
    /// Whether the frame is a key frame.
    pub is_kf: bool,
    /// Whether it is an intra-only frame.
    pub intra_only: bool,
    /// Its size, if it declares one (key and intra-only frames do).
    pub width: u32,
    pub height: u32,
}

/// Step over a colour format without checking it as the decoder does: only
/// that sRGB comes with a profile that allows it. libvpx's
/// `parse_bitdepth_colorspace_sampling`, the peek's laxer twin of
/// [`read_color_config`].
fn skip_color_config(rb: &mut BitReader<'_>, profile: u8) -> bool {
    if profile >= 2 {
        rb.bit(); // 10 or 12 bits
    }
    if rb.literal(3) as u8 == CS_SRGB {
        if profile == 1 || profile == 3 {
            rb.bit(); // unused
            true
        } else {
            false
        }
    } else {
        rb.bit(); // range
        if profile == 1 || profile == 3 {
            rb.literal(3); // subsampling x and y, unused
        }
        true
    }
}

/// Peek at a frame: libvpx's `decoder_peek_si_internal`.
pub fn peek_stream_info(data: &[u8]) -> Result<StreamInfo, Error> {
    let unsupported = Err(Error::Unsupported("not the start of a VP9 frame"));
    if data.is_empty() {
        return unsupported;
    }
    let mut rb = BitReader::new(data);
    let frame_marker = rb.literal(2);
    let profile = read_profile(&mut rb);
    if frame_marker != FRAME_MARKER || profile >= MAX_PROFILES {
        return unsupported;
    }
    let mut si = StreamInfo::default();
    if rb.flag() {
        // Show an existing frame: nothing more to learn.
        if profile > 2 && data.len() < 2 {
            return unsupported;
        }
        return Ok(si);
    }
    if data.len() < 10 {
        return unsupported;
    }
    si.is_kf = !rb.flag();
    let show_frame = rb.flag();
    let error_resilient = rb.flag();
    if si.is_kf {
        if !read_sync_code(&mut rb) || !skip_color_config(&mut rb, profile) {
            return unsupported;
        }
        (si.width, si.height) = read_frame_size(&mut rb);
    } else {
        si.intra_only = if show_frame { false } else { rb.flag() };
        if !error_resilient {
            rb.literal(2); // reset_frame_context
        }
        if si.intra_only {
            if !read_sync_code(&mut rb) {
                return unsupported;
            }
            if profile > 0 {
                if !skip_color_config(&mut rb, profile) {
                    return unsupported;
                }
                if data.len() < 11 {
                    return unsupported;
                }
            }
            rb.literal(8); // refresh_frame_flags
            (si.width, si.height) = read_frame_size(&mut rb);
        }
    }
    Ok(si)
}

/// Whether a reference frame of `ref_w` by `ref_h` may predict one of
/// `this_w` by `this_h`: at most twice as large, at most sixteen times
/// smaller. libvpx's `valid_ref_frame_size`.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "u32 dimensions widened to u64 cannot overflow when doubled or multiplied by 16"
)]
#[must_use]
pub fn valid_ref_frame_size(ref_w: u32, ref_h: u32, this_w: u32, this_h: u32) -> bool {
    let (rw, rh, tw, th) = (
        u64::from(ref_w),
        u64::from(ref_h),
        u64::from(this_w),
        u64::from(this_h),
    );
    2 * tw >= rw && 2 * th >= rh && tw <= 16 * rw && th <= 16 * rh
}

/// The frame marker check, for the full header parser.
#[must_use]
pub fn frame_marker_ok(marker: u32) -> bool {
    marker == FRAME_MARKER
}

// --- The compressed header ------------------------------------------------------------

/// The frame's transform mode: libvpx's `read_tx_mode`.
pub fn read_tx_mode(r: &mut BoolReader<'_>) -> TxMode {
    let mut tx_mode = r.read_literal(2) as TxMode;
    if tx_mode == ALLOW_32X32 {
        tx_mode = tx_mode.saturating_add(r.read_bit() as TxMode);
    }
    tx_mode
}

/// Transform size probability updates: libvpx's `read_tx_mode_probs`.
pub fn read_tx_mode_probs(fc: &mut FrameContext, r: &mut BoolReader<'_>) {
    for ctx in &mut fc.tx_probs.p8x8[..TX_SIZE_CONTEXTS] {
        for p in ctx {
            diff_update_prob(r, p);
        }
    }
    for ctx in &mut fc.tx_probs.p16x16[..TX_SIZE_CONTEXTS] {
        for p in ctx {
            diff_update_prob(r, p);
        }
    }
    for ctx in &mut fc.tx_probs.p32x32[..TX_SIZE_CONTEXTS] {
        for p in ctx {
            diff_update_prob(r, p);
        }
    }
}

/// Coefficient probability updates for every transform size the mode
/// allows: libvpx's `read_coef_probs`.
pub fn read_coef_probs(fc: &mut FrameContext, tx_mode: TxMode, r: &mut BoolReader<'_>) {
    let max_tx = tables::TX_MODE_TO_BIGGEST_TX_SIZE
        .get(usize::from(tx_mode))
        .copied()
        .unwrap_or(0);
    for probs in fc.coef_probs.iter_mut().take(usize::from(max_tx) + 1) {
        if r.read_bit() == 0 {
            continue;
        }
        for plane in probs.iter_mut().take(PLANE_TYPES) {
            for refs in plane.iter_mut().take(REF_TYPES) {
                for (band, ctxs) in refs.iter_mut().enumerate().take(COEF_BANDS) {
                    for ctx in ctxs.iter_mut().take(band_coeff_contexts(band)) {
                        for p in ctx.iter_mut().take(UNCONSTRAINED_NODES) {
                            diff_update_prob(r, p);
                        }
                    }
                }
            }
        }
    }
}

/// Skip probability updates.
pub fn read_skip_probs(fc: &mut FrameContext, r: &mut BoolReader<'_>) {
    for p in &mut fc.skip_probs {
        diff_update_prob(r, p);
    }
}

/// Inter mode probability updates: libvpx's `read_inter_mode_probs`.
pub fn read_inter_mode_probs(fc: &mut FrameContext, r: &mut BoolReader<'_>) {
    for ctx in &mut fc.inter_mode_probs[..INTER_MODE_CONTEXTS] {
        for p in ctx {
            diff_update_prob(r, p);
        }
    }
}

/// Interpolation filter probability updates: libvpx's
/// `read_switchable_interp_probs`.
pub fn read_switchable_interp_probs(fc: &mut FrameContext, r: &mut BoolReader<'_>) {
    for ctx in &mut fc.switchable_interp_prob[..SWITCHABLE_FILTER_CONTEXTS] {
        for p in &mut ctx[..SWITCHABLE_FILTERS - 1] {
            diff_update_prob(r, p);
        }
    }
}

/// Intra-or-inter probability updates.
pub fn read_intra_inter_probs(fc: &mut FrameContext, r: &mut BoolReader<'_>) {
    for p in &mut fc.intra_inter_prob[..INTRA_INTER_CONTEXTS] {
        diff_update_prob(r, p);
    }
}

/// Whether the frame's references face both ways in time, which compound
/// prediction needs: libvpx's `vp9_compound_reference_allowed`.
#[must_use]
pub fn compound_reference_allowed(sign_bias: &[bool; 4]) -> bool {
    sign_bias[2] != sign_bias[1] || sign_bias[3] != sign_bias[1]
}

/// The frame's reference mode: libvpx's `read_frame_reference_mode`.
pub fn read_frame_reference_mode(sign_bias: &[bool; 4], r: &mut BoolReader<'_>) -> ReferenceMode {
    if compound_reference_allowed(sign_bias) {
        if r.read_bit() == 1 {
            if r.read_bit() == 1 {
                REFERENCE_MODE_SELECT
            } else {
                COMPOUND_REFERENCE
            }
        } else {
            SINGLE_REFERENCE
        }
    } else {
        SINGLE_REFERENCE
    }
}

/// Which reference every compound block uses, and the two it chooses between:
/// libvpx's `vp9_setup_compound_reference_mode`.
#[must_use]
pub fn compound_references(sign_bias: &[bool; 4]) -> (RefFrame, [RefFrame; 2]) {
    let b = |f: RefFrame| sign_bias.get(f as usize).copied().unwrap_or(false);
    if b(LAST_FRAME) == b(GOLDEN_FRAME) {
        (ALTREF_FRAME, [LAST_FRAME, GOLDEN_FRAME])
    } else if b(LAST_FRAME) == b(ALTREF_FRAME) {
        (GOLDEN_FRAME, [LAST_FRAME, ALTREF_FRAME])
    } else {
        (LAST_FRAME, [GOLDEN_FRAME, ALTREF_FRAME])
    }
}

/// Reference probability updates: libvpx's `read_frame_reference_mode_probs`.
pub fn read_frame_reference_mode_probs(
    fc: &mut FrameContext,
    reference_mode: ReferenceMode,
    r: &mut BoolReader<'_>,
) {
    if reference_mode == REFERENCE_MODE_SELECT {
        for p in &mut fc.comp_inter_prob[..COMP_INTER_CONTEXTS] {
            diff_update_prob(r, p);
        }
    }
    if reference_mode != COMPOUND_REFERENCE {
        for ctx in &mut fc.single_ref_prob[..REF_CONTEXTS] {
            for p in ctx {
                diff_update_prob(r, p);
            }
        }
    }
    if reference_mode != SINGLE_REFERENCE {
        for p in &mut fc.comp_ref_prob[..REF_CONTEXTS] {
            diff_update_prob(r, p);
        }
    }
}

/// Luma mode probability updates, by size group.
pub fn read_y_mode_probs(fc: &mut FrameContext, r: &mut BoolReader<'_>) {
    for group in &mut fc.y_mode_prob[..BLOCK_SIZE_GROUPS] {
        for p in &mut group[..INTRA_MODES - 1] {
            diff_update_prob(r, p);
        }
    }
}

/// Partition probability updates.
pub fn read_partition_probs(fc: &mut FrameContext, r: &mut BoolReader<'_>) {
    for ctx in &mut fc.partition_prob[..PARTITION_CONTEXTS] {
        for p in &mut ctx[..PARTITION_TYPES - 1] {
            diff_update_prob(r, p);
        }
    }
}

/// Motion vector probability updates: libvpx's `read_mv_probs`.
pub fn read_mv_probs(fc: &mut FrameContext, allow_hp: bool, r: &mut BoolReader<'_>) {
    let ctx = &mut fc.nmvc;
    update_mv_probs(r, &mut ctx.joints[..MV_JOINTS - 1]);
    for comp in &mut ctx.comps {
        update_mv_probs(r, core::slice::from_mut(&mut comp.sign));
        update_mv_probs(r, &mut comp.classes[..MV_CLASSES - 1]);
        update_mv_probs(r, &mut comp.class0[..CLASS0_SIZE - 1]);
        update_mv_probs(r, &mut comp.bits[..MV_OFFSET_BITS]);
    }
    for comp in &mut ctx.comps {
        for fp in &mut comp.class0_fp[..CLASS0_SIZE] {
            update_mv_probs(r, &mut fp[..MV_FP_SIZE - 1]);
        }
        update_mv_probs(r, &mut comp.fp[..3]);
    }
    if allow_hp {
        for comp in &mut ctx.comps {
            update_mv_probs(r, core::slice::from_mut(&mut comp.class0_hp));
            update_mv_probs(r, core::slice::from_mut(&mut comp.hp));
        }
    }
}

/// Whether `mode` lets each block choose its transform size.
#[must_use]
pub const fn is_tx_mode_select(mode: TxMode) -> bool {
    mode == TX_MODE_SELECT
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use super::*;

    #[test]
    fn a_superframe_index_lists_its_frames_sizes() {
        // Two frames, two-byte sizes: marker 0b110_01_001 = 0xc9.
        let mut data = vec![0u8; 0x0102 + 0x0003];
        data.extend_from_slice(&[0xc9, 0x02, 0x01, 0x03, 0x00, 0xc9]);
        assert_eq!(parse_superframe_index(&data).unwrap(), vec![0x0102, 0x0003]);
    }

    #[test]
    fn a_packet_without_an_index_has_none() {
        assert!(
            parse_superframe_index(&[0x82, 0x49, 0x83])
                .unwrap()
                .is_empty()
        );
        assert!(parse_superframe_index(&[]).unwrap().is_empty());
    }

    #[test]
    fn a_damaged_index_is_corrupt() {
        // Marker at the end but not at the front of the index.
        assert!(parse_superframe_index(&[0x00, 0x05, 0xc0]).is_err());
        // Longer than the packet.
        assert!(parse_superframe_index(&[0xc7]).is_err());
    }

    #[test]
    fn tile_bits_match_libvpx_at_the_edges() {
        // One superblock: one tile column, no choice.
        assert_eq!(tile_n_bits(1), (0, 0));
        // 1920 wide: 240 mi columns, 30 superblocks: up to 4 tile columns.
        assert_eq!(tile_n_bits(240), (0, 2));
        // 65536 wide: 8192 mi columns, 1024 superblocks.
        assert_eq!(tile_n_bits(8192), (4, 8));
    }

    #[test]
    fn tile_offsets_split_superblocks_evenly_and_stop_at_the_edge() {
        // 30 superblocks in 4 columns: 0, 7, 15, 22 superblocks.
        let starts: Vec<u32> = (0..=4).map(|i| tile_offset(i, 240, 2)).collect();
        assert_eq!(starts, vec![0, 56, 120, 176, 240]);
        // The last tile ends at the frame, not at a whole superblock.
        assert_eq!(tile_offset(1, 13, 0), 13);
    }

    #[test]
    fn segment_qindex_adds_or_replaces() {
        let mut seg = Segmentation {
            enabled: true,
            ..Segmentation::default()
        };
        seg.feature_mask[2] = 1 << SEG_LVL_ALT_Q;
        seg.feature_data[2][SEG_LVL_ALT_Q] = -40;
        assert_eq!(seg.qindex(2, 100), 60);
        assert_eq!(seg.qindex(2, 10), 0, "clamped");
        assert_eq!(seg.qindex(1, 100), 100, "feature off for segment 1");
        seg.abs_delta = true;
        seg.feature_data[2][SEG_LVL_ALT_Q] = 77;
        assert_eq!(seg.qindex(2, 100), 77);
        seg.enabled = false;
        assert_eq!(seg.qindex(2, 100), 100, "segmentation off");
    }

    #[test]
    fn filter_limits_follow_libvpx_update_sharpness() {
        let mut lf = LoopFilterParams::default();
        let t = lf.limits();
        // Sharpness 0: the inside limit is the level, at least 1.
        assert_eq!(
            t[0],
            FilterLimits {
                mblim: 5,
                lim: 1,
                hev_thr: 0
            }
        );
        assert_eq!(
            t[40],
            FilterLimits {
                mblim: 124,
                lim: 40,
                hev_thr: 2
            }
        );
        assert_eq!(
            t[63],
            FilterLimits {
                mblim: 193,
                lim: 63,
                hev_thr: 3
            }
        );
        // Sharpness 5 shifts the level down by two and caps it at 9 - 5.
        lf.sharpness_level = 5;
        let t = lf.limits();
        assert_eq!(t[40].lim, 4);
        assert_eq!(t[8].lim, 2);
        assert_eq!(t[2].lim, 1, "at least 1");
        assert_eq!(t[40].mblim, 2 * 42 + 4);
        // Sharpness 1 shifts by one and caps at 8.
        lf.sharpness_level = 1;
        assert_eq!(lf.limits()[10].lim, 5);
        assert_eq!(lf.limits()[40].lim, 8);
    }

    #[test]
    fn filter_levels_apply_deltas_scaled_by_level() {
        let mut lf = LoopFilterParams {
            filter_level: 20,
            ..LoopFilterParams::default()
        };
        lf.set_default_deltas();
        lf.mode_deltas = [0, 3];
        let seg = Segmentation::default();
        let t = lf.levels(&seg);
        // Intra +1, last 0, golden and altref -1; moving modes +3.
        assert_eq!(t[0][0][0], 21);
        assert_eq!(t[0][1], [20, 23]);
        assert_eq!(t[0][3], [19, 22]);
        // From 32 up the deltas count double.
        lf.filter_level = 40;
        let t = lf.levels(&seg);
        assert_eq!(t[5][0][0], 42);
        assert_eq!(t[5][2], [38, 44]);
        // Clamped to the filter's range.
        lf.filter_level = 63;
        assert_eq!(lf.levels(&seg)[0][1][1], 63);
    }

    #[test]
    fn filter_levels_follow_segments_and_can_ignore_deltas() {
        let mut lf = LoopFilterParams {
            filter_level: 30,
            ..LoopFilterParams::default()
        };
        lf.set_default_deltas();
        let mut seg = Segmentation {
            enabled: true,
            ..Segmentation::default()
        };
        seg.feature_mask[3] = 1 << SEG_LVL_ALT_LF;
        seg.feature_data[3][SEG_LVL_ALT_LF] = -10;
        let t = lf.levels(&seg);
        assert_eq!(t[3][1], [20, 20], "segment 3 adds -10");
        assert_eq!(t[2][1], [30, 30]);
        seg.abs_delta = true;
        seg.feature_data[3][SEG_LVL_ALT_LF] = 7;
        assert_eq!(lf.levels(&seg)[3][0][0], 8, "absolute 7, intra +1");
        lf.mode_ref_delta_enabled = false;
        let t = lf.levels(&seg);
        assert_eq!(
            t[3],
            [[7, 7]; 4],
            "no deltas: every entry the segment level"
        );
        assert_eq!(t[0], [[30, 30]; 4]);
    }

    #[test]
    fn mode_classes_mark_the_moving_inter_modes() {
        use crate::common::{DC_PRED, NEARESTMV, NEARMV, NEWMV, TM_PRED, ZEROMV};
        assert_eq!(MODE_LF_LUT[usize::from(DC_PRED)], 0);
        assert_eq!(MODE_LF_LUT[usize::from(TM_PRED)], 0);
        assert_eq!(MODE_LF_LUT[usize::from(ZEROMV)], 0);
        for m in [NEARESTMV, NEARMV, NEWMV] {
            assert_eq!(MODE_LF_LUT[usize::from(m)], 1);
        }
    }

    #[test]
    fn quantisers_index_their_tables_clamped() {
        assert_eq!(dc_quant(0, 0, 8), 4);
        assert_eq!(dc_quant(0, -5, 8), 4, "clamped below");
        assert_eq!(ac_quant(255, 10, 8), 1828, "clamped above");
        assert_eq!(ac_quant(255, 0, 10), 7312);
        assert_eq!(ac_quant(255, 0, 12), 29247);
    }

    #[test]
    fn compound_references_follow_the_sign_biases() {
        // Golden and last forward, altref backward.
        let (fixed, var) = compound_references(&[false, false, false, true]);
        assert_eq!((fixed, var), (ALTREF_FRAME, [LAST_FRAME, GOLDEN_FRAME]));
        assert!(compound_reference_allowed(&[false, false, false, true]));
        assert!(!compound_reference_allowed(&[false, true, true, true]));
        let (fixed, var) = compound_references(&[false, false, true, false]);
        assert_eq!((fixed, var), (GOLDEN_FRAME, [LAST_FRAME, ALTREF_FRAME]));
        let (fixed, var) = compound_references(&[false, true, false, false]);
        assert_eq!((fixed, var), (LAST_FRAME, [GOLDEN_FRAME, ALTREF_FRAME]));
    }

    #[test]
    fn decode_unsigned_max_caps_its_value() {
        // 8 bits of 0xFF against a max of 63 needs only 6 bits: 0b111111.
        let mut rb = BitReader::new(&[0xFF]);
        assert_eq!(decode_unsigned_max(&mut rb, 63), 63);
        // A max of 3 reads two bits.
        let mut rb = BitReader::new(&[0b1100_0000]);
        assert_eq!(decode_unsigned_max(&mut rb, 3), 3);
        // A max of 0 reads none.
        let mut rb = BitReader::new(&[0xFF]);
        assert_eq!(decode_unsigned_max(&mut rb, 0), 0);
        assert_eq!(rb.bytes_read(), 0);
    }

    #[test]
    fn segmentation_reads_features_with_signs() {
        // enabled, update_map 0, update_data 1, abs_delta 0, then for
        // segment 0: ALT_Q on, value 5 (8 bits), sign 1; the other 31
        // feature flags off.
        let mut bits = String::from("1" /*enabled*/);
        bits += "0"; // update_map
        bits += "1"; // update_data
        bits += "0"; // abs_delta
        bits += "1"; // ALT_Q enabled
        bits += "00000101"; // 5
        bits += "1"; // negative
        bits += &"0".repeat(31);
        let bytes: Vec<u8> = bits
            .as_bytes()
            .chunks(8)
            .map(|c| {
                let mut b = 0u8;
                for (i, &ch) in c.iter().enumerate() {
                    if ch == b'1' {
                        b |= 0x80 >> i;
                    }
                }
                b
            })
            .collect();
        let mut seg = Segmentation::default();
        let mut rb = BitReader::new(&bytes);
        seg.read(&mut rb);
        assert!(seg.enabled && !seg.update_map && seg.update_data);
        assert_eq!(seg.feature_mask[0], 1);
        assert_eq!(seg.feature_data[0][SEG_LVL_ALT_Q], -5);
        assert_eq!(seg.feature_mask[1..], [0; 7]);
        assert!(!rb.overran());
    }

    #[test]
    fn peeking_a_key_frame_finds_its_size() {
        // A key frame: marker 10, profile 0 (two zero bits), show_existing 0,
        // frame_type 0, show 1, error_res 0, sync code, colour space 0
        // (3 bits), range 0, then 16-bit width-1 and height-1.
        let mut bits = String::from("10" /*marker*/);
        bits += "00"; // profile 0
        bits += "0"; // show_existing_frame
        bits += "0"; // key frame
        bits += "1"; // show_frame
        bits += "0"; // error_resilient
        for b in SYNC_CODE {
            bits += &format!("{b:08b}");
        }
        bits += "000"; // colour space
        bits += "0"; // colour range
        bits += &format!("{:016b}", 351);
        bits += &format!("{:016b}", 287);
        while bits.len() % 8 != 0 {
            bits.push('0');
        }
        let mut bytes: Vec<u8> = (0..bits.len() / 8)
            .map(|i| u8::from_str_radix(&bits[i * 8..i * 8 + 8], 2).unwrap())
            .collect();
        // libvpx's peek refuses a frame shorter than ten bytes, as this one
        // (sixty-eight bits) is: it cannot be a whole frame.
        assert!(peek_stream_info(&bytes).is_err());
        bytes.resize(10, 0);
        let si = peek_stream_info(&bytes).unwrap();
        assert!(si.is_kf);
        assert_eq!((si.width, si.height), (352, 288));
    }
}
