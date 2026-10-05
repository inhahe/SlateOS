//! One frame: its header, its partitions, and its macroblocks, row by row.
//!
//! The first partition holds the frame header and every macroblock's modes;
//! up to eight more hold the coefficients, macroblock row `r` in partition
//! `r % count`. Each macroblock is predicted (from its neighbours or a
//! reference frame), its residual added, and each row loop-filtered once the
//! row below it is reconstructed, since intra prediction reads unfiltered
//! pixels. The borders are then filled with copies of the edges, for the
//! frames that will predict from this one. A frame of several partitions
//! does its rows on several threads instead (`threading`), to the same
//! pixels.
//!
//! What lasts between frames is here too ([`Common`]): the probabilities a
//! frame may keep for the next, the segmentation and loop filter settings,
//! the mode grid (whose segment numbers a frame may keep), and the token
//! partition count, which a frame that cannot read its own keeps from the
//! frame before.
//!
//! Translated into Rust from libvpx v1.17.0's `vp8/decoder/decodeframe.c`,
//! `vp8/common/quant_common.c` and `vp8/common/entropymode.c`
//! (`vp8_init_mbmode_probs`), copyright the WebM project authors, used under
//! libvpx's BSD licence and patent grant (`licenses/libvpx-LICENSE`,
//! `licenses/libvpx-PATENTS`).

#![allow(
    clippy::indexing_slicing,
    reason = "block and context arrays are fixed-size and indexed by loop positions within them; grid cells by macroblock positions inside the grid; quantiser tables by indices clamped to 0..=127; partitions by a row number modulo their count; reference frames by numbers masked to 0..=3"
)]
#![allow(
    clippy::arithmetic_side_effects,
    reason = "positions and sizes are bounded by the frame's (at most 16384 pixels a side); quantiser arithmetic is on values below 300; coefficient products wrap at 16 bits through explicit casts, as libvpx's do"
)]
#![allow(
    clippy::cast_possible_truncation,
    reason = "libvpx keeps dequantised coefficients in 16 bits and lets them wrap; the casts are that wrap"
)]
#![allow(
    clippy::cast_possible_wrap,
    reason = "the header's signed values are magnitudes of at most 7 bits, which fit an i8"
)]

use crate::Error;
use crate::boolread::BoolDecoder;
use crate::frame::{Frame, Target};
use crate::header::{START_CODE, Tag, VersionSetup};
use crate::idct;
use crate::inter::{self, Filter, InterContext, Prior};
use crate::intra;
use crate::loopfilter::{self, Adjustments, LoopFilterInfo};
use crate::modes::{
    self, ALTREF_FRAME, B_PRED, Edges, GOLDEN_FRAME, INTRA_FRAME, LAST_FRAME, MVP_COUNT, ModeGrid,
    ModeHeader, ModeInfo, SPLITMV,
};
use crate::tables::{
    AC_QLOOKUP, COEF_UPDATE_PROBS, DC_QLOOKUP, DEFAULT_COEF_PROBS, DEFAULT_MV_CONTEXT,
    UV_MODE_PROB, YMODE_PROB,
};
use crate::threading;
use crate::tokens::{self, CoefProbs, Context};

/// The probabilities a frame may update and the next frame keep: the parts
/// of libvpx's `FRAME_CONTEXT` that a decoder changes.
#[derive(Clone, Debug)]
pub(crate) struct FrameContext {
    ymode_prob: [u8; 4],
    uv_mode_prob: [u8; 3],
    coef_probs: CoefProbs,
    mvc: [[u8; MVP_COUNT]; 2],
}

impl FrameContext {
    /// What a key frame resets them to: libvpx's `vp8_init_mbmode_probs`,
    /// `vp8_default_coef_probs` and `vp8_default_mv_context`.
    fn key_frame_defaults() -> Self {
        Self {
            ymode_prob: YMODE_PROB,
            uv_mode_prob: UV_MODE_PROB,
            coef_probs: DEFAULT_COEF_PROBS,
            mvc: DEFAULT_MV_CONTEXT,
        }
    }
}

/// Segmentation: up to four classes of macroblock, each with its own
/// quantiser and loop filter level.
#[derive(Clone, Copy, Debug, Default)]
struct Segmentation {
    enabled: bool,
    /// Whether this frame codes each macroblock's segment.
    update_map: bool,
    /// Whether the values are the levels themselves, rather than changes to
    /// the frame's: libvpx's `mb_segment_abs_delta`.
    absolute: bool,
    /// By feature (quantiser, loop filter level) and segment.
    feature_data: [[i8; 4]; 2],
    /// The probabilities segment numbers are coded with.
    tree_probs: [u8; 3],
}

/// The loop filter's changes by reference frame and by mode class.
#[derive(Clone, Copy, Debug, Default)]
struct LfDeltas {
    enabled: bool,
    ref_deltas: [i8; 4],
    mode_deltas: [i8; 4],
}

/// The quantiser index and its changes for each kind of coefficient.
#[derive(Clone, Copy, Debug, Default)]
struct Quant {
    base: u8,
    y1dc: i8,
    y2dc: i8,
    y2ac: i8,
    uvdc: i8,
    uvac: i8,
}

