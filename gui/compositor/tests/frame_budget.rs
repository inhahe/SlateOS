//! A full desktop composites inside the frame budget.
//!
//! `performance-targets.md` asks for a whole desktop at 4K in under 2 ms, so a
//! 144 Hz display does not miss a refresh. Until now nothing measured whether
//! it does: the compositor recorded `last_frame_time_us` every frame and the
//! only assertion on that number anywhere was `> 0` -- a test that the clock
//! runs. That is half one of
//! `TD-C-THE-COMPOSITOR-FRAME-BUDGET-HAS-NO-INSTRUMENT`; half two, a ceiling
//! on the demo scene, was added on 2026-09-11 and caught a 30x regression
//! within the day.
//!
//! # Why a test and not a benchmark
//!
//! No `gui` crate depends on criterion and none has a `benches/` directory.
//! Adding one for a single number would be a larger change than the number is
//! worth, and this lane already has the pattern:
//! `gui/appearance/tests/resolve_cost.rs` bounds `Palette::from_settings` this
//! way. A `tests/` file runs in the ordinary suite, which is the property that
//! matters -- a benchmark nobody runs measures nothing.
//!
//! # The measurement, taken 2026-09-13
//!
//! Eight 960x720 windows on a 3840x2160 surface, each window a background, a
//! title strip and 29 rows of text -- about 60 commands a window, 480 in all:
//!
//! | build | best of five |
//! |---|---|
//! | debug | ~160 000 us |
//! | release | ~13 800 us |
//!
//! A second scene of the same shape with the text removed measured ~5 400 us
//! in release, so roughly eight of those thirteen milliseconds are text.
//!
//! **Against a 2 ms target that is six to seven times over**, and it is the
//! first time anyone has been able to say so -- which was the point of the
//! entry. It is *not* yet a claim that the compositor is too slow, for two
//! reasons written down so the next person does not have to re-derive them:
//! this is one scene and a heavy one, and `FrameStats::should_compose` rate
//! limits, so a second `compose_frame` inside the 144 Hz interval returns
//! early without updating `last_frame_time_us` -- an idle-frame comparison
//! measured this way reports the previous frame's number five times over and
//! means nothing. See known-issues.md.
//!
//! # What the ceiling is, and is not
//!
//! Stated against a **debug** build, because that is what `cargo test` runs
//! and a bound nobody runs is not a bound. It is therefore far looser than the
//! 2 ms target: the point is to catch a *regression in kind*, of the sort half
//! two caught, rather than to certify the target. Certifying it needs a
//! release measurement, and the figure this test prints on every run is what
//! to compare against.
//!
//! # If this fails
//!
//! Read the printed figure before blaming the machine. The adjudication rule
//! that worked last time: a genuine regression is an order of magnitude, not a
//! factor of two. `67_074 us` against a `50_000 us` ceiling looked like load
//! and was a 30x regression in `contrast_ratio`. This test takes the best of
//! five samples, so if the *best* is over the ceiling the machine was not the
//! problem.

// A benchmark divides and asserts on the result; the defensive lints that
// forbid that in production code are off here, as `CLAUDE.md` prescribes for
// test code.
#![allow(clippy::arithmetic_side_effects, clippy::panic, clippy::expect_used)]

use compositor::Compositor;
use guitk::color::Color;
use guitk::render::RenderTree;

/// A window's worth of drawing: a background, a title strip, and rows.
///
/// Deliberately not three commands. The existing ceiling test composites a
/// demo scene of a rectangle, a line and a string, which is a fine canary and
/// a poor measurement: the budget is about a desktop, and a desktop is
/// hundreds of commands across several windows.
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

/// The whole desktop, composited once, in microseconds.
fn one_frame() -> u64 {
    let mut compositor = Compositor::new(3840, 2160, 144).expect("compositor");
    for i in 0..8 {
        let id = compositor.create_window(format!("Window {i}"), 960, 720, 1);
        compositor
            .submit_render(id, window_tree(960.0, 720.0).commands)
            .expect("submit");
    }
    compositor.compose_frame();
    compositor.frame_stats().last_frame_time_us
}

/// A ceiling on a *debug* frame, to catch a regression in kind.
///
/// Two and a half times the measured 160 000 us rather than just over it. The
/// first draft was 200 000, which the measurement cleared by 1.25x -- a bound
/// that tight fails on a busy afternoon and teaches everyone to re-run it,
/// which is how a bound stops being read at all.
const FRAME_CEILING_US: u64 = 400_000;

#[test]
fn a_full_desktop_composites_inside_the_frame_budget() {
    // Best of five fresh compositors. A single sample on a machine running a
    // parallel workspace test measures the machine, and this lane has already
    // spent an afternoon on a ceiling that fired for contention.
    let samples: Vec<u64> = (0..5).map(|_| one_frame()).collect();
    let best = *samples.iter().min().expect("five samples");

    assert!(
        best < FRAME_CEILING_US,
        "the best of five frames took {best} us, over the {FRAME_CEILING_US} us \
         ceiling; all five were {samples:?}. Load cannot explain the *minimum* \
         of five -- see this file's header for the rule that settled the last one."
    );

    println!("eight 960x720 windows at 3840x2160, debug: {best} us (best of {samples:?})");
}
