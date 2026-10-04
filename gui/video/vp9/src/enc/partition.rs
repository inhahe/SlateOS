//! Variance-based partitioning: how libvpx's realtime speeds cut each
//! superblock into blocks without trying the cuts.
//!
//! Each 64x64 superblock gets a tree of variances and is cut wherever a
//! block's variance is above a threshold that grows with the quantiser:
//! smooth regions stay in large blocks, detailed ones split. On a key frame
//! the variances are of 4x4 averages against a flat grey, nothing is kept
//! larger than 32x32, and nothing cut below 8x8. On an inter frame they are
//! of 8x8 averages against the superblock's prediction from the last frame
//! -- what moves or changes splits -- and below 16x16 a block goes straight
//! to 8x8. An inter superblock may also skip the tree: kept whole when it
//! barely differs from the last frame, or given the last frame's partition
//! again for a few frames (the copy, kept here between frames).
//!
//! Translated into Rust from libvpx v1.17.0's `vp9/encoder/vp9_encodeframe.c`
//! (`choose_partitioning`, `set_vt_partitioning`, `set_vbp_thresholds`,
//! `fill_variance_4x4avg`, `fill_variance_8x8avg`, `set_low_temp_var_flag`,
//! `copy_partitioning_helper`, `update_prev_partition_helper` and the
//! variance tree) and `vpx_dsp/avg.c` (`vpx_avg_4x4_c`) (copyright the WebM
//! project authors), used under libvpx's BSD licence and patent grant
//! (`licenses/libvpx-LICENSE`, `licenses/libvpx-PATENTS`).

#![allow(
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    reason = "sums of 8-bit samples over at most 64x64, their squares in u32 as libvpx keeps them (wrapping where its unsigned arithmetic wraps), and cell coordinates inside a superblock"
)]
#![allow(
    clippy::indexing_slicing,
    reason = "every index is a quadrant (0..4) composed into the fixed-size node arrays of one superblock's tree"
)]

use crate::common::{
    BLOCK_4X4, BLOCK_8X8, BLOCK_16X16, BLOCK_32X32, BLOCK_64X64, BLOCK_INVALID, BlockSize,
    PARTITION_HORZ, PARTITION_VERT,
};
use crate::frame::Plane;
use crate::tables;

/// libvpx's `Var`: the sums a block's variance is computed from, and the
/// variance once it is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Var {
    sum_square_error: u32,
    sum_error: i32,
    log2_count: u32,
    variance: i32,
}

impl Var {
    /// libvpx's `get_variance`, in its unsigned 32-bit arithmetic.
    fn compute(&mut self) {
        let mean_sq =
            ((i64::from(self.sum_error) * i64::from(self.sum_error)) >> self.log2_count) as u32;
        self.variance = (256u32.wrapping_mul(self.sum_square_error.wrapping_sub(mean_sq))
            >> self.log2_count) as i32;
    }

    /// libvpx's `sum_2_variances`.
    fn sum(a: &Var, b: &Var) -> Var {
        Var {
            sum_square_error: a.sum_square_error.wrapping_add(b.sum_square_error),
            sum_error: a.sum_error.wrapping_add(b.sum_error),
            log2_count: a.log2_count + 1,
            variance: 0,
        }
    }
}

/// A node's variances whole and halved: libvpx's `partition_variance`.
#[derive(Clone, Copy, Debug, Default)]
struct PartVar {
    none: Var,
    horz: [Var; 2],
    vert: [Var; 2],
}

impl PartVar {
    /// From four children, raster order: libvpx's `fill_variance_tree`.
    fn fill(children: [&Var; 4]) -> Self {
        let horz = [
            Var::sum(children[0], children[1]),
            Var::sum(children[2], children[3]),
        ];
        let vert = [
            Var::sum(children[0], children[2]),
            Var::sum(children[1], children[3]),
        ];
        Self {
            none: Var::sum(&vert[0], &vert[1]),
            horz,
            vert,
        }
    }
}

/// One superblock's variance tree: libvpx's `v64x64`, flattened by level.
/// A node's children are the next level's four entries at 4 times its index.
#[derive(Clone, Debug)]
struct Tree {
    v64: PartVar,
    v32: [PartVar; 4],
    v16: [PartVar; 16],
    v8: [PartVar; 64],
    v4: [Var; 256],
}

impl Default for Tree {
    fn default() -> Self {
        Self {
            v64: PartVar::default(),
            v32: [PartVar::default(); 4],
            v16: [PartVar::default(); 16],
            v8: [PartVar::default(); 64],
            v4: [Var::default(); 256],
        }
    }
}

/// libvpx's `vpx_avg_4x4_c`: the rounded mean of a 4x4 block.
fn avg_4x4(p: &Plane<u8>, x: usize, y: usize) -> i32 {
    crate::enc::variance::avg_4x4(p.data.get(y * p.stride + x..).unwrap_or(&[]), p.stride)
}

/// The partitioning thresholds for a quantiser: libvpx's
/// `set_vbp_thresholds` on a key frame -- twenty times the luma AC step,
/// a quarter of that at 32x32 and 16x16, four times it at 8x8.
pub(crate) fn key_frame_thresholds(y_ac_dequant: i32) -> [i64; 4] {
    let base = i64::from(20 * y_ac_dequant);
    [base, base >> 2, base >> 2, base << 2]
}