/// A macroblock's dequantisation factors, `[0]` for the DC and the rest for
/// the AC coefficients: libvpx's `dequant_y1` and the rest. `y1_dc` is luma's
/// when its DC comes from the second-order block, already dequantised.
#[derive(Clone, Copy, Debug)]
struct Dequant {
    y1: [i16; 16],
    y1_dc: [i16; 16],
    y2: [i16; 16],
    uv: [i16; 16],
}

impl Quant {
    /// The factors for quantiser index `q`: libvpx's `vp8cx_init_de_quantizer`
    /// and `vp8_mb_init_dequantizer`, from `quant_common.c`'s lookups.
    fn dequant(&self, q: i32) -> Dequant {
        let index = |delta: i8| (q + i32::from(delta)).clamp(0, 127) as usize;
        let dc = |delta: i8| i32::from(DC_QLOOKUP[index(delta)]);
        let ac = |delta: i8| i32::from(AC_QLOOKUP[index(delta)]);
        let y1 = (dc(self.y1dc), ac(0));
        // 155/100 of the AC step, in libvpx's exact integer form.
        let y2 = (dc(self.y2dc) * 2, ((ac(self.y2ac) * 101_581) >> 16).max(8));
        let uv = (dc(self.uvdc).min(132), ac(self.uvac));
        let factors = |(d, a): (i32, i32)| {
            let mut f = [a as i16; 16];
            f[0] = d as i16;
            f
        };
        let y1 = factors(y1);
        let mut y1_dc = y1;
        y1_dc[0] = 1;
        Dequant {
            y1,
            y1_dc,
            y2: factors(y2),
            uv: factors(uv),
        }
    }
}

/// What lasts from frame to frame: libvpx's `VP8_COMMON`, and the parts of
/// its `VP8D_COMP` and `MACROBLOCKD` that outlive a frame.
#[derive(Clone, Debug)]
pub(crate) struct Common {
    /// The picture's size, from the last key frame: libvpx's `Width` and
    /// `Height`.
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) mb_rows: usize,
    pub(crate) mb_cols: usize,
    /// The last frame's type and whether it is shown.
    pub(crate) key_frame: bool,
    pub(crate) show_frame: bool,
    /// The last key frame's colour space bit (0 for VP8's YUV, which the
    /// specification likens to BT.601; 1 reserved) and clamping type bit (0
    /// if pixels must be clamped). libvpx reads both and goes by neither;
    /// they are kept for a caller choosing a colour, as FFmpeg does.
    pub(crate) color_space: u8,
    pub(crate) clamping_type: u8,
    /// Log2 of the token partitions' count.
    multi_token_partition: u8,
    quant: Quant,
    pub(crate) refresh_golden: bool,
    pub(crate) refresh_altref: bool,
    pub(crate) refresh_last: bool,
    /// Which frame the golden (or altref) reference becomes, when not this
    /// one: 0 none, 1 the last, 2 the altref (or golden); 3 is invalid.
    pub(crate) copy_to_gf: u8,
    pub(crate) copy_to_arf: u8,
    refresh_entropy_probs: bool,
    /// By reference frame: whether its motion vectors point back in time.
    sign_bias: [bool; 4],
    fc: FrameContext,
    /// The probabilities to go back to after a frame that does not keep its
    /// own: libvpx's `lfc`.
    lfc: FrameContext,
    pub(crate) grid: ModeGrid,
    /// By macroblock column: whether the blocks above had coefficients.
    above: Vec<Context>,
    seg: Segmentation,
    lf: LfDeltas,
    /// Whether a key frame has decoded since the decoder was made or the
    /// size changed: libvpx's `decoded_key_frame`.
    pub(crate) decoded_key_frame: bool,
}

impl Common {
    /// A decoder's state before its first frame: libvpx's
    /// `create_decompressor` and `vp8_create_common`.
    pub(crate) fn new() -> Self {
        let fc = FrameContext::key_frame_defaults();
        Self {
            width: 0,
            height: 0,
            mb_rows: 0,
            mb_cols: 0,
            key_frame: true,
            show_frame: false,
            color_space: 0,
            clamping_type: 0,
            multi_token_partition: 0,
            quant: Quant::default(),
            refresh_golden: false,
            refresh_altref: false,
            refresh_last: false,
            copy_to_gf: 0,
            copy_to_arf: 0,
            refresh_entropy_probs: true,
            sign_bias: [false; 4],
            lfc: fc.clone(),
            fc,
            grid: ModeGrid::new(0, 0),
            above: Vec::new(),
            seg: Segmentation::default(),
            lf: LfDeltas::default(),
            decoded_key_frame: false,
        }
    }

    /// Size the per-macroblock state for a picture of `width` by `height`,
    /// cleared: the part of libvpx's `vp8_alloc_frame_buffers` that is not
    /// frames.
    pub(crate) fn allocate(&mut self, width: u32, height: u32) {
        self.mb_cols = (width as usize).div_ceil(16);
        self.mb_rows = (height as usize).div_ceil(16);
        self.grid = ModeGrid::new(self.mb_cols, self.mb_rows);
        self.above = vec![[0; 9]; self.mb_cols];
    }

