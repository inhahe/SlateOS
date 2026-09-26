//! VP8's loop filter (RFC 6386 §15), as libwebp computes it.
//!
//! Block-based coding leaves seams at block edges; the loop filter smooths
//! them where the step across an edge is small enough to be an artefact
//! rather than a real edge in the picture. Each macroblock filters its left
//! edge, then its interior vertical edges, then its top edge, then its
//! interior horizontal edges, in raster order over the frame -- so a
//! macroblock's filtering reads samples its left and upper neighbours have
//! already filtered, and modifies up to three samples on their side.
//!
//! Two filters exist. The simple one touches luma only, and one sample either
//! side of an edge; the normal one touches luma and chroma, up to three
//! samples either side at macroblock edges and two at interior ones, and
//! backs off to the simple adjustment where the edge has "high variance".
//!
//! The arithmetic here is libwebp's (`src/dsp/dec.c`), which is written to
//! equal the RFC's reference code exactly -- the RFC clamps an intermediate to
//! a signed byte where libwebp clamps the result to the range the next step
//! can use, and the two agree on every input; the tests hold them to it.

/// The strength of the filter for one macroblock (libwebp's `VP8FInfo`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Strength {
    /// `2 * level + interior`, the interior-edge limit: zero means the
    /// macroblock is not filtered at all. Macroblock edges use this plus 4.
    pub limit: u8,
    /// The limit on differences between samples on the same side of an edge.
    pub interior: u8,
    /// Above this, a side of the edge has high variance.
    pub hev_threshold: u8,
}

impl Strength {
    /// The filter for a macroblock whose filter level is `level` (0..=63),
    /// in a frame of the given sharpness (RFC 6386 §15.2, §15.4; libwebp's
    /// `PrecomputeFilterStrengths` for a key frame).
    #[allow(
        clippy::arithmetic_side_effects,
        clippy::cast_possible_truncation,
        reason = "level is clamped to 0..=63 and sharpness is at most 7, so every value fits in a byte: at most 2 * 63 + 63"
    )]
    pub(super) fn new(level: i32, sharpness: u8) -> Self {
        let level = level.clamp(0, 63);
        if level == 0 {
            return Self::default();
        }
        let mut interior = level;
        if sharpness > 0 {
            interior >>= if sharpness > 4 { 2 } else { 1 };
            interior = interior.min(9 - i32::from(sharpness));
        }
        let interior = interior.max(1);
        Self {
            limit: (2 * level + interior) as u8,
            interior: interior as u8,
            hev_threshold: if level >= 40 {
                2
            } else if level >= 15 {
                1
            } else {
                0
            },
        }
    }
}

/// Which filter a frame uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    /// Luma only, one sample either side.
    Simple,
    /// Luma and chroma, with the high-variance test.
    Normal,
}

/// Up to sixteen segments' eight taps, a row a tap: `[p3, p2, p1, p0, q0, q1,
/// q2, q3][lane]`, lanes past the edge's segments zero. Filtering a whole
/// edge's segments side by side, the same arithmetic in every lane and the
/// choice between filters a select rather than a branch, lets the compiler
/// work on several at once; each lane is exactly one segment's filter.
type Lanes = [[i16; 16]; 8];

/// The taps of `count` segments starting at `at`, `along` apart, each
/// crossing the edge in steps of `across`.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "count is at most 16 and the segments stay inside the plane, four samples clear of its start"
)]
fn load(buf: &[u8], at: usize, across: usize, along: usize, count: usize) -> Lanes {
    let mut taps = [[0i16; 16]; 8];
    let first = at.saturating_sub(4 * across);
    if across == 1 {
        // An edge between columns: each segment is eight samples in a row.
        for lane in 0..count.min(16) {
            let start = first + lane * along;
            if let Some(row) = buf.get(start..start + 8) {
                for (tap, &sample) in taps.iter_mut().zip(row) {
                    if let Some(slot) = tap.get_mut(lane) {
                        *slot = i16::from(sample);
                    }
                }
            }
        }
    } else {
        // An edge between rows: each tap is a run of the segments' samples.
        for (k, tap) in taps.iter_mut().enumerate() {
            let start = first + k * across;
            if let Some(run) = buf.get(start..start + count.min(16) * along) {
                for (slot, &sample) in tap.iter_mut().zip(run.iter().step_by(along.max(1))) {
                    *slot = i16::from(sample);
                }
            }
        }
    }
    taps
}

