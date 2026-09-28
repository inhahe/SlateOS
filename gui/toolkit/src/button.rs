//! The toolkit's push button, drawn as the default theme's reference draws
//! one.
//!
//! The reference (`Aero Desktop (offline).html`, the search dialog's
//! `aero-srch-btn`) draws a light face, brighter across its upper half, with a
//! line round it in a tint of its blue and the label in bold; the button that
//! does what the dialog is for (`primary`) is tinted blue. Until this module
//! every button in the tree was drawn where it was used -- the dialogs' as
//! flat blue or grey slabs, the file dialog's as flat fills, the Run box's as
//! its own -- and none of them looked like the reference or like each other.
//!
//! # What is the theme's and what is fixed
//!
//! The reference's colours are its blues on a white ground. Here every colour
//! is derived from the palette, so a button follows the user's theme: the face
//! is the palette's button colour (`surface1`, "a button at rest"), tinted
//! towards the accent for a primary button and towards red for one that
//! destroys something; the brighter upper half is the face lightened; the edge
//! is the face tinted further. Everything is opaque -- blended against the
//! ground the button is drawn on -- so the label's contrast can be *computed*
//! rather than hoped for: the label is the palette's text colour where that
//! reads on both halves of the face, and black or white where it does not --
//! and a face on which no one ink reads is tinted further until one does.
//!
//! # What a caller decides
//!
//! Where the button goes and how tall it is: a dialog lays its own button row
//! out. [`width`] gives the width a label needs, so a row of buttons is laid
//! out from the same measure the drawing uses.

use crate::color::Color;
use crate::palette::{Palette, TEXT_CONTRAST_FLOOR, legible_on};
use crate::render::{FontWeightHint, RenderCommand, TextOverflow};
use crate::style::CornerRadii;
use crate::surface::CommandSink;
use crate::theme::{contrast_ratio, relative_luminance};

/// A button's height where the caller has no layout of its own to fit: the
/// reference's 28.
pub const HEIGHT: f32 = 28.0;
/// The label's size.
pub const FONT_SIZE: f32 = 13.0;
/// Room either side of the label: the reference's `padding: 0 14px`.
pub const PADDING_H: f32 = 14.0;
/// The narrowest a button is, so a row of short labels -- OK, Cancel -- is a
/// row of equal buttons, as every desktop draws them.
pub const MIN_WIDTH: f32 = 80.0;
/// The corners: the reference's 4.
pub const RADIUS: f32 = 4.0;
/// How far a primary or destructive button's face is tinted towards its
/// colour, from the palette's button colour.
const TINT: f32 = 0.35;
/// How much further the pointer over a button tints it: the reference's hover
/// is its face a step bluer.
const HOVER_TINT: f32 = 0.12;
/// How much further a button held down is tinted.
const PRESSED_TINT: f32 = 0.22;
/// How much brighter the upper half of the face is: the reference's
/// `#fbfdff` over `#e6eef6`.
const GLOSS: f32 = 0.18;
/// How far the edge is tinted beyond the face, towards the accent -- or, for a
/// primary or destructive button, towards its own colour, further.
const EDGE_TINT: f32 = 0.45;
const EDGE_TINT_STRONG: f32 = 0.7;
/// A ground brighter than this is a light one, where tints are pale.
const LIGHT_GROUND: f32 = 0.4;
/// How pale: the tint colour this far towards white.
const PALE: f32 = 0.55;
/// How much further a face is tinted, each step, while no one ink reads on
/// both its halves.
const TINT_STEP: f32 = 0.1;

/// What a button does, which decides how it is marked.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Kind {
    /// An ordinary action: Cancel, Browse, Close.
    #[default]
    Plain,
    /// The action the dialog exists for, the one Enter presses: tinted with
    /// the accent.
    Primary,
    /// An action that destroys something the user cannot get back: tinted
    /// with red, so a "Delete" never looks like an "OK".
    Destructive,
}

/// What is happening to a button now.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct State {
    /// The pointer is over it.
    pub hovered: bool,
    /// It is held down.
    pub pressed: bool,
    /// It cannot be pressed now. Drawn in the palette's disabled grey, and
    /// neither hover nor press changes it.
    pub disabled: bool,
    /// The keyboard is on it: a ring round it in the accent.
    pub focused: bool,
}

/// The colours of one button, derived from the palette for its kind and
/// state on its ground.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Paint {
    /// The upper half of the face.
    pub upper: Color,
    /// The lower half of the face.
    pub lower: Color,
    /// The line round the face.
    pub edge: Color,
    /// The label.
    pub ink: Color,
}

