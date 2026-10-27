//! Intra prediction (`h264pred.c`, `h264pred_template.c`), and the checks
//! that turn a coded mode into one the neighbours allow
//! (`ff_h264_check_intra4x4_pred_mode` and `ff_h264_check_intra_pred_mode`
//! in `h264_parse.c`).
//!
//! An intra block is predicted from the samples above and to its left --
//! straight down, across, their average, along one of six diagonals, or a
//! plane fitted to them -- and its residual added. Where a neighbour is not
//! available (the picture's edge, another slice, an inter neighbour under
//! constrained intra prediction) the checks swap the mode for one that does
//! without it: DC of the side that is there, or 128.

#![allow(
    clippy::indexing_slicing,
    reason = "cache indices are scan8's for the macroblock's own blocks; table indices are \
              modes the bitstream reader has bounded; sample indices are a block's and the \
              neighbours the mode checks leave it, inside its plane"
)]
#![allow(
    clippy::arithmetic_side_effects,
    reason = "sums of a few samples of at most 14 bits, and the plane's gradients over them"
)]
#![allow(
    clippy::cast_possible_wrap,
    reason = "offsets within a block, under 16"
)]

use crate::decoder::DecodeError;
use crate::mbtype::SCAN8;
use crate::picture::Pixel;

/// 4x4 and 8x8 luma modes (`VERT_PRED` to `HOR_UP_PRED`), and the DC
/// variants that stand in where a side is missing.
pub(crate) const VERT_PRED: i8 = 0;
pub(crate) const HOR_PRED: i8 = 1;
pub(crate) const DC_PRED: i8 = 2;
pub(crate) const DIAG_DOWN_LEFT_PRED: i8 = 3;
pub(crate) const DIAG_DOWN_RIGHT_PRED: i8 = 4;
pub(crate) const VERT_RIGHT_PRED: i8 = 5;
pub(crate) const HOR_DOWN_PRED: i8 = 6;
pub(crate) const VERT_LEFT_PRED: i8 = 7;
pub(crate) const HOR_UP_PRED: i8 = 8;
pub(crate) const LEFT_DC_PRED: i8 = 9;
pub(crate) const TOP_DC_PRED: i8 = 10;
pub(crate) const DC_128_PRED: i8 = 11;

/// 16x16 luma and chroma modes (`DC_PRED8x8` to `PLANE_PRED8x8`), their DC
/// variants, and the four "Alzheimer" DCs for a chroma block whose left
/// pair is half available -- an MBAFF frame with constrained intra
/// prediction.
pub(crate) const DC_PRED8X8: i32 = 0;
pub(crate) const HOR_PRED8X8: i32 = 1;
pub(crate) const VERT_PRED8X8: i32 = 2;
pub(crate) const PLANE_PRED8X8: i32 = 3;
pub(crate) const LEFT_DC_PRED8X8: i32 = 4;
pub(crate) const TOP_DC_PRED8X8: i32 = 5;
pub(crate) const DC_128_PRED8X8: i32 = 6;
pub(crate) const ALZHEIMER_DC_L0T_PRED8X8: i32 = 7;
pub(crate) const ALZHEIMER_DC_0LT_PRED8X8: i32 = 8;
pub(crate) const ALZHEIMER_DC_L00_PRED8X8: i32 = 9;
pub(crate) const ALZHEIMER_DC_0L0_PRED8X8: i32 = 10;

/// The macroblock's 4x4 (or 8x8) modes made ones its available neighbours
/// allow: along an unavailable top edge vertical and diagonal-down-left are
/// refused and DC becomes left DC; along an unavailable left edge the
/// horizontal modes are refused and DC becomes top DC, or 128 where that is
/// gone too (`ff_h264_check_intra4x4_pred_mode`).
pub(crate) fn check_intra4x4_pred_mode(
    cache: &mut [i8; 40],
    top_samples_available: u32,
    left_samples_available: u32,
) -> Result<(), DecodeError> {
    // What each mode becomes without the top row, or the left column: -1
    // refused, 0 unchanged.
    const TOP: [i8; 12] = [-1, 0, LEFT_DC_PRED, -1, -1, -1, -1, -1, 0, 0, 0, 0];
    const LEFT: [i8; 12] = [0, -1, TOP_DC_PRED, 0, -1, -1, -1, 0, -1, DC_128_PRED, 0, 0];
    let swap = |table: &[i8; 12], mode: &mut i8| -> Result<(), DecodeError> {
        let status = usize::try_from(*mode)
            .ok()
            .and_then(|m| table.get(m))
            .copied()
            .ok_or(DecodeError)?;
        if status < 0 {
            return Err(DecodeError);
        }
        if status != 0 {
            *mode = status;
        }
        Ok(())
    };

    if top_samples_available & 0x8000 == 0 {
        for i in 0..4 {
            swap(&TOP, &mut cache[SCAN8[0] + i])?;
        }
    }
    if left_samples_available & 0x8888 != 0x8888 {
        const MASK: [u32; 4] = [0x8000, 0x2000, 0x80, 0x20];
        for (i, mask) in MASK.into_iter().enumerate() {
            if left_samples_available & mask == 0 {
                swap(&LEFT, &mut cache[SCAN8[0] + 8 * i])?;
            }
        }
    }
    Ok(())
}

/// A 16x16 luma or a chroma mode made one the available neighbours allow
/// (`ff_h264_check_intra_pred_mode`): vertical refused without the top
/// row, horizontal without the left column, plane without either, DC made
/// the DC of what is there. A chroma block with only half its left column
/// (an MBAFF pair beside one of each kind under constrained intra
/// prediction) takes one of the Alzheimer DCs.
pub(crate) fn check_intra_pred_mode(
    top_samples_available: u32,
    left_samples_available: u32,
    mode: u32,
    is_chroma: bool,
) -> Result<i32, DecodeError> {
    const TOP: [i32; 4] = [LEFT_DC_PRED8X8, 1, -1, -1];
    const LEFT: [i32; 5] = [TOP_DC_PRED8X8, -1, 2, -1, DC_128_PRED8X8];

    if mode > 3 {
        return Err(DecodeError);
    }
    #[allow(clippy::cast_possible_wrap, reason = "under 4")]
    let mut mode = mode as i32;
    let index = |m: i32| usize::try_from(m).map_err(|_| DecodeError);
    if top_samples_available & 0x8000 == 0 {
        mode = TOP[index(mode)?];
        if mode < 0 {
            return Err(DecodeError);
        }
    }
    if left_samples_available & 0x8080 != 0x8080 {
        mode = LEFT[index(mode)?];
        if mode < 0 {
            return Err(DecodeError);
        }
        if is_chroma && left_samples_available & 0x8080 != 0 {
            mode = ALZHEIMER_DC_L0T_PRED8X8
                + i32::from(left_samples_available & 0x8000 == 0)
                + 2 * i32::from(mode == DC_128_PRED8X8);
        }
    }
    Ok(mode)
}

