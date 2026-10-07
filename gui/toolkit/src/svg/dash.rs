//! Dashed strokes: `stroke-dasharray`, `stroke-dashoffset` and `pathLength`.
//!
//! # What is drawn
//!
//! - **The pattern**: lengths -- user units, `px`, or percentages of the
//!   viewport's normalised diagonal -- separated by commas or spaces, a dash,
//!   then a gap, then a dash... An odd count is repeated to make an even one,
//!   as SVG says. `none`, or a list whose lengths sum to nought, is a solid
//!   stroke; a list with a negative or unreadable length is not said, and the
//!   stroke is dashed as its parent's is.
//! - **The offset**: how far into the pattern the stroke starts, negative
//!   too, in the same units.
//! - **`pathLength`**: the length the author says the shape has; the pattern
//!   and the offset are scaled by the length it has over it. Nought scales
//!   them without end: a length of nought stays nought, any other is longer
//!   than the shape.
//! - **Where it starts**: again at each subpath. Each dash is an open run,
//!   capped as the stroke's ends are; a dash of no length is a dot under a
//!   round or square cap, turned along the path, and nothing under a butt
//!   one. A closed subpath that one dash covers whole is stroked closed.
//! - **Measured in user space**: a dash is as long as the pattern says in
//!   the shape's own units, along whichever axis the transform stretches --
//!   or in the host's, for a non-scaling stroke.
//! - **Too many**: a shape with more than [`MAX_DASHES`] dashes is stroked
//!   solid, as Skia does past its own limit, rather than spending without end
//!   on dashes too small to see.

use std::sync::Arc;

use super::{Dashes, LineCap, Subpath, viewport_length};

/// The most dashes one shape is cut into before its stroke is drawn solid.
pub(super) const MAX_DASHES: usize = 1 << 16;

/// How long, in pixels, a dash of no length is drawn: long enough to give
/// its caps a direction, too short for any coverage of its own.
const DOT_PX: f32 = 1e-3;

/// What `stroke-dasharray` says, in a viewport `viewport` wide and high:
/// `None` where it says nothing this renderer can read, which inherits.
pub(super) fn dasharray(value: &str, viewport: (f32, f32)) -> Option<Dashes> {
    let value = value.trim();
    if value == "none" {
        return Some(Dashes::Solid);
    }
    let diagonal = normalised_diagonal(viewport);
    let mut lengths = Vec::new();
    for part in value.split(|c: char| c == ',' || c.is_ascii_whitespace()) {
        if part.is_empty() {
            continue;
        }
        let length = viewport_length(part, diagonal)?;
        if length.is_nan() || length < 0.0 {
            return None;
        }
        lengths.push(length);
    }
    if lengths.is_empty() {
        return None;
    }
    if !lengths.len().is_multiple_of(2) {
        lengths.extend_from_within(..);
    }
    let sum: f32 = lengths.iter().sum();
    if !(sum > 0.0 && sum.is_finite()) {
        return Some(Dashes::Solid);
    }
    Some(Dashes::Pattern(Arc::from(lengths)))
}

/// What `stroke-dashoffset` says: a length, negative too, or a percentage
/// of the viewport's normalised diagonal.
pub(super) fn dashoffset(value: &str, viewport: (f32, f32)) -> Option<f32> {
    viewport_length(value, normalised_diagonal(viewport))
}

/// What a percentage of a length with no direction is of: the viewport's
/// diagonal over the square root of two.
fn normalised_diagonal((w, h): (f32, f32)) -> f32 {
    f32::midpoint(w * w, h * h).sqrt()
}

/// What `pathLength` says: a length not negative.
pub(super) fn path_length(value: &str) -> Option<f32> {
    value
        .trim()
        .parse::<f32>()
        .ok()
        .filter(|v| v.is_finite() && *v >= 0.0)
}

/// How a dashed stroke is cut.
pub(super) struct Dashing<'p, M> {
    /// The pattern, in the units `measure` answers in.
    pub(super) pattern: &'p [f32],
    /// How far into the pattern each subpath starts.
    pub(super) offset: f32,
    /// What `pathLength` said, if anything.
    pub(super) path_length: Option<f32>,
    /// The length a step between two device points stands for.
    pub(super) measure: M,
    /// The stroke's caps, which decide whether a dash of no length is drawn.
    pub(super) cap: LineCap,
}