    /// The token contexts above each macroblock column, as the last frame
    /// left them.
    #[cfg(test)]
    pub(crate) fn above(&self) -> &[Context] {
        &self.above
    }

    /// Free it, as libvpx's `vp8_de_alloc_frame_buffers` does when a new
    /// size cannot be allocated.
    pub(crate) fn deallocate(&mut self) {
        self.mb_cols = 0;
        self.mb_rows = 0;
        self.grid = ModeGrid::new(0, 0);
        self.above = Vec::new();
    }
}

/// Why a frame did not decode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FrameError {
    /// libvpx's `vpx_internal_error`: the frame is damaged or unsupported.
    Error(Error),
    /// An inter frame before any key frame has decoded: `vp8_decode_frame`'s
    /// return of -1.
    NoKeyFrameYet,
}

fn corrupt(why: &'static str) -> FrameError {
    FrameError::Error(Error::Corrupt(why))
}

/// The reference frames an inter frame predicts from, and whether each is
/// itself corrupt.
pub(crate) struct Refs<'a> {
    pub(crate) last: &'a Frame,
    pub(crate) golden: &'a Frame,
    pub(crate) altref: &'a Frame,
    pub(crate) corrupted: [bool; 4],
}

/// Decode `data` into `new`: libvpx's `vp8_decode_frame`. `stale` is what
/// `new`'s buffer held before, if `new` is not that buffer. The macroblock
/// rows decode on up to `threads` threads. Returns whether the frame is
/// corrupt: decoded past the end of a partition, or predicted from a
/// corrupt frame.
pub(crate) fn decode_frame(
    c: &mut Common,
    data: &[u8],
    new: &mut Frame,
    stale: Option<&Frame>,
    refs: &Refs<'_>,
    threads: usize,
) -> Result<bool, FrameError> {
    let [b0, b1, b2, ..] = *data else {
        return Err(corrupt("a frame shorter than its three-byte tag"));
    };
    let tag = Tag::read([b0, b1, b2]);
    c.key_frame = tag.key_frame;
    c.show_frame = tag.show_frame;
    if tag.first_partition_len == 0 {
        return Err(corrupt("a first partition of length 0"));
    }
    let setup = VersionSetup::of(tag.version);
    let mut start = 3;
    if tag.key_frame {
        let Some(h) = data.get(3..10) else {
            return Err(corrupt("a key frame header cut short"));
        };
        if h[..3] != START_CODE {
            return Err(FrameError::Error(Error::Unsupported(
                "a key frame without VP8's start code",
            )));
        }
        c.width = u32::from(u16::from_le_bytes([h[3], h[4]]) & 0x3fff);
        c.height = u32::from(u16::from_le_bytes([h[5], h[6]]) & 0x3fff);
        start = 10;
    }
    if !c.decoded_key_frame && !tag.key_frame {
        return Err(FrameError::NoKeyFrameYet);
    }
    let first_len = tag.first_partition_len as usize;
    if data.len() - start < first_len {
        return Err(corrupt("a first partition longer than the frame"));
    }

    // libvpx's `init_frame`.
    if tag.key_frame {
        c.fc = FrameContext::key_frame_defaults();
        c.seg.feature_data = [[0; 4]; 2];
        c.seg.absolute = false;
        c.lf.ref_deltas = [0; 4];
        c.lf.mode_deltas = [0; 4];
        c.refresh_golden = true;
        c.refresh_altref = true;
        c.copy_to_gf = 0;
        c.copy_to_arf = 0;
        c.sign_bias[usize::from(GOLDEN_FRAME)] = false;
        c.sign_bias[usize::from(ALTREF_FRAME)] = false;
    }

    // The first partition's decoder reads to the end of the frame, as
    // libvpx's does: a header that runs past its partition reads on into the
    // partition sizes and the coefficients.
    let mut bc = BoolDecoder::new(&data[start..]);
    if tag.key_frame {
        c.color_space = u8::from(bc.read_bit());
        // libvpx's `clamp_type`: its decoder clamps whatever this says.
        c.clamping_type = u8::from(bc.read_bit());
    }
    read_segmentation(c, &mut bc);
    let simple_filter = bc.read_bit();
    let filter_level = bc.read_u8(6);
    let sharpness = bc.read_u8(3);
    read_lf_deltas(c, &mut bc);
    let mut partitions = setup_token_decoder(c, &mut bc, data, start + first_len)?;

    let delta = |bc: &mut BoolDecoder<'_>| {
        if bc.read_bit() {
            let v = bc.read_u8(4) as i8;
            if bc.read_bit() { -v } else { v }
        } else {
            0
        }
    };
    c.quant.base = bc.read_u8(7);
    c.quant.y1dc = delta(&mut bc);
    c.quant.y2dc = delta(&mut bc);
    c.quant.y2ac = delta(&mut bc);
    c.quant.uvdc = delta(&mut bc);
    c.quant.uvac = delta(&mut bc);

    if !tag.key_frame {
        c.refresh_golden = bc.read_bit();
        c.refresh_altref = bc.read_bit();
        c.copy_to_gf = if c.refresh_golden { 0 } else { bc.read_u8(2) };
        c.copy_to_arf = if c.refresh_altref { 0 } else { bc.read_u8(2) };
        c.sign_bias[usize::from(GOLDEN_FRAME)] = bc.read_bit();
        c.sign_bias[usize::from(ALTREF_FRAME)] = bc.read_bit();
    }
    c.refresh_entropy_probs = bc.read_bit();
    if !c.refresh_entropy_probs {
        c.lfc = c.fc.clone();
    }
    c.refresh_last = tag.key_frame || bc.read_bit();

    for (i, types) in COEF_UPDATE_PROBS.iter().enumerate() {
        for (j, bands) in types.iter().enumerate() {
            for (k, contexts) in bands.iter().enumerate() {
                for (l, &update) in contexts.iter().enumerate() {
                    if bc.read(update) {
                        c.fc.coef_probs[i][j][k][l] = bc.read_u8(8);
                    }
                }
            }
        }
    }

    let (skip_coded, prob_skip_false, prob_intra, prob_last, prob_gf) = modes::read_mode_probs(
        &mut bc,
        tag.key_frame,
        &mut c.fc.ymode_prob,
        &mut c.fc.uv_mode_prob,
        &mut c.fc.mvc,
    );
    let header = ModeHeader {
        key_frame: tag.key_frame,
        update_segment_map: c.seg.update_map,
        segment_tree_probs: c.seg.tree_probs,
        skip_coded,
        prob_skip_false,
        prob_intra,
        prob_last,
        prob_gf,
        ymode_prob: &c.fc.ymode_prob,
        uv_mode_prob: &c.fc.uv_mode_prob,
        mvc: &c.fc.mvc,
        sign_bias: c.sign_bias,
    };
    modes::decode_mode_mvs(&mut bc, &header, &mut c.grid);

    c.above.fill([0; 9]);
    let lf = (filter_level != 0).then(|| {
        let adj = Adjustments {
            segments: c
                .seg
                .enabled
                .then_some((c.seg.absolute, c.seg.feature_data[1])),
            deltas: c.lf.enabled.then_some((c.lf.ref_deltas, c.lf.mode_deltas)),
        };
        LoopFilterInfo::new(filter_level, sharpness, &adj)
    });
    // libvpx's `vp8_mb_init_dequantizer`, for each segment a macroblock may
    // name; without segmentation, each is the frame's.
    let dequant = core::array::from_fn(|segment| {
        let q = if c.seg.enabled {
            let v = i32::from(c.seg.feature_data[0][segment]);
            let q = if c.seg.absolute {
                v
            } else {
                i32::from(c.quant.base) + v
            };
            q.clamp(0, 127)
        } else {
            i32::from(c.quant.base)
        };
        c.quant.dequant(q)
    });
    let rows = Rows {
        filter: if setup.bilinear {
            Filter::Bilinear
        } else {
            Filter::SixTap
        },
        fullpixel_mask: if setup.full_pixel { !7 } else { !0 },
        lf: lf.as_ref(),
        simple_filter,
        key_frame: tag.key_frame,
        mb_rows: c.mb_rows,
        mb_cols: c.mb_cols,
        coef_probs: &c.fc.coef_probs,
        dequant,
    };
    let corrupt_tokens = decode_mb_rows(
        &rows,
        &mut c.grid,
        &mut c.above,
        &mut partitions,
        (new, stale),
        refs,
        threads,
    );
    let corrupted = bc.has_error() || corrupt_tokens;

    if !c.decoded_key_frame {
        if tag.key_frame && !corrupted {
            c.decoded_key_frame = true;
        } else {
            return Err(corrupt("a stream must start with a complete key frame"));
        }
    }
    if !c.refresh_entropy_probs {
        c.fc = c.lfc.clone();
    }
    Ok(corrupted)
}

