//! WebP lossy (`VP8 `): a VP8 key frame, RFC 6386.
//!
//! A lossy WebP is one intra-coded frame of the VP8 video format. The frame
//! is cut into 16x16 macroblocks, each predicted from the reconstructed
//! samples above it and to its left -- as a whole, or luma as sixteen 4x4
//! subblocks -- and corrected by a residual: dequantised coefficients through
//! an inverse DCT, with the luma DCs of a whole-predicted macroblock carried
//! by one more, Walsh-Hadamard, block. A loop filter then smooths the seams
//! the blocks leave, and the result, Y'CbCr with half-resolution chroma, is
//! converted to RGB.
//!
//! Everything up to the conversion is specified to the bit, so every correct
//! decoder produces the same planes; the conversion is libwebp's, the decoder
//! behind every browser that shows WebP, so the pixels are the ones people
//! see (`lossy/yuv.rs`). Where libwebp and the RFC's reference decoder part
//! company -- only on streams no encoder writes -- this follows libwebp, and
//! says so where it does.
//!
//! # Layout
//!
//! The bitstream is a ten-byte uncompressed header, then one partition of
//! entropy-coded header fields and per-macroblock modes, then one to eight
//! partitions of coefficients, macroblock row `r` in partition `r % count`.
//! Each partition is decoded by its own boolean decoder (`lossy/reader.rs`).
//!
//! Prediction reads the frame before loop filtering. So a row of macroblocks
//! is filtered only once the row below it has been reconstructed, and
//! filtering a row changes nothing the next row's prediction reads: its
//! filter reaches three samples into the row above, never down.
//!
//! # Hostile input
//!
//! The frame's size is bounded by [`Limits`] before its planes are allocated,
//! and nothing else is sized from the stream. Every partition length is
//! checked against the bytes present. A partition that runs dry mid-frame is
//! a truncated file ([`ImageError::Truncated`]), detected where libwebp
//! detects it. Coefficients from a crafted stream can overflow the sixteen
//! bits libwebp keeps them in; they wrap here as they do there
//! (`lossy/transform.rs`), and no value from the stream is ever an index
//! without a bound.

use alloc::vec;
use alloc::vec::Vec;

use crate::{ImageError, ImageResult, Limits};

mod filter;
mod predict;
mod reader;
mod tables;
mod transform;
mod yuv;

use filter::{Kind, Strength};
use predict::{DC_PRED, Edges, H_PRED, TM_PRED, V_PRED};
use reader::Reader;
use tables::{AC_QUANT, COEFF_UPDATE_PROBS, DC_QUANT, DEFAULT_COEFF_PROBS, KF_BMODE_PROBS};

/// The uncompressed part of a key frame's header (RFC 6386 §9.1): a three-byte
/// frame tag, the start code, and the size.
const TAG_LEN: usize = 10;

/// A key frame's uncompressed header.
struct Tag {
    width: usize,
    height: usize,
    /// Bytes in the first partition, which follows the tag.
    first_partition: usize,
}

/// Read and check a frame's tag, as libwebp's `VP8GetInfo` does: a key frame,
/// of a known profile, meant to be shown, whose first partition is shorter
/// than the `declared` length of its chunk, and of some size.
fn read_tag(data: &[u8], declared: usize) -> ImageResult<Tag> {
    let tag: &[u8; TAG_LEN] = data
        .get(..TAG_LEN)
        .and_then(|t| t.try_into().ok())
        .ok_or(ImageError::Truncated)?;
    let [b0, b1, b2, s0, s1, s2, w0, w1, h0, h1] = *tag;
    if [s0, s1, s2] != [0x9D, 0x01, 0x2A] {
        return Err(ImageError::Malformed("a VP8 frame without its start code"));
    }
    let bits = u32::from_le_bytes([b0, b1, b2, 0]);
    if bits & 1 != 0 {
        return Err(ImageError::Malformed("a VP8 frame that is not a key frame"));
    }
    if (bits >> 1) & 7 > 3 {
        return Err(ImageError::Malformed("a VP8 frame of an unknown profile"));
    }
    if (bits >> 4) & 1 == 0 {
        return Err(ImageError::Malformed("a VP8 frame marked not to be shown"));
    }
    let first_partition = (bits >> 5) as usize;
    if first_partition >= declared {
        return Err(ImageError::Malformed(
            "a VP8 first partition longer than its frame",
        ));
    }
    // The top two bits of each are an upscaling hint, which WebP ignores.
    let width = usize::from(u16::from_le_bytes([w0, w1]) & 0x3FFF);
    let height = usize::from(u16::from_le_bytes([h0, h1]) & 0x3FFF);
    if width == 0 || height == 0 {
        return Err(ImageError::Malformed("a VP8 frame of no size"));
    }
    Ok(Tag {
        width,
        height,
        first_partition,
    })
}

/// A key frame's width and height, from its uncompressed header, checked as
/// libwebp's `VP8GetInfo` checks it against the `declared` size of its chunk.
#[allow(clippy::cast_possible_truncation, reason = "14-bit fields")]
pub(super) fn info(data: &[u8], declared: usize) -> ImageResult<(u32, u32)> {
    let tag = read_tag(data, declared)?;
    Ok((tag.width as u32, tag.height as u32))
}

// ---------------------------------------------------------------------------
// The frame header
// ---------------------------------------------------------------------------

/// Segment-based adjustments (RFC 6386 §9.3): up to four segments, each with
/// its own quantiser and loop-filter level.
struct Segmentation {
    enabled: bool,
    /// Whether each macroblock says which segment it is in.
    update_map: bool,
    /// Whether the per-segment values replace the frame's (or are added).
    absolute: bool,
    quantizer: [i32; 4],
    filter_level: [i32; 4],
    tree_probs: [u8; 3],
}

impl Segmentation {
    /// Read the segmentation header. A segmentation with no data of its own
    /// keeps the defaults -- absolute zeros -- which is libwebp's reading;
    /// the RFC's reference decoder resets them as deltas instead, and the two
    /// differ only for a stream that enables segments and gives them no
    /// values, which no encoder writes.
    #[allow(clippy::cast_possible_truncation, reason = "8-bit literals")]
    fn read(r: &mut Reader<'_>) -> Self {
        let mut s = Self {
            enabled: r.flag(),
            update_map: false,
            absolute: true,
            quantizer: [0; 4],
            filter_level: [0; 4],
            tree_probs: [255; 3],
        };
        if s.enabled {
            s.update_map = r.flag();
            if r.flag() {
                s.absolute = r.flag();
                for q in &mut s.quantizer {
                    *q = r.optional_signed(7);
                }
                for f in &mut s.filter_level {
                    *f = r.optional_signed(6);
                }
            }
            if s.update_map {
                for p in &mut s.tree_probs {
                    *p = if r.flag() { r.literal(8) as u8 } else { 255 };
                }
            }
        }
        s
    }

