//! The CJK writing system: FreeType's `afcjk.c` as light hinting uses it --
//! both dimensions, stems never resized -- and the Indic one (`afindic.c`),
//! which is the same without blue zones.
//!
//! FreeType hints Chinese, Japanese and Korean ideographs with this system,
//! and every glyph no script claims: the *fallback style*, which in a Latin
//! font holds its arrows, mathematical signs, box drawing and `.notdef`, and
//! in a font without a Unicode `cmap` every glyph. Unlike the Latin system
//! in light mode it moves points horizontally too: an ideograph is a lattice
//! of strokes both ways, and both ways its stems are nudged onto the grid.
//!
//! Two halves, as in FreeType:
//!
//! * **Metrics**, once per face, style and instance ([`Metrics::new`]): each
//!   dimension's standard stem width, from one reference glyph
//!   (`af_cjk_metrics_init_widths`) -- a fifth of it is how close two
//!   segments must be to join one edge -- and the style's blue zones, for
//!   ideographs the top and bottom of the ink, each from the medians of its
//!   reference glyphs' extremes (`af_cjk_metrics_init_blues`). At each size
//!   ([`Metrics::scale`]) a zone under 3/4 pixel tall is fitted to the grid
//!   (`af_cjk_metrics_scale_dim`); no scale is nudged, as Latin's is.
//! * **One glyph** ([`hint`]), in each dimension: segments found as the Latin
//!   system finds them (`af_cjk_hints_compute_segments`, whose own roundness
//!   rule never runs), paired across stems -- a stroke's wider ends told from
//!   the stem they end (`af_cjk_hints_link_segments`) -- and gathered into
//!   edges; edges in a zone placed on it, each stem
//!   moved by at most 14/64 pixel to put its edges on pixel boundaries, a
//!   stem too close to the one before left where it is to keep the gap
//!   between them, and the rest interpolated (`af_cjk_hint_edges`). Each
//!   edge then carries its points by the distance it moved
//!   (`af_cjk_align_edge_points`), and the points between follow
//!   ([`Hints`]'s strong and weak passes).
//!
//! Only light mode's paths are ported: there FreeType's stem-width
//! computation changes nothing, its snapping is off, and its monochrome
//! rules do not apply.
//!
//! Portions of this file are copyright (C) 2006-2023 by David Turner, Robert
//! Wilhelm and Werner Lemberg, from The FreeType Project (www.freetype.org).
//! Used under the FreeType License: see `gui/font/licenses/FTL.TXT`.

#![allow(
    clippy::arithmetic_side_effects,
    reason = "bounded operands, as in `fixed`: font units within i16 and \
              scales within fixed::MAX_SCALE"
)]

use alloc::vec::Vec;

use super::Blue as BlueString;
use super::fixed::{div_fix, mul_div, mul_fix, pix_floor, pix_round};
use super::glyph::{
    Dim, EDGE_DONE, EDGE_ROUND, EDGE_SERIF, Edge, Hints, font_unit, insertion_point,
};
use super::latin::{self, Source};

/// Blue zone properties, as the tables give them (`AF_BLUE_PROPERTY_CJK_*`).
/// A top zone -- or, horizontally, a right one.
const PROP_TOP: u8 = 1 << 0;
/// A zone across [`Dim::Horz`]: a left or right one.
const PROP_HORIZ: u8 = 1 << 1;

/// Light mode keeps a stem's edges this close to the grid only if moving it
/// no further than [`MAX_DELTA`]: the gaps below are what the two round
/// edges of a stem may leave, in 1/64 pixel (`AF_LIGHT_MODE_MAX_*`).
const MAX_HORZ_GAP: i64 = 9;
const MAX_VERT_GAP: i64 = 15;
/// The furthest light mode moves a stem, 1/64 pixel
/// (`AF_LIGHT_MODE_MAX_DELTA_ABS`).
const MAX_DELTA: i64 = 14;

/// A dimension's slot in the per-dimension arrays: [`Dim::Horz`] first.
const fn slot(dim: Dim) -> usize {
    match dim {
        Dim::Horz => 0,
        Dim::Vert => 1,
    }
}

/// One blue zone as measured, in font units (`AF_CJKBlueRec`, unscaled).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Blue {
    /// The line the letters before the zone string's `|` settle on.
    reference: i64,
    /// The line the letters after it reach.
    shoot: i64,
    /// A top zone, or a right one.
    top: bool,
}

/// One dimension's metrics, in font units (`AF_CJKAxisRec`, unscaled).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Axis {
    /// How close two segments must be to join one edge: a fifth of the
    /// standard stem width.
    edge_distance_threshold: i64,
    blues: Vec<Blue>,
}

/// A style's metrics, in font units (`AF_CJKMetricsRec`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Metrics {
    units_per_em: i64,
    /// [`Dim::Horz`]'s, then [`Dim::Vert`]'s.
    axes: [Axis; 2],
}

/// One zone fitted to a size, 26.6.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Fitted {
    reference: i64,
    shoot: i64,
    /// Under 3/4 pixel tall at this size, and so used.
    active: bool,
}

/// A style's metrics at one size (`AF_CJKMetricsRec` after scaling).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Scaled {
    /// The size's scales, 16.16, which this system never nudges.
    pub(super) x_scale: i64,
    pub(super) y_scale: i64,
    /// Per dimension, as [`Metrics`]' axes.
    blues: [Vec<Fitted>; 2],
}

