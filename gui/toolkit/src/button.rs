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
//! # What the theme's widget style decides
//!
//! The shape (`Palette::widget_style`, design-decisions 1435): how round the
//! corners are, how much room the label has either side, whether the upper
//! half is the brighter glass or the face is one flat colour, and whether a
//! soft shadow lifts the button off the page. The built-in theme's are the
//! reference's -- 4-pixel corners, 14 pixels either side, the gloss, no
//! shadow. The colours and the label's legibility are the same whichever
//! shape: a face without gloss is only its lower half, on which the ink was
//! already checked.
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
use crate::text::scaled;
use crate::theme::{contrast_ratio, relative_luminance};
use crate::widget_style::ButtonStyle;

/// A button's height where the caller has no layout of its own to fit: the
/// reference's 28, at the default text size -- [`height`] is at the user's.
pub const HEIGHT: f32 = 28.0;
/// The label's size at the default text size -- [`font_size`] is at the
/// user's.
pub const FONT_SIZE: f32 = 13.0;
/// Room either side of the label in the built-in theme: the reference's
/// `padding: 0 14px`. A theme's own is its widget style's
/// (`ButtonStyle::padding`), which [`width`] and [`draw`] read.
pub const PADDING_H: f32 = 14.0;
/// The narrowest a button is, so a row of short labels -- OK, Cancel -- is a
/// row of equal buttons, as every desktop draws them. At the default text
/// size: [`width`] keeps it at the user's.
pub const MIN_WIDTH: f32 = 80.0;
/// The corners in the built-in theme: the reference's 4. A theme's own are
/// its widget style's (`ButtonStyle::radius`), which [`draw`] reads.
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
/// `#fbfdff` over `#e6eef6`. Nothing, in a style without gloss.
const GLOSS: f32 = 0.18;
/// The shadow a style with one puts under a button: a pixel down and three
/// of blur, faint -- a button lifted off the page, not floating over it as a
/// menu does.
const SHADOW_OFFSET_Y: f32 = 1.0;
const SHADOW_BLUR: f32 = 3.0;
const SHADOW_COLOR: Color = Color::rgba(0, 0, 0, 80);
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

/// The width a button needs for `label` in `style`: the label in bold at
/// [`font_size`], with the style's padding either side, and never narrower
/// than [`MIN_WIDTH`] -- all at the user's text size.
///
/// A row that lays its buttons out from this must draw them with the same
/// style, and test a click against the rectangles it drew -- the padding moves
/// every button after the first.
#[must_use]
pub fn width(style: &ButtonStyle, label: &str) -> f32 {
    (crate::text::measure(label, font_size(), FontWeightHint::Bold) + padding(style) * 2.0)
        .max(scaled(MIN_WIDTH))
}

/// The room either side of a label in `style`, held to the style's bounds,
/// at the user's text size: a larger label is a larger button, not a label
/// pressed against its edges.
fn padding(style: &ButtonStyle) -> f32 {
    scaled(f32::from(
        style
            .padding
            .clamp(ButtonStyle::MIN_PADDING, ButtonStyle::MAX_PADDING),
    ))
}

/// A button's height at the user's text size ([`HEIGHT`] at the default):
/// what a row of buttons with no layout of its own to fit lays out from,
/// so a larger label has the room it needs.
#[must_use]
pub fn height() -> f32 {
    scaled(HEIGHT)
}

/// The label's size at the user's text size ([`FONT_SIZE`] at the
/// default): what [`width`] measures and [`draw`] draws.
#[must_use]
pub fn font_size() -> f32 {
    scaled(FONT_SIZE)
}

