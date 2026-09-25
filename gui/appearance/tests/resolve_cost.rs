//! Resolving a palette stays cheap enough for the render path.
//!
//! `Palette::from_settings` is called by the compositor **per blurred window,
//! per frame** (`gui/compositor/src/lib.rs`, the blur pass). So its cost is
//! not a startup cost, and a regression in it is a regression in the frame
//! budget -- which is exactly how this test came to exist.
//!
//! # What happened
//!
//! Adding the 4.5:1 text floor (§837) made resolution call `contrast_ratio`,
//! which was three `powf(2.4)` evaluations per colour. Measured after:
//!
//! | | before the table | after |
//! |---|---|---|
//! | `contrast_ratio` | 436 ns | 4 ns |
//! | `Palette::for_mode` | 3 775 ns | 115 ns |
//! | `from_settings`, bordered theme | 11 343 ns | 273 ns |
//! | `from_settings`, card theme | 87 395 ns | 3 102 ns |
//!
//! The compositor's own frame ceiling caught it, at 67 074 us against a
//! 50 000 us bound -- an instrument with a threshold, firing. The fix was a
//! 256-entry table for the sRGB transfer function, which is exact rather than
//! approximate because a channel is a `u8` and all 256 inputs are precomputed
//! from the same formula (`guitk::theme::the_table_is_the_formula`).
//!
//! # Why the ceilings are where they are
//!
//! Twenty times the measured figure. Wide, because this is wall-clock on a
//! loaded developer machine and a tight bound would fail for reasons that have
//! nothing to do with the code. Still useful, because the regression it exists
//! to catch was 30-100x: a `powf` creeping back in does not cost 50%, it costs
//! two orders of magnitude. A bound that only catches a catastrophe is the
//! right bound when only catastrophes are possible.
//!
//! **That reasoning is right about the bound and was wrong to stop there.** A
//! wide bound makes host noise *less likely* to fail the test; it does not
//! make it unable to. The measurement itself is now the best of three runs
//! (see `per_call`), which is the other half of the answer and the one lane A
//! settled on in design-decisions.md §952 -- a measurement the host can
//! distort needs a repeat, not only a wider bound. With both, the ceiling
//! could be tightened; it deliberately has not been, because the regression
//! this exists to catch is two orders of magnitude and a tighter bound would
//! buy sensitivity nobody needs at the price of the flakiness just removed.
// A benchmark divides and asserts on the result; the defensive lints that
// forbid that in production code are off here, as `CLAUDE.md` prescribes for
// test code.
#![allow(clippy::arithmetic_side_effects, clippy::panic)]

use appearance::{AppearanceSettings, Palette, SurfaceStyle};
use std::time::Instant;

/// Nanoseconds per call, over `n` iterations after a warm-up, taking the best
/// of three runs.
///
/// **The minimum rather than the mean, and three runs rather than one.** The
/// thing being measured is the code; the thing that distorts it is whatever
/// else the machine is doing. Load can only ever push a measurement *up* -- a
/// descheduled thread does not run faster -- so the smallest of several
/// samples is the one least contaminated by the host, while a genuine
/// regression raises every sample including the smallest. That makes the
/// statistic one-sided in the same direction as the noise, which is what lets
/// a ceiling mean something.
///
/// Lane A reached this from a red boot whose kernel delta was comment text
/// (design-decisions.md §952): *a measurement the host can distort needs a
/// repeat, not a wider bound.* This test originally took the other option --
/// see the module doc above -- and a sibling in `apps/benchmark` failed a
/// `cargo test --workspace` on exactly this, a 5ms sleep measuring 1.407s
/// because it was descheduled. Averaging over 50 000 iterations makes that
/// far less likely here than it was there, not impossible, and a red
/// workspace run blocks all three lanes rather than just this one.
fn per_call(n: u32, mut f: impl FnMut()) -> u128 {
    for _ in 0..1000 {
        f();
    }
    let mut best = u128::MAX;
    for _ in 0..3 {
        let t = Instant::now();
        for _ in 0..n {
            f();
        }
        best = best.min(t.elapsed().as_nanos() / u128::from(n));
    }
    best
}

#[test]
fn resolving_a_palette_stays_cheap_enough_for_a_frame() {
    let settings = AppearanceSettings {
        surface_style: SurfaceStyle::Borders,
        ..AppearanceSettings::default()
    };
    let bordered = per_call(50_000, || {
        std::hint::black_box(Palette::from_settings(&settings));
    });
    assert!(
        bordered < 11_000,
        "resolving a bordered palette took {bordered} ns; it measured 1 867 ns \
         in a debug build when this bound was set (273 ns release, and 11 343 \
         ns release before the sRGB table). Six times over is a `powf` back in \
         the contrast path, not noise."
    );

    let settings = AppearanceSettings {
        surface_style: SurfaceStyle::Cards,
        ..AppearanceSettings::default()
    };
    let carded = per_call(20_000, || {
        std::hint::black_box(Palette::from_settings(&settings));
    });
    assert!(
        carded < 62_000,
        "resolving a card palette took {carded} ns; it measured 10 264 ns in a \
         debug build when this bound was set (3 102 ns release, and 87 395 ns \
         release before the sRGB table)."
    );

    // Printed on success as well, so the next person to read this has the
    // number rather than only the bound. A bound whose value nobody can
    // explain is the one that gets relaxed.
    println!("from_settings: bordered {bordered} ns, carded {carded} ns");
}