impl Metrics {
    /// Measure a CJK- or Indic-system style: both dimensions' standard stem
    /// widths from `standard`'s letters, and the zones of `blues` (an Indic
    /// style has none). With `unicode` false -- a face without a Unicode
    /// `cmap`, all of whose glyphs fall to the fallback style -- FreeType
    /// measures nothing and every threshold stays zero; so here. `None` only
    /// for an em the hinter's arithmetic is not proved for.
    pub(super) fn new(
        units_per_em: i64,
        standard: &str,
        blues: &[BlueString],
        unicode: bool,
        source: &Source<'_>,
    ) -> Option<Self> {
        if !(16..=16384).contains(&units_per_em) {
            return None;
        }
        let mut axes = [Axis::default(), Axis::default()];
        if unicode {
            let widths = standard_widths(units_per_em, standard, source);
            for (axis, widths) in axes.iter_mut().zip(&widths) {
                // Without a measured width, 50 units of a 2048 em.
                let standard_width = widths
                    .first()
                    .copied()
                    .unwrap_or_else(|| latin::constant(units_per_em, 50));
                axis.edge_distance_threshold = standard_width / 5;
            }
            measure_blues(blues, source, &mut axes);
        }
        Some(Self { units_per_em, axes })
    }

    /// Fit the zones to a size whose scales are `x_scale` and `y_scale`
    /// (16.16): `af_cjk_metrics_scale_dim` for each dimension.
    pub(super) fn scale(&self, x_scale: i64, y_scale: i64) -> Scaled {
        let fit = |blues: &[Blue], scale: i64| -> Vec<Fitted> {
            blues
                .iter()
                .map(|b| {
                    let reference = mul_fix(b.reference, scale);
                    let shoot = mul_fix(b.shoot, scale);
                    let mut fitted = Fitted {
                        reference,
                        shoot,
                        active: false,
                    };
                    // A zone is used only while under 3/4 pixel tall: its
                    // reference goes to the nearest pixel boundary and its
                    // overshoot to none, or to whole pixels from there.
                    let dist = mul_fix(b.reference - b.shoot, scale);
                    if (-48..=48).contains(&dist) {
                        fitted.reference = pix_round(reference);
                        let delta1 = div_fix(fitted.reference, scale) - b.shoot;
                        let delta2 = mul_fix(delta1.abs(), scale);
                        let delta2 = if delta2 < 32 { 0 } else { pix_round(delta2) };
                        fitted.shoot = fitted.reference - if delta1 < 0 { -delta2 } else { delta2 };
                        fitted.active = true;
                    }
                    fitted
                })
                .collect()
        };
        let [horz, vert] = &self.axes;
        Scaled {
            x_scale,
            y_scale,
            blues: [fit(&horz.blues, x_scale), fit(&vert.blues, y_scale)],
        }
    }
}

/// Each dimension's standard stem widths, from the first of `standard`'s
/// letters the face has: `af_cjk_metrics_init_widths`. A dimension left
/// empty was not measured.
fn standard_widths(units_per_em: i64, standard: &str, source: &Source<'_>) -> [Vec<i64>; 2] {
    let mut widths = [Vec::new(), Vec::new()];
    let Some(mut hints) = latin::standard_glyph_hints(units_per_em, standard, source) else {
        return widths;
    };
    for dim in [Dim::Horz, Dim::Vert] {
        // A failure leaves this dimension, and the ones after it, unmeasured.
        let Some(measured) = latin::stem_widths(&mut hints, dim, units_per_em) else {
            break;
        };
        if let Some(w) = widths.get_mut(slot(dim)) {
            *w = measured;
        }
    }
    widths
}

/// Measure every zone of a style into its dimension's axis:
/// `af_cjk_metrics_init_blues`.
///
/// A zone's string holds two groups of letters, split by `|`: the median of
/// the first group's extremes is its reference, of the second's its
/// overshoot. A letter's extreme is simply its highest point (lowest for a
/// bottom zone; rightmost or leftmost across), control points included.
fn measure_blues(strings: &[BlueString], source: &Source<'_>, axes: &mut [Axis; 2]) {
    for string in strings {
        let across = string.props & PROP_HORIZ != 0;
        let top = string.props & PROP_TOP != 0;
        let (mut fills, mut flats): (Vec<i64>, Vec<i64>) = (Vec::new(), Vec::new());
        let mut fill = true;
        for mut cluster in string.chars.split(' ').filter(|c| !c.is_empty()) {
            // The switch to the second group, however it is spaced.
            while let Some(rest) = cluster.strip_prefix('|') {
                fill = false;
                cluster = rest;
            }
            if cluster.is_empty() {
                continue;
            }
            // A letter that shapes to more than one glyph, or to none the
            // face has, is skipped.
            let gid = match (source.shape)(cluster).as_slice() {
                [(gid, _)] if *gid != 0 => *gid,
                _ => continue,
            };
            let Some(outline) = (source.outline)(gid) else {
                continue;
            };
            // Glyphs that draw nothing, or next to nothing.
            if outline.points.len() <= 2 {
                continue;
            }
            let Some(extreme) = extreme(&outline, source.units, across, top) else {
                continue;
            };
            if fill {
                fills.push(extreme);
            } else {
                flats.push(extreme);
            }
        }
        fills.sort_unstable();
        flats.sort_unstable();
        let median = |v: &[i64]| v.get(v.len() / 2).copied();
        let (mut reference, mut shoot) = match (median(&fills), median(&flats)) {
            (Some(f), None) => (f, f),
            (None, Some(s)) => (s, s),
            (Some(f), Some(s)) => (f, s),
            // Not one of its letters in the face: no zone.
            (None, None) => continue,
        };
        // An overshoot on the wrong side of its reference is a measuring
        // accident: both take the mean, rounded toward zero as C divides.
        if shoot != reference && top != (shoot < reference) {
            let mean = shoot.midpoint(reference);
            reference = mean;
            shoot = mean;
        }
        let axis = if across { &mut axes[0] } else { &mut axes[1] };
        axis.blues.push(Blue {
            reference,
            shoot,
            top,
        });
    }
}

