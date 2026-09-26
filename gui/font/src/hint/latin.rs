//! The Latin writing system: FreeType's `aflatin.c`, as light hinting uses it
//! -- the vertical dimension only, stems never resized.
//!
//! Most of the world's scripts are hinted by this module, not just Latin:
//! FreeType's Latin writing system serves every script whose letters share
//! a few horizontal alignment lines, each script supplying its own reference
//! letters (see [`super::tables`]).
//!
//! Two halves, as in FreeType:
//!
//! * **Metrics**, once per face, style and instance ([`Metrics::new`]): the
//!   standard stem width, measured from one reference letter
//!   (`af_latin_metrics_init_widths`), and the *blue zones* -- baseline,
//!   x-height, cap height and the rest -- measured from the extremes of each
//!   zone's reference letters (`af_latin_metrics_init_blues`). Then, once per
//!   size ([`Metrics::scale`]), the vertical scale is nudged so the x-height
//!   lands on a pixel boundary and the zones are fitted to the grid
//!   (`af_latin_metrics_scale_dim`).
//! * **One glyph** ([`hint`]): its outline cut into horizontal segments
//!   (`af_latin_hints_compute_segments`), segments paired across stems
//!   (`..._link_segments`) and gathered into edges (`..._compute_edges`),
//!   edges in a blue zone found (`..._compute_blue_edges`), and every edge
//!   placed (`af_latin_hint_edges`): blue edges on their zone, stems at whole
//!   positions with their widths kept exactly (light hinting's defining
//!   choice), the rest interpolated. The points then follow ([`Hints`]'s
//!   three align passes).
//!
//! Only the light mode's paths are ported. FreeType's stem-width snapping,
//! monochrome rounding and horizontal hinting are switched off in light mode
//! and are absent here.
//!
//! Portions of this file are copyright (C) 2003-2023 by David Turner, Robert
//! Wilhelm and Werner Lemberg, from The FreeType Project (www.freetype.org).
//! Used under the FreeType License: see `gui/font/licenses/FTL.TXT`.

#![allow(
    clippy::arithmetic_side_effects,
    reason = "bounded operands, as in `fixed`: font units within i16 and \
              scales within fixed::MAX_SCALE"
)]

use alloc::vec::Vec;

use super::fixed::{div_fix, mul_div, mul_fix, pix_floor, pix_round};
use super::glyph::{
    DIR_NONE, EDGE_DONE, EDGE_NEUTRAL, EDGE_ROUND, EDGE_SERIF, Edge, FLAG_CONTROL, Hints, Segment,
    Units, font_unit,
};
use super::{Blue as BlueString, Glyphs};
use crate::sfnt::{Tag, TaggedOutline};

/// Blue zone properties, as the tables give them (`AF_BLUE_PROPERTY_LATIN_*`).
const PROP_TOP: u8 = 1 << 0;
const PROP_SUB_TOP: u8 = 1 << 1;
const PROP_NEUTRAL: u8 = 1 << 2;
const PROP_X_HEIGHT: u8 = 1 << 3;
const PROP_LONG: u8 = 1 << 4;

/// A zone's flags once measured (`AF_LATIN_BLUE_*`).
const BLUE_ACTIVE: u8 = 1 << 0;
const BLUE_TOP: u8 = 1 << 1;
const BLUE_SUB_TOP: u8 = 1 << 2;
const BLUE_NEUTRAL: u8 = 1 << 3;
const BLUE_ADJUSTMENT: u8 = 1 << 4;

/// At most this many standard widths (`AF_LATIN_MAX_WIDTHS`).
const MAX_WIDTHS: usize = 16;

/// `c` units of a 2048 em in this face's units (`AF_LATIN_CONSTANT`).
const fn constant(units_per_em: i64, c: i64) -> i64 {
    c * units_per_em / 2048
}

/// One blue zone as measured, in font units (`AF_LatinBlueRec`, unscaled).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Blue {
    /// The flat letters' line: the zone's reference.
    reference: i64,
    /// The round letters' line: the overshoot.
    shoot: i64,
    /// The reference letters' highest and lowest points, which bound how
    /// far the x-height nudge may move anything.
    ascender: i64,
    descender: i64,
    flags: u8,
}

/// A style's vertical metrics, in font units (`AF_LatinMetricsRec`, vertical
/// axis only).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Metrics {
    units_per_em: i64,
    /// Horizontal stems' heights, standard first.
    widths: Vec<i64>,
    /// How close two segments must be to join one edge.
    edge_distance_threshold: i64,
    blues: Vec<Blue>,
    /// Whether this script's stems are placed from the top down.
    top_to_bottom: bool,
}

/// One zone fitted to a size, 26.6.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Fitted {
    reference: i64,
    shoot: i64,
    flags: u8,
}

/// A style's metrics at one size (`AF_LatinMetricsRec` after scaling).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Scaled {
    /// The vertical scale, 16.16, after the x-height nudge.
    pub(super) y_scale: i64,
    blues: Vec<Fitted>,
}

/// Where the reference letters come from: a cluster of text shaped to glyphs,
/// and a glyph's points.
pub(super) struct Source<'a> {
    /// The glyphs a cluster shapes to, each with its vertical offset in font
    /// units -- as FreeType has HarfBuzz shape it at the em size.
    pub(super) shape: &'a dyn Fn(&str) -> Glyphs,
    /// A glyph's stored points, at the instance being hinted.
    pub(super) outline: &'a dyn Fn(u16) -> Option<TaggedOutline>,
    /// How the face's coordinates become whole font units.
    pub(super) units: Units,
}

impl Metrics {
    /// Measure a Latin-system style: its standard widths from `standard`'s
    /// letters and its zones from `blues`'. `None` when not one zone could be
    /// measured, which FreeType answers by not hinting the style at all.
    pub(super) fn new(
        units_per_em: i64,
        standard: &str,
        blues: &[BlueString],
        top_to_bottom: bool,
        source: &Source<'_>,
    ) -> Option<Self> {
        if !(16..=16384).contains(&units_per_em) {
            return None;
        }
        let widths = standard_widths(units_per_em, standard, source);
        let standard_width = widths
            .first()
            .copied()
            .unwrap_or_else(|| constant(units_per_em, 50));
        let blues = measure_blues(units_per_em, blues, source)?;
        Some(Self {
            units_per_em,
            widths,
            // A fifth of the standard width.
            edge_distance_threshold: standard_width / 5,
            blues,
            top_to_bottom,
        })
    }