    /// A per-segment value: the segment's own, or the frame's adjusted by it,
    /// or -- segments off -- the frame's.
    #[allow(
        clippy::arithmetic_side_effects,
        reason = "a 7-bit frame value plus a signed 7-bit adjustment"
    )]
    fn value(&self, frame: i32, own: i32) -> i32 {
        if !self.enabled {
            frame
        } else if self.absolute {
            own
        } else {
            frame + own
        }
    }
}

/// The loop filter's frame-level settings (RFC 6386 §9.4).
struct FilterHeader {
    simple: bool,
    level: i32,
    sharpness: u8,
    use_deltas: bool,
    /// Adjustments by reference frame; a key frame's macroblocks are all
    /// intra, entry 0.
    ref_delta: [i32; 4],
    /// Adjustments by mode; entry 0 is for subblock-predicted macroblocks.
    mode_delta: [i32; 4],
}

impl FilterHeader {
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_possible_wrap,
        reason = "3- and 6-bit literals"
    )]
    fn read(r: &mut Reader<'_>) -> Self {
        let mut f = Self {
            simple: r.flag(),
            level: r.literal(6) as i32,
            sharpness: r.literal(3) as u8,
            use_deltas: r.flag(),
            ref_delta: [0; 4],
            mode_delta: [0; 4],
        };
        if f.use_deltas && r.flag() {
            for d in f.ref_delta.iter_mut().chain(f.mode_delta.iter_mut()) {
                if r.flag() {
                    *d = r.signed(6);
                }
            }
        }
        f
    }

    /// Which filter the frame uses, if any. A frame whose own level is zero
    /// is not filtered at all, whatever its segments say -- libwebp's reading,
    /// and the RFC reference decoder's too.
    const fn kind(&self) -> Option<Kind> {
        if self.level == 0 {
            None
        } else if self.simple {
            Some(Kind::Simple)
        } else {
            Some(Kind::Normal)
        }
    }

    /// The filter strength for a macroblock in each segment, predicted as a
    /// whole (`[s][0]`) or by subblocks (`[s][1]`). The level is clamped once,
    /// after every adjustment, as libwebp clamps it; the RFC's reference
    /// decoder also clamps the segment's level before the deltas, which only
    /// matters for a segment level outside 0..=63 -- again not something an
    /// encoder writes.
    #[allow(
        clippy::arithmetic_side_effects,
        reason = "levels and deltas are small: 6 bits and signed 6 bits each"
    )]
    fn strengths(&self, segmentation: &Segmentation) -> [[Strength; 2]; 4] {
        let mut out = [[Strength::default(); 2]; 4];
        for (per_segment, &own) in out.iter_mut().zip(&segmentation.filter_level) {
            let base = segmentation.value(self.level, own);
            for (subblocks, strength) in per_segment.iter_mut().enumerate() {
                let mut level = base;
                if self.use_deltas {
                    level += self.ref_delta[0];
                    if subblocks == 1 {
                        level += self.mode_delta[0];
                    }
                }
                *strength = Strength::new(level, self.sharpness);
            }
        }
        out
    }
}

/// Dequantisation factors for one segment, `[DC, AC]` per kind of block
/// (RFC 6386 §9.6, §14.1).
#[derive(Clone, Copy, Default)]
struct Quant {
    y1: [i32; 2],
    y2: [i32; 2],
    uv: [i32; 2],
}

/// The quantiser indices (RFC 6386 §9.6): the luma AC index every other is
/// relative to, and the deltas for luma DC, second-order DC and AC, and
/// chroma DC and AC.
#[derive(Clone, Copy)]
struct QuantIndices {
    base: i32,
    deltas: [i32; 5],
}

impl QuantIndices {
    #[allow(clippy::cast_possible_wrap, reason = "a 7-bit literal")]
    fn read(r: &mut Reader<'_>) -> Self {
        let base = r.literal(7) as i32;
        let mut deltas = [0; 5];
        for d in &mut deltas {
            *d = r.optional_signed(4);
        }
        Self { base, deltas }
    }
}

/// Every segment's dequantisation factors, as libwebp's `VP8ParseQuant`
/// derives them.
#[allow(
    clippy::cast_sign_loss,
    clippy::arithmetic_side_effects,
    reason = "indices are clamped to the tables before use; factors are at most 157 * 2 and 284 * 155"
)]
fn quantizers(indices: QuantIndices, segmentation: &Segmentation) -> [Quant; 4] {
    let [y1_dc, y2_dc, y2_ac, uv_dc, uv_ac] = indices.deltas;
    let dc = |q: i32, most: i32| {
        i32::from(
            DC_QUANT
                .get(q.clamp(0, most) as usize)
                .copied()
                .unwrap_or(0),
        )
    };
    let ac = |q: i32| i32::from(AC_QUANT.get(q.clamp(0, 127) as usize).copied().unwrap_or(0));
    let mut out = [Quant::default(); 4];
    for (quant, &own) in out.iter_mut().zip(&segmentation.quantizer) {
        let q = segmentation.value(indices.base, own);
        *quant = Quant {
            y1: [dc(q + y1_dc, 127), ac(q)],
            // The RFC's `* 155 / 100`; libwebp's `* 101581 >> 16` is the same
            // over the table's range, which a test checks.
            y2: [dc(q + y2_dc, 127) * 2, (ac(q + y2_ac) * 155 / 100).max(8)],
            // Chroma DC is capped at index 117, factor 132 (§14.1).
            uv: [dc(q + uv_dc, 117), ac(q + uv_ac)],
        };
    }
    out
}

/// Coefficient probabilities, `[block type][band][context][tree node]`.
type Probs = [[[[u8; 11]; 3]; 8]; 4];

/// The key frame's coefficient probabilities: the defaults, with the updates
/// the header carries (RFC 6386 §13.4).
#[allow(clippy::cast_possible_truncation, reason = "8-bit literals")]
fn read_probs(r: &mut Reader<'_>) -> Probs {
    let mut probs = DEFAULT_COEFF_PROBS;
    let slots = probs.iter_mut().flatten().flatten().flatten();
    let odds = COEFF_UPDATE_PROBS.iter().flatten().flatten().flatten();
    for (slot, &update) in slots.zip(odds) {
        if r.bit(update) {
            *slot = r.literal(8) as u8;
        }
    }
    probs
}

/// Everything the first partition says before the macroblocks.
struct Header<'a> {
    width: usize,
    height: usize,
    mb_width: usize,
    mb_height: usize,
    /// The first partition's decoder, positioned at the first macroblock.
    modes: Reader<'a>,
    /// The coefficient partitions.
    tokens: Vec<Reader<'a>>,
    segmentation: Segmentation,
    filter: FilterHeader,
    quant: QuantIndices,
    probs: Probs,
    skip_prob: Option<u8>,
}