/// The extreme coordinate of one reference glyph -- its highest point for a
/// top zone, lowest for a bottom one, rightmost or leftmost across -- over
/// every contour of more than one point; zero if there is none, as FreeType
/// leaves it. `None` for a glyph with a coordinate beyond 16 bits, which
/// the hinter does not take on.
fn extreme(
    outline: &crate::sfnt::TaggedOutline,
    units: super::glyph::Units,
    across: bool,
    top: bool,
) -> Option<i64> {
    let coords: Vec<i64> = outline
        .points
        .iter()
        .map(|p| font_unit(if across { p.x } else { p.y }, units))
        .collect::<Option<_>>()?;
    let mut best: Option<i64> = None;
    let mut start = 0usize;
    for &end in &outline.ends {
        let (first, last) = (start, end.checked_sub(1)?);
        start = end;
        // One-point contours are never drawn; some fonts use them for mark
        // attachment far outside the glyph.
        if last <= first {
            continue;
        }
        for &v in coords.get(first..=last)? {
            let better = best.is_none_or(|b| if top { v > b } else { v < b });
            if better {
                best = Some(v);
            }
        }
    }
    Some(best.unwrap_or(0))
}

/// Hint one glyph of this style in both dimensions: `af_cjk_hints_apply`.
pub(super) fn hint(hints: &mut Hints, metrics: &Metrics, scaled: &Scaled) -> Option<()> {
    for dim in [Dim::Horz, Dim::Vert] {
        compute_segments(hints, dim)?;
        link_segments(hints, dim);
        compute_edges(hints, metrics, dim)?;
        compute_blue_edges(hints, metrics, scaled, dim);
    }
    for dim in [Dim::Horz, Dim::Vert] {
        hint_edges(hints, dim)?;
        align_edge_points(hints, dim)?;
        hints.align_strong_points(dim)?;
        hints.align_weak_points(dim)?;
    }
    Some(())
}

/// The Latin system's segments, roundness and all:
/// `af_cjk_hints_compute_segments`.
///
/// FreeType's function then means to judge each segment round by this
/// system's own rule -- no two on-curve points in a row -- but its loop runs
/// over the segment count it read *before* the Latin pass filled the table,
/// which the reload has just set to zero; so the rule never runs, and the
/// Latin one stands. Ported as it behaves: Malgun Gothic's stems otherwise
/// land 3/64 pixel from FreeType's.
fn compute_segments(hints: &mut Hints, dim: Dim) -> Option<()> {
    latin::compute_segments(hints, dim)
}

/// Pair each segment with the nearest opposite one across a stem, and sort
/// out a stroke's wider ends: `af_cjk_hints_link_segments`.
fn link_segments(hints: &mut Hints, dim: Dim) {
    let g = hints.view(dim);
    let len_threshold = latin::constant(g.units_per_em, 8);
    // Three pixels, in font units.
    let dist_threshold = div_fix(64 * 3, g.axis.scale);
    let major = g.axis.major_dir;
    let segments = &mut g.axis.segments;
    let n = segments.len();

    // Each segment in the major direction against every opposite one above
    // it that overlaps it enough: the nearer pairing wins, or the longer
    // overlap at nearly the same distance. The scores change as the loops
    // run, so each is read afresh.
    for i in 0..n {
        if segments.get(i).is_none_or(|s| s.dir != major) {
            continue;
        }
        for j in 0..n {
            let (Some(&seg1), Some(&seg2)) = (segments.get(i), segments.get(j)) else {
                continue;
            };
            if i == j || seg1.dir + seg2.dir != 0 {
                continue;
            }
            let dist = seg2.pos - seg1.pos;
            if dist < 0 {
                continue;
            }
            let min = seg1.min_coord.max(seg2.min_coord);
            let max = seg1.max_coord.min(seg2.max_coord);
            let len = max - min;
            if len < len_threshold {
                continue;
            }
            let better = |s: &super::glyph::Segment| {
                dist * 8 < s.score * 9 && (dist * 8 < s.score * 7 || s.len < len)
            };
            if let Some(s1) = segments.get_mut(i)
                && better(s1)
            {
                s1.score = dist;
                s1.len = len;
                s1.link = Some(j);
            }
            if let Some(s2) = segments.get_mut(j)
                && better(s2)
            {
                s2.score = dist;
                s2.len = len;
                s2.link = Some(i);
            }
        }
    }

    // A Hanzi stroke is often wider at one end or both. A stem found across
    // such an end (seg2 to link2) enclosing a thinner one (seg1 to link1)
    // is the end's: if the thin stem is long, the end's segments become its
    // serifs; if not, the thin stem is no stem.
    for i in 0..n {
        let Some(seg1) = segments.get(i).copied() else {
            continue;
        };
        let Some(l1) = seg1.link else {
            continue;
        };
        let Some(link1) = segments.get(l1).copied() else {
            continue;
        };
        if link1.link != Some(i) || link1.pos <= seg1.pos || seg1.score >= dist_threshold {
            continue;
        }
        for j in 0..n {
            // `seg1` itself is read afresh: its link is what changes.
            let (Some(seg1), Some(seg2)) = (segments.get(i).copied(), segments.get(j).copied())
            else {
                continue;
            };
            if seg2.pos > seg1.pos || i == j {
                continue;
            }
            let Some(l2) = seg2.link else {
                continue;
            };
            let Some(link2) = segments.get(l2).copied() else {
                continue;
            };
            if link2.link != Some(j) || link2.pos < link1.pos {
                continue;
            }
            if seg1.pos == seg2.pos && link1.pos == link2.pos {
                continue;
            }
            if seg2.score <= seg1.score || seg1.score * 4 <= seg2.score {
                continue;
            }
            // seg2 <= seg1 < link1 <= link2.
            if seg1.len >= seg2.len * 3 {
                for seg in segments.iter_mut() {
                    if seg.link == Some(j) {
                        seg.link = None;
                        seg.serif = Some(l1);
                    } else if seg.link == Some(l2) {
                        seg.link = None;
                        seg.serif = Some(i);
                    }
                }
            } else {
                if let Some(s) = segments.get_mut(l1) {
                    s.link = None;
                }
                if let Some(s) = segments.get_mut(i) {
                    s.link = None;
                }
                break;
            }
        }
    }

    // A segment whose partner prefers another loses its link, and, if the
    // pairing was close or much the better, serifs its partner's partner.
    for i in 0..n {
        let Some(l) = segments.get(i).and_then(|s| s.link) else {
            continue;
        };
        let Some(seg2) = segments.get(l).copied() else {
            continue;
        };
        if seg2.link == Some(i) {
            continue;
        }
        let Some(seg1) = segments.get_mut(i) else {
            continue;
        };
        seg1.link = None;
        if seg2.score < dist_threshold || seg1.score < seg2.score * 4 {
            seg1.serif = seg2.link;
        }
    }
}