/// The width a button needs for `label`: the label in bold at [`FONT_SIZE`],
/// with [`PADDING_H`] either side, and never narrower than [`MIN_WIDTH`].
#[must_use]
pub fn width(label: &str) -> f32 {
    (crate::text::measure(label, FONT_SIZE, FontWeightHint::Bold) + PADDING_H * 2.0).max(MIN_WIDTH)
}

/// The colours a button of `kind` in `state` is drawn in, on `ground` -- the
/// colour of whatever the button sits on.
///
/// Every colour is opaque, so the label's contrast against the face is known:
/// the ink clears the text floor on both halves of the face.
#[must_use]
pub fn paint(palette: &Palette, kind: Kind, state: State, ground: Color) -> Paint {
    if state.disabled {
        // The palette's disabled grey, which is what it is for: off should
        // look off. Not held to the text floor, by the palette's own rule for
        // `overlay0`.
        let lower = opaque(palette.surface0, ground);
        return Paint {
            upper: lower.lerp(Color::WHITE, GLOSS / 2.0),
            lower,
            edge: opaque(palette.surface1, ground),
            ink: palette.overlay0,
        };
    }
    let rest = opaque(palette.surface1, ground);
    let (base_tint, tint_colour) = match kind {
        Kind::Plain => (0.0, palette.accent),
        Kind::Primary => (TINT, palette.accent),
        Kind::Destructive => (TINT, palette.red),
    };
    // On a light ground the tint is the colour's pale form, as the
    // reference's pale blues are: a light-mode accent is deepened until it
    // reads as text, and a face tinted towards *that* goes dark enough to
    // lose its label.
    let target = if relative_luminance(ground) > LIGHT_GROUND {
        tint_colour.lerp(Color::WHITE, PALE)
    } else {
        tint_colour
    };
    let mut tint = base_tint
        + if state.pressed {
            PRESSED_TINT
        } else if state.hovered {
            HOVER_TINT
        } else {
            0.0
        };
    loop {
        let lower = rest.lerp(target, tint);
        let upper = lower.lerp(Color::WHITE, GLOSS);
        // One ink for both halves. A face whose two halves sit either side of
        // the point where dark text stops reading and light text starts has
        // none, and is tinted further until it has -- a theme's colours
        // decide where that point falls, so no fixed tint can be right for
        // every theme.
        let ink = [palette.text, Color::rgb(0, 0, 0), Color::rgb(255, 255, 255)]
            .into_iter()
            .find(|ink| {
                contrast_ratio(*ink, upper) >= TEXT_CONTRAST_FLOOR
                    && contrast_ratio(*ink, lower) >= TEXT_CONTRAST_FLOOR
            });
        if ink.is_some() || tint >= 1.0 {
            let edge = match kind {
                Kind::Plain => lower.lerp(palette.accent, EDGE_TINT),
                Kind::Primary | Kind::Destructive => lower.lerp(tint_colour, EDGE_TINT_STRONG),
            };
            return Paint {
                upper,
                lower,
                edge,
                // A face tinted all the way still has an answer: the ink that
                // reads on its lower half, where the label's baseline sits.
                ink: ink.unwrap_or_else(|| legible_on(palette.text, lower)),
            };
        }
        tint = (tint + TINT_STEP).min(1.0);
    }
}

/// `colour` as it looks drawn over `ground`: its own alpha composited away.
fn opaque(colour: Color, ground: Color) -> Color {
    let alpha = f32::from(colour.a) / 255.0;
    let solid = Color::rgb(colour.r, colour.g, colour.b);
    Color::rgb(ground.r, ground.g, ground.b).lerp(solid, alpha)
}