    /// Fit the zones to a size whose scale is `y_scale` (16.16), nudging the
    /// scale first so the x-height lands on a pixel boundary:
    /// `af_latin_metrics_scale_dim` for the vertical axis.
    pub(super) fn scale(&self, y_scale: i64) -> Scaled {
        let mut scale = y_scale;
        if let Some(blue) = self.blues.iter().find(|b| b.flags & BLUE_ADJUSTMENT != 0) {
            let scaled = mul_fix(blue.shoot, scale);
            // Round the x-height up from 40/64 of a pixel down. (FreeType's
            // `increase-x-height` property, off by default, would use 52.)
            let fitted = pix_floor(scaled + 40);
            if scaled != fitted {
                let new_scale = mul_div(scale, fitted, scaled);
                // Only if nothing moves by two pixels or more.
                let max_height = self.blues.iter().fold(self.units_per_em, |m, b| {
                    m.max(b.ascender).max(-b.descender)
                });
                let dist = mul_fix(max_height, new_scale - scale).abs() & !127;
                if dist == 0 {
                    scale = new_scale;
                }
            }
        }
        let mut blues: Vec<Fitted> = self
            .blues
            .iter()
            .map(|b| {
                let reference = mul_fix(b.reference, scale);
                let shoot = mul_fix(b.shoot, scale);
                let mut fitted = Fitted {
                    reference,
                    shoot,
                    flags: b.flags & !BLUE_ACTIVE,
                };
                // A zone is used only while under 3/4 pixel tall; its
                // overshoot is then none, half a pixel or a whole one.
                let dist = mul_fix(b.reference - b.shoot, scale);
                if (-48..=48).contains(&dist) {
                    let d = dist.abs();
                    let d = if d < 32 {
                        0
                    } else if d < 48 {
                        32
                    } else {
                        64
                    };
                    let delta = if dist < 0 { -d } else { d };
                    fitted.reference = pix_round(reference);
                    fitted.shoot = fitted.reference - delta;
                    fitted.flags |= BLUE_ACTIVE;
                }
                fitted
            })
            .collect();
        // A sub-top zone overlapping another zone would act as a neutral one,
        // which it must not: it is switched off.
        for i in 0..blues.len() {
            let Some(&blue) = blues.get(i) else {
                continue;
            };
            if blue.flags & BLUE_SUB_TOP == 0 || blue.flags & BLUE_ACTIVE == 0 {
                continue;
            }
            let overlaps = blues.iter().any(|b| {
                b.flags & BLUE_SUB_TOP == 0
                    && b.flags & BLUE_ACTIVE != 0
                    && b.reference <= blue.shoot
                    && b.shoot >= blue.reference
            });
            if overlaps && let Some(b) = blues.get_mut(i) {
                b.flags &= !BLUE_ACTIVE;
            }
        }
        Scaled {
            y_scale: scale,
            blues,
        }
    }
}

/// The standard stems' heights, from the first of `standard`'s letters the
/// face has: `af_latin_metrics_init_widths` for the vertical axis.
fn standard_widths(units_per_em: i64, standard: &str, source: &Source<'_>) -> Vec<i64> {
    // The first cluster that is one glyph the face has.
    let Some(gid) = standard
        .split(' ')
        .filter(|c| !c.is_empty())
        .find_map(|cluster| {
            let glyphs = (source.shape)(cluster);
            match glyphs.as_slice() {
                [(gid, _)] if *gid != 0 => Some(*gid),
                _ => None,
            }
        })
    else {
        return Vec::new();
    };
    let Some(outline) = (source.outline)(gid).filter(|o| !o.points.is_empty()) else {
        return Vec::new();
    };
    // Unscaled: a 16.16 scale of one.
    let Some(mut hints) = Hints::load(&outline, source.units, 0x10000, units_per_em) else {
        return Vec::new();
    };
    let mut widths = Vec::new();
    if compute_segments(&mut hints).is_some() {
        link_segments(&mut hints, &[]);
        for (i, seg) in hints.segments.iter().enumerate() {
            let Some(j) = seg.link else {
                continue;
            };
            let Some(link) = hints.segments.get(j) else {
                continue;
            };
            // Stems only: segments linked both ways, each counted once.
            if link.link == Some(i) && j > i && widths.len() < MAX_WIDTHS {
                widths.push((seg.pos - link.pos).abs());
            }
        }
    }
    sort_and_quantize(&mut widths, units_per_em / 100);
    widths
}

/// Sort `widths` and replace each cluster of nearly equal ones with a mean:
/// `af_sort_and_quantize_widths`, whose arithmetic is kept exactly -- the
/// divisor is the cluster's end index, not its size, and an empty list comes
/// back as one zero width, which is what FreeType computes from them.
fn sort_and_quantize(widths: &mut Vec<i64>, threshold: i64) {
    if widths.len() == 1 {
        return;
    }
    if widths.is_empty() {
        widths.push(0);
        return;
    }
    widths.sort_unstable();
    let count = widths.len();
    let mut cur_idx = 0usize;
    let mut cur_val = widths.first().copied().unwrap_or(0);
    let mut i = 1usize;
    while i < count {
        let here = widths.get(i).copied().unwrap_or(0);
        if here - cur_val > threshold || i == count - 1 {
            if here - cur_val <= threshold && i == count - 1 {
                i += 1;
            }
            let mut sum = 0;
            let mut j = cur_idx;
            while j < i {
                if let Some(w) = widths.get_mut(j) {
                    sum += *w;
                    *w = 0;
                }
                j += 1;
            }
            if let Some(w) = widths.get_mut(cur_idx) {
                *w = sum / i64::try_from(j.max(1)).unwrap_or(1);
            }
            if i < count - 1 {
                cur_idx = i + 1;
                cur_val = widths.get(cur_idx).copied().unwrap_or(0);
            }
        }
        i += 1;
    }
    // Keep the first entry and every non-zero one after it.
    let first = widths.first().copied();
    let rest: Vec<i64> = widths.iter().skip(1).copied().filter(|&w| w != 0).collect();
    widths.clear();
    widths.extend(first);
    widths.extend(rest);
}

