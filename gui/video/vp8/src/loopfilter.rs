//! The loop filter: smoothing across the edges between blocks, in place,
//! before the frame is shown or predicted from.
//!
//! Each macroblock's left and top edges are filtered with the macroblock
//! filter, which reaches three pixels to each side; its inner 4x4 edges, if
//! it has any coefficients or is split or 4x4-predicted, with the block
//! filter, which reaches two. How hard is the macroblock's filter level: the
//! frame's, adjusted by its segment, its reference frame and its mode. The
//! simple filter (VP8's versions 1 and 3, or a header that asks for it)
//! filters luma only, and only the two pixels next to an edge.
//!
//! The decoder filters a row of macroblocks once the row below it has been
//! reconstructed (`decodeframe`), since intra prediction reads unfiltered
//! pixels; filtering a row never reaches into the row below it.
//!
//! Translated into Rust from libvpx v1.17.0's `vp8/common/vp8_loopfilter.c`
//! and `vp8/common/loopfilter_filters.c` (copyright the WebM project
//! authors), used under libvpx's BSD licence and patent grant
//! (`licenses/libvpx-LICENSE`, `licenses/libvpx-PATENTS`).

#![allow(
    clippy::indexing_slicing,
    reason = "edge positions are a macroblock's inside the plane, with the three pixels the filters reach on each side inside the picture or its border; level tables are indexed by levels clamped to 0..=63, segments below 4, references below 4 and modes below 10"
)]
#![allow(
    clippy::arithmetic_side_effects,
    reason = "filter arithmetic is on pixels less 128, clamped to i8 at each step as libvpx's is; positions are inside the plane"
)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    reason = "values are clamped to i8 before they become i8, and turned back into pixels by flipping the top bit, as libvpx's signed chars are"
)]

use crate::frame::{Frame, Target};
use crate::modes::{B_PRED, ModeGrid, ModeInfo, SPLITMV};

/// The highest filter level.
pub(crate) const MAX_LOOP_FILTER: usize = 63;

/// The filter's thresholds for one frame: libvpx's `loop_filter_info_n`,
/// computed for the frame's level, sharpness and adjustments.
#[derive(Clone, Debug)]
pub(crate) struct LoopFilterInfo {
    /// How different two neighbours inside a block may be and still be
    /// smoothed, by level: libvpx's `lim`.
    lim: [u8; MAX_LOOP_FILTER + 1],
    /// The edge threshold of the block filter, by level: `blim`.
    blim: [u8; MAX_LOOP_FILTER + 1],
    /// The edge threshold of the macroblock filter, by level: `mblim`.
    mblim: [u8; MAX_LOOP_FILTER + 1],
    /// The high edge variance threshold, by key frame (0) or inter frame (1)
    /// and level: `hev_thr_lut`.
    hev_thr: [[u8; MAX_LOOP_FILTER + 1]; 2],
    /// Each macroblock's level, by segment, reference frame and mode class
    /// (`mode_lf_lut`): libvpx's `lvl`.
    lvl: [[[u8; 4]; 4]; 4],
}

/// The class of each macroblock mode for the level adjustments: libvpx's
/// `mode_lf_lut`. 0 is 4x4-predicted, 1 whole-block intra or zero motion, 2
/// moving, 3 split.
const MODE_LF_LUT: [u8; 10] = [1, 1, 1, 1, 0, 2, 2, 1, 2, 3];

/// The adjustments to the frame's level: by segment and by reference frame
/// and mode.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Adjustments {
    /// Each segment's level, or its change to the frame's, if segmentation is
    /// on: `(absolute, values)`.
    pub(crate) segments: Option<(bool, [i8; 4])>,
    /// Changes by reference frame and by mode class, if enabled.
    pub(crate) deltas: Option<([i8; 4], [i8; 4])>,
}

