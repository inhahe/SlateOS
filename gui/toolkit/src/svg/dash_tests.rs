//! Tests for dashed strokes: what `stroke-dasharray`, `stroke-dashoffset`
//! and `pathLength` say, how a run is cut into dashes, and dashes as drawn.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_cmp,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::super::{Dashes, LineCap, Subpath, SvgDocument};
use super::{DOT_PX, Dashing, dasharray, dashoffset, path_length};

fn pattern(dashes: Option<Dashes>) -> Option<Vec<f32>> {
    match dashes? {
        Dashes::Solid => Some(Vec::new()),
        Dashes::Pattern(p) => Some(p.to_vec()),
    }
}

// ─── What the properties say ────────────────────────────────────────────────

/// **A pattern is lengths in turn, an odd count repeated; `none` and a sum
/// of nought are solid; a negative or unreadable length says nothing.**
#[test]
fn a_dash_pattern_reads_as_svg_says() {
    let read = |v: &str| pattern(dasharray(v, (100.0, 100.0)));
    assert_eq!(read("5 3"), Some(vec![5.0, 3.0]));
    assert_eq!(read("5,3,2"), Some(vec![5.0, 3.0, 2.0, 5.0, 3.0, 2.0]));
    assert_eq!(read(" 5px , 2 "), Some(vec![5.0, 2.0]));
    assert_eq!(read("none"), Some(vec![]), "solid");
    assert_eq!(read("0 0"), Some(vec![]), "a sum of nought is solid");
    assert_eq!(read("0"), Some(vec![]));
    assert_eq!(read("5 -3"), None);
    assert_eq!(read("5 x"), None);
    assert_eq!(read("5em"), None);
    assert_eq!(read(""), None);
    assert_eq!(read("NaN 2"), None);
    // A percentage is of the normalised diagonal: 100 for a 100-square.
    assert_eq!(read("10%"), Some(vec![10.0, 10.0]));
}

/// **An offset is a length, negative too, or a percentage of the
/// normalised diagonal; `pathLength` a number not negative.**
#[test]
fn an_offset_and_a_path_length_read() {
    assert_eq!(dashoffset("-2", (30.0, 40.0)), Some(-2.0));
    let half = dashoffset("50%", (30.0, 40.0)).unwrap();
    assert!((half - 1250f32.sqrt() / 2.0).abs() < 1e-4, "{half}");
    assert_eq!(dashoffset("x", (30.0, 40.0)), None);
    assert_eq!(path_length("100"), Some(100.0));
    assert_eq!(path_length("1e3"), Some(1000.0));
    assert_eq!(path_length("0"), Some(0.0));
    assert_eq!(path_length("-1"), None);
    assert_eq!(path_length("10px"), None);
}

// ─── Cutting ────────────────────────────────────────────────────────────────

fn run(points: &[(f32, f32)], closed: bool) -> Subpath {
    Subpath {
        points: points.to_vec(),
        closed,
    }
}

/// `subpaths` cut by `pattern` from `offset`, lengths as they are.
fn cut(subpaths: &[Subpath], pattern: &[f32], offset: f32) -> Option<Vec<Subpath>> {
    cut_with(subpaths, pattern, offset, None, LineCap::Butt)
}

fn cut_with(
    subpaths: &[Subpath],
    pattern: &[f32],
    offset: f32,
    path_length: Option<f32>,
    cap: LineCap,
) -> Option<Vec<Subpath>> {
    Dashing {
        pattern,
        offset,
        path_length,
        measure: |(dx, dy): (f32, f32)| dx.hypot(dy),
        cap,
    }
    .cut(subpaths)
}

/// Each dash's points, rounded to a thousandth.
fn shape(dashes: &[Subpath]) -> Vec<Vec<(f32, f32)>> {
    let r = |v: f32| (v * 1000.0).round() / 1000.0;
    dashes
        .iter()
        .map(|d| d.points.iter().map(|&(x, y)| (r(x), r(y))).collect())
        .collect()
}