/// The extreme of one reference glyph, if it has one: its height (with the
/// shaper's offset) and whether it is round.
fn extreme(
    outline: &TaggedOutline,
    units: Units,
    y_offset: i64,
    props: u8,
    units_per_em: i64,
    ascender: &mut i64,
    descender: &mut i64,
) -> Option<(i64, bool)> {
    let top = props & (PROP_TOP | PROP_SUB_TOP) != 0;
    let xs: Vec<i64> = outline
        .points
        .iter()
        .map(|p| font_unit(p.x, units))
        .collect::<Option<_>>()?;
    let ys: Vec<i64> = outline
        .points
        .iter()
        .map(|p| font_unit(p.y, units))
        .collect::<Option<_>>()?;
    let on = |i: usize| outline.tags.get(i) == Some(&Tag::On);
    let x = |i: usize| xs.get(i).copied().unwrap_or(0);
    let y = |i: usize| ys.get(i).copied().unwrap_or(0);

    // The extreme point, in the contour that holds it.
    let mut best: Option<usize> = None;
    let mut best_y = 0i64;
    let mut contour: Option<(usize, usize)> = None;
    let mut start = 0usize;
    for &end in &outline.ends {
        let (first, last) = (start, end.checked_sub(1)?);
        start = end;
        // One-point contours are never drawn; some fonts use them for mark
        // attachment far outside the glyph.
        if last <= first {
            continue;
        }
        for pp in first..=last {
            let v = y(pp);
            let better = match best {
                None => true,
                Some(_) => {
                    if top {
                        v > best_y
                    } else {
                        v < best_y
                    }
                }
            };
            if better {
                best = Some(pp);
                best_y = v;
                if top {
                    *ascender = (*ascender).max(v + y_offset);
                } else {
                    *descender = (*descender).min(v + y_offset);
                }
            } else if top {
                *descender = (*descender).min(v + y_offset);
            } else {
                *ascender = (*ascender).max(v + y_offset);
            }
        }
        if let Some(b) = best
            && contour.is_none_or(|(_, l)| b > l)
        {
            contour = Some((first, last));
        }
    }
    let (contour_first, contour_last) = contour.unwrap_or((0, 0));

    let mut round = false;
    if let Some(best_point) = best {
        let best_x = x(best_point);
        let (mut seg_first, mut seg_last) = (best_point, best_point);
        let (mut on_first, mut on_last): (Option<usize>, Option<usize>) = if on(best_point) {
            (Some(best_point), Some(best_point))
        } else {
            (None, None)
        };
        // Walk out both ways while the outline stays nearly level: within
        // five units, or under about three degrees.
        let prev_of = |i: usize| {
            if i > contour_first {
                i - 1
            } else {
                contour_last
            }
        };
        let next_of = |i: usize| {
            if i < contour_last {
                i + 1
            } else {
                contour_first
            }
        };
        let mut prev = best_point;
        loop {
            prev = prev_of(prev);
            let dist = (y(prev) - best_y).abs();
            if dist > 5 && (x(prev) - best_x).abs() <= 20 * dist {
                break;
            }
            seg_first = prev;
            if on(prev) {
                on_first = Some(prev);
                if on_last.is_none() {
                    on_last = Some(prev);
                }
            }
            if prev == best_point {
                break;
            }
        }
        let mut next = best_point;
        loop {
            next = next_of(next);
            let dist = (y(next) - best_y).abs();
            if dist > 5 && (x(next) - best_x).abs() <= 20 * dist {
                break;
            }
            seg_last = next;
            if on(next) {
                on_last = Some(next);
                if on_first.is_none() {
                    on_first = Some(next);
                }
            }
            if next == best_point {
                break;
            }
        }

        if props & PROP_LONG != 0 {
            // A long zone takes its height from a segment long enough to be a
            // real stroke, not a small bump such as a Hebrew letter's vertical
            // serif: if the extreme's own segment is short, the nearest long
            // one heading the same way within a quarter em is used instead.
            let length_threshold = units_per_em / 25;
            let dist = (x(seg_last) - x(seg_first)).abs();
            if dist < length_threshold
                && seg_last as i64 - seg_first as i64 + 2
                    <= contour_last as i64 - contour_first as i64
            {
                let height_threshold = units_per_em / 4;
                // Which way the outline runs through the extreme.
                let mut p = best_point;
                let mut found_dir = None;
                loop {
                    p = prev_of(p);
                    if x(p) != best_x {
                        found_dir = Some(x(p) < best_x);
                        break;
                    }
                    if p == best_point {
                        break;
                    }
                }
                // The degenerate case: no direction at all, and the glyph
                // is skipped as FreeType skips it.
                let left_to_right = found_dir?;
                let mut first = seg_last;
                let mut last = first;
                let mut hit = false;
                let (mut p_first, mut p_last): (Option<usize>, Option<usize>) = (None, None);
                loop {
                    if !hit {
                        first = last;
                        if on(first) {
                            p_first = Some(first);
                            p_last = Some(first);
                        } else {
                            p_first = None;
                            p_last = None;
                        }
                        hit = true;
                    }
                    last = next_of(last);
                    if (best_y - y(first)).abs() > height_threshold {
                        hit = false;
                    } else {
                        let dist = (y(last) - y(first)).abs();
                        if dist > 5 && (x(last) - x(first)).abs() <= 20 * dist {
                            hit = false;
                        } else {
                            if on(last) {
                                p_last = Some(last);
                                if p_first.is_none() {
                                    p_first = Some(last);
                                }
                            }
                            let l2r = x(first) < x(last);
                            let d = (x(last) - x(first)).abs();
                            if l2r == left_to_right && d >= length_threshold {
                                // Found: extend it to its end. (FreeType's test
                                // here reads the forward walk's stopping point
                                // and the outer loop's distance; so does this.)
                                loop {
                                    last = next_of(last);
                                    let d = (y(last) - y(first)).abs();
                                    if d > 5 && (x(next) - x(first)).abs() <= 20 * dist {
                                        last = prev_of(last);
                                        break;
                                    }
                                    p_last = Some(last);
                                    if on(last) {
                                        p_last = Some(last);
                                        if p_first.is_none() {
                                            p_first = Some(last);
                                        }
                                    }
                                    if last == seg_first {
                                        break;
                                    }
                                }
                                best_y = y(first);
                                seg_first = first;
                                seg_last = last;
                                on_first = p_first;
                                on_last = p_last;
                                break;
                            }
                        }
                    }
                    if last == seg_first {
                        break;
                    }
                }
            }
        }

        best_y += y_offset;

        // Flat if the on-curve points span more than a fourteenth of an em;
        // otherwise round if either end of the segment is a control point.
        let flat_threshold = units_per_em / 14;
        round = match (on_first, on_last) {
            (Some(a), Some(b)) if (x(b) - x(a)).abs() > flat_threshold => false,
            _ => !on(seg_first) || !on(seg_last),
        };
        if round && props & PROP_NEUTRAL != 0 {
            // Only flat segments measure a neutral zone.
            return None;
        }
    }
    Some((best_y, round))
}