impl LoopFilterInfo {
    /// The thresholds for a frame filtered at `level` with `sharpness`:
    /// libvpx's `vp8_loop_filter_update_sharpness`, `lf_init_lut` and
    /// `vp8_loop_filter_frame_init`.
    pub(crate) fn new(level: u8, sharpness: u8, adj: &Adjustments) -> Self {
        let mut lfi = Self {
            lim: [0; MAX_LOOP_FILTER + 1],
            blim: [0; MAX_LOOP_FILTER + 1],
            mblim: [0; MAX_LOOP_FILTER + 1],
            hev_thr: [[0; MAX_LOOP_FILTER + 1]; 2],
            lvl: [[[0; 4]; 4]; 4],
        };
        for i in 0..=MAX_LOOP_FILTER {
            let filt_lvl = i as u8;
            let mut limit = filt_lvl >> u8::from(sharpness > 0);
            limit >>= u8::from(sharpness > 4);
            if sharpness > 0 && limit > 9 - sharpness.min(9) {
                limit = 9 - sharpness.min(9);
            }
            limit = limit.max(1);
            lfi.lim[i] = limit;
            lfi.blim[i] = 2 * filt_lvl + limit;
            lfi.mblim[i] = 2 * (filt_lvl + 2) + limit;
            (lfi.hev_thr[0][i], lfi.hev_thr[1][i]) = match i {
                40.. => (2, 3),
                20..=39 => (1, 2),
                15..=19 => (1, 1),
                _ => (0, 0),
            };
        }
        let clamp = |v: i32| v.clamp(0, MAX_LOOP_FILTER as i32) as u8;
        for seg in 0..4 {
            let mut lvl_seg = i32::from(level);
            if let Some((absolute, values)) = adj.segments {
                let v = i32::from(values[seg]);
                lvl_seg = if absolute { v } else { lvl_seg + v };
                lvl_seg = lvl_seg.clamp(0, 63);
            }
            let Some((ref_deltas, mode_deltas)) = adj.deltas else {
                lfi.lvl[seg] = [[lvl_seg as u8; 4]; 4];
                continue;
            };
            // Intra: 4x4-predicted macroblocks take the first mode delta;
            // the rest none.
            let lvl_ref = lvl_seg + i32::from(ref_deltas[0]);
            lfi.lvl[seg][0][0] = clamp(lvl_ref + i32::from(mode_deltas[0]));
            lfi.lvl[seg][0][1] = clamp(lvl_ref);
            // Inter: by reference frame and mode class. Class 0 never
            // occurs, and is left 0 as libvpx leaves it unset.
            for r in 1..4 {
                let lvl_ref = lvl_seg + i32::from(ref_deltas[r]);
                for mode in 1..4 {
                    lfi.lvl[seg][r][mode] = clamp(lvl_ref + i32::from(mode_deltas[mode]));
                }
            }
        }
        lfi
    }
}

/// A pixel as a signed byte, about 0: libvpx's `(signed char)p ^ 0x80`.
#[inline(always)]
fn s(p: u8) -> i8 {
    (p ^ 0x80) as i8
}

/// A signed byte back to a pixel: libvpx's `v ^ 0x80`.
#[inline(always)]
fn u(v: i8) -> u8 {
    (v as u8) ^ 0x80
}

/// A condition as libvpx's masks hold it: all ones, or nothing.
#[inline(always)]
fn mask(c: bool) -> i8 {
    -i8::from(c)
}

/// `clamp(f + 3 * d)` to a signed byte, where `d` is the difference of two
/// signed bytes: libvpx's C computes it in an int and clamps; this adds the
/// difference, clamped, three times with saturation, as libvpx's SSE2 does.
/// The two agree: the sum moves the same way at each step, so once it
/// saturates it stays saturated, and the true sum is beyond the clamp too.
#[inline(always)]
fn add_three(f: i8, a: i8, b: i8) -> i8 {
    let d = a.saturating_sub(b);
    f.saturating_add(d).saturating_add(d).saturating_add(d)
}

/// The pixels across an edge at `N` positions along it: `px[k][i]` is
/// pixel `k - 4` across the edge (p3, p2, p1, p0, q0, q1, q2, q3) at
/// position `i`. The filters below compute on every position at once, as
/// libvpx's SIMD does, in signed bytes with saturation where libvpx's C
/// clamps, and with libvpx's C's masks instead of its branches -- the same
/// values, since the C computes unconditionally too.
type Across<const N: usize> = [[u8; N]; 8];