/// The block sizes the partitioning chose for one superblock, by cell:
/// what libvpx's `set_block_size` writes into the mode info grid, and
/// `nonrd_use_partition` reads back to walk the partition. A cell no choice
/// starts at keeps 4x4, as the grid's cleared entries do.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SbPartition {
    pub mi_row: usize,
    pub mi_col: usize,
    sizes: [[BlockSize; 8]; 8],
}

impl SbPartition {
    /// The size chosen at cell (`mi_row`, `mi_col`), which must be in this
    /// superblock.
    pub(crate) fn size_at(&self, mi_row: usize, mi_col: usize) -> BlockSize {
        mi_row
            .checked_sub(self.mi_row)
            .zip(mi_col.checked_sub(self.mi_col))
            .and_then(|(r, c)| self.sizes.get(r).and_then(|row| row.get(c)))
            .copied()
            .unwrap_or(BLOCK_4X4)
    }
}

/// What the partitioning reads of the frame.
pub(crate) struct Frame<'a> {
    pub luma: &'a Plane<u8>,
    pub mi_rows: usize,
    pub mi_cols: usize,
}

struct Chooser<'a> {
    f: &'a Frame<'a>,
    part: SbPartition,
}

impl Chooser<'_> {
    /// libvpx's `set_block_size`: a block's size at its top-left cell, if
    /// that is in the frame.
    fn set_block_size(&mut self, mi_row: usize, mi_col: usize, bsize: BlockSize) {
        if self.f.mi_cols > mi_col && self.f.mi_rows > mi_row {
            let (r, c) = (mi_row - self.part.mi_row, mi_col - self.part.mi_col);
            if let Some(cell) = self.part.sizes.get_mut(r).and_then(|row| row.get_mut(c)) {
                *cell = bsize;
            }
        }
    }

    /// libvpx's `set_vt_partitioning` on a key frame: keep the block whole,
    /// or in two halves, if its variance allows; false to cut it further.
    fn set_vt_partitioning(
        &mut self,
        node: &mut PartVar,
        bsize: BlockSize,
        mi_row: usize,
        mi_col: usize,
        threshold: i64,
        bsize_min: BlockSize,
        force_split: bool,
    ) -> bool {
        let n = usize::from(tables::NUM_8X8_WIDE[usize::from(bsize)]);
        if force_split {
            return false;
        }
        let fits = mi_col + n / 2 < self.f.mi_cols && mi_row + n / 2 < self.f.mi_rows;
        if bsize == bsize_min {
            node.none.compute();
            if fits && i64::from(node.none.variance) < threshold {
                self.set_block_size(mi_row, mi_col, bsize);
                return true;
            }
            return false;
        }
        if bsize < bsize_min {
            return false;
        }
        node.none.compute();
        // A key frame keeps nothing above 32x32, nor a block far above the
        // threshold.
        if bsize > BLOCK_32X32 || i64::from(node.none.variance) > (threshold << 4) {
            return false;
        }
        if fits && i64::from(node.none.variance) < threshold {
            self.set_block_size(mi_row, mi_col, bsize);
            return true;
        }
        if mi_row + n / 2 < self.f.mi_rows {
            let subsize = tables::SUBSIZE[usize::from(PARTITION_VERT)][usize::from(bsize)];
            node.vert[0].compute();
            node.vert[1].compute();
            if i64::from(node.vert[0].variance) < threshold
                && i64::from(node.vert[1].variance) < threshold
                && chroma_size_valid(subsize)
            {
                self.set_block_size(mi_row, mi_col, subsize);
                self.set_block_size(mi_row, mi_col + n / 2, subsize);
                return true;
            }
        }
        if mi_col + n / 2 < self.f.mi_cols {
            let subsize = tables::SUBSIZE[usize::from(PARTITION_HORZ)][usize::from(bsize)];
            node.horz[0].compute();
            node.horz[1].compute();
            if i64::from(node.horz[0].variance) < threshold
                && i64::from(node.horz[1].variance) < threshold
                && chroma_size_valid(subsize)
            {
                self.set_block_size(mi_row, mi_col, subsize);
                self.set_block_size(mi_row + n / 2, mi_col, subsize);
                return true;
            }
        }
        false
    }
}

/// Whether a block size has a 4:2:0 chroma size: libvpx's
/// `get_plane_block_size(subsize, &xd->plane[1]) < BLOCK_INVALID`.
fn chroma_size_valid(bsize: BlockSize) -> bool {
    tables::SS_SIZE
        .get(usize::from(bsize))
        .is_some_and(|s| s[1][1] < BLOCK_INVALID)
}

