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

use crate::frame::Frame;
use crate::modes::{B_PRED, ModeGrid, SPLITMV};

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

/// Clamp to a signed byte: libvpx's `vp8_signed_char_clamp`.
#[inline]
fn sclamp(t: i32) -> i8 {
    t.clamp(-128, 127) as i8
}

/// A pixel as a signed byte, about 0: libvpx's `(signed char)p ^ 0x80`.
#[inline]
fn s(p: u8) -> i8 {
    (p ^ 0x80) as i8
}

/// A signed byte back to a pixel.
#[inline]
fn u(v: i8) -> u8 {
    (v as u8) ^ 0x80
}

/// Whether to filter across an edge at all: libvpx's `vp8_filter_mask`, as
/// a bool.
#[inline]
#[allow(clippy::too_many_arguments, reason = "the eight pixels across an edge")]
fn filter_mask(
    limit: u8,
    blimit: u8,
    p3: u8,
    p2: u8,
    p1: u8,
    p0: u8,
    q0: u8,
    q1: u8,
    q2: u8,
    q3: u8,
) -> bool {
    let limit = i32::from(limit);
    let d = |a: u8, b: u8| (i32::from(a) - i32::from(b)).abs();
    d(p3, p2) <= limit
        && d(p2, p1) <= limit
        && d(p1, p0) <= limit
        && d(q1, q0) <= limit
        && d(q2, q1) <= limit
        && d(q3, q2) <= limit
        && d(p0, q0) * 2 + d(p1, q1) / 2 <= i32::from(blimit)
}

/// Whether the edge has high variance on either side: libvpx's
/// `vp8_hevmask`, as a bool.
#[inline]
fn hev(thresh: u8, p1: u8, p0: u8, q0: u8, q1: u8) -> bool {
    let t = i32::from(thresh);
    (i32::from(p1) - i32::from(p0)).abs() > t || (i32::from(q1) - i32::from(q0)).abs() > t
}

/// The block filter across one edge, at pixels `px[0..4]` = p1, p0, q0, q1:
/// libvpx's `vp8_filter`.
#[inline]
fn filter(hev: bool, px: [u8; 4]) -> [u8; 4] {
    let [ps1, ps0, qs0, qs1] = px.map(s);
    let mut f = if hev {
        sclamp(i32::from(ps1) - i32::from(qs1))
    } else {
        0
    };
    f = sclamp(i32::from(f) + 3 * (i32::from(qs0) - i32::from(ps0)));
    // Round one side +4 and the other +3, so that a value of 4 does not
    // overshoot.
    let filter1 = sclamp(i32::from(f) + 4) >> 3;
    let filter2 = sclamp(i32::from(f) + 3) >> 3;
    let q0 = u(sclamp(i32::from(qs0) - i32::from(filter1)));
    let p0 = u(sclamp(i32::from(ps0) + i32::from(filter2)));
    // The outer taps, only where the edge is not high variance.
    let a = if hev { 0 } else { (filter1 + 1) >> 1 };
    let q1 = u(sclamp(i32::from(qs1) - i32::from(a)));
    let p1 = u(sclamp(i32::from(ps1) + i32::from(a)));
    [p1, p0, q0, q1]
}

/// The macroblock filter across one edge, at pixels `px[0..6]` = p2, p1,
/// p0, q0, q1, q2: libvpx's `vp8_mbfilter`.
#[inline]
fn mbfilter(hev: bool, px: [u8; 6]) -> [u8; 6] {
    let [ps2, ps1, ps0, qs0, qs1, qs2] = px.map(s);
    let mut f = sclamp(i32::from(ps1) - i32::from(qs1));
    f = sclamp(i32::from(f) + 3 * (i32::from(qs0) - i32::from(ps0)));
    let (mut qs0, mut ps0) = (qs0, ps0);
    if hev {
        // High variance: only the two pixels at the edge move.
        let filter1 = sclamp(i32::from(f) + 4) >> 3;
        let filter2 = sclamp(i32::from(f) + 3) >> 3;
        qs0 = sclamp(i32::from(qs0) - i32::from(filter1));
        ps0 = sclamp(i32::from(ps0) + i32::from(filter2));
        return [px[0], px[1], u(ps0), u(qs0), px[4], px[5]];
    }
    // Otherwise libvpx's first step moves nothing (its filter is masked to
    // 0), and the wide filter moves three pixels each side by 27/128,
    // 18/128 and 9/128 of the difference.
    let f = i32::from(f);
    let w = |k: i32| sclamp((63 + f * k) >> 7);
    let (u27, u18, u9) = (w(27), w(18), w(9));
    [
        u(sclamp(i32::from(ps2) + i32::from(u9))),
        u(sclamp(i32::from(ps1) + i32::from(u18))),
        u(sclamp(i32::from(ps0) + i32::from(u27))),
        u(sclamp(i32::from(qs0) - i32::from(u27))),
        u(sclamp(i32::from(qs1) - i32::from(u18))),
        u(sclamp(i32::from(qs2) - i32::from(u9))),
    ]
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
    for i in 0..count * 8 {
        let c = at + i * along;
        let px = |k: usize| data[c + k * across - 4 * across];
        let (p3, p2, p1, p0, q0, q1, q2, q3) =
            (px(0), px(1), px(2), px(3), px(4), px(5), px(6), px(7));
        if !filter_mask(t.interior, t.edge, p3, p2, p1, p0, q0, q1, q2, q3) {
            continue;
        }
        let out = filter(hev(t.hev, p1, p0, q0, q1), [p1, p0, q0, q1]);
        for (k, v) in out.into_iter().enumerate() {
            data[c + (k + 2) * across - 4 * across] = v;
        }
    }
}