/// Gather `rows` of the pixels across an edge at `at`, centred on it: the
/// edge's position `i` is `at + i * along`, the pixels across it `across`
/// apart.
#[inline(always)]
fn load<const N: usize>(
    data: &[u8],
    at: usize,
    across: usize,
    along: usize,
    rows: usize,
) -> Across<N> {
    let mut px = [[0u8; N]; 8];
    let first = 4 - rows / 2;
    if along == 1 {
        // A horizontal edge: each pixel across it is a run of N in a row.
        for (k, out) in px.iter_mut().enumerate().skip(first).take(rows) {
            let start = at + k * across - 4 * across;
            out.copy_from_slice(&data[start..start + N]);
        }
    } else {
        // A vertical edge: the pixels across it are a run of `rows` in each
        // of N rows, transposed into lanes.
        for i in 0..N {
            let start = at + i * along + first - 4;
            let run = &data[start..start + rows];
            for (j, &p) in run.iter().enumerate() {
                px[first + j][i] = p;
            }
        }
    }
    px
}

/// Put back the pixels `k` in `range` that [`load`] gathered.
#[inline(always)]
fn store<const N: usize>(
    data: &mut [u8],
    at: usize,
    across: usize,
    along: usize,
    px: &Across<N>,
    range: core::ops::Range<usize>,
) {
    if along == 1 {
        for k in range {
            let start = at + k * across - 4 * across;
            data[start..start + N].copy_from_slice(&px[k]);
        }
    } else {
        let (first, len) = (range.start, range.len());
        for i in 0..N {
            let start = at + i * along + first - 4;
            let run = &mut data[start..start + len];
            for (j, p) in run.iter_mut().enumerate() {
                *p = px[first + j][i];
            }
        }
    }
}

/// The normal filters' common start, at each position: whether to filter
/// at all (libvpx's `vp8_filter_mask`) and whether the edge has high
/// variance (`vp8_hevmask`), as masks. The edge sum saturates at 255, as
/// libvpx's SSE2 lets it, which changes no answer: every threshold is below
/// 255.
#[inline(always)]
fn masks<const N: usize>(px: &Across<N>, t: Thresholds) -> ([i8; N], [i8; N]) {
    let [p3, p2, p1, p0, q0, q1, q2, q3] = px;
    let mut m = [0i8; N];
    let mut h = [0i8; N];
    for i in 0..N {
        let interior = p3[i]
            .abs_diff(p2[i])
            .max(p2[i].abs_diff(p1[i]))
            .max(p1[i].abs_diff(p0[i]))
            .max(q1[i].abs_diff(q0[i]))
            .max(q2[i].abs_diff(q1[i]))
            .max(q3[i].abs_diff(q2[i]));
        let d0 = p0[i].abs_diff(q0[i]);
        let edge = d0
            .saturating_add(d0)
            .saturating_add(p1[i].abs_diff(q1[i]) >> 1);
        m[i] = mask(interior <= t.interior && edge <= t.edge);
        h[i] = mask(p1[i].abs_diff(p0[i]) > t.hev || q1[i].abs_diff(q0[i]) > t.hev);
    }
    (m, h)
}

/// The block filter across `N` positions of an edge: libvpx's `vp8_filter`
/// where `vp8_filter_mask` says to.
#[inline(always)]
fn filter_lanes<const N: usize>(px: &mut Across<N>, t: Thresholds) {
    let (m, h) = masks(px, t);
    for i in 0..N {
        let (ps1, ps0, qs0, qs1) = (s(px[2][i]), s(px[3][i]), s(px[4][i]), s(px[5][i]));
        // The outer taps only where the edge has high variance.
        let f = add_three(ps1.saturating_sub(qs1) & h[i], qs0, ps0) & m[i];
        // Round one side +4 and the other +3, so that a value of 4 does not
        // overshoot.
        let filter1 = f.saturating_add(4) >> 3;
        let filter2 = f.saturating_add(3) >> 3;
        px[4][i] = u(qs0.saturating_sub(filter1));
        px[3][i] = u(ps0.saturating_add(filter2));
        // And the next pixels out, only where the variance is not high.
        // `filter1` is at most 15, so the increment cannot overflow.
        let a = ((filter1 + 1) >> 1) & !h[i];
        px[5][i] = u(qs1.saturating_sub(a));
        px[2][i] = u(ps1.saturating_add(a));
    }
}

