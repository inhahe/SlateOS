//! Tests for markers: the vertices of each markable shape and the way a
//! marker faces there, and markers as drawn.
//!
//! The drawings are 20 by 20 user units on 20 by 20 pixels.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_cmp
)]

use core::f32::consts::{FRAC_PI_2, FRAC_PI_4, PI};

use super::super::{SvgDocument, SvgNode, Transform, parse_path_data};
use super::{Vertex, vertices};

fn path(d: &str) -> SvgNode {
    SvgNode::Path {
        commands: parse_path_data(d).unwrap(),
        transform: Transform::IDENTITY,
        style: super::super::SvgStyle::default(),
    }
}

/// The vertices of path `d`, as `(x, y, angle)`.
fn of(d: &str) -> Vec<Vertex> {
    vertices(&path(d))
}

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-4
}

/// An angle, folded into `-pi..=pi`.
fn fold(angle: f32) -> f32 {
    let mut a = angle;
    while a > PI {
        a -= 2.0 * PI;
    }
    while a < -PI {
        a += 2.0 * PI;
    }
    a
}

// ─── Vertices and directions ────────────────────────────────────────────────

/// **A straight line's two ends face along it.**
#[test]
fn a_line_faces_along_itself() {
    let line = SvgNode::Line {
        x1: 2.0,
        y1: 2.0,
        x2: 12.0,
        y2: 12.0,
        transform: Transform::IDENTITY,
        style: super::super::SvgStyle::default(),
    };
    let v = vertices(&line);
    assert_eq!(v.len(), 2);
    assert!(near(v[0].angle, FRAC_PI_4) && near(v[1].angle, FRAC_PI_4));
    assert_eq!((v[1].x, v[1].y), (12.0, 12.0));
}

/// **A corner faces half-way between the way in and the way out.**
#[test]
fn a_corner_faces_half_way() {
    let v = of("M0 0 L10 0 L10 10");
    assert_eq!(v.len(), 3);
    assert!(near(v[0].angle, 0.0));
    assert!(near(v[1].angle, FRAC_PI_4), "{}", v[1].angle);
    assert!(near(v[2].angle, FRAC_PI_2));
}

/// **A closed subpath's first and last vertex face both ways round the
/// loop**: a square's start faces half-way between coming up its left side
/// and going along its top.
#[test]
fn a_closed_loop_faces_round_its_start() {
    let v = of("M0 0 L10 0 L10 10 L0 10 Z");
    assert_eq!(v.len(), 5, "the closing vertex is a vertex");
    // In along the closing side, upward (-pi/2); out along the top (0).
    assert!(near(v[0].angle, -FRAC_PI_4), "{}", v[0].angle);
    assert!(near(v[4].angle, -FRAC_PI_4), "{}", v[4].angle);
    assert_eq!((v[4].x, v[4].y), (0.0, 0.0));
    // A polygon is the same loop.
    let polygon = SvgNode::Polygon {
        points: vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)],
        transform: Transform::IDENTITY,
        style: super::super::SvgStyle::default(),
    };
    let p = vertices(&polygon);
    assert_eq!(p.len(), 5);
    assert!(near(p[0].angle, v[0].angle));
}

/// **A curve faces toward its control points at its ends**: a cubic that
/// leaves downward and arrives leftward.
#[test]
fn a_curve_faces_its_control_points() {
    let v = of("M0 0 C0 10 20 10 10 10");
    assert!(near(v[0].angle, FRAC_PI_2), "{}", v[0].angle);
    assert!(
        near(fold(v[1].angle), PI) || near(fold(v[1].angle), -PI),
        "{}",
        v[1].angle
    );
    // A control point on its end leaves the direction to the next one.
    let w = of("M0 0 C0 0 10 0 10 10");
    assert!(near(w[0].angle, 0.0), "{}", w[0].angle);
    // A quadratic, and the smooth curves' reflected points.
    let q = of("M0 0 Q10 0 10 10");
    assert!(near(q[0].angle, 0.0) && near(q[1].angle, FRAC_PI_2));
    let s = of("M0 0 C0 10 10 10 10 0 S20 -10 20 0");
    assert!(near(s[1].angle, -FRAC_PI_2), "{}", s[1].angle);
    let t = of("M0 0 Q5 -5 10 0 T20 0");
    // The first arrives heading down and right; the reflection leaves the
    // same way.
    assert!(near(t[1].angle, FRAC_PI_4), "{}", t[1].angle);
}