/// A block being predicted: its plane, where it starts and the plane's row
/// stride, in samples. Neighbours lie at negative offsets from the start;
/// the mode checks keep every read to neighbours that are there.
struct Block<'a, P> {
    s: &'a mut [P],
    at: usize,
    stride: usize,
}

impl<P: Pixel> Block<'_, P> {
    #[allow(
        clippy::cast_possible_wrap,
        clippy::cast_sign_loss,
        reason = "a neighbour's offset within the plane, which the mode checks keep inside it"
    )]
    fn get(&self, x: isize, y: isize) -> i32 {
        let at = self.at as isize + x + y * self.stride as isize;
        self.s[at as usize].int()
    }

    fn put(&mut self, x: usize, y: usize, v: i32) {
        let at = self.at + x + y * self.stride;
        self.s[at] = P::from_int(v);
    }

    /// Each of `points` (column, row) set to `v`.
    fn set(&mut self, points: &[(usize, usize)], v: i32) {
        for &(x, y) in points {
            self.put(x, y, v);
        }
    }

    /// A `w` by `h` rectangle from (`x`, `y`) set to `v`.
    fn fill(&mut self, x: usize, y: usize, w: usize, h: usize, v: i32) {
        for row in y..y + h {
            for col in x..x + w {
                self.put(col, row, v);
            }
        }
    }

    /// The `n` samples above, from column `x`.
    fn top<const N: usize>(&self, x: isize) -> [i32; N] {
        core::array::from_fn(|i| self.get(x + i as isize, -1))
    }

    /// The `n` samples to the left, from row `y`.
    fn left<const N: usize>(&self, y: isize) -> [i32; N] {
        core::array::from_fn(|i| self.get(-1, y + i as isize))
    }
}

/// A sample at `bits` clipped to its range (`av_clip_pixel`).
fn clip(v: i32, bits: u32) -> i32 {
    v.clamp(0, (1 << bits) - 1)
}