/// The macroblock filter across `N` positions of an edge: libvpx's
/// `vp8_mbfilter` where `vp8_filter_mask` says to.
#[inline(always)]
fn mbfilter_lanes<const N: usize>(px: &mut Across<N>, t: Thresholds) {
    let (m, h) = masks(px, t);
    for i in 0..N {
        let (ps2, ps1, ps0) = (s(px[1][i]), s(px[2][i]), s(px[3][i]));
        let (qs0, qs1, qs2) = (s(px[4][i]), s(px[5][i]), s(px[6][i]));
        let f = add_three(ps1.saturating_sub(qs1), qs0, ps0) & m[i];
        // Where the variance is high, only the two pixels at the edge move,
        // as the block filter moves them.
        let hf = f & h[i];
        let filter1 = hf.saturating_add(4) >> 3;
        let filter2 = hf.saturating_add(3) >> 3;
        let qs0 = qs0.saturating_sub(filter1);
        let ps0 = ps0.saturating_add(filter2);
        // Elsewhere three pixels each side move by 27/128, 18/128 and 9/128
        // of the difference: at most 27 either way, so no clamp is needed.
        let w = i16::from(f & !h[i]);
        let part = |k: i16| ((63 + w * k) >> 7) as i8;
        let (u27, u18, u9) = (part(27), part(18), part(9));
        px[4][i] = u(qs0.saturating_sub(u27));
        px[3][i] = u(ps0.saturating_add(u27));
        px[5][i] = u(qs1.saturating_sub(u18));
        px[2][i] = u(ps1.saturating_add(u18));
        px[6][i] = u(qs2.saturating_sub(u9));
        px[1][i] = u(ps2.saturating_add(u9));
    }
}

/// The thresholds one edge is filtered with.
#[derive(Clone, Copy, Debug)]
struct Thresholds {
    edge: u8,
    interior: u8,
    hev: u8,
}

/// Filter `count` times 8 positions along an edge, the pixels across it at
/// `at + k * across` for k from -4 to 3, positions `along` apart: libvpx's
/// `loop_filter_horizontal_edge_c` (across = stride, along = 1) and
/// `_vertical_edge_c` (across = 1, along = stride).
fn block_edge(
    data: &mut [u8],
    at: usize,
    across: usize,
    along: usize,
    count: usize,
    t: Thresholds,
) {
    if count == 2 {
        let mut px = load::<16>(data, at, across, along, 8);
        filter_lanes(&mut px, t);
        store(data, at, across, along, &px, 2..6);
    } else {
        let mut px = load::<8>(data, at, across, along, 8);
        filter_lanes(&mut px, t);
        store(data, at, across, along, &px, 2..6);
    }
}

/// [`block_edge`] with the macroblock filter: libvpx's
/// `mbloop_filter_horizontal_edge_c` and `_vertical_edge_c`.
fn mb_edge(data: &mut [u8], at: usize, across: usize, along: usize, count: usize, t: Thresholds) {
    if count == 2 {
        let mut px = load::<16>(data, at, across, along, 8);
        mbfilter_lanes(&mut px, t);
        store(data, at, across, along, &px, 1..7);
    } else {
        let mut px = load::<8>(data, at, across, along, 8);
        mbfilter_lanes(&mut px, t);
        store(data, at, across, along, &px, 1..7);
    }
}

/// The simple filter across 16 positions of an edge: libvpx's
/// `vp8_loop_filter_simple_horizontal_edge_c` and `_vertical_edge_c`, with
/// `vp8_simple_filter_mask` and `vp8_simple_filter`.
fn simple_edge(data: &mut [u8], at: usize, across: usize, along: usize, blimit: u8) {
    let mut px = load::<16>(data, at, across, along, 4);
    for i in 0..16 {
        let (p1, p0, q0, q1) = (px[2][i], px[3][i], px[4][i], px[5][i]);
        let d0 = p0.abs_diff(q0);
        let edge = d0.saturating_add(d0).saturating_add(p1.abs_diff(q1) >> 1);
        let (ps1, ps0, qs0, qs1) = (s(p1), s(p0), s(q0), s(q1));
        let f = add_three(ps1.saturating_sub(qs1), qs0, ps0) & mask(edge <= blimit);
        let filter1 = f.saturating_add(4) >> 3;
        let filter2 = f.saturating_add(3) >> 3;
        px[4][i] = u(qs0.saturating_sub(filter1));
        px[3][i] = u(ps0.saturating_add(filter2));
    }
    store(data, at, across, along, &px, 3..5);
}

