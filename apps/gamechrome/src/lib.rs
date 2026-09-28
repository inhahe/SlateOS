//! The chrome every game draws round its board, in the user's theme.
//!
//! The operator's answer to C-Q16 (`design-decisions.md` §1422): every game's
//! menus, score panels, dialogs and window background follow the desktop
//! theme, as the applications' do; on the playing surface a colour keeps its
//! own value only where a player *reads* it -- tells two things apart by it,
//! or knows a convention by it -- and follows the theme where it only fills
//! space. Thirty-odd games each carried a copy of Catppuccin Mocha as
//! constants, stayed dark on a light desktop, and drew their own flat
//! buttons, each a little different.
//!
//! This is the one place their chrome comes from, so they follow the theme
//! together and look alike:
//!
//! - [`Chrome`], the roles a game's chrome is drawn in, from the palette;
//! - [`button`], the toolkit's push button in the reference's look
//!   (`guitk::button`), with its label at the size the game lays out -- the
//!   toolkit's draws a fixed 13-pixel label, and a game scales with its
//!   window;
//! - [`apart_from_accent`], the second of two sides' colours, for a game whose
//!   pieces follow the theme: a palette hue that cannot be mistaken for the
//!   accent, the first side's.
//!
//! A game's own colours -- the seven tetrominoes, the four ghosts, a card's
//! red suits -- stay in the game, named, and its palette test lists them as
//! the ones not drawn from the palette.

use guitk::button::{self as tk_button, Kind, State};
use guitk::color::Color;
use guitk::palette::{Palette, hard_to_tell_apart};
use guitk::render::{FontWeightHint, RenderCommand, TextOverflow};
use guitk::style::CornerRadii;
use guitk::surface::CommandSink;
use guitk::text;
use guitk::theme::{contrast_ratio, with_alpha};

/// The roles a game's chrome is drawn in, from the user's palette.
///
/// Named for what they are in a game rather than for the palette entries
/// behind them, so a game reads as its own and the mapping is made once.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Chrome {
    /// The window behind everything.
    pub page: Color,
    /// A header or a footer band: a step off the page.
    pub band: Color,
    /// A board's well, and a score panel: a step darker than the page in a
    /// dark theme, lighter in a light one.
    pub well: Color,
    /// A square or a tile raised off the well: a cell being pointed at.
    pub raised: Color,
    /// Raised further: a cell that matters -- the winning line.
    pub lit: Color,
    /// Raised furthest: a third state of a tile (a flagged square).
    pub high: Color,
    /// Text.
    pub text: Color,
    /// Secondary text: captions, hints.
    pub dim: Color,
    /// Text for something that cannot be used now: the palette's disabled
    /// grey, below the text floor on purpose (WCAG exempts a disabled control).
    pub off: Color,
    /// Text saying the player won.
    pub good: Color,
    /// Text saying the player lost.
    pub bad: Color,
    /// Text saying neither: a draw, a pause, a title on a help sheet.
    pub even: Color,
    /// A game's title.
    pub title: Color,
    /// The keyboard's focus, and the cell it is on: the accent, as every focus
    /// ring in the desktop is.
    pub ring: Color,
    /// A key's name on a help sheet.
    pub key: Color,
    /// Laid over the whole window behind a help sheet or a dialog.
    pub scrim: Color,
    /// A banner or a sheet over the board: the well, nearly opaque.
    pub veil: Color,
}

impl Chrome {
    /// The roles for palette `p`. Text roles are inked for the page, so they
    /// clear the contrast floor in a light theme as in a dark one.
    #[must_use]
    pub fn of(p: &Palette) -> Self {
        Self {
            page: p.base,
            band: p.mantle,
            well: p.crust,
            raised: p.surface0,
            lit: p.surface1,
            high: p.surface2,
            text: p.text,
            dim: p.subtext0,
            off: p.overlay0,
            good: p.ink(p.green),
            bad: p.ink(p.red),
            even: p.ink(p.yellow),
            title: p.ink(p.lavender),
            ring: p.accent,
            key: p.ink(p.blue),
            scrim: with_alpha(p.base, 158),
            veil: with_alpha(p.crust, 214),
        }
    }
}

/// The hues a second side may take, in the order they are tried: those that
/// stand furthest from the usual accents first.
const SECOND_SIDES: [fn(&Palette) -> Color; 6] = [
    |p| p.red,
    |p| p.peach,
    |p| p.green,
    |p| p.mauve,
    |p| p.yellow,
    |p| p.teal,
];