/// A 4x4 luma block predicted by `mode`, `topright` the four samples above
/// and to the right -- or their stand-ins where those are not available
/// (`pred4x4`, its H.264 entries).
#[allow(
    clippy::too_many_lines,
    reason = "one arm a predictor, as h264pred_template.c has them"
)]
pub(crate) fn pred4x4<P: Pixel>(
    mode: i8,
    s: &mut [P],
    at: usize,
    stride: usize,
    topright: [i32; 4],
    bits: u32,
) {
    let mut b = Block { s, at, stride };
    match mode {
        VERT_PRED => {
            let t: [i32; 4] = b.top(0);
            for y in 0..4 {
                for (x, &v) in t.iter().enumerate() {
                    b.put(x, y, v);
                }
            }
        }
        HOR_PRED => {
            let l: [i32; 4] = b.left(0);
            for (y, &v) in l.iter().enumerate() {
                b.fill(0, y, 4, 1, v);
            }
        }
        DC_PRED => {
            let t: [i32; 4] = b.top(0);
            let l: [i32; 4] = b.left(0);
            let dc = (t.iter().sum::<i32>() + l.iter().sum::<i32>() + 4) >> 3;
            b.fill(0, 0, 4, 4, dc);
        }
        LEFT_DC_PRED => {
            let l: [i32; 4] = b.left(0);
            b.fill(0, 0, 4, 4, (l.iter().sum::<i32>() + 2) >> 2);
        }
        TOP_DC_PRED => {
            let t: [i32; 4] = b.top(0);
            b.fill(0, 0, 4, 4, (t.iter().sum::<i32>() + 2) >> 2);
        }
        DC_128_PRED => b.fill(0, 0, 4, 4, 1 << (bits - 1)),
        DIAG_DOWN_RIGHT_PRED => {
            let lt = b.get(-1, -1);
            let [t0, t1, t2, t3]: [i32; 4] = b.top(0);
            let [l0, l1, l2, l3]: [i32; 4] = b.left(0);
            b.set(&[(0, 3)], (l3 + 2 * l2 + l1 + 2) >> 2);
            b.set(&[(0, 2), (1, 3)], (l2 + 2 * l1 + l0 + 2) >> 2);
            b.set(&[(0, 1), (1, 2), (2, 3)], (l1 + 2 * l0 + lt + 2) >> 2);
            b.set(
                &[(0, 0), (1, 1), (2, 2), (3, 3)],
                (l0 + 2 * lt + t0 + 2) >> 2,
            );
            b.set(&[(1, 0), (2, 1), (3, 2)], (lt + 2 * t0 + t1 + 2) >> 2);
            b.set(&[(2, 0), (3, 1)], (t0 + 2 * t1 + t2 + 2) >> 2);
            b.set(&[(3, 0)], (t1 + 2 * t2 + t3 + 2) >> 2);
        }
        DIAG_DOWN_LEFT_PRED => {
            let [t0, t1, t2, t3]: [i32; 4] = b.top(0);
            let [t4, t5, t6, t7] = topright;
            b.set(&[(0, 0)], (t0 + t2 + 2 * t1 + 2) >> 2);
            b.set(&[(1, 0), (0, 1)], (t1 + t3 + 2 * t2 + 2) >> 2);
            b.set(&[(2, 0), (1, 1), (0, 2)], (t2 + t4 + 2 * t3 + 2) >> 2);
            b.set(
                &[(3, 0), (2, 1), (1, 2), (0, 3)],
                (t3 + t5 + 2 * t4 + 2) >> 2,
            );
            b.set(&[(3, 1), (2, 2), (1, 3)], (t4 + t6 + 2 * t5 + 2) >> 2);
            b.set(&[(3, 2), (2, 3)], (t5 + t7 + 2 * t6 + 2) >> 2);
            b.set(&[(3, 3)], (t6 + 3 * t7 + 2) >> 2);
        }
        VERT_RIGHT_PRED => {
            let lt = b.get(-1, -1);
            let [t0, t1, t2, t3]: [i32; 4] = b.top(0);
            let [l0, l1, l2, _]: [i32; 4] = b.left(0);
            b.set(&[(0, 0), (1, 2)], (lt + t0 + 1) >> 1);
            b.set(&[(1, 0), (2, 2)], (t0 + t1 + 1) >> 1);
            b.set(&[(2, 0), (3, 2)], (t1 + t2 + 1) >> 1);
            b.set(&[(3, 0)], (t2 + t3 + 1) >> 1);
            b.set(&[(0, 1), (1, 3)], (l0 + 2 * lt + t0 + 2) >> 2);
            b.set(&[(1, 1), (2, 3)], (lt + 2 * t0 + t1 + 2) >> 2);
            b.set(&[(2, 1), (3, 3)], (t0 + 2 * t1 + t2 + 2) >> 2);
            b.set(&[(3, 1)], (t1 + 2 * t2 + t3 + 2) >> 2);
            b.set(&[(0, 2)], (lt + 2 * l0 + l1 + 2) >> 2);
            b.set(&[(0, 3)], (l0 + 2 * l1 + l2 + 2) >> 2);
        }
        VERT_LEFT_PRED => {
            let [t0, t1, t2, t3]: [i32; 4] = b.top(0);
            let [t4, t5, t6, _] = topright;
            b.set(&[(0, 0)], (t0 + t1 + 1) >> 1);
            b.set(&[(1, 0), (0, 2)], (t1 + t2 + 1) >> 1);
            b.set(&[(2, 0), (1, 2)], (t2 + t3 + 1) >> 1);
            b.set(&[(3, 0), (2, 2)], (t3 + t4 + 1) >> 1);
            b.set(&[(3, 2)], (t4 + t5 + 1) >> 1);
            b.set(&[(0, 1)], (t0 + 2 * t1 + t2 + 2) >> 2);
            b.set(&[(1, 1), (0, 3)], (t1 + 2 * t2 + t3 + 2) >> 2);
            b.set(&[(2, 1), (1, 3)], (t2 + 2 * t3 + t4 + 2) >> 2);
            b.set(&[(3, 1), (2, 3)], (t3 + 2 * t4 + t5 + 2) >> 2);
            b.set(&[(3, 3)], (t4 + 2 * t5 + t6 + 2) >> 2);
        }
        HOR_UP_PRED => {
            let [l0, l1, l2, l3]: [i32; 4] = b.left(0);
            b.set(&[(0, 0)], (l0 + l1 + 1) >> 1);
            b.set(&[(1, 0)], (l0 + 2 * l1 + l2 + 2) >> 2);
            b.set(&[(2, 0), (0, 1)], (l1 + l2 + 1) >> 1);
            b.set(&[(3, 0), (1, 1)], (l1 + 2 * l2 + l3 + 2) >> 2);
            b.set(&[(2, 1), (0, 2)], (l2 + l3 + 1) >> 1);
            b.set(&[(3, 1), (1, 2)], (l2 + 2 * l3 + l3 + 2) >> 2);
            b.set(&[(3, 2), (1, 3), (0, 3), (2, 2), (2, 3), (3, 3)], l3);
        }
        HOR_DOWN_PRED => {
            let lt = b.get(-1, -1);
            let [t0, t1, t2, _]: [i32; 4] = b.top(0);
            let [l0, l1, l2, l3]: [i32; 4] = b.left(0);
            b.set(&[(0, 0), (2, 1)], (lt + l0 + 1) >> 1);
            b.set(&[(1, 0), (3, 1)], (l0 + 2 * lt + t0 + 2) >> 2);
            b.set(&[(2, 0)], (lt + 2 * t0 + t1 + 2) >> 2);
            b.set(&[(3, 0)], (t0 + 2 * t1 + t2 + 2) >> 2);
            b.set(&[(0, 1), (2, 2)], (l0 + l1 + 1) >> 1);
            b.set(&[(1, 1), (3, 2)], (lt + 2 * l0 + l1 + 2) >> 2);
            b.set(&[(0, 2), (2, 3)], (l1 + l2 + 1) >> 1);
            b.set(&[(1, 2), (3, 3)], (l0 + 2 * l1 + l2 + 2) >> 2);
            b.set(&[(0, 3)], (l2 + l3 + 1) >> 1);
            b.set(&[(1, 3)], (l1 + 2 * l2 + l3 + 2) >> 2);
        }
        // The mode checks leave no other.
        _ => {}
    }
}

/// The filtered edges an 8x8 luma block predicts from
/// (`PREDICT_8x8_LOAD_*`), each made by one of these: the left column, the
/// top row and the eight samples beyond it, and the corner -- each smoothed
/// with its neighbours, the ends with what is there.
struct Edges8;

impl Edges8 {
    fn left<P: Pixel>(b: &Block<'_, P>, has_topleft: bool) -> [i32; 8] {
        let src = |y: isize| b.get(-1, y);
        let mut l = [0i32; 8];
        l[0] = ((if has_topleft { src(-1) } else { src(0) }) + 2 * src(0) + src(1) + 2) >> 2;
        for y in 1..7 {
            l[y] = (src(y as isize - 1) + 2 * src(y as isize) + src(y as isize + 1) + 2) >> 2;
        }
        l[7] = (src(6) + 3 * src(7) + 2) >> 2;
        l
    }

    /// The top row, and with `right` the eight beyond it (`t8` to `t15`).
    fn top<P: Pixel>(
        b: &Block<'_, P>,
        has_topleft: bool,
        has_topright: bool,
        right: bool,
    ) -> [i32; 16] {
        let src = |x: isize| b.get(x, -1);
        let mut t = [0i32; 16];
        t[0] = ((if has_topleft { src(-1) } else { src(0) }) + 2 * src(0) + src(1) + 2) >> 2;
        for x in 1..7 {
            t[x] = (src(x as isize - 1) + 2 * src(x as isize) + src(x as isize + 1) + 2) >> 2;
        }
        t[7] = ((if has_topright { src(8) } else { src(7) }) + 2 * src(7) + src(6) + 2) >> 2;
        if right {
            if has_topright {
                for x in 8..15 {
                    t[x] =
                        (src(x as isize - 1) + 2 * src(x as isize) + src(x as isize + 1) + 2) >> 2;
                }
                t[15] = (src(14) + 3 * src(15) + 2) >> 2;
            } else {
                let v = src(7);
                t[8..].fill(v);
            }
        }
        t
    }

