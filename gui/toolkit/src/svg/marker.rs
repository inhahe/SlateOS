//! Markers: the arrowheads and dots that `marker-start`, `marker-mid` and
//! `marker-end` put on the vertices of a path, line, polyline or polygon.
//!
//! # What is drawn
//!
//! - **Where**: `marker-start` on a shape's first vertex, `marker-end` on its
//!   last, `marker-mid` on every other -- across all its subpaths, so the
//!   start of a second subpath is a middle vertex. A closed subpath's
//!   closing vertex is a vertex of its own, where the subpath began. The
//!   properties are inherited, and `marker` sets all three from a `style`.
//! - **Facing**: `orient="auto"` turns a marker to the direction of the path
//!   at its vertex -- half-way between the way in and the way out where it
//!   has both, the way out at a start and the way in at an end, and at a
//!   closed subpath's first and last vertex both ways round the loop.
//!   `auto-start-reverse` turns the start marker about; an angle fixes it.
//!   A segment of no length has no direction, and the nearest that has one
//!   is used.
//! - **Size**: `markerWidth` and `markerHeight` (3 by default; nought hides
//!   the marker) in the stroke's widths (`markerUnits="strokeWidth"`, the
//!   default) or in user units; a `viewBox` fitted into them by
//!   `preserveAspectRatio`; `refX` and `refY` -- numbers in the view box's
//!   units, or SVG 2's `left`, `center`, `right`, `top`, `bottom` -- the
//!   point set on the vertex.
//! - **Cut**: to the marker's viewport, unless it says `overflow: visible`
//!   (or `auto`).
//! - **Content**: drawn as the `<marker>`'s children, inheriting from the
//!   marker and not from the shape it marks, after the shape's fill and
//!   stroke.
//!
//! A marker reference to nothing draws nothing. A marker whose content marks
//! with itself is drawn once, not without end, and everything drawn inside
//! markers counts toward the nodes a drawing may draw through `<use>`.

use super::{AspectRatio, PathCommand, SvgNode, XmlElement, parse_viewbox, property};

/// Which way a marker faces: `orient`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Orient {
    /// The path's direction at the vertex.
    Auto,
    /// The path's direction, turned about at the start.
    AutoStartReverse,
    /// A fixed turn, in radians.
    Angle(f32),
}

/// `refX` or `refY`: a coordinate in the view box's units, or SVG 2's
/// keyword for a place along the box -- 0 its start, 0.5 its middle, 1 its
/// end.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Reference {
    At(f32),
    Along(f32),
}

/// A `<marker>`, built.
#[derive(Clone, Debug)]
pub(super) struct MarkerDef {
    /// `markerUnits="strokeWidth"`: its size is in the marked stroke's
    /// widths.
    pub(super) in_stroke_widths: bool,
    pub(super) view_box: Option<(f32, f32, f32, f32)>,
    pub(super) aspect: AspectRatio,
    pub(super) reference: (Reference, Reference),
    /// `markerWidth` and `markerHeight`.
    pub(super) size: (f32, f32),
    pub(super) orient: Orient,
    /// Whether what it draws is cut to its viewport.
    pub(super) clip: bool,
    /// What it draws.
    pub(super) content: Vec<SvgNode>,
}

/// The parts of a `<marker>` that are not its content.
pub(super) fn marker_frame(elem: &XmlElement) -> MarkerDef {
    let number = |name: &str, default: f32| {
        elem.attr(name)
            .and_then(|v| v.trim().parse::<f32>().ok())
            .filter(|v| v.is_finite())
            .unwrap_or(default)
    };
    let reference = |name: &str, keywords: [(&str, f32); 3]| {
        let value = elem.attr(name).map(str::trim).unwrap_or("0");
        keywords
            .iter()
            .find(|(word, _)| *word == value)
            .map(|&(_, along)| Reference::Along(along))
            .or_else(|| {
                value
                    .parse::<f32>()
                    .ok()
                    .filter(|v| v.is_finite())
                    .map(Reference::At)
            })
            .unwrap_or(Reference::At(0.0))
    };
    let orient = match elem.attr("orient").map(str::trim) {
        Some("auto") => Orient::Auto,
        Some("auto-start-reverse") => Orient::AutoStartReverse,
        Some(angle) => Orient::Angle(angle_value(angle).unwrap_or(0.0)),
        None => Orient::Angle(0.0),
    };
    let overflow = property(elem, "overflow").map(str::trim);
    MarkerDef {
        in_stroke_widths: elem.attr("markerUnits").map(str::trim) != Some("userSpaceOnUse"),
        view_box: elem.attr("viewBox").and_then(|s| parse_viewbox(s).ok()),
        aspect: elem
            .attr("preserveAspectRatio")
            .map_or(AspectRatio::DEFAULT, AspectRatio::parse),
        reference: (
            reference("refX", [("left", 0.0), ("center", 0.5), ("right", 1.0)]),
            reference("refY", [("top", 0.0), ("center", 0.5), ("bottom", 1.0)]),
        ),
        size: (number("markerWidth", 3.0), number("markerHeight", 3.0)),
        orient,
        clip: !matches!(overflow, Some("visible" | "auto")),
        content: Vec::new(),
    }
}

