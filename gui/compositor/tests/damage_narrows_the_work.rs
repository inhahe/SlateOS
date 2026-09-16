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
fn full_frame() -> (u64, u64) {
    let (mut comp, _) = desktop();
    comp.compose_frame();
    {
        let s = comp.frame_stats();
        (s.windows_rendered, s.last_frame_time_us)
    }
}

/// A warm frame in which one window redrew its own content.
///
/// Returns (windows re-rendered, microseconds). The first is what is
/// asserted; the second is printed.
fn one_window_frame() -> (u64, u64) {
    let (mut comp, ids) = desktop();
    comp.compose_frame();
    let id = *ids.first().expect("eight windows");
    comp.submit_render(id, window_tree(480.0, 360.0).commands)
        .expect("submit");
    comp.compose_frame();
    let s = comp.frame_stats();
    (s.windows_rendered, s.last_frame_time_us)
}

/// Redrawing one window of eight re-renders fewer windows than a full frame.
///
/// **This used to compare two wall-clock durations, and it was flaky in the
/// one place it mattered.** On an idle machine a full recomposite takes
/// roughly five times a one-window frame and the assertion passed
/// comfortably; under `cargo test --workspace`, with dozens of test binaries
/// running at once, it measured 2.77x and failed the whole gate. Minutes
/// apart, no code change between. Taking `min()` of three runs does not help
/// when the load is sustained for the length of the run.
///
/// **A flaky assertion in a gate every lane pays is worse than no assertion**,
/// because it teaches its readers to re-run rather than to read -- and the one
/// time it is right, it is indistinguishable from the times it was not.
///
/// The property actually wanted is countable: a partial frame re-renders the
/// windows overlapping the damage, a full recomposite re-renders all of them.
/// That number does not move when the machine is busy. The timing is still
/// measured and printed, because it is the reason anyone cares -- it is just
/// no longer what decides whether the tree is broken.
#[test]
fn redrawing_one_window_costs_a_fraction_of_the_whole_desktop() {
    let (full_windows, full_us) = full_frame();
    let (steady_windows, steady_us) = one_window_frame();

    println!(
        "full recomposite {full_windows} window(s) in {full_us} us, one window redrawn {steady_windows} window(s) in {steady_us} us"
    );

    // The control. Without it this passes when the full frame renders nothing
    // either -- which is what a broken `desktop()` fixture would produce, and
    // "0 is not more than 0" would have read as a pass.
    assert!(
        full_windows >= 8,
        "control: a full recomposite of eight windows should re-render all of them, and re-rendered {full_windows}"
    );
    assert!(
        steady_windows < full_windows,
        "redrawing one window of eight re-rendered {steady_windows} of them, against {full_windows} for the whole desktop. Damage tracking is not narrowing the work: check first whether the windows overlap, which makes every partial recomposite a full one."
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
