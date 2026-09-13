//! The colour arithmetic every legibility question on this desktop is asked in.
//!
//! [`relative_luminance`] and [`contrast_ratio`] are the WCAG 2 definitions;
//! [`contrast_text`] and [`is_dark`] are the ink decision built on them; and
//! [`lighten`], [`darken`] and [`with_alpha`] are the derived-shade helpers.
//! They live in the toolkit, which every other GUI crate already depends on,
//! because a widget asking which ink to use must be able to reach the same
//! answer the shell reaches. `appearance::relative_luminance` and
//! `appearance::contrast_ratio` are re-exports of the two functions here, not
//! second copies.
//!
//! **This module used to be a theme system, and no longer is.** It held a
//! `Theme` struct of thirty-two widget-role colours, four built-in themes
//! written out in hex, a `ThemeManager` to switch between them at runtime with
//! change callbacks, a `ThemeColors` accessor and a `ThemeMode` enum — about
//! 450 lines. **Nothing in the tree ever constructed one**: every consumer of
//! this module imports a colour function and none imports the type. What the
//! desktop actually paints from is `appearance::Palette`, resolved from the
//! user's saved settings, rendered through `gui/desktop`'s `DesktopTheme`.
//!
//! Two theming systems is one too many, and the unused one was the one that
//! was wrong: its hand-written Catppuccin Latte table had comments naming roles
//! one rung off from the values beside them, and its `text_secondary` shipped
//! Catppuccin's `#6C6F85`, which measures 4.37:1 on its own background — under
//! the 4.5:1 that body text needs, and a value `appearance` had already
//! measured and rejected. It was deleted rather than rewritten on 2026-09-03.
//! See design-decisions.md §810 and known-issues.md
//! `TD-C-THE-TOOLKIT-HOLDS-A-THIRD-COPY-OF-THE-PALETTE-AND-DISAGREES-WITH-ITSELF-ABOUT-IT`.

use crate::color::Color;

// ---------------------------------------------------------------------------
// Color Utilities
// ---------------------------------------------------------------------------

/// The relative luminance of an opaque colour, as WCAG 2 defines it:
/// 0.0 for black, 1.0 for white.
///
/// Each channel is un-gamma'd back to light before it is weighted. That step
/// is what separates this from a plain average of the stored bytes: sRGB keeps
/// brightness on a curve, so averaging the bytes measures the *encoding*
/// rather than the light. The curve is the real piecewise one — a bare
/// `powf(2.2)`, which this used to use, is a fair approximation in the middle
/// and wrong near black, where the standard is linear.
///
/// Alpha is ignored. A translucent colour has no luminance of its own until
/// you know what is behind it, so compose first and ask afterwards.
///
/// This lives here, in the crate every other GUI crate already depends on,
/// because it is the input to every legibility question anything drawn on this
/// desktop can ask, and there must be exactly one implementation of it to ask.
/// `appearance::relative_luminance` is this function, re-exported.
#[must_use]
pub fn relative_luminance(c: Color) -> f32 {
    let t = channel_table();
    // SAFETY-of-indexing note: `u8` has 256 values and the table has 256
    // entries, so `usize::from(u8)` is in range by construction. `get` with a
    // fallback would be dead code that can never be taken and would hide that.
    #[allow(clippy::indexing_slicing)]
    {
        0.2126 * t[usize::from(c.r)] + 0.7152 * t[usize::from(c.g)] + 0.0722 * t[usize::from(c.b)]
    }
}

/// The sRGB transfer function, evaluated once for each of its 256 inputs.
///
/// # Why this is a table
///
/// It was three `powf(2.4)` calls per colour, and `powf` is slow enough that
/// it showed up in a frame budget. Measured: `contrast_ratio` cost **436 ns**,
/// which is not a number anything in a render path should pay, and
/// `Palette::from_settings` -- which the compositor calls per blurred window,
/// per frame -- reached **92 us** under the card theme. The compositor's own
/// frame ceiling is what caught it, which is the entire reason that bound has
/// a threshold rather than being a log line.
///
/// A table is exact rather than approximate: a channel is a `u8`, so there are
/// 256 possible inputs and all 256 are precomputed from the same formula.
/// `the_table_is_the_formula` asserts that over every one of them, so this can
/// never drift into an approximation the way a `powf(2.2)` shortcut would.
fn channel_table() -> &'static [f32; 256] {
    static TABLE: std::sync::LazyLock<[f32; 256]> = std::sync::LazyLock::new(|| {
        let mut t = [0.0_f32; 256];
        for (i, slot) in t.iter_mut().enumerate() {
            *slot = srgb_to_linear(u8::try_from(i).unwrap_or(u8::MAX));
        }
        t
    });
    &TABLE
}