/// **A run is cut into the pattern's dashes, from its offset** -- forwards,
/// and backwards for a negative one.
#[test]
fn a_run_is_cut_from_its_offset() {
    let line = [run(&[(0.0, 0.0), (10.0, 0.0)], false)];
    assert_eq!(
        shape(&cut(&line, &[2.0, 3.0], 0.0).unwrap()),
        [vec![(0.0, 0.0), (2.0, 0.0)], vec![(5.0, 0.0), (7.0, 0.0)],]
    );
    assert_eq!(
        shape(&cut(&line, &[2.0, 3.0], 1.0).unwrap()),
        [
            vec![(0.0, 0.0), (1.0, 0.0)],
            vec![(4.0, 0.0), (6.0, 0.0)],
            vec![(9.0, 0.0), (10.0, 0.0)],
        ]
    );
    // -1 is 4 into a pattern of 5: a gap with 1 left.
    assert_eq!(
        shape(&cut(&line, &[2.0, 3.0], -1.0).unwrap()),
        [vec![(1.0, 0.0), (3.0, 0.0)], vec![(6.0, 0.0), (8.0, 0.0)]]
    );
    // An offset of a whole period is none.
    assert_eq!(cut(&line, &[2.0, 3.0], 15.0), cut(&line, &[2.0, 3.0], 0.0));
}

/// **A dash turns a corner with the run**, the corner one of its points.
#[test]
fn a_dash_turns_a_corner() {
    let bend = [run(&[(0.0, 0.0), (4.0, 0.0), (4.0, 4.0)], false)];
    assert_eq!(
        shape(&cut(&bend, &[6.0, 10.0], 0.0).unwrap()),
        [vec![(0.0, 0.0), (4.0, 0.0), (4.0, 2.0)]]
    );
}

/// **A closed run is cut along its closing side too, into open dashes**;
/// one dash covering all of it is the run, closed.
#[test]
fn a_closed_run_is_cut_round_its_closing_side() {
    let square = [run(&[(0.0, 0.0), (4.0, 0.0), (4.0, 4.0), (0.0, 4.0)], true)];
    let dashes = cut(&square, &[6.0, 2.0], 0.0).unwrap();
    assert_eq!(
        shape(&dashes),
        [
            vec![(0.0, 0.0), (4.0, 0.0), (4.0, 2.0)],
            vec![(4.0, 4.0), (0.0, 4.0), (0.0, 2.0)],
        ]
    );
    assert!(dashes.iter().all(|d| !d.closed));
    let whole = cut(&square, &[100.0, 1.0], 0.0).unwrap();
    assert_eq!(
        whole,
        square.to_vec(),
        "drawn closed, joined where it closes"
    );
    // Started inside the dash, still whole.
    assert_eq!(cut(&square, &[100.0, 1.0], 50.0).unwrap(), square.to_vec());
}

/// **Each subpath starts the pattern again.**
#[test]
fn each_subpath_starts_the_pattern_again() {
    let two = [
        run(&[(0.0, 0.0), (3.0, 0.0)], false),
        run(&[(0.0, 5.0), (3.0, 5.0)], false),
    ];
    assert_eq!(
        shape(&cut(&two, &[2.0, 2.0], 0.0).unwrap()),
        [vec![(0.0, 0.0), (2.0, 0.0)], vec![(0.0, 5.0), (2.0, 5.0)]]
    );
}

/// **A dash of no length is a dot under a round or square cap**, turned
/// along the run -- the last at the run's very end -- **and nothing under a
/// butt cap.**
#[test]
fn a_dash_of_no_length_is_a_dot() {
    let up = [run(&[(0.0, 10.0), (0.0, 0.0)], false)];
    for cap in [LineCap::Round, LineCap::Square] {
        let dots = cut_with(&up, &[0.0, 5.0], 0.0, None, cap).unwrap();
        let starts: Vec<(f32, f32)> = dots.iter().map(|d| d.points[0]).collect();
        assert_eq!(starts, [(0.0, 10.0), (0.0, 5.0), (0.0, 0.0)], "{cap:?}");
        for dot in &dots {
            let [a, b] = [dot.points[0], dot.points[1]];
            assert_eq!(a.0, b.0, "turned along the run");
            assert!((a.1 - b.1 - DOT_PX).abs() < 1e-6, "upwards: {a:?} {b:?}");
        }
    }
    let none = cut_with(&up, &[0.0, 5.0], 0.0, None, LineCap::Butt).unwrap();
    assert!(none.is_empty());
}