/// Partition the key frame's superblock at (`mi_row`, `mi_col`): libvpx's
/// `choose_partitioning` with a key frame's flat reference, `thresholds`
/// from [`key_frame_thresholds`], at the realtime speeds (no minimum-maximum
/// variance check, no 4x4 averages for low resolutions, no 4x4 blocks).
#[allow(
    clippy::too_many_lines,
    reason = "libvpx's choose_partitioning, kept whole so that it ports line for line"
)]
pub(crate) fn choose_key_frame_partitioning(
    f: &Frame<'_>,
    mi_row: usize,
    mi_col: usize,
    thresholds: [i64; 4],
) -> SbPartition {
    let mut c = Chooser {
        f,
        part: SbPartition {
            mi_row,
            mi_col,
            sizes: [[BLOCK_4X4; 8]; 8],
        },
    };
    let mut vt = Tree::default();
    // The superblock's distance to the frame's right and bottom edges, in
    // pixels: libvpx's mb_to_right_edge >> 3.
    let mb_to_right = (f.mi_cols as i64 - 8 - mi_col as i64) * 8;
    let mb_to_bottom = (f.mi_rows as i64 - 8 - mi_row as i64) * 8;
    let pixels_wide = 64 + mb_to_right.min(0);
    let pixels_high = 64 + mb_to_bottom.min(0);
    let (sx, sy) = (mi_col * 8, mi_row * 8);
    let mut force_split = [false; 21];

    // Each 4x4 average against the flat 128 a key frame predicts from.
    for i in 0..4 {
        let x32 = (i & 1) << 5;
        let y32 = (i >> 1) << 5;
        for j in 0..4 {
            let x16 = x32 + ((j & 1) << 4);
            let y16 = y32 + ((j >> 1) << 4);
            for k in 0..4 {
                let x8 = x16 + ((k & 1) << 3);
                let y8 = y16 + ((k >> 1) << 3);
                for l in 0..4 {
                    let x4 = x8 + ((l & 1) << 2);
                    let y4 = y8 + ((l >> 1) << 2);
                    let leaf = &mut vt.v4[((i * 4 + j) * 4 + k) * 4 + l];
                    let (sum, sse) = if (x4 as i64) < pixels_wide && (y4 as i64) < pixels_high {
                        let s_avg = avg_4x4(f.luma, sx + x4, sy + y4);
                        let sum = s_avg - 128;
                        (sum, (sum * sum) as u32)
                    } else {
                        (0, 0)
                    };
                    *leaf = Var {
                        sum_square_error: sse,
                        sum_error: sum,
                        log2_count: 0,
                        variance: 0,
                    };
                }
            }
        }
    }

    for i in 0..4 {
        for j in 0..4 {
            let n16 = i * 4 + j;
            for m in 0..4 {
                let n8 = n16 * 4 + m;
                let ch = n8 * 4;
                vt.v8[n8] =
                    PartVar::fill([&vt.v4[ch], &vt.v4[ch + 1], &vt.v4[ch + 2], &vt.v4[ch + 3]]);
            }
            let ch = n16 * 4;
            vt.v16[n16] = PartVar::fill([
                &vt.v8[ch].none,
                &vt.v8[ch + 1].none,
                &vt.v8[ch + 2].none,
                &vt.v8[ch + 3].none,
            ]);
            vt.v16[n16].none.compute();
            if i64::from(vt.v16[n16].none.variance) > thresholds[2] {
                force_split[5 + n16] = true;
                force_split[i + 1] = true;
                force_split[0] = true;
            }
        }
        let ch = i * 4;
        vt.v32[i] = PartVar::fill([
            &vt.v16[ch].none,
            &vt.v16[ch + 1].none,
            &vt.v16[ch + 2].none,
            &vt.v16[ch + 3].none,
        ]);
        if !force_split[i + 1] {
            vt.v32[i].none.compute();
            if i64::from(vt.v32[i].none.variance) > thresholds[1] {
                force_split[i + 1] = true;
                force_split[0] = true;
            }
        }
    }
    if !force_split[0] {
        vt.v64 = PartVar::fill([
            &vt.v32[0].none,
            &vt.v32[1].none,
            &vt.v32[2].none,
            &vt.v32[3].none,
        ]);
        vt.v64.none.compute();
    }

    let mut v64 = vt.v64;
    if mi_col + 8 > f.mi_cols
        || mi_row + 8 > f.mi_rows
        || !c.set_vt_partitioning(
            &mut v64,
            BLOCK_64X64,
            mi_row,
            mi_col,
            thresholds[0],
            BLOCK_16X16,
            force_split[0],
        )
    {
        for i in 0..4 {
            let x32 = (i & 1) << 2;
            let y32 = (i >> 1) << 2;
            let mut v32 = vt.v32[i];
            if !c.set_vt_partitioning(
                &mut v32,
                BLOCK_32X32,
                mi_row + y32,
                mi_col + x32,
                thresholds[1],
                BLOCK_16X16,
                force_split[i + 1],
            ) {
                for j in 0..4 {
                    let x16 = (j & 1) << 1;
                    let y16 = (j >> 1) << 1;
                    let mut v16 = vt.v16[i * 4 + j];
                    // A key frame's smallest variance-chosen block is 8x8:
                    // libvpx's vbp_bsize_min.
                    if !c.set_vt_partitioning(
                        &mut v16,
                        BLOCK_16X16,
                        mi_row + y32 + y16,
                        mi_col + x32 + x16,
                        thresholds[2],
                        BLOCK_8X8,
                        force_split[5 + i * 4 + j],
                    ) {
                        for k in 0..4 {
                            let x8 = k & 1;
                            let y8 = k >> 1;
                            // The realtime speeds cut no further than 8x8
                            // (nonrd_keyframe).
                            c.set_block_size(
                                mi_row + y32 + y16 + y8,
                                mi_col + x32 + x16 + x8,
                                BLOCK_8X8,
                            );
                        }
                    }
                }
            }
        }
    }
    c.part
}

