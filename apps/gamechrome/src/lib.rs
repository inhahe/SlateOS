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
//!   accent, the first side's;
//! - [`legibility`], for a game's tests: every text a frame draws, read
//!   against what is drawn under it and held to WCAG's floor for its size;
//! - [`history`], the keys a game's undo history answers (C-Q24, §1416):
//!   Ctrl+Z, Ctrl+Y or Ctrl+Shift+Z, and Alt+Z / Alt+Shift+Z, read the same
//!   way in every game.
//! - [`help`], the keys that raise and put away a game's list of keys --
//!   F1, `?` -- which every game draws with the toolkit's card.
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
use guitk::theme::{contrast_ratio, relative_luminance, with_alpha};

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

    /// These roles for text written on `ground` rather than on the page:
    /// every text role moved only as far as it must be to read there as
    /// ordinary text (4.5:1), keeping its hue; the rest as they are.
    ///
    /// The palette's inks are made for the page, and a panel a game raises
    /// off it -- a score box, a side panel, a sheet -- is darker than the
    /// page in a light theme: the secondary grey fell to 4.1:1 on one, in
    /// game after game. `on(page)` is these roles unchanged. The disabled
    /// grey is left alone: it is below the floor on purpose.
    #[must_use]
    pub fn on(self, ground: Color) -> Self {
        let read = |ink: Color| Ink::on(ink, &[ground]).small;
        Self {
            text: read(self.text),
            dim: read(self.dim),
            good: read(self.good),
            bad: read(self.bad),
            even: read(self.even),
            title: read(self.title),
            key: read(self.key),
            ..self
        }
    }

    /// Every text role, in one list: what a palette test declares as
    /// derived when a window writes on a ground of its own through
    /// [`on`](Self::on).
    #[must_use]
    pub fn inks(&self) -> [Color; 7] {
        [
            self.text, self.dim, self.good, self.bad, self.even, self.title, self.key,
        ]
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

/// A text colour for text drawn on grounds of a game's own, in the two
/// strengths WCAG 1.4.3 asks for: [`large`](Self::large), moved only as far
/// as large text needs to read on every ground (3:1), and
/// [`small`](Self::small), as far as ordinary text needs (4.5:1). Whether a
/// game's text is large depends on the size it is drawn at, which follows the
/// window, so it holds both and picks with [`Ink::at`].
///
/// The palette's inks are made for the page (`Palette::ink`); a game writes
/// on squares, cards and tints the palette never saw, darker than the page in
/// a light theme and lighter in a dark one -- sudoku's hints fell to 2.5:1 on
/// its selected square. Moving an ink only as far as it must leaves a theme
/// whose inks already read exactly as it is, and keeps a hue as near to the
/// palette's as legibility allows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ink {
    /// For large text: at least 3:1 on every ground.
    pub large: Color,
    /// For ordinary text: at least 4.5:1 on every ground.
    pub small: Color,
}

impl Ink {
    /// `ink` made to read on every one of `grounds`, in both strengths.
    #[must_use]
    pub fn on(ink: Color, grounds: &[Color]) -> Self {
        let to = |floor: f32| {
            grounds
                .iter()
                .fold(ink, |ink, &ground| moved_to_read(ink, ground, floor))
        };
        Self {
            large: to(legibility::LARGE_TEXT_FLOOR),
            small: to(legibility::TEXT_FLOOR),
        }
    }

    /// The strength for text drawn at `size` pixels, bold or not.
    #[must_use]
    pub fn at(self, size: f32, bold: bool) -> Color {
        if legibility::is_large(size, bold) {
            self.large
        } else {
            self.small
        }
    }
}