/// Measure every zone of a style: `af_latin_metrics_init_blues`.
fn measure_blues(
    units_per_em: i64,
    strings: &[BlueString],
    source: &Source<'_>,
) -> Option<Vec<Blue>> {
    let mut blues: Vec<Blue> = Vec::new();
    for string in strings {
        let props = string.props;
        let top = props & (PROP_TOP | PROP_SUB_TOP) != 0;
        // Which of a cluster's glyphs wins: the highest for a top zone -- and,
        // as FreeType has it, the *lowest* for a sub-top one, whose glyphs'
        // own extremes are nonetheless their highest points.
        let highest_wins = props & PROP_TOP != 0;
        let (mut flats, mut rounds): (Vec<i64>, Vec<i64>) = (Vec::new(), Vec::new());
        let (mut ascender, mut descender) = (0i64, 0i64);
        for cluster in string.chars.split(' ').filter(|c| !c.is_empty()) {
            // The extreme of the cluster's glyphs taken together.
            let mut best: Option<(i64, bool)> = None;
            for (gid, y_offset) in (source.shape)(cluster) {
                if gid == 0 {
                    continue;
                }
                let Some(outline) = (source.outline)(gid) else {
                    continue;
                };
                // Glyphs that draw nothing, or next to nothing.
                if outline.points.len() <= 2 {
                    continue;
                }
                let Some((y, round)) = extreme(
                    &outline,
                    source.units,
                    i64::from(y_offset),
                    props,
                    units_per_em,
                    &mut ascender,
                    &mut descender,
                ) else {
                    continue;
                };
                let better = best.is_none_or(|(b, _)| if highest_wins { y > b } else { y < b });
                if better {
                    best = Some((y, round));
                }
            }
            match best {
                Some((y, true)) => rounds.push(y),
                Some((y, false)) => flats.push(y),
                None => {}
            }
        }
        if flats.is_empty() && rounds.is_empty() {
            continue;
        }
        flats.sort_unstable();
        rounds.sort_unstable();
        let median = |v: &[i64]| v.get(v.len() / 2).copied();
        let (reference, shoot) = match (median(&flats), median(&rounds)) {
            (None, Some(r)) => (r, r),
            (Some(f), None) => (f, f),
            (Some(f), Some(r)) => (f, r),
            (None, None) => continue,
        };
        // An overshoot on the wrong side of its reference is a measuring
        // accident: both take the mean.
        let (reference, shoot) = if shoot != reference && top != (shoot > reference) {
            // Rounded toward zero, as C's division rounds it.
            let mean = shoot.midpoint(reference);
            (mean, mean)
        } else {
            (reference, shoot)
        };
        let mut flags = 0;
        if props & PROP_TOP != 0 {
            flags |= BLUE_TOP;
        }
        if props & PROP_SUB_TOP != 0 {
            flags |= BLUE_SUB_TOP;
        }
        if props & PROP_NEUTRAL != 0 {
            flags |= BLUE_NEUTRAL;
        }
        if props & PROP_X_HEIGHT != 0 {
            flags |= BLUE_ADJUSTMENT;
        }
        blues.push(Blue {
            reference,
            shoot,
            ascender,
            descender,
            flags,
        });
    }
    if blues.is_empty() {
        return None;
    }

    // Zones must not overlap: sorted bottom to top (by their reference if a
    // top zone, their overshoot otherwise), each one's upper line is lowered
    // to the next one's lower line where it reaches past it.
    let key = |b: &Blue| {
        if b.flags & (BLUE_TOP | BLUE_SUB_TOP) != 0 {
            b.reference
        } else {
            b.shoot
        }
    };
    let mut order: Vec<usize> = (0..blues.len()).collect();
    // FreeType's insertion sort, whose tie order this keeps.
    for i in 1..order.len() {
        let mut j = i;
        while j > 0 {
            let (Some(&a), Some(&b)) = (order.get(j - 1), order.get(j)) else {
                break;
            };
            let (Some(ba), Some(bb)) = (blues.get(a), blues.get(b)) else {
                break;
            };
            if key(bb) >= key(ba) {
                break;
            }
            order.swap(j - 1, j);
            j -= 1;
        }
    }
    for w in 0..order.len().saturating_sub(1) {
        let (Some(&i), Some(&j)) = (order.get(w), order.get(w + 1)) else {
            continue;
        };
        let upper = |b: &Blue| {
            if b.flags & (BLUE_TOP | BLUE_SUB_TOP) != 0 {
                b.shoot
            } else {
                b.reference
            }
        };
        let (Some(bi), Some(bj)) = (blues.get(i).copied(), blues.get(j).copied()) else {
            continue;
        };
        let (a, b) = (upper(&bi), upper(&bj));
        if a > b
            && let Some(blue) = blues.get_mut(i)
        {
            if blue.flags & (BLUE_TOP | BLUE_SUB_TOP) != 0 {
                blue.shoot = b;
            } else {
                blue.reference = b;
            }
        }
    }
    Some(blues)
}

/// Hint one glyph of this style: segments, edges, blue zones, edge placement,
/// then every point: `af_latin_hints_apply` for the vertical dimension.
/// `nonbase` glyphs (marks and the like) are never snapped to a zone.
pub(super) fn hint(
    hints: &mut Hints,
    metrics: &Metrics,
    scaled: &Scaled,
    nonbase: bool,
) -> Option<()> {
    compute_segments(hints)?;
    link_segments(hints, &metrics.widths);
    compute_edges(hints, metrics)?;
    if !nonbase {
        compute_blue_edges(hints, metrics, scaled);
    }
    hint_edges(hints, metrics.top_to_bottom)?;
    hints.align_edge_points()?;
    hints.align_strong_points()?;
    hints.align_weak_points()
}