/// One channel of the sRGB transfer function, computed rather than looked up.
///
/// The definition [`channel_table`] is built from, and what
/// `the_table_is_the_formula` checks it against. The curve is the real
/// piecewise one -- a bare `powf(2.2)`, which this used to use, is a fair
/// approximation in the middle and wrong near black, where the standard is
/// linear.
/// How many times [`srgb_to_linear`] has been evaluated in this process.
///
/// **This is a load-independent regression guard, and it is compiled into
/// every build on purpose.** The thing it watches for is a colour decision
/// migrating from where the palette is built to inside a pixel loop: on
/// 2026-09-11 `contrast_ratio` was doing three `powf(2.4)` per colour once
/// per blit, and a 4K frame took 67 ms instead of 6.9. The timing test that
/// caught it had to be `#[ignore]`d, because two of its three firings were a
/// merely busy machine and a wall-clock ceiling cannot separate those.
///
/// A count can. The transfer function has 256 possible inputs and
/// [`channel_table`] precomputes all of them once, so *256 for the life of
/// the process* is not a budget chosen to leave headroom -- it is the
/// design property itself, and any per-pixel or per-frame evaluation blows
/// straight past it no matter how fast or loaded the machine is.
///
/// **Why not `#[cfg(test)]`.** A counter behind `cfg(test)` would see only
/// this crate's own unit tests. Every caller that could regress -- the
/// compositor, the desktop, the widgets -- links the *non-test* build of
/// `guitk`, so the latch would sit at zero while the very code it guards ran
/// hot, and report success over a population it cannot see. The cost of
/// compiling it in is 256 relaxed increments per process, once, because the
/// path being instrumented is by construction cold. Instrumenting the cold
/// path is free precisely when the regression is "this path became hot".
static TRANSFER_EVALUATIONS: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

/// How many times the sRGB transfer function has been evaluated so far.
///
/// Expected to reach 256 -- one per possible channel value, when
/// [`channel_table`] is first built -- and never to move again. See
/// [`TRANSFER_EVALUATIONS`] for why this is public and always compiled.
///
/// The count is cumulative and process-wide, so a caller comparing before and
/// after some operation should take a difference rather than an absolute.
#[must_use]
pub fn transfer_function_evaluations() -> u64 {
    TRANSFER_EVALUATIONS.load(core::sync::atomic::Ordering::Relaxed)
}