/// Draw a button with its label, at `(x, y)`, `w` by `h`, on `ground`.
///
/// `focus_ring` is the ring's width when the button has the keyboard -- a
/// caller that scales its lines for accessibility passes the scaled width.
#[allow(clippy::too_many_arguments)]
pub fn draw(
    sink: &mut impl CommandSink,
    palette: &Palette,
    (x, y, w, h): (f32, f32, f32, f32),
    label: &str,
    kind: Kind,
    state: State,
    ground: Color,
    focus_ring: f32,
) {
    let colours = paint(palette, kind, state, ground);
    let radii = CornerRadii::all(RADIUS);
    // The face, then its brighter upper half over it: the reference's
    // gradient, in the two steps the renderer can draw.
    sink.emit(RenderCommand::FillRect {
        x,
        y,
        width: w,
        height: h,
        color: colours.lower,
        corner_radii: radii,
    });
    sink.emit(RenderCommand::FillRect {
        x,
        y,
        width: w,
        height: h / 2.0,
        color: colours.upper,
        corner_radii: CornerRadii {
            top_left: RADIUS,
            top_right: RADIUS,
            bottom_left: 0.0,
            bottom_right: 0.0,
        },
    });
    sink.emit(RenderCommand::StrokeRect {
        x,
        y,
        width: w,
        height: h,
        color: colours.edge,
        line_width: 1.0,
        corner_radii: radii,
    });
    if state.focused && !state.disabled && focus_ring > 0.0 {
        // Outside the button, so a thicker ring is still a ring round it
        // rather than a band across its face.
        sink.emit(RenderCommand::StrokeRect {
            x: x - focus_ring,
            y: y - focus_ring,
            width: w + focus_ring * 2.0,
            height: h + focus_ring * 2.0,
            color: palette.accent,
            line_width: focus_ring,
            corner_radii: CornerRadii::all(RADIUS + focus_ring),
        });
    }
    let text_w = crate::text::measure(label, FONT_SIZE, FontWeightHint::Bold);
    let room = (w - PADDING_H).max(0.0);
    sink.emit(RenderCommand::Text {
        x: x + ((w - text_w) / 2.0).max(PADDING_H / 2.0),
        y: y + (h - FONT_SIZE) / 2.0,
        text: label.to_string(),
        color: colours.ink,
        font_size: FONT_SIZE,
        font_weight: FontWeightHint::Bold,
        max_width: Some(room),
        overflow: TextOverflow::Ellipsis,
    });
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        clippy::float_cmp
    )]

    use super::*;

    const STATES: [State; 4] = [
        State {
            hovered: false,
            pressed: false,
            disabled: false,
            focused: false,
        },
        State {
            hovered: true,
            pressed: false,
            disabled: false,
            focused: false,
        },
        State {
            hovered: true,
            pressed: true,
            disabled: false,
            focused: false,
        },
        State {
            hovered: false,
            pressed: false,
            disabled: false,
            focused: true,
        },
    ];
    const KINDS: [Kind; 3] = [Kind::Plain, Kind::Primary, Kind::Destructive];

    /// **Every label can be read**, on both halves of its face, for every
    /// kind and state, in both modes, on the grounds a button is drawn on.
    #[test]
    fn every_label_clears_the_text_floor_on_both_halves() {
        for light in [false, true] {
            let p = Palette::for_mode(light);
            for ground in [p.base, p.mantle, p.surface0, p.crust] {
                for kind in KINDS {
                    for state in STATES {
                        let c = paint(&p, kind, state, ground);
                        for half in [c.upper, c.lower] {
                            assert!(
                                contrast_ratio(c.ink, half) >= TEXT_CONTRAST_FLOOR,
                                "{kind:?} {state:?} light={light}: {:?} on {half:?} is {:.2}:1",
                                c.ink,
                                contrast_ratio(c.ink, half)
                            );
                        }
                    }
                }
            }
        }
    }

    /// **The three kinds are told apart**: a primary button is tinted
    /// towards the accent and a destructive one towards red, so a "Delete"
    /// never looks like an "OK" -- and the pointer over a button, and a press
    /// on it, change it.
    #[test]
    fn kinds_and_states_are_told_apart() {
        for light in [false, true] {
            let p = Palette::for_mode(light);
            let rest = State::default();
            let plain = paint(&p, Kind::Plain, rest, p.base);
            let primary = paint(&p, Kind::Primary, rest, p.base);
            let destructive = paint(&p, Kind::Destructive, rest, p.base);
            assert_ne!(plain.lower, primary.lower);
            assert_ne!(primary.lower, destructive.lower);
            // Tinted towards their own colours: nearer to them than the plain
            // face is.
            let distance = |a: Color, b: Color| {
                (i32::from(a.r) - i32::from(b.r)).abs()
                    + (i32::from(a.g) - i32::from(b.g)).abs()
                    + (i32::from(a.b) - i32::from(b.b)).abs()
            };
            assert!(distance(primary.lower, p.accent) < distance(plain.lower, p.accent));
            assert!(distance(destructive.lower, p.red) < distance(plain.lower, p.red));

            let hovered = paint(&p, Kind::Plain, STATES[1], p.base);
            let pressed = paint(&p, Kind::Plain, STATES[2], p.base);
            assert_ne!(hovered.lower, plain.lower, "the pointer changes nothing");
            assert_ne!(pressed.lower, hovered.lower, "a press changes nothing");
        }
    }

    /// **On a light ground the label is the theme's own text colour**, in
    /// every kind and state -- the tints there are pale, as the reference's
    /// blues are. Tinted towards the deepened light-mode accent itself, a
    /// pressed primary button went dark enough to need white text, and its
    /// label changed colour between hover and press.
    #[test]
    fn on_a_light_ground_the_label_is_the_themes_text_colour() {
        let p = Palette::for_mode(true);
        for kind in KINDS {
            for state in STATES {
                assert_eq!(
                    paint(&p, kind, state, p.base).ink,
                    p.text,
                    "{kind:?} {state:?}"
                );
            }
        }
    }

    /// **The upper half is the brighter**, as the reference's glass is.
    #[test]
    fn the_upper_half_is_brighter() {
        for light in [false, true] {
            let p = Palette::for_mode(light);
            for kind in KINDS {
                let c = paint(&p, kind, State::default(), p.base);
                assert!(relative_luminance(c.upper) > relative_luminance(c.lower));
            }
        }
    }

    /// **A disabled button is drawn in the disabled grey, and nothing moves
    /// it**: not the pointer, not the kind.
    #[test]
    fn a_disabled_button_is_grey_whatever_else_is_true() {
        let p = Palette::for_mode(false);
        let off = State {
            disabled: true,
            ..State::default()
        };
        let hovered_off = State {
            hovered: true,
            pressed: true,
            ..off
        };
        let plain = paint(&p, Kind::Plain, off, p.base);
        assert_eq!(plain.ink, p.overlay0);
        assert_eq!(paint(&p, Kind::Primary, hovered_off, p.base), plain);
    }

    /// **The drawing is the paint**: the face in its two halves, the edge,
    /// the label centred and bounded, and a focus ring only when focused.
    #[test]
    fn the_drawing_is_the_paint() {
        let p = Palette::for_mode(false);
        let rect = (10.0, 20.0, 100.0, 28.0);
        let mut cmds: Vec<RenderCommand> = Vec::new();
        draw(
            &mut cmds,
            &p,
            rect,
            "OK",
            Kind::Primary,
            State::default(),
            p.base,
            2.0,
        );
        let c = paint(&p, Kind::Primary, State::default(), p.base);
        let fills: Vec<(f32, Color)> = cmds
            .iter()
            .filter_map(|cmd| match cmd {
                RenderCommand::FillRect { height, color, .. } => Some((*height, *color)),
                _ => None,
            })
            .collect();
        assert_eq!(fills, [(28.0, c.lower), (14.0, c.upper)]);
        let strokes = cmds
            .iter()
            .filter(|cmd| matches!(cmd, RenderCommand::StrokeRect { .. }))
            .count();
        assert_eq!(
            strokes, 1,
            "a ring round a button that does not have the keyboard"
        );
        let label = cmds.iter().find_map(|cmd| match cmd {
            RenderCommand::Text {
                x,
                text,
                color,
                max_width,
                ..
            } => Some((*x, text.clone(), *color, *max_width)),
            _ => None,
        });
        let (label_x, text, ink, bound) = label.expect("no label");
        assert_eq!((text.as_str(), ink), ("OK", c.ink));
        assert!(bound.is_some());
        // Centred on its measured width, in the button's middle.
        let measured = crate::text::measure("OK", FONT_SIZE, FontWeightHint::Bold);
        assert!(
            (label_x + measured / 2.0 - (rect.0 + rect.2 / 2.0)).abs() < 0.5,
            "the label at {label_x} is not centred in its button"
        );

        let mut focused = Vec::new();
        let state = State {
            focused: true,
            ..State::default()
        };
        draw(
            &mut focused,
            &p,
            rect,
            "OK",
            Kind::Primary,
            state,
            p.base,
            2.0,
        );
        let rings = focused
            .iter()
            .filter(
                |cmd| matches!(cmd, RenderCommand::StrokeRect { color, .. } if *color == p.accent),
            )
            .count();
        assert_eq!(rings, 1, "the keyboard's button has no ring");
    }

    /// A row of short labels is a row of equal buttons; a long one is as
    /// wide as its label needs.
    #[test]
    fn short_labels_share_the_least_width() {
        assert_eq!(width("OK"), MIN_WIDTH);
        assert_eq!(width("Cancel"), MIN_WIDTH);
        let long = "Overwrite the existing file";
        assert!(width(long) > MIN_WIDTH);
        assert!(
            width(long)
                >= crate::text::measure(long, FONT_SIZE, FontWeightHint::Bold) + PADDING_H * 2.0
        );
    }
}
