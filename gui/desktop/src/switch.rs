//! The on/off switch every settings panel draws, in one place.
//!
//! # Why this module exists
//!
//! Seventeen modules of the shell drew this control by hand, and all seventeen
//! made the same mistake: the knob — the little circle that tells you *which
//! side the switch is on* — was filled with `p.text`. On the "off" track
//! (`surface2`) that is fine. On the "on" track it is the user's accent, and
//! the ordinary text colour on a pale accent is a light grey on a light blue:
//! **1.35:1** measured on the stock dark theme, against the 4.5:1 that ordinary
//! text is expected to reach. The one part of the control that carries the
//! state was the one part you could not see.
//!
//! The knob is now [`appearance::readable_on`] of whatever the track is, which
//! is the same helper the shell already uses to put a label on a button. It is
//! *derived* from the track rather than chosen beside it, so a track colour
//! that changes — a new accent, a panel that switches from `accent` to `green`
//! — drags the knob with it. This is the module-49 lesson stated as code: a
//! fill and the ink on top of it are one decision, and separating them is how
//! the ink ends up wrong.
//!
//! # Why a shared function rather than seventeen corrected copies
//!
//! Because seventeen corrected copies is what we had: `mouse_settings.rs` had
//! *already* worked out that a knob on an accent track needs a derived colour
//! and used [`appearance::emphasized`] for its slider, and that fix stayed in
//! the one file where it was made. A correct answer callers cannot reach grows
//! wrong copies — which is the defect the whole 49-module palette conversion
//! existed to remove. Leaving the geometry inline and fixing only the colour
//! would have left the same shape open for the eighteenth panel.
//!
//! # The geometry is not a new opinion
//!
//! Every one of the seventeen hand-written switches obeyed the same rule
//! without ever writing it down: the knob is a circle inset [`INSET`] from
//! every edge of the track, so its diameter is `height - 2 * INSET` and the
//! track's corner radius is `height / 2`. That held at 40x20, 40x22, 36x20 and
//! 36x18 alike. This module states the rule and takes the track's size as
//! arguments, so every existing call site renders the same pixels it did
//! before and only the knob's colour changes.

use appearance::readable_on;
use guitk::color::Color;
use guitk::render::RenderCommand;
use guitk::style::CornerRadii;

/// The gap between the knob and each edge of the track.
///
/// Not a preference: it is the value all seventeen hand-written switches
/// already used, recovered by measuring them.
pub const INSET: f32 = 2.0;

/// Draw an on/off switch: the track, then the knob that sits on it.
///
/// `track` is the colour of the pill, which the caller chooses because the
/// panels disagree about it on purpose — most use `p.accent` for "on", but the
/// accessibility and notification panels use `p.green`, since there "on" means
/// *safe* rather than *selected*. The knob's colour is not a choice: it is
/// [`readable_on`] the track, so it stays visible whatever the caller picked.
///
/// The two commands are returned in painting order, track first.
///
/// # Panics
///
/// Does not panic. A `height` smaller than `2 * INSET` yields a knob of zero
/// or negative diameter, which the renderer draws as nothing — the same as any
/// other empty rectangle.
#[must_use]
pub fn switch(
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    on: bool,
    track: Color,
) -> [RenderCommand; 2] {
    let knob = height - INSET * 2.0;
    let knob_x = if on {
        x + width - knob - INSET
    } else {
        x + INSET
    };
    [
        RenderCommand::FillRect {
            x,
            y,
            width,
            height,
            color: track,
            corner_radii: CornerRadii::all(height / 2.0),
        },
        RenderCommand::FillRect {
            x: knob_x,
            y: y + INSET,
            width: knob,
            height: knob,
            color: readable_on(track),
            corner_radii: CornerRadii::all(knob / 2.0),
        },
    ]
}

#[cfg(test)]
mod tests {
    // A helper handed the wrong command shape has nothing useful to return,
    // and a search over a render this test just built cannot legitimately come
    // up empty — in both cases the panic *is* the failure report, and naming
    // what was found instead is more use than an `Option` the caller would
    // only unwrap. Scoped to this module rather than added to the crate's
    // `cfg_attr(test, ...)` list, which would relax it for fifty others.
    #![allow(clippy::panic, clippy::expect_used)]

    use super::*;
    use appearance::{AccentColor, DARK_EXTREME, LIGHT_EXTREME, Palette};