/// Gather segments into edges, sorted by position, and link the edges as
/// their segments are linked: `af_cjk_hints_compute_edges`.
fn compute_edges(hints: &mut Hints, metrics: &Metrics, dim: Dim) -> Option<()> {
    let g = hints.view(dim);
    let scale = g.axis.scale;
    g.axis.edges.clear();
    // A fifth of the standard width, and a quarter pixel at most.
    let threshold = metrics.axes.get(slot(dim))?.edge_distance_threshold;
    let threshold = if mul_fix(threshold, scale) > 64 / 4 {
        div_fix(64 / 4, scale)
    } else {
        threshold
    };
    let major_dir = g.axis.major_dir;
    let (segments, edges) = (&mut g.axis.segments, &mut g.axis.edges);

    for s in 0..segments.len() {
        let seg = *segments.get(s)?;
        // The nearest edge of the same direction within the threshold, whose
        // segments' partners are all within it of this one's partner.
        let mut found: Option<usize> = None;
        let mut best = 0xFFFF_i64;
        for (e, edge) in edges.iter().enumerate() {
            if edge.dir != seg.dir {
                continue;
            }
            let dist = (seg.pos - edge.fpos).abs();
            if dist >= threshold || dist >= best {
                continue;
            }
            if let Some(link) = seg.link {
                let link_pos = segments.get(link)?.pos;
                let mut s1 = edge.first;
                let mut dist2 = 0;
                let mut guard = segments.len() + 1;
                loop {
                    let seg1 = segments.get(s1)?;
                    if let Some(link1) = seg1.link {
                        dist2 = (link_pos - segments.get(link1)?.pos).abs();
                        if dist2 >= threshold {
                            break;
                        }
                    }
                    s1 = seg1.edge_next?;
                    if s1 == edge.first {
                        break;
                    }
                    guard = guard.checked_sub(1)?;
                }
                if dist2 >= threshold {
                    continue;
                }
            }
            best = dist;
            found = Some(e);
        }
        match found {
            Some(e) => {
                // Into the edge's ring of segments.
                let (first, last) = {
                    let edge = edges.get(e)?;
                    (edge.first, edge.last)
                };
                segments.get_mut(s)?.edge_next = Some(first);
                segments.get_mut(last)?.edge_next = Some(s);
                edges.get_mut(e)?.last = s;
            }
            None => {
                let at = insertion_point(edges, seg.pos, seg.dir, major_dir, false);
                let opos = mul_fix(seg.pos, scale);
                edges.insert(
                    at,
                    Edge {
                        fpos: seg.pos,
                        opos,
                        pos: opos,
                        flags: 0,
                        dir: seg.dir,
                        blue: None,
                        link: None,
                        serif: None,
                        first: s,
                        last: s,
                    },
                );
                segments.get_mut(s)?.edge_next = Some(s);
            }
        }
    }

    // Each segment learns its edge.
    for e in 0..edges.len() {
        let first = edges.get(e)?.first;
        let mut s = first;
        let mut guard = segments.len() + 1;
        loop {
            let seg = segments.get_mut(s)?;
            seg.edge = Some(e);
            s = seg.edge_next?;
            if s == first {
                break;
            }
            guard = guard.checked_sub(1)?;
        }
    }

    // Each edge's roundness and links, from its segments'.
    for e in 0..edges.len() {
        let (mut is_round, mut is_straight) = (0u32, 0u32);
        let first = edges.get(e)?.first;
        let mut s = first;
        let mut guard = segments.len() + 1;
        loop {
            let seg = *segments.get(s)?;
            if seg.flags & EDGE_ROUND != 0 {
                is_round += 1;
            } else {
                is_straight += 1;
            }
            // A serif's own edge, where it has one other than this.
            let serif_edge = match seg.serif {
                Some(x) => Some(segments.get(x)?.edge),
                None => None,
            };
            let is_serif = serif_edge.is_some_and(|x| x != Some(e));
            if seg.link.is_some() || is_serif {
                let edge = *edges.get(e)?;
                let (seg2, current) = if is_serif {
                    (seg.serif?, edge.serif)
                } else {
                    (seg.link?, edge.link)
                };
                let seg2 = *segments.get(seg2)?;
                // The nearer of the edge found so far and this segment's.
                let edge2 = match current {
                    Some(c) => {
                        let edge_delta = (edge.fpos - edges.get(c)?.fpos).abs();
                        let seg_delta = (seg.pos - seg2.pos).abs();
                        if seg_delta < edge_delta {
                            seg2.edge
                        } else {
                            Some(c)
                        }
                    }
                    None => seg2.edge,
                };
                if is_serif {
                    edges.get_mut(e)?.serif = edge2;
                    if let Some(e2) = edge2 {
                        edges.get_mut(e2)?.flags |= EDGE_SERIF;
                    }
                } else {
                    edges.get_mut(e)?.link = edge2;
                }
            }
            s = seg.edge_next?;
            if s == first {
                break;
            }
            guard = guard.checked_sub(1)?;
        }
        let edge = edges.get_mut(e)?;
        // Assigned, not or-ed: a serif mark on an edge already processed
        // survives, on one still to come it does not -- as in FreeType.
        edge.flags = if is_round > 0 && is_round >= is_straight {
            EDGE_ROUND
        } else {
            0
        };
        if edge.serif.is_some() && edge.link.is_some() {
            edge.serif = None;
        }
    }
    Some(())
}