/// Write taps p2 to q2 (the only ones a filter changes) of `count` segments
/// back, as [`load`] read them.
#[allow(
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "count is at most 16 and the segments stay inside the plane; every tap is a sample, 0..=255"
)]
fn store(buf: &mut [u8], at: usize, across: usize, along: usize, count: usize, taps: &Lanes) {
    let first = at.saturating_sub(4 * across);
    if across == 1 {
        for lane in 0..count.min(16) {
            let start = first + lane * along;
            if let Some(row) = buf.get_mut(start..start + 8) {
                for (slot, tap) in row.iter_mut().zip(taps.iter()).skip(1).take(6) {
                    if let Some(&value) = tap.get(lane) {
                        *slot = value as u8;
                    }
                }
            }
        }
    } else {
        for (k, tap) in taps.iter().enumerate().skip(1).take(6) {
            let start = first + k * across;
            if let Some(run) = buf.get_mut(start..start + count.min(16) * along) {
                for (slot, &value) in run.iter_mut().step_by(along.max(1)).zip(tap) {
                    *slot = value as u8;
                }
            }
        }
    }
}

/// `v` clamped to a signed byte.
fn sclip1(v: i16) -> i16 {
    v.clamp(-128, 127)
}

/// `v` clamped to the range a filter tap can move a sample: -16..=15.
fn sclip2(v: i16) -> i16 {
    v.clamp(-16, 15)
}

/// `v` clamped to a sample, as a lane holds it.
fn clamp8(v: i16) -> i16 {
    v.clamp(0, 255)
}

/// The simple filter on every lane: where the step across the edge is small
/// enough to be an artefact -- `2 * |p0 - q0| + |p1 - q1| / 2 <= limit`,
/// which libwebp writes as `4 * |p0 - q0| + |p1 - q1| <= 2 * limit + 1` --
/// move p0 and q0 toward each other (libwebp's `DoFilter2`, the RFC's
/// `common_adjust` with the outer taps).
#[allow(clippy::arithmetic_side_effects, reason = "samples are bytes")]
fn simple_lanes(taps: &mut Lanes, limit: i16) {
    let [_, _, p1, p0, q0, q1, _, _] = taps;
    for (((p1, p0), q0), q1) in p1
        .iter()
        .zip(p0.iter_mut())
        .zip(q0.iter_mut())
        .zip(q1.iter())
    {
        let (vp1, vp0, vq0, vq1) = (*p1, *p0, *q0, *q1);
        let mask = 4 * (vp0 - vq0).abs() + (vp1 - vq1).abs() <= 2 * limit + 1;
        let a = 3 * (vq0 - vp0) + sclip1(vp1 - vq1);
        let a1 = sclip2((a + 4) >> 3);
        let a2 = sclip2((a + 3) >> 3);
        *p0 = if mask { clamp8(vp0 + a2) } else { vp0 };
        *q0 = if mask { clamp8(vq0 - a1) } else { vq0 };
    }
}

