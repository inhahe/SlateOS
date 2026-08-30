//! The horizontal value slider every settings panel draws, in one place.
//!
//! # Why this module exists
//!
//! Five modules of the shell drew this control by hand — `display_settings`,
//! `notif_pane`, `osd`, `touchpad` and `mouse_settings` — and they disagreed
//! about the one thing that matters, the colour of the thumb. Three used
//! `p.text`, one used [`appearance::emphasized`] of the fill, and `touchpad`
//! used `p.accent`: **the same colour as the fill it sits on top of**. That is
//! 1.00:1. On the left half of its travel the touchpad thumb was not merely
//! hard to see, it was not there — the handle and the filled track were one
//! undifferentiated accent-coloured blob, and the only cue to where the value
//! sat was the blob's ragged right edge.
//!
//! Same shape as the switch defect [`crate::switch`] fixed: one control, five
//! hand-drawn copies, and a correct answer in one of them that could not reach
//! the other four.
//!
//! # The thumb is `p.text`, which is the *opposite* rule from a switch knob
//!
//! [`crate::switch`] derives its knob with [`appearance::readable_on`] of the
//! track. Doing that here would be wrong, and the reason is geometry, not
//! taste.
//!
//! A switch knob is *contained* by its track: inset two pixels from every edge,
//! so the track is the only thing behind it and the only thing it has to be
//! legible against. A slider thumb is **larger than its track** — 10 to 14
//! pixels of circle on a track 4 or 6 pixels tall — so most of it hangs over
//! the panel. Its silhouette, the round outline that says *this is the handle*,
//! is drawn against the card, not against the fill. Pick the ink for the fill
//! and you lose the silhouette. Measured on the stock dark theme with a `blue`
//! accent, a thumb of `readable_on(accent)` is `#11111B`, which is Mocha
//! `crust` — on a `base` card that is 1.1:1, an invisible handle with a crisp
//! interior nobody can see.
//!
//! `p.text` is the other way round: 11.34:1 against the card and 1.46:1 against
//! the fill. The weak number is the one that costs nothing, because the fill
//! only ever touches the thumb's *interior*, which carries no information once
//! you can see the circle.
//!
//! This is why the fix is not "make the two controls consistent." They have
//! different backgrounds, so they take different inks, and the module that
//! draws each one is where that reasoning is allowed to live.
//!
//! # The geometry is recovered, not invented
//!
//! All five hand-written sliders already agreed on it: the track's corner
//! radius is `height / 2`; the thumb is a circle of its own diameter centred on
//! `(x + fill_width, y + height / 2)` — the end of the filled portion, on the
//! track's midline. That held at every combination the shell uses (track
//! heights 4 and 6, thumbs 10, 12 and 14), so converting the call sites moves
//! nothing on screen except the touchpad thumb's colour.

use appearance::Palette;
use guitk::color::Color;
use guitk::render::RenderCommand;
use guitk::style::CornerRadii;

/// Re-apply an alpha to a role colour.
///
/// The OSD fades its whole overlay in and out, so every colour it draws is its
/// palette role at the overlay's current opacity. Rather than have that one
/// caller pre-multiply three colours and the other four pass them straight
/// through — which is how the thumb's ink became a caller's choice in the
/// first place — the fade happens here, once, to all three.
fn fade(c: Color, alpha: u8) -> Color {
    if alpha == u8::MAX {
        c
    } else {
        Color::rgba(c.r, c.g, c.b, alpha)
    }
}

/// A horizontal value slider: a track, the portion of it that is filled, and
/// the thumb at the boundary between them.
///
/// The thumb's colour is deliberately **not** a field. It is `p.text` at
/// `alpha`, for the reason in the module docs, and making it a parameter is
/// exactly what produced a thumb the same colour as its own fill.
#[derive(Clone, Copy, Debug)]
pub struct Slider<'a> {
    /// Left edge of the track. The thumb overhangs this by half its diameter
    /// at `frac == 0.0`, which is what every hand-written slider already did.
    pub x: f32,
    /// Top edge of the *track*, not of the thumb.
    pub y: f32,
    pub width: f32,
    /// Height of the track. The thumb is taller; see the module docs.
    pub height: f32,
    /// How full, in `0.0..=1.0`. Clamped, so a caller that divides by a
    /// user-supplied maximum cannot push the thumb off the end of the track.
    pub frac: f32,
    /// Thumb diameter.
    pub thumb: f32,
    /// The unfilled part of the track — a surface role.
    pub track: Color,
    /// The filled part. Usually `p.accent`; the panels that mean *safe* rather
    /// than *selected* pass `p.green`, the same split as [`crate::switch`].
    pub fill: Color,
    /// Read for `text` only, to ink the thumb.
    pub p: &'a Palette,
    /// Opacity applied to all three colours. `u8::MAX` for an opaque control.
    pub alpha: u8,
}