/// **`pathLength` scales the pattern by the length the run has over it**;
/// nought scales it without end.
#[test]
fn path_length_scales_the_pattern() {
    let line = [run(&[(0.0, 0.0), (10.0, 0.0)], false)];
    assert_eq!(
        shape(&cut_with(&line, &[4.0, 4.0], 0.0, Some(20.0), LineCap::Butt).unwrap()),
        [
            vec![(0.0, 0.0), (2.0, 0.0)],
            vec![(4.0, 0.0), (6.0, 0.0)],
            vec![(8.0, 0.0), (10.0, 0.0)],
        ]
    );
    // The offset is scaled with it.
    assert_eq!(
        shape(&cut_with(&line, &[4.0, 4.0], 4.0, Some(20.0), LineCap::Butt).unwrap()),
        [vec![(2.0, 0.0), (4.0, 0.0)], vec![(6.0, 0.0), (8.0, 0.0)]]
    );
    // Nought: the first dash covers the run; a dot stays a dot.
    assert_eq!(
        shape(&cut_with(&line, &[4.0, 4.0], 0.0, Some(0.0), LineCap::Butt).unwrap()),
        [vec![(0.0, 0.0), (10.0, 0.0)]]
    );
    let dots = cut_with(&line, &[0.0, 4.0], 0.0, Some(0.0), LineCap::Round).unwrap();
    assert_eq!(dots.len(), 1, "one dot, then a gap without end");
}

/// **A run cut into more than [`MAX_DASHES`](super::MAX_DASHES) dashes is
/// stroked solid**, and so is one whose length cannot be measured.
#[test]
fn too_many_dashes_is_solid() {
    let long = [run(&[(0.0, 0.0), (1.0e6, 0.0)], false)];
    assert_eq!(cut(&long, &[0.01, 0.01], 0.0), None);
    assert!(cut(&long, &[1000.0, 1000.0], 0.0).is_some());
    let bad = [run(&[(0.0, 0.0), (f32::NAN, 0.0)], false)];
    assert_eq!(cut(&bad, &[1.0, 1.0], 0.0), None);
}

// ─── As drawn ───────────────────────────────────────────────────────────────