/// Read a key frame's headers, as libwebp's `VP8GetHeaders` does.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "sizes are checked against the data before they are used, and a partition count is at most 8"
)]
fn read_header(data: &[u8], declared: usize) -> ImageResult<Header<'_>> {
    let tag = read_tag(data, declared)?;
    let after_tag = data.get(TAG_LEN..).unwrap_or(&[]);
    let (first, rest) = after_tag
        .split_at_checked(tag.first_partition)
        .ok_or(ImageError::Truncated)?;
    let mut r = Reader::new(first);
    let _colour_space = r.flag();
    let _clamping_type = r.flag();
    let segmentation = Segmentation::read(&mut r);
    let filter_header = FilterHeader::read(&mut r);
    if r.eof() {
        return Err(ImageError::Malformed(
            "a VP8 frame header longer than its partition",
        ));
    }

    // The coefficient partitions (§9.5): all but the last are preceded by
    // three-byte sizes, which libwebp trims to what is present; the last is
    // whatever remains, and must not be empty.
    let count = 1usize << r.literal(2);
    let (sizes, mut body) = rest
        .split_at_checked(3 * (count - 1))
        .ok_or(ImageError::Truncated)?;
    let mut tokens = Vec::with_capacity(count);
    for size in sizes.chunks_exact(3) {
        let declared = size
            .iter()
            .rev()
            .fold(0usize, |acc, &b| (acc << 8) | usize::from(b));
        let (part, after) = body.split_at(declared.min(body.len()));
        tokens.push(Reader::new(part));
        body = after;
    }
    if body.is_empty() {
        return Err(ImageError::Truncated);
    }
    tokens.push(Reader::new(body));

    let quant = QuantIndices::read(&mut r);
    // Whether to keep these probabilities for the next frame: a still
    // picture has none.
    let _refresh_entropy = r.flag();
    let probs = read_probs(&mut r);
    let skip_prob = if r.flag() {
        u8::try_from(r.literal(8)).ok()
    } else {
        None
    };
    Ok(Header {
        width: tag.width,
        height: tag.height,
        mb_width: tag.width.div_ceil(16),
        mb_height: tag.height.div_ceil(16),
        modes: r,
        tokens,
        segmentation,
        filter: filter_header,
        quant,
        probs,
        skip_prob,
    })
}

// ---------------------------------------------------------------------------
// Macroblock modes
// ---------------------------------------------------------------------------

/// How a macroblock's luma is predicted.
enum Luma {
    /// As a whole, by one of the four whole-block modes.
    Whole(u8),
    /// As sixteen subblocks, each by its own mode, in raster order.
    Subblocks([u8; 16]),
}

/// A macroblock's prediction record (RFC 6386 §11, §19.3).
struct Modes {
    segment: usize,
    /// The coded "no non-zero coefficients" flag, if the frame codes one.
    skip: bool,
    luma: Luma,
    chroma: u8,
}

/// Read a subblock mode by the tree of RFC 6386 §11.2 with probabilities
/// `p`.
fn read_subblock_mode(r: &mut Reader<'_>, p: &[u8; 9]) -> u8 {
    let [p0, p1, p2, p3, p4, p5, p6, p7, p8] = *p;
    if !r.bit(p0) {
        predict::B_DC_PRED
    } else if !r.bit(p1) {
        predict::B_TM_PRED
    } else if !r.bit(p2) {
        predict::B_VE_PRED
    } else if !r.bit(p3) {
        if !r.bit(p4) {
            predict::B_HE_PRED
        } else if !r.bit(p5) {
            predict::B_RD_PRED
        } else {
            predict::B_VR_PRED
        }
    } else if !r.bit(p6) {
        predict::B_LD_PRED
    } else if !r.bit(p7) {
        predict::B_VL_PRED
    } else if !r.bit(p8) {
        predict::B_HD_PRED
    } else {
        predict::B_HU_PRED
    }
}

/// Read one macroblock's modes. `above` holds the subblock modes along the
/// bottom of the macroblock above, `left` those down the right of the one to
/// the left; both are updated to this macroblock's.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "a segment number is at most 3"
)]
fn read_modes(
    r: &mut Reader<'_>,
    segmentation: &Segmentation,
    skip_prob: Option<u8>,
    above: &mut [u8; 4],
    left: &mut [u8; 4],
) -> Modes {
    let segment = if segmentation.update_map {
        let [p0, p1, p2] = segmentation.tree_probs;
        if r.bit(p0) {
            2 + usize::from(r.bit(p2))
        } else {
            usize::from(r.bit(p1))
        }
    } else {
        0
    };
    let skip = skip_prob.is_some_and(|p| r.bit(p));
    let luma = if r.bit(145) {
        let mode = if r.bit(156) {
            if r.bit(128) { TM_PRED } else { H_PRED }
        } else if r.bit(163) {
            V_PRED
        } else {
            DC_PRED
        };
        let implied = predict::implied_subblock_mode(mode);
        *above = [implied; 4];
        *left = [implied; 4];
        Luma::Whole(mode)
    } else {
        let mut modes = [0u8; 16];
        for (row, l) in modes.chunks_exact_mut(4).zip(left.iter_mut()) {
            for (slot, a) in row.iter_mut().zip(above.iter_mut()) {
                let probs = KF_BMODE_PROBS
                    .get(usize::from(*a))
                    .and_then(|by_left| by_left.get(usize::from(*l)))
                    .unwrap_or(&[128; 9]);
                let mode = read_subblock_mode(r, probs);
                *slot = mode;
                *a = mode;
                *l = mode;
            }
        }
        Luma::Subblocks(modes)
    };
    let chroma = if !r.bit(142) {
        DC_PRED
    } else if !r.bit(114) {
        V_PRED
    } else if r.bit(183) {
        TM_PRED
    } else {
        H_PRED
    };
    Modes {
        segment,
        skip,
        luma,
        chroma,
    }
}

// ---------------------------------------------------------------------------
// Coefficients
// ---------------------------------------------------------------------------

/// The band each coefficient position's probabilities come from (§13.3).
const BANDS: [usize; 16] = [0, 1, 2, 3, 6, 4, 5, 6, 6, 6, 6, 6, 6, 6, 6, 7];

/// Coefficient positions in raster order, by their order in the stream.
const ZIGZAG: [usize; 16] = [0, 1, 4, 8, 5, 2, 3, 6, 9, 12, 13, 10, 7, 11, 14, 15];

/// The extra-bit probabilities of the four widest token categories (§13.2).
const CATEGORY_PROBS: [&[u8]; 4] = [
    &[173, 148, 140],
    &[176, 155, 140, 135],
    &[180, 157, 141, 134, 130],
    &[254, 254, 243, 230, 196, 177, 153, 140, 133, 130, 129],
];