/// The glyph's horizontal segments: runs of points heading left or right,
/// each with its height and extent: `af_latin_hints_compute_segments`.
fn compute_segments(hints: &mut Hints) -> Option<()> {
    let flat_threshold = hints.units_per_em / 14;
    // For horizontal segments, `u` is the height and `v` the coordinate along.
    for p in &mut hints.points {
        p.u = p.fy;
        p.v = p.fx;
    }
    let major_dir = hints.major_dir.abs();
    hints.segments.clear();
    let blank = |first: usize, dir: i8| Segment {
        flags: 0,
        dir,
        pos: 0,
        delta: 0,
        min_coord: 0,
        max_coord: 0,
        height: 0,
        edge: None,
        edge_next: None,
        link: None,
        serif: None,
        score: 32000,
        first,
        last: first,
    };
    // `FT_Short`, as FreeType stores a segment's measures.
    let short = |v: i64| i64::from(v as i16);

    for c in 0..hints.contours.len() {
        let mut point = *hints.contours.get(c)?;
        let mut last = hints.points.get(point)?.prev;
        let mut on_edge = false;
        let (mut min_pos, mut max_pos) = (32000i64, -32000i64);
        let (mut min_coord, mut max_coord) = (32000i64, -32000i64);
        let (mut min_flags, mut max_flags) = (0u16, 0u16);
        let (mut min_on, mut max_on) = (32000i64, -32000i64);
        let mut segment: Option<usize> = None;
        let mut prev_segment: Option<usize> = None;
        let (mut prev_min_pos, mut prev_max_pos) = (min_pos, max_pos);
        let (mut prev_min_coord, mut prev_max_coord) = (min_coord, max_coord);
        let (mut prev_min_flags, mut prev_max_flags) = (min_flags, max_flags);
        let (mut prev_min_on, mut prev_max_on) = (min_on, max_on);
        let mut segment_dir = major_dir;

        // Starting in the middle of an edge: back up to its start.
        let is_major = |h: &Hints, i: usize| {
            h.points
                .get(i)
                .is_some_and(|p| p.out_dir.abs() == major_dir)
        };
        if is_major(hints, last) && is_major(hints, point) {
            last = point;
            let mut guard = hints.points.len();
            loop {
                point = hints.points.get(point)?.prev;
                if !is_major(hints, point) {
                    point = hints.points.get(point)?.next;
                    break;
                }
                if point == last {
                    break;
                }
                guard = guard.checked_sub(1)?;
            }
        }
        last = point;
        let mut passed = false;
        let mut guard = 2 * hints.points.len() + 2;

        loop {
            guard = guard.checked_sub(1)?;
            let p = *hints.points.get(point)?;
            if on_edge {
                min_pos = min_pos.min(p.u);
                max_pos = max_pos.max(p.u);
                if p.v < min_coord {
                    min_coord = p.v;
                    min_flags = p.flags;
                }
                if p.v > max_coord {
                    max_coord = p.v;
                    max_flags = p.flags;
                }
                if !p.is_control() {
                    min_on = min_on.min(p.v);
                    max_on = max_on.max(p.v);
                }
                if p.out_dir != segment_dir || point == last {
                    let seg_i = segment?;
                    let same_start = match prev_segment {
                        Some(ps) => {
                            hints.segments.get(seg_i)?.first == hints.segments.get(ps)?.last
                        }
                        None => false,
                    };
                    let round = |minf: u16, maxf: u16, lo: i64, hi: i64| {
                        (minf | maxf) & FLAG_CONTROL != 0 && hi - lo < flat_threshold
                    };
                    if !same_start {
                        // Leaving an edge: record the segment.
                        let seg = hints.segments.get_mut(seg_i)?;
                        seg.last = point;
                        seg.pos = short((min_pos + max_pos) >> 1);
                        seg.delta = short((max_pos - min_pos) >> 1);
                        if round(min_flags, max_flags, min_on, max_on) {
                            seg.flags |= EDGE_ROUND;
                        }
                        seg.min_coord = short(min_coord);
                        seg.max_coord = short(max_coord);
                        seg.height = short(seg.max_coord - seg.min_coord);
                        prev_segment = Some(seg_i);
                        (prev_min_pos, prev_max_pos) = (min_pos, max_pos);
                        (prev_min_coord, prev_max_coord) = (min_coord, max_coord);
                        (prev_min_flags, prev_max_flags) = (min_flags, max_flags);
                        (prev_min_on, prev_max_on) = (min_on, max_on);
                    } else {
                        // This segment starts where the last one ended (a
                        // spike, say): merge the two rather than keep both.
                        let ps = prev_segment?;
                        let prev_last = hints.segments.get(ps)?.last;
                        let same_dir = hints.points.get(prev_last)?.in_dir == p.in_dir;
                        if same_dir {
                            // A zig-zag along the axis: one segment.
                            min_pos = min_pos.min(prev_min_pos);
                            max_pos = max_pos.max(prev_max_pos);
                            if prev_min_coord < min_coord {
                                min_coord = prev_min_coord;
                                min_flags = prev_min_flags;
                            }
                            if prev_max_coord > max_coord {
                                max_coord = prev_max_coord;
                                max_flags = prev_max_flags;
                            }
                            min_on = min_on.min(prev_min_on);
                            max_on = max_on.max(prev_max_on);
                            let seg = hints.segments.get_mut(ps)?;
                            seg.last = point;
                            seg.pos = short((min_pos + max_pos) >> 1);
                            seg.delta = short((max_pos - min_pos) >> 1);
                            if round(min_flags, max_flags, min_on, max_on) {
                                seg.flags |= EDGE_ROUND;
                            } else {
                                seg.flags &= !EDGE_ROUND;
                            }
                            seg.min_coord = short(min_coord);
                            seg.max_coord = short(max_coord);
                            seg.height = short(seg.max_coord - seg.min_coord);
                        } else if (prev_max_coord - prev_min_coord).abs()
                            > (max_coord - min_coord).abs()
                        {
                            // Opposite directions: keep the longer. The
                            // previous one.
                            prev_min_pos = prev_min_pos.min(min_pos);
                            prev_max_pos = prev_max_pos.max(max_pos);
                            let seg = hints.segments.get_mut(ps)?;
                            seg.last = point;
                            seg.pos = short((prev_min_pos + prev_max_pos) >> 1);
                            seg.delta = short((prev_max_pos - prev_min_pos) >> 1);
                        } else {
                            // This one, in the previous one's place.
                            min_pos = min_pos.min(prev_min_pos);
                            max_pos = max_pos.max(prev_max_pos);
                            let mut seg = *hints.segments.get(seg_i)?;
                            seg.last = point;
                            seg.pos = short((min_pos + max_pos) >> 1);
                            seg.delta = short((max_pos - min_pos) >> 1);
                            if round(min_flags, max_flags, min_on, max_on) {
                                seg.flags |= EDGE_ROUND;
                            }
                            seg.min_coord = short(min_coord);
                            seg.max_coord = short(max_coord);
                            seg.height = short(seg.max_coord - seg.min_coord);
                            *hints.segments.get_mut(ps)? = seg;
                            (prev_min_pos, prev_max_pos) = (min_pos, max_pos);
                            (prev_min_coord, prev_max_coord) = (min_coord, max_coord);
                            (prev_min_flags, prev_max_flags) = (min_flags, max_flags);
                            (prev_min_on, prev_max_on) = (min_on, max_on);
                        }
                        // The current segment is the last one made; it goes.
                        hints.segments.pop();
                    }
                    on_edge = false;
                    segment = None;
                }
            }

            if point == last {
                if passed {
                    break;
                }
                passed = true;
            }

            let p = *hints.points.get(point)?;
            if !on_edge && (p.out_dir.abs() == major_dir || p.prev == point) {
                // FreeType gives up on a glyph of more than a thousand
                // segments: no pixel grid could show them.
                if hints.segments.len() > 1000 {
                    hints.segments.clear();
                    return Some(());
                }
                segment_dir = p.out_dir;
                let mut seg = blank(point, segment_dir);
                min_pos = p.u;
                max_pos = p.u;
                min_coord = p.v;
                max_coord = p.v;
                min_flags = p.flags;
                max_flags = p.flags;
                if p.is_control() {
                    min_on = 32000;
                    max_on = -32000;
                } else {
                    min_on = p.v;
                    max_on = p.v;
                }
                on_edge = true;
                if p.prev == point {
                    // A one-point contour: a one-point segment of no
                    // direction.
                    seg.pos = short(min_pos);
                    if p.is_control() {
                        seg.flags |= EDGE_ROUND;
                    }
                    seg.min_coord = short(p.v);
                    seg.max_coord = short(p.v);
                    seg.height = 0;
                    on_edge = false;
                    hints.segments.push(seg);
                    segment = None;
                } else {
                    hints.segments.push(seg);
                    segment = Some(hints.segments.len() - 1);
                }
            }
            point = hints.points.get(point)?.next;
        }
    }

    // Lengthen a segment by half of what its neighbours add, the better to
    // tell a serif from a stem.
    for i in 0..hints.segments.len() {
        let seg = *hints.segments.get(i)?;
        let (first, last) = (*hints.points.get(seg.first)?, *hints.points.get(seg.last)?);
        let (before, after) = (
            *hints.points.get(first.prev)?,
            *hints.points.get(last.next)?,
        );
        // Each step truncated to 16 bits, as FreeType stores it.
        let mut height = seg.height;
        if first.v < last.v {
            if before.v < first.v {
                height = short(height + ((first.v - before.v) >> 1));
            }
            if after.v > last.v {
                height = short(height + ((after.v - last.v) >> 1));
            }
        } else {
            if before.v > first.v {
                height = short(height + ((before.v - first.v) >> 1));
            }
            if after.v < last.v {
                height = short(height + ((last.v - after.v) >> 1));
            }
        }
        hints.segments.get_mut(i)?.height = height;
    }
    Some(())
}