    fn topleft<P: Pixel>(b: &Block<'_, P>) -> i32 {
        (b.get(-1, 0) + 2 * b.get(-1, -1) + b.get(0, -1) + 2) >> 2
    }
}

/// An 8x8 luma block predicted by `mode` from its filtered edges
/// (`pred8x8l`).
#[allow(
    clippy::too_many_lines,
    reason = "one arm a predictor, as h264pred_template.c has them"
)]
pub(crate) fn pred8x8l<P: Pixel>(
    mode: i8,
    s: &mut [P],
    at: usize,
    stride: usize,
    has_topleft: bool,
    has_topright: bool,
    bits: u32,
) {
    let mut b = Block { s, at, stride };
    let (tl, tr) = (has_topleft, has_topright);
    match mode {
        DC_128_PRED => b.fill(0, 0, 8, 8, 1 << (bits - 1)),
        LEFT_DC_PRED => {
            let l = Edges8::left(&b, tl);
            b.fill(0, 0, 8, 8, (l.iter().sum::<i32>() + 4) >> 3);
        }
        TOP_DC_PRED => {
            let t = Edges8::top(&b, tl, tr, false);
            b.fill(0, 0, 8, 8, (t[..8].iter().sum::<i32>() + 4) >> 3);
        }
        DC_PRED => {
            let l = Edges8::left(&b, tl);
            let t = Edges8::top(&b, tl, tr, false);
            b.fill(
                0,
                0,
                8,
                8,
                (l.iter().sum::<i32>() + t[..8].iter().sum::<i32>() + 8) >> 4,
            );
        }
        HOR_PRED => {
            let l = Edges8::left(&b, tl);
            for (y, &v) in l.iter().enumerate() {
                b.fill(0, y, 8, 1, v);
            }
        }
        VERT_PRED => {
            let t = Edges8::top(&b, tl, tr, false);
            for y in 0..8 {
                for (x, &v) in t[..8].iter().enumerate() {
                    b.put(x, y, v);
                }
            }
        }
        DIAG_DOWN_LEFT_PRED => {
            let t = Edges8::top(&b, tl, tr, true);
            let [
                t0,
                t1,
                t2,
                t3,
                t4,
                t5,
                t6,
                t7,
                t8,
                t9,
                t10,
                t11,
                t12,
                t13,
                t14,
                t15,
            ] = t;
            b.set(&[(0, 0)], (t0 + 2 * t1 + t2 + 2) >> 2);
            b.set(&[(0, 1), (1, 0)], (t1 + 2 * t2 + t3 + 2) >> 2);
            b.set(&[(0, 2), (1, 1), (2, 0)], (t2 + 2 * t3 + t4 + 2) >> 2);
            b.set(
                &[(0, 3), (1, 2), (2, 1), (3, 0)],
                (t3 + 2 * t4 + t5 + 2) >> 2,
            );
            b.set(
                &[(0, 4), (1, 3), (2, 2), (3, 1), (4, 0)],
                (t4 + 2 * t5 + t6 + 2) >> 2,
            );
            b.set(
                &[(0, 5), (1, 4), (2, 3), (3, 2), (4, 1), (5, 0)],
                (t5 + 2 * t6 + t7 + 2) >> 2,
            );
            b.set(
                &[(0, 6), (1, 5), (2, 4), (3, 3), (4, 2), (5, 1), (6, 0)],
                (t6 + 2 * t7 + t8 + 2) >> 2,
            );
            b.set(
                &[
                    (0, 7),
                    (1, 6),
                    (2, 5),
                    (3, 4),
                    (4, 3),
                    (5, 2),
                    (6, 1),
                    (7, 0),
                ],
                (t7 + 2 * t8 + t9 + 2) >> 2,
            );
            b.set(
                &[(1, 7), (2, 6), (3, 5), (4, 4), (5, 3), (6, 2), (7, 1)],
                (t8 + 2 * t9 + t10 + 2) >> 2,
            );
            b.set(
                &[(2, 7), (3, 6), (4, 5), (5, 4), (6, 3), (7, 2)],
                (t9 + 2 * t10 + t11 + 2) >> 2,
            );
            b.set(
                &[(3, 7), (4, 6), (5, 5), (6, 4), (7, 3)],
                (t10 + 2 * t11 + t12 + 2) >> 2,
            );
            b.set(
                &[(4, 7), (5, 6), (6, 5), (7, 4)],
                (t11 + 2 * t12 + t13 + 2) >> 2,
            );
            b.set(&[(5, 7), (6, 6), (7, 5)], (t12 + 2 * t13 + t14 + 2) >> 2);
            b.set(&[(6, 7), (7, 6)], (t13 + 2 * t14 + t15 + 2) >> 2);
            b.set(&[(7, 7)], (t14 + 3 * t15 + 2) >> 2);
        }
        DIAG_DOWN_RIGHT_PRED => {
            let [t0, t1, t2, t3, t4, t5, t6, t7, ..] = Edges8::top(&b, tl, tr, false);
            let [l0, l1, l2, l3, l4, l5, l6, l7] = Edges8::left(&b, tl);
            let lt = Edges8::topleft(&b);
            b.set(&[(0, 7)], (l7 + 2 * l6 + l5 + 2) >> 2);
            b.set(&[(0, 6), (1, 7)], (l6 + 2 * l5 + l4 + 2) >> 2);
            b.set(&[(0, 5), (1, 6), (2, 7)], (l5 + 2 * l4 + l3 + 2) >> 2);
            b.set(
                &[(0, 4), (1, 5), (2, 6), (3, 7)],
                (l4 + 2 * l3 + l2 + 2) >> 2,
            );
            b.set(
                &[(0, 3), (1, 4), (2, 5), (3, 6), (4, 7)],
                (l3 + 2 * l2 + l1 + 2) >> 2,
            );
            b.set(
                &[(0, 2), (1, 3), (2, 4), (3, 5), (4, 6), (5, 7)],
                (l2 + 2 * l1 + l0 + 2) >> 2,
            );
            b.set(
                &[(0, 1), (1, 2), (2, 3), (3, 4), (4, 5), (5, 6), (6, 7)],
                (l1 + 2 * l0 + lt + 2) >> 2,
            );
            b.set(
                &[
                    (0, 0),
                    (1, 1),
                    (2, 2),
                    (3, 3),
                    (4, 4),
                    (5, 5),
                    (6, 6),
                    (7, 7),
                ],
                (l0 + 2 * lt + t0 + 2) >> 2,
            );
            b.set(
                &[(1, 0), (2, 1), (3, 2), (4, 3), (5, 4), (6, 5), (7, 6)],
                (lt + 2 * t0 + t1 + 2) >> 2,
            );
            b.set(
                &[(2, 0), (3, 1), (4, 2), (5, 3), (6, 4), (7, 5)],
                (t0 + 2 * t1 + t2 + 2) >> 2,
            );
            b.set(
                &[(3, 0), (4, 1), (5, 2), (6, 3), (7, 4)],
                (t1 + 2 * t2 + t3 + 2) >> 2,
            );
            b.set(
                &[(4, 0), (5, 1), (6, 2), (7, 3)],
                (t2 + 2 * t3 + t4 + 2) >> 2,
            );
            b.set(&[(5, 0), (6, 1), (7, 2)], (t3 + 2 * t4 + t5 + 2) >> 2);
            b.set(&[(6, 0), (7, 1)], (t4 + 2 * t5 + t6 + 2) >> 2);
            b.set(&[(7, 0)], (t5 + 2 * t6 + t7 + 2) >> 2);
        }
        VERT_RIGHT_PRED => {
            let [t0, t1, t2, t3, t4, t5, t6, t7, ..] = Edges8::top(&b, tl, tr, false);
            let [l0, l1, l2, l3, l4, l5, l6, _] = Edges8::left(&b, tl);
            let lt = Edges8::topleft(&b);
            b.set(&[(0, 6)], (l5 + 2 * l4 + l3 + 2) >> 2);
            b.set(&[(0, 7)], (l6 + 2 * l5 + l4 + 2) >> 2);
            b.set(&[(0, 4), (1, 6)], (l3 + 2 * l2 + l1 + 2) >> 2);
            b.set(&[(0, 5), (1, 7)], (l4 + 2 * l3 + l2 + 2) >> 2);
            b.set(&[(0, 2), (1, 4), (2, 6)], (l1 + 2 * l0 + lt + 2) >> 2);
            b.set(&[(0, 3), (1, 5), (2, 7)], (l2 + 2 * l1 + l0 + 2) >> 2);
            b.set(
                &[(0, 1), (1, 3), (2, 5), (3, 7)],
                (l0 + 2 * lt + t0 + 2) >> 2,
            );
            b.set(&[(0, 0), (1, 2), (2, 4), (3, 6)], (lt + t0 + 1) >> 1);
            b.set(
                &[(1, 1), (2, 3), (3, 5), (4, 7)],
                (lt + 2 * t0 + t1 + 2) >> 2,
            );
            b.set(&[(1, 0), (2, 2), (3, 4), (4, 6)], (t0 + t1 + 1) >> 1);
            b.set(
                &[(2, 1), (3, 3), (4, 5), (5, 7)],
                (t0 + 2 * t1 + t2 + 2) >> 2,
            );
            b.set(&[(2, 0), (3, 2), (4, 4), (5, 6)], (t1 + t2 + 1) >> 1);
            b.set(
                &[(3, 1), (4, 3), (5, 5), (6, 7)],
                (t1 + 2 * t2 + t3 + 2) >> 2,
            );
            b.set(&[(3, 0), (4, 2), (5, 4), (6, 6)], (t2 + t3 + 1) >> 1);
            b.set(
                &[(4, 1), (5, 3), (6, 5), (7, 7)],
                (t2 + 2 * t3 + t4 + 2) >> 2,
            );
            b.set(&[(4, 0), (5, 2), (6, 4), (7, 6)], (t3 + t4 + 1) >> 1);
            b.set(&[(5, 1), (6, 3), (7, 5)], (t3 + 2 * t4 + t5 + 2) >> 2);
            b.set(&[(5, 0), (6, 2), (7, 4)], (t4 + t5 + 1) >> 1);
            b.set(&[(6, 1), (7, 3)], (t4 + 2 * t5 + t6 + 2) >> 2);
            b.set(&[(6, 0), (7, 2)], (t5 + t6 + 1) >> 1);
            b.set(&[(7, 1)], (t5 + 2 * t6 + t7 + 2) >> 2);
            b.set(&[(7, 0)], (t6 + t7 + 1) >> 1);
        }
        HOR_DOWN_PRED => {
            let [t0, t1, t2, t3, t4, t5, t6, ..] = Edges8::top(&b, tl, tr, false);
            let [l0, l1, l2, l3, l4, l5, l6, l7] = Edges8::left(&b, tl);
            let lt = Edges8::topleft(&b);
            b.set(&[(0, 7)], (l6 + l7 + 1) >> 1);
            b.set(&[(1, 7)], (l5 + 2 * l6 + l7 + 2) >> 2);
            b.set(&[(0, 6), (2, 7)], (l5 + l6 + 1) >> 1);
            b.set(&[(1, 6), (3, 7)], (l4 + 2 * l5 + l6 + 2) >> 2);
            b.set(&[(0, 5), (2, 6), (4, 7)], (l4 + l5 + 1) >> 1);
            b.set(&[(1, 5), (3, 6), (5, 7)], (l3 + 2 * l4 + l5 + 2) >> 2);
            b.set(&[(0, 4), (2, 5), (4, 6), (6, 7)], (l3 + l4 + 1) >> 1);
            b.set(
                &[(1, 4), (3, 5), (5, 6), (7, 7)],
                (l2 + 2 * l3 + l4 + 2) >> 2,
            );
            b.set(&[(0, 3), (2, 4), (4, 5), (6, 6)], (l2 + l3 + 1) >> 1);
            b.set(
                &[(1, 3), (3, 4), (5, 5), (7, 6)],
                (l1 + 2 * l2 + l3 + 2) >> 2,
            );
            b.set(&[(0, 2), (2, 3), (4, 4), (6, 5)], (l1 + l2 + 1) >> 1);
            b.set(
                &[(1, 2), (3, 3), (5, 4), (7, 5)],
                (l0 + 2 * l1 + l2 + 2) >> 2,
            );
            b.set(&[(0, 1), (2, 2), (4, 3), (6, 4)], (l0 + l1 + 1) >> 1);
            b.set(
                &[(1, 1), (3, 2), (5, 3), (7, 4)],
                (lt + 2 * l0 + l1 + 2) >> 2,
            );
            b.set(&[(0, 0), (2, 1), (4, 2), (6, 3)], (lt + l0 + 1) >> 1);
            b.set(
                &[(1, 0), (3, 1), (5, 2), (7, 3)],
                (l0 + 2 * lt + t0 + 2) >> 2,
            );
            b.set(&[(2, 0), (4, 1), (6, 2)], (t1 + 2 * t0 + lt + 2) >> 2);
            b.set(&[(3, 0), (5, 1), (7, 2)], (t2 + 2 * t1 + t0 + 2) >> 2);
            b.set(&[(4, 0), (6, 1)], (t3 + 2 * t2 + t1 + 2) >> 2);
            b.set(&[(5, 0), (7, 1)], (t4 + 2 * t3 + t2 + 2) >> 2);
            b.set(&[(6, 0)], (t5 + 2 * t4 + t3 + 2) >> 2);
            b.set(&[(7, 0)], (t6 + 2 * t5 + t4 + 2) >> 2);
        }
        VERT_LEFT_PRED => {
            let [t0, t1, t2, t3, t4, t5, t6, t7, t8, t9, t10, t11, t12, ..] =
                Edges8::top(&b, tl, tr, true);
            b.set(&[(0, 0)], (t0 + t1 + 1) >> 1);
            b.set(&[(0, 1)], (t0 + 2 * t1 + t2 + 2) >> 2);
            b.set(&[(0, 2), (1, 0)], (t1 + t2 + 1) >> 1);
            b.set(&[(0, 3), (1, 1)], (t1 + 2 * t2 + t3 + 2) >> 2);
            b.set(&[(0, 4), (1, 2), (2, 0)], (t2 + t3 + 1) >> 1);
            b.set(&[(0, 5), (1, 3), (2, 1)], (t2 + 2 * t3 + t4 + 2) >> 2);
            b.set(&[(0, 6), (1, 4), (2, 2), (3, 0)], (t3 + t4 + 1) >> 1);
            b.set(
                &[(0, 7), (1, 5), (2, 3), (3, 1)],
                (t3 + 2 * t4 + t5 + 2) >> 2,
            );
            b.set(&[(1, 6), (2, 4), (3, 2), (4, 0)], (t4 + t5 + 1) >> 1);
            b.set(
                &[(1, 7), (2, 5), (3, 3), (4, 1)],
                (t4 + 2 * t5 + t6 + 2) >> 2,
            );
            b.set(&[(2, 6), (3, 4), (4, 2), (5, 0)], (t5 + t6 + 1) >> 1);
            b.set(
                &[(2, 7), (3, 5), (4, 3), (5, 1)],
                (t5 + 2 * t6 + t7 + 2) >> 2,
            );
            b.set(&[(3, 6), (4, 4), (5, 2), (6, 0)], (t6 + t7 + 1) >> 1);
            b.set(
                &[(3, 7), (4, 5), (5, 3), (6, 1)],
                (t6 + 2 * t7 + t8 + 2) >> 2,
            );
            b.set(&[(4, 6), (5, 4), (6, 2), (7, 0)], (t7 + t8 + 1) >> 1);
            b.set(
                &[(4, 7), (5, 5), (6, 3), (7, 1)],
                (t7 + 2 * t8 + t9 + 2) >> 2,
            );
            b.set(&[(5, 6), (6, 4), (7, 2)], (t8 + t9 + 1) >> 1);
            b.set(&[(5, 7), (6, 5), (7, 3)], (t8 + 2 * t9 + t10 + 2) >> 2);
            b.set(&[(6, 6), (7, 4)], (t9 + t10 + 1) >> 1);
            b.set(&[(6, 7), (7, 5)], (t9 + 2 * t10 + t11 + 2) >> 2);
            b.set(&[(7, 6)], (t10 + t11 + 1) >> 1);
            b.set(&[(7, 7)], (t10 + 2 * t11 + t12 + 2) >> 2);
        }
        HOR_UP_PRED => {
            let [l0, l1, l2, l3, l4, l5, l6, l7] = Edges8::left(&b, tl);
            b.set(&[(0, 0)], (l0 + l1 + 1) >> 1);
            b.set(&[(1, 0)], (l0 + 2 * l1 + l2 + 2) >> 2);
            b.set(&[(0, 1), (2, 0)], (l1 + l2 + 1) >> 1);
            b.set(&[(1, 1), (3, 0)], (l1 + 2 * l2 + l3 + 2) >> 2);
            b.set(&[(0, 2), (2, 1), (4, 0)], (l2 + l3 + 1) >> 1);
            b.set(&[(1, 2), (3, 1), (5, 0)], (l2 + 2 * l3 + l4 + 2) >> 2);
            b.set(&[(0, 3), (2, 2), (4, 1), (6, 0)], (l3 + l4 + 1) >> 1);
            b.set(
                &[(1, 3), (3, 2), (5, 1), (7, 0)],
                (l3 + 2 * l4 + l5 + 2) >> 2,
            );
            b.set(&[(0, 4), (2, 3), (4, 2), (6, 1)], (l4 + l5 + 1) >> 1);
            b.set(
                &[(1, 4), (3, 3), (5, 2), (7, 1)],
                (l4 + 2 * l5 + l6 + 2) >> 2,
            );
            b.set(&[(0, 5), (2, 4), (4, 3), (6, 2)], (l5 + l6 + 1) >> 1);
            b.set(
                &[(1, 5), (3, 4), (5, 3), (7, 2)],
                (l5 + 2 * l6 + l7 + 2) >> 2,
            );
            b.set(&[(0, 6), (2, 5), (4, 4), (6, 3)], (l6 + l7 + 1) >> 1);
            b.set(&[(1, 6), (3, 5), (5, 4), (7, 3)], (l6 + 3 * l7 + 2) >> 2);
            b.set(
                &[
                    (0, 7),
                    (1, 7),
                    (2, 6),
                    (2, 7),
                    (3, 6),
                    (3, 7),
                    (4, 5),
                    (4, 6),
                    (4, 7),
                    (5, 5),
                    (5, 6),
                    (5, 7),
                    (6, 4),
                    (6, 5),
                    (6, 6),
                    (6, 7),
                    (7, 4),
                    (7, 5),
                    (7, 6),
                    (7, 7),
                ],
                l7,
            );
        }
        _ => {}
    }
}