/// A coefficient of magnitude 2 or more: the rest of the token tree past the
/// "one" node, then any extra bits (libwebp's `GetLargeValue`).
#[allow(
    clippy::arithmetic_side_effects,
    reason = "at most eleven extra bits: the value stays below 2^12"
)]
fn large_value(r: &mut Reader<'_>, p: &[u8; 11]) -> i32 {
    let [_, _, _, p3, p4, p5, p6, p7, p8, p9, p10] = *p;
    if !r.bit(p3) {
        if r.bit(p4) {
            3 + i32::from(r.bit(p5))
        } else {
            2
        }
    } else if !r.bit(p6) {
        if r.bit(p7) {
            7 + 2 * i32::from(r.bit(165)) + i32::from(r.bit(145))
        } else {
            5 + i32::from(r.bit(159))
        }
    } else {
        let high = r.bit(p8);
        let low = r.bit(if high { p10 } else { p9 });
        let category = 2 * usize::from(high) + usize::from(low);
        let extra = CATEGORY_PROBS
            .get(category)
            .copied()
            .unwrap_or(&[])
            .iter()
            .fold(0i32, |v, &prob| 2 * v + i32::from(r.bit(prob)));
        extra + 3 + (8 << category)
    }
}

/// The probabilities for coefficient position `n` in `context`; past the last
/// position, a harmless stand-in that is never read.
fn band_probs(bands: &[[[u8; 11]; 3]; 8], n: usize, context: usize) -> &[u8; 11] {
    BANDS
        .get(n)
        .and_then(|&band| bands.get(band))
        .and_then(|contexts| contexts.get(context))
        .unwrap_or(&[128; 11])
}

/// Read one block's coefficients from position `first` on, dequantised by
/// `dq` (`[DC, AC]`), into `out` in raster order (libwebp's `GetCoeffs`).
///
/// Returns the position after the last coefficient read -- 16 if a run of
/// zeros reached the end -- which is how libwebp records whether the block
/// had any, for its neighbours' contexts and its own transform.
#[allow(
    clippy::cast_possible_truncation,
    clippy::arithmetic_side_effects,
    reason = "positions stay below 16; the dequantised value wraps to sixteen bits as libwebp stores it"
)]
fn read_block(
    r: &mut Reader<'_>,
    bands: &[[[u8; 11]; 3]; 8],
    context: usize,
    dq: [i32; 2],
    first: usize,
    out: &mut [i16; 16],
) -> usize {
    let mut n = first;
    let mut p = band_probs(bands, n, context);
    while n < 16 {
        if !r.bit(p[0]) {
            return n;
        }
        while !r.bit(p[1]) {
            n += 1;
            if n == 16 {
                return 16;
            }
            p = band_probs(bands, n, 0);
        }
        let (magnitude, next_context) = if r.bit(p[2]) {
            (large_value(r, p), 2)
        } else {
            (1, 1)
        };
        let value = if r.sign() { -magnitude } else { magnitude };
        let factor = if n > 0 { dq[1] } else { dq[0] };
        if let Some(slot) = ZIGZAG.get(n).and_then(|&at| out.get_mut(at)) {
            *slot = value.wrapping_mul(factor) as i16;
        }
        n += 1;
        p = band_probs(bands, n, next_context);
    }
    16
}

/// Which of a block's neighbours had coefficients, for the first
/// coefficient's context: per column (above) or row (left) of luma blocks,
/// of each chroma plane's blocks, and the second-order block.
#[derive(Clone, Copy, Default)]
struct Nonzero {
    y: [bool; 4],
    u: [bool; 2],
    v: [bool; 2],
    y2: bool,
}

/// How much of the inverse DCT a block needs.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Coded {
    /// Nothing: every coefficient is zero.
    #[default]
    Nothing,
    /// Only the DC is non-zero.
    Dc,
    /// Anything more.
    Full,
}

impl Coded {
    /// libwebp's classification: by the position after the last coefficient
    /// read, and -- for a block with at most its DC read -- by whether that
    /// DC, which for a whole-predicted macroblock came from the second-order
    /// block, is non-zero.
    const fn of(end: usize, dc: i16) -> Self {
        if end > 1 {
            Self::Full
        } else if dc != 0 {
            Self::Dc
        } else {
            Self::Nothing
        }
    }
}

/// A macroblock's residual: 16 luma blocks, then 4 U, then 4 V.
struct Residual {
    coeffs: [[i16; 16]; 24],
    coded: [Coded; 24],
}

impl Residual {
    const fn empty() -> Self {
        Self {
            coeffs: [[0; 16]; 24],
            coded: [Coded::Nothing; 24],
        }
    }

    fn any(&self) -> bool {
        self.coded.iter().any(|&c| c != Coded::Nothing)
    }
}

/// Read a macroblock's coefficients (libwebp's `ParseResiduals`), updating
/// the neighbour contexts.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "contexts are sums of two flags; block indices are below 24"
)]
fn read_residual(
    r: &mut Reader<'_>,
    probs: &Probs,
    quant: &Quant,
    whole: bool,
    above: &mut Nonzero,
    left: &mut Nonzero,
    out: &mut Residual,
) {
    let [p_y_after_y2, p_y2, p_uv, p_y] = probs;
    let (first, y_probs) = if whole {
        let context = usize::from(above.y2) + usize::from(left.y2);
        let mut dcs = [0i16; 16];
        let end = read_block(r, p_y2, context, quant.y2, 0, &mut dcs);
        above.y2 = end > 0;
        left.y2 = end > 0;
        let spread = if end > 1 {
            transform::inverse_wht(&dcs)
        } else {
            // A DC alone spreads evenly: libwebp's shortcut, and the WHT's
            // own answer for such a block.
            let [dc, ..] = dcs;
            #[allow(
                clippy::cast_possible_truncation,
                reason = "a sixteen-bit value plus 3, shifted right by 3"
            )]
            let even = ((i32::from(dc) + 3) >> 3) as i16;
            [even; 16]
        };
        for (block, dc) in out.coeffs.iter_mut().zip(spread) {
            block[0] = dc;
        }
        (1, p_y_after_y2)
    } else {
        (0, p_y)
    };
    for (i, (block, coded)) in out
        .coeffs
        .iter_mut()
        .zip(out.coded.iter_mut())
        .take(16)
        .enumerate()
    {
        let (bx, by) = (i % 4, i / 4);
        let (Some(a), Some(l)) = (above.y.get_mut(bx), left.y.get_mut(by)) else {
            continue;
        };
        let context = usize::from(*a) + usize::from(*l);
        let end = read_block(r, y_probs, context, quant.y1, first, block);
        *a = end > first;
        *l = end > first;
        *coded = Coded::of(end, block[0]);
    }
    for (plane, start) in [(0usize, 16usize), (1, 20)] {
        let (a_flags, l_flags) = if plane == 0 {
            (&mut above.u, &mut left.u)
        } else {
            (&mut above.v, &mut left.v)
        };
        let blocks = out
            .coeffs
            .iter_mut()
            .zip(out.coded.iter_mut())
            .skip(start)
            .take(4);
        for (i, (block, coded)) in blocks.enumerate() {
            let (bx, by) = (i % 2, i / 2);
            let (Some(a), Some(l)) = (a_flags.get_mut(bx), l_flags.get_mut(by)) else {
                continue;
            };
            let context = usize::from(*a) + usize::from(*l);
            let end = read_block(r, p_uv, context, quant.uv, 0, block);
            *a = end > 0;
            *l = end > 0;
            *coded = Coded::of(end, block[0]);
        }
    }
}