/// The segmentation part of the header.
fn read_segmentation(c: &mut Common, bc: &mut BoolDecoder<'_>) {
    let seg = &mut c.seg;
    seg.enabled = bc.read_bit();
    if !seg.enabled {
        seg.update_map = false;
        return;
    }
    seg.update_map = bc.read_bit();
    let update_data = bc.read_bit();
    if update_data {
        seg.absolute = bc.read_bit();
        // Quantiser values have 7 bits, loop filter levels 6.
        for (feature, bits) in seg.feature_data.iter_mut().zip([7, 6]) {
            for value in feature.iter_mut() {
                *value = if bc.read_bit() {
                    let v = bc.read_u8(bits) as i8;
                    if bc.read_bit() { -v } else { v }
                } else {
                    0
                };
            }
        }
    }
    if seg.update_map {
        seg.tree_probs = [255; 3];
        for p in &mut seg.tree_probs {
            if bc.read_bit() {
                *p = bc.read_u8(8);
            }
        }
    }
}

/// The loop filter's reference and mode changes, which a frame may update.
fn read_lf_deltas(c: &mut Common, bc: &mut BoolDecoder<'_>) {
    c.lf.enabled = bc.read_bit();
    if !c.lf.enabled || !bc.read_bit() {
        return;
    }
    for d in
        c.lf.ref_deltas
            .iter_mut()
            .chain(c.lf.mode_deltas.iter_mut())
    {
        if bc.read_bit() {
            let v = bc.read_u8(6) as i8;
            *d = if bc.read_bit() { -v } else { v };
        }
    }
}