impl Slider<'_> {
    /// Append this slider's commands to `cmds`, in painting order: track,
    /// then fill, then thumb.
    ///
    /// A slider sitting on its floor has nothing to fill, and a zero-width
    /// rectangle is a command the compositor has to carry and cannot draw — on
    /// every frame, for as long as the value stays there. It is emitted only
    /// when it covers something, so callers must not assume a fixed command
    /// count. (Two of the five hand-written sliders already skipped it; this
    /// makes the other three agree.)
    pub fn draw(&self, cmds: &mut Vec<RenderCommand>) {
        let radius = CornerRadii::all(self.height / 2.0);
        cmds.push(RenderCommand::FillRect {
            x: self.x,
            y: self.y,
            width: self.width,
            height: self.height,
            color: fade(self.track, self.alpha),
            corner_radii: radius,
        });
        // `f32::clamp` propagates NaN rather than bounding it, and a NaN here
        // would reach the compositor as a rectangle at an undefined position.
        // A fraction that is not a number is not a value the control has, so
        // it reads as empty.
        let frac = if self.frac.is_nan() {
            0.0
        } else {
            self.frac.clamp(0.0, 1.0)
        };
        let fill_w = self.width * frac;
        if fill_w > 0.0 {
            cmds.push(RenderCommand::FillRect {
                x: self.x,
                y: self.y,
                width: fill_w,
                height: self.height,
                color: fade(self.fill, self.alpha),
                corner_radii: radius,
            });
        }
        cmds.push(RenderCommand::FillRect {
            x: self.x + fill_w - self.thumb / 2.0,
            y: self.y + self.height / 2.0 - self.thumb / 2.0,
            width: self.thumb,
            height: self.thumb,
            color: fade(self.p.text, self.alpha),
            corner_radii: CornerRadii::all(self.thumb / 2.0),
        });
    }
}

#[cfg(test)]
mod tests {
    // A helper handed the wrong command shape has nothing useful to return,
    // and the panic names what it got instead, which is more use than an
    // `Option` the caller would only unwrap. Indexing a render this module
    // just built is the same argument: the command at position 0 is there by
    // construction, and if it is not, the index panic *is* the failure the
    // test exists to report. Scoped here rather than added to the crate's
    // `cfg_attr(test, ...)` list, which would relax all three for fifty other
    // modules.
    #![allow(clippy::panic, clippy::expect_used, clippy::indexing_slicing)]

    use super::*;
    use appearance::{AccentColor, readable_on};