/// The contexts a macroblock's parse reads and updates: the subblock modes and
/// coefficient flags left along the bottom of the macroblock above, and those
/// down the right of the one to the left.
struct Neighbours<'n> {
    above_modes: &'n mut [u8; 4],
    left_modes: &'n mut [u8; 4],
    above_nonzero: &'n mut Nonzero,
    left_nonzero: &'n mut Nonzero,
}

/// Read one macroblock: its modes from the first partition, its coefficients
/// (unless it is coded as having none) from `tokens` into `residual`.
/// Returns the modes, and whether the macroblock has no coefficients -- coded
/// so, or found so -- which spares its interior edges the loop filter.
fn read_macroblock(
    first: &mut Reader<'_>,
    tokens: &mut Reader<'_>,
    header: &MacroblockHeader<'_>,
    neighbours: Neighbours<'_>,
    residual: &mut Residual,
) -> (Modes, bool) {
    let Neighbours {
        above_modes,
        left_modes,
        above_nonzero,
        left_nonzero,
    } = neighbours;
    let modes = read_modes(
        first,
        header.segmentation,
        header.skip_prob,
        above_modes,
        left_modes,
    );
    let whole = matches!(modes.luma, Luma::Whole(_));
    *residual = Residual::empty();
    let empty = if header.skip_prob.is_some() && modes.skip {
        // No coefficients: every context this macroblock sets is empty --
        // except the second-order block's, which a subblock-predicted
        // macroblock does not have and so leaves as it was.
        let (y2_above, y2_left) = (above_nonzero.y2, left_nonzero.y2);
        *above_nonzero = Nonzero::default();
        *left_nonzero = Nonzero::default();
        if !whole {
            above_nonzero.y2 = y2_above;
            left_nonzero.y2 = y2_left;
        }
        true
    } else {
        let quant = header
            .quantizers
            .get(modes.segment)
            .copied()
            .unwrap_or_default();
        read_residual(
            tokens,
            header.probs,
            &quant,
            whole,
            above_nonzero,
            left_nonzero,
            residual,
        );
        !residual.any()
    };
    (modes, empty)
}

/// What parsing a macroblock needs from the frame header.
struct MacroblockHeader<'h> {
    segmentation: &'h Segmentation,
    skip_prob: Option<u8>,
    quantizers: &'h [Quant; 4],
    probs: &'h Probs,
}

// ---------------------------------------------------------------------------
// Reconstruction
// ---------------------------------------------------------------------------

/// A decoded frame: three planes, each a whole number of macroblocks, of
/// which the top-left `width` x `height` (half that, rounded up, for chroma)
/// is the picture.
pub(super) struct Frame {
    pub width: usize,
    pub height: usize,
    y: Vec<u8>,
    u: Vec<u8>,
    v: Vec<u8>,
    y_stride: usize,
    uv_stride: usize,
}

/// Luma work area: the macroblock at rows 1..=16, columns 1..=16 of rows
/// `Y_STRIDE` apart, the row above it in row 0 (with the four samples beyond
/// its right end in columns 17..=20), the column to its left in column 0.
const Y_STRIDE: usize = 32;
/// Chroma work area: the same for an 8x8 block.
const UV_STRIDE: usize = 16;

/// A macroblock being reconstructed, with the samples around it that its
/// prediction reads.
struct Work {
    y: [u8; 17 * Y_STRIDE],
    u: [u8; 9 * UV_STRIDE],
    v: [u8; 9 * UV_STRIDE],
}

impl Frame {
    /// Lay out the samples around macroblock (`mx`, `my`) that prediction
    /// reads, from the frame -- which above and to the left of it is
    /// reconstructed and not yet filtered -- or, outside the picture, the
    /// RFC's stand-ins: 127 above, 129 to the left (§12.2, §12.3).
    #[allow(
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        reason = "the macroblock is inside the planes, which hold whole macroblocks; work-area indices are below its fixed size"
    )]
    fn load_edges(&self, work: &mut Work, (mx, my): (usize, usize), mb_width: usize) {
        let ys = self.y_stride;
        if my == 0 {
            work.y[..21].fill(127);
        } else {
            let above = (16 * my - 1) * ys + 16 * mx;
            work.y[0] = if mx == 0 { 129 } else { self.y[above - 1] };
            work.y[1..17].copy_from_slice(&self.y[above..above + 16]);
            if mx + 1 < mb_width {
                work.y[17..21].copy_from_slice(&self.y[above + 16..above + 20]);
            } else {
                // The rightmost macroblock repeats the last sample above it.
                work.y[17..21].fill(self.y[above + 15]);
            }
        }
        for r in 0..16 {
            work.y[(r + 1) * Y_STRIDE] = if mx == 0 {
                129
            } else {
                self.y[(16 * my + r) * ys + 16 * mx - 1]
            };
        }
        // Subblocks down the right edge see the samples above and to the
        // right of the whole macroblock, not of themselves (§12.3).
        for r in [4, 8, 12] {
            work.y.copy_within(17..21, r * Y_STRIDE + 17);
        }

        let cs = self.uv_stride;
        for (plane, area) in [(&self.u, &mut work.u), (&self.v, &mut work.v)] {
            if my == 0 {
                area[..9].fill(127);
            } else {
                let above = (8 * my - 1) * cs + 8 * mx;
                area[0] = if mx == 0 { 129 } else { plane[above - 1] };
                area[1..9].copy_from_slice(&plane[above..above + 8]);
            }
            for r in 0..8 {
                area[(r + 1) * UV_STRIDE] = if mx == 0 {
                    129
                } else {
                    plane[(8 * my + r) * cs + 8 * mx - 1]
                };
            }
        }
    }

    /// Copy a reconstructed macroblock from the work area into the frame.
    #[allow(
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        reason = "the macroblock is inside the planes, which hold whole macroblocks"
    )]
    fn store(&mut self, work: &Work, (mx, my): (usize, usize)) {
        let ys = self.y_stride;
        for r in 0..16 {
            let to = (16 * my + r) * ys + 16 * mx;
            let from = (r + 1) * Y_STRIDE + 1;
            self.y[to..to + 16].copy_from_slice(&work.y[from..from + 16]);
        }
        let cs = self.uv_stride;
        for (plane, area) in [(&mut self.u, &work.u), (&mut self.v, &work.v)] {
            for r in 0..8 {
                let to = (8 * my + r) * cs + 8 * mx;
                let from = (r + 1) * UV_STRIDE + 1;
                plane[to..to + 8].copy_from_slice(&area[from..from + 8]);
            }
        }
    }
}

/// Add a block's residual to the samples at `at`.
fn add_residual(coded: Coded, coeffs: &[i16; 16], samples: &mut [u8], at: usize, stride: usize) {
    match coded {
        Coded::Nothing => {}
        Coded::Dc => transform::add_dc(coeffs[0], samples, at, stride),
        Coded::Full => transform::add_idct(coeffs, samples, at, stride),
    }
}