/// A superblock's temporal content, from its sum of differences against
/// the last source picture: libvpx's `CONTENT_STATE_SB`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ContentState {
    #[default]
    Invalid = 0,
    LowSadLowSumdiff = 1,
    LowSadHighSumdiff = 2,
    HighSadLowSumdiff = 3,
    HighSadHighSumdiff = 4,
    LowVarHighSumdiff = 5,
    VeryHighSad = 6,
}

/// How noisy the source is estimated to be: libvpx's `NOISE_LEVEL`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum NoiseLevel {
    #[default]
    LowLow = 0,
    Low = 1,
    Medium = 2,
    High = 3,
}

/// The partitioning thresholds of an inter frame at 8 bits per sample, by
/// level (64x64, 32x32, 16x16; 8x8 is a key frame's): libvpx's
/// `set_vbp_thresholds` for a frame of `width` x `height` at `speed`.
/// `y_ac_dequant` is the luma AC step of the quantiser, `noise` the
/// estimated noise level if libvpx estimates it here, `key_thresholds_8x8`
/// the 8x8 threshold libvpx leaves from the last key frame (an inter frame
/// does not set it).
#[allow(
    clippy::too_many_arguments,
    reason = "libvpx's set_vbp_thresholds reads all of these"
)]
pub(crate) fn inter_thresholds(
    y_ac_dequant: i32,
    thresh_mult: i32,
    noise: Option<NoiseLevel>,
    content_state: ContentState,
    width: u32,
    height: u32,
    speed: i32,
    disable_16x16: bool,
    avg_inter_qindex: i32,
    key_thresholds_8x8: i64,
) -> [i64; 4] {
    let mut base = i64::from(thresh_mult) * i64::from(y_ac_dequant);
    if let Some(level) = noise
        && width >= 640
        && height >= 480
    {
        match level {
            NoiseLevel::High => base *= 3,
            NoiseLevel::Medium => base <<= 1,
            NoiseLevel::LowLow => base = (7 * base) >> 3,
            NoiseLevel::Low => {}
        }
    }
    // scale_part_thresh_sumdiff.
    let low_sumdiff = matches!(
        content_state,
        ContentState::LowSadLowSumdiff
            | ContentState::HighSadLowSumdiff
            | ContentState::LowVarHighSumdiff
    );
    if (speed >= 8 && ((width <= 640 && height <= 480) || low_sumdiff))
        || (speed == 7 && low_sumdiff)
    {
        base = (5 * base) >> 2;
    }
    let mut t = [base, 0, base << speed.clamp(0, 31), key_thresholds_8x8];
    if width >= 1280 && height >= 720 && speed < 7 {
        t[2] <<= 1;
    }
    if width <= 352 && height <= 288 {
        t[0] = base >> 3;
        t[1] = base >> 1;
        t[2] = base << 3;
        if avg_inter_qindex > 220 {
            t[2] <<= 2;
        } else if avg_inter_qindex > 200 {
            t[2] <<= 1;
        }
    } else if width < 1280 && height < 720 {
        t[1] = (5 * base) >> 2;
    } else if width < 1920 && height < 1080 {
        t[1] = base << 1;
    } else {
        t[1] = (5 * base) >> 1;
    }
    if disable_16x16 {
        t[2] = i64::MAX;
    }
    t
}

/// libvpx's `vpx_avg_8x8_c` at (`x`, `y`) of a plane.
fn avg_8x8(p: &Plane<u8>, x: usize, y: usize) -> i32 {
    crate::enc::variance::avg_8x8(p.data.get(y * p.stride + x..).unwrap_or(&[]), p.stride)
}

/// What an inter frame's partitioning of one superblock reads.
pub(crate) struct InterSb<'a> {
    /// The source's luma, and the superblock's prediction from the last
    /// frame (libvpx's `xd->plane[0].dst`, where it was built).
    pub src: &'a Plane<u8>,
    pub pred: &'a Plane<u8>,
    pub mi_rows: usize,
    pub mi_cols: usize,
    pub mi_row: usize,
    pub mi_col: usize,
    /// The frame's thresholds, by level ([`inter_thresholds`]).
    pub thresholds: [i64; 4],
    /// Split the superblock whatever its variance: a scene change, or a
    /// superblock that is mostly skin.
    pub force_split_64: bool,
    /// libvpx's estimated noise level (`vp9_noise_estimate_extract_level`);
    /// `Low` where it does not estimate, as `choose_partitioning` starts it.
    pub noise_level: NoiseLevel,
}

/// One inter superblock's partitioning: the sizes, every `set_block_size`
/// in the order libvpx made it (whose last one leaves libvpx's `xd->mi`
/// there), and the variance tree the low-variance flags read.
pub(crate) struct InterPartition {
    pub part: SbPartition,
    pub set_calls: Vec<(usize, usize, BlockSize)>,
    tree: Tree,
}

impl InterPartition {
    /// The 64x64, halves and quadrants' variances libvpx's
    /// `set_low_temp_var_flag` reads, by its indices: (64x64), (64x32 top,
    /// bottom), (32x64 left, right), (32x32 by quadrant), (16x16 by
    /// quadrant, then raster within it).
    fn var64(&self) -> i32 {
        self.tree.v64.none.variance
    }
}