/// Filter macroblock row `mb_row` of `frame`: libvpx's
/// `vp8_loop_filter_row_normal` or `_row_simple`. `key_frame` chooses the
/// high-variance thresholds.
pub(crate) fn filter_row(
    frame: &mut Frame,
    grid: &ModeGrid,
    lfi: &LoopFilterInfo,
    mb_row: usize,
    simple: bool,
    key_frame: bool,
) {
    let mut t = frame.target();
    for mb_col in 0..grid.mb_cols {
        filter_mb(
            &mut t,
            grid.get(mb_row, mb_col),
            lfi,
            (mb_row, mb_col),
            simple,
            key_frame,
        );
    }
}

/// Filter the macroblock at `(mb_row, mb_col)` of `t`, whose modes are `mi`:
/// its left edge, the edges inside it, its top edge -- the order libvpx's
/// row filters take, and its threads take a macroblock at a time
/// (`mt_decode_mb_rows`). Its left and top edges reach three pixels into
/// the macroblocks there.
pub(crate) fn filter_mb(
    t: &mut Target<'_>,
    mi: &ModeInfo,
    lfi: &LoopFilterInfo,
    (mb_row, mb_col): (usize, usize),
    simple: bool,
    key_frame: bool,
) {
    let frame_type = usize::from(!key_frame);
    let skip_lf = mi.mode != B_PRED && mi.mode != SPLITMV && mi.mb_skip_coeff;
    let mode_index = usize::from(MODE_LF_LUT[usize::from(mi.mode.min(9))]);
    let level = lfi.lvl[usize::from(mi.segment_id & 3)][usize::from(mi.ref_frame & 3)][mode_index];
    if level == 0 {
        return;
    }
    let level = usize::from(level);
    if simple {
        let (at, stride) = (t.at(0, mb_col * 16, mb_row * 16), t.strides[0]);
        let y = &mut *t.planes[0];
        let (mblim, blim) = (lfi.mblim[level], lfi.blim[level]);
        if mb_col > 0 {
            simple_edge(y, at, 1, stride, mblim);
        }
        if !skip_lf {
            for k in [4, 8, 12] {
                simple_edge(y, at + k, 1, stride, blim);
            }
        }
        if mb_row > 0 {
            simple_edge(y, at, stride, 1, mblim);
        }
        if !skip_lf {
            for k in [4, 8, 12] {
                simple_edge(y, at + k * stride, stride, 1, blim);
            }
        }
        return;
    }
    let mb = Thresholds {
        edge: lfi.mblim[level],
        interior: lfi.lim[level],
        hev: lfi.hev_thr[frame_type][level],
    };
    let b = Thresholds {
        edge: lfi.blim[level],
        ..mb
    };
    // Each plane's macroblock: where it starts, its stride, and how many
    // eight-pixel runs an edge of it is.
    let place = |t: &Target<'_>, p: usize| {
        let (size, count) = if p == 0 { (16, 2) } else { (8, 1) };
        (t.at(p, mb_col * size, mb_row * size), t.strides[p], count)
    };
    // libvpx's order: the left edge, the inner vertical edges, the top
    // edge, the inner horizontal edges; each over luma then chroma.
    if mb_col > 0 {
        for p in 0..3 {
            let (at, stride, count) = place(t, p);
            mb_edge(t.planes[p], at, 1, stride, count, mb);
        }
    }
    if !skip_lf {
        for p in 0..3 {
            let (at, stride, count) = place(t, p);
            let inner: &[usize] = if p == 0 { &[4, 8, 12] } else { &[4] };
            for &k in inner {
                block_edge(t.planes[p], at + k, 1, stride, count, b);
            }
        }
    }
    if mb_row > 0 {
        for p in 0..3 {
            let (at, stride, count) = place(t, p);
            mb_edge(t.planes[p], at, stride, 1, count, mb);
        }
    }
    if !skip_lf {
        for p in 0..3 {
            let (at, stride, count) = place(t, p);
            let inner: &[usize] = if p == 0 { &[4, 8, 12] } else { &[4] };
            for &k in inner {
                block_edge(t.planes[p], at + k * stride, stride, 1, count, b);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thresholds_follow_level_and_sharpness() {
        let lfi = LoopFilterInfo::new(32, 0, &Adjustments::default());
        // Sharpness 0: the interior limit is the level itself (at least 1).
        assert_eq!((lfi.lim[32], lfi.blim[32], lfi.mblim[32]), (32, 96, 100));
        assert_eq!(lfi.lim[0], 1);
        let sharp = LoopFilterInfo::new(32, 5, &Adjustments::default());
        // Sharpness 5: the level is quartered, then held to 9 - 5.
        assert_eq!(sharp.lim[32], 4);
        assert_eq!(sharp.lim[8], 2);
        assert_eq!((lfi.hev_thr[0][40], lfi.hev_thr[1][40]), (2, 3));
        assert_eq!((lfi.hev_thr[0][15], lfi.hev_thr[1][15]), (1, 1));
        assert_eq!((lfi.hev_thr[0][14], lfi.hev_thr[1][14]), (0, 0));
    }

    #[test]
    fn levels_take_segment_reference_and_mode_adjustments() {
        let adj = Adjustments {
            segments: Some((false, [0, 10, -40, 63])),
            deltas: Some(([2, -3, 4, 5], [-6, 7, 8, 9])),
        };
        let lfi = LoopFilterInfo::new(30, 0, &adj);
        // Segment 0, intra, 4x4-predicted: 30 + 2 - 6.
        assert_eq!(lfi.lvl[0][0][0], 26);
        // Segment 1, intra, whole-block: 40 + 2.
        assert_eq!(lfi.lvl[1][0][1], 42);
        // Segment 2 clamps to 0 before the deltas: 0 - 3 + 7 = 4.
        assert_eq!(lfi.lvl[2][1][1], 4);
        // Segment 3 clamps to 63, and so does the sum.
        assert_eq!(lfi.lvl[3][3][3], 63);
        let flat = LoopFilterInfo::new(30, 0, &Adjustments::default());
        assert_eq!(flat.lvl, [[[30; 4]; 4]; 4]);
    }

    /// Eight rows of `row`, each a position along a vertical edge at 4.
    fn rows(row: [u8; 8]) -> Vec<u8> {
        row.repeat(8)
    }

    #[test]
    fn a_step_is_smoothed_and_a_cliff_is_left_alone() {
        // A small step across the edge is filtered; a large one (a real
        // edge in the picture) is not.
        let t = Thresholds {
            edge: 20,
            interior: 10,
            hev: 3,
        };
        let mut data = rows([100, 100, 100, 100, 104, 104, 104, 104]);
        block_edge(&mut data, 4, 1, 8, 1, t);
        for row in data.chunks(8) {
            assert_eq!(row, [100, 100, 101, 101, 102, 103, 104, 104]);
        }
        let cliff = rows([10, 10, 10, 10, 200, 200, 200, 200]);
        let mut data = cliff.clone();
        block_edge(&mut data, 4, 1, 8, 1, t);
        assert_eq!(data, cliff);
    }

    #[test]
    fn the_macroblock_filter_moves_three_pixels_each_side() {
        let t = Thresholds {
            edge: 40,
            interior: 10,
            hev: 20,
        };
        let mut data = rows([100, 100, 100, 100, 110, 110, 110, 110]);
        mb_edge(&mut data, 4, 1, 8, 1, t);
        // 27/128, 18/128 and 9/128 of a difference of 20, rounded.
        for row in data.chunks(8) {
            assert_eq!(row, [100, 101, 103, 104, 106, 107, 109, 110]);
        }
    }

    #[test]
    fn a_horizontal_edge_filters_down_columns() {
        // The same step, across a horizontal edge: positions along it are
        // columns, one byte apart, and the pixels across it rows apart.
        let t = Thresholds {
            edge: 20,
            interior: 10,
            hev: 3,
        };
        let mut data = Vec::new();
        for v in [100u8, 100, 100, 100, 104, 104, 104, 104] {
            data.extend([v; 8]);
        }
        block_edge(&mut data, 4 * 8, 8, 1, 1, t);
        let column: Vec<u8> = data.chunks(8).map(|r| r[0]).collect();
        assert_eq!(column, [100, 100, 101, 101, 102, 103, 104, 104]);
        assert!(data.chunks(8).all(|r| r.iter().all(|&p| p == r[0])));
    }
}