impl<M: Fn((f32, f32)) -> f32> Dashing<'_, M> {
    /// `subpaths`, in device pixels, cut into dashes; `None` where the
    /// stroke is drawn solid instead: a pattern with nothing to repeat, or
    /// more than [`MAX_DASHES`] dashes.
    pub(super) fn cut(&self, subpaths: &[Subpath]) -> Option<Vec<Subpath>> {
        let lengths: Vec<Vec<f32>> = subpaths
            .iter()
            .map(|sub| {
                segments(sub)
                    .map(|(a, b)| (self.measure)(sub_vec(b, a)))
                    .collect()
            })
            .collect();
        let total: f32 = lengths.iter().flatten().sum();
        // Scaled by the length the shape has over the length it is said to.
        let ratio = match self.path_length {
            Some(said) if said > 0.0 => total / said,
            Some(_) => f32::INFINITY,
            None => 1.0,
        };
        let scale = |v: f32| if v == 0.0 { 0.0 } else { v * ratio };
        let pattern: Vec<f32> = self.pattern.iter().map(|&v| scale(v)).collect();
        let period: f32 = pattern.iter().sum();
        if period.is_nan() || period <= 0.0 || pattern.is_empty() {
            return None;
        }
        // More dashes than are worth cutting: drawn solid.
        if period.is_finite() {
            #[allow(
                clippy::cast_precision_loss,
                reason = "a pattern's length, a handful; and the cap, exact in f32"
            )]
            let (count, cap) = (total / period * pattern.len() as f32, MAX_DASHES as f32);
            if count.is_nan() || count > cap {
                return None;
            }
        }
        let offset = scale(self.offset);
        let mut out = Vec::new();
        for (sub, lengths) in subpaths.iter().zip(&lengths) {
            self.cut_one(sub, lengths, &pattern, period, offset, &mut out);
            if out.len() > MAX_DASHES {
                return None;
            }
        }
        Some(out)
    }

    /// One subpath cut into `out`, the pattern starting `offset` into it.
    fn cut_one(
        &self,
        sub: &Subpath,
        lengths: &[f32],
        pattern: &[f32],
        period: f32,
        offset: f32,
        out: &mut Vec<Subpath>,
    ) {
        let Some(&start) = sub.points.first() else {
            return;
        };
        // Where in the pattern the subpath starts: the element, and how much
        // of it is left.
        let mut at = if period.is_finite() {
            offset.rem_euclid(period)
        } else {
            0.0
        };
        let mut index = 0usize;
        let mut left = pattern.first().copied().unwrap_or(0.0);
        // Walk past the elements the offset skips: each runs from where it
        // starts up to where the next does, so one ending exactly at the
        // offset is skipped -- except one of no length, a dot, which is where
        // it starts. Less than a period, so at most the pattern's length.
        for _ in 0..pattern.len() {
            let passed = if left > 0.0 { at >= left } else { at > 0.0 };
            if !passed || !at.is_finite() {
                break;
            }
            at -= left;
            index = next(index, pattern.len());
            left = pattern.get(index).copied().unwrap_or(0.0);
        }
        // A rounding can leave the offset a hair past the last element.
        left = (left - at).max(0.0);
        // Even elements are dashes, odd ones gaps.
        let mut dash: Option<Vec<(f32, f32)>> = index.is_multiple_of(2).then(|| vec![start]);
        // Whether no element ended inside the subpath.
        let mut whole = true;
        // The way the path last went, for a dot's caps.
        let mut heading = (1.0f32, 0.0f32);
        for ((a, b), &length) in segments(sub).zip(lengths) {
            let step = sub_vec(b, a);
            if step.0 != 0.0 || step.1 != 0.0 {
                heading = step;
            }
            let mut done = 0.0f32;
            // Each pass ends an element inside the segment; the pattern's sum
            // is more than nought, so a period of passes moves on along it.
            loop {
                if left > length - done || length.is_nan() || length <= 0.0 {
                    left -= (length - done).max(0.0);
                    // Not a second time, for a dash that began at this end.
                    if let Some(points) = dash.as_mut()
                        && points.last() != Some(&b)
                    {
                        points.push(b);
                    }
                    break;
                }
                done += left;
                let p = lerp(a, b, done / length);
                if let Some(mut points) = dash.take() {
                    points.push(p);
                    self.emit(points, heading, out);
                }
                whole = false;
                index = next(index, pattern.len());
                left = pattern.get(index).copied().unwrap_or(0.0);
                if index.is_multiple_of(2) {
                    dash = Some(vec![p]);
                }
                if out.len() > MAX_DASHES {
                    return;
                }
            }
        }
        if let Some(points) = dash {
            if sub.closed && whole {
                // One dash covering the whole of a closed subpath: the stroke
                // is the subpath's own, joined where it closes.
                out.push(sub.clone());
            } else {
                self.emit(points, heading, out);
            }
        }
    }

    /// One dash, its points `points`, onto `out`: as it is, or -- for one of
    /// no length -- as a dot facing `direction`, or as nothing under a butt
    /// cap.
    fn emit(&self, points: Vec<(f32, f32)>, direction: (f32, f32), out: &mut Vec<Subpath>) {
        let moves = points
            .windows(2)
            .any(|w| matches!(*w, [a, b] if (b.0 - a.0).hypot(b.1 - a.1) > DOT_PX));
        if moves {
            out.push(Subpath {
                points,
                closed: false,
            });
            return;
        }
        if self.cap == LineCap::Butt {
            return;
        }
        let Some(&at) = points.first() else {
            return;
        };
        let len = direction.0.hypot(direction.1);
        let d = if len > 0.0 && len.is_finite() {
            (direction.0 / len, direction.1 / len)
        } else {
            (1.0, 0.0)
        };
        out.push(Subpath {
            points: vec![at, (at.0 + d.0 * DOT_PX, at.1 + d.1 * DOT_PX)],
            closed: false,
        });
    }
}

/// The element after `index` in a pattern of `len`, round to the first.
fn next(index: usize, len: usize) -> usize {
    index.checked_add(1).filter(|&n| n < len).unwrap_or(0)
}

/// A subpath's segments, the closing one included where it is closed.
fn segments(sub: &Subpath) -> impl Iterator<Item = ((f32, f32), (f32, f32))> + '_ {
    let closing = if sub.closed {
        match sub.points.as_slice() {
            [first, .., last] => Some((*last, *first)),
            _ => None,
        }
    } else {
        None
    };
    sub.points
        .windows(2)
        .filter_map(|w| match *w {
            [a, b] => Some((a, b)),
            _ => None,
        })
        .chain(closing)
}

fn sub_vec(a: (f32, f32), b: (f32, f32)) -> (f32, f32) {
    (a.0 - b.0, a.1 - b.1)
}

fn lerp(a: (f32, f32), b: (f32, f32), t: f32) -> (f32, f32) {
    (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t)
}

#[cfg(test)]
#[path = "dash_tests.rs"]
mod tests;