/// The token partitions: libvpx's `setup_token_decoder` for a frame that
/// arrives whole. `sizes_at` is where the partition sizes start: after the
/// first partition. A partition the frame ends before is empty, as libvpx's
/// are.
fn setup_token_decoder<'d>(
    c: &mut Common,
    bc: &mut BoolDecoder<'_>,
    data: &'d [u8],
    sizes_at: usize,
) -> Result<Vec<BoolDecoder<'d>>, FrameError> {
    let count = bc.read_u8(2);
    if !bc.has_error() {
        c.multi_token_partition = count;
    }
    let num = 1usize << c.multi_token_partition;
    let first_end = sizes_at + 3 * (num - 1);
    if data.len() < first_end {
        return Err(corrupt("a frame too short for its partition sizes"));
    }
    let mut parts: Vec<&'d [u8]> = vec![&[]; num];
    let mut pos = first_end;
    let mut i = 0;
    while pos < data.len() {
        let bytes_left = data.len() - pos;
        // The last partition's size is what is left; the others' are 24-bit
        // numbers after the first partition.
        let size = if i < num - 1 {
            let at = sizes_at + 3 * i;
            let Some(&[s0, s1, s2]) = data.get(at..at + 3) else {
                return Err(corrupt("truncated partition size data"));
            };
            usize::from(s0) | usize::from(s1) << 8 | usize::from(s2) << 16
        } else {
            bytes_left
        };
        if size == 0 || size > bytes_left {
            return Err(corrupt("a token partition longer than the frame, or empty"));
        }
        parts[i] = &data[pos..pos + size];
        pos += size;
        i += 1;
    }
    Ok(parts.into_iter().map(BoolDecoder::new).collect())
}

/// What decoding the rows needs from the header: the same for every
/// macroblock, and shared by the threads that decode them.
pub(crate) struct Rows<'a> {
    pub(crate) filter: Filter,
    pub(crate) fullpixel_mask: i16,
    pub(crate) lf: Option<&'a LoopFilterInfo>,
    pub(crate) simple_filter: bool,
    pub(crate) key_frame: bool,
    pub(crate) mb_rows: usize,
    pub(crate) mb_cols: usize,
    pub(crate) coef_probs: &'a CoefProbs,
    /// By segment: its dequantisation factors.
    dequant: [Dequant; 4],
}

/// A macroblock's coefficients and their end positions, which outlast it:
/// libvpx's `qcoeff` and `eobs`. Reconstruction clears what it used, so the
/// coefficients are zero between macroblocks.
pub(crate) struct Residual {
    qcoeff: [[i16; 16]; 25],
    eobs: [u8; 25],
}

impl Residual {
    pub(crate) fn new() -> Self {
        Self {
            qcoeff: [[0; 16]; 25],
            eobs: [0; 25],
        }
    }
}