/// Pair each bottom segment with the top segment across the stem it bounds,
/// best-scoring first, and mark the unpaired as serifs:
/// `af_latin_hints_link_segments`. `widths` are the style's standard widths,
/// empty when measuring them.
fn link_segments(hints: &mut Hints, widths: &[i64]) {
    let max_width = widths.last().copied().unwrap_or(0);
    let len_threshold = constant(hints.units_per_em, 8).max(1);
    let len_score = constant(hints.units_per_em, 6000);
    let dist_score = 3000;
    let major = hints.major_dir;
    let n = hints.segments.len();
    for i in 0..n {
        let Some(&seg1) = hints.segments.get(i) else {
            continue;
        };
        if seg1.dir != major {
            continue;
        }
        for j in 0..n {
            let Some(&seg2) = hints.segments.get(j) else {
                continue;
            };
            let (pos1, pos2) = (seg1.pos, seg2.pos);
            if seg1.dir + seg2.dir != 0 || pos2 <= pos1 {
                continue;
            }
            let min = seg1.min_coord.max(seg2.min_coord);
            let max = seg1.max_coord.min(seg2.max_coord);
            let len = max - min;
            if len < len_threshold {
                continue;
            }
            // Demerits: for overlapping little, and for being wider than the
            // widest standard stem.
            let dist = pos2 - pos1;
            let dist_demerit = if max_width != 0 {
                let delta = (dist << 10) / max_width - (1 << 10);
                if delta > 10000 {
                    32000
                } else if delta > 0 {
                    delta * delta / dist_score
                } else {
                    0
                }
            } else {
                dist
            };
            let score = dist_demerit + len_score / len;
            // The segments' scores change as the loops run, so each is read
            // afresh.
            if let Some(s1) = hints.segments.get_mut(i)
                && score < s1.score
            {
                s1.score = score;
                s1.link = Some(j);
            }
            if let Some(s2) = hints.segments.get_mut(j)
                && score < s2.score
            {
                s2.score = score;
                s2.link = Some(i);
            }
        }
    }
    // A segment whose partner prefers another is a serif of that one.
    for i in 0..n {
        let Some(link) = hints.segments.get(i).and_then(|s| s.link) else {
            continue;
        };
        let Some(back) = hints.segments.get(link).map(|s| s.link) else {
            continue;
        };
        if back != Some(i)
            && let Some(s) = hints.segments.get_mut(i)
        {
            s.link = None;
            s.serif = back;
        }
    }
}