/// [`block_edge`] with the macroblock filter: libvpx's
/// `mbloop_filter_horizontal_edge_c` and `_vertical_edge_c`.
fn mb_edge(data: &mut [u8], at: usize, across: usize, along: usize, count: usize, t: Thresholds) {
    for i in 0..count * 8 {
        let c = at + i * along;
        let px = |k: usize| data[c + k * across - 4 * across];
        let (p3, p2, p1, p0, q0, q1, q2, q3) =
            (px(0), px(1), px(2), px(3), px(4), px(5), px(6), px(7));
        if !filter_mask(t.interior, t.edge, p3, p2, p1, p0, q0, q1, q2, q3) {
            continue;
        }
        let out = mbfilter(hev(t.hev, p1, p0, q0, q1), [p2, p1, p0, q0, q1, q2]);
        for (k, v) in out.into_iter().enumerate() {
            data[c + (k + 1) * across - 4 * across] = v;
        }
    }
}

/// The simple filter across 16 positions of an edge: libvpx's
/// `vp8_loop_filter_simple_horizontal_edge_c` and `_vertical_edge_c`.
fn simple_edge(data: &mut [u8], at: usize, across: usize, along: usize, blimit: u8) {
    for i in 0..16 {
        let c = at + i * along;
        let (p1, p0, q0, q1) = (
            data[c - 2 * across],
            data[c - across],
            data[c],
            data[c + across],
        );
        let d = |a: u8, b: u8| (i32::from(a) - i32::from(b)).abs();
        if d(p0, q0) * 2 + d(p1, q1) / 2 > i32::from(blimit) {
            continue;
        }
        // libvpx's `vp8_simple_filter`.
        let (ps1, ps0, qs0, qs1) = (s(p1), s(p0), s(q0), s(q1));
        let mut f = sclamp(i32::from(ps1) - i32::from(qs1));
        f = sclamp(i32::from(f) + 3 * (i32::from(qs0) - i32::from(ps0)));
        let filter1 = sclamp(i32::from(f) + 4) >> 3;
        data[c] = u(sclamp(i32::from(qs0) - i32::from(filter1)));
        let filter2 = sclamp(i32::from(f) + 3) >> 3;
        data[c - across] = u(sclamp(i32::from(ps0) + i32::from(filter2)));
    }
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
    let frame_type = usize::from(!key_frame);
    for mb_col in 0..grid.mb_cols {
        let mi = grid.get(mb_row, mb_col);
        let skip_lf = mi.mode != B_PRED && mi.mode != SPLITMV && mi.mb_skip_coeff;
        let mode_index = usize::from(MODE_LF_LUT[usize::from(mi.mode.min(9))]);
        let level =
            lfi.lvl[usize::from(mi.segment_id & 3)][usize::from(mi.ref_frame & 3)][mode_index];
        if level == 0 {
            continue;
        }
        let level = usize::from(level);
        if simple {
            let y = &mut frame.planes[0];
            let (at, stride) = (y.at(mb_col * 16, mb_row * 16), y.stride);
            let (mblim, blim) = (lfi.mblim[level], lfi.blim[level]);
            if mb_col > 0 {
                simple_edge(&mut y.data, at, 1, stride, mblim);
            }
            if !skip_lf {
                for k in [4, 8, 12] {
                    simple_edge(&mut y.data, at + k, 1, stride, blim);
                }
            }
            if mb_row > 0 {
                simple_edge(&mut y.data, at, stride, 1, mblim);
            }
            if !skip_lf {
                for k in [4, 8, 12] {
                    simple_edge(&mut y.data, at + k * stride, stride, 1, blim);
                }
            }
            continue;
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
        // libvpx's order: the left edge, the inner vertical edges, the top
        // edge, the inner horizontal edges; each over luma then chroma.
        if mb_col > 0 {
            for (p, plane) in frame.planes.iter_mut().enumerate() {
                let (size, count) = if p == 0 { (16, 2) } else { (8, 1) };
                let (at, stride) = (plane.at(mb_col * size, mb_row * size), plane.stride);
                mb_edge(&mut plane.data, at, 1, stride, count, mb);
            }
        }
        if !skip_lf {
            for (p, plane) in frame.planes.iter_mut().enumerate() {
                let (size, count) = if p == 0 { (16, 2) } else { (8, 1) };
                let (at, stride) = (plane.at(mb_col * size, mb_row * size), plane.stride);
                let inner: &[usize] = if p == 0 { &[4, 8, 12] } else { &[4] };
                for &k in inner {
                    block_edge(&mut plane.data, at + k, 1, stride, count, b);
                }
            }
        }
        if mb_row > 0 {
            for (p, plane) in frame.planes.iter_mut().enumerate() {
                let (size, count) = if p == 0 { (16, 2) } else { (8, 1) };
                let (at, stride) = (plane.at(mb_col * size, mb_row * size), plane.stride);
                mb_edge(&mut plane.data, at, stride, 1, count, mb);
            }
        }
        if !skip_lf {
            for (p, plane) in frame.planes.iter_mut().enumerate() {
                let (size, count) = if p == 0 { (16, 2) } else { (8, 1) };
                let (at, stride) = (plane.at(mb_col * size, mb_row * size), plane.stride);
                let inner: &[usize] = if p == 0 { &[4, 8, 12] } else { &[4] };
                for &k in inner {
                    block_edge(&mut plane.data, at + k * stride, stride, 1, count, b);
                }
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
