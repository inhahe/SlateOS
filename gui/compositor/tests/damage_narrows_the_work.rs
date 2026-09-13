//! Redrawing one window costs a fraction of redrawing the desktop.
//!
//! The compositor has a partial-recomposite path: when nothing structural has
//! changed it clears only the damaged rectangles and re-renders only the
//! windows overlapping them. Nothing asserted that the path is actually
//! narrower than the full one, and "narrower" is the entire reason it exists.
//!
//! # Why this is a ratio and not a budget
//!
//! The absolute cost of a frame is already measured, by
//! `compositor::tests::bench_compose_frame_4k` against the baseline recorded
//! in `bench/baselines.toml` under `[compositor_frame_4k]` -- a 2 ms target,
//! 7.0 ms measured, and the history of five optimisations that took it down
//! from 48.6 ms. That benchmark is `#[ignore]`d and belongs that way: an
//! eight-window 4K composite saturates memory bandwidth for a second or two,
//! which is enough to fail the *neighbouring* timing test when the whole
//! workspace runs. This file asserts something that benchmark does not,
//! cheaply enough to run every time.
//!
//! A ratio also survives what an absolute number does not. These two figures
//! are taken on a machine that may be running four hundred other tests, and
//! their spread run to run is wide; their quotient is stable.
//!
//! # The trap this file exists downstream of
//!
//! Measured with all eight windows left at the compositor's default position,
//! redrawing one cost 73% of redrawing all eight -- which reads as "damage
//! tracking does nothing". It was the scene: stacked windows all overlap, so
//! damage to one is damage to every one of them. Placed apart it costs an
//! eighth. Hence `move_window` below, which is load-bearing and not cosmetic.

// A benchmark divides and asserts on the result; the defensive lints that
// forbid that in production code are off here, as `CLAUDE.md` prescribes for
// test code.
#![allow(clippy::arithmetic_side_effects, clippy::panic, clippy::expect_used)]

use compositor::Compositor;
use guitk::color::Color;
use guitk::render::RenderTree;

/// A window's worth of drawing: a background, a title strip, and rows.
fn window_tree(width: f32, height: f32) -> RenderTree {
    let mut tree = RenderTree::new();
    tree.fill_rect(0.0, 0.0, width, height, Color::rgb(30, 30, 46));
    tree.fill_rect(0.0, 0.0, width, 32.0, Color::rgb(24, 24, 37));
    tree.text(12.0, 8.0, "A window", Color::rgb(205, 214, 244), 14.0);

    let mut y = 44.0;
    while y < height - 24.0 {
        tree.fill_rect(8.0, y, width - 16.0, 20.0, Color::rgb(49, 50, 68));
        tree.text(
            16.0,
            y + 3.0,
            "a row of text, of the sort a list draws",
            Color::rgb(166, 173, 200),
            12.0,
        );
        y += 24.0;
    }
    tree
}

/// Eight windows, spread across the screen.
///
/// The refresh rate is absurd on purpose: `FrameStats::should_compose` gates
/// on the refresh interval, so a second `compose_frame` inside it returns
/// early. Handing the compositor a 100 kHz display turns the rate limiter
/// from an obstacle into a parameter. (`bench_full_composite` is the other
/// way round the same problem, and is what the 4K benchmark uses.)
fn desktop() -> (Compositor, Vec<compositor::WindowId>) {
    let mut comp = Compositor::new(1920, 1080, 100_000).expect("compositor");
    let (w, h) = (480u32, 360u32);
    let mut ids = Vec::new();
    for i in 0..8 {
        let id = comp.create_window(format!("Window {i}"), w, h, 1);
        comp.move_window(id, (i % 4) * w as i32, (i / 4) * h as i32)
            .expect("move");
        comp.submit_render(id, window_tree(w as f32, h as f32).commands)
            .expect("submit");
        ids.push(id);
    }
    (comp, ids)
}

/// A cold frame: everything is damaged, so this is the full path.
fn full_frame() -> u64 {
    let (mut comp, _) = desktop();
    comp.compose_frame();
    comp.frame_stats().last_frame_time_us
}

/// A warm frame in which one window redrew its own content.
fn one_window_frame() -> u64 {
    let (mut comp, ids) = desktop();
    comp.compose_frame();
    let id = *ids.first().expect("eight windows");
    comp.submit_render(id, window_tree(480.0, 360.0).commands)
        .expect("submit");
    comp.compose_frame();
    comp.frame_stats().last_frame_time_us
}

#[test]
fn redrawing_one_window_costs_a_fraction_of_the_whole_desktop() {
    let full = (0..3).map(|_| full_frame()).min().expect("three");
    let steady = (0..3).map(|_| one_window_frame()).min().expect("three");

    println!("full recomposite {full} us, one window redrawn {steady} us");

    // A third, not an eighth. What is asserted is that the partial path is
    // genuinely partial; a tight ratio would fail for the ordinary reason
    // that these are two measurements on a shared machine. An eighth is what
    // it actually measures.
    assert!(
        steady * 3 < full,
        "redrawing one window of eight cost {steady} us against {full} us for \
         the whole desktop. Damage tracking is not narrowing the work: check \
         first whether the windows overlap, which makes every partial \
         recomposite a full one."
    );
}

/// A frame with nothing to draw reports no time, rather than the last one's.
///
/// `compose_frame` returns early on two paths -- the refresh-interval gate and
/// the nothing-is-damaged check -- and both used to leave `last_frame_time_us`
/// holding the previous frame's figure. That does not look stale; it looks
/// like a beautifully repeatable measurement, and two separate attempts to
/// measure an idle frame read it as a result before anyone noticed.
#[test]
fn a_skipped_frame_reports_no_time_rather_than_the_last_one_s() {
    let (mut comp, _) = desktop();
    comp.compose_frame();
    assert!(
        comp.frame_stats().last_frame_time_us > 0,
        "the first frame drew"
    );

    for _ in 0..3 {
        comp.compose_frame();
        assert_eq!(
            comp.frame_stats().last_frame_time_us,
            0,
            "a frame with nothing to draw must not report the last frame's time"
        );
    }
}