    fn rect(c: &RenderCommand) -> (f32, f32, f32, f32, Color, f32) {
        match c {
            RenderCommand::FillRect {
                x,
                y,
                width,
                height,
                color,
                corner_radii,
            } => (*x, *y, *width, *height, *color, corner_radii.top_left),
            other => panic!("a switch draws two FillRects, not {other:?}"),
        }
    }

    /// WCAG relative-luminance contrast, the same formula the accessibility
    /// module measures its high-contrast schemes with.
    fn contrast(a: Color, b: Color) -> f64 {
        fn lum(c: Color) -> f64 {
            fn ch(v: u8) -> f64 {
                let v = f64::from(v) / 255.0;
                if v <= 0.03928 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            }
            0.2126 * ch(c.r) + 0.7152 * ch(c.g) + 0.0722 * ch(c.b)
        }
        let (x, y) = (lum(a), lum(b));
        let (hi, lo) = if x > y { (x, y) } else { (y, x) };
        (hi + 0.05) / (lo + 0.05)
    }

    /// The pixels the seventeen hand-written switches drew, recovered from
    /// them before they were replaced. If this module's geometry rule were
    /// wrong, the conversion would have moved something on screen.
    #[test]
    fn the_geometry_is_the_one_every_hand_written_switch_already_used() {
        // (track w, h, expected knob side, expected track radius)
        for (w, h, side) in [
            (40.0, 20.0, 16.0),
            (40.0, 22.0, 18.0),
            (36.0, 20.0, 16.0),
            (36.0, 18.0, 14.0),
        ] {
            for on in [false, true] {
                let cmds = switch(100.0, 50.0, w, h, on, Color::from_hex(0x0080_8080));
                let (tx, ty, tw, th, _, tr) = rect(&cmds[0]);
                assert_eq!((tx, ty, tw, th), (100.0, 50.0, w, h));
                assert!(
                    (tr - h / 2.0).abs() < f32::EPSILON,
                    "a {w}x{h} track's radius is {tr}, not {}",
                    h / 2.0
                );
                let (kx, ky, kw, kh, _, kr) = rect(&cmds[1]);
                assert_eq!((kw, kh), (side, side), "a {w}x{h} switch's knob");
                assert!((kr - side / 2.0).abs() < f32::EPSILON, "the knob is round");
                assert!((ky - (50.0 + INSET)).abs() < f32::EPSILON);
                let expected_x = if on {
                    100.0 + w - side - INSET
                } else {
                    100.0 + INSET
                };
                assert!(
                    (kx - expected_x).abs() < f32::EPSILON,
                    "knob at {kx} with on={on}, expected {expected_x}"
                );
            }
        }
    }

    /// The knob moves, and it moves to the side that says what the state is.
    #[test]
    fn the_knob_is_at_the_right_end_when_on_and_the_left_end_when_off() {
        let bg = Color::from_hex(0x0080_8080);
        let (off_x, ..) = rect(&switch(0.0, 0.0, 40.0, 20.0, false, bg)[1]);
        let (on_x, ..) = rect(&switch(0.0, 0.0, 40.0, 20.0, true, bg)[1]);
        assert!(
            on_x > off_x,
            "the on knob ({on_x}) is right of the off knob ({off_x})"
        );
        // Symmetric: the gap at each end is the same inset.
        assert!((off_x - INSET).abs() < f32::EPSILON, "off knob at {off_x}");
        assert!(
            (on_x - (40.0 - 16.0 - INSET)).abs() < f32::EPSILON,
            "on knob at {on_x}"
        );
    }