/// Partition an inter frame's superblock: libvpx's `choose_partitioning`
/// from its variance tree on, at the realtime speeds of 8 and up (no
/// minimum-maximum check, no 4x4 averages above 352x288): 8x8 averages of
/// the source against the prediction from the last frame, cut where a
/// block's variance is above its level's threshold, never below 16x16 by
/// variance and then straight to 8x8.
#[allow(
    clippy::too_many_lines,
    reason = "libvpx's choose_partitioning, kept whole so that it ports line for line"
)]
pub(crate) fn choose_inter_partitioning(s: &InterSb<'_>) -> InterPartition {
    let f = Frame {
        luma: s.src,
        mi_rows: s.mi_rows,
        mi_cols: s.mi_cols,
    };
    let mut c = Chooser {
        f: &f,
        part: SbPartition {
            mi_row: s.mi_row,
            mi_col: s.mi_col,
            sizes: [[BLOCK_4X4; 8]; 8],
        },
    };
    let (mi_row, mi_col) = (s.mi_row, s.mi_col);
    let thresholds = s.thresholds;
    let mut vt = Tree::default();
    let mb_to_right = (s.mi_cols as i64 - 8 - mi_col as i64) * 8;
    let mb_to_bottom = (s.mi_rows as i64 - 8 - mi_row as i64) * 8;
    let pixels_wide = 64 + mb_to_right.min(0);
    let pixels_high = 64 + mb_to_bottom.min(0);
    let (sx, sy) = (mi_col * 8, mi_row * 8);
    let mut force_split = [false; 21];
    force_split[0] = s.force_split_64;
    let mut avg_16x16 = [0i32; 4];
    let mut maxvar_16x16 = [0i32; 4];
    let mut minvar_16x16 = [i32::MAX; 4];

    for i in 0..4 {
        let x32 = (i & 1) << 5;
        let y32 = (i >> 1) << 5;
        for j in 0..4 {
            let x16 = x32 + ((j & 1) << 4);
            let y16 = y32 + ((j >> 1) << 4);
            let n16 = i * 4 + j;
            // fill_variance_8x8avg: each 8x8's average, source less
            // prediction.
            for k in 0..4 {
                let x8 = x16 + ((k & 1) << 3);
                let y8 = y16 + ((k >> 1) << 3);
                let (sum, sse) = if (x8 as i64) < pixels_wide && (y8 as i64) < pixels_high {
                    let s_avg = avg_8x8(s.src, sx + x8, sy + y8);
                    let d_avg = avg_8x8(s.pred, sx + x8, sy + y8);
                    let sum = s_avg - d_avg;
                    (sum, (sum * sum) as u32)
                } else {
                    (0, 0)
                };
                vt.v8[n16 * 4 + k].none = Var {
                    sum_square_error: sse,
                    sum_error: sum,
                    log2_count: 0,
                    variance: 0,
                };
            }
            let ch = n16 * 4;
            vt.v16[n16] = PartVar::fill([
                &vt.v8[ch].none,
                &vt.v8[ch + 1].none,
                &vt.v8[ch + 2].none,
                &vt.v8[ch + 3].none,
            ]);
            vt.v16[n16].none.compute();
            let v = vt.v16[n16].none.variance;
            avg_16x16[i] += v;
            minvar_16x16[i] = minvar_16x16[i].min(v);
            maxvar_16x16[i] = maxvar_16x16[i].max(v);
            if i64::from(v) > thresholds[2] {
                force_split[5 + n16] = true;
                force_split[i + 1] = true;
                force_split[0] = true;
            }
        }
    }
    let mut avg_32x32 = 0i32;
    let mut max_var_32x32 = 0i32;
    let mut min_var_32x32 = i32::MAX;
    for i in 0..4 {
        let ch = i * 4;
        vt.v32[i] = PartVar::fill([
            &vt.v16[ch].none,
            &vt.v16[ch + 1].none,
            &vt.v16[ch + 2].none,
            &vt.v16[ch + 3].none,
        ]);
        if !force_split[i + 1] {
            vt.v32[i].none.compute();
            let var_32x32 = vt.v32[i].none.variance;
            max_var_32x32 = max_var_32x32.max(var_32x32);
            min_var_32x32 = min_var_32x32.min(var_32x32);
            let v = i64::from(var_32x32);
            let busy =
                v > thresholds[1] || (v > (thresholds[1] >> 1) && v > i64::from(avg_16x16[i] >> 1));
            // A clean low-resolution source whose 16x16s vary a lot
            // (libvpx's cm->height <= 360: the same test on whole cells).
            let uneven = s.noise_level < NoiseLevel::Low
                && s.mi_rows * 8 <= 360
                && i64::from(maxvar_16x16[i] - minvar_16x16[i]) > (thresholds[1] >> 1)
                && i64::from(maxvar_16x16[i]) > thresholds[1];
            if busy || uneven {
                force_split[i + 1] = true;
                force_split[0] = true;
            }
            avg_32x32 += var_32x32;
        }
    }
    if !force_split[0] {
        vt.v64 = PartVar::fill([
            &vt.v32[0].none,
            &vt.v32[1].none,
            &vt.v32[2].none,
            &vt.v32[3].none,
        ]);
        vt.v64.none.compute();
        let v = i64::from(vt.v64.none.variance);
        if s.noise_level >= NoiseLevel::Medium {
            if v > i64::from((9 * avg_32x32) >> 5) {
                force_split[0] = true;
            }
        } else if i64::from(max_var_32x32 - min_var_32x32) > 3 * (thresholds[0] >> 3)
            && i64::from(max_var_32x32) > thresholds[0] >> 1
        {
            force_split[0] = true;
        }
    }

    let mut calls = Vec::new();
    let mut v64 = vt.v64;
    if mi_col + 8 > s.mi_cols
        || mi_row + 8 > s.mi_rows
        || !c.set_vt_partitioning_inter(
            &mut v64,
            BLOCK_64X64,
            mi_row,
            mi_col,
            thresholds[0],
            BLOCK_16X16,
            force_split[0],
            &mut calls,
        )
    {
        for i in 0..4 {
            let x32 = (i & 1) << 2;
            let y32 = (i >> 1) << 2;
            let mut v32 = vt.v32[i];
            let kept = c.set_vt_partitioning_inter(
                &mut v32,
                BLOCK_32X32,
                mi_row + y32,
                mi_col + x32,
                thresholds[1],
                BLOCK_16X16,
                force_split[i + 1],
                &mut calls,
            );
            vt.v32[i] = v32;
            if !kept {
                for j in 0..4 {
                    let x16 = (j & 1) << 1;
                    let y16 = (j >> 1) << 1;
                    let mut v16 = vt.v16[i * 4 + j];
                    if !c.set_vt_partitioning_inter(
                        &mut v16,
                        BLOCK_16X16,
                        mi_row + y32 + y16,
                        mi_col + x32 + x16,
                        thresholds[2],
                        BLOCK_16X16,
                        force_split[5 + i * 4 + j],
                        &mut calls,
                    ) {
                        for k in 0..4 {
                            let (r, cc) =
                                (mi_row + y32 + y16 + (k >> 1), mi_col + x32 + x16 + (k & 1));
                            c.set_block_size_logged(r, cc, BLOCK_8X8, &mut calls);
                        }
                    }
                }
            }
        }
    }
    vt.v64 = v64;
    InterPartition {
        part: c.part,
        set_calls: calls,
        tree: vt,
    }
}