/// A 16x16 luma block predicted by `mode` (`pred16x16`).
pub(crate) fn pred16x16<P: Pixel>(mode: i32, s: &mut [P], at: usize, stride: usize, bits: u32) {
    let mut b = Block { s, at, stride };
    match mode {
        VERT_PRED8X8 => {
            let t: [i32; 16] = b.top(0);
            for y in 0..16 {
                for (x, &v) in t.iter().enumerate() {
                    b.put(x, y, v);
                }
            }
        }
        HOR_PRED8X8 => {
            let l: [i32; 16] = b.left(0);
            for (y, &v) in l.iter().enumerate() {
                b.fill(0, y, 16, 1, v);
            }
        }
        DC_PRED8X8 => {
            let l: [i32; 16] = b.left(0);
            let t: [i32; 16] = b.top(0);
            b.fill(
                0,
                0,
                16,
                16,
                (l.iter().sum::<i32>() + t.iter().sum::<i32>() + 16) >> 5,
            );
        }
        LEFT_DC_PRED8X8 => {
            let l: [i32; 16] = b.left(0);
            b.fill(0, 0, 16, 16, (l.iter().sum::<i32>() + 8) >> 4);
        }
        TOP_DC_PRED8X8 => {
            let t: [i32; 16] = b.top(0);
            b.fill(0, 0, 16, 16, (t.iter().sum::<i32>() + 8) >> 4);
        }
        DC_128_PRED8X8 => b.fill(0, 0, 16, 16, 1 << (bits - 1)),
        PLANE_PRED8X8 => {
            // pred16x16_plane_compat without SVQ3's or RV40's rounding.
            let top = |x: isize| b.get(x, -1);
            let left = |y: isize| b.get(-1, y);
            let mut h = top(8) - top(6);
            let mut v = left(8) - left(6);
            for k in 2..=8 {
                h += k as i32 * (top(7 + k) - top(7 - k));
                v += k as i32 * (left(7 + k) - left(7 - k));
            }
            let h = (5 * h + 32) >> 6;
            let v = (5 * v + 32) >> 6;
            let mut a = 16 * (left(15) + top(15) + 1) - 7 * (v + h);
            for y in 0..16 {
                let mut c = a;
                a += v;
                for x in 0..16 {
                    b.put(x, y, clip(c >> 5, bits));
                    c += h;
                }
            }
        }
        _ => {}
    }
}