/// Find the zone, if any, each edge snaps to: the nearer of its reference
/// and overshoot, within a fortieth of an em and half a pixel at most:
/// `af_cjk_hints_compute_blue_edges`.
fn compute_blue_edges(hints: &mut Hints, metrics: &Metrics, scaled: &Scaled, dim: Dim) {
    let g = hints.view(dim);
    let (Some(axis), Some(fitted)) = (metrics.axes.get(slot(dim)), scaled.blues.get(slot(dim)))
    else {
        return;
    };
    let scale = g.axis.scale;
    let best_dist0 = mul_fix(metrics.units_per_em / 40, scale).min(64 / 2);
    let major = g.axis.major_dir;
    for edge in &mut g.axis.edges {
        let mut best: Option<i64> = None;
        let mut best_dist = best_dist0;
        for (blue, fit) in axis.blues.iter().zip(fitted) {
            if !fit.active {
                continue;
            }
            // A top zone catches edges with ink below them -- running against
            // the major direction -- and a bottom zone the opposite.
            if blue.top == (edge.dir == major) {
                continue;
            }
            let (org, at) = if (edge.fpos - blue.reference).abs() > (edge.fpos - blue.shoot).abs() {
                (blue.shoot, fit.shoot)
            } else {
                (blue.reference, fit.reference)
            };
            let dist = mul_fix((edge.fpos - org).abs(), scale);
            if dist < best_dist {
                best_dist = dist;
                best = Some(at);
            }
        }
        if best.is_some() {
            edge.blue = best;
        }
    }
}

/// A stem's second edge, placed its original distance from the first: in
/// light mode a stem keeps its width (`af_cjk_align_linked_edge`, where
/// `af_cjk_compute_stem_width` changes nothing). Also a serif's edge after
/// its base (`af_cjk_align_serif_edge`), which is the same arithmetic.
fn align_linked_edge(edges: &mut [Edge], base: usize, stem: usize) -> Option<()> {
    let b = *edges.get(base)?;
    let s = edges.get_mut(stem)?;
    s.pos = b.pos + (s.opos - b.opos);
    Some(())
}

/// Place a stem -- edges `e` and `e2` -- moved by `anchor` and then by as
/// little as puts both edges on pixel boundaries, or one where both would
/// move it too far; at most [`MAX_DELTA`] either way. The move is returned:
/// `af_hint_normal_stem` in light mode.
fn hint_normal_stem(edges: &mut [Edge], e: usize, e2: usize, anchor: i64, dim: Dim) -> Option<i64> {
    let (edge, edge2) = (*edges.get(e)?, *edges.get(e2)?);
    // The most a stem's edges may be off the grid and still count as on it:
    // less for two round edges, and less vertically than horizontally.
    let gap = match dim {
        Dim::Vert => MAX_HORZ_GAP,
        Dim::Horz => MAX_VERT_GAP,
    };
    let threshold = if edge.flags & EDGE_ROUND != 0 && edge2.flags & EDGE_ROUND != 0 {
        64 - gap
    } else {
        64 - gap / 3
    };
    let org_len = edge2.opos - edge.opos;
    let cur_len = org_len;
    // C's divisions, which round toward zero, as Rust's do (and
    // `midpoint`, for a sum halved).
    let org_center = edge.opos.midpoint(edge2.opos) + anchor;
    let mut cur_pos1 = org_center - cur_len / 2;
    let cur_pos2 = cur_pos1 + cur_len;
    let mut d_off1 = cur_pos1 - pix_floor(cur_pos1);
    let mut d_off2 = cur_pos2 - pix_floor(cur_pos2);
    let mut u_off1 = 64 - d_off1;
    let mut u_off2 = 64 - d_off2;
    let mut delta = 0;
    'fit: {
        if d_off1 == 0 || d_off2 == 0 {
            break 'fit;
        }
        if cur_len <= threshold {
            // A thin stem: one of its edges onto a boundary, if it is within
            // the stem's width of one.
            if d_off2 < cur_len {
                delta = if u_off1 <= d_off2 { u_off1 } else { -d_off2 };
            }
            break 'fit;
        }
        if threshold < 64
            && (d_off1 >= threshold
                || u_off1 >= threshold
                || d_off2 >= threshold
                || u_off2 >= threshold)
        {
            break 'fit;
        }
        let mut offset = cur_len & 63;
        if offset < 32 {
            if u_off1 <= offset || d_off2 <= offset {
                break 'fit;
            }
        } else {
            offset = 64 - threshold;
        }
        d_off1 = threshold - u_off1;
        u_off1 -= offset;
        u_off2 = threshold - d_off2;
        d_off2 -= offset;
        if d_off1 <= u_off1 {
            u_off1 = -d_off1;
        }
        if d_off2 <= u_off2 {
            u_off2 = -d_off2;
        }
        delta = if u_off1.abs() <= u_off2.abs() {
            u_off1
        } else {
            u_off2
        };
    }
    let delta = delta.clamp(-MAX_DELTA, MAX_DELTA);
    cur_pos1 += delta;
    let (lower, upper) = (cur_pos1, cur_pos1 + cur_len);
    if edge.opos < edge2.opos {
        edges.get_mut(e)?.pos = lower;
        edges.get_mut(e2)?.pos = upper;
    } else {
        edges.get_mut(e)?.pos = upper;
        edges.get_mut(e2)?.pos = lower;
    }
    Some(delta)
}