/// The normal filter on every lane, where the simple filter's test passes and
/// no step on either side of the edge exceeds `interior`: at a macroblock
/// edge (`MB`) three samples either side move by 3/7, 2/7 and 1/7 of the step
/// (libwebp's `DoFilter6`), at an interior one p0 and q0 as `DoFilter2`
/// without the outer taps and p1 and q1 by half as much (`DoFilter4`) --
/// except where the edge has high variance (a step beside it above
/// `hev_threshold`), which gets the simple filter's `DoFilter2`.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "samples are bytes; a is at most 3 * 255 + 127"
)]
fn normal_lanes<const MB: bool>(taps: &mut Lanes, limit: i16, interior: i16, hev_threshold: i16) {
    let [p3, p2, p1, p0, q0, q1, q2, q3] = taps;
    let lanes = p3
        .iter()
        .zip(p2.iter_mut())
        .zip(p1.iter_mut())
        .zip(p0.iter_mut())
        .zip(q0.iter_mut())
        .zip(q1.iter_mut())
        .zip(q2.iter_mut())
        .zip(q3.iter());
    for (((((((p3, p2), p1), p0), q0), q1), q2), q3) in lanes {
        let (vp3, vp2, vp1, vp0) = (*p3, *p2, *p1, *p0);
        let (vq0, vq1, vq2, vq3) = (*q0, *q1, *q2, *q3);
        let mask = 4 * (vp0 - vq0).abs() + (vp1 - vq1).abs() <= 2 * limit + 1
            && (vp3 - vp2).abs() <= interior
            && (vp2 - vp1).abs() <= interior
            && (vp1 - vp0).abs() <= interior
            && (vq3 - vq2).abs() <= interior
            && (vq2 - vq1).abs() <= interior
            && (vq1 - vq0).abs() <= interior;
        let hev = (vp1 - vp0).abs() > hev_threshold || (vq1 - vq0).abs() > hev_threshold;
        // `filter2`.
        let a = 3 * (vq0 - vp0) + sclip1(vp1 - vq1);
        let (f2p0, f2q0) = (
            clamp8(vp0 + sclip2((a + 3) >> 3)),
            clamp8(vq0 - sclip2((a + 4) >> 3)),
        );
        // `filter6`, or `filter4`.
        let [np2, np1, np0, nq0, nq1, nq2] = if MB {
            let w = sclip1(3 * (vq0 - vp0) + sclip1(vp1 - vq1));
            let a1 = (27 * w + 63) >> 7;
            let a2 = (18 * w + 63) >> 7;
            let a3 = (9 * w + 63) >> 7;
            [
                clamp8(vp2 + a3),
                clamp8(vp1 + a2),
                clamp8(vp0 + a1),
                clamp8(vq0 - a1),
                clamp8(vq1 - a2),
                clamp8(vq2 - a3),
            ]
        } else {
            let a = 3 * (vq0 - vp0);
            let a1 = sclip2((a + 4) >> 3);
            let a2 = sclip2((a + 3) >> 3);
            let a3 = (a1 + 1) >> 1;
            [
                vp2,
                clamp8(vp1 + a3),
                clamp8(vp0 + a2),
                clamp8(vq0 - a1),
                clamp8(vq1 - a3),
                vq2,
            ]
        };
        let full = mask && !hev;
        let two = mask && hev;
        *p2 = if full { np2 } else { vp2 };
        *p1 = if full { np1 } else { vp1 };
        *p0 = if full {
            np0
        } else if two {
            f2p0
        } else {
            vp0
        };
        *q0 = if full {
            nq0
        } else if two {
            f2q0
        } else {
            vq0
        };
        *q1 = if full { nq1 } else { vq1 };
        *q2 = if full { nq2 } else { vq2 };
    }
}

/// Filter `count` segments across one edge with the simple filter: the
/// segments start at `at`, `along` apart, and cross the edge in steps of
/// `across`.
fn simple_edge(buf: &mut [u8], at: usize, across: usize, along: usize, count: usize, limit: i32) {
    let mut taps = load(buf, at, across, along, count);
    // A limit is at most 2 * 63 + 63 + 4.
    simple_lanes(&mut taps, i16::try_from(limit).unwrap_or(i16::MAX));
    store(buf, at, across, along, count, &taps);
}