/// A chroma block -- 8x8, or 8x16 for 4:2:2 (`tall`) -- predicted by
/// `mode` (`pred8x8`, and `pred8x16` for 4:2:2), the Alzheimer DCs
/// included.
#[allow(
    clippy::too_many_lines,
    reason = "one arm a predictor, as h264pred_template.c has them"
)]
pub(crate) fn pred_chroma<P: Pixel>(
    mode: i32,
    s: &mut [P],
    at: usize,
    stride: usize,
    tall: bool,
    bits: u32,
) {
    let rows = if tall { 16 } else { 8 };
    match mode {
        VERT_PRED8X8 => {
            let mut b = Block { s, at, stride };
            let t: [i32; 8] = b.top(0);
            for y in 0..rows {
                for (x, &v) in t.iter().enumerate() {
                    b.put(x, y, v);
                }
            }
        }
        HOR_PRED8X8 => {
            let mut b = Block { s, at, stride };
            for y in 0..rows {
                let v = b.get(-1, y as isize);
                b.fill(0, y, 8, 1, v);
            }
        }
        DC_128_PRED8X8 => {
            let mut b = Block { s, at, stride };
            b.fill(0, 0, 8, rows, 1 << (bits - 1));
        }
        LEFT_DC_PRED8X8 => {
            left_dc8(s, at, stride);
            if tall {
                left_dc8(s, at + 8 * stride, stride);
            }
        }
        TOP_DC_PRED8X8 => {
            let mut b = Block { s, at, stride };
            let t: [i32; 8] = b.top(0);
            let dc0 = (t[..4].iter().sum::<i32>() + 2) >> 2;
            let dc1 = (t[4..].iter().sum::<i32>() + 2) >> 2;
            b.fill(0, 0, 4, rows, dc0);
            b.fill(4, 0, 4, rows, dc1);
        }
        DC_PRED8X8 => dc8(s, at, stride, tall),
        PLANE_PRED8X8 => plane8(s, at, stride, tall, bits),
        ALZHEIMER_DC_L0T_PRED8X8 => {
            pred_chroma(TOP_DC_PRED8X8, s, at, stride, tall, bits);
            pred4x4(DC_PRED, s, at, stride, [0; 4], bits);
        }
        ALZHEIMER_DC_0LT_PRED8X8 => {
            dc8(s, at, stride, tall);
            pred4x4(TOP_DC_PRED, s, at, stride, [0; 4], bits);
        }
        ALZHEIMER_DC_L00_PRED8X8 => {
            pred_chroma(LEFT_DC_PRED8X8, s, at, stride, tall, bits);
            pred4x4(DC_128_PRED, s, at + 4 * stride, stride, [0; 4], bits);
            pred4x4(DC_128_PRED, s, at + 4 * stride + 4, stride, [0; 4], bits);
        }
        ALZHEIMER_DC_0L0_PRED8X8 => {
            pred_chroma(LEFT_DC_PRED8X8, s, at, stride, tall, bits);
            pred4x4(DC_128_PRED, s, at, stride, [0; 4], bits);
            pred4x4(DC_128_PRED, s, at + 4, stride, [0; 4], bits);
        }
        _ => {}
    }
}