/// An angle, in radians: a number of degrees, or one with a CSS unit.
fn angle_value(text: &str) -> Option<f32> {
    let units: [(&str, f32); 4] = [
        ("deg", core::f32::consts::PI / 180.0),
        ("grad", core::f32::consts::PI / 200.0),
        ("rad", 1.0),
        ("turn", 2.0 * core::f32::consts::PI),
    ];
    for (unit, scale) in units {
        if let Some(n) = text.strip_suffix(unit) {
            return n
                .trim()
                .parse::<f32>()
                .ok()
                .filter(|v| v.is_finite())
                .map(|v| v * scale);
        }
    }
    text.parse::<f32>()
        .ok()
        .filter(|v| v.is_finite())
        .map(f32::to_radians)
}

/// A vertex a marker goes on: where, and the direction a marker facing
/// `auto` turns to there, in radians.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Vertex {
    pub(super) x: f32,
    pub(super) y: f32,
    pub(super) angle: f32,
}

/// One segment of a subpath: where it ends, and its direction where it
/// starts and where it ends -- `None` for one of no length.
struct Segment {
    end: (f32, f32),
    out_of_start: Option<(f32, f32)>,
    into_end: Option<(f32, f32)>,
}

/// One subpath: where it starts, its segments, and whether it is closed.
struct Subpath {
    start: (f32, f32),
    segments: Vec<Segment>,
    closed: bool,
}

/// Whether a shape takes markers: SVG's markable elements.
pub(super) fn markable(node: &SvgNode) -> bool {
    matches!(
        node,
        SvgNode::Path { .. }
            | SvgNode::Line { .. }
            | SvgNode::Polyline { .. }
            | SvgNode::Polygon { .. }
    )
}

/// The vertices of a markable shape, in order, in its own user space; none
/// for any other node.
pub(super) fn vertices(node: &SvgNode) -> Vec<Vertex> {
    let subpaths = match node {
        SvgNode::Path { commands, .. } => path_subpaths(commands),
        SvgNode::Line { x1, y1, x2, y2, .. } => {
            vec![points_subpath(&[(*x1, *y1), (*x2, *y2)], false)]
        }
        SvgNode::Polyline { points, .. } => vec![points_subpath(points, false)],
        SvgNode::Polygon { points, .. } => vec![points_subpath(points, true)],
        _ => return Vec::new(),
    };
    let mut out = Vec::new();
    for sub in &subpaths {
        sub_vertices(sub, &mut out);
    }
    out
}

/// The subpath through `points`, closed back to the first where `closed`.
fn points_subpath(points: &[(f32, f32)], closed: bool) -> Subpath {
    let mut sub = Subpath {
        start: points.first().copied().unwrap_or((0.0, 0.0)),
        segments: Vec::new(),
        closed: false,
    };
    let mut at = sub.start;
    for &point in points.iter().skip(1) {
        sub.segments.push(line(at, point));
        at = point;
    }
    if closed && !points.is_empty() {
        sub.segments.push(line(at, sub.start));
        sub.closed = true;
    }
    sub
}

/// A straight segment from `from` to `to`.
fn line(from: (f32, f32), to: (f32, f32)) -> Segment {
    let d = direction(from, to);
    Segment {
        end: to,
        out_of_start: d,
        into_end: d,
    }
}