/// Filter `count` segments across a macroblock edge with the normal filter.
fn macroblock_edge(
    buf: &mut [u8],
    at: usize,
    across: usize,
    along: usize,
    count: usize,
    strength: Strength,
) {
    let mut taps = load(buf, at, across, along, count);
    normal_lanes::<true>(
        &mut taps,
        i16::from(strength.limit).saturating_add(4),
        i16::from(strength.interior),
        i16::from(strength.hev_threshold),
    );
    store(buf, at, across, along, count, &taps);
}

/// Filter `count` segments across an interior (subblock) edge with the
/// normal filter.
fn subblock_edge(
    buf: &mut [u8],
    at: usize,
    across: usize,
    along: usize,
    count: usize,
    strength: Strength,
) {
    let mut taps = load(buf, at, across, along, count);
    normal_lanes::<false>(
        &mut taps,
        i16::from(strength.limit),
        i16::from(strength.interior),
        i16::from(strength.hev_threshold),
    );
    store(buf, at, across, along, count, &taps);
}

/// One plane of a frame being filtered: its samples and its row stride.
pub(super) struct Plane<'a> {
    pub samples: &'a mut [u8],
    pub stride: usize,
}

/// What filtering one macroblock needs to know about it.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Macroblock {
    pub strength: Strength,
    /// Whether its interior edges are filtered too: always for a macroblock
    /// predicted by subblocks, otherwise only if it has coefficients.
    pub inner: bool,
}