    /// The four shapes the shell actually draws, as `(track height, thumb)`.
    const SHAPES: [(f32, f32); 4] = [(6.0, 12.0), (6.0, 10.0), (6.0, 14.0), (4.0, 12.0)];

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
            other => panic!("a slider draws FillRects, not {other:?}"),
        }
    }

    fn draw(p: &Palette, height: f32, thumb: f32, frac: f32) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();
        Slider {
            x: 100.0,
            y: 50.0,
            width: 150.0,
            height,
            frac,
            thumb,
            track: p.surface1,
            fill: p.accent,
            p,
            alpha: u8::MAX,
        }
        .draw(&mut cmds);
        cmds
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

    /// The pixels the five hand-written sliders drew, recovered from them
    /// before they were replaced. If the geometry rule were wrong, the
    /// conversion would have moved something on screen.
    #[test]
    fn the_geometry_is_the_one_every_hand_written_slider_already_used() {
        let p = Palette::for_mode(false);
        for (height, thumb) in SHAPES {
            for frac in [0.0_f32, 0.25, 1.0] {
                let cmds = draw(&p, height, thumb, frac);
                let (tx, ty, tw, th, _, tr) = rect(&cmds[0]);
                assert_eq!((tx, ty, tw, th), (100.0, 50.0, 150.0, height));
                assert!(
                    (tr - height / 2.0).abs() < f32::EPSILON,
                    "a track {height} tall has radius {tr}"
                );
                let fill_w = 150.0 * frac;
                if frac > 0.0 {
                    let (fx, fy, fw, fh, _, fr) = rect(&cmds[1]);
                    assert_eq!((fx, fy, fh), (100.0, 50.0, height));
                    assert!((fw - fill_w).abs() < f32::EPSILON, "fill width {fw}");
                    assert!((fr - height / 2.0).abs() < f32::EPSILON);
                }
                let last = cmds.last().expect("a slider draws at least two rects");
                let (kx, ky, kw, kh, _, kr) = rect(last);
                assert_eq!((kw, kh), (thumb, thumb), "the thumb is square");
                assert!(
                    (kr - thumb / 2.0).abs() < f32::EPSILON,
                    "the thumb is round"
                );
                // Centred on the end of the fill, on the track's midline.
                assert!(
                    (kx + thumb / 2.0 - (100.0 + fill_w)).abs() < 1e-3,
                    "thumb centre x is {}, fill ends at {}",
                    kx + thumb / 2.0,
                    100.0 + fill_w
                );
                assert!(
                    (ky + thumb / 2.0 - (50.0 + height / 2.0)).abs() < f32::EPSILON,
                    "thumb centre y is {}, track midline is {}",
                    ky + thumb / 2.0,
                    50.0 + height / 2.0
                );
            }
        }
    }

    /// The premise of the whole ink rule, asserted rather than assumed: the
    /// thumb hangs over the track on both sides, so what is behind most of it
    /// is the card, not the fill.
    ///
    /// If a shape were ever added whose thumb fitted *inside* its track, this
    /// module's reasoning would no longer apply to it and the ink would have to
    /// be reconsidered — which is what this test is for.
    #[test]
    fn the_thumb_overhangs_the_track_on_every_shape_the_shell_draws() {
        let p = Palette::for_mode(false);
        for (height, thumb) in SHAPES {
            let cmds = draw(&p, height, thumb, 0.5);
            let (_, ky, _, kh, ..) = rect(cmds.last().expect("a thumb"));
            assert!(
                ky < 50.0 && ky + kh > 50.0 + height,
                "a {thumb}px thumb on a {height}px track spans {ky}..{} — \
                 it does not overhang 50.0..{}",
                ky + kh,
                50.0 + height
            );
        }
    }

    /// The thumb is `text`, and `text` is what a card can actually show.
    ///
    /// Sliders sit on cards, and a card in this shell is `base`, `mantle`,
    /// `crust` or one of the three surfaces. The thumb's outline is read
    /// against whichever of those is behind it, so that is where the floor
    /// belongs — not against the fill, which only ever touches the thumb's
    /// interior.
    ///
    /// **The floor is 3:1, not the 4.5:1 [`crate::switch`] uses**, because a
    /// 10-to-14-pixel disc is a graphical object rather than a glyph, and the
    /// criterion for a graphical object that identifies a control is WCAG SC
    /// 1.4.11 *Non-text Contrast*, which asks 3:1. The stricter number is kept
    /// where it is reachable and not invented where it is not; measured
    /// 2026-08-24 the full table is
    ///
    /// | card | dark | light |
    /// |---|---|---|
    /// | `base` | 11.34 | 7.06 |
    /// | `mantle` | 12.14 | 6.57 |
    /// | `crust` | 12.97 | 6.04 |
    /// | `surface0` | 8.69 | 5.17 |
    /// | `surface1` | 6.31 | **4.39** |
    /// | `surface2` | 4.62 | **3.69** |
    ///
    /// so ten of the twelve clear 4.5:1 outright and the two that do not are
    /// the pale theme's two lightest surfaces. Those two are a palette fact,
    /// not a slider fact — *ordinary text* on a light `surface2` card is 3.69:1
    /// too — and they are logged as such in `known-issues.md` rather than
    /// worked around here.
    #[test]
    fn the_thumb_is_legible_against_every_card_it_can_sit_on() {
        let mut worst: Option<(String, f64)> = None;
        for light in [false, true] {
            let p = Palette::for_mode(light);
            let mode = if light { "light" } else { "dark" };
            for (name, card) in [
                ("base", p.base),
                ("mantle", p.mantle),
                ("crust", p.crust),
                ("surface0", p.surface0),
                ("surface1", p.surface1),
                ("surface2", p.surface2),
            ] {
                let c = contrast(card, p.text);
                if worst.as_ref().is_none_or(|(_, w)| c < *w) {
                    worst = Some((format!("`{name}` in {mode} mode"), c));
                }
            }
            // And the module draws that colour, whatever the fill is — which
            // is the defect: the touchpad thumb was `p.accent` on a `p.accent`
            // fill, so it vanished for the whole left half of its travel.
            for accent in AccentColor::presets() {
                let fill = p.hue(*accent);
                let mut cmds = Vec::new();
                Slider {
                    x: 0.0,
                    y: 0.0,
                    width: 100.0,
                    height: 6.0,
                    frac: 0.5,
                    thumb: 12.0,
                    track: p.surface1,
                    fill,
                    p: &p,
                    alpha: u8::MAX,
                }
                .draw(&mut cmds);
                let (.., ink, _) = rect(cmds.last().expect("a thumb"));
                assert_eq!(
                    ink,
                    p.text,
                    "the thumb over a `{}` fill in {mode} mode is not `text`",
                    accent.label()
                );
                assert_ne!(
                    ink,
                    fill,
                    "the thumb over a `{}` fill in {mode} mode is that fill",
                    accent.label()
                );
            }
        }
        let (where_, c) = worst.expect("there are cards");
        assert!(c >= 3.0, "the tightest thumb is on {where_}, at {c:.2}:1");
    }

    /// The reasoning in the module docs, stated as a number so nobody
    /// "corrects" the inconsistency with [`crate::switch`] by hand.
    ///
    /// [`readable_on`] of the fill is the right ink for a knob a track
    /// contains, and the wrong one here: on the stock themes it is one of the
    /// two extremes, and an extreme is by construction close to the card the
    /// slider sits on. This asserts that `text` beats it on the background that
    /// actually defines the thumb's outline.
    #[test]
    fn text_beats_readable_on_the_fill_against_the_card() {
        for light in [false, true] {
            let p = Palette::for_mode(light);
            for accent in AccentColor::presets() {
                let fill = p.hue(*accent);
                let derived = readable_on(fill);
                let (mine, theirs) = (contrast(p.base, p.text), contrast(p.base, derived));
                assert!(
                    mine > theirs,
                    "on a `{}` fill in {} mode, `text` is worth {mine:.2}:1 \
                     against the card and `readable_on(fill)` {theirs:.2}:1",
                    accent.label(),
                    if light { "light" } else { "dark" }
                );
            }
        }
    }

    /// An empty fill draws nothing; a non-empty one draws a rectangle.
    #[test]
    fn a_slider_on_its_floor_emits_no_fill_rectangle() {
        let p = Palette::for_mode(false);
        assert_eq!(draw(&p, 6.0, 12.0, 0.0).len(), 2, "track and thumb only");
        assert_eq!(draw(&p, 6.0, 12.0, 0.001).len(), 3, "the fill is drawn");
    }

    /// A caller that divides by a user-supplied maximum can hand over a
    /// fraction outside `0..=1`; the thumb still stops at the ends.
    #[test]
    fn an_out_of_range_fraction_cannot_push_the_thumb_off_the_track() {
        let p = Palette::for_mode(false);
        for (frac, expected) in [(-3.0_f32, 100.0_f32), (7.0, 250.0), (f32::NAN, 100.0)] {
            let cmds = draw(&p, 6.0, 12.0, frac);
            let (kx, ..) = rect(cmds.last().expect("a thumb"));
            assert!(
                (kx + 6.0 - expected).abs() < f32::EPSILON,
                "frac {frac} put the thumb's centre at {}",
                kx + 6.0
            );
        }
    }

    /// The overlay fades as one thing: all three colours take the alpha, and
    /// none of them takes it twice.
    #[test]
    fn the_alpha_reaches_every_part_of_the_control() {
        let p = Palette::for_mode(false);
        let mut faded = Vec::new();
        Slider {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 6.0,
            frac: 0.5,
            thumb: 10.0,
            track: p.surface0,
            fill: p.accent,
            p: &p,
            alpha: 128,
        }
        .draw(&mut faded);
        for (c, role) in faded.iter().zip([p.surface0, p.accent, p.text]) {
            let (.., color, _) = rect(c);
            assert_eq!(
                color,
                Color::rgba(role.r, role.g, role.b, 128),
                "a faded slider part kept the wrong colour"
            );
        }
        // Opaque is the role itself, not a re-wrapped copy of it, so a palette
        // role with its own alpha survives untouched.
        let opaque = draw(&p, 6.0, 12.0, 0.5);
        let (.., c, _) = rect(&opaque[0]);
        assert_eq!(c, p.surface1);
    }
}