/// Place every edge of dimension `dim`: `af_cjk_hint_edges` in light mode.
fn hint_edges(hints: &mut Hints, dim: Dim) -> Option<()> {
    let g = hints.view(dim);
    let edges = &mut g.axis.edges;
    let n = edges.len();
    let mut anchored = false;
    // The first horizontal stem's move, which every later one repeats.
    let mut delta = 0i64;
    let mut skipped = 0usize;
    // The last stem's far edge, which the next stem must clear by a pixel.
    let mut last_stem: Option<i64> = None;

    // Edges in zones, and the stems they bound.
    for e in 0..n {
        let edge = *edges.get(e)?;
        if edge.flags & EDGE_DONE != 0 {
            continue;
        }
        let (edge1, edge2, blue) = if let Some(blue) = edge.blue {
            (e, edge.link, blue)
        } else if let Some(e2) = edge.link
            && let Some(blue) = edges.get(e2)?.blue
        {
            // The linked edge is the one in the zone.
            (e2, Some(e), blue)
        } else {
            continue;
        };
        {
            let x = edges.get_mut(edge1)?;
            x.pos = blue;
            x.flags |= EDGE_DONE;
        }
        if let Some(e2) = edge2
            && edges.get(e2)?.blue.is_none()
        {
            align_linked_edge(edges, edge1, e2)?;
            edges.get_mut(e2)?.flags |= EDGE_DONE;
        }
        anchored = true;
    }

    // The stems.
    for e in 0..n {
        let edge = *edges.get(e)?;
        if edge.flags & EDGE_DONE != 0 {
            continue;
        }
        let Some(e2) = edge.link else {
            skipped += 1;
            continue;
        };
        let edge2 = *edges.get(e2)?;
        // Ideographs can have so many stems that two would merge: one within
        // a pixel of the last is left to be interpolated, keeping the gap.
        if let Some(last) = last_stem
            && (edge.pos < last + 64 || edge2.pos < last + 64)
        {
            skipped += 1;
            continue;
        }
        if edge2.blue.is_some() {
            // Cannot happen, FreeType notes; handled all the same.
            align_linked_edge(edges, e2, e)?;
            edges.get_mut(e)?.flags |= EDGE_DONE;
            continue;
        }
        if e2 < e {
            align_linked_edge(edges, e2, e)?;
            edges.get_mut(e)?.flags |= EDGE_DONE;
            last_stem = Some(edges.get(e)?.pos);
            continue;
        }
        if dim != Dim::Vert && !anchored {
            delta = hint_normal_stem(edges, e, e2, 0, Dim::Horz)?;
        } else {
            hint_normal_stem(edges, e, e2, delta, dim)?;
        }
        anchored = true;
        edges.get_mut(e)?.flags |= EDGE_DONE;
        edges.get_mut(e2)?.flags |= EDGE_DONE;
        last_stem = Some(edges.get(e2)?.pos);
    }

    // A lowercase m's three stems -- six edges without serifs, twelve with
    // -- kept evenly spaced, if they were so drawn.
    if dim == Dim::Horz && (n == 6 || n == 12) {
        let (i1, i2, i3) = if n == 6 { (0, 2, 4) } else { (1, 5, 9) };
        let (e1, e2, e3) = (*edges.get(i1)?, *edges.get(i2)?, *edges.get(i3)?);
        let span = ((e2.opos - e1.opos) - (e3.opos - e2.opos)).abs();
        if e1.link == Some(i1 + 1) && e2.link == Some(i2 + 1) && e3.link == Some(i3 + 1) && span < 8
        {
            let d = e3.pos - (2 * e2.pos - e1.pos);
            edges.get_mut(i3)?.pos -= d;
            edges.get_mut(i3 + 1)?.pos -= d;
            // The serifs go with the stem.
            if n == 12 {
                edges.get_mut(8)?.pos -= d;
                edges.get_mut(11)?.pos -= d;
            }
            edges.get_mut(i3)?.flags |= EDGE_DONE;
            edges.get_mut(i3 + 1)?.flags |= EDGE_DONE;
        }
    }

    if skipped == 0 {
        return Some(());
    }
    // Serifs, with the edge they serif.
    for e in 0..n {
        let edge = *edges.get(e)?;
        if edge.flags & EDGE_DONE != 0 {
            continue;
        }
        if let Some(s) = edge.serif {
            align_linked_edge(edges, s, e)?;
            edges.get_mut(e)?.flags |= EDGE_DONE;
            skipped = skipped.saturating_sub(1);
        }
    }
    if skipped == 0 {
        return Some(());
    }
    // The rest: between the placed edges either side of each, in proportion
    // to their unscaled positions, or with the one placed edge beside it.
    // These are not marked placed, so none is interpolated from another.
    for e in 0..n {
        let edge = *edges.get(e)?;
        if edge.flags & EDGE_DONE != 0 {
            continue;
        }
        let done = |i: &usize| edges.get(*i).is_some_and(|x| x.flags & EDGE_DONE != 0);
        let before = (0..e).rev().find(done);
        let after = (e + 1..n).find(done);
        match (before, after) {
            (None, None) => {}
            (None, Some(a)) => align_linked_edge(edges, a, e)?,
            (Some(b), None) => align_linked_edge(edges, b, e)?,
            (Some(b), Some(a)) => {
                let (b, a) = (*edges.get(b)?, *edges.get(a)?);
                edges.get_mut(e)?.pos = if a.fpos == b.fpos {
                    b.pos
                } else {
                    b.pos + mul_div(edge.fpos - b.fpos, a.pos - b.pos, a.fpos - b.fpos)
                };
            }
        }
    }
    Some(())
}