/// Gather segments at one height into edges, sorted by height, and link the
/// edges as their segments are linked: `af_latin_hints_compute_edges`.
fn compute_edges(hints: &mut Hints, metrics: &Metrics) -> Option<()> {
    let scale = hints.y_scale;
    hints.edges.clear();
    // Segments wider than half a pixel are no edge.
    let width_threshold = div_fix(32, scale);
    // Joined into one edge within a fifth of the standard width, and a
    // quarter pixel at most.
    let distance = mul_fix(metrics.edge_distance_threshold, scale).min(64 / 4);
    let distance = div_fix(distance, scale);
    let top_to_bottom = metrics.top_to_bottom;

    for s in 0..hints.segments.len() {
        let seg = *hints.segments.get(s)?;
        // (Vertically there is no minimum segment length.)
        if seg.height < 0 || seg.delta > width_threshold || seg.dir == DIR_NONE {
            continue;
        }
        if seg.serif.is_some() && 2 * seg.height < 0 {
            continue;
        }
        let found = hints
            .edges
            .iter()
            .position(|e| (seg.pos - e.fpos).abs() < distance && e.dir == seg.dir);
        match found {
            Some(e) => {
                // Into the edge's ring of segments.
                let (first, last) = {
                    let edge = hints.edges.get(e)?;
                    (edge.first, edge.last)
                };
                hints.segments.get_mut(s)?.edge_next = Some(first);
                hints.segments.get_mut(last)?.edge_next = Some(s);
                hints.edges.get_mut(e)?.last = s;
            }
            None => {
                let at = insertion_point(
                    &hints.edges,
                    seg.pos,
                    seg.dir,
                    hints.major_dir,
                    top_to_bottom,
                );
                let opos = mul_fix(seg.pos, scale);
                hints.edges.insert(
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
                hints.segments.get_mut(s)?.edge_next = Some(s);
            }
        }
    }
    // One-point segments join an edge they are near, of either direction.
    for s in 0..hints.segments.len() {
        let seg = *hints.segments.get(s)?;
        if seg.dir != DIR_NONE {
            continue;
        }
        let Some(e) = hints
            .edges
            .iter()
            .position(|e| (seg.pos - e.fpos).abs() < distance)
        else {
            continue;
        };
        let (first, last) = {
            let edge = hints.edges.get(e)?;
            (edge.first, edge.last)
        };
        hints.segments.get_mut(s)?.edge_next = Some(first);
        hints.segments.get_mut(last)?.edge_next = Some(s);
        hints.edges.get_mut(e)?.last = s;
    }

    // Each segment learns its edge.
    for e in 0..hints.edges.len() {
        let first = hints.edges.get(e)?.first;
        let mut s = first;
        let mut guard = hints.segments.len() + 1;
        loop {
            let seg = hints.segments.get_mut(s)?;
            seg.edge = Some(e);
            s = seg.edge_next?;
            if s == first {
                break;
            }
            guard = guard.checked_sub(1)?;
        }
    }

    // Each edge's roundness and links, from its segments'.
    for e in 0..hints.edges.len() {
        let (mut is_round, mut is_straight) = (0u32, 0u32);
        let first = hints.edges.get(e)?.first;
        let mut s = first;
        let mut guard = hints.segments.len() + 1;
        loop {
            let seg = *hints.segments.get(s)?;
            if seg.flags & EDGE_ROUND != 0 {
                is_round += 1;
            } else {
                is_straight += 1;
            }
            let serif_edge = seg
                .serif
                .and_then(|x| hints.segments.get(x))
                .and_then(|x| x.edge);
            let is_serif = serif_edge.is_some_and(|x| x != e);
            let link_edge = seg
                .link
                .and_then(|x| hints.segments.get(x))
                .and_then(|x| x.edge);
            if link_edge.is_some() || is_serif {
                let edge = *hints.edges.get(e)?;
                let (seg2, current) = if is_serif {
                    (seg.serif?, edge.serif)
                } else {
                    (seg.link?, edge.link)
                };
                let seg2_edge = hints.segments.get(seg2)?.edge;
                let edge2 = match current {
                    Some(c) => {
                        let edge_delta = (edge.fpos - hints.edges.get(c)?.fpos).abs();
                        let seg_delta = (seg.pos - hints.segments.get(seg2)?.pos).abs();
                        if seg_delta < edge_delta {
                            seg2_edge
                        } else {
                            Some(c)
                        }
                    }
                    None => seg2_edge,
                };
                if is_serif {
                    hints.edges.get_mut(e)?.serif = edge2;
                    if let Some(e2) = edge2 {
                        hints.edges.get_mut(e2)?.flags |= EDGE_SERIF;
                    }
                } else {
                    hints.edges.get_mut(e)?.link = edge2;
                }
            }
            s = seg.edge_next?;
            if s == first {
                break;
            }
            guard = guard.checked_sub(1)?;
        }
        let edge = hints.edges.get_mut(e)?;
        // Assigned, not or-ed: FreeType's serif mark on an edge already
        // processed survives, on one still to come it does not.
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

/// Where a new edge at `fpos` goes in the sorted list: after every edge below
/// it (above, hinting top down), and, at an equal height, after the edges of
/// the minor direction but before the major's: `af_axis_hints_new_edge`.
fn insertion_point(
    edges: &[Edge],
    fpos: i64,
    dir: i8,
    major_dir: i8,
    top_to_bottom: bool,
) -> usize {
    let mut at = edges.len();
    while at > 0 {
        let Some(prev) = edges.get(at - 1) else {
            break;
        };
        let before = if top_to_bottom {
            prev.fpos > fpos
        } else {
            prev.fpos < fpos
        };
        if before || (prev.fpos == fpos && dir == major_dir) {
            break;
        }
        at -= 1;
    }
    at
}

/// Find the blue zone, if any, each edge snaps to:
/// `af_latin_hints_compute_blue_edges`.
fn compute_blue_edges(hints: &mut Hints, metrics: &Metrics, scaled: &Scaled) {
    let scale = scaled.y_scale;
    // Within a fortieth of an em, and half a pixel at most.
    let threshold = mul_fix(metrics.units_per_em / 40, scale).min(64 / 2);
    let major = hints.major_dir;
    for edge in &mut hints.edges {
        let mut best: Option<i64> = None;
        let mut best_dist = threshold;
        let mut best_neutral = false;
        for (blue, fitted) in metrics.blues.iter().zip(&scaled.blues) {
            if fitted.flags & BLUE_ACTIVE == 0 {
                continue;
            }
            // A top zone catches edges with ink below them -- running against
            // the major direction -- and a bottom zone the opposite; a
            // neutral zone catches both.
            let is_top = fitted.flags & (BLUE_TOP | BLUE_SUB_TOP) != 0;
            let is_neutral = fitted.flags & BLUE_NEUTRAL != 0;
            let is_major = edge.dir == major;
            if !(is_top != is_major || is_neutral) {
                continue;
            }
            let dist = mul_fix((edge.fpos - blue.reference).abs(), scale);
            if dist < best_dist {
                best_dist = dist;
                best = Some(fitted.reference);
                best_neutral = is_neutral;
            }
            // A round edge beyond the reference may belong to the overshoot.
            if edge.flags & EDGE_ROUND != 0 && dist != 0 && !is_neutral {
                let is_under_ref = edge.fpos < blue.reference;
                if is_top != is_under_ref {
                    let dist = mul_fix((edge.fpos - blue.shoot).abs(), scale);
                    if dist < best_dist {
                        best_dist = dist;
                        best = Some(fitted.shoot);
                        best_neutral = is_neutral;
                    }
                }
            }
        }
        if best.is_some() {
            edge.blue = best;
            if best_neutral {
                edge.flags |= EDGE_NEUTRAL;
            }
        }
    }
}

/// A stem's second edge, placed its original distance from the first: in
/// light mode a stem keeps its width (`af_latin_align_linked_edge`, where
/// `af_latin_compute_stem_width` changes nothing).
fn align_linked_edge(edges: &mut [Edge], base: usize, stem: usize) -> Option<()> {
    let b = *edges.get(base)?;
    let s = edges.get_mut(stem)?;
    s.pos = b.pos + (s.opos - b.opos);
    Some(())
}

/// Place every edge: `af_latin_hint_edges` for the vertical dimension in light
/// mode.
fn hint_edges(hints: &mut Hints, top_to_bottom: bool) -> Option<()> {
    let edges = &mut hints.edges;
    let n = edges.len();
    let mut anchor: Option<usize> = None;
    let mut has_serifs = false;

    // Edges in blue zones, and the stems they bound.
    for e in 0..n {
        let edge = *edges.get(e)?;
        if edge.flags & EDGE_DONE != 0 {
            continue;
        }
        let mut edge2 = edge.link;
        // A stem with both edges in zones keeps the non-neutral one; with two
        // neutral ones, the first.
        if edge.blue.is_some()
            && let Some(e2) = edge2
            && edges.get(e2)?.blue.is_some()
        {
            let neutral2 = edges.get(e2)?.flags & EDGE_NEUTRAL != 0;
            if neutral2 {
                let x = edges.get_mut(e2)?;
                x.blue = None;
                x.flags &= !EDGE_NEUTRAL;
            } else if edge.flags & EDGE_NEUTRAL != 0 {
                let x = edges.get_mut(e)?;
                x.blue = None;
                x.flags &= !EDGE_NEUTRAL;
            }
        }
        let edge = *edges.get(e)?;
        let (edge1, blue) = if let Some(b) = edge.blue {
            (e, b)
        } else if let Some(e2) = edge2
            && let Some(b) = edges.get(e2)?.blue
        {
            // The linked edge is the one in the zone.
            edge2 = Some(e);
            (e2, b)
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
        if anchor.is_none() {
            anchor = Some(e);
        }
    }

    // The other stems, keeping the stems' order.
    for e in 0..n {
        let edge = *edges.get(e)?;
        if edge.flags & EDGE_DONE != 0 {
            continue;
        }
        let Some(e2) = edge.link else {
            has_serifs = true;
            continue;
        };
        let edge2 = *edges.get(e2)?;
        if edge2.blue.is_some() {
            // Cannot happen, FreeType notes; handled all the same.
            align_linked_edge(edges, e2, e)?;
            edges.get_mut(e)?.flags |= EDGE_DONE;
            continue;
        }
        match anchor {
            None => {
                // The first stem: its centre on a pixel boundary or a pixel's
                // middle, whichever suits its width, or its lower edge on a
                // boundary if it is wide.
                let org_len = edge2.opos - edge.opos;
                let cur_len = org_len;
                let (u_off, d_off) = if cur_len <= 64 { (32, 32) } else { (38, 26) };
                if cur_len < 96 {
                    let org_center = edge.opos + (org_len >> 1);
                    let mut cur_pos1 = pix_round(org_center);
                    let error1 = (org_center - (cur_pos1 - u_off)).abs();
                    let error2 = (org_center - (cur_pos1 + d_off)).abs();
                    if error1 < error2 {
                        cur_pos1 -= u_off;
                    } else {
                        cur_pos1 += d_off;
                    }
                    let pos = cur_pos1 - cur_len / 2;
                    edges.get_mut(e)?.pos = pos;
                    edges.get_mut(e2)?.pos = pos + cur_len;
                } else {
                    edges.get_mut(e)?.pos = pix_round(edge.opos);
                }
                anchor = Some(e);
                edges.get_mut(e)?.flags |= EDGE_DONE;
                align_linked_edge(edges, e, e2)?;
            }
            Some(a) => {
                let anchor_edge = *edges.get(a)?;
                let org_pos = anchor_edge.pos + (edge.opos - anchor_edge.opos);
                let org_len = edge2.opos - edge.opos;
                let org_center = org_pos + (org_len >> 1);
                let cur_len = org_len;
                if edge2.flags & EDGE_DONE != 0 {
                    edges.get_mut(e)?.pos = edge2.pos - cur_len;
                } else if cur_len < 96 {
                    let mut cur_pos1 = pix_round(org_center);
                    let (u_off, d_off) = if cur_len <= 64 { (32, 32) } else { (38, 26) };
                    let delta1 = (org_center - (cur_pos1 - u_off)).abs();
                    let delta2 = (org_center - (cur_pos1 + d_off)).abs();
                    if delta1 < delta2 {
                        cur_pos1 -= u_off;
                    } else {
                        cur_pos1 += d_off;
                    }
                    edges.get_mut(e)?.pos = cur_pos1 - cur_len / 2;
                    edges.get_mut(e2)?.pos = cur_pos1 + cur_len / 2;
                } else {
                    // Round one edge or the other, whichever keeps the stem's
                    // centre nearer where it was.
                    let cur_pos1 = pix_round(org_pos);
                    let delta1 = (cur_pos1 + (cur_len >> 1) - org_center).abs();
                    let cur_pos2 = pix_round(org_pos + org_len) - cur_len;
                    let delta2 = (cur_pos2 + (cur_len >> 1) - org_center).abs();
                    let pos = if delta1 < delta2 { cur_pos1 } else { cur_pos2 };
                    edges.get_mut(e)?.pos = pos;
                    edges.get_mut(e2)?.pos = pos + cur_len;
                }
                edges.get_mut(e)?.flags |= EDGE_DONE;
                edges.get_mut(e2)?.flags |= EDGE_DONE;
                bound_below(edges, e, top_to_bottom)?;
            }
        }
    }

    if has_serifs || anchor.is_none() {
        // Serifs and lone edges: with the edge they serif, interpolated
        // between placed edges, or rounded.
        for e in 0..n {
            let edge = *edges.get(e)?;
            if edge.flags & EDGE_DONE != 0 {
                continue;
            }
            let serif_dist = match edge.serif {
                Some(s) => (edges.get(s)?.opos - edge.opos).abs(),
                None => 1000,
            };
            if serif_dist < 64 + 16
                && let Some(s) = edge.serif
            {
                let base = *edges.get(s)?;
                edges.get_mut(e)?.pos = base.pos + (edge.opos - base.opos);
            } else if anchor.is_none() {
                edges.get_mut(e)?.pos = pix_round(edge.opos);
                anchor = Some(e);
            } else {
                let before = (0..e)
                    .rev()
                    .find(|&i| edges.get(i).is_some_and(|x| x.flags & EDGE_DONE != 0));
                let after =
                    (e + 1..n).find(|&i| edges.get(i).is_some_and(|x| x.flags & EDGE_DONE != 0));
                let pos = match (before, after) {
                    (Some(b), Some(a)) => {
                        let (b, a) = (*edges.get(b)?, *edges.get(a)?);
                        if a.opos == b.opos {
                            b.pos
                        } else {
                            b.pos + mul_div(edge.opos - b.opos, a.pos - b.pos, a.opos - b.opos)
                        }
                    }
                    _ => {
                        let a = *edges.get(anchor?)?;
                        a.pos + ((edge.opos - a.opos + 16) & !31)
                    }
                };
                edges.get_mut(e)?.pos = pos;
            }
            edges.get_mut(e)?.flags |= EDGE_DONE;
            bound_below(edges, e, top_to_bottom)?;
            // And not past the next edge, if that is placed. (FreeType's
            // test compares against the edge *below*, as here.)
            if let Some(next) = edges.get(e + 1).copied()
                && next.flags & EDGE_DONE != 0
            {
                let x = *edges.get(e)?;
                let past = if top_to_bottom {
                    x.pos < next.pos
                } else {
                    x.pos > next.pos
                };
                if past
                    && let Some(link) = x.link
                    && e > 0
                {
                    let below = edges.get(e - 1)?.pos;
                    if (edges.get(link)?.pos - below).abs() > 16 {
                        edges.get_mut(e)?.pos = next.pos;
                    }
                }
            }
        }
    }
    Some(())
}

/// Keep edge `e` from passing the edge before it in the sorted order, unless
/// that would all but collapse its stem (a quarter pixel).
fn bound_below(edges: &mut [Edge], e: usize, top_to_bottom: bool) -> Option<()> {
    if e == 0 {
        return Some(());
    }
    let prev = *edges.get(e - 1)?;
    let x = *edges.get(e)?;
    let past = if top_to_bottom {
        x.pos > prev.pos
    } else {
        x.pos < prev.pos
    };
    if past
        && let Some(link) = x.link
        && (edges.get(link)?.pos - prev.pos).abs() > 16
    {
        edges.get_mut(e)?.pos = prev.pos;
    }
    Some(())
}