#[must_use]
fn srgb_to_linear(v: u8) -> f32 {
    // Relaxed: this is a diagnostic total, not a synchronisation point. No
    // reader draws a conclusion about *ordering* from it, only about
    // magnitude, and 256 is three orders of magnitude from any regression
    // this would catch.
    TRANSFER_EVALUATIONS.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    let v = f32::from(v) / 255.0;
    if v <= 0.039_28 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

/// The WCAG 2 contrast ratio between two opaque colours: 1.0 for a colour
/// against itself, 21.0 for black against white.
///
/// The accessibility floors quoted throughout the shell are ratios on this
/// scale: 4.5 for body text (SC 1.4.3) and 3.0 for a control's outline
/// (SC 1.4.11). Ordering does not matter — the lighter of the two is always
/// the numerator.
#[must_use]
pub fn contrast_ratio(a: Color, b: Color) -> f32 {
    let (la, lb) = (relative_luminance(a), relative_luminance(b));
    let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

/// Returns `true` if white ink is more legible on `color` than black ink is.
///
/// Deliberately defined in terms of [`contrast_text`] rather than a threshold
/// of its own, so the two can never disagree about a colour. The name is older
/// than the definition and reads as a question about brightness, but the only
/// question anyone has ever asked it is which ink to use, and that is the
/// question it now answers exactly.
///
/// Note that the crossover is **not** halfway. Black and white swap places at
/// a relative luminance of `sqrt(0.0525) - 0.05`, or about **0.179** — barely
/// a sixth of the way up, because the `+0.05` in the ratio flatters dark
/// backgrounds far less than light ones. Thresholding at 0.5, which this used
/// to do, calls plain red and mid grey "dark" and inks them white when black
/// is the better answer.
#[must_use]
pub fn is_dark(color: Color) -> bool {
    contrast_text(color) == Color::WHITE
}

/// White or black, whichever can actually be read on `background`.
///
/// **The choice is measured, not estimated.** This asks [`contrast_ratio`]
/// which of the two is further from `background` and returns that one, so the
/// answer is the better one *by the same metric the accessibility floors are
/// stated in*. There is no second definition of "bright" here that could
/// disagree with the ratio a test then measures.
///
/// That is worth stating, because the cheaper answer this replaced was wrong
/// for **41.78 % of the 24-bit cube** — not a handful of corners, nearly half
/// of every colour there is. It thresholded luminance at 0.5, and the true
/// crossover is 0.179 (see [`is_dark`]), so every colour in between got the
/// worse ink. `#21D828` is the worst of them: black reaches 10.92:1 on it and
/// white manages 1.92:1, and the old rule returned white. Plain `#FF0000` and
/// plain `#808080` were wrong the same way, at 4.00:1 where 5.25:1 and 5.32:1
/// were available.
///
/// Comparing the ratios has no such corner. The worst background in the whole
/// cube still gets an ink at **4.58:1** — which is the floor this function
/// guarantees for *any* colour whatsoever, and enough for body text under SC
/// 1.4.3. A threshold cannot make that promise; it is only ever as good as the
/// colours that happened to be nearby when it was tuned.
///
/// For a surface that belongs to one of the shell's palettes, prefer
/// `appearance::readable_on`, which answers the same question with that
/// palette's own near-black and near-white so the result still looks like part
/// of this desktop. This function is the right answer for a widget that may be
/// drawn on a background the toolkit knows nothing about.
#[must_use]
pub fn contrast_text(background: Color) -> Color {
    // `>=` rather than `>`: at the exact crossover the two are equally
    // legible, and preferring black keeps the answer stable.
    if contrast_ratio(background, Color::BLACK) >= contrast_ratio(background, Color::WHITE) {
        Color::BLACK
    } else {
        Color::WHITE
    }
}

/// Lighten a color by `amount` (0.0 = unchanged, 1.0 = white).
pub fn lighten(color: Color, amount: f32) -> Color {
    let amount = amount.clamp(0.0, 1.0);
    Color::rgba(
        (color.r as f32 + (255.0 - color.r as f32) * amount) as u8,
        (color.g as f32 + (255.0 - color.g as f32) * amount) as u8,
        (color.b as f32 + (255.0 - color.b as f32) * amount) as u8,
        color.a,
    )
}

/// Darken a color by `amount` (0.0 = unchanged, 1.0 = black).
pub fn darken(color: Color, amount: f32) -> Color {
    let amount = amount.clamp(0.0, 1.0);
    Color::rgba(
        (color.r as f32 * (1.0 - amount)) as u8,
        (color.g as f32 * (1.0 - amount)) as u8,
        (color.b as f32 * (1.0 - amount)) as u8,
        color.a,
    )
}

/// Return a copy of `color` with the specified alpha value.
pub fn with_alpha(color: Color, alpha: u8) -> Color {
    Color::rgba(color.r, color.g, color.b, alpha)
}

// There is deliberately no `mix(a, b, ratio)` here. It existed until
// 2026-09-03 and was [`Color::lerp`] written a second time — same formula,
// same clamp — except that `lerp` reads a NaN `ratio` as "no progress" and
// this one passed it through, because `f32::clamp` does: `NaN as u8` is 0 in
// every channel, so a mix at a NaN ratio returned transparent black rather
// than either end colour. An animation gets a NaN ratio the moment it divides
// elapsed time by a zero duration, which is the case `lerp` documents. Two
// spellings of one operation is how one of them ends up being the wrong one;
// use `a.lerp(b, t)`.

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    // A test module's job is to fail loudly the instant the code under test is
    // wrong, so the defensive lints that forbid exactly that in production code
    // are off here — as `CLAUDE.md` prescribes.
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        clippy::float_cmp
    )]

    use super::*;

    /// The lookup table is the formula, at every one of its 256 inputs.
    ///
    /// The check that makes the optimisation safe to have made. A table that
    /// is *nearly* the formula is the classic way a precomputation becomes an
    /// approximation nobody notices -- and this one feeds every contrast
    /// judgement the desktop makes, so an error near black would show up as
    /// text that passes a 4.5:1 test and cannot be read.
    #[test]
    fn the_table_is_the_formula() {
        for v in 0..=u8::MAX {
            let looked_up = super::channel_table()[usize::from(v)];
            let computed = super::srgb_to_linear(v);
            assert_eq!(
                looked_up.to_bits(),
                computed.to_bits(),
                "channel {v}: table {looked_up} is not the formula's {computed}"
            );
        }
    }

    /// Black, white and mid-grey land where the standard says.
    ///
    /// An independent anchor, so the test above cannot pass by both sides
    /// being wrong in the same way -- which is exactly what it would do if the
    /// formula itself were edited.
    #[test]
    fn the_curve_matches_the_standards_own_anchors() {
        assert!(relative_luminance(Color::rgb(0, 0, 0)).abs() < 1e-6);
        assert!((relative_luminance(Color::rgb(255, 255, 255)) - 1.0).abs() < 1e-5);
        // WCAG's worked example: #808080 has a relative luminance of 0.2159.
        assert!(
            (relative_luminance(Color::rgb(128, 128, 128)) - 0.2159).abs() < 0.001,
            "got {}",
            relative_luminance(Color::rgb(128, 128, 128))
        );
        // 21:1 is the largest ratio the scale can produce.
        assert!(
            (contrast_ratio(Color::rgb(0, 0, 0), Color::rgb(255, 255, 255)) - 21.0).abs() < 1e-4
        );
    }
    #[test]
    fn test_lighten_zero_is_identity() {
        let c = Color::rgb(100, 150, 200);
        let result = lighten(c, 0.0);
        assert_eq!(result, c);
    }

    #[test]
    fn test_lighten_one_is_white() {
        let c = Color::rgb(100, 150, 200);
        let result = lighten(c, 1.0);
        assert_eq!(result.r, 255);
        assert_eq!(result.g, 255);
        assert_eq!(result.b, 255);
    }

    #[test]
    fn test_darken_zero_is_identity() {
        let c = Color::rgb(100, 150, 200);
        let result = darken(c, 0.0);
        assert_eq!(result, c);
    }

    #[test]
    fn test_darken_one_is_black() {
        let c = Color::rgb(100, 150, 200);
        let result = darken(c, 1.0);
        assert_eq!(result.r, 0);
        assert_eq!(result.g, 0);
        assert_eq!(result.b, 0);
    }

    #[test]
    fn test_darken_preserves_alpha() {
        let c = Color::rgba(100, 150, 200, 128);
        let result = darken(c, 0.5);
        assert_eq!(result.a, 128);
    }

    #[test]
    fn test_lighten_preserves_alpha() {
        let c = Color::rgba(100, 150, 200, 128);
        let result = lighten(c, 0.5);
        assert_eq!(result.a, 128);
    }

    #[test]
    fn test_with_alpha() {
        let c = Color::rgb(100, 150, 200);
        let result = with_alpha(c, 128);
        assert_eq!(result.r, 100);
        assert_eq!(result.g, 150);
        assert_eq!(result.b, 200);
        assert_eq!(result.a, 128);
    }

    #[test]
    fn test_contrast_text_on_dark_bg() {
        let dark = Color::rgb(20, 20, 30);
        assert_eq!(contrast_text(dark), Color::WHITE);
    }

    #[test]
    fn test_contrast_text_on_light_bg() {
        let light = Color::rgb(240, 240, 240);
        assert_eq!(contrast_text(light), Color::BLACK);
    }

    #[test]
    fn test_is_dark_black() {
        assert!(is_dark(Color::BLACK));
    }

    #[test]
    fn test_is_dark_white() {
        assert!(!is_dark(Color::WHITE));
    }

    /// The WCAG ratio, transcribed a second time from the specification rather
    /// than called, so that measuring `contrast_text`'s choice below does not
    /// assert the production code against itself. Kept deliberately naive: no
    /// shared helper, no clever factoring, so that a change to
    /// `relative_luminance` cannot quietly change the oracle with it.
    fn wcag(a: Color, b: Color) -> f64 {
        fn lum(c: Color) -> f64 {
            let ch = |v: u8| {
                let v = f64::from(v) / 255.0;
                if v <= 0.03928 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * ch(c.r) + 0.7152 * ch(c.g) + 0.0722 * ch(c.b)
        }
        let (x, y) = (lum(a), lum(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    #[test]
    fn the_published_ratios_are_what_this_module_computes() {
        // The two definitional anchors. Black on white is 21:1 by
        // construction, and any colour against itself is 1:1.
        assert!((contrast_ratio(Color::BLACK, Color::WHITE) - 21.0).abs() < 1e-3);
        assert!((contrast_ratio(Color::WHITE, Color::BLACK) - 21.0).abs() < 1e-3);
        assert!((contrast_ratio(Color::WHITE, Color::WHITE) - 1.0).abs() < 1e-6);

        // And agreement with an independent transcription, on colours chosen
        // to sit in each branch of the sRGB curve.
        for c in [
            Color::rgb(0, 0, 0),
            Color::rgb(8, 8, 8),
            Color::rgb(0x21, 0xD8, 0x28),
            Color::rgb(0xFF, 0x00, 0x00),
            Color::rgb(0x80, 0x80, 0x80),
        ] {
            for other in [Color::BLACK, Color::WHITE] {
                let (got, want) = (contrast_ratio(c, other), wcag(c, other));
                assert!(
                    (f64::from(got) - want).abs() < 1e-3,
                    "contrast_ratio({c:?}, {other:?}) = {got}, the specification says {want}"
                );
            }
        }
    }

    /// The ink returned is the more legible of the two, for every colour.
    ///
    /// **What this cannot prove, deliberately:** that the luminance curve is
    /// right. Every assertion here is written in terms of `contrast_text` and
    /// `contrast_ratio`, which share one `relative_luminance` — so replacing
    /// the sRGB curve with something else entirely moves both sides of every
    /// comparison together. `chosen >= other` survives (it is the maximum by
    /// construction), and even the 4.58:1 floor survives, because for *any*
    /// luminance mapping into `[0, 1]` the worst case sits at the crossover
    /// and equals `1.05 / sqrt(1.05 * 0.05)`. Reintroducing a raw-bytes
    /// luminance in the 2026-08-24 sweep confirmed this test stays green.
    ///
    /// So this test pins the *decision rule* and
    /// `the_published_ratios_are_what_this_module_computes` — which checks
    /// against an independent f64 transcription of the standard — pins the
    /// *arithmetic*. Neither substitutes for the other, and removing either
    /// leaves a real defect with nothing to catch it.
    #[test]
    fn the_chosen_ink_is_the_more_legible_of_the_two_for_every_colour_there_is() {
        // A grid over the whole cube rather than a handful of samples,
        // because the rule this replaced was correct on every colour anyone
        // had thought to test and wrong on 41.78% of the rest. 18 levels per
        // channel steps past the crossover in every hue.
        let mut worst = f32::MAX;
        let mut worst_at = Color::BLACK;
        let mut step = 0u32;
        while step < 18 * 18 * 18 {
            let at = |shift: u32| {
                #[allow(clippy::cast_possible_truncation)]
                {
                    (step / 18u32.pow(shift) % 18 * 255 / 17) as u8
                }
            };
            let bg = Color::rgb(at(0), at(1), at(2));
            let ink = contrast_text(bg);
            let rejected = if ink == Color::BLACK {
                Color::WHITE
            } else {
                Color::BLACK
            };
            let (chosen, other) = (contrast_ratio(bg, ink), contrast_ratio(bg, rejected));
            assert!(
                chosen >= other,
                "on {bg:?} the returned ink reaches {chosen}:1 and the one it \
                 passed over reaches {other}:1"
            );
            if chosen < worst {
                worst = chosen;
                worst_at = bg;
            }
            step += 1;
        }
        // Black and white swap at a luminance of sqrt(0.0525) - 0.05, where
        // both reach sqrt(0.0525)/0.05. Nothing can be worse than that, so
        // this is a floor for *any* colour, not a property of the sample.
        assert!(
            worst >= 4.58,
            "the worst background found was {worst_at:?} at {worst}:1, below \
             the 4.58:1 the crossover guarantees"
        );
    }

    #[test]
    fn plain_red_and_mid_grey_are_lettered_in_black() {
        // The three worked examples from `contrast_text`'s doc comment. Each
        // was inked white by the luminance-at-0.5 rule this replaced, and each
        // reads better in black. `#21D828` is the worst case in the entire
        // cube: 10.92:1 was available and the old rule returned 1.92:1.
        for (bg, black, white) in [
            (Color::rgb(0x21, 0xD8, 0x28), 10.92, 1.92),
            (Color::rgb(0xFF, 0x00, 0x00), 5.25, 4.00),
            (Color::rgb(0x80, 0x80, 0x80), 5.32, 3.95),
        ] {
            assert_eq!(contrast_text(bg), Color::BLACK, "{bg:?} should ink black");
            assert!(!is_dark(bg), "{bg:?} should not be called dark");
            assert!((contrast_ratio(bg, Color::BLACK) - black).abs() < 0.01);
            assert!((contrast_ratio(bg, Color::WHITE) - white).abs() < 0.01);
        }
    }

    #[test]
    fn is_dark_and_contrast_text_cannot_disagree() {
        // `is_dark` is defined in terms of `contrast_text`, so this is a
        // tautology today. It is here to fail the moment someone gives
        // `is_dark` a threshold of its own again, which is how the two came
        // apart the first time.
        let mut step = 0u32;
        while step < 12 * 12 * 12 {
            let at = |shift: u32| {
                #[allow(clippy::cast_possible_truncation)]
                {
                    (step / 12u32.pow(shift) % 12 * 255 / 11) as u8
                }
            };
            let bg = Color::rgb(at(0), at(1), at(2));
            assert_eq!(
                is_dark(bg),
                contrast_text(bg) == Color::WHITE,
                "{bg:?} is called dark by one of the pair and not the other"
            );
            step += 1;
        }
    }

    #[test]
    fn test_lighten_clamps_above_one() {
        let c = Color::rgb(100, 100, 100);
        let result = lighten(c, 2.0); // Should clamp to 1.0
        assert_eq!(result.r, 255);
    }

    #[test]
    fn test_darken_clamps_below_zero() {
        let c = Color::rgb(100, 100, 100);
        let result = darken(c, -1.0); // Should clamp to 0.0
        assert_eq!(result, c);
    }

    /// This module names no colour of its own, and something checks.
    ///
    /// It used to name eighty-odd — a whole Catppuccin palette in both modes,
    /// plus two high-contrast themes, written out in hex. That was the third
    /// copy of the palette in the tree and the one that disagreed with the
    /// other two, and it is gone (see this module's documentation for what
    /// went and why). But "gone" is not a property anything checks, and the
    /// impulse that put it here — *the toolkit ought to know what a button
    /// looks like* — is a reasonable-sounding thought that will occur to the
    /// next author as readily as it did to the last one.
    ///
    /// So this reads the module's own source and fails if a colour literal has
    /// come back. Every function above takes its colours as arguments and
    /// returns colours derived from them; not one of them needs to know a
    /// particular colour, and the day one does is the day this crate has a
    /// palette again. The one palette is `appearance::Palette`, resolved from
    /// the user's saved settings — a colour written here instead would be
    /// beyond the reach of the light/dark switch, the accent setting and the
    /// contrast sweeps that guard them, exactly as the deleted one was.
    ///
    /// `Color::BLACK` and `Color::WHITE` are deliberately not caught: they are
    /// the two ends of [`contrast_text`]'s choice, they belong to no palette,
    /// and a legibility floor stated against pure black and pure white is a
    /// floor for every colour rather than for this desktop's.
    #[test]
    fn this_module_names_no_colours_of_its_own() {
        // Assembled from pieces so that this test's own text is not the thing
        // it finds, and spelled nowhere else in this file for the same reason —
        // a comment that names it in full would fail this test from inside it.
        // It is the hex-literal colour constructor, which is how all three
        // copies of the palette were written: here, in `appearance`, in `apps/`.
        let spelling = concat!("from_", "hex(");
        let source = include_str!("theme.rs");
        assert!(
            !source.contains(spelling),
            "a colour literal is back in guitk::theme — the palette belongs to \
             appearance::Palette, which the light/dark switch and the contrast \
             sweeps can actually reach"
        );
    }
}