/// `pred8x8_left_dc`: each 4-row half its left neighbours' DC.
fn left_dc8<P: Pixel>(s: &mut [P], at: usize, stride: usize) {
    let mut b = Block { s, at, stride };
    let l: [i32; 8] = b.left(0);
    let dc0 = (l[..4].iter().sum::<i32>() + 2) >> 2;
    let dc2 = (l[4..].iter().sum::<i32>() + 2) >> 2;
    b.fill(0, 0, 8, 4, dc0);
    b.fill(0, 4, 8, 4, dc2);
}

/// `pred8x8_dc` and `pred8x16_dc`: each 4x4 block the DC of the
/// neighbours it borders -- the corner block both, the edge blocks their
/// own side, the rest a mix.
fn dc8<P: Pixel>(s: &mut [P], at: usize, stride: usize, tall: bool) {
    let mut b = Block { s, at, stride };
    let t: [i32; 8] = b.top(0);
    let l: [i32; 16] = if tall {
        b.left(0)
    } else {
        let l8: [i32; 8] = b.left(0);
        core::array::from_fn(|i| l8.get(i).copied().unwrap_or(0))
    };
    let sum = |v: &[i32]| v.iter().sum::<i32>();
    let dc0 = sum(&l[..4]) + sum(&t[..4]);
    let dc1 = sum(&t[4..]);
    let dc2 = sum(&l[4..8]);
    b.fill(0, 0, 4, 4, (dc0 + 4) >> 3);
    b.fill(4, 0, 4, 4, (dc1 + 2) >> 2);
    b.fill(0, 4, 4, 4, (dc2 + 2) >> 2);
    b.fill(4, 4, 4, 4, (dc1 + dc2 + 4) >> 3);
    if tall {
        let dc3 = sum(&l[8..12]);
        let dc4 = sum(&l[12..]);
        b.fill(0, 8, 4, 4, (dc3 + 2) >> 2);
        b.fill(4, 8, 4, 4, (dc1 + dc3 + 4) >> 3);
        b.fill(0, 12, 4, 4, (dc4 + 2) >> 2);
        b.fill(4, 12, 4, 4, (dc1 + dc4 + 4) >> 3);
    }
}