/// A 40 by 10 drawing on 40 by 10 pixels.
fn draw(body: &str) -> Vec<[u8; 4]> {
    let svg = format!(r#"<svg viewBox="0 0 40 10" width="40" height="10">{body}</svg>"#);
    SvgDocument::parse(&svg)
        .unwrap()
        .render(40, 10)
        .chunks_exact(4)
        .map(|p| [p[0], p[1], p[2], p[3]])
        .collect()
}

/// Whether the pixel at `(x, y)` is mostly painted.
fn inked(image: &[[u8; 4]], x: usize, y: usize) -> bool {
    image[y * 40 + x][3] > 200
}

/// Whether nothing is painted at `(x, y)`.
fn bare(image: &[[u8; 4]], x: usize, y: usize) -> bool {
    image[y * 40 + x][3] < 30
}

/// **A dashed line is drawn as its dashes**, with gaps between.
#[test]
fn a_dashed_line_is_drawn_in_dashes() {
    let image = draw(
        r#"<line x1="0" y1="5" x2="40" y2="5" stroke="black" stroke-width="4" stroke-dasharray="5 5"/>"#,
    );
    for x in [1, 3, 11, 13, 21, 33] {
        assert!(inked(&image, x, 5), "dash at {x}");
    }
    for x in [6, 8, 16, 18, 26, 38] {
        assert!(bare(&image, x, 5), "gap at {x}");
    }
}

/// **Dashes are inherited, and `none` takes them away again**; an offset is
/// inherited with them.
#[test]
fn dashes_are_inherited() {
    let image = draw(
        r#"<g stroke-dasharray="5 5" stroke-dashoffset="5">
             <line x1="0" y1="2" x2="40" y2="2" stroke="black" stroke-width="2"/>
             <line x1="0" y1="7" x2="40" y2="7" stroke="black" stroke-width="2" stroke-dasharray="none"/>
           </g>"#,
    );
    assert!(bare(&image, 2, 2), "offset by a dash: a gap first");
    assert!(inked(&image, 7, 2));
    for x in [2, 7, 12] {
        assert!(inked(&image, x, 7), "solid at {x}");
    }
}

/// **A dash is as long as the pattern says in user units** -- twice as many
/// pixels under a scale of two, and measured along the axis the transform
/// stretches.
#[test]
fn dashes_are_measured_in_user_space() {
    let doubled = draw(
        r#"<line x1="0" y1="2.5" x2="20" y2="2.5" stroke="black" stroke-width="2" stroke-dasharray="5 5" transform="scale(2)"/>"#,
    );
    assert!(inked(&doubled, 8, 5), "a dash of 5 is 10 pixels");
    assert!(bare(&doubled, 12, 5));
    assert!(inked(&doubled, 22, 5));
    // Stretched across, not up: a dash along x is twice as long as one of
    // the same user length would be along y.
    let across = draw(
        r#"<line x1="0" y1="5" x2="20" y2="5" stroke="black" stroke-width="2" stroke-dasharray="4 4" transform="scale(2 1)"/>"#,
    );
    assert!(inked(&across, 6, 5), "the first dash reaches 8 pixels");
    assert!(bare(&across, 10, 5));
    assert!(inked(&across, 17, 5));
}

/// **A rectangle's dashes start at its top left corner, going right** --
/// where SVG says its outline starts.
#[test]
fn a_rectangles_dashes_start_at_its_corner() {
    let image = draw(
        r#"<rect x="2" y="2" width="30" height="6" fill="none" stroke="black" stroke-width="2" stroke-dasharray="10 50"/>"#,
    );
    assert!(inked(&image, 6, 2), "along the top from the corner");
    assert!(bare(&image, 20, 2), "the gap after it");
    assert!(bare(&image, 32, 5), "and down the right side");
}

/// **A dash of no length under round caps is a dot**: a pattern of `0 10`
/// makes a row of them.
#[test]
fn round_dots_are_drawn() {
    let image = draw(
        r#"<line x1="5" y1="5" x2="35" y2="5" stroke="black" stroke-width="4" stroke-linecap="round" stroke-dasharray="0 10"/>"#,
    );
    for x in [5, 15, 25, 35] {
        assert!(inked(&image, x, 5), "dot at {x}");
    }
    for x in [10, 20, 30] {
        assert!(bare(&image, x, 5), "between at {x}");
    }
    let butt = draw(
        r#"<line x1="5" y1="5" x2="35" y2="5" stroke="black" stroke-width="4" stroke-dasharray="0 10"/>"#,
    );
    assert!(
        butt.iter().all(|px| px[3] < 30),
        "a butt-capped dot is nothing"
    );
}

/// **`pathLength` measures the pattern against the length it says**: a
/// line said to be 4 long, dashed `1 1`, is half dashes, half gaps.
#[test]
fn path_length_measures_the_pattern() {
    let image = draw(
        r#"<line x1="0" y1="5" x2="40" y2="5" pathLength="4" stroke="black" stroke-width="2" stroke-dasharray="1 1"/>"#,
    );
    assert!(inked(&image, 5, 5));
    assert!(bare(&image, 15, 5));
    assert!(inked(&image, 25, 5));
    assert!(bare(&image, 35, 5));
}