/// A colour for the second of two sides, when the first is drawn in the
/// accent: the first palette hue in [`SECOND_SIDES`] that
/// [`hard_to_tell_apart`] does not call too close to the accent, inked for the
/// page. Whatever accent the user picks, the two sides stay apart -- a red
/// accent gets a peach second side rather than a second red.
#[must_use]
pub fn apart_from_accent(p: &Palette) -> Color {
    let accent = p.ink(p.accent);
    SECOND_SIDES
        .iter()
        .map(|hue| p.ink(hue(p)))
        .find(|&candidate| !hard_to_tell_apart(candidate, accent))
        // Every hue too close to this accent cannot happen with a palette's
        // six spread hues, but a theme may set them all alike; its text colour
        // is then the one thing the accent is not.
        .unwrap_or(p.text)
}

/// Of a colour's two shades -- the dark theme's and the light theme's -- the
/// one that reads better on `ground`.
///
/// For a colour a player reads, drawn as text on a tile whose own colour
/// follows the theme (`design-decisions.md` §1225): a minesweeper 7 stays a
/// yellow, but the pale yellow of a dark theme vanishes on a light tile, so
/// on a light tile it is the light theme's deeper yellow.
#[must_use]
pub fn legible_on((dark, light): (Color, Color), ground: Color) -> Color {
    if contrast_ratio(dark, ground) >= contrast_ratio(light, ground) {
        dark
    } else {
        light
    }
}

/// Every colour [`button`] can draw a button of `kind` on `ground` in: at
/// rest, pointed at, held down and switched off, face, gloss, edge and label.
///
/// They are the toolkit's own blends of palette colours, not palette entries,
/// so a game's palette test (`appearance::palette_check::assert_drawn_from`)
/// names them as derived: this is the list to name.
#[must_use]
pub fn button_colours(p: &Palette, kind: Kind, ground: Color) -> Vec<Color> {
    [
        State::default(),
        State {
            hovered: true,
            ..State::default()
        },
        State {
            pressed: true,
            ..State::default()
        },
        State {
            disabled: true,
            ..State::default()
        },
    ]
    .into_iter()
    .flat_map(|state| {
        let paint = tk_button::paint(p, kind, state, ground);
        [paint.upper, paint.lower, paint.edge, paint.ink]
    })
    .collect()
}

/// A push button in the reference's look -- the toolkit's colours for its
/// kind and state, on `ground` -- with its label at `font_size` in bold,
/// centred and cut with an ellipsis if the button is too narrow.
///
/// The toolkit's own `guitk::button::draw` draws the label at a fixed 13
/// pixels, which a game that scales with its window cannot use; the colours
/// are the toolkit's all the same, so a game's buttons look like every other
/// button in the desktop.
#[allow(clippy::too_many_arguments)]
pub fn button(
    sink: &mut impl CommandSink,
    p: &Palette,
    (x, y, w, h): (f32, f32, f32, f32),
    label: &str,
    font_size: f32,
    kind: Kind,
    state: State,
    ground: Color,
) {
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    let paint = tk_button::paint(p, kind, state, ground);
    let radius = tk_button::RADIUS.min(h / 2.0);
    sink.emit(RenderCommand::FillRect {
        x,
        y,
        width: w,
        height: h,
        color: paint.lower,
        corner_radii: CornerRadii::all(radius),
    });
    sink.emit(RenderCommand::FillRect {
        x,
        y,
        width: w,
        height: h / 2.0,
        color: paint.upper,
        corner_radii: CornerRadii {
            top_left: radius,
            top_right: radius,
            bottom_left: 0.0,
            bottom_right: 0.0,
        },
    });
    sink.emit(RenderCommand::StrokeRect {
        x,
        y,
        width: w,
        height: h,
        color: paint.edge,
        line_width: 1.0,
        corner_radii: CornerRadii::all(radius),
    });
    if state.focused && !state.disabled {
        sink.emit(RenderCommand::StrokeRect {
            x: x - 2.0,
            y: y - 2.0,
            width: w + 4.0,
            height: h + 4.0,
            color: p.accent,
            line_width: 2.0,
            corner_radii: CornerRadii::all(radius + 2.0),
        });
    }
    let pad = (h * 0.3).min(tk_button::PADDING_H);
    let room = (w - pad * 2.0).max(0.0);
    let text_w = text::measure(label, font_size, FontWeightHint::Bold).min(room);
    let line = text::line_height(font_size, FontWeightHint::Bold);
    sink.emit(RenderCommand::Text {
        x: x + (w - text_w) / 2.0,
        y: y + (h - line) / 2.0,
        text: label.to_owned(),
        color: paint.ink,
        font_size,
        font_weight: FontWeightHint::Bold,
        max_width: Some(room),
        overflow: TextOverflow::Ellipsis,
    });
}

#[cfg(test)]
mod tests;