/// `pred8x8_plane` and `pred8x16_plane`.
fn plane8<P: Pixel>(s: &mut [P], at: usize, stride: usize, tall: bool, bits: u32) {
    let mut b = Block { s, at, stride };
    let top = |b: &Block<'_, P>, x: isize| b.get(x, -1);
    let left = |b: &Block<'_, P>, y: isize| b.get(-1, y);
    // The left column's centre: row 3.5 of 8, or 7.5 of 16.
    let mid: isize = if tall { 7 } else { 3 };
    let mut h = top(&b, 4) - top(&b, 2);
    let mut v = left(&b, mid + 1) - left(&b, mid - 1);
    for k in 2..=4 {
        h += k as i32 * (top(&b, 3 + k) - top(&b, 3 - k));
        v += k as i32 * (left(&b, mid + k) - left(&b, mid - k));
    }
    if tall {
        for k in 5..=8 {
            v += k as i32 * (left(&b, mid + k) - left(&b, mid - k));
        }
    }
    let h = (17 * h + 16) >> 5;
    let (v, mut a) = if tall {
        let v = (5 * v + 32) >> 6;
        (v, 16 * (left(&b, 15) + top(&b, 7) + 1) - 7 * v - 3 * h)
    } else {
        let v = (17 * v + 16) >> 5;
        (v, 16 * (left(&b, 7) + top(&b, 7) + 1) - 3 * (v + h))
    };
    for y in 0..if tall { 16 } else { 8 } {
        let mut c = a;
        a += v;
        for x in 0..8 {
            b.put(x, y, clip(c >> 5, bits));
            c += h;
        }
    }
}

/// Lossless 4x4 prediction with the residual summed along it
/// (`pred4x4_vertical_add`, `pred4x4_horizontal_add`): each sample the one
/// before it plus its residual, wrapping as the sample type does. The
/// block left zero.
pub(crate) fn pred4x4_add<P: Pixel>(
    vertical: bool,
    s: &mut [P],
    at: usize,
    stride: usize,
    block: &mut [i32],
) {
    pred_add(vertical, 4, s, at, stride, block);
}

/// The same for an 8x8 block from its unfiltered edge
/// (`pred8x8l_vertical_add`, `pred8x8l_horizontal_add`).
pub(crate) fn pred8x8l_add<P: Pixel>(
    vertical: bool,
    s: &mut [P],
    at: usize,
    stride: usize,
    block: &mut [i32],
) {
    pred_add(vertical, 8, s, at, stride, block);
}

#[allow(
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    reason = "the neighbour row or column of a block inside the plane"
)]
fn pred_add<P: Pixel>(
    vertical: bool,
    n: usize,
    s: &mut [P],
    at: usize,
    stride: usize,
    block: &mut [i32],
) {
    for i in 0..n {
        if vertical {
            let mut v = s[at + i - stride].int();
            for y in 0..n {
                v = P::from_int(v + block[i + n * y]).int();
                s[at + i + y * stride] = P::from_int(v);
            }
        } else {
            let mut v = s[at + i * stride - 1].int();
            for x in 0..n {
                v = P::from_int(v + block[x + n * i]).int();
                s[at + x + i * stride] = P::from_int(v);
            }
        }
    }
    block[..n * n].fill(0);
}

/// Lossless 8x8 prediction from the filtered edge, the residual summed
/// along it (`pred8x8l_vertical_filter_add`,
/// `pred8x8l_horizontal_filter_add`).
#[allow(clippy::too_many_arguments, reason = "FFmpeg's signature")]
pub(crate) fn pred8x8l_filter_add<P: Pixel>(
    vertical: bool,
    s: &mut [P],
    at: usize,
    stride: usize,
    block: &mut [i32],
    has_topleft: bool,
    has_topright: bool,
) {
    let b = Block { s, at, stride };
    let edge = if vertical {
        let t = Edges8::top(&b, has_topleft, has_topright, false);
        core::array::from_fn::<i32, 8, _>(|i| t[i])
    } else {
        Edges8::left(&b, has_topleft)
    };
    let s = b.s;
    for (i, &e) in edge.iter().enumerate() {
        // The filtered edge as a sample first, as FFmpeg's pixel array
        // holds it.
        let mut v = P::from_int(e).int();
        for k in 0..8 {
            let (x, y, c) = if vertical {
                (i, k, block[i + 8 * k])
            } else {
                (k, i, block[k + 8 * i])
            };
            v = P::from_int(v + c).int();
            s[at + x + y * stride] = P::from_int(v);
        }
    }
    block[..64].fill(0);
}

/// A 16x16 or chroma block's lossless prediction, a 4x4 block at a time
/// (`pred16x16_*_add`, `pred8x8_*_add`, `pred8x16_*_add`): `blocks` the
/// blocks' indices into `offsets` and their coefficients' into `block`.
pub(crate) fn pred_blocks_add<P: Pixel>(
    vertical: bool,
    s: &mut [P],
    at: usize,
    offsets: &[usize],
    blocks: impl Iterator<Item = (usize, usize)>,
    block: &mut [i32],
    stride: usize,
) {
    for (offset, coefficients) in blocks {
        pred4x4_add(
            vertical,
            s,
            at + offsets[offset],
            stride,
            &mut block[coefficients * 16..],
        );
    }
}

#[cfg(test)]
mod tests;