/// Move every point of every edge's segments by as far as its edge moved:
/// `af_cjk_align_edge_points` without snapping, which light mode never
/// does. A point two segments share -- a spike's tip -- moves by both.
fn align_edge_points(hints: &mut Hints, dim: Dim) -> Option<()> {
    let g = hints.view(dim);
    for edge in &g.axis.edges {
        let delta = edge.pos - edge.opos;
        let mut s = edge.first;
        let mut seg_guard = g.axis.segments.len() + 1;
        loop {
            let seg = g.axis.segments.get(s)?;
            let mut point = seg.first;
            let mut guard = g.points.len() + 1;
            loop {
                let p = g.points.get_mut(point)?;
                p.place(dim, p.at(dim) + delta);
                if point == seg.last {
                    break;
                }
                point = p.next;
                guard = guard.checked_sub(1)?;
            }
            s = seg.edge_next?;
            if s == edge.first {
                break;
            }
            seg_guard = seg_guard.checked_sub(1)?;
        }
    }
    Some(())
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test fixtures"
)]
mod tests {
    use super::*;
    use crate::hint::Glyphs;
    use crate::hint::glyph::Units;
    use crate::sfnt::{Exact as P, Tag, TaggedOutline};

    fn edge(opos: i64, flags: u8) -> Edge {
        Edge {
            fpos: opos,
            opos,
            pos: opos,
            flags,
            dir: 0,
            blue: None,
            link: None,
            serif: None,
            first: 0,
            last: 0,
        }
    }

    /// A rectangle `(x0, y0, x1, y1)`, clockwise.
    type Rect = (f64, f64, f64, f64);

    /// Clockwise rectangles, one contour each -- and one of no size a
    /// contour of one point, as a mark anchor is drawn.
    fn rects(rs: &[Rect]) -> TaggedOutline {
        let mut o = TaggedOutline::default();
        for &(x0, y0, x1, y1) in rs {
            if (x0, y0) == (x1, y1) {
                o.points.push(P::new(x0, y0));
                o.tags.push(Tag::On);
            } else {
                o.points.extend([
                    P::new(x0, y0),
                    P::new(x0, y1),
                    P::new(x1, y1),
                    P::new(x1, y0),
                ]);
                o.tags.extend([Tag::On; 4]);
            }
            o.ends.push(o.points.len());
        }
        o
    }

    #[test]
    fn a_stem_moves_at_most_fourteen_sixty_fourths_toward_the_grid() {
        // A stem of Malgun Gothic's at 16 px, its edges at 474 and 549. Two
        // round edges may sit 15/64 off the grid: it moves 11 left, putting
        // its right edge 26 over a boundary.
        let mut edges = [edge(474, EDGE_ROUND), edge(549, EDGE_ROUND)];
        assert_eq!(hint_normal_stem(&mut edges, 0, 1, 0, Dim::Horz), Some(-11));
        assert_eq!((edges[0].pos, edges[1].pos), (463, 538));
        // Straight ones only 5/64: it wants 21, and 14 is light mode's most.
        let mut edges = [edge(474, 0), edge(549, 0)];
        assert_eq!(hint_normal_stem(&mut edges, 0, 1, 0, Dim::Horz), Some(-14));
        assert_eq!((edges[0].pos, edges[1].pos), (460, 535));
        // A thin stem puts one edge on a boundary: here the upper, 12 down.
        let mut edges = [edge(100, 0), edge(140, 0)];
        assert_eq!(hint_normal_stem(&mut edges, 0, 1, 0, Dim::Vert), Some(-12));
        assert_eq!((edges[0].pos, edges[1].pos), (88, 128));
        // The anchor's move comes first: a stem on the grid, carried 10 off
        // it, comes back only 7, to 3/64 above the boundaries -- as near as
        // a straight stem's edges need be.
        let mut edges = [edge(128, 0), edge(192, 0)];
        assert_eq!(hint_normal_stem(&mut edges, 0, 1, 10, Dim::Vert), Some(-7));
        assert_eq!((edges[0].pos, edges[1].pos), (131, 195));
    }