/// `ink`, moved toward black or white -- whichever reads on `ground` -- only
/// as far as it must be to reach `floor` against it; unchanged where it
/// already does.
///
/// `guitk::palette::legible_on` with the floor as a parameter: that one is
/// fixed at 4.5:1, and moving large text there moves its hue twice as far as
/// large text needs. Scaling toward an extreme keeps the hue, for the reason
/// that function's documentation gives; the bisection is its too.
fn moved_to_read(ink: Color, ground: Color, floor: f32) -> Color {
    if contrast_ratio(ink, ground) >= floor {
        return ink;
    }
    let (black, white) = (Color::rgb(0, 0, 0), Color::rgb(255, 255, 255));
    let toward = if contrast_ratio(ground, black) >= contrast_ratio(ground, white) {
        black
    } else {
        white
    };
    let (mut lo, mut hi) = (0.0_f32, 1.0_f32);
    for _ in 0..24 {
        let mid = f32::midpoint(lo, hi);
        if contrast_ratio(ink.lerp(toward, mid), ground) >= floor {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    ink.lerp(toward, hi)
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

/// A board's two squares, `(light, dark)`, from the palette.
///
/// A board's squares only fill space, so they follow the theme (§1422) --
/// a dark theme's board is dark -- but a player still tells them apart, and
/// checkers is played on one of them alone: two shades that stay clearly
/// apart in either theme. `light` is the lighter of the two whatever the
/// theme, so a game that plays on the dark squares finds them by name.
#[must_use]
pub fn squares(p: &Palette) -> (Color, Color) {
    let (a, b) = (p.surface0, p.overlay0);
    if relative_luminance(a) >= relative_luminance(b) {
        (a, b)
    } else {
        (b, a)
    }
}

/// The two outlines [`edge_on`] chooses between when a piece does not stand
/// off its square by itself, `(light, dark)`.
pub const RIMS: (Color, Color) = (Color::from_hex(0xBBBBBB), Color::from_hex(0x333333));

/// The outline for a piece coloured `piece`, whose own outline is `own`, on
/// `ground`.
///
/// A piece keeps its colours in every theme (§1422) and the board under it
/// follows the theme, so a black disc lands on a dark square in a dark theme
/// and a white one on a light square in a light one: a disc the shade of its
/// square, seen by nothing but its shadow. Where the piece stands off the
/// ground by itself -- 3:1, the contrast a shape needs to be seen -- it keeps
/// its own outline; where it does not, the outline is whichever of [`RIMS`]
/// stands off the ground, and the ring is what shows the piece.
#[must_use]
pub fn edge_on(own: Color, piece: Color, ground: Color) -> Color {
    if contrast_ratio(piece, ground) >= 3.0 {
        own
    } else {
        legible_on(RIMS, ground)
    }
}

/// Playing cards, alike in every card game and every theme.
///
/// A card's face is white with red and black suits in any theme -- that is
/// what a card is, and lane C's call for the card games under §1422 -- while
/// the table and the backs follow the theme, the backs in the accent. The
/// four card games each drew their own: two with pale-pink hearts on a
/// lavender face (1.6:1), two on a green felt that stayed green in any theme.
pub mod cards {
    use super::{Palette, apart_from_accent, edge_on};
    use guitk::color::Color;
    use guitk::theme::with_alpha;

    /// A card's face: white.
    pub const FACE: Color = Color::from_hex(0xFAFAF7);
    /// The red suits' ink: hearts and diamonds.
    pub const RED: Color = Color::from_hex(0xD20F39);
    /// The black suits' ink: spades and clubs.
    ///
    /// A neutral near-black, not Mocha's base (`1E1E2E`) that it was: a
    /// game's palette test names this as its own colour and matches it on
    /// RGB, so Mocha's base here would pass a leftover Mocha page in a light
    /// window.
    pub const BLACK: Color = Color::from_hex(0x161616);

    /// A suit's ink on a card's face.
    #[must_use]
    pub const fn suit_ink(red: bool) -> Color {
        if red { RED } else { BLACK }
    }

    /// The colours a card table is drawn in, from the palette.
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct Table {
        /// The felt: the page. A card table is the window a card game is
        /// played in, and the text written on it -- scores, counts, names --
        /// is inked for the page.
        pub felt: Color,
        /// A card's back: the accent.
        pub back: Color,
        /// The lines crossing a back: the page's colour, faint.
        pub pattern: Color,
        /// The keyboard's ring round a card: a hue apart from the accent
        /// ([`apart_from_accent`]), because a back *is* the accent and a ring
        /// the colour of the card it rings is no ring at all -- and not a grey,
        /// which a light theme's rim round every card already is.
        pub focus: Color,
        /// The ring round the cards picked up: the accent. Only a face-up
        /// card is ever picked up, so it never rings a back.
        pub picked: Color,
        /// Where a pile goes when it has no cards, and that place's outline.
        pub empty: Color,
        pub empty_edge: Color,
    }

    impl Table {
        /// The table for palette `p`.
        #[must_use]
        pub fn of(p: &Palette) -> Self {
            Self {
                felt: p.base,
                back: p.accent,
                pattern: with_alpha(p.base, 110),
                focus: apart_from_accent(p),
                picked: p.accent,
                empty: p.surface0,
                empty_edge: p.overlay0,
            }
        }

        /// A face's outline on this felt: the face's own colour where it
        /// stands off the felt, and a rim where it does not -- a light
        /// theme's felt is nearly white itself.
        #[must_use]
        pub fn face_edge(&self) -> Color {
            edge_on(FACE, FACE, self.felt)
        }

        /// A back's outline on this felt, the same way: an accent close to
        /// the felt's shade gets a rim.
        #[must_use]
        pub fn back_edge(&self) -> Color {
            edge_on(self.back, self.back, self.felt)
        }
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

/// The width [`button`] needs to show `label` whole at `font_size` in a
/// button `h` high: the label, and the room the button keeps either side of
/// it. A game that lays its buttons out by their labels asks this rather than
/// adding a padding of its own, which is a second copy of the button's and
/// cuts the label the day the two disagree.
#[must_use]
pub fn button_width(label: &str, font_size: f32, h: f32) -> f32 {
    text::measure(label, font_size, FontWeightHint::Bold) + label_pad(h) * 2.0
}

/// The room [`button`] keeps either side of its label, in a button `h` high:
/// the toolkit's padding, or less in a button too short to afford it.
fn label_pad(h: f32) -> f32 {
    (h * 0.3).min(tk_button::PADDING_H)
}

/// A push button in the reference's look -- the toolkit's colours for its
/// kind and state, on `ground` -- with its label at `font_size` in bold,
/// centred and cut with an ellipsis if the button is too narrow. A label
/// taller than the button, or with no room across it, is left out rather
/// than drawn over the button's edges: the face is still the control.
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
    let room = (w - label_pad(h) * 2.0).max(0.0);
    let line = text::line_height(font_size, FontWeightHint::Bold);
    if line > h || room <= 0.0 {
        return;
    }
    let text_w = text::measure(label, font_size, FontWeightHint::Bold).min(room);
    let text_x = x + (w - text_w) / 2.0;
    sink.emit(RenderCommand::Text {
        x: text_x,
        y: y + (h - line) / 2.0,
        text: label.to_owned(),
        color: paint.ink,
        font_size,
        font_weight: FontWeightHint::Bold,
        // The room from where the label starts to the padding's edge -- not
        // the whole room, which measured from a centred start reaches past
        // the button's right edge by half the slack.
        max_width: Some(x + w - label_pad(h) - text_x),
        overflow: TextOverflow::Ellipsis,
    });
}

pub mod help;
pub mod history;
pub mod legibility;

pub use history::HistoryKey;

#[cfg(test)]
mod tests;
