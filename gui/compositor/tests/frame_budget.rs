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
fn one_frame_at(width: u32, height: u32) -> u64 {
    let mut compositor = Compositor::new(width, height, 144).expect("compositor");
    let (w, h) = (width / 4, height / 3);
    for i in 0..8 {
        let id = compositor.create_window(format!("Window {i}"), w, h, 1);
        // Placed apart, and this is not cosmetic: eight windows left at the
        // same origin all overlap, so damaging one damages every one of them
        // and the partial path becomes a full recomposite wearing a different
        // name. Measured stacked, one window's redraw cost 73% of the whole
        // desktop's; placed apart it costs an eighth, which is what damage
        // tracking is for.
        compositor
            .move_window(id, (i % 4) * w as i32, (i / 4) * h as i32)
            .expect("move");
        compositor
            .submit_render(id, window_tree(w as f32, h as f32).commands)
            .expect("submit");
    }
    compositor.compose_frame();
    compositor.frame_stats().last_frame_time_us
}

/// One window redraws its own content: the steady state of a running desktop.
///
/// The refresh rate is absurd on purpose. `FrameStats::should_compose` gates
/// on the refresh interval, so a second `compose_frame` inside it returns
/// early -- at 144 Hz this experiment cannot be run at all. Handing the
/// compositor a 100 kHz display turns the rate limiter from an obstacle into
/// a parameter.
fn steady_frame() -> u64 {
    // 1080p, not 4K: this feeds the *ratio* test, which asks whether the
    // partial path is narrower than the full one and does not care how many
    // pixels either walks. A quarter of the pixels is a quarter of the cost
    // to everything else running beside it.
    let mut compositor = Compositor::new(1920, 1080, 100_000).expect("compositor");
    let mut first = None;
    for i in 0..8 {
        let id = compositor.create_window(format!("Window {i}"), 480, 360, 1);
        compositor
            .move_window(id, (i % 4) * 480, (i / 4) * 540)
            .expect("move");
        compositor
            .submit_render(id, window_tree(480.0, 360.0).commands)
            .expect("submit");
        first.get_or_insert(id);
    }
    compositor.compose_frame();

    let id = first.expect("eight windows");
    compositor
        .submit_render(id, window_tree(480.0, 360.0).commands)
        .expect("submit");
    compositor.compose_frame();
    compositor.frame_stats().last_frame_time_us
}

/// A ceiling on a *debug* frame, to catch a regression in kind.
///
/// Three times the measured ~360 000 us rather than just over it.
///
/// This number moved twice while the test was being written, and both moves
/// are the same lesson. The first draft was 200 000 against a 160 000 us
/// measurement -- 1.25x -- and one of the five samples on the very next run
/// exceeded it. The second was 400 000, and then *placing the windows apart*
/// (which is what makes the damage test meaningful) more than doubled the
/// full-recomposite cost, because eight spread windows cover far more of a
/// 4K screen than eight stacked ones. A bound with 10% headroom fails on a
/// busy afternoon and teaches everyone to re-run it, which is how a bound
/// stops being read at all.
const FRAME_CEILING_US: u64 = 1_200_000;

/// Ignored by default, and the reason is the whole point of the test.
///
/// Compositing eight 4K frames five times over saturates memory bandwidth for
/// a second or two. Run alongside the rest of the workspace that is enough to
/// push the *neighbouring* timing test --
/// `compositor::tests::the_demo_scene_still_composites`, whose ceiling has 10x
/// headroom -- to 27x its median and fail it. A timing test heavy enough to
/// break other timing tests is worse than no timing test.
///
/// Run it on demand:
///
/// ```text
/// cargo test -p compositor --release --test frame_budget -- --ignored --nocapture
/// ```
#[test]
#[ignore = "saturates memory bandwidth; run on demand, see the doc comment"]
fn a_full_desktop_composites_inside_the_frame_budget() {
    // Best of five fresh compositors. A single sample on a machine running a
    // parallel workspace test measures the machine, and this lane has already
    // spent an afternoon on a ceiling that fired for contention.
    let samples: Vec<u64> = (0..5).map(|_| one_frame_at(3840, 2160)).collect();
    let best = *samples.iter().min().expect("five samples");

    assert!(
        best < FRAME_CEILING_US,
        "the best of five frames took {best} us, over the {FRAME_CEILING_US} us \
         ceiling; all five were {samples:?}. Load cannot explain the *minimum* \
         of five -- see this file's header for the rule that settled the last one."
    );

    println!("eight 960x720 windows at 3840x2160, debug: {best} us (best of {samples:?})");
}

/// The steady state -- one window of eight redrawing -- is a fraction of a
/// full recomposite, which is what says damage tracking is doing its job.
/// This one *does* run in the ordinary suite, and can afford to: it asserts a
/// ratio, so it needs neither a 4K surface nor five samples. Three frames at
/// 1920x1080 cost a tenth of what the measurement above does.
#[test]
fn redrawing_one_window_costs_a_fraction_of_the_whole_desktop() {
    let full = (0..3)
        .map(|_| one_frame_at(1920, 1080))
        .min()
        .expect("three");
    let steady = (0..3).map(|_| steady_frame()).min().expect("three");

    println!("full recomposite {full} us, one window redrawn {steady} us");

    // A third, not an eighth. The ratio is what is being asserted -- that the
    // partial path is genuinely partial -- and a tight ratio would fail for
    // the ordinary reason that these are two different measurements on a
    // shared machine. An eighth is what it actually measures.
    assert!(
        steady * 3 < full,
        "redrawing one window of eight cost {steady} us against {full} us for          the whole desktop. Damage tracking is not narrowing the work: check          whether the windows overlap, which makes every partial recomposite a          full one."
    );
}