/// The direction from `from` to `to`, or `None` where they are one point.
fn direction(from: (f32, f32), to: (f32, f32)) -> Option<(f32, f32)> {
    let d = (to.0 - from.0, to.1 - from.1);
    (d.0.hypot(d.1) > 1e-9 && d.0.is_finite() && d.1.is_finite()).then_some(d)
}

/// A step from one point to another: a direction, where it has a length.
type Step = ((f32, f32), (f32, f32));

/// The first of `candidates` with a length.
fn first_direction(candidates: &[Step]) -> Option<(f32, f32)> {
    candidates
        .iter()
        .find_map(|&(from, to)| direction(from, to))
}

/// A cubic Bézier from `p0` through `p1` and `p2` to `p3`: its direction at
/// each end is toward the nearest control point that is not the end itself.
fn cubic(p0: (f32, f32), p1: (f32, f32), p2: (f32, f32), p3: (f32, f32)) -> Segment {
    Segment {
        end: p3,
        out_of_start: first_direction(&[(p0, p1), (p0, p2), (p0, p3)]),
        into_end: first_direction(&[(p2, p3), (p1, p3), (p0, p3)]),
    }
}

/// A quadratic Bézier from `p0` through `p1` to `p2`.
fn quadratic(p0: (f32, f32), p1: (f32, f32), p2: (f32, f32)) -> Segment {
    Segment {
        end: p2,
        out_of_start: first_direction(&[(p0, p1), (p0, p2)]),
        into_end: first_direction(&[(p1, p2), (p0, p2)]),
    }
}

/// An elliptical arc, as a path's `A` command draws it: its directions at
/// each end the ellipse's tangents there, the way it turns.
fn arc(
    from: (f32, f32),
    radii: (f32, f32),
    x_rotation: f32,
    flags: (bool, bool),
    to: (f32, f32),
) -> Segment {
    let Some(centre) = super::arc_center(from, radii, x_rotation, flags, to) else {
        return line(from, to);
    };
    let tangent = |theta: f32| {
        let (sin, cos) = theta.sin_cos();
        let way = if centre.dtheta < 0.0 { -1.0 } else { 1.0 };
        let dx = (-centre.cos_phi * centre.rx * sin - centre.sin_phi * centre.ry * cos) * way;
        let dy = (-centre.sin_phi * centre.rx * sin + centre.cos_phi * centre.ry * cos) * way;
        (dx.hypot(dy) > 1e-9).then_some((dx, dy))
    };
    Segment {
        end: to,
        out_of_start: tangent(centre.theta1),
        into_end: tangent(centre.theta1 + centre.dtheta),
    }
}