impl Chooser<'_> {
    /// [`Self::set_block_size`], noting the call.
    fn set_block_size_logged(
        &mut self,
        mi_row: usize,
        mi_col: usize,
        bsize: BlockSize,
        calls: &mut Vec<(usize, usize, BlockSize)>,
    ) {
        calls.push((mi_row, mi_col, bsize));
        self.set_block_size(mi_row, mi_col, bsize);
    }

    /// libvpx's `set_vt_partitioning` on an inter frame: the variances were
    /// computed where the forced splits were decided, the vertical and
    /// horizontal halves are computed here.
    #[allow(clippy::too_many_arguments)]
    fn set_vt_partitioning_inter(
        &mut self,
        node: &mut PartVar,
        bsize: BlockSize,
        mi_row: usize,
        mi_col: usize,
        threshold: i64,
        bsize_min: BlockSize,
        force_split: bool,
        calls: &mut Vec<(usize, usize, BlockSize)>,
    ) -> bool {
        let n = usize::from(tables::NUM_8X8_WIDE[usize::from(bsize)]);
        if force_split {
            return false;
        }
        let fits = mi_col + n / 2 < self.f.mi_cols && mi_row + n / 2 < self.f.mi_rows;
        if bsize == bsize_min {
            if fits && i64::from(node.none.variance) < threshold {
                self.set_block_size_logged(mi_row, mi_col, bsize, calls);
                return true;
            }
            return false;
        }
        if bsize < bsize_min {
            return false;
        }
        if fits && i64::from(node.none.variance) < threshold {
            self.set_block_size_logged(mi_row, mi_col, bsize, calls);
            return true;
        }
        if mi_row + n / 2 < self.f.mi_rows {
            let subsize = tables::SUBSIZE[usize::from(PARTITION_VERT)][usize::from(bsize)];
            node.vert[0].compute();
            node.vert[1].compute();
            if i64::from(node.vert[0].variance) < threshold
                && i64::from(node.vert[1].variance) < threshold
                && chroma_size_valid(subsize)
            {
                self.set_block_size_logged(mi_row, mi_col, subsize, calls);
                self.set_block_size_logged(mi_row, mi_col + n / 2, subsize, calls);
                return true;
            }
        }
        if mi_col + n / 2 < self.f.mi_cols {
            let subsize = tables::SUBSIZE[usize::from(PARTITION_HORZ)][usize::from(bsize)];
            node.horz[0].compute();
            node.horz[1].compute();
            if i64::from(node.horz[0].variance) < threshold
                && i64::from(node.horz[1].variance) < threshold
                && chroma_size_valid(subsize)
            {
                self.set_block_size_logged(mi_row, mi_col, subsize, calls);
                self.set_block_size_logged(mi_row + n / 2, mi_col, subsize, calls);
                return true;
            }
        }
        false
    }
}

/// The 64x64 early exit's and the copy's partitions are set the same way:
/// one 64x64 block (libvpx's `set_block_size` of the whole superblock).
pub(crate) fn whole_superblock(mi_row: usize, mi_col: usize) -> SbPartition {
    let mut part = SbPartition {
        mi_row,
        mi_col,
        sizes: [[BLOCK_4X4; 8]; 8],
    };
    part.sizes[0][0] = BLOCK_64X64;
    part
}