/// Predict a macroblock in the work area and add its residual (libwebp's
/// `ReconstructRow`, for one macroblock).
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "subblock positions are below 16 and every work-area index below its fixed size"
)]
fn reconstruct(work: &mut Work, modes: &Modes, residual: &Residual, edges: Edges) {
    match &modes.luma {
        Luma::Whole(mode) => {
            predict::whole_block(*mode, &mut work.y, Y_STRIDE, 16, edges);
            for (i, (coeffs, &coded)) in residual
                .coeffs
                .iter()
                .zip(&residual.coded)
                .take(16)
                .enumerate()
            {
                let at = (4 * (i / 4) + 1) * Y_STRIDE + 4 * (i % 4) + 1;
                add_residual(coded, coeffs, &mut work.y, at, Y_STRIDE);
            }
        }
        Luma::Subblocks(modes) => {
            for (i, ((&mode, coeffs), &coded)) in modes
                .iter()
                .zip(&residual.coeffs)
                .zip(&residual.coded)
                .enumerate()
            {
                // The corner above-left of the subblock, in the work area.
                let corner = 4 * (i / 4) * Y_STRIDE + 4 * (i % 4);
                let y = &mut work.y;
                let edge: predict::Edge = [
                    y[corner + 4 * Y_STRIDE],
                    y[corner + 3 * Y_STRIDE],
                    y[corner + 2 * Y_STRIDE],
                    y[corner + Y_STRIDE],
                    y[corner],
                    y[corner + 1],
                    y[corner + 2],
                    y[corner + 3],
                    y[corner + 4],
                    y[corner + 5],
                    y[corner + 6],
                    y[corner + 7],
                    y[corner + 8],
                ];
                let prediction = predict::subblock(mode, &edge);
                let at = corner + Y_STRIDE + 1;
                for (r, row) in prediction.chunks_exact(4).enumerate() {
                    let start = at + r * Y_STRIDE;
                    y[start..start + 4].copy_from_slice(row);
                }
                add_residual(coded, coeffs, y, at, Y_STRIDE);
            }
        }
    }
    for (area, first) in [(&mut work.u, 16usize), (&mut work.v, 20)] {
        predict::whole_block(modes.chroma, area, UV_STRIDE, 8, edges);
        for i in 0..4 {
            let at = (4 * (i / 2) + 1) * UV_STRIDE + 4 * (i % 2) + 1;
            add_residual(
                residual.coded[first + i],
                &residual.coeffs[first + i],
                area,
                at,
                UV_STRIDE,
            );
        }
    }
}

/// Decode a `VP8 ` chunk's key frame into its planes: `data` is the chunk's
/// payload with its padding byte if it has one, `declared` the payload's
/// length as the chunk states it (see `Chunk::padded` in `webp.rs`).
///
/// # Errors
///
/// [`ImageError::TooLarge`] if its planes would pass `limits`;
/// [`ImageError::Truncated`] if a partition runs out; otherwise
/// [`ImageError::Malformed`] naming what is wrong with its headers.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "macroblock counts are below 2^10 each, and the product of the plane sizes is checked against the byte budget"
)]
pub(super) fn decode(data: &[u8], declared: usize, limits: Limits) -> ImageResult<Frame> {
    let Header {
        width,
        height,
        mb_width,
        mb_height,
        modes: mut first_partition,
        mut tokens,
        segmentation,
        filter: filter_header,
        quant: quant_indices,
        probs,
        skip_prob,
    } = read_header(data, declared)?;
    let filter_kind = filter_header.kind();
    let strengths = filter_header.strengths(&segmentation);
    let quantizers = quantizers(quant_indices, &segmentation);
    let pixels = (width * height) as u64;
    if pixels > limits.max_pixels {
        return Err(ImageError::TooLarge {
            pixels,
            limit: limits.max_pixels,
        });
    }
    // Three planes of whole macroblocks, then four bytes a pixel for the
    // picture they become.
    let plane_bytes = mb_width * mb_height * 384;
    let bytes = plane_bytes + width * height * 4;
    if bytes > limits.max_decompressed_bytes {
        return Err(ImageError::TooLarge {
            pixels: bytes as u64,
            limit: limits.max_decompressed_bytes as u64,
        });
    }
    let (y_stride, uv_stride) = (mb_width * 16, mb_width * 8);
    let mut frame = Frame {
        width,
        height,
        y: vec![0; y_stride * mb_height * 16],
        u: vec![0; uv_stride * mb_height * 8],
        v: vec![0; uv_stride * mb_height * 8],
        y_stride,
        uv_stride,
    };

    let mut work = Work {
        y: [0; 17 * Y_STRIDE],
        u: [0; 9 * UV_STRIDE],
        v: [0; 9 * UV_STRIDE],
    };
    // Per column: the subblock modes along the bottom of the macroblock
    // above, for mode contexts (B_DC_PRED above the picture), and which of
    // its blocks had coefficients.
    let mut above_modes = vec![[predict::B_DC_PRED; 4]; mb_width];
    let mut above_nonzero = vec![Nonzero::default(); mb_width];
    // Filter settings for the row being decoded and the one above it, which
    // is filtered once this one no longer needs it unfiltered.
    let mut row_filters = vec![filter::Macroblock::default(); mb_width];
    let mut pending: Option<(usize, Vec<filter::Macroblock>)> = None;
    let mut residual = Residual::empty();
    let partitions = tokens.len();
    let mb_header = MacroblockHeader {
        segmentation: &segmentation,
        skip_prob,
        quantizers: &quantizers,
        probs: &probs,
    };

    for my in 0..mb_height {
        let mut left_modes = [predict::B_DC_PRED; 4];
        let mut left_nonzero = Nonzero::default();
        for (mx, ((above_mode, above_nz), filter_info)) in above_modes
            .iter_mut()
            .zip(above_nonzero.iter_mut())
            .zip(row_filters.iter_mut())
            .enumerate()
        {
            let tokens = tokens
                .get_mut(my % partitions)
                .ok_or(ImageError::Malformed(
                    "a VP8 frame with no coefficient partition",
                ))?;
            let neighbours = Neighbours {
                above_modes: above_mode,
                left_modes: &mut left_modes,
                above_nonzero: above_nz,
                left_nonzero: &mut left_nonzero,
            };
            let (modes, empty) = read_macroblock(
                &mut first_partition,
                tokens,
                &mb_header,
                neighbours,
                &mut residual,
            );
            let whole = matches!(modes.luma, Luma::Whole(_));
            if tokens.eof() {
                return Err(ImageError::Truncated);
            }
            let strength = strengths
                .get(modes.segment)
                .and_then(|s| s.get(usize::from(!whole)))
                .copied()
                .unwrap_or_default();
            *filter_info = filter::Macroblock {
                strength,
                inner: !whole || !empty,
            };
            frame.load_edges(&mut work, (mx, my), mb_width);
            let edges = Edges {
                above: my > 0,
                left: mx > 0,
            };
            reconstruct(&mut work, &modes, &residual, edges);
            frame.store(&work, (mx, my));
        }
        if first_partition.eof() {
            return Err(ImageError::Truncated);
        }
        if let Some(kind) = filter_kind {
            if let Some((row, infos)) = pending.take() {
                frame.filter_row(kind, row, &infos);
            }
            pending = Some((my, row_filters.clone()));
        }
    }
    if let (Some(kind), Some((row, infos))) = (filter_kind, pending) {
        frame.filter_row(kind, row, &infos);
    }
    Ok(frame)
}