/// A path's commands as subpaths of segments, as `path_to_subpaths` draws
/// them: a smooth curve reflects the last control point, and drawing after a
/// closepath starts again from where the closed subpath began.
fn path_subpaths(commands: &[PathCommand]) -> Vec<Subpath> {
    let mut subs: Vec<Subpath> = Vec::new();
    let mut current: Option<Subpath> = None;
    let mut cursor = (0.0f32, 0.0f32);
    let mut start = (0.0f32, 0.0f32);
    let mut last_cubic: Option<(f32, f32)> = None;
    let mut last_quad: Option<(f32, f32)> = None;
    for cmd in commands {
        if let PathCommand::MoveTo { x, y } = cmd {
            subs.extend(current.take());
            cursor = (*x, *y);
            start = cursor;
            current = Some(Subpath {
                start,
                segments: Vec::new(),
                closed: false,
            });
            last_cubic = None;
            last_quad = None;
            continue;
        }
        // Drawing with no subpath open -- after a closepath, or at the very
        // start -- opens one where the pen is.
        let sub = current.get_or_insert_with(|| Subpath {
            start,
            segments: Vec::new(),
            closed: false,
        });
        let (segment, cubic_cp, quad_cp) = match *cmd {
            PathCommand::MoveTo { .. } => continue,
            PathCommand::LineTo { x, y } => (line(cursor, (x, y)), None, None),
            PathCommand::HorizontalLineTo { x } => (line(cursor, (x, cursor.1)), None, None),
            PathCommand::VerticalLineTo { y } => (line(cursor, (cursor.0, y)), None, None),
            PathCommand::CubicBezier {
                x1,
                y1,
                x2,
                y2,
                x,
                y,
            } => (
                cubic(cursor, (x1, y1), (x2, y2), (x, y)),
                Some((x2, y2)),
                None,
            ),
            PathCommand::SmoothCubic { x2, y2, x, y } => {
                let p1 = last_cubic.map_or(cursor, |(lx, ly)| {
                    (2.0 * cursor.0 - lx, 2.0 * cursor.1 - ly)
                });
                (cubic(cursor, p1, (x2, y2), (x, y)), Some((x2, y2)), None)
            }
            PathCommand::QuadraticBezier { x1, y1, x, y } => {
                (quadratic(cursor, (x1, y1), (x, y)), None, Some((x1, y1)))
            }
            PathCommand::SmoothQuadratic { x, y } => {
                let p1 = last_quad.map_or(cursor, |(lx, ly)| {
                    (2.0 * cursor.0 - lx, 2.0 * cursor.1 - ly)
                });
                (quadratic(cursor, p1, (x, y)), None, Some(p1))
            }
            PathCommand::Arc {
                rx,
                ry,
                x_rotation,
                large_arc,
                sweep,
                x,
                y,
            } => (
                arc(cursor, (rx, ry), x_rotation, (large_arc, sweep), (x, y)),
                None,
                None,
            ),
            PathCommand::Close => {
                let back = line(cursor, sub.start);
                sub.segments.push(back);
                sub.closed = true;
                cursor = sub.start;
                start = sub.start;
                subs.extend(current.take());
                last_cubic = None;
                last_quad = None;
                continue;
            }
        };
        cursor = segment.end;
        sub.segments.push(segment);
        last_cubic = cubic_cp;
        last_quad = quad_cp;
    }
    subs.extend(current);
    subs
}

/// The way into vertex `k` of `sub` -- vertex 0 its start, vertex `k` the
/// end of segment `k - 1` -- from the nearest segment before it with a
/// direction; round the loop, for a closed subpath's start.
fn way_in(sub: &Subpath, k: usize) -> Option<(f32, f32)> {
    let before = sub.segments.get(..k).unwrap_or(&[]);
    before.iter().rev().find_map(|s| s.into_end).or_else(|| {
        if sub.closed && k == 0 {
            sub.segments.iter().rev().find_map(|s| s.into_end)
        } else {
            None
        }
    })
}

/// The way out of vertex `k`, from the nearest segment after it with a
/// direction; round the loop, for a closed subpath's last vertex.
fn way_out(sub: &Subpath, k: usize) -> Option<(f32, f32)> {
    let after = sub.segments.get(k..).unwrap_or(&[]);
    after.iter().find_map(|s| s.out_of_start).or_else(|| {
        if sub.closed && k == sub.segments.len() {
            sub.segments.iter().find_map(|s| s.out_of_start)
        } else {
            None
        }
    })
}

/// The direction half-way between the ways in and out, in radians: the one
/// there is where there is one, the way in where they are opposite, and
/// nought where there is neither.
fn bisect(into: Option<(f32, f32)>, out: Option<(f32, f32)>) -> f32 {
    let unit = |(x, y): (f32, f32)| {
        let len = x.hypot(y);
        (x / len, y / len)
    };
    match (into, out) {
        (Some(a), Some(b)) => {
            let (ua, ub) = (unit(a), unit(b));
            let sum = (ua.0 + ub.0, ua.1 + ub.1);
            if sum.0.hypot(sum.1) < 1e-6 {
                a.1.atan2(a.0)
            } else {
                sum.1.atan2(sum.0)
            }
        }
        (Some(a), None) => a.1.atan2(a.0),
        (None, Some(b)) => b.1.atan2(b.0),
        (None, None) => 0.0,
    }
}

/// `sub`'s vertices, added to `out`.
fn sub_vertices(sub: &Subpath, out: &mut Vec<Vertex>) {
    let points = core::iter::once(sub.start).chain(sub.segments.iter().map(|s| s.end));
    for (k, (x, y)) in points.enumerate() {
        out.push(Vertex {
            x,
            y,
            angle: bisect(way_in(sub, k), way_out(sub, k)),
        });
    }
}

#[cfg(test)]
#[path = "marker_tests.rs"]
mod tests;