/// The partition libvpx copies from the last frame's: its
/// `copy_partitioning_helper` over `prev_partition` (the sizes
/// `update_prev_partition` stored), with the `set_block_size` calls in
/// order. `prev(mi_row, mi_col)` reads the stored size of a cell.
pub(crate) fn copy_partition(
    mi_row: usize,
    mi_col: usize,
    mi_rows: usize,
    mi_cols: usize,
    prev: &dyn Fn(usize, usize) -> BlockSize,
) -> (SbPartition, Vec<(usize, usize, BlockSize)>) {
    let f = Frame {
        luma: &EMPTY_PLANE,
        mi_rows,
        mi_cols,
    };
    let mut c = Chooser {
        f: &f,
        part: SbPartition {
            mi_row,
            mi_col,
            sizes: [[BLOCK_4X4; 8]; 8],
        },
    };
    let mut calls = Vec::new();
    copy_helper(&mut c, &mut calls, prev, BLOCK_64X64, mi_row, mi_col);
    (c.part, calls)
}

/// An empty plane for a [`Frame`] that reads no samples.
static EMPTY_PLANE: Plane<u8> = Plane {
    data: Vec::new(),
    stride: 0,
    alloc_height: 0,
    width: 0,
    height: 0,
    crop_width: 0,
    crop_height: 0,
};

fn copy_helper(
    c: &mut Chooser<'_>,
    calls: &mut Vec<(usize, usize, BlockSize)>,
    prev: &dyn Fn(usize, usize) -> BlockSize,
    bsize: BlockSize,
    mi_row: usize,
    mi_col: usize,
) {
    if mi_row >= c.f.mi_rows || mi_col >= c.f.mi_cols {
        return;
    }
    let bsl = usize::from(tables::B_WIDTH_LOG2[usize::from(bsize)]);
    let bs = (1usize << bsl) >> 2;
    let partition = tables::PARTITION_LOOKUP[bsl][usize::from(prev(mi_row, mi_col))];
    let subsize = tables::SUBSIZE[usize::from(partition)][usize::from(bsize)];
    if subsize < BLOCK_8X8 {
        c.set_block_size_logged(mi_row, mi_col, bsize, calls);
        return;
    }
    match partition {
        crate::common::PARTITION_NONE => c.set_block_size_logged(mi_row, mi_col, bsize, calls),
        PARTITION_HORZ => {
            c.set_block_size_logged(mi_row, mi_col, subsize, calls);
            c.set_block_size_logged(mi_row + bs, mi_col, subsize, calls);
        }
        PARTITION_VERT => {
            c.set_block_size_logged(mi_row, mi_col, subsize, calls);
            c.set_block_size_logged(mi_row, mi_col + bs, subsize, calls);
        }
        _ => {
            copy_helper(c, calls, prev, subsize, mi_row, mi_col);
            copy_helper(c, calls, prev, subsize, mi_row + bs, mi_col);
            copy_helper(c, calls, prev, subsize, mi_row, mi_col + bs);
            copy_helper(c, calls, prev, subsize, mi_row + bs, mi_col + bs);
        }
    }
}

/// The sizes to store for the next frame's copy: libvpx's
/// `update_prev_partition_helper`, walking this frame's partition (`size`
/// reads the mode info grid's block size at a cell) and writing `store`.
pub(crate) fn store_partition(
    mi_row: usize,
    mi_col: usize,
    mi_rows: usize,
    mi_cols: usize,
    bsize: BlockSize,
    size: &dyn Fn(usize, usize) -> BlockSize,
    store: &mut dyn FnMut(usize, usize, BlockSize),
) {
    if mi_row >= mi_rows || mi_col >= mi_cols {
        return;
    }
    let bsl = usize::from(tables::B_WIDTH_LOG2[usize::from(bsize)]);
    let bs = (1usize << bsl) >> 2;
    let partition = tables::PARTITION_LOOKUP[bsl][usize::from(size(mi_row, mi_col))];
    let subsize = tables::SUBSIZE[usize::from(partition)][usize::from(bsize)];
    if subsize < BLOCK_8X8 {
        store(mi_row, mi_col, bsize);
        return;
    }
    match partition {
        crate::common::PARTITION_NONE => store(mi_row, mi_col, bsize),
        PARTITION_HORZ => {
            store(mi_row, mi_col, subsize);
            if mi_row + bs < mi_rows {
                store(mi_row + bs, mi_col, subsize);
            }
        }
        PARTITION_VERT => {
            store(mi_row, mi_col, subsize);
            if mi_col + bs < mi_cols {
                store(mi_row, mi_col + bs, subsize);
            }
        }
        _ => {
            store_partition(mi_row, mi_col, mi_rows, mi_cols, subsize, size, store);
            store_partition(mi_row + bs, mi_col, mi_rows, mi_cols, subsize, size, store);
            store_partition(mi_row, mi_col + bs, mi_rows, mi_cols, subsize, size, store);
            store_partition(
                mi_row + bs,
                mi_col + bs,
                mi_rows,
                mi_cols,
                subsize,
                size,
                store,
            );
        }
    }
}