/// **An arc faces along the ellipse at its ends**: a half circle from left
/// to right over the top leaves upward and arrives downward.
#[test]
fn an_arc_faces_along_its_ellipse() {
    let v = of("M0 0 A5 5 0 0 1 10 0");
    assert!(near(v[0].angle, -FRAC_PI_2), "{}", v[0].angle);
    assert!(near(v[1].angle, FRAC_PI_2), "{}", v[1].angle);
    // The other way round, under the line: down, then up.
    let w = of("M0 0 A5 5 0 0 0 10 0");
    assert!(near(w[0].angle, FRAC_PI_2), "{}", w[0].angle);
    assert!(near(w[1].angle, -FRAC_PI_2), "{}", w[1].angle);
}

/// **A segment of no length takes the nearest direction there is.**
#[test]
fn a_segment_of_no_length_borrows_a_direction() {
    let v = of("M0 0 L0 0 L10 0");
    assert_eq!(v.len(), 3);
    assert!(near(v[0].angle, 0.0));
    assert!(near(v[1].angle, 0.0));
    assert!(near(v[2].angle, 0.0));
    let lone = of("M5 5");
    assert_eq!(lone.len(), 1);
    assert_eq!(lone[0].angle, 0.0, "no direction at all");
}

/// **Each subpath's vertices are vertices of the whole**, a second subpath
/// facing its own way; drawing on after a closepath starts from where the
/// closed subpath began.
#[test]
fn subpaths_follow_one_another() {
    let v = of("M0 0 L10 0 M20 0 L20 10");
    let at: Vec<(f32, f32)> = v.iter().map(|v| (v.x, v.y)).collect();
    assert_eq!(at, [(0.0, 0.0), (10.0, 0.0), (20.0, 0.0), (20.0, 10.0)]);
    assert!(near(v[1].angle, 0.0) && near(v[2].angle, FRAC_PI_2));
    let w = of("M0 0 L10 0 Z L0 10");
    let at: Vec<(f32, f32)> = w.iter().map(|v| (v.x, v.y)).collect();
    assert_eq!(
        at,
        [(0.0, 0.0), (10.0, 0.0), (0.0, 0.0), (0.0, 0.0), (0.0, 10.0)]
    );
}

// ─── Markers as drawn ───────────────────────────────────────────────────────

