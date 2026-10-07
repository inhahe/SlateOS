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
//! make it unable to. The measurement itself is now the fastest of a hundred
//! short runs (see `per_call`), which is the other half of the answer and the
//! one lane A settled on in design-decisions.md §952 -- a measurement the host
//! can distort needs a repeat, not only a wider bound. With both, the ceiling
//! could be tightened; it deliberately has not been, because the regression
//! this exists to catch is two orders of magnitude and a tighter bound would
//! buy sensitivity nobody needs at the price of the flakiness just removed.
//!
//! # What it costs now
//!
//! Measured 2026-10-05 beside the code the ceilings were set against
//! (`5cbb2ee60`, 2026-09-12), the two built and run back to back on one
//! loaded machine: the bordered palette costs half as much again as it did
//! (3 359 ns then, 5 014 ns now, under that load), the card palette a tenth
//! more (16 053, 17 438). The palette has gained the widget style, the
//! motion, the window frames, the taskbar panel and the terminal and syntax
//! colours since -- work, not a `powf`: a `powf` costs thirty times, not
//! one and a half. The ceilings stand at four and five times the cost.
// A benchmark divides and asserts on the result; the defensive lints that
// forbid that in production code are off here, as `CLAUDE.md` prescribes for
// test code.
#![allow(clippy::arithmetic_side_effects, clippy::panic)]

use appearance::{AppearanceSettings, Palette, SurfaceStyle};
use std::time::Instant;

/// Nanoseconds per call: `runs` runs of `per_run` calls each, after a
/// warm-up, and the fastest run's figure.
///
/// **The minimum rather than the mean.** The thing being measured is the
/// code; the thing that distorts it is whatever else the machine is doing.
/// Load can only ever push a measurement *up* -- a descheduled thread does
/// not run faster -- so the smallest of several samples is the one least
/// contaminated by the host, while a genuine regression raises every sample
/// including the smallest. That makes the statistic one-sided in the same
/// direction as the noise, which is what lets a ceiling mean something.
///
/// Lane A reached this from a red boot whose kernel delta was comment text
/// (design-decisions.md §952): *a measurement the host can distort needs a
/// repeat, not a wider bound.* This test originally took the other option --
/// see the module doc above -- and a sibling in `apps/benchmark` failed a
/// `cargo test --workspace` on exactly this, a 5ms sleep measuring 1.407s
/// because it was descheduled.
///
/// **Many short runs, because load is not always brief.** This was the best
/// of three runs of 50 000 calls, a tenth of a second each in a debug build.
/// On 2026-10-05 a workspace run at below-normal priority, beside a boot
/// test's builds at normal priority, measured the bordered palette at
/// 16 645 ns against this ceiling of 11 000 -- every core was busy for the
/// whole of all three runs, and a thread below them in priority ran through
/// each in pieces. A run of a millisecond or two fits inside one of the
/// scheduler's time slices, which are tens of milliseconds on a Windows
/// desktop and a few on Linux, so of a hundred such runs some start at the top
/// of a slice and finish inside it, untouched however busy the machine is --
/// and the minimum is theirs. The calls measured are as many as before.
fn per_call(runs: u32, per_run: u32, mut f: impl FnMut()) -> u128 {
    for _ in 0..1000 {
        f();
    }
    let mut best = u128::MAX;
    for _ in 0..runs {
        let t = Instant::now();
        for _ in 0..per_run {
            f();
        }
        best = best.min(t.elapsed().as_nanos() / u128::from(per_run));
    }
    best
}

#[test]
fn resolving_a_palette_stays_cheap_enough_for_a_frame() {
    let settings = AppearanceSettings {
        surface_style: SurfaceStyle::Borders,
        ..AppearanceSettings::default()
    };
    let bordered = per_call(100, 500, || {
        std::hint::black_box(Palette::from_settings(&settings));
    });
    assert!(
        bordered < 11_000,
        "resolving a bordered palette took {bordered} ns; it measured 1 867 ns \
         in a debug build when this bound was set (273 ns release, and 11 343 \
         ns release before the sRGB table), and half as much again on \
         2026-10-05 (the module doc). Four times over is a `powf` back in the \
         contrast path, not noise."
    );

    let settings = AppearanceSettings {
        surface_style: SurfaceStyle::Cards,
        ..AppearanceSettings::default()
    };
    let carded = per_call(100, 200, || {
        std::hint::black_box(Palette::from_settings(&settings));
    });
    assert!(
        carded < 62_000,
        "resolving a card palette took {carded} ns; it measured 10 264 ns in a \
         debug build when this bound was set (3 102 ns release, and 87 395 ns \
         release before the sRGB table), and a tenth more on 2026-10-05 (the \
         module doc)."
    );

    // Printed on success as well, so the next person to read this has the
    // number rather than only the bound. A bound whose value nobody can
    // explain is the one that gets relaxed.
    println!("from_settings: bordered {bordered} ns, carded {carded} ns");
}