    #[test]
    fn a_zone_under_three_quarters_of_a_pixel_is_fitted_to_the_grid() {
        let blue = |reference, shoot| Blue {
            reference,
            shoot,
            top: true,
        };
        let metrics = Metrics {
            units_per_em: 1000,
            axes: [
                Axis::default(),
                Axis {
                    edge_distance_threshold: 12,
                    blues: alloc::vec![blue(880, 860), blue(880, 840), blue(880, 830)],
                },
            ],
        };
        // 16 px on a 1000-unit em.
        let scale = div_fix(16 * 64, 1000);
        let s = metrics.scale(scale, scale);
        let fitted = &s.blues[1];
        // 20 units apart: the reference to the pixel boundary below 14.08 px,
        // and the overshoot, under half a pixel away, with it.
        assert_eq!(
            fitted[0],
            Fitted {
                reference: 896,
                shoot: 896,
                active: true
            }
        );
        // 40 apart: the overshoot a whole pixel from the reference.
        assert_eq!(
            fitted[1],
            Fitted {
                reference: 896,
                shoot: 832,
                active: true
            }
        );
        // 50 apart is past 3/4 pixel: left as scaled, and unused.
        assert!(!fitted[2].active);
        assert_eq!(
            (fitted[2].reference, fitted[2].shoot),
            (mul_fix(880, scale), mul_fix(830, scale))
        );
        assert!(s.blues[0].is_empty());
    }

    /// A face of a few ideographs, each character one glyph drawn as the
    /// rectangles given, measured as a CJK style.
    fn measure(chars: &[(char, &[Rect])], standard: &str, zones: &[BlueString]) -> Metrics {
        let shape = |cluster: &str| -> Glyphs {
            let mut it = cluster.chars();
            let (Some(c), None) = (it.next(), it.next()) else {
                // Two characters make two glyphs, which a zone skips.
                return alloc::vec![(1, 0), (2, 0)];
            };
            chars.iter().position(|&(ch, _)| ch == c).map_or_else(
                || alloc::vec![(0, 0)],
                |i| alloc::vec![(u16::try_from(i + 1).unwrap(), 0)],
            )
        };
        let outline = |gid: u16| {
            chars
                .get(usize::from(gid).checked_sub(1)?)
                .map(|&(_, rs)| rects(rs))
        };
        let source = Source {
            shape: &shape,
            outline: &outline,
            units: Units::Rounded,
        };
        Metrics::new(1000, standard, zones, true, &source).unwrap()
    }

    #[test]
    fn zones_are_the_medians_of_their_two_groups_and_the_mean_if_crossed() {
        let chars: &[(char, &[Rect])] = &[
            ('\u{4ED6}', &[(100.0, -80.0, 900.0, 880.0)]),
            ('\u{4EEC}', &[(100.0, -70.0, 900.0, 870.0)]),
            ('\u{4F60}', &[(100.0, -75.0, 900.0, 876.0)]),
            // Its high one-point contour is a mark anchor, never drawn.
            (
                '\u{519B}',
                &[(100.0, -100.0, 900.0, 860.0), (500.0, 990.0, 500.0, 990.0)],
            ),
        ];
        let zones = [
            // Top: 876, the median of the first group, and 860 of the
            // second; the missing U+540C and the two-glyph cluster skipped.
            BlueString {
                chars: "\u{4ED6} \u{4EEC} \u{4F60} \u{540C} \u{4ED6}\u{4EEC} | \u{519B}",
                props: PROP_TOP,
            },
            // Bottom: -80 against -100, an overshoot below its reference --
            // the wrong side for a bottom zone -- so both take the mean.
            BlueString {
                chars: "\u{4ED6} | \u{519B}",
                props: 0,
            },
        ];
        let m = measure(chars, "", &zones);
        assert_eq!(
            m.axes[1].blues,
            [
                Blue {
                    reference: 876,
                    shoot: 860,
                    top: true
                },
                Blue {
                    reference: -90,
                    shoot: -90,
                    top: false
                }
            ]
        );
        assert!(m.axes[0].blues.is_empty());
    }

    #[test]
    fn both_dimensions_take_their_standard_width_from_the_first_letter_the_face_has() {
        // U+7530 with 60-unit stems both ways; U+56D7 is not in the face.
        let chars: &[(char, &[Rect])] = &[(
            '\u{7530}',
            &[
                (100.0, 0.0, 160.0, 800.0),
                (840.0, 0.0, 900.0, 800.0),
                (470.0, 0.0, 530.0, 800.0),
                (160.0, 0.0, 470.0, 60.0),
                (530.0, 0.0, 840.0, 60.0),
                (160.0, 370.0, 470.0, 430.0),
                (530.0, 370.0, 840.0, 430.0),
                (160.0, 740.0, 470.0, 800.0),
                (530.0, 740.0, 840.0, 800.0),
            ],
        )];
        let m = measure(chars, "\u{56D7} \u{7530}", &[]);
        assert_eq!(m.axes[0].edge_distance_threshold, 60 / 5);
        assert_eq!(m.axes[1].edge_distance_threshold, 60 / 5);
        // No standard letter at all: 50 units of a 2048 em, a fifth of it.
        let m = measure(chars, "\u{56D7}", &[]);
        assert_eq!(m.axes[0].edge_distance_threshold, (50 * 1000 / 2048) / 5);
    }

    #[test]
    fn a_face_without_a_unicode_cmap_measures_nothing() {
        let shape = |_: &str| -> Glyphs { alloc::vec![(1, 0)] };
        let outline = |_: u16| Some(rects(&[(0.0, 0.0, 60.0, 800.0)]));
        let source = Source {
            shape: &shape,
            outline: &outline,
            units: Units::Rounded,
        };
        let zones = [BlueString {
            chars: "\u{4ED6} | \u{519B}",
            props: PROP_TOP,
        }];
        let m = Metrics::new(1000, "\u{7530}", &zones, false, &source).unwrap();
        assert_eq!(m.axes, [Axis::default(), Axis::default()]);
    }
}