impl Frame {
    /// Loop-filter macroblock row `my`.
    fn filter_row(&mut self, kind: Kind, my: usize, infos: &[filter::Macroblock]) {
        let mut luma = filter::Plane {
            samples: &mut self.y,
            stride: self.y_stride,
        };
        let mut u = filter::Plane {
            samples: &mut self.u,
            stride: self.uv_stride,
        };
        let mut v = filter::Plane {
            samples: &mut self.v,
            stride: self.uv_stride,
        };
        for (mx, &info) in infos.iter().enumerate() {
            filter::filter_macroblock(kind, &mut luma, [&mut u, &mut v], (mx, my), info);
        }
    }

    /// The picture as `0xAARRGGBB`, alpha from `alpha` (a byte per pixel) or
    /// opaque.
    pub(super) fn to_argb(&self, alpha: Option<&[u8]>) -> Vec<u32> {
        let (cw, ch) = (self.width.div_ceil(2), self.height.div_ceil(2));
        let luma = yuv::Plane {
            samples: &self.y,
            stride: self.y_stride,
            width: self.width,
            height: self.height,
        };
        let u = yuv::Plane {
            samples: &self.u,
            stride: self.uv_stride,
            width: cw,
            height: ch,
        };
        let v = yuv::Plane {
            samples: &self.v,
            stride: self.uv_stride,
            width: cw,
            height: ch,
        };
        yuv::to_argb(&luma, &u, &v, alpha)
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
mod tests {
    use super::*;

    #[test]
    fn libwebps_y2_ac_factor_is_the_rfcs() {
        for &q in &AC_QUANT {
            let q = u32::from(q);
            assert_eq!(q * 155 / 100, (q * 101_581) >> 16, "{q}");
        }
    }

    #[test]
    fn the_tables_are_the_shapes_the_rfc_gives() {
        // Spot values from the RFC's text (§11.5, §13.5, §14.1), a check that
        // the generated tables are indexed the way this module reads them.
        assert_eq!(
            KF_BMODE_PROBS[0][0],
            [231, 120, 48, 89, 115, 113, 120, 152, 112]
        );
        assert_eq!(KF_BMODE_PROBS[9][9], [112, 19, 12, 61, 195, 128, 48, 4, 24]);
        assert_eq!(DEFAULT_COEFF_PROBS[0][0][0], [128; 11]);
        assert_eq!(COEFF_UPDATE_PROBS[0][0][0], [255; 11]);
        assert_eq!(DC_QUANT[0], 4);
        assert_eq!(DC_QUANT[127], 157);
        assert_eq!(DC_QUANT[117], 132);
        assert_eq!(AC_QUANT[127], 284);
    }

    #[test]
    fn a_tag_is_checked_as_libwebp_checks_it() {
        // A 17x9 key frame, profile 0, shown, first partition of 5 bytes.
        let mut frame = vec![0u8; 40];
        let bits: u32 = (5 << 5) | (1 << 4);
        frame[..3].copy_from_slice(&bits.to_le_bytes()[..3]);
        frame[3..6].copy_from_slice(&[0x9D, 0x01, 0x2A]);
        frame[6..8].copy_from_slice(&(17u16 | 0xC000).to_le_bytes());
        frame[8..10].copy_from_slice(&9u16.to_le_bytes());
        assert_eq!(
            info(&frame, frame.len()).unwrap(),
            (17, 9),
            "scale bits ignored"
        );
        // The first partition must be shorter than the chunk declares.
        assert_eq!(info(&frame, 6).unwrap(), (17, 9));
        assert!(matches!(info(&frame, 5), Err(ImageError::Malformed(_))));

        let broken = |f: &dyn Fn(&mut Vec<u8>)| {
            let mut copy = frame.clone();
            f(&mut copy);
            info(&copy, copy.len())
        };
        assert!(
            matches!(broken(&|f| f[0] |= 1), Err(ImageError::Malformed(_))),
            "inter frame"
        );
        assert!(
            matches!(broken(&|f| f[0] |= 0b1000), Err(ImageError::Malformed(_))),
            "profile 4"
        );
        assert!(
            matches!(broken(&|f| f[0] &= !0x10), Err(ImageError::Malformed(_))),
            "not shown"
        );
        assert!(
            matches!(broken(&|f| f[4] = 0), Err(ImageError::Malformed(_))),
            "start code"
        );
        assert!(
            matches!(broken(&|f| f[6..8].fill(0)), Err(ImageError::Malformed(_))),
            "no width"
        );
        assert!(
            matches!(broken(&|f| f[2] = 0xFF), Err(ImageError::Malformed(_))),
            "first partition past the end"
        );
        assert_eq!(info(&frame[..9], 40), Err(ImageError::Truncated));
    }

    /// Every lossy fixture in `tests/data`, by name.
    macro_rules! fixtures {
        ($($name:literal),* $(,)?) => {
            [$(($name, include_bytes!(concat!("../../tests/data/", $name, ".webp")).as_slice())),*]
        };
    }

    const FIXTURES: [(&str, &[u8]); 34] = fixtures![
        "webp_alpha_lossless_gradient",
        "webp_alpha_lossless_horizontal",
        "webp_alpha_lossless_none",
        "webp_alpha_lossless_vertical",
        "webp_alpha_raw_gradient",
        "webp_alpha_raw_horizontal",
        "webp_alpha_raw_none",
        "webp_alpha_raw_vertical",
        "webp_lossy_1x1",
        "webp_lossy_alpha",
        "webp_lossy_alpha_levels",
        "webp_lossy_alpha_raw",
        "webp_lossy_big",
        "webp_lossy_colour_space",
        "webp_lossy_column",
        "webp_lossy_even",
        "webp_lossy_libvpx",
        "webp_lossy_noise",
        "webp_lossy_odd",
        "webp_lossy_one_segment",
        "webp_lossy_partitions",
        "webp_lossy_photo",
        "webp_lossy_q100",
        "webp_lossy_q5",
        "webp_lossy_quant_deltas",
        "webp_lossy_row",
        "webp_lossy_segment_deltas",
        "webp_lossy_segments_unmapped",
        "webp_lossy_segments_unvalued",
        "webp_lossy_sharp",
        "webp_lossy_simple",
        "webp_lossy_skip",
        "webp_lossy_strong",
        "webp_lossy_unfiltered",
    ];

    /// What the fixtures use between them, gathered by parsing them with the
    /// decoder's own functions.
    #[derive(Default, Debug)]
    struct Census {
        /// Segmentation: absolute values with a map; values as deltas; no
        /// map; no values.
        segments_absolute: bool,
        segments_delta: bool,
        segments_unmapped: bool,
        segments_unvalued: bool,
        segment_used: [bool; 4],
        filters: [bool; 3],
        sharpness: bool,
        /// Loop-filter deltas under a filter that is on.
        live_filter_deltas: bool,
        /// Filter levels high enough for each high-variance threshold.
        hev_thresholds: [bool; 3],
        partitions: [bool; 4],
        quant_deltas: [bool; 5],
        skip_prob: bool,
        skipped: bool,
        whole_modes: [bool; 4],
        subblock_modes: [bool; 10],
        chroma_modes: [bool; 4],
        /// Coefficient magnitudes by token: 1, 2, 3, 4, then the six
        /// categories.
        tokens: [bool; 10],
    }

    fn token_class(level: u32) -> usize {
        match level {
            0 => usize::MAX,
            1..=4 => level as usize - 1,
            5..=6 => 4,
            7..=10 => 5,
            11..=18 => 6,
            19..=34 => 7,
            35..=66 => 8,
            _ => 9,
        }
    }

    fn take_census(frame: &[u8], census: &mut Census) {
        let Header {
            mb_width,
            mb_height,
            modes: mut first,
            mut tokens,
            segmentation,
            filter,
            quant,
            probs,
            skip_prob,
            ..
        } = read_header(frame, frame.len()).unwrap();
        if segmentation.enabled {
            let valued = segmentation
                .quantizer
                .iter()
                .chain(&segmentation.filter_level)
                .any(|&v| v != 0);
            census.segments_absolute |= segmentation.update_map && segmentation.absolute && valued;
            census.segments_delta |= !segmentation.absolute;
            census.segments_unmapped |= !segmentation.update_map;
            census.segments_unvalued |= !valued;
        }
        let kind = filter.kind();
        census.filters[match kind {
            None => 0,
            Some(Kind::Simple) => 1,
            Some(Kind::Normal) => 2,
        }] = true;
        census.sharpness |= filter.sharpness > 0;
        census.live_filter_deltas |= kind.is_some()
            && filter.use_deltas
            && (filter.ref_delta[0] != 0 || filter.mode_delta[0] != 0);
        for per_segment in filter.strengths(&segmentation) {
            for strength in per_segment {
                if strength.limit > 0 && kind.is_some() {
                    census.hev_thresholds[usize::from(strength.hev_threshold)] = true;
                }
            }
        }
        census.partitions[tokens.len().trailing_zeros() as usize] = true;
        for (seen, &delta) in census.quant_deltas.iter_mut().zip(&quant.deltas) {
            *seen |= delta != 0;
        }
        census.skip_prob |= skip_prob.is_some();

        let quantizers = quantizers(quant, &segmentation);
        let header = MacroblockHeader {
            segmentation: &segmentation,
            skip_prob,
            quantizers: &quantizers,
            probs: &probs,
        };
        let mut above_modes = vec![[0u8; 4]; mb_width];
        let mut above_nonzero = vec![Nonzero::default(); mb_width];
        let partitions = tokens.len();
        let mut residual = Residual::empty();
        for my in 0..mb_height {
            let mut left_modes = [0u8; 4];
            let mut left_nonzero = Nonzero::default();
            for mx in 0..mb_width {
                let neighbours = Neighbours {
                    above_modes: &mut above_modes[mx],
                    left_modes: &mut left_modes,
                    above_nonzero: &mut above_nonzero[mx],
                    left_nonzero: &mut left_nonzero,
                };
                let (modes, _) = read_macroblock(
                    &mut first,
                    &mut tokens[my % partitions],
                    &header,
                    neighbours,
                    &mut residual,
                );
                census.segment_used[modes.segment] = true;
                census.skipped |= skip_prob.is_some() && modes.skip;
                census.chroma_modes[usize::from(modes.chroma)] = true;
                let whole = match modes.luma {
                    Luma::Whole(mode) => {
                        census.whole_modes[usize::from(mode)] = true;
                        true
                    }
                    Luma::Subblocks(sub) => {
                        for mode in sub {
                            census.subblock_modes[usize::from(mode)] = true;
                        }
                        false
                    }
                };
                // Levels back from dequantised coefficients: a luma DC of a
                // whole-predicted macroblock came from the second-order
                // block, and is not a token of its own.
                let q = quantizers[modes.segment];
                for (i, block) in residual.coeffs.iter().enumerate() {
                    let factors = if i < 16 { q.y1 } else { q.uv };
                    for (k, &v) in block.iter().enumerate() {
                        if whole && i < 16 && k == 0 {
                            continue;
                        }
                        let factor = if k == 0 { factors[0] } else { factors[1] };
                        let class = token_class((i32::from(v) / factor).unsigned_abs());
                        if class != usize::MAX {
                            census.tokens[class] = true;
                        }
                    }
                }
            }
        }
    }

    /// The payload of a file's `VP8 ` chunk, found by walking its chunks.
    fn vp8_chunk(file: &[u8]) -> Option<&[u8]> {
        let mut at = 12;
        while let Some(header) = file.get(at..at + 8) {
            let size = u32::from_le_bytes(header[4..8].try_into().unwrap()) as usize;
            let payload = file.get(at + 8..at + 8 + size)?;
            if &header[..4] == b"VP8 " {
                return Some(payload);
            }
            at += 8 + size + (size & 1);
        }
        None
    }

    #[test]
    fn the_fixtures_between_them_use_every_feature_the_format_has() {
        let mut census = Census::default();
        for (name, file) in FIXTURES {
            let frame = vp8_chunk(file).unwrap_or_else(|| panic!("{name}: no VP8 chunk"));
            take_census(frame, &mut census);
        }
        let everything = |flags: &[bool]| flags.iter().all(|&f| f);
        assert!(census.segments_absolute, "{census:?}");
        assert!(census.segments_delta, "{census:?}");
        assert!(census.segments_unmapped, "{census:?}");
        assert!(census.segments_unvalued, "{census:?}");
        assert!(everything(&census.segment_used), "{census:?}");
        assert!(everything(&census.filters), "{census:?}");
        assert!(census.sharpness, "{census:?}");
        assert!(census.live_filter_deltas, "{census:?}");
        assert!(everything(&census.hev_thresholds), "{census:?}");
        assert!(everything(&census.partitions), "{census:?}");
        assert!(everything(&census.quant_deltas), "{census:?}");
        assert!(census.skip_prob && census.skipped, "{census:?}");
        assert!(everything(&census.whole_modes), "{census:?}");
        assert!(everything(&census.subblock_modes), "{census:?}");
        assert!(everything(&census.chroma_modes), "{census:?}");
        assert!(everything(&census.tokens), "{census:?}");
    }
}