/// `body` inside a 20 by 20 drawing at 20 by 20: each pixel's straight
/// `[r, g, b, a]`.
fn draw(body: &str) -> Vec<[u8; 4]> {
    let svg = format!(r#"<svg viewBox="0 0 20 20" width="20" height="20">{body}</svg>"#);
    SvgDocument::parse(&svg)
        .unwrap()
        .render(20, 20)
        .chunks_exact(4)
        .map(|p| [p[0], p[1], p[2], p[3]])
        .collect()
}

fn at(image: &[[u8; 4]], x: usize, y: usize) -> [u8; 4] {
    image[y * 20 + x]
}

const RED: [u8; 4] = [255, 0, 0, 255];
const CLEAR: [u8; 4] = [0, 0, 0, 0];

/// An arrowhead: a red triangle pointing right, its tip the reference
/// point, four of the stroke's widths long.
const ARROW: &str = r#"<marker id="a" viewBox="0 0 10 10" refX="10" refY="5"
        markerWidth="4" markerHeight="4" orient="auto">
      <path d="M0 0 L10 5 L0 10 z" fill="red"/>
    </marker>"#;

/// **An end marker sits on the last vertex, its reference point on it,
/// turned along the line** -- right along a horizontal line, down along a
/// vertical one.
#[test]
fn an_arrowhead_points_along_its_line() {
    // The arrowhead is the triangle (12, 8) (16, 10) (12, 12): its tip on
    // the end.
    let across = draw(&format!(
        r#"{ARROW}<path d="M2 10 L16 10" stroke="none" stroke-width="1" marker-end="url(#a)"/>"#
    ));
    assert_eq!(at(&across, 13, 10), RED);
    assert_eq!(at(&across, 13, 9), RED);
    assert_eq!(
        at(&across, 17, 10),
        CLEAR,
        "the tip on the end, not past it"
    );
    assert_eq!(at(&across, 14, 7), CLEAR);
    // Turned down: (12, 12) (10, 16) (8, 12).
    let down = draw(&format!(
        r#"{ARROW}<path d="M10 2 L10 16" stroke="none" marker-end="url(#a)"/>"#
    ));
    assert_eq!(at(&down, 10, 13), RED);
    assert_eq!(at(&down, 9, 13), RED);
    assert_eq!(at(&down, 10, 17), CLEAR);
    assert_eq!(at(&down, 14, 14), CLEAR);
}

/// **A marker is sized in the stroke's widths by default, and in user
/// units when it says so.**
#[test]
fn a_marker_is_sized_by_its_units() {
    let wide = draw(&format!(
        r#"{ARROW}<path d="M1 10 L19 10" stroke="none" stroke-width="2" marker-end="url(#a)"/>"#
    ));
    // Eight long at a stroke width of 2: from 11 to 19.
    assert_eq!(at(&wide, 12, 10), RED);
    let fixed = draw(
        r#"<marker id="a" viewBox="0 0 10 10" refX="10" refY="5" markerUnits="userSpaceOnUse"
             markerWidth="4" markerHeight="4" orient="auto">
             <path d="M0 0 L10 5 L0 10 z" fill="red"/>
           </marker>
           <path d="M1 10 L19 10" stroke="none" stroke-width="2" marker-end="url(#a)"/>"#,
    );
    // Four long, whatever the stroke: (15, 8) (19, 10) (15, 12).
    assert_eq!(at(&fixed, 16, 10), RED);
    assert_eq!(at(&fixed, 12, 10), CLEAR, "four long, whatever the stroke");
}

/// **`auto-start-reverse` turns the start marker about**, so one arrowhead
/// serves both ends; a fixed angle does not follow the line.
#[test]
fn the_start_can_face_backward() {
    let image = draw(
        r#"<marker id="a" viewBox="0 0 10 10" refX="10" refY="5" markerWidth="4" markerHeight="4"
             orient="auto-start-reverse">
             <path d="M0 0 L10 5 L0 10 z" fill="red"/>
           </marker>
           <path d="M4 10 L16 10" stroke="none" marker-start="url(#a)" marker-end="url(#a)"/>"#,
    );
    // Start: the tip on (4, 10), pointing left -- the body to the right.
    assert_eq!(at(&image, 6, 10), RED);
    assert_eq!(at(&image, 2, 10), CLEAR);
    // End: the tip on (16, 10), pointing right.
    assert_eq!(at(&image, 13, 10), RED);
    let fixed = draw(
        r#"<marker id="a" viewBox="0 0 10 10" refX="10" refY="5" markerWidth="4" markerHeight="4"
             orient="90">
             <path d="M0 0 L10 5 L0 10 z" fill="red"/>
           </marker>
           <path d="M2 10 L16 10" stroke="none" marker-end="url(#a)"/>"#,
    );
    // Turned a quarter: pointing down, its tip on the end -- (18, 6)
    // (16, 10) (14, 6).
    assert_eq!(at(&fixed, 15, 7), RED);
    assert_eq!(at(&fixed, 14, 10), CLEAR);
}

/// A dot: a blue square 2 across, centred on its vertex, in user units.
const DOT: &str = r#"<marker id="d" viewBox="0 0 2 2" refX="1" refY="1" markerUnits="userSpaceOnUse"
        markerWidth="2" markerHeight="2"><rect width="2" height="2" fill="blue"/></marker>"#;

/// **The middle marker goes on every vertex but the first and the last.**
#[test]
fn middle_markers_go_between() {
    let image = draw(&format!(
        r#"{DOT}<polyline points="2 2 10 2 10 10 18 10" fill="none" marker-mid="url(#d)"/>"#
    ));
    assert_eq!(at(&image, 10, 2), [0, 0, 255, 255]);
    assert_eq!(at(&image, 10, 10), [0, 0, 255, 255]);
    assert_eq!(at(&image, 2, 2), CLEAR);
    assert_eq!(at(&image, 18, 10), CLEAR);
}

/// **The `marker` shorthand sets all three, and the properties inherit**;
/// `none` takes one back, and a name of no `<marker>` draws none.
#[test]
fn markers_inherit_and_shorthand() {
    let all = draw(&format!(
        r#"{DOT}<polyline points="2 2 10 2 18 2" fill="none" style="marker: url(#d)"/>"#
    ));
    for x in [2, 10, 18] {
        assert_eq!(at(&all, x, 2), [0, 0, 255, 255], "{x}");
    }
    let inherited = draw(&format!(
        r#"{DOT}<g marker-end="url(#d)"><polyline points="2 2 10 2 18 2" fill="none"/></g>"#
    ));
    assert_eq!(at(&inherited, 18, 2), [0, 0, 255, 255]);
    let refused = draw(&format!(
        r#"{DOT}<g marker-end="url(#d)"><polyline points="2 2 10 2 18 2" fill="none" marker-end="none"/></g>"#
    ));
    assert_eq!(at(&refused, 18, 2), CLEAR);
    let missing = draw(r#"<polyline points="2 2 10 2 18 2" fill="none" marker-end="url(#gone)"/>"#);
    assert!(missing.iter().all(|px| *px == CLEAR));
}

/// **A marker is cut to its viewport, unless its overflow is visible.**
#[test]
fn a_marker_is_cut_to_its_viewport() {
    let marker = |overflow: &str| {
        draw(&format!(
            r#"<marker id="m" markerUnits="userSpaceOnUse" markerWidth="2" markerHeight="2" {overflow}>
                 <rect width="6" height="2" fill="red"/>
               </marker>
               <path d="M4 4 L4 4" marker-start="url(#m)"/>"#
        ))
    };
    let cut = marker("");
    assert_eq!(at(&cut, 5, 4), RED);
    assert_eq!(at(&cut, 8, 4), CLEAR);
    let visible = marker(r#"overflow="visible""#);
    assert_eq!(at(&visible, 8, 4), RED);
}

/// **A marker inherits from itself, not from what it marks**: a shape's
/// blue stroke does not colour a marker that says no fill of its own.
#[test]
fn a_marker_does_not_inherit_from_its_shape() {
    let image = draw(
        r#"<marker id="m" markerUnits="userSpaceOnUse" markerWidth="4" markerHeight="4" refX="2" refY="2">
             <rect width="4" height="4"/>
           </marker>
           <path d="M10 10 L10 10" fill="blue" stroke="blue" marker-start="url(#m)"/>"#,
    );
    assert_eq!(
        at(&image, 10, 10),
        [0, 0, 0, 255],
        "black: SVG's own default"
    );
}

/// **A marker whose content marks with itself is drawn once.**
#[test]
fn a_marker_inside_itself_is_drawn_once() {
    let image = draw(
        r#"<marker id="m" markerUnits="userSpaceOnUse" markerWidth="20" markerHeight="20" overflow="visible">
             <path d="M0 0 L4 0" stroke="red" marker-end="url(#m)"/>
           </marker>
           <path d="M2 2 L2 2" marker-start="url(#m)"/>"#,
    );
    // The outer marker's line, from (2, 2) to (6, 2); nothing of a marker
    // inside it at (6, 2), which would draw another line to (10, 2).
    assert!(at(&image, 4, 2)[3] > 0);
    assert_eq!(at(&image, 8, 2), CLEAR);
}

/// **A faded shape's marker is faded with it, as one**: where the marker
/// lies over the shape's stroke, the stroke does not show through.
#[test]
fn a_faded_shape_and_its_marker_fade_as_one() {
    let image = draw(&format!(
        r#"{DOT}<path d="M2 10 L18 10" stroke="red" stroke-width="2" opacity="0.5" marker-mid="url(#d)" marker-start="url(#d)"/>"#
    ));
    let px = at(&image, 2, 10);
    assert_eq!((px[0], px[2]), (0, 255), "{px:?}");
    assert!(px[3].abs_diff(128) <= 1, "{px:?}");
}

// ─── Arcs ───────────────────────────────────────────────────────────────────

/// **An arc whose ends are one point is omitted, as SVG says** -- it drew a
/// spike one radius long, in the direction of its x-axis rotation.
#[test]
fn an_arc_to_where_it_began_is_omitted() {
    let image = draw(r#"<path d="M5 5 A 3 3 90 0 1 5 5 L 15 5" fill="none" stroke="black"/>"#);
    for y in 6..10 {
        assert_eq!(at(&image, 5, y), CLEAR, "a spike at (5, {y})");
    }
    assert!(at(&image, 10, 5)[3] > 0, "the line after it is drawn");
}