/// The colours a button of `kind` in `state` is drawn in, on `ground` -- the
/// colour of whatever the button sits on.
///
/// Every colour is opaque, so the label's contrast against the face is known:
/// the ink clears the text floor on both halves of the face. Without the
/// style's gloss the two halves are one colour.
#[must_use]
pub fn paint(palette: &Palette, kind: Kind, state: State, ground: Color) -> Paint {
    let gloss = if palette.widget_style.button.gloss {
        GLOSS
    } else {
        0.0
    };
    if state.disabled {
        // The palette's disabled grey, which is what it is for: off should
        // look off. Not held to the text floor, by the palette's own rule for
        // `overlay0`.
        let lower = opaque(palette.surface0, ground);
        return Paint {
            upper: lower.lerp(Color::WHITE, gloss / 2.0),
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
        let upper = lower.lerp(Color::WHITE, gloss);
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

/// The corner radius a button `h` tall is drawn with in `style`: the style's,
/// but never more than half the height -- a pill is the roundest a button
/// can be, whatever a theme asks.
#[must_use]
pub fn radius(style: &ButtonStyle, h: f32) -> f32 {
    f32::from(style.radius).min((h / 2.0).max(0.0))
}

/// Draw a button with its label, at `(x, y)`, `w` by `h`, on `ground`, in the
/// palette's colours and its widget style's shape.
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
    let style = &palette.widget_style.button;
    let colours = paint(palette, kind, state, ground);
    let r = radius(style, h);
    let radii = CornerRadii::all(r);
    // A shadow lifts a button that can be pressed; one held down, or one that
    // cannot be pressed at all, sits on the page.
    if style.shadow && !state.disabled && !state.pressed {
        sink.emit(RenderCommand::BoxShadow {
            x,
            y,
            width: w,
            height: h,
            offset_x: 0.0,
            offset_y: SHADOW_OFFSET_Y,
            blur: SHADOW_BLUR,
            spread: 0.0,
            color: SHADOW_COLOR,
            corner_radii: radii,
        });
    }
    // The face, then its brighter upper half over it: the reference's
    // gradient, in the two steps the renderer can draw. A face without gloss
    // is one colour, and one fill.
    sink.emit(RenderCommand::FillRect {
        x,
        y,
        width: w,
        height: h,
        color: colours.lower,
        corner_radii: radii,
    });
    if colours.upper != colours.lower {
        sink.emit(RenderCommand::FillRect {
            x,
            y,
            width: w,
            height: h / 2.0,
            color: colours.upper,
            // The upper half's lower corners are square: they sit on the
            // lower half, not on the page. Its upper corners are the
            // button's, which `radius` already holds to half the height.
            corner_radii: CornerRadii {
                top_left: r,
                top_right: r,
                bottom_left: 0.0,
                bottom_right: 0.0,
            },
        });
    }
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
            corner_radii: CornerRadii::all(r + focus_ring),
        });
    }
    let font_size = font_size();
    let text_w = crate::text::measure(label, font_size, FontWeightHint::Bold);
    let pad = padding(style);
    let room = (w - pad).max(0.0);
    sink.emit(RenderCommand::Text {
        x: x + ((w - text_w) / 2.0).max(pad / 2.0),
        y: y + (h - font_size) / 2.0,
        text: label.to_string(),
        color: colours.ink,
        font_size,
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
    use crate::widget_style::WidgetStyle;

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

    /// **A button follows the user's text size** (`crate::text::set_base_size`,
    /// on this test's thread): at twice the size its label, the height it
    /// offers and its least width are twice as large, and a label's width
    /// grows with the label -- a larger label is a larger button, not one
    /// spilling out of the old.
    #[test]
    fn a_button_follows_the_text_size() {
        let style = WidgetStyle::AERO.button;
        let label = "Install updates";
        let wide = width(&style, label);
        assert_eq!((height(), font_size()), (HEIGHT, FONT_SIZE));

        crate::text::set_base_size(crate::text::DEFAULT_SIZE * 2.0);
        assert_eq!((height(), font_size()), (HEIGHT * 2.0, FONT_SIZE * 2.0));
        assert_eq!(width(&style, "OK"), MIN_WIDTH * 2.0, "the least width");
        let ratio = width(&style, label) / wide;
        assert!((1.9..=2.1).contains(&ratio), "{ratio}");

        let p = Palette::for_mode(false);
        let mut drawn: Vec<RenderCommand> = Vec::new();
        draw(
            &mut drawn,
            &p,
            (0.0, 0.0, 400.0, height()),
            label,
            Kind::Plain,
            State::default(),
            p.base,
            2.0,
        );
        let sizes: Vec<f32> = drawn
            .iter()
            .filter_map(|cmd| match cmd {
                RenderCommand::Text { font_size, .. } => Some(*font_size),
                _ => None,
            })
            .collect();
        assert_eq!(sizes, [FONT_SIZE * 2.0]);
    }

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

    /// A palette whose widget style's button is `button`.
    fn styled(button: ButtonStyle) -> Palette {
        let mut p = Palette::for_mode(false);
        p.widget_style.button = button;
        p
    }

    /// The fills and shadows a button draws, in order: `(kind, height,
    /// corner radius)` with `kind` `'s'` for a shadow and `'f'` for a fill.
    fn shapes(p: &Palette, state: State) -> Vec<(char, f32, f32)> {
        let mut cmds: Vec<RenderCommand> = Vec::new();
        draw(
            &mut cmds,
            p,
            (0.0, 0.0, 100.0, HEIGHT),
            "OK",
            Kind::Plain,
            state,
            p.base,
            2.0,
        );
        cmds.iter()
            .filter_map(|cmd| match cmd {
                RenderCommand::BoxShadow {
                    height,
                    corner_radii,
                    ..
                } => Some(('s', *height, corner_radii.top_left)),
                RenderCommand::FillRect {
                    height,
                    corner_radii,
                    ..
                } => Some(('f', *height, corner_radii.top_left)),
                _ => None,
            })
            .collect()
    }

    /// **The built-in theme's button is the reference's**: 4-pixel corners,
    /// the brighter upper half, no shadow -- what it was before styles.
    #[test]
    fn the_built_in_style_draws_the_reference_button() {
        let p = Palette::for_mode(false);
        assert_eq!(
            shapes(&p, State::default()),
            [('f', HEIGHT, RADIUS), ('f', HEIGHT / 2.0, RADIUS)]
        );
    }

    /// **A style's corners are drawn, but never rounder than a pill**, and
    /// the focus ring follows them round.
    #[test]
    fn the_corners_are_the_styles_up_to_a_pill() {
        let p = styled(ButtonStyle {
            radius: 9,
            ..WidgetStyle::AERO.button
        });
        assert_eq!(shapes(&p, State::default())[0], ('f', HEIGHT, 9.0));
        assert_eq!(
            radius(&p.widget_style.button, 10.0),
            5.0,
            "a pill at 10 high"
        );
        assert_eq!(radius(&p.widget_style.button, -3.0), 0.0);

        let mut cmds: Vec<RenderCommand> = Vec::new();
        let focused = State {
            focused: true,
            ..State::default()
        };
        draw(
            &mut cmds,
            &p,
            (0.0, 0.0, 100.0, HEIGHT),
            "OK",
            Kind::Plain,
            focused,
            p.base,
            2.0,
        );
        let ring = cmds.iter().find_map(|cmd| match cmd {
            RenderCommand::StrokeRect {
                color,
                corner_radii,
                ..
            } if *color == p.accent => Some(corner_radii.top_left),
            _ => None,
        });
        assert_eq!(ring, Some(11.0), "the ring is round with the button");
    }

    /// **Without gloss the face is one flat colour**, drawn once -- and its
    /// label still clears the floor, on every kind, state and ground.
    #[test]
    fn a_flat_face_is_one_colour_and_its_label_reads() {
        for light in [false, true] {
            let mut p = Palette::for_mode(light);
            p.widget_style.button.gloss = false;
            for ground in [p.base, p.mantle, p.surface0, p.crust] {
                for kind in KINDS {
                    for state in STATES {
                        let c = paint(&p, kind, state, ground);
                        assert_eq!(c.upper, c.lower, "{kind:?} {state:?}");
                        assert!(contrast_ratio(c.ink, c.lower) >= TEXT_CONTRAST_FLOOR);
                    }
                }
            }
            let off = State {
                disabled: true,
                ..State::default()
            };
            let c = paint(&p, Kind::Plain, off, p.base);
            assert_eq!(c.upper, c.lower, "a disabled flat face");
            assert_eq!(shapes(&p, State::default()), [('f', HEIGHT, RADIUS)]);
        }
    }

    /// **A style with a shadow lifts the button**, under its face and round
    /// its corners -- but not one held down, and not one that cannot be
    /// pressed.
    #[test]
    fn a_shadow_lifts_a_button_that_can_be_pressed() {
        let p = styled(ButtonStyle {
            shadow: true,
            ..WidgetStyle::AERO.button
        });
        let rest = shapes(&p, State::default());
        assert_eq!(
            rest[0],
            ('s', HEIGHT, RADIUS),
            "the shadow first, under the face"
        );
        assert_eq!(rest.iter().filter(|s| s.0 == 's').count(), 1);
        let pressed = State {
            pressed: true,
            hovered: true,
            ..State::default()
        };
        let off = State {
            disabled: true,
            ..State::default()
        };
        for state in [pressed, off] {
            assert!(
                shapes(&p, state).iter().all(|s| s.0 != 's'),
                "{state:?} has a shadow"
            );
        }
        assert!(
            shapes(&Palette::for_mode(false), State::default())
                .iter()
                .all(|s| s.0 != 's'),
            "the built-in theme's button has none"
        );
    }

    /// A row of short labels is a row of equal buttons; a long one is as
    /// wide as its label needs.
    #[test]
    fn short_labels_share_the_least_width() {
        let aero = WidgetStyle::AERO.button;
        assert_eq!(width(&aero, "OK"), MIN_WIDTH);
        assert_eq!(width(&aero, "Cancel"), MIN_WIDTH);
        let long = "Overwrite the existing file";
        assert!(width(&aero, long) > MIN_WIDTH);
        assert!(
            width(&aero, long)
                >= crate::text::measure(long, FONT_SIZE, FontWeightHint::Bold) + PADDING_H * 2.0
        );
    }

    /// **A theme's padding is the room either side of the label** -- in the
    /// width a row lays out, and in where the label is drawn -- held to its
    /// bounds.
    #[test]
    fn the_padding_is_the_themes() {
        let long = "Overwrite the existing file";
        let text = crate::text::measure(long, FONT_SIZE, FontWeightHint::Bold);
        let with = |padding: u8| ButtonStyle {
            padding,
            ..WidgetStyle::AERO.button
        };
        assert!((width(&with(20), long) - (text + 40.0)).abs() < 0.01);
        assert!((width(&with(6), long) - (text + 12.0)).abs() < 0.01);
        assert!(
            (width(&with(0), long) - (text + f32::from(ButtonStyle::MIN_PADDING) * 2.0)).abs()
                < 0.01,
            "no padding at all"
        );
        assert!(
            (width(&with(200), long) - (text + f32::from(ButtonStyle::MAX_PADDING) * 2.0)).abs()
                < 0.01
        );

        // The label's room is the button less its padding.
        let p = styled(with(20));
        let mut cmds: Vec<RenderCommand> = Vec::new();
        draw(
            &mut cmds,
            &p,
            (0.0, 0.0, 100.0, HEIGHT),
            "OK",
            Kind::Plain,
            State::default(),
            p.base,
            2.0,
        );
        let room = cmds.iter().find_map(|c| match c {
            RenderCommand::Text { max_width, .. } => *max_width,
            _ => None,
        });
        assert_eq!(room, Some(80.0));
    }
}