/// Filter the macroblock at (`mx`, `my`) (RFC 6386 §15.1; libwebp's
/// `DoFilter`): left edge, interior vertical edges, top edge, interior
/// horizontal edges; luma, and for the normal filter chroma too, each block
/// `size` samples (16, or 8 for chroma) at `n`-sample spacing.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "the macroblock is inside the planes, which hold whole macroblocks"
)]
pub(super) fn filter_macroblock(
    kind: Kind,
    luma: &mut Plane<'_>,
    chroma: [&mut Plane<'_>; 2],
    (mx, my): (usize, usize),
    mb: Macroblock,
) {
    let strength = mb.strength;
    if strength.limit == 0 {
        return;
    }
    let ys = luma.stride;
    let y0 = my * 16 * ys + mx * 16;
    match kind {
        Kind::Simple => {
            let limit = i32::from(strength.limit);
            let buf = &mut *luma.samples;
            if mx > 0 {
                simple_edge(buf, y0, 1, ys, 16, limit + 4);
            }
            if mb.inner {
                for x in [4, 8, 12] {
                    simple_edge(buf, y0 + x, 1, ys, 16, limit);
                }
            }
            if my > 0 {
                simple_edge(buf, y0, ys, 1, 16, limit + 4);
            }
            if mb.inner {
                for y in [4, 8, 12] {
                    simple_edge(buf, y0 + y * ys, ys, 1, 16, limit);
                }
            }
        }
        Kind::Normal => {
            let [u, v] = chroma;
            let cs = u.stride;
            let c0 = my * 8 * cs + mx * 8;
            let buf = &mut *luma.samples;
            if mx > 0 {
                macroblock_edge(buf, y0, 1, ys, 16, strength);
                macroblock_edge(u.samples, c0, 1, cs, 8, strength);
                macroblock_edge(v.samples, c0, 1, cs, 8, strength);
            }
            if mb.inner {
                for x in [4, 8, 12] {
                    subblock_edge(buf, y0 + x, 1, ys, 16, strength);
                }
                subblock_edge(u.samples, c0 + 4, 1, cs, 8, strength);
                subblock_edge(v.samples, c0 + 4, 1, cs, 8, strength);
            }
            if my > 0 {
                macroblock_edge(buf, y0, ys, 1, 16, strength);
                macroblock_edge(u.samples, c0, cs, 1, 8, strength);
                macroblock_edge(v.samples, c0, cs, 1, 8, strength);
            }
            if mb.inner {
                for y in [4, 8, 12] {
                    subblock_edge(buf, y0 + y * ys, ys, 1, 16, strength);
                }
                subblock_edge(u.samples, c0 + 4 * cs, cs, 1, 8, strength);
                subblock_edge(v.samples, c0 + 4 * cs, cs, 1, 8, strength);
            }
        }
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
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap
)]
mod tests {
    use super::*;

    // The RFC's reference filters (dixie_loopfilter.c, §20.6), transcribed
    // on an 8-sample segment, to hold libwebp's arithmetic to them.

    fn saturate_int8(x: i32) -> i32 {
        x.clamp(-128, 127)
    }

    fn saturate_uint8(x: i32) -> u8 {
        x.clamp(0, 255) as u8
    }

    fn simple_threshold(s: &[u8; 8], filter_limit: i32) -> bool {
        let (p1, p0, q0, q1) = (
            i32::from(s[2]),
            i32::from(s[3]),
            i32::from(s[4]),
            i32::from(s[5]),
        );
        ((p0 - q0).abs() * 2 + ((p1 - q1).abs() >> 1)) <= filter_limit
    }

    fn normal_threshold(s: &[u8; 8], edge_limit: i32, interior: i32) -> bool {
        let v = s.map(i32::from);
        simple_threshold(s, 2 * edge_limit + interior)
            && (v[0] - v[1]).abs() <= interior
            && (v[1] - v[2]).abs() <= interior
            && (v[2] - v[3]).abs() <= interior
            && (v[7] - v[6]).abs() <= interior
            && (v[6] - v[5]).abs() <= interior
            && (v[5] - v[4]).abs() <= interior
    }

    fn high_edge_variance(s: &[u8; 8], threshold: i32) -> bool {
        let v = s.map(i32::from);
        (v[2] - v[3]).abs() > threshold || (v[5] - v[4]).abs() > threshold
    }

    fn filter_common(s: &mut [u8; 8], use_outer_taps: bool) {
        let v = s.map(i32::from);
        let (p1, p0, q0, q1) = (v[2], v[3], v[4], v[5]);
        let mut a = 3 * (q0 - p0);
        if use_outer_taps {
            a += saturate_int8(p1 - q1);
        }
        a = saturate_int8(a);
        let f1 = (if a + 4 > 127 { 127 } else { a + 4 }) >> 3;
        let f2 = (if a + 3 > 127 { 127 } else { a + 3 }) >> 3;
        s[3] = saturate_uint8(p0 + f2);
        s[4] = saturate_uint8(q0 - f1);
        if !use_outer_taps {
            let a = (f1 + 1) >> 1;
            s[2] = saturate_uint8(p1 + a);
            s[5] = saturate_uint8(q1 - a);
        }
    }

    fn filter_mb_edge(s: &mut [u8; 8]) {
        let v = s.map(i32::from);
        let w = saturate_int8(saturate_int8(v[2] - v[5]) + 3 * (v[4] - v[3]));
        let a = (27 * w + 63) >> 7;
        s[3] = saturate_uint8(v[3] + a);
        s[4] = saturate_uint8(v[4] - a);
        let a = (18 * w + 63) >> 7;
        s[2] = saturate_uint8(v[2] + a);
        s[5] = saturate_uint8(v[5] - a);
        let a = (9 * w + 63) >> 7;
        s[1] = saturate_uint8(v[1] + a);
        s[6] = saturate_uint8(v[6] - a);
    }

    /// Segments that sit near every threshold: small steps with noise, big
    /// steps, flat runs, and the extremes.
    fn segments() -> impl Iterator<Item = [u8; 8]> {
        let mut state = 5u32;
        (0..40_000u32).map(move |k| {
            let mut next = || {
                state = state.wrapping_mul(1_103_515_245).wrapping_add(12345);
                (state >> 16) as i32
            };
            let base = next() % 256;
            let step = match k % 4 {
                0 => next() % 7 - 3,
                1 => next() % 41 - 20,
                2 => next() % 511 - 255,
                _ => 0,
            };
            let noise = 1 + k as i32 % 9;
            let mut s = [0u8; 8];
            for (i, slot) in s.iter_mut().enumerate() {
                let side = if i < 4 { 0 } else { step };
                *slot = (base + side + next() % noise - noise / 2).clamp(0, 255) as u8;
            }
            s
        })
    }

    #[test]
    fn the_simple_filter_is_the_rfcs() {
        for level in [1, 5, 20, 40, 63] {
            for sharpness in [0u8, 3, 7] {
                let strength = Strength::new(level, sharpness);
                for edge_limit in [i32::from(strength.limit), i32::from(strength.limit) + 4] {
                    for s in segments().take(5000) {
                        let mut ours = s;
                        simple_edge(&mut ours, 4, 1, 8, 1, edge_limit);
                        let mut rfc = s;
                        if simple_threshold(&rfc, edge_limit) {
                            filter_common(&mut rfc, true);
                        }
                        assert_eq!(ours, rfc, "{s:?} limit {edge_limit}");
                    }
                }
            }
        }
    }

    #[test]
    fn the_normal_filter_is_the_rfcs() {
        for level in [1, 9, 15, 30, 40, 63] {
            for sharpness in [0u8, 2, 5, 7] {
                let strength = Strength::new(level, sharpness);
                // The RFC's edge limits are the level (+ 2 at macroblock
                // edges); libwebp folds them with the interior limit.
                let (interior, hev) = (
                    i32::from(strength.interior),
                    i32::from(strength.hev_threshold),
                );
                for s in segments().take(4000) {
                    let mut ours = s;
                    macroblock_edge(&mut ours, 4, 1, 8, 1, strength);
                    let mut rfc = s;
                    if normal_threshold(&rfc, level + 2, interior) {
                        if high_edge_variance(&rfc, hev) {
                            filter_common(&mut rfc, true);
                        } else {
                            filter_mb_edge(&mut rfc);
                        }
                    }
                    assert_eq!(
                        ours, rfc,
                        "mb edge {s:?} level {level} sharpness {sharpness}"
                    );

                    let mut ours = s;
                    subblock_edge(&mut ours, 4, 1, 8, 1, strength);
                    let mut rfc = s;
                    if normal_threshold(&rfc, level, interior) {
                        let hv = high_edge_variance(&rfc, hev);
                        filter_common(&mut rfc, hv);
                    }
                    assert_eq!(
                        ours, rfc,
                        "subblock edge {s:?} level {level} sharpness {sharpness}"
                    );
                }
            }
        }
    }

    #[test]
    fn strengths_follow_the_rfcs_formulas() {
        // Level 0 filters nothing.
        assert_eq!(Strength::new(0, 3).limit, 0);
        // Out-of-range levels are clamped, as libwebp clamps them.
        assert_eq!(Strength::new(80, 0), Strength::new(63, 0));
        assert_eq!(Strength::new(-5, 0).limit, 0);
        // The interior limit: the level, shifted by sharpness and capped at
        // 9 - sharpness, never below 1.
        assert_eq!(Strength::new(40, 0).interior, 40);
        assert_eq!(Strength::new(40, 3).interior, 6);
        assert_eq!(Strength::new(40, 5).interior, 4);
        assert_eq!(Strength::new(2, 7).interior, 1);
        // The high-variance threshold steps at 15 and 40 in a key frame.
        assert_eq!(Strength::new(14, 0).hev_threshold, 0);
        assert_eq!(Strength::new(15, 0).hev_threshold, 1);
        assert_eq!(Strength::new(40, 0).hev_threshold, 2);
        assert_eq!(Strength::new(10, 0).limit, 30);
    }
}