/// Decode, predict, reconstruct and filter every macroblock into `new`,
/// whose buffer held `stale` before if it is not that buffer: libvpx's
/// `decode_mb_rows`, or its `vp8mt_decode_mb_rows` on up to `threads`
/// threads when the frame has several token partitions. Returns whether a
/// token partition ran dry or a reference was corrupt.
fn decode_mb_rows(
    rows: &Rows<'_>,
    grid: &mut ModeGrid,
    above: &mut [Context],
    partitions: &mut [BoolDecoder<'_>],
    (new, stale): (&mut Frame, Option<&Frame>),
    refs: &Refs<'_>,
    threads: usize,
) -> bool {
    if let Some(corrupted) =
        threading::decode_mb_rows(rows, grid, above, partitions, new, refs, threads)
    {
        return corrupted;
    }
    let prior = stale.map_or(Prior::InPlace, Prior::Copy);
    let (mb_rows, mb_cols) = (rows.mb_rows, rows.mb_cols);
    let mut corrupted = false;
    let mut residual = Residual::new();
    new.setup_intra_top_line();
    for mb_row in 0..mb_rows {
        let part = mb_row % partitions.len().max(1);
        let mut left: Context = [0; 9];
        new.setup_intra_left(mb_row);
        for mb_col in 0..mb_cols {
            let idx = grid.index(mb_row, mb_col);
            let mut mi = grid.cells[idx];
            corrupted |= refs.corrupted[usize::from(mi.ref_frame & 3)];
            let (Some(bc), Some(ctx)) = (partitions.get_mut(part), above.get_mut(mb_col)) else {
                break;
            };
            let at = MbAt {
                mb_row,
                mb_col,
                left_available: mb_col > 0,
                up_available: mb_row > 0,
            };
            let predicted = decode_macroblock(
                rows,
                &at,
                &mut mi,
                (bc, ctx, &mut left),
                &mut residual,
                &mut new.target(),
                prior,
                refs,
            );
            // Only a thread's band leaves a macroblock unpredicted.
            debug_assert!(predicted, "a frame's own buffer always predicts");
            // Coefficients found to be none skip the loop filter's inner
            // edges, as a coded skip does.
            grid.cells[idx].mb_skip_coeff = mi.mb_skip_coeff;
            corrupted |= bc.has_error();
        }
        new.extend_mb_row(mb_row);
        if let Some(lfi) = rows.lf {
            if mb_row > 0 {
                loopfilter::filter_row(
                    new,
                    grid,
                    lfi,
                    mb_row - 1,
                    rows.simple_filter,
                    rows.key_frame,
                );
                if mb_row > 1 {
                    new.extend_row_left_right(mb_row - 2);
                }
            }
        } else if mb_row > 0 {
            new.extend_row_left_right(mb_row - 1);
        }
    }
    if mb_rows > 0 {
        if let Some(lfi) = rows.lf {
            loopfilter::filter_row(
                new,
                grid,
                lfi,
                mb_rows - 1,
                rows.simple_filter,
                rows.key_frame,
            );
            if mb_rows > 1 {
                new.extend_row_left_right(mb_rows - 2);
            }
        }
        new.extend_row_left_right(mb_rows - 1);
        new.extend_top_bottom();
    }
    corrupted
}

/// Where a macroblock is, and which of its neighbours exist.
pub(crate) struct MbAt {
    pub(crate) mb_row: usize,
    pub(crate) mb_col: usize,
    pub(crate) left_available: bool,
    pub(crate) up_available: bool,
}

/// One macroblock: libvpx's `decode_macroblock`. `tokens` is the partition
/// it reads its coefficients from and the token contexts above and left of
/// it. `false`, with the macroblock half made, if its chroma is one libvpx
/// leaves as the buffer held it and `prior` has nothing to leave.
#[allow(
    clippy::too_many_arguments,
    reason = "libvpx's MACROBLOCKD, split into what each part of it is"
)]
#[must_use]
pub(crate) fn decode_macroblock(
    rows: &Rows<'_>,
    at: &MbAt,
    mi: &mut ModeInfo,
    (bc, above, left): (&mut BoolDecoder<'_>, &mut Context, &mut Context),
    r: &mut Residual,
    new: &mut Target<'_>,
    prior: Prior<'_>,
    refs: &Refs<'_>,
) -> bool {
    if mi.mb_skip_coeff {
        tokens::reset_mb_tokens_context(mi.is_4x4, above, left);
    } else if !bc.has_error() {
        let eobtotal = tokens::decode_mb_tokens(
            bc,
            rows.coef_probs,
            mi.is_4x4,
            above,
            left,
            &mut r.qcoeff,
            &mut r.eobs,
        );
        mi.mb_skip_coeff = eobtotal == 0;
    }
    // A partition that has run dry decodes no coefficients, and the
    // macroblock keeps the end positions of the one before; its
    // coefficients are all zero, so they add nothing.

    let d = &rows.dequant[usize::from(mi.segment_id & 3)];
    let (mb_x, mb_y) = (at.mb_col * 16, at.mb_row * 16);
    let mode = mi.mode;
    if mi.ref_frame == INTRA_FRAME {
        for p in 1..3 {
            let (pos, stride) = (new.at(p, mb_x / 2, mb_y / 2), new.strides[p]);
            intra::predict_mb(
                mi.uv_mode,
                8,
                at.left_available,
                at.up_available,
                new.planes[p],
                pos,
                stride,
            );
        }
        let (pos, stride) = (new.at(0, mb_x, mb_y), new.strides[0]);
        let y = &mut *new.planes[0];
        if mode == B_PRED {
            if mi.mb_skip_coeff {
                r.eobs = [0; 25];
            }
            intra::down_copy_above_right(y, pos, stride);
            for i in 0..16 {
                let b = pos + (i >> 2) * 4 * stride + (i & 3) * 4;
                intra::predict_4x4(mi.bmodes[i], y, b, stride);
                match r.eobs[i] {
                    0 => {}
                    1 => {
                        let dc = (i32::from(r.qcoeff[i][0]) * i32::from(d.y1[0])) as i16;
                        idct::dc_only_idct_add(dc, y, b, stride);
                        r.qcoeff[i][0] = 0;
                        r.qcoeff[i][1] = 0;
                    }
                    _ => idct::dequant_idct_add(&mut r.qcoeff[i], &d.y1, y, b, stride),
                }
            }
        } else {
            intra::predict_mb(mode, 16, at.left_available, at.up_available, y, pos, stride);
        }
    } else {
        let refp = match mi.ref_frame {
            LAST_FRAME => refs.last,
            GOLDEN_FRAME => refs.golden,
            _ => refs.altref,
        };
        let ctx = InterContext {
            filter: rows.filter,
            fullpixel_mask: rows.fullpixel_mask,
            edges: Edges::of(at.mb_row, at.mb_col, rows.mb_rows, rows.mb_cols),
        };
        if !inter::predict_mb(&ctx, mi, refp, new, prior, mb_x, mb_y) {
            return false;
        }
    }

    if mi.mb_skip_coeff {
        return true;
    }
    if mode != B_PRED {
        let mut dq_y = &d.y1;
        if mode != SPLITMV {
            // The second-order block carries the sixteen luma DCs, which
            // arrive dequantised: libvpx's `dequant_y1_dc` multiplies them
            // by 1.
            let (y_blocks, rest) = r.qcoeff.split_at_mut(16);
            let y2 = &mut rest[8];
            if r.eobs[24] > 1 {
                let mut dq = [0i16; 16];
                for ((o, &qc), &f) in dq.iter_mut().zip(y2.iter()).zip(&d.y2) {
                    *o = (i32::from(qc) * i32::from(f)) as i16;
                }
                idct::inv_walsh4x4(&dq, y_blocks);
                *y2 = [0; 16];
            } else {
                let dc = (i32::from(y2[0]) * i32::from(d.y2[0])) as i16;
                idct::inv_walsh4x4_dc(dc, y_blocks);
                y2[0] = 0;
                y2[1] = 0;
            }
            dq_y = &d.y1_dc;
        }
        let (pos, stride) = (new.at(0, mb_x, mb_y), new.strides[0]);
        idct::add_y_blocks(
            &mut r.qcoeff[..16],
            dq_y,
            &r.eobs[..16],
            new.planes[0],
            pos,
            stride,
        );
    }
    for (p, blocks) in [(1, 16..20), (2, 20..24)] {
        let (pos, stride) = (new.at(p, mb_x / 2, mb_y / 2), new.strides[p]);
        let eobs = &r.eobs[blocks.clone()];
        idct::add_uv_blocks(
            &mut r.qcoeff[blocks],
            &d.uv,
            eobs,
            new.planes[p],
            pos,
            stride,
        );
    }
    true
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "a test: a failure should be loud")]

    use super::*;

    #[test]
    fn dequantisation_follows_libvpx_s_quant_common() {
        let q = Quant {
            base: 0,
            y1dc: 0,
            y2dc: 0,
            y2ac: 0,
            uvdc: 0,
            uvac: 0,
        };
        let d = q.dequant(0);
        assert_eq!((d.y1[0], d.y1[1]), (4, 4));
        assert_eq!(d.y1_dc[0], 1);
        // y2: DC doubled; AC 155/100 but at least 8.
        assert_eq!((d.y2[0], d.y2[1]), (8, 8));
        let d = q.dequant(127);
        assert_eq!((d.y1[0], d.y1[1]), (157, 284));
        // (284 * 101581) >> 16 = 440, which is 284 * 155 / 100 rounded down.
        assert_eq!((d.y2[0], d.y2[1]), (314, 440));
        // Chroma's DC step stops at 132.
        assert_eq!((d.uv[0], d.uv[1]), (132, 284));
        // Indices clamp at both ends.
        let low = Quant { y1dc: -15, ..q };
        assert_eq!(low.dequant(3).y1[0], 4);
    }

    #[test]
    fn a_missing_partition_is_empty() {
        let mut c = Common::new();
        // Four partitions: a header decoder that has run dry cannot read the
        // count, so the frame keeps the one it had.
        c.multi_token_partition = 2;
        let mut dry = BoolDecoder::new(&[]);
        // Two bytes of first partition, three sizes of 1, and one byte of
        // coefficients: the frame ends after the first token partition.
        let mut frame = vec![0u8; 2];
        frame.extend_from_slice(&[1, 0, 0, 1, 0, 0, 1, 0, 0]);
        frame.push(0xaa);
        let parts = setup_token_decoder(&mut c, &mut dry, &frame, 2).unwrap();
        assert_eq!(parts.len(), 4);
        assert!(!parts[0].has_error());
        assert!(parts[1..].iter().all(BoolDecoder::has_error));
    }

    /// Numerical Recipes' generator.
    struct Lcg(u32);

    impl Lcg {
        fn below(&mut self, n: usize) -> usize {
            self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (self.0 >> 8) as usize % n.max(1)
        }

        fn chance(&mut self, one_in: usize) -> bool {
            self.below(one_in) == 0
        }

        fn byte(&mut self) -> u8 {
            self.below(256) as u8
        }

        fn mv(&mut self, reach: usize) -> modes::Mv {
            let mut part = || (self.below(2 * reach + 1) as i32 - reach as i32) as i16;
            modes::Mv {
                row: part(),
                col: part(),
            }
        }
    }

    /// A frame of `w` x `h` pixels holding noise, borders and all.
    fn noise(rng: &mut Lcg, w: u32, h: u32) -> Frame {
        let mut f = Frame::new(w, h);
        for plane in &mut f.planes {
            plane.data.fill_with(|| rng.byte());
        }
        f
    }

    /// Modes at random: any intra mode, or any inter mode with vectors near
    /// the macroblock or, if `wild`, far outside the picture.
    fn random_mode(rng: &mut Lcg, key_frame: bool, wild: bool) -> ModeInfo {
        let mut mi = ModeInfo {
            segment_id: rng.below(4) as u8,
            mb_skip_coeff: rng.chance(3),
            ..ModeInfo::default()
        };
        if key_frame || rng.chance(3) {
            mi.ref_frame = INTRA_FRAME;
            mi.mode = rng.below(usize::from(B_PRED) + 1) as u8;
            mi.uv_mode = rng.below(4) as u8;
            for b in &mut mi.bmodes {
                *b = rng.below(10) as u8;
            }
        } else {
            mi.ref_frame = LAST_FRAME + rng.below(3) as u8;
            mi.mode = modes::NEARESTMV + rng.below(5) as u8;
            let reach = if wild { 2400 } else { 160 };
            mi.mv = rng.mv(reach);
            mi.partitioning = rng.below(4) as u8;
            for b in &mut mi.bmvs {
                *b = rng.mv(reach);
            }
            mi.need_to_clamp_mvs = rng.chance(2);
        }
        mi.is_4x4 = mi.mode == B_PRED || mi.mode == SPLITMV;
        mi
    }

    #[test]
    fn rows_decode_alike_on_any_number_of_threads() {
        let mut rng = Lcg(0x0f08_7eed);
        let (threaded, fell_back) = threading::TALLY.get();
        for case in 0..240 {
            let (w, h) = (8 + rng.below(90) as u32, 17 + rng.below(100) as u32);
            let mut c = Common::new();
            c.allocate(w, h);
            let key_frame = rng.chance(4);
            let wild = rng.chance(3);
            for mb_row in 0..c.mb_rows {
                for mb_col in 0..c.mb_cols {
                    let idx = c.grid.index(mb_row, mb_col);
                    c.grid.cells[idx] = random_mode(&mut rng, key_frame, wild);
                }
            }
            let references = [
                noise(&mut rng, w, h),
                noise(&mut rng, w, h),
                noise(&mut rng, w, h),
            ];
            let refs = Refs {
                last: &references[0],
                golden: &references[1],
                altref: &references[2],
                corrupted: [false, rng.chance(8), rng.chance(8), rng.chance(8)],
            };
            let count = 1 << rng.below(4);
            let data: Vec<Vec<u8>> = (0..count)
                .map(|_| (0..rng.below(400)).map(|_| rng.byte()).collect())
                .collect();
            let lfi = LoopFilterInfo::new(
                rng.below(64) as u8,
                rng.below(8) as u8,
                &Adjustments::default(),
            );
            let quant = Quant {
                base: rng.below(128) as u8,
                ..Quant::default()
            };
            let rows = Rows {
                filter: if rng.chance(2) {
                    Filter::SixTap
                } else {
                    Filter::Bilinear
                },
                fullpixel_mask: if rng.chance(2) { !7 } else { !0 },
                lf: (!rng.chance(5)).then_some(&lfi),
                simple_filter: rng.chance(2),
                key_frame,
                mb_rows: c.mb_rows,
                mb_cols: c.mb_cols,
                coef_probs: &DEFAULT_COEF_PROBS,
                dequant: core::array::from_fn(|_| quant.dequant(rng.below(128) as i32)),
            };
            // What the buffer held, and, half the time, the buffer the
            // caller kept, which a fresh buffer takes the place of.
            let prior = noise(&mut rng, w, h);
            let stale = rng.chance(2).then(|| noise(&mut rng, w, h));
            let run = |threads: usize| {
                let mut new = prior.clone();
                let mut grid = c.grid.clone();
                let mut above = c.above.clone();
                let mut partitions: Vec<BoolDecoder<'_>> =
                    data.iter().map(|d| BoolDecoder::new(d)).collect();
                let corrupted = decode_mb_rows(
                    &rows,
                    &mut grid,
                    &mut above,
                    &mut partitions,
                    (&mut new, stale.as_ref()),
                    &refs,
                    threads,
                );
                let skips: Vec<bool> = grid.cells.iter().map(|m| m.mb_skip_coeff).collect();
                (corrupted, new, skips, above)
            };
            let (corrupted, one, skips, above) = run(1);
            for threads in [2, 3, 8] {
                let what =
                    format!("case {case} ({w}x{h}, {count} partitions) on {threads} threads");
                let (corrupted_too, many, skips_too, above_too) = run(threads);
                assert_eq!(corrupted, corrupted_too, "{what}");
                for p in 0..3 {
                    assert!(
                        one.planes[p].data == many.planes[p].data,
                        "{what}: plane {p} differs"
                    );
                }
                assert!(skips == skips_too, "{what}: skip flags differ");
                assert_eq!(above, above_too, "{what}");
            }
        }
        let (threaded_now, fell_back_now) = threading::TALLY.get();
        assert!(
            threaded_now - threaded > 100,
            "{} frames threaded",
            threaded_now - threaded
        );
        // The version-3 macroblock whose chroma keeps what its buffer held.
        assert!(
            fell_back_now - fell_back > 10,
            "{} frames fell back",
            fell_back_now - fell_back
        );
    }

    #[test]
    fn an_empty_partition_is_an_error() {
        let mut c = Common::new();
        c.multi_token_partition = 1;
        let mut empty = BoolDecoder::new(&[]);
        // Two partitions: the first is given size 0.
        let frame = [0u8, 0, 0, 0, 0, 5, 5];
        assert!(matches!(
            setup_token_decoder(&mut c, &mut empty, &frame, 2),
            Err(FrameError::Error(Error::Corrupt(_)))
        ));
    }
}