/// Which blocks of the superblock change too little since the last frame
/// to be worth searching hard: libvpx's `set_low_temp_var_flag` for the
/// last frame's partitioning reference. `last` is libvpx's `xd->mi[0]` as
/// the partitioning left it -- the block size and vector of the cell its
/// last `set_block_size` wrote -- and `size_at` this frame's sizes.
/// `short_circuit` is the speed feature's level, `wide` whether the frame is
/// over 640 wide.
pub(crate) fn low_temp_var_flags(
    p: &InterPartition,
    thresholds: [i64; 4],
    last: (BlockSize, crate::common::Mv),
    short_circuit: i32,
    wide: bool,
    mi_rows: usize,
    mi_cols: usize,
) -> [bool; 25] {
    let mut low = [false; 25];
    let mv_thr = if wide { 8 } else { 4 };
    let (sb_type, mv) = last;
    let (r, c) = (i32::from(mv.row), i32::from(mv.col));
    if !(short_circuit == 1 || (c < mv_thr && c > -mv_thr && r < mv_thr && r > -mv_thr)) {
        return low;
    }
    let tree = &p.tree;
    if sb_type == BLOCK_64X64 {
        if i64::from(p.var64()) < (thresholds[0] >> 1) {
            low[0] = true;
        }
    } else if sb_type == crate::common::BLOCK_64X32 {
        for i in 0..2 {
            if i64::from(tree.v64.horz[i].variance) < (thresholds[0] >> 2) {
                low[i + 1] = true;
            }
        }
    } else if sb_type == crate::common::BLOCK_32X64 {
        for i in 0..2 {
            if i64::from(tree.v64.vert[i].variance) < (thresholds[0] >> 2) {
                low[i + 3] = true;
            }
        }
    } else {
        let idx = [(0, 0), (0, 4), (4, 0), (4, 4)];
        for (i, &(dr, dc)) in idx.iter().enumerate() {
            let (row, col) = (p.part.mi_row + dr, p.part.mi_col + dc);
            if mi_cols <= col || mi_rows <= row {
                continue;
            }
            let this = p.part.size_at(row, col);
            if this == BLOCK_32X32 {
                let threshold_32x32 = if short_circuit == 1 || short_circuit == 3 {
                    (5 * thresholds[1]) >> 3
                } else {
                    thresholds[1] >> 1
                };
                if i64::from(tree.v32[i].none.variance) < threshold_32x32 {
                    low[i + 5] = true;
                }
            } else if short_circuit >= 2
                && (this == BLOCK_16X16
                    || this == crate::common::BLOCK_32X16
                    || this == crate::common::BLOCK_16X32)
            {
                for j in 0..4 {
                    if i64::from(tree.v16[i * 4 + j].none.variance) < (thresholds[2] >> 8) {
                        low[(i << 2) + j + 9] = true;
                    }
                }
            }
        }
    }
    low
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "a test: a failure should be loud")]

    use super::*;

    fn plane(w: usize, h: usize, f: impl Fn(usize, usize) -> u8) -> Plane<u8> {
        let (aw, ah) = (w.div_ceil(64) * 64, h.div_ceil(64) * 64);
        let mut data = vec![0u8; aw * ah];
        for y in 0..ah {
            for x in 0..aw {
                data[y * aw + x] = f(x.min(w - 1), y.min(h - 1));
            }
        }
        Plane {
            data,
            stride: aw,
            alloc_height: ah,
            width: w,
            height: h,
            crop_width: w,
            crop_height: h,
        }
    }

    #[test]
    fn a_flat_superblock_is_kept_in_32x32_blocks() {
        let p = plane(64, 64, |_, _| 128);
        let f = Frame {
            luma: &p,
            mi_rows: 8,
            mi_cols: 8,
        };
        let part = choose_key_frame_partitioning(&f, 0, 0, key_frame_thresholds(300));
        for (r, c) in [(0, 0), (0, 4), (4, 0), (4, 4)] {
            assert_eq!(part.size_at(r, c), BLOCK_32X32);
        }
    }

    #[test]
    fn a_busy_superblock_is_cut_to_8x8() {
        let p = plane(
            64,
            64,
            |x, y| if (x / 4 + y / 4) % 2 == 0 { 0 } else { 255 },
        );
        let f = Frame {
            luma: &p,
            mi_rows: 8,
            mi_cols: 8,
        };
        let part = choose_key_frame_partitioning(&f, 0, 0, key_frame_thresholds(4));
        for r in 0..8 {
            for c in 0..8 {
                assert_eq!(part.size_at(r, c), BLOCK_8X8, "cell {r},{c}");
            }
        }
    }

    #[test]
    fn variance_is_libvpxs_unsigned_arithmetic() {
        // Four 4x4 averages of 10 above 128: sum 40, squares 400, so the
        // variance is 0.
        let mut v = Var {
            sum_square_error: 400,
            sum_error: 40,
            log2_count: 2,
            variance: 0,
        };
        v.compute();
        assert_eq!(v.variance, 0);
        // 0 and 20 twice each: sum 40, squares 800, (800 - 400) * 256 / 4.
        let mut v = Var {
            sum_square_error: 800,
            sum_error: 40,
            log2_count: 2,
            variance: 0,
        };
        v.compute();
        assert_eq!(v.variance, 25_600);
    }
}