    /// On **every** role, the knob is the more legible of the two inks the
    /// shell actually has to offer.
    ///
    /// This is the claim that holds everywhere, and it is deliberately not the
    /// 4.5:1 floor below. `readable_on` chooses between exactly two values —
    /// [`DARK_EXTREME`] and [`LIGHT_EXTREME`] — so on a mid-grey there is no
    /// choice that clears 4.5:1: `overlay0` (`#6C7086`) gives 4.32:1 with the
    /// light extreme and 3.83:1 with the dark one. Demanding 4.5:1 on every
    /// role would therefore be demanding a third ink, which is a different
    /// decision from the one this module makes. What *can* be demanded on
    /// every role is that the choice between the two is the right way round —
    /// and that is worth demanding, because `readable_on` picks with a
    /// **weighted-luma threshold** (`0.299r + 0.587g + 0.114b > 140`) while
    /// legibility is actually governed by WCAG *relative* luminance, which is
    /// gamma-corrected and weighted differently. Two different functions with
    /// the same job: this test is what says they agree on all 42 role-mode
    /// pairs, rather than assuming a fast approximation never crosses over.
    #[test]
    fn the_knob_is_the_more_legible_of_the_two_inks_the_shell_has() {
        for light in [false, true] {
            let p = Palette::for_mode(light);
            for (name, track) in p.roles() {
                let (.., ink, _) = rect(&switch(0.0, 0.0, 40.0, 20.0, true, track)[1]);
                let other = if ink == DARK_EXTREME {
                    LIGHT_EXTREME
                } else {
                    DARK_EXTREME
                };
                let (chosen, rejected) = (contrast(track, ink), contrast(track, other));
                assert!(
                    chosen >= rejected,
                    "on `{name}` in {} mode the knob took {chosen:.2}:1 when \
                     the other ink was worth {rejected:.2}:1",
                    if light { "light" } else { "dark" }
                );
            }
        }
    }

    /// The defect this module exists to remove, stated as a floor over the
    /// tracks that are actually passed.
    ///
    /// A switch track is never `overlay0` or `text`; it is an accent, a
    /// `green`, or one of the three surfaces — the "on" colour a panel chose
    /// and the "off" grey underneath it. Those are the colours a knob has to
    /// survive, and on those 4.5:1 is reachable, so this asserts it. `p.text`
    /// on the stock dark accent reaches 1.35:1, so this test fails outright on
    /// the code that was here before.
    ///
    /// Every *preset* accent is walked rather than a sample, because
    /// `readable_on` is a threshold and one accent samples one side of it.
    /// [`AccentColor::presets`] is the list the appearance page offers, so a
    /// hue added there is covered here without anyone remembering to.
    ///
    /// **There is almost no headroom.** Measured 2026-08-24, the tightest
    /// track in the whole shell is light-mode `Maroon` at **4.60:1** — nine
    /// hundredths above the floor. If you are here because you added an accent
    /// and this failed, the accent is the thing to change, not the number: the
    /// light accents are a hand-tuned set that sits just inside the standard,
    /// and a new hue has to earn its place in it the same way.
    #[test]
    fn the_knob_is_legible_on_every_track_a_panel_can_choose() {
        let mut worst: Option<(String, f64)> = None;
        for light in [false, true] {
            let p = Palette::for_mode(light);
            let mode = if light { "light" } else { "dark" };
            let mut tracks = vec![
                ("green", p.green),
                ("surface0", p.surface0),
                ("surface1", p.surface1),
                ("surface2", p.surface2),
            ];
            for accent in AccentColor::presets() {
                tracks.push((accent.label(), p.hue(*accent)));
            }
            for (name, track) in tracks {
                let c = contrast(track, readable_on(track));
                if worst.as_ref().is_none_or(|(_, w)| c < *w) {
                    worst = Some((format!("`{name}` in {mode} mode"), c));
                }
                // And the module actually draws that colour.
                let (.., ink, _) = rect(&switch(0.0, 0.0, 40.0, 20.0, true, track)[1]);
                assert_eq!(
                    ink,
                    readable_on(track),
                    "the knob on `{name}` in {mode} mode is not the derived ink"
                );
            }
        }
        // Asserted on the *minimum* rather than inside the loop, so the message
        // names the tightest track in the shell rather than whichever one the
        // iteration order happened to reach first.
        let (where_, c) = worst.expect("there are tracks");
        assert!(c >= 4.5, "the tightest knob is on {where_}, at {c:.2}:1");
    }

    /// The knob is derived from the track, not chosen beside it: change the
    /// track and the knob follows without anyone editing a second line.
    #[test]
    fn the_knob_follows_the_track_rather_than_the_theme() {
        let pale = Color::from_hex(0x00F5_E0DC);
        let deep = Color::from_hex(0x0011_1B2B);
        let (.., pale_ink, _) = rect(&switch(0.0, 0.0, 40.0, 20.0, true, pale)[1]);
        let (.., deep_ink, _) = rect(&switch(0.0, 0.0, 40.0, 20.0, true, deep)[1]);
        assert_ne!(
            pale_ink, deep_ink,
            "the same ink on a pale and a deep track means it is not derived"
        );
        assert!(contrast(pale, pale_ink) >= 4.5);
        assert!(contrast(deep, deep_ink) >= 4.5);
    }
}
