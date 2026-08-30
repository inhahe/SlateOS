// `dead_code` and `unused_imports` were both allowed at the crate root. They
// were not stylistic: `main` was `let _app = Mahjong::new();`, so *every*
// drawing and event-handling function in the file was unreachable and the
// compiler would have said so on every build. Silencing the warning is what
// let the app sit unwired. Both are now denied by the lane's clippy gate.
#![allow(clippy::too_many_lines)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::cast_possible_wrap)]

//! Slate OS Mahjong Solitaire — a classic tile-matching puzzle game.
//!
//! Features a 144-tile "Turtle" pyramid layout across multiple layers. Tiles
//! are free when they have no tile on top and at least one open side (left or
//! right). Click or keyboard-select two matching free tiles to remove them.
//! Seasons match any season; flowers match any flower; all other tiles must
//! match exactly. Supports undo (Z), hints (H), shuffle (S), and new game (N).
//! The deal is seeded from the system and re-dealt from a stored seed, so a
//! game can be repeated on request but is not the same for every player.
//! Catppuccin Mocha color palette.
//!
//! # Layout
//!
//! Every coordinate in this file comes from [`Layout::solve`], which is handed
//! the size the window reports on the frame being drawn. The board's tile size
//! is *solved for* rather than fixed: the turtle is 14 columns by 8 rows plus
//! the four-layer stagger, and the tile is made as large as will fit that
//! bounding box into the space between the header and the help bar. The
//! previous version pinned `TILE_W`/`TILE_H`/`BOARD_OFFSET_*` to constants and
//! put the legend at x = 730, so any window narrower than about 900 pixels
//! silently lost the legend off its right edge and any window shorter than 500
//! drew the bottom row of tiles underneath the help bar.

use std::process::ExitCode;

use guitk::color::Color;
use guitk::event::{Event, EventResult, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::frame::{Frame, Rect};
use guitk::probe::Probe;
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::rng::{RandomSource, SeededRng, seed_from_system};
use guitk::style::CornerRadii;
use guitk::text;
use oswindow::app::{self, App, Response};

// ── Catppuccin Mocha palette ────────────────────────────────────────
const BASE: Color = Color::from_hex(0x1E1E2E);
const MANTLE: Color = Color::from_hex(0x181825);
const CRUST: Color = Color::from_hex(0x11111B);
const TEXT_COLOR: Color = Color::from_hex(0xCDD6F4);
const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const GREEN: Color = Color::from_hex(0xA6E3A1);
const RED: Color = Color::from_hex(0xF38BA8);
const YELLOW: Color = Color::from_hex(0xF9E2AF);
const PEACH: Color = Color::from_hex(0xFAB387);
const LAVENDER: Color = Color::from_hex(0xB4BEFE);
const OVERLAY0: Color = Color::from_hex(0x6C7086);

// ── Tile colors (one per suit category) ─────────────────────────────
const BAMBOO_COLOR: Color = Color::from_hex(0xA6E3A1);
const CIRCLE_COLOR: Color = Color::from_hex(0x89B4FA);
const CHARACTER_COLOR: Color = Color::from_hex(0xF38BA8);
const WIND_COLOR: Color = Color::from_hex(0xF9E2AF);
const DRAGON_COLOR: Color = Color::from_hex(0xCBA6F7);
const SEASON_COLOR: Color = Color::from_hex(0xFAB387);
const FLOWER_COLOR: Color = Color::from_hex(0x94E2D5);
const TILE_BG: Color = Color::from_hex(0x45475A);
const TILE_BG_FREE: Color = Color::from_hex(0x585B70);
const TILE_SELECTED: Color = Color::from_hex(0x89B4FA);
const TILE_HINT: Color = Color::from_hex(0xA6E3A1);
const TILE_SHADOW: Color = Color::from_hex(0x11111B);

/// The legend's rows: the codes that appear on the tiles, and one tile of that
/// group to ask for the rest.
///
/// A module constant rather than a `let` inside the drawing pass, because
/// [`legend_width`] measures it to decide how wide the legend column has to be.
/// Two copies of this list -- one measured, one drawn -- would be a column
/// sized for a legend other than the one on the screen.
///
/// The group's name, its colour and its asterisk are all asked of the tile
/// rather than written out again beside it. The old table repeated the name and
/// the colour, so a palette change would have left the legend's swatch behind
/// and an asterisk typed by hand could promise a matching rule the game does
/// not play.
const LEGEND_ITEMS: [(&str, TileKind); 7] = [
    ("B1-B9", TileKind::Bamboo(1)),
    ("C1-C9", TileKind::Circle(1)),
    ("W1-W9", TileKind::Character(1)),
    ("E/S/W/N", TileKind::Wind(0)),
    ("Dr/Dg/Dw", TileKind::Dragon(0)),
    ("Sp/Su/Au/Wi", TileKind::Season(0)),
    ("Pl/Or/Ch/Bm", TileKind::Flower(0)),
];

/// One legend row's text: `codes (Group)`, with an asterisk on the groups whose
/// tiles match any other tile of the same group rather than only their twin.
fn legend_label(codes: &str, kind: TileKind) -> String {
    let wild = if kind.wildcard() { "*" } else { "" };
    format!("{codes} ({}{wild})", kind.category())
}

/// The note under the legend explaining the asterisks.
const LEGEND_NOTE: &str = "* match any in group";

/// The key hints along the bottom of the window.
const HELP_TEXT: &str =
    "N=New  Z=Undo  H=Hint  S=Shuffle  Arrows=Navigate  Enter/Space=Select  Esc=Deselect";

// ── Layout ──────────────────────────────────────────────────────────

/// The size the window asks for when it opens.
const WINDOW_WIDTH: f32 = 1000.0;
/// The height the window asks for when it opens.
const WINDOW_HEIGHT: f32 = 700.0;

/// A tile is this many times as tall as it is wide.
///
/// The old constants were 42 by 54; the ratio, not either number, is what has
/// to survive being resized, so it is the ratio that is written down.
const TILE_ASPECT: f32 = 54.0 / 42.0;

/// The gap between neighbouring tiles, as a share of the tile's width.
const TILE_GAP_SHARE: f32 = 2.0 / 42.0;

/// How far each layer is shifted up and left from the one below, as a share of
/// the tile's width. This is what makes the stack read as a stack.
const LAYER_OFFSET_SHARE: f32 = 4.0 / 42.0;

/// A tile's corner rounding, as a share of its width.
const TILE_CORNER_SHARE: f32 = 4.0 / 42.0;

/// How far a tile's shadow is displaced, as a share of its width.
const SHADOW_SHARE: f32 = 3.0 / 42.0;

/// Total number of tile positions in the Turtle layout.
const LAYOUT_SIZE: usize = 144;

/// The legend's seven rows plus its heading and its footnote.
const LEGEND_ROWS: usize = 9;

/// Where a tile sits relative to the turtle's origin, measured in tile widths.
///
/// One tile is 1.0 wide and [`TILE_ASPECT`] tall in these units, so multiplying
/// a pair of them by `tile_w` gives pixels. Both [`turtle_extent`] and
/// [`Layout::tile_rect`] go through here, which is what makes the bounding box
/// the tiles are fitted into the same box they are then drawn in.
fn tile_offset_units(pos: TilePos) -> (f32, f32) {
    // The stagger runs up and left, so a higher layer subtracts.
    let off = pos.layer as f32 * LAYER_OFFSET_SHARE;
    (
        pos.col as f32 * (1.0 + TILE_GAP_SHARE) - off,
        pos.row as f32 * (TILE_ASPECT + TILE_GAP_SHARE) - off,
    )
}

/// The turtle's bounding box in tile widths: `(min_x, min_y, span_w, span_h)`.
///
/// Measured over the positions the deal actually uses rather than written down
/// as `cols x rows + layers`. That formula was here first, and it was wrong in
/// two directions at once: it reserved four layer-offsets on the left and top
/// that no tile ever reaches -- the upper layers sit at the *middle* columns,
/// so nothing is ever staggered out past column 0 -- which shrank every tile to
/// buy margin, and it restated the turtle's shape a second time, so an edit to
/// [`turtle_layout`] could put a tile outside the box that was supposed to
/// contain it with nothing to say so.
fn turtle_extent() -> (f32, f32, f32, f32) {
    static EXTENT: std::sync::OnceLock<(f32, f32, f32, f32)> = std::sync::OnceLock::new();
    *EXTENT.get_or_init(|| {
        let mut min_x = f32::INFINITY;
        let mut min_y = f32::INFINITY;
        let mut max_x = f32::NEG_INFINITY;
        let mut max_y = f32::NEG_INFINITY;
        for pos in turtle_layout() {
            let (x, y) = tile_offset_units(pos);
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            // A tile is one unit wide and `TILE_ASPECT` units tall.
            max_x = max_x.max(x + 1.0);
            max_y = max_y.max(y + TILE_ASPECT);
        }
        if min_x > max_x || min_y > max_y {
            // An empty layout has no extent; a zero span makes the tile size
            // solve to zero, which every drawing guard already handles.
            return (0.0, 0.0, 0.0, 0.0);
        }
        (min_x, min_y, max_x - min_x, max_y - min_y)
    })
}

/// Every rectangle and font size the frame is drawn from, solved for the size
/// the window reported.
#[derive(Clone, Copy, Debug)]
struct Layout {
    /// The whole window.
    window: Rect,
    /// The strip holding the title, the counters and the message.
    header: Rect,
    /// The strip along the bottom holding the key hints.
    help: Rect,
    /// The column on the right holding the legend.
    legend: Rect,
    /// What is left for the board once the header, help and legend are taken.
    board: Rect,
    /// The bounding box the 144 tiles actually occupy inside `board`, centred.
    turtle: Rect,
    /// One tile's width. Everything on the board is a multiple of it.
    tile_w: f32,
    /// One tile's height.
    tile_h: f32,
    /// General padding.
    pad: f32,
    /// The title's font size.
    title: f32,
    /// The counters' and message's font size.
    status: f32,
    /// The legend's font size, also used for the help bar.
    small: f32,
    /// The font a tile's label is drawn at.
    tile_font: f32,
}

impl Layout {
    /// Solve the whole layout for a window of `w` by `h`.
    fn solve(w: f32, h: f32) -> Self {
        let w = w.max(0.0);
        let h = h.max(0.0);
        // Scaled off the smaller side so a wide-and-short window gets small
        // padding rather than padding that eats its whole height.
        let pad = (w.min(h) * 0.015).clamp(2.0, 14.0).min(w.min(h) / 2.0);
        let title = (h * 0.031).clamp(10.0, 26.0);
        let status = (h * 0.023).clamp(8.0, 18.0);
        let small = (h * 0.017).clamp(7.0, status);

        let window = Rect::new(0.0, 0.0, w, h);
        // The header holds two stacked lines -- title, then counters -- so it
        // is sized from those two fonts rather than from a share of the height
        // that happens to be big enough at 700 pixels.
        let header_h = (title + status + pad * 3.0).min(h);
        let header = Rect::new(0.0, 0.0, w, header_h);
        // The help bar takes what the header left and no more, which is what
        // keeps it off the header: `help_h <= h - header_h`, so `h - help_h`
        // is already at or below `header_h`. An earlier draft wrote
        // `(h - help_h).max(header.bottom())` on top of that, and mutation
        // testing showed the `max` could not be made to bind by any window --
        // a guard whose losing side never loses is a guard no test can check.
        let help_h = (small + pad * 2.0).min((h - header_h).max(0.0));
        let help = Rect::new(0.0, h - help_h, w, help_h);

        let middle = Rect::new(0.0, header.bottom(), w, (help.y - header.bottom()).max(0.0));

        // The legend takes the width its widest row actually measures, or it is
        // not drawn at all. The first draft capped it at `w / 3` and then asked
        // whether the capped width fit in half the window -- which is true for
        // every `w`, so the whole test was decoration and the legend was drawn
        // squeezed at every width instead of dropped at the narrow ones. A
        // column narrowed to fit reads "B1-B9 (Bam..." and tells the player
        // nothing while still taking the width off the board, so the honest
        // choice is all of it or none of it.
        let needed = legend_width(small, pad);
        let legend_fits = needed <= w / 3.0 && middle.h >= small * LEGEND_ROWS as f32;
        let legend = if legend_fits {
            Rect::new(middle.right() - needed, middle.y, needed, middle.h)
        } else {
            Rect::new(middle.right(), middle.y, 0.0, middle.h)
        };
        let board = Rect::new(middle.x, middle.y, (legend.x - middle.x).max(0.0), middle.h);

        // Solve the tile size: the turtle's bounding box is `span_w` by
        // `span_h` tile widths, measured over the positions the deal uses, and
        // both must fit inside the padded board.
        let inner = inset(board, pad);
        let (_, _, span_w, span_h) = turtle_extent();
        // No `.max(0.0)` on the ratio: `inset` never returns a negative side,
        // so neither ratio can be negative. It was here, and the sweep could
        // not make it bind -- a floor that never floors is a floor no test can
        // check, and it invited the reader to believe `inner` might be
        // negative, which is the thing `inset` exists to rule out.
        let tile_w = if span_w > 0.0 && span_h > 0.0 {
            (inner.w / span_w).min(inner.h / span_h)
        } else {
            0.0
        };
        let tile_h = tile_w * TILE_ASPECT;

        // Centre the turtle in what is left, so a window wider than the board
        // needs does not pin it to the left with all the slack on one side.
        let turtle = Rect::new(
            inner.x + (inner.w - tile_w * span_w) / 2.0,
            inner.y + (inner.h - tile_w * span_h) / 2.0,
            tile_w * span_w,
            tile_w * span_h,
        );

        // A label has to fit inside the tile it names; the widest is three
        // characters ("Dr", "Su", "B1" are two, but the measure is taken from
        // the real set rather than assumed).
        let tile_font = fit_tile_font(tile_w, tile_h);

        Self {
            window,
            header,
            help,
            legend,
            board,
            turtle,
            tile_w,
            tile_h,
            pad,
            title,
            status,
            small,
            tile_font,
        }
    }

    /// The rectangle a tile at `pos` occupies, in window coordinates.
    ///
    /// This is the *only* place a tile's position is computed. The drawing pass
    /// records a hit box from it and the click reads that box back, so the
    /// picture and the hit test cannot disagree -- the old code had
    /// `tile_screen_pos` for drawing and `tile_at_screen` for clicking, two
    /// copies of one geometry kept in step by nothing but care.
    fn tile_rect(&self, pos: TilePos) -> Rect {
        let (min_x, min_y, _, _) = turtle_extent();
        let (x, y) = tile_offset_units(pos);
        // Subtracting the minimum is what puts the leftmost tile on the
        // turtle's left edge, whichever layer that tile happens to be on.
        Rect::new(
            self.turtle.x + (x - min_x) * self.tile_w,
            self.turtle.y + (y - min_y) * self.tile_w,
            self.tile_w,
            self.tile_h,
        )
    }
}

/// The width the legend column needs: a swatch, a gap and its widest row.
fn legend_width(font: f32, pad: f32) -> f32 {
    let widest = LEGEND_ITEMS
        .iter()
        .map(|&(codes, kind)| {
            text::measure(&legend_label(codes, kind), font, FontWeightHint::Regular)
        })
        .fold(
            text::measure(LEGEND_NOTE, font, FontWeightHint::Regular),
            f32::max,
        );
    widest + font + pad * 3.0
}

/// The largest font at which every tile label still fits inside a tile.
///
/// The old code drew every label at a fixed 16pt, which overflowed the tile the
/// moment the tile was smaller than the window it had been designed for -- and
/// since the tile was a constant too, that could not happen, so the bug was
/// invisible until the tile started moving.
fn fit_tile_font(tile_w: f32, tile_h: f32) -> f32 {
    // Measured over the real set rather than over a label picked as "surely the
    // widest": `all_tile_kinds` is the same list the deal is built from, so a
    // label added to the game cannot escape the fit.
    let widest = all_tile_kinds()
        .iter()
        .map(|k| text::measure(k.label(), 100.0, FontWeightHint::Bold))
        .fold(0.0_f32, f32::max);
    // `measure` is linear in the font size, so one measurement at 100 gives the
    // width per point and the font that fits follows by division.
    let per_point = widest / 100.0;
    let by_width = if per_point > 0.0 {
        (tile_w * 0.8) / per_point
    } else {
        tile_h
    };
    by_width.min(tile_h * 0.5).max(0.0)
}

/// Shrink a rectangle by `by` on every side, never past nothing.
fn inset(r: Rect, by: f32) -> Rect {
    // Inset by at most half of each side. A rect thinner than `by * 2` cannot
    // give `by` away at both edges, and the first draft clamped only the
    // *size* while moving the origin in by the full amount regardless -- which
    // puts the "inset" rect outside the rect it came from. Mahjong's 200x20
    // window is where that showed: the middle band is zero-tall there, so the
    // padded board came back starting two pixels below the window's bottom
    // edge, and the whole turtle was solved from it.
    let dx = by.clamp(0.0, r.w / 2.0);
    let dy = by.clamp(0.0, r.h / 2.0);
    Rect::new(r.x + dx, r.y + dy, r.w - dx * 2.0, r.h - dy * 2.0)
}

// ── LCG random number generator ────────────────────────────────────

// ── Randomness ──────────────────────────────────────────────────────

/// Seed used when the kernel's entropy source cannot be reached.
///
/// A per-crate constant rather than a shared one, so that two programs which
/// lose entropy on the same boot do not then produce correlated streams. The
/// bytes spell `MAHJONG!`.
const FALLBACK_SEED: u64 = 0x4D41_484A_4F4E_4721;

// This crate used to carry its own copy of the LCG that got copied into
// sixteen crates, together with its own Fisher-Yates over it. Its reduction was
// `(next() >> 33) % max`, which discards the low 31 bits before taking the
// remainder, so unlike most of the copies it never read the counter-like low
// bits of a power-of-two-modulus LCG.
//
// That was luck worth recording rather than a design: a shuffle is the worst
// possible caller for the broken reduction, because its bound counts all the
// way down to 2 and so passes through every power of two on the way. In
// `apps/maze`, where the same hand-rolled shuffle ran over four elements
// against the un-shifted reduction, it produced 3 of the 24 possible orderings.
// A 144-tile deal would have had a comparable hole in it.
//
// It is replaced anyway. The copy is the defect; this one happened to be a good
// copy, and the only way to know that was to read all sixteen and check.

// ── Tile types ──────────────────────────────────────────────────────

/// The 36 distinct tile types in a Mahjong set.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum TileKind {
    Bamboo(u8),    // 1..=9
    Circle(u8),    // 1..=9
    Character(u8), // 1..=9
    Wind(u8),      // 0=East, 1=South, 2=West, 3=North
    Dragon(u8),    // 0=Red, 1=Green, 2=White
    Season(u8),    // 0=Spring, 1=Summer, 2=Autumn, 3=Winter
    Flower(u8),    // 0=Plum, 1=Orchid, 2=Chrysanthemum, 3=Bamboo
}

impl TileKind {
    /// Whether this tile matches any other tile of its own group rather than
    /// only an identical one.
    ///
    /// This is the asterisk the legend prints. It is the same predicate
    /// [`Self::matches`] plays by, so the legend cannot promise a rule the game
    /// does not follow -- the asterisks used to be typed into the legend table
    /// by hand, one edit away from saying something untrue.
    fn wildcard(self) -> bool {
        matches!(self, TileKind::Season(_) | TileKind::Flower(_))
    }

    /// Whether two tiles can be matched and removed together.
    /// Seasons match any other season; flowers match any other flower;
    /// all other tiles must match exactly.
    fn matches(self, other: Self) -> bool {
        if self.wildcard() && other.wildcard() {
            // Same group only: a season does not match a flower. Comparing
            // discriminants asks "same variant?" without asking "same number?",
            // which is exactly the rule.
            std::mem::discriminant(&self) == std::mem::discriminant(&other)
        } else {
            self == other
        }
    }

    /// Short label for rendering on the tile face.
    fn label(self) -> &'static str {
        match self {
            TileKind::Bamboo(1) => "B1",
            TileKind::Bamboo(2) => "B2",
            TileKind::Bamboo(3) => "B3",
            TileKind::Bamboo(4) => "B4",
            TileKind::Bamboo(5) => "B5",
            TileKind::Bamboo(6) => "B6",
            TileKind::Bamboo(7) => "B7",
            TileKind::Bamboo(8) => "B8",
            TileKind::Bamboo(9) => "B9",
            TileKind::Circle(1) => "C1",
            TileKind::Circle(2) => "C2",
            TileKind::Circle(3) => "C3",
            TileKind::Circle(4) => "C4",
            TileKind::Circle(5) => "C5",
            TileKind::Circle(6) => "C6",
            TileKind::Circle(7) => "C7",
            TileKind::Circle(8) => "C8",
            TileKind::Circle(9) => "C9",
            TileKind::Character(1) => "W1",
            TileKind::Character(2) => "W2",
            TileKind::Character(3) => "W3",
            TileKind::Character(4) => "W4",
            TileKind::Character(5) => "W5",
            TileKind::Character(6) => "W6",
            TileKind::Character(7) => "W7",
            TileKind::Character(8) => "W8",
            TileKind::Character(9) => "W9",
            TileKind::Wind(0) => "E",
            TileKind::Wind(1) => "S",
            TileKind::Wind(2) => "W",
            TileKind::Wind(3) => "N",
            TileKind::Dragon(0) => "Dr",
            TileKind::Dragon(1) => "Dg",
            TileKind::Dragon(2) => "Dw",
            TileKind::Season(0) => "Sp",
            TileKind::Season(1) => "Su",
            TileKind::Season(2) => "Au",
            TileKind::Season(3) => "Wi",
            TileKind::Flower(0) => "Pl",
            TileKind::Flower(1) => "Or",
            TileKind::Flower(2) => "Ch",
            TileKind::Flower(3) => "Bm",
            _ => "??",
        }
    }

    /// Color for rendering this tile's text.
    fn text_color(self) -> Color {
        match self {
            TileKind::Bamboo(_) => BAMBOO_COLOR,
            TileKind::Circle(_) => CIRCLE_COLOR,
            TileKind::Character(_) => CHARACTER_COLOR,
            TileKind::Wind(_) => WIND_COLOR,
            TileKind::Dragon(_) => DRAGON_COLOR,
            TileKind::Season(_) => SEASON_COLOR,
            TileKind::Flower(_) => FLOWER_COLOR,
        }
    }

    /// Category label for the sidebar legend.
    fn category(self) -> &'static str {
        match self {
            TileKind::Bamboo(_) => "Bamboo",
            TileKind::Circle(_) => "Circle",
            TileKind::Character(_) => "Character",
            TileKind::Wind(_) => "Wind",
            TileKind::Dragon(_) => "Dragon",
            TileKind::Season(_) => "Season",
            TileKind::Flower(_) => "Flower",
        }
    }
}

/// Generate all 42 distinct tile types across 7 categories.
/// Base types (34): 9 Bamboo + 9 Circle + 9 Character + 4 Wind + 3 Dragon.
/// Bonus types (8): 4 Season + 4 Flower (each unique, but seasons match any
/// season and flowers match any flower).
fn all_tile_kinds() -> Vec<TileKind> {
    let mut kinds = Vec::with_capacity(42);
    for i in 1..=9 {
        kinds.push(TileKind::Bamboo(i));
    }
    for i in 1..=9 {
        kinds.push(TileKind::Circle(i));
    }
    for i in 1..=9 {
        kinds.push(TileKind::Character(i));
    }
    for i in 0..4 {
        kinds.push(TileKind::Wind(i));
    }
    for i in 0..3 {
        kinds.push(TileKind::Dragon(i));
    }
    for i in 0..4 {
        kinds.push(TileKind::Season(i));
    }
    for i in 0..4 {
        kinds.push(TileKind::Flower(i));
    }
    kinds
}

/// The 34 base tile types that each appear 4 times.
fn base_tile_kinds() -> Vec<TileKind> {
    let mut kinds = Vec::with_capacity(34);
    for i in 1..=9 {
        kinds.push(TileKind::Bamboo(i));
    }
    for i in 1..=9 {
        kinds.push(TileKind::Circle(i));
    }
    for i in 1..=9 {
        kinds.push(TileKind::Character(i));
    }
    for i in 0..4 {
        kinds.push(TileKind::Wind(i));
    }
    for i in 0..3 {
        kinds.push(TileKind::Dragon(i));
    }
    kinds
}

/// Generate the traditional 144-tile Mahjong set.
/// 34 base types x 4 copies each = 136, plus 4 unique seasons and 4 unique
/// flowers = 144 total.
fn full_tile_set() -> Vec<TileKind> {
    let base = base_tile_kinds();
    let mut tiles = Vec::with_capacity(144);
    for kind in &base {
        for _ in 0..4 {
            tiles.push(*kind);
        }
    }
    // Seasons and flowers: one copy each (they match any in their group).
    for i in 0..4 {
        tiles.push(TileKind::Season(i));
    }
    for i in 0..4 {
        tiles.push(TileKind::Flower(i));
    }
    tiles
}

// ── Layout positions ────────────────────────────────────────────────

/// A position in the 3D tile grid: (layer, row, col).
/// Layer 0 is the bottom; higher layers are stacked on top.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct TilePos {
    layer: usize,
    row: usize,
    col: usize,
}

/// A placed tile on the board.
#[derive(Clone, Copy, Debug)]
struct PlacedTile {
    pos: TilePos,
    kind: TileKind,
    removed: bool,
}

/// The classic "Turtle" Mahjong Solitaire layout: exactly 144 positions across
/// five layers, each layer resting wholly on the one below it.
///
/// | Layer | Tiles | Shape |
/// |---|---|---|
/// | 0 | 87 | the body: eight rows of 12/8/10/12/12/10/8/12, plus the tail on the left and the two head tiles on the right |
/// | 1 | 36 | 6x6, rows 1..=6 by cols 4..=9 |
/// | 2 | 16 | 4x4, rows 2..=5 by cols 5..=8 |
/// | 3 | 4 | 2x2, rows 3..=4 by cols 6..=7 |
/// | 4 | 1 | the cap |
///
/// The first version of this function returned **172** positions, not 144, and
/// nothing said so: `Board::new` deals `min(positions, kinds)` tiles, so the
/// last 28 positions simply never received one. Those 28 were the whole of
/// layers 3 and 4 and two thirds of layer 2 -- the turtle had no cap, and the
/// pyramid stopped mid-course. It also floated tiles: layer 1 ran cols 3..=10
/// over a layer-0 row that only ran 2..=9, so `(1, 1, 10)` and `(1, 6, 10)`
/// rested on nothing, covering no tile and freeing none when taken.
///
/// Both faults were invisible because nothing ever drew the board.
fn turtle_layout() -> Vec<TilePos> {
    let mut positions = Vec::with_capacity(LAYOUT_SIZE);
    let mut push = |layer: usize, row: usize, cols: std::ops::RangeInclusive<usize>| {
        for col in cols {
            positions.push(TilePos { layer, row, col });
        }
    };

    // Layer 0, the body: 84 tiles in eight rows, waisted in the middle so the
    // silhouette reads as a shell rather than a rectangle.
    push(0, 0, 2..=13);
    push(0, 1, 4..=11);
    push(0, 2, 3..=12);
    push(0, 3, 2..=13);
    push(0, 4, 2..=13);
    push(0, 5, 3..=12);
    push(0, 6, 4..=11);
    push(0, 7, 2..=13);
    // The tail, one tile off the left of the middle, and the head, two off the
    // right. Both are always free -- they have an open side by construction --
    // which is what gives the player a first move on any deal.
    push(0, 3, 1..=1);
    push(0, 4, 14..=15);

    // Layers 1..=3, each nested strictly inside the one below so that every
    // tile rests on a tile and every layer covers fewer squares than it sits on.
    for row in 1..=6 {
        push(1, row, 4..=9);
    }
    for row in 2..=5 {
        push(2, row, 5..=8);
    }
    for row in 3..=4 {
        push(3, row, 6..=7);
    }

    // The cap.
    push(4, 3, 6..=6);

    positions
}

// ── Game state ──────────────────────────────────────────────────────

/// Undo record: a pair of tiles that was removed.
#[derive(Clone, Debug)]
struct UndoEntry {
    tile_a: usize,
    tile_b: usize,
}

/// Overall game status.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GameStatus {
    Playing,
    Won,
    Lost,
}

/// The board state: a collection of placed tiles.
struct Board {
    tiles: Vec<PlacedTile>,
}

impl Board {
    /// Create a new board by placing shuffled tiles on the turtle layout.
    fn new(rng: &mut SeededRng) -> Self {
        let positions = turtle_layout();
        let mut tile_kinds = full_tile_set();

        // Trim or pad tile_kinds to match the number of layout positions.
        // The turtle layout should have exactly 144 positions but we handle
        // slight mismatches gracefully by truncating.
        let count = positions.len().min(tile_kinds.len());
        tile_kinds.truncate(count);

        rng.shuffle(&mut tile_kinds);

        let tiles = positions
            .iter()
            .zip(tile_kinds.iter())
            .map(|(&pos, &kind)| PlacedTile {
                pos,
                kind,
                removed: false,
            })
            .collect();

        Board { tiles }
    }

    /// Create a board from explicit positions and kinds.
    ///
    /// Test-only, and marked so: it was reachable from a non-test build and
    /// dead there, which is one of the things the crate-wide
    /// `#![allow(dead_code)]` was hiding.
    #[cfg(test)]
    fn from_parts(positions: &[TilePos], kinds: &[TileKind]) -> Self {
        // `zip` stops at the shorter of the two, which is the same "handle a
        // mismatch by truncating" rule `new` uses, without an index that
        // could outrun either slice.
        let tiles = positions
            .iter()
            .zip(kinds.iter())
            .map(|(&pos, &kind)| PlacedTile {
                pos,
                kind,
                removed: false,
            })
            .collect();
        Board { tiles }
    }

    /// Number of tiles still on the board (not removed).
    fn remaining(&self) -> usize {
        self.tiles.iter().filter(|t| !t.removed).count()
    }

    /// Check if a tile is "free" — not removed, no tile on top, and has an
    /// open left or right side.
    fn is_free(&self, idx: usize) -> bool {
        let tile = match self.tiles.get(idx) {
            Some(t) => t,
            None => return false,
        };
        if tile.removed {
            return false;
        }

        let pos = tile.pos;

        // Check if any non-removed tile is directly on top (one layer above,
        // overlapping in row/col). A tile on layer L+1 at (r, c) covers
        // the tile at layer L if their row and column ranges overlap.
        for (i, other) in self.tiles.iter().enumerate() {
            if i == idx || other.removed {
                continue;
            }
            if pos.layer.checked_add(1) == Some(other.pos.layer) {
                // Tiles on adjacent layer overlap if they share the same row/col
                // position. Because tiles occupy a 1x1 cell in our grid, a tile
                // at (r, c) on layer L+1 covers (r, c) on layer L.
                if other.pos.row == pos.row && other.pos.col == pos.col {
                    return false;
                }
            }
        }

        // Check left/right openness. A tile is blocked on the left if there
        // is a non-removed tile at the same layer and row at col-1, and
        // blocked on the right if there is one at col+1.
        let mut blocked_left = false;
        let mut blocked_right = false;
        for (i, other) in self.tiles.iter().enumerate() {
            if i == idx || other.removed {
                continue;
            }
            if other.pos.layer == pos.layer && other.pos.row == pos.row {
                if other.pos.col.checked_add(1) == Some(pos.col) {
                    blocked_left = true;
                }
                if pos.col.checked_add(1) == Some(other.pos.col) {
                    blocked_right = true;
                }
            }
        }

        // Free if at least one side is open.
        !blocked_left || !blocked_right
    }

    /// Find all currently free tile indices.
    fn free_tiles(&self) -> Vec<usize> {
        (0..self.tiles.len()).filter(|&i| self.is_free(i)).collect()
    }

    /// Find a matching pair of free tiles, if one exists.
    fn find_hint(&self) -> Option<(usize, usize)> {
        let free = self.free_tiles();
        for (n, &a) in free.iter().enumerate() {
            for &b in free.iter().skip(n).skip(1) {
                let (Some(ta), Some(tb)) = (self.tiles.get(a), self.tiles.get(b)) else {
                    continue;
                };
                if ta.kind.matches(tb.kind) {
                    return Some((a, b));
                }
            }
        }
        None
    }

    /// Check if the game is won (all tiles removed).
    fn is_won(&self) -> bool {
        self.remaining() == 0
    }

    /// Check if the game is lost (no valid pairs remain among free tiles).
    fn is_lost(&self) -> bool {
        self.remaining() > 0 && self.find_hint().is_none()
    }

    /// Remove a pair of tiles.
    fn remove_pair(&mut self, a: usize, b: usize) {
        if let Some(tile) = self.tiles.get_mut(a) {
            tile.removed = true;
        }
        if let Some(tile) = self.tiles.get_mut(b) {
            tile.removed = true;
        }
    }

    /// Restore a pair of tiles (undo).
    fn restore_pair(&mut self, a: usize, b: usize) {
        if let Some(tile) = self.tiles.get_mut(a) {
            tile.removed = false;
        }
        if let Some(tile) = self.tiles.get_mut(b) {
            tile.removed = false;
        }
    }

    /// Shuffle the remaining (non-removed) tiles' kinds while keeping
    /// their positions fixed.
    fn shuffle_remaining(&mut self, rng: &mut SeededRng) {
        let active_indices: Vec<usize> = self
            .tiles
            .iter()
            .enumerate()
            .filter(|(_, t)| !t.removed)
            .map(|(i, _)| i)
            .collect();

        let mut kinds: Vec<TileKind> = active_indices
            .iter()
            .filter_map(|&i| self.tiles.get(i).map(|t| t.kind))
            .collect();

        rng.shuffle(&mut kinds);

        for (&idx, &kind) in active_indices.iter().zip(kinds.iter()) {
            if let Some(t) = self.tiles.get_mut(idx) {
                t.kind = kind;
            }
        }
    }

    /// Where a tile sits in the window, at the layout the frame was drawn with.
    ///
    /// A thin forward to [`Layout::tile_rect`] so that the board keeps its
    /// index-based interface -- `move_cursor` compares tile positions and does
    /// not want to know about `TilePos` -- while there is still exactly one
    /// piece of code that turns a `TilePos` into a rectangle.
    fn tile_rect(&self, l: &Layout, idx: usize) -> Option<Rect> {
        self.tiles.get(idx).map(|t| l.tile_rect(t.pos))
    }

    /// The live tiles in the order they must be *painted*: bottom layer first,
    /// so a tile on a higher layer covers the one it rests on.
    ///
    /// The hit test then reads this order backwards, which is what makes the
    /// topmost tile win a click without a second sort keyed on layer -- the old
    /// `tile_at_screen` had that second sort, and it was the only thing
    /// standing between a click and the tile buried under the one you aimed at.
    fn paint_order(&self) -> Vec<usize> {
        let mut order: Vec<usize> = self
            .tiles
            .iter()
            .enumerate()
            .filter(|(_, t)| !t.removed)
            .map(|(i, _)| i)
            .collect();
        order.sort_by_key(|&i| self.tiles.get(i).map_or(0, |t| t.pos.layer));
        order
    }
}

// ── Keyboard cursor ─────────────────────────────────────────────────

/// Cursor for keyboard navigation. Tracks which tile index in the free
/// list is currently focused.
#[derive(Clone, Debug)]
struct Cursor {
    /// Index into the board tiles array that the cursor is on.
    tile_idx: Option<usize>,
}

// ── Main app ────────────────────────────────────────────────────────

struct Mahjong {
    board: Board,
    rng: SeededRng,
    seed: u64,
    selected: Option<usize>,
    cursor: Cursor,
    undo_stack: Vec<UndoEntry>,
    moves: u32,
    status: GameStatus,
    hint: Option<(usize, usize)>,
    show_hint: bool,
    message: Option<&'static str>,
    /// The width the window last reported.
    ///
    /// Kept because a click arrives as a pair of window coordinates and has to
    /// be read against the layout the *last frame* was drawn with; without it
    /// the app would have to guess a size to hit-test against.
    width: f32,
    /// The height the window last reported.
    height: f32,
}

impl Mahjong {
    fn new() -> Self {
        // Was `with_seed(42)`: every player, on every machine, got the
        // same 144-tile layout. Predicting a mahjong deal costs the user
        // nothing but the puzzle, so this asks the kernel and falls back
        // rather than refusing -- see `randrange::seeded_from_system`.
        // The `u64` form is used and not the generator form because this
        // app *stores* its seed: `self.seed` is read back to re-deal.
        Self::with_seed(seed_from_system(FALLBACK_SEED))
    }

    fn with_seed(seed: u64) -> Self {
        let mut rng = SeededRng::new(seed);
        let board = Board::new(&mut rng);

        // Initialize cursor to the first free tile, if any.
        let first_free = board.free_tiles().first().copied();

        let mut app = Self {
            board,
            rng,
            seed,
            selected: None,
            cursor: Cursor {
                tile_idx: first_free,
            },
            undo_stack: Vec::new(),
            moves: 0,
            status: GameStatus::Playing,
            hint: None,
            show_hint: false,
            message: None,
            width: WINDOW_WIDTH,
            height: WINDOW_HEIGHT,
        };
        app.update_status();
        app
    }

    /// Remember the size the window last reported.
    fn resize(&mut self, width: f32, height: f32) {
        self.width = width.max(0.0);
        self.height = height.max(0.0);
    }

    /// The layout the last frame was drawn with.
    fn layout(&self) -> Layout {
        Layout::solve(self.width, self.height)
    }

    /// Start a new game with a fresh seed.
    fn new_game(&mut self) {
        self.seed = self.seed.wrapping_add(1);
        let mut rng = SeededRng::new(self.seed);
        self.board = Board::new(&mut rng);
        self.rng = rng;
        self.selected = None;
        self.cursor.tile_idx = self.board.free_tiles().first().copied();
        self.undo_stack.clear();
        self.moves = 0;
        self.status = GameStatus::Playing;
        self.hint = None;
        self.show_hint = false;
        self.message = None;
    }

    /// Update status based on board state.
    fn update_status(&mut self) {
        if self.board.is_won() {
            self.status = GameStatus::Won;
            self.message = Some("You win! Press N for new game.");
        } else if self.board.is_lost() {
            self.status = GameStatus::Lost;
            self.message = Some("No moves left! S=shuffle, N=new");
        }
    }

    /// Try to select a tile (by index) and match if two are selected.
    ///
    /// Returns whether anything on the screen changed, so the window can be
    /// told to redraw only when there is something new to draw. It is **not**
    /// "was the tile selected": clicking a covered tile selects nothing and
    /// still returns `true`, because the refusal is printed in the message
    /// line and that line has to be repainted to be read.
    fn try_select(&mut self, idx: usize) -> bool {
        if self.status != GameStatus::Playing {
            return false;
        }

        if !self.board.is_free(idx) {
            let changed = self.message != Some("Tile is not free");
            self.message = Some("Tile is not free");
            return changed;
        }

        match self.selected {
            None => {
                self.selected = Some(idx);
                self.show_hint = false;
                self.message = None;
            }
            Some(prev) => {
                if prev == idx {
                    // Deselect
                    self.selected = None;
                    self.message = None;
                } else if match (self.board.tiles.get(prev), self.board.tiles.get(idx)) {
                    (Some(a), Some(b)) => a.kind.matches(b.kind),
                    _ => false,
                } {
                    // Match found!
                    self.board.remove_pair(prev, idx);
                    self.undo_stack.push(UndoEntry {
                        tile_a: prev,
                        tile_b: idx,
                    });
                    self.selected = None;
                    self.moves = self.moves.saturating_add(1);
                    self.show_hint = false;
                    self.hint = None;
                    self.message = None;
                    self.update_status();
                    // Update cursor to a free tile if current is removed.
                    if let Some(ci) = self.cursor.tile_idx
                        && self.board.tiles.get(ci).is_none_or(|t| t.removed)
                    {
                        self.cursor.tile_idx = self.board.free_tiles().first().copied();
                    }
                } else {
                    self.message = Some("Tiles don't match!");
                    self.selected = Some(idx);
                }
            }
        }
        true
    }

    /// Undo the last move. Returns whether the screen changed.
    fn undo(&mut self) -> bool {
        if let Some(entry) = self.undo_stack.pop() {
            self.board.restore_pair(entry.tile_a, entry.tile_b);
            self.moves = self.moves.saturating_sub(1);
            self.selected = None;
            self.show_hint = false;
            self.hint = None;
            self.status = GameStatus::Playing;
            self.message = Some("Undo!");
            true
        } else {
            let changed = self.message != Some("Nothing to undo");
            self.message = Some("Nothing to undo");
            changed
        }
    }

    /// Show a hint (highlight a valid pair). Returns whether anything changed.
    fn show_hint_pair(&mut self) -> bool {
        if self.status != GameStatus::Playing {
            return false;
        }
        match self.board.find_hint() {
            Some(pair) => {
                self.hint = Some(pair);
                self.show_hint = true;
                self.message = Some("Hint shown (green tiles)");
            }
            None => {
                self.show_hint = false;
                self.message = Some("No valid pairs!");
            }
        }
        true
    }

    /// Shuffle remaining tiles. Returns whether anything changed.
    fn shuffle_tiles(&mut self) -> bool {
        // A won board has nothing left to shuffle; a *lost* one is exactly the
        // board this is for, which is why the guard names `Won` and not
        // `!= Playing`.
        if self.status == GameStatus::Won {
            return false;
        }
        self.board.shuffle_remaining(&mut self.rng);
        self.selected = None;
        self.show_hint = false;
        self.hint = None;
        self.status = GameStatus::Playing;
        self.message = Some("Tiles shuffled!");
        self.update_status();
        true
    }

    /// Move cursor in the given direction among free tiles.
    ///
    /// Distances are measured in *tile widths*, not pixels, so the same key
    /// press picks the same neighbour whatever size the window is. Measuring in
    /// pixels would have been just as wrong at every size but the one the
    /// weights were tuned at, and silently so.
    fn move_cursor(&mut self, dx: i32, dy: i32) -> bool {
        let free = self.board.free_tiles();
        let Some(&first_free) = free.first() else {
            let moved = self.cursor.tile_idx.is_some();
            self.cursor.tile_idx = None;
            return moved;
        };

        let l = self.layout();
        let current_idx = self.cursor.tile_idx.unwrap_or(first_free);
        let Some(current) = self.board.tile_rect(&l, current_idx) else {
            let moved = self.cursor.tile_idx != Some(first_free);
            self.cursor.tile_idx = Some(first_free);
            return moved;
        };
        let current_pos = (current.x, current.y);
        // A window so small that a tile has no width leaves every tile at the
        // same point, and "the closest one in this direction" has no answer.
        // Sitting still is the honest response; the alternative is a cursor
        // that jumps to an arbitrary tile on every arrow press.
        if l.tile_w <= 0.0 {
            return false;
        }
        let unit = l.tile_w;

        // Find the closest free tile in the requested direction.
        let mut best: Option<(usize, f32)> = None;
        for &fi in &free {
            if fi == current_idx {
                continue;
            }
            if let Some(r) = self.board.tile_rect(&l, fi) {
                let delta_x = (r.x - current_pos.0) / unit;
                let delta_y = (r.y - current_pos.1) / unit;

                // Check if the tile is in the requested direction. The
                // threshold is a fraction of a tile rather than "one pixel":
                // a layer's stagger is a tenth of a tile, so at any size the
                // tile directly above another must not read as being to its
                // left as well.
                let eps = LAYER_OFFSET_SHARE * 1.5;
                let in_direction = match (dx, dy) {
                    (1, 0) => delta_x > eps,   // right
                    (-1, 0) => delta_x < -eps, // left
                    (0, 1) => delta_y > eps,   // down
                    (0, -1) => delta_y < -eps, // up
                    _ => false,
                };

                if in_direction {
                    // Distance with bias towards the main axis.
                    let main = if dx != 0 {
                        delta_x.abs()
                    } else {
                        delta_y.abs()
                    };
                    let cross = if dx != 0 {
                        delta_y.abs()
                    } else {
                        delta_x.abs()
                    };
                    let dist = main + cross * 2.0;

                    if best.is_none_or(|(_, bd)| dist < bd) {
                        best = Some((fi, dist));
                    }
                }
            }
        }

        if let Some((bi, _)) = best {
            let moved = self.cursor.tile_idx != Some(bi);
            self.cursor.tile_idx = Some(bi);
            moved
        } else {
            // Already at the edge in that direction. Reporting `false` is what
            // stops the window redrawing an identical frame on every arrow
            // press held against the edge.
            false
        }
    }

    // ── Event handling ──────────────────────────────────────────────

    /// Answer a key press.
    ///
    /// Every arm reports whether the screen changed. The old version returned
    /// `()`, so the window had no way to tell a key that did something from one
    /// that did nothing and would have had to redraw on every keystroke -- or,
    /// worse, on none.
    fn handle_key(&mut self, event: &KeyEvent) -> EventResult {
        if !event.pressed {
            return EventResult::Ignored;
        }

        let changed = match event.key {
            Key::N => {
                self.new_game();
                true
            }
            Key::Z => self.undo(),
            Key::H => self.show_hint_pair(),
            Key::S => self.shuffle_tiles(),
            Key::Left => self.move_cursor(-1, 0),
            Key::Right => self.move_cursor(1, 0),
            Key::Up => self.move_cursor(0, -1),
            Key::Down => self.move_cursor(0, 1),
            Key::Enter | Key::Space => match self.cursor.tile_idx {
                Some(ci) => self.try_select(ci),
                None => false,
            },
            Key::Escape => {
                let changed = self.selected.is_some() || self.show_hint || self.message.is_some();
                self.selected = None;
                self.show_hint = false;
                self.message = None;
                changed
            }
            // A key this game does not use is not this game's to swallow: the
            // window may want it for a shortcut of its own.
            _ => return EventResult::Ignored,
        };
        if changed {
            EventResult::Consumed
        } else {
            EventResult::Ignored
        }
    }

    /// Answer a click by asking the frame what is under it.
    ///
    /// The old `handle_mouse` called `tile_at_screen`, which recomputed every
    /// tile's rectangle from the same constants the drawing pass used -- a
    /// second copy of the geometry, kept in step with the picture by nothing
    /// but care, and wrong the moment either side was edited alone.
    fn handle_mouse(&mut self, event: &MouseEvent) -> EventResult {
        // A tile game answers the left button. Answering all three meant a
        // right-click removed a pair, which is a move the player did not make.
        if !matches!(event.kind, MouseEventKind::Press(MouseButton::Left)) {
            return EventResult::Ignored;
        }
        let Some(target) = self
            .frame(self.width, self.height)
            .hit_test(event.x, event.y)
        else {
            return EventResult::Ignored;
        };
        match target {
            Target::Tile(idx) => {
                let moved = self.cursor.tile_idx != Some(idx);
                self.cursor.tile_idx = Some(idx);
                let acted = self.try_select(idx);
                if moved || acted {
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            Target::Title
            | Target::Status
            | Target::Message
            | Target::Legend(_)
            | Target::Help
            | Target::Board => EventResult::Ignored,
        }
    }

    // ── Rendering ───────────────────────────────────────────────────

    /// Draw the whole game at the size the window reports.
    ///
    /// Every box a click is tested against is recorded here as it is painted,
    /// so the hit test cannot disagree with the picture.
    fn frame(&self, w: f32, h: f32) -> Frame<Target> {
        let l = Layout::solve(w, h);
        let mut f = Frame::new(w, h);

        // The background is the window, not a remembered size.
        f.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: w,
            height: h,
            color: BASE,
            corner_radii: CornerRadii::ZERO,
        });
        f.clip(l.window);

        self.draw_header(&mut f, &l);
        self.draw_board(&mut f, &l);
        self.draw_legend(&mut f, &l);
        self.draw_help(&mut f, &l);

        f.unclip();
        f
    }

    /// The title, the counters and the message, stacked in the header band.
    ///
    /// The title used to be at a fixed `(40, 20)` and the message at
    /// `BOARD_OFFSET_X + 400.0`, so in any window narrower than about 640 the
    /// message ran off the right edge and the player never learned why their
    /// click did nothing.
    fn draw_header(&self, f: &mut Frame<Target>, l: &Layout) {
        let inner = inset(l.header, l.pad);
        if inner.w <= 0.0 || inner.h <= 0.0 {
            return;
        }
        let title_box = Rect::new(inner.x, inner.y, inner.w, l.title);
        f.push(RenderCommand::Text {
            x: title_box.x,
            y: title_box.y,
            text: "Mahjong Solitaire".into(),
            color: LAVENDER,
            font_size: l.title,
            font_weight: FontWeightHint::Bold,
            max_width: Some(title_box.w),
            overflow: TextOverflow::Ellipsis,
        });
        f.hit(Target::Title, title_box);

        let line_y = (title_box.bottom() + l.pad).min(inner.bottom() - l.status);
        // The counters take the left half and the message the right, so the
        // two can never be drawn over each other however long either gets.
        let half = inner.w / 2.0;
        let status_box = Rect::new(inner.x, line_y, half, l.status);
        let message_box = Rect::new(inner.x + half, line_y, inner.w - half, l.status);

        let status_text = match self.status {
            GameStatus::Playing => {
                format!(
                    "Tiles: {}  Moves: {}  Free: {}",
                    self.board.remaining(),
                    self.moves,
                    self.board.free_tiles().len()
                )
            }
            GameStatus::Won => format!("YOU WIN!  Moves: {}", self.moves),
            GameStatus::Lost => "No valid moves remain!".into(),
        };
        let status_color = match self.status {
            GameStatus::Playing => SUBTEXT0,
            GameStatus::Won => GREEN,
            GameStatus::Lost => RED,
        };
        f.push(RenderCommand::Text {
            x: status_box.x,
            y: status_box.y,
            text: status_text,
            color: status_color,
            font_size: l.status,
            font_weight: FontWeightHint::Regular,
            max_width: Some(status_box.w),
            overflow: TextOverflow::Ellipsis,
        });
        f.hit(Target::Status, status_box);

        if let Some(msg) = self.message {
            f.push(RenderCommand::Text {
                x: message_box.x,
                y: message_box.y,
                text: msg.into(),
                color: PEACH,
                font_size: l.status,
                font_weight: FontWeightHint::Regular,
                max_width: Some(message_box.w),
                overflow: TextOverflow::Ellipsis,
            });
            f.hit(Target::Message, message_box);
        }
    }

    /// The turtle: every live tile, bottom layer first.
    fn draw_board(&self, f: &mut Frame<Target>, l: &Layout) {
        // A box behind the tiles, so a click in the board's margin is answered
        // as "the board" and not as whichever tile happens to be nearest.
        f.hit(Target::Board, l.board);
        if l.tile_w <= 0.0 || l.tile_h <= 0.0 {
            return;
        }

        for idx in self.board.paint_order() {
            let Some(tile) = self.board.tiles.get(idx) else {
                continue;
            };
            let r = l.tile_rect(tile.pos);
            let (tx, ty) = (r.x, r.y);

            let is_free = self.board.is_free(idx);
            let is_selected = self.selected == Some(idx);
            let is_cursor = self.cursor.tile_idx == Some(idx);
            let is_hint = self.show_hint && self.hint.is_some_and(|(a, b)| idx == a || idx == b);

            // Shadow (gives depth illusion for stacked layers)
            let shadow = l.tile_w * SHADOW_SHARE;
            let corner = CornerRadii::all(l.tile_w * TILE_CORNER_SHARE);
            if tile.pos.layer > 0 {
                f.push(RenderCommand::FillRect {
                    x: tx + shadow,
                    y: ty + shadow,
                    width: r.w,
                    height: r.h,
                    color: TILE_SHADOW,
                    corner_radii: corner,
                });
            }

            // Tile background
            let bg_color = if is_selected {
                TILE_SELECTED
            } else if is_hint {
                TILE_HINT
            } else if is_free {
                TILE_BG_FREE
            } else {
                TILE_BG
            };
            f.push(RenderCommand::FillRect {
                x: r.x,
                y: r.y,
                width: r.w,
                height: r.h,
                color: bg_color,
                corner_radii: corner,
            });

            // Cursor highlight (border effect via slightly larger rect behind)
            if is_cursor && !is_selected {
                // A border thick enough to see at any tile size; at the old
                // fixed 2 pixels it vanished on a large window and swallowed
                // the tile on a small one.
                let bw = (l.tile_w * 0.05).clamp(1.0, 4.0);
                // Top edge
                f.push(RenderCommand::Line {
                    x1: r.x,
                    y1: r.y,
                    x2: r.right(),
                    y2: r.y,
                    color: YELLOW,
                    width: bw,
                });
                // Bottom edge
                f.push(RenderCommand::Line {
                    x1: r.x,
                    y1: r.bottom(),
                    x2: r.right(),
                    y2: r.bottom(),
                    color: YELLOW,
                    width: bw,
                });
                // Left edge
                f.push(RenderCommand::Line {
                    x1: r.x,
                    y1: r.y,
                    x2: r.x,
                    y2: r.bottom(),
                    color: YELLOW,
                    width: bw,
                });
                // Right edge
                f.push(RenderCommand::Line {
                    x1: r.right(),
                    y1: r.y,
                    x2: r.right(),
                    y2: r.bottom(),
                    color: YELLOW,
                    width: bw,
                });
            }

            // Tile label text
            let label = tile.kind.label();
            let text_color = if is_selected {
                CRUST
            } else {
                tile.kind.text_color()
            };
            f.push(RenderCommand::Text {
                x: text::center_x(label, r.x + r.w / 2.0, l.tile_font, FontWeightHint::Bold),
                y: r.y + r.h / 2.0 - l.tile_font / 2.0,
                text: label.into(),
                color: text_color,
                font_size: l.tile_font,
                font_weight: FontWeightHint::Bold,
                max_width: Some(r.w),
                overflow: TextOverflow::Clip,
            });

            // Recorded last, after everything that draws this tile, so the hit
            // box and the picture are the same rectangle by construction. The
            // paint order runs bottom layer first and `hit_test` reads the
            // boxes back-to-front, so the tile on top of a stack wins the click.
            f.hit(Target::Tile(idx), r);
        }
    }

    /// The key hints along the bottom, on their own strip.
    ///
    /// The strip used to be `height - 24.0` tall by a constant 30, which in a
    /// window shorter than 54 pixels was drawn above its own top edge.
    fn draw_help(&self, f: &mut Frame<Target>, l: &Layout) {
        if l.help.h <= 0.0 {
            return;
        }
        f.push(RenderCommand::FillRect {
            x: l.help.x,
            y: l.help.y,
            width: l.help.w,
            height: l.help.h,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });
        let inner = inset(l.help, l.pad);
        f.push(RenderCommand::Text {
            x: inner.x,
            y: inner.y,
            text: HELP_TEXT.into(),
            color: OVERLAY0,
            font_size: l.small,
            font_weight: FontWeightHint::Regular,
            max_width: Some(inner.w),
            overflow: TextOverflow::Ellipsis,
        });
        f.hit(Target::Help, l.help);
    }

    /// The legend down the right-hand side.
    ///
    /// Its column is solved for in [`Layout::solve`] and is zero-width when the
    /// window is too narrow to carry it, which is the case the old fixed
    /// `legend_x = 730` could not express: it drew the legend off the edge and
    /// left no sign that anything was missing.
    fn draw_legend(&self, f: &mut Frame<Target>, l: &Layout) {
        if l.legend.w <= 0.0 {
            return;
        }
        let inner = inset(l.legend, l.pad);
        if inner.w <= 0.0 || inner.h <= 0.0 {
            return;
        }
        // The heading, the seven groups and the footnote share the column
        // evenly, so the legend cannot run off the bottom of its own band.
        let line = (inner.h / LEGEND_ROWS as f32).min(l.small * 2.0);
        let row = |i: usize| Rect::new(inner.x, inner.y + i as f32 * line, inner.w, line);

        let head = row(0);
        f.push(RenderCommand::Text {
            x: head.x,
            y: head.y,
            text: "Legend".into(),
            color: TEXT_COLOR,
            font_size: l.small,
            font_weight: FontWeightHint::Bold,
            max_width: Some(head.w),
            overflow: TextOverflow::Ellipsis,
        });

        for (i, &(codes, kind)) in LEGEND_ITEMS.iter().enumerate() {
            let r = row(i.saturating_add(1));
            let swatch = l.small * 0.7;
            f.push(RenderCommand::FillRect {
                x: r.x,
                y: r.y + (l.small - swatch) / 2.0,
                width: swatch,
                height: swatch,
                color: kind.text_color(),
                corner_radii: CornerRadii::all(swatch / 4.0),
            });
            let text_x = r.x + swatch + l.pad / 2.0;
            f.push(RenderCommand::Text {
                x: text_x,
                y: r.y,
                text: legend_label(codes, kind),
                color: SUBTEXT0,
                font_size: l.small,
                font_weight: FontWeightHint::Regular,
                max_width: Some((r.right() - text_x).max(0.0)),
                overflow: TextOverflow::Ellipsis,
            });
            f.hit(Target::Legend(i), r);
        }

        // Note about wildcards
        let note = row(LEGEND_ROWS - 1);
        f.push(RenderCommand::Text {
            x: note.x,
            y: note.y,
            text: LEGEND_NOTE.into(),
            color: OVERLAY0,
            font_size: l.small,
            font_weight: FontWeightHint::Regular,
            max_width: Some(note.w),
            overflow: TextOverflow::Ellipsis,
        });
    }
}

// ── What a click can land on ────────────────────────────────────────

/// Everything the drawing pass records a box for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Target {
    /// A live tile, by its index into `Board::tiles`.
    Tile(usize),
    Title,
    /// The counters: tiles left, moves made, tiles currently free.
    Status,
    /// The line that says why the last thing you did did or did not work.
    Message,
    /// One row of the legend, by its index into [`LEGEND_ITEMS`].
    Legend(usize),
    Help,
    /// The board behind the tiles, so a click in its margin is not read as the
    /// tile nearest to it.
    Board,
}

// ── Event dispatch ──────────────────────────────────────────────────

fn handle_event(game: &mut Mahjong, event: &Event) -> EventResult {
    match event {
        Event::Key(ke) => game.handle_key(ke),
        Event::Mouse(me) => game.handle_mouse(me),
        Event::Resize { width, height } => {
            game.resize(f32_from_u32(*width), f32_from_u32(*height));
            EventResult::Consumed
        }
        _ => EventResult::Ignored,
    }
}

/// Widen a window dimension without the lossy-cast lint, and without pretending
/// a `u32` that does not fit an `f32` exactly is an error.
fn f32_from_u32(v: u32) -> f32 {
    // A window dimension is at most a few tens of thousands of pixels, which
    // every `f32` represents exactly; the cast is only "lossy" in the general
    // case the lint has to assume.
    v as f32
}

impl App for Mahjong {
    fn title(&self) -> String {
        "Mahjong Solitaire".to_string()
    }

    fn app_id(&self) -> String {
        "mahjong".to_string()
    }

    fn initial_size(&self) -> (u32, u32) {
        // Converted from the float pair rather than written out again: two
        // spellings of one size are two things that can drift apart.
        (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)
    }

    fn on_event(&mut self, event: &Event) -> Response {
        if matches!(event, Event::CloseRequested) {
            return Response::Exit;
        }
        match handle_event(self, event) {
            EventResult::Consumed => Response::Redraw,
            EventResult::Ignored => Response::Idle,
        }
    }

    fn render(&mut self, width: f32, height: f32) -> RenderTree {
        // The size the frame is drawn at is the size the next click is read
        // against, which is the only reason it is stored at all.
        self.resize(width, height);
        self.frame(width, height).into_tree()
    }
}

impl Probe for Mahjong {
    type Target = Target;
    type Outcome = EventResult;
    const SIZE: (f32, f32) = (WINDOW_WIDTH, WINDOW_HEIGHT);

    fn draw(&self, size: (f32, f32)) -> Frame<Target> {
        self.frame(size.0, size.1)
    }

    fn click_at(&mut self, x: f32, y: f32, button: MouseButton, size: (f32, f32)) -> Self::Outcome {
        self.resize(size.0, size.1);
        handle_event(
            self,
            &Event::Mouse(MouseEvent {
                x,
                y,
                kind: MouseEventKind::Press(button),
            }),
        )
    }

    fn key_at(&mut self, key: &KeyEvent, size: (f32, f32)) -> Self::Outcome {
        self.resize(size.0, size.1);
        handle_event(self, &Event::Key(key.clone()))
    }
}

// ── Entry point ─────────────────────────────────────────────────────

fn main() -> ExitCode {
    let mut game = Mahjong::new();
    app::launch("mahjong", &mut game)
}

// =====================================================================
// Tests
// =====================================================================

#[cfg(test)]
mod tests {
    // A test that indexes past the end, or unwraps a `None`, is a test that
    // has already failed; panicking is the reporting mechanism, not a fault.
    #![allow(
        clippy::arithmetic_side_effects,
        clippy::cast_possible_truncation,
        clippy::cast_precision_loss,
        clippy::cast_sign_loss,
        clippy::expect_used,
        clippy::float_cmp,
        clippy::indexing_slicing,
        clippy::panic,
        clippy::too_many_lines,
        clippy::unwrap_used
    )]

    use super::*;
    use guitk::event::Modifiers;
    use guitk::probe;
    use std::collections::{HashMap, HashSet};

    /// A game whose deal is the same on every run, so a test that names a tile
    /// index names the same tile every time.
    fn game() -> Mahjong {
        Mahjong::with_seed(42)
    }

    fn press_key(g: &mut Mahjong, key: Key) -> EventResult {
        handle_event(
            g,
            &Event::Key(KeyEvent {
                key,
                pressed: true,
                modifiers: Modifiers::NONE,
                text: String::new(),
            }),
        )
    }

    /// Shapes the layout has to survive.
    ///
    /// The last two are not decoration. `(1000, 90)` is short enough that the
    /// legend's nine rows do not fit its column, and `(360, 700)` is narrow
    /// enough that its widest row does not fit a third of the window: those are
    /// the two halves of the legend's fits-test, and without a size that trips
    /// each of them the whole test is unreachable and can say anything.
    const SIZES: [(f32, f32); 6] = [
        (WINDOW_WIDTH, WINDOW_HEIGHT),
        (640.0, 480.0),
        (1600.0, 1000.0),
        (520.0, 900.0),
        (1000.0, 90.0),
        (360.0, 700.0),
    ];

    /// Windows smaller than the layout's own furniture.
    ///
    /// Every font and every padding has a floor, so below roughly 24 pixels of
    /// height the header alone wants more room than the whole window has. Not
    /// one of `SIZES` is that small, and mutation testing found what that cost:
    /// the `.min(h)` that keeps the header inside the window, and the guards
    /// that stop the board solving a negative tile, could all be deleted
    /// without a single test noticing. A compositor hands out sizes like these
    /// during a drag, so they are not hypothetical.
    const SQUEEZED: [(f32, f32); 5] = [
        (200.0, 20.0),
        (20.0, 200.0),
        (30.0, 30.0),
        (1.0, 1.0),
        (0.0, 0.0),
    ];

    // ════════════════════════════════════════════════════════════════
    // The window: every coordinate solved from the live size
    // ════════════════════════════════════════════════════════════════

    #[test]
    fn the_layout_covers_exactly_the_window_it_was_given() {
        // `Layout::window` is what the frame clips to. If it were ever smaller
        // than the window the compositor handed over, the difference would be
        // painted with whatever was in the buffer before.
        for (w, h) in SIZES {
            let l = Layout::solve(w, h);
            assert_eq!((l.window.x, l.window.y), (0.0, 0.0), "at {w}x{h}");
            assert_eq!((l.window.w, l.window.h), (w, h), "at {w}x{h}");
        }
    }

    #[test]
    fn the_three_bands_stack_without_a_gap_or_an_overlap() {
        // Header on top, help along the bottom, everything else in between.
        // A gap would be a stripe of bare background; an overlap would be the
        // help text drawn over the counters.
        for (w, h) in SIZES {
            let l = Layout::solve(w, h);
            assert_eq!(l.header.y, 0.0, "at {w}x{h}");
            assert!(
                l.help.y >= l.header.bottom() - 0.01,
                "at {w}x{h} the help bar at {} starts above the header's bottom {}",
                l.help.y,
                l.header.bottom()
            );
            assert!(
                l.help.bottom() <= h + 0.01,
                "at {w}x{h} the help bar ends at {} past the window",
                l.help.bottom()
            );
        }
    }

    #[test]
    fn an_inset_never_leaves_the_rect_it_came_from() {
        // Checked here rather than through the layout, because through the
        // layout it cannot be checked: a negative height and an origin pushed
        // too far in are the same two pixels with opposite signs, and by the
        // time the turtle is centred in the result they have cancelled. The
        // helper is where the claim is small enough to state.
        for (w, h) in [
            (100.0, 100.0),
            (10.0, 3.0),
            (3.0, 80.0),
            (1.0, 1.0),
            (0.0, 0.0),
        ] {
            let r = Rect::new(5.0, 7.0, w, h);
            let i = inset(r, 4.0);
            assert!(i.w >= 0.0 && i.h >= 0.0, "inset of {r:?} is {i:?}");
            assert!(
                i.x >= r.x - 0.01
                    && i.y >= r.y - 0.01
                    && i.right() <= r.right() + 0.01
                    && i.bottom() <= r.bottom() + 0.01,
                "inset of {r:?} is {i:?}, which is not inside it"
            );
        }
        // And on a rect with room to spare it is a real inset, not a no-op.
        assert_eq!(
            inset(Rect::new(5.0, 7.0, 100.0, 100.0), 4.0),
            Rect::new(9.0, 11.0, 92.0, 92.0)
        );
    }

    #[test]
    fn a_window_smaller_than_its_own_furniture_still_solves_a_sane_layout() {
        // The window is the only thing whose size is not ours to choose, and a
        // drag past the minimum hands us these. Nothing may be negative,
        // nothing may hang outside the window, and the bands must still stack.
        for (w, h) in SQUEEZED {
            let l = Layout::solve(w, h);
            for (name, r) in [
                ("header", l.header),
                ("help", l.help),
                ("legend", l.legend),
                ("board", l.board),
                ("turtle", l.turtle),
            ] {
                assert!(r.w >= 0.0 && r.h >= 0.0, "at {w}x{h} the {name} is {r:?}");
                assert!(
                    r.x >= -0.01 && r.y >= -0.01,
                    "at {w}x{h} the {name} starts outside the window at {r:?}"
                );
                assert!(
                    r.right() <= w + 0.01 && r.bottom() <= h + 0.01,
                    "at {w}x{h} the {name} runs to {}x{} past the window",
                    r.right(),
                    r.bottom()
                );
            }
            assert!(
                l.help.y >= l.header.bottom() - 0.01,
                "at {w}x{h} the help bar at {} is drawn over the header ending at {}",
                l.help.y,
                l.header.bottom()
            );
            assert!(
                l.tile_w >= 0.0 && l.tile_h >= 0.0,
                "at {w}x{h} the tiles are {}x{}",
                l.tile_w,
                l.tile_h
            );
        }
    }

    #[test]
    fn the_board_and_the_legend_divide_the_middle_between_them() {
        // Whatever the legend takes, the board gets the rest -- and neither
        // reaches into the header or the help bar. The old code had the board
        // at a fixed offset and the legend at a fixed x = 730, so on a narrow
        // window they overlapped and on a wide one there was dead space
        // between them that belonged to neither.
        for (w, h) in SIZES {
            let l = Layout::solve(w, h);
            assert!(
                l.board.right() <= l.legend.x + 0.01,
                "at {w}x{h} the board runs to {} and the legend starts at {}",
                l.board.right(),
                l.legend.x
            );
            assert_eq!(
                l.legend.right().min(w),
                w,
                "at {w}x{h} the legend does not reach the right edge"
            );
            assert!(l.board.y >= l.header.bottom() - 0.01, "at {w}x{h}");
            assert!(l.board.bottom() <= l.help.y + 0.01, "at {w}x{h}");
        }
    }

    #[test]
    fn the_header_is_sized_from_the_two_fonts_it_stacks() {
        // Two lines -- the title, then the counters -- plus the padding between
        // and around them. Sized from a share of the height instead, it would
        // have been right at one window height and wrong at every other.
        for (w, h) in SIZES {
            let l = Layout::solve(w, h);
            if l.header.h < h {
                assert_eq!(l.header.h, l.title + l.status + l.pad * 3.0, "at {w}x{h}");
            }
        }
    }

    #[test]
    fn the_header_grows_with_the_font_it_holds() {
        // The claim the equality above cannot make on its own: the header is
        // not merely *equal to* an expression, it *moves* when the fonts move.
        let small = Layout::solve(1000.0, 300.0);
        let large = Layout::solve(1000.0, 1200.0);
        assert!(
            large.title > small.title,
            "the fixture is broken: both windows use a {}pt title",
            small.title
        );
        assert!(
            large.header.h > small.header.h,
            "the header is {} tall at {}pt and {} at {}pt",
            small.header.h,
            small.title,
            large.header.h,
            large.title
        );

        // "It grows" on its own is not the claim the name makes, and mutation
        // testing said so: a header sized at `h * 0.11` also grows with every
        // one of the windows above, because the fonts are themselves derived
        // from `h`. Varying only the input that both the right and the wrong
        // formula depend on cannot tell them apart. What separates them is the
        // clamp: the fonts stop at 26pt and 18pt, and a share of the height
        // never stops. So take two windows tall enough that both fonts and the
        // padding have all hit their ceilings, and require the header to have
        // stopped with them.
        let tall = Layout::solve(1000.0, 1000.0);
        let taller = Layout::solve(1000.0, 1400.0);
        assert_eq!(
            (tall.title, tall.status, tall.pad),
            (taller.title, taller.status, taller.pad),
            "the fixture is broken: the fonts are still growing at 1000 and 1400 tall"
        );
        assert_eq!(
            tall.header.h, taller.header.h,
            "the fonts stopped growing at 1000 tall but the header kept going, \
             so it is sized from the window and not from what it holds"
        );
    }

    #[test]
    fn the_fonts_grow_with_the_window_and_stop_at_both_ends() {
        // A font that grew without a ceiling would swallow a 4K window; one
        // without a floor would be a smudge on a small one.
        let tiny = Layout::solve(300.0, 120.0);
        let huge = Layout::solve(4000.0, 3000.0);
        assert_eq!(tiny.title, 10.0, "the title has no floor");
        assert_eq!(huge.title, 26.0, "the title has no ceiling");
        assert_eq!(tiny.status, 8.0, "the counters have no floor");
        assert_eq!(huge.status, 18.0, "the counters have no ceiling");
        let mid = Layout::solve(1000.0, 700.0);
        assert!(
            mid.title > tiny.title && mid.title < huge.title,
            "the title is pinned to a clamp at every size: {}",
            mid.title
        );
    }

    #[test]
    fn the_small_font_never_outgrows_the_font_above_it() {
        // The legend and the help bar are subordinate to the counters; a small
        // font larger than the status font would read as the more important of
        // the two. The ceiling is `status`, not a constant, so it follows.
        //
        // The extra windows are not decoration. `status` stops at 18pt above
        // 783 pixels of height, and `small` only reaches 18pt at 1058 -- so a
        // ceiling written as a constant instead of as `status` is wrong only
        // in windows taller than that, and the tallest in `SIZES` is 1000. The
        // sweep replaced the ceiling with a constant and no test could see it.
        for (w, h) in SIZES
            .iter()
            .copied()
            .chain([(1400.0, 1400.0), (900.0, 2200.0)])
        {
            let l = Layout::solve(w, h);
            assert!(
                l.small <= l.status,
                "at {w}x{h} the legend is {}pt against {}pt counters",
                l.small,
                l.status
            );
        }
    }

    #[test]
    fn the_padding_is_taken_from_the_shorter_side() {
        // Scaled off the width alone, a wide-and-short window would get
        // padding that ate its whole height. `1400x100` is exactly that shape.
        let wide_short = Layout::solve(1400.0, 100.0);
        let square = Layout::solve(1400.0, 1400.0);
        assert!(
            wide_short.pad < square.pad,
            "a 1400x100 window pads by {} and a 1400x1400 one by {}",
            wide_short.pad,
            square.pad
        );
        assert!(
            wide_short.pad * 2.0 <= 100.0,
            "the padding alone is {} of a 100-pixel window",
            wide_short.pad * 2.0
        );
        // And it stops at both ends, which neither claim above needs: the
        // comparison is satisfied by any padding that grows, including one
        // that grows without limit until a 4K window is mostly margin. The
        // sweep found that hole by deleting the ceiling and going unnoticed.
        assert_eq!(
            Layout::solve(300.0, 120.0).pad,
            2.0,
            "the padding has no floor"
        );
        assert_eq!(
            Layout::solve(4000.0, 3000.0).pad,
            14.0,
            "the padding has no ceiling"
        );
    }
    // ════════════════════════════════════════════════════════════════
    // The legend column: all of it or none of it
    // ════════════════════════════════════════════════════════════════

    #[test]
    fn the_legend_takes_the_width_its_widest_row_measures() {
        // Not a constant: the old code put the legend at x = 730 and let it run
        // to the right edge, so its width was "whatever is left" and the text
        // was ellipsised or lost depending on the window.
        let l = Layout::solve(WINDOW_WIDTH, WINDOW_HEIGHT);
        assert_eq!(
            l.legend.w,
            legend_width(l.small, l.pad),
            "the legend column is not the width its rows need"
        );
    }

    #[test]
    fn the_legend_column_widens_with_the_font_it_is_drawn_at() {
        // The equality above is satisfied by any function of the font,
        // including one that ignores it. This is the claim that it does not:
        // two windows of the same width, differing only in the font the height
        // picks, must not get the same column.
        //
        // "Differing only in the font" has to be arranged, and the first draft
        // did not arrange it: it compared 1400x420 with 1400x1400, where the
        // padding also grows from 6.3 to 14 -- and the column is `widest +
        // font + pad * 3`, so it widened by the padding alone. A mutant that
        // measured the legend at a fixed 12pt passed that comparison. Both
        // windows here are past the padding's ceiling, so the font is the only
        // thing left that differs.
        let short = Layout::solve(1400.0, 1000.0);
        let tall = Layout::solve(1400.0, 1400.0);
        assert_eq!(
            short.pad, tall.pad,
            "the fixture is broken: the padding differs too, so it could be what widens the column"
        );
        assert!(
            tall.small > short.small,
            "the fixture is broken: both draw the legend at {}pt",
            short.small
        );
        assert!(
            tall.legend.w > short.legend.w + 1.0,
            "the legend is {} wide at {}pt and {} at {}pt",
            short.legend.w,
            short.small,
            tall.legend.w,
            tall.small
        );
    }

    #[test]
    fn a_window_too_narrow_for_the_legend_drops_it_rather_than_squeezing_it() {
        // A column narrowed to fit shows "B1-B9 (Bam..." -- unreadable, and
        // still charged to the board's width. 360 wide is under three times
        // what the rows measure.
        let l = Layout::solve(360.0, 700.0);
        assert!(
            legend_width(l.small, l.pad) > 360.0 / 3.0,
            "the fixture is broken: the legend fits a third of a 360-wide window"
        );
        assert_eq!(
            l.legend.w, 0.0,
            "the legend was squeezed instead of dropped"
        );
        assert_eq!(
            l.board.right(),
            360.0,
            "the width the legend gave up did not go to the board"
        );
    }

    #[test]
    fn a_window_too_short_for_the_legends_rows_drops_it_too() {
        // Nine rows at the small font is the least the legend can be drawn in.
        // Below that the rows overlap each other, which the first draft could
        // not express: its fits-test only looked at the width.
        let l = Layout::solve(1000.0, 90.0);
        assert!(
            l.legend.h < l.small * LEGEND_ROWS as f32,
            "the fixture is broken: {} of column holds {} rows of {}pt",
            l.legend.h,
            LEGEND_ROWS,
            l.small
        );
        assert_eq!(l.legend.w, 0.0, "a legend that cannot be read was drawn");
    }

    #[test]
    fn a_window_with_room_for_the_legend_gets_one() {
        // The two tests above are only meaningful if the drop is not
        // unconditional -- a `legend.w = 0.0` would pass both of them.
        let l = Layout::solve(WINDOW_WIDTH, WINDOW_HEIGHT);
        assert!(l.legend.w > 0.0, "the default window has no legend at all");
    }

    #[test]
    fn the_legend_never_takes_more_than_a_third_of_the_window() {
        // The board is the game; the legend is a reference card. Letting it
        // grow past a third would leave the tiles smaller than the label they
        // carry on the very windows where the legend is largest.
        for (w, h) in SIZES {
            let l = Layout::solve(w, h);
            assert!(
                l.legend.w <= w / 3.0 + 0.01,
                "at {w}x{h} the legend takes {} of {w}",
                l.legend.w
            );
        }
    }

    #[test]
    fn every_legend_row_names_its_group_and_marks_the_wild_ones() {
        // The asterisks used to be typed into the legend table by hand, one
        // edit away from promising a matching rule the game does not play.
        // They are asked of the tile now, so this test compares the label
        // against `matches`, not against a second copy of the table.
        for &(codes, kind) in &LEGEND_ITEMS {
            let label = legend_label(codes, kind);
            assert!(
                label.contains(codes),
                "the row for {codes} does not show its codes: {label}"
            );
            assert!(
                label.contains(kind.category()),
                "the row {label} does not name its group"
            );
            let starred = label.contains('*');
            let wild = matches!(kind, TileKind::Season(_) | TileKind::Flower(_));
            assert_eq!(
                starred, wild,
                "the row {label} promises a matching rule the game does not play"
            );
        }
    }

    #[test]
    fn the_legend_lists_every_group_the_deal_contains_exactly_once() {
        // A group left off the legend is a tile the player cannot look up. A
        // group listed twice is a row that says nothing new.
        let listed: Vec<&str> = LEGEND_ITEMS.iter().map(|&(_, k)| k.category()).collect();
        let mut dealt: Vec<&str> = full_tile_set().iter().map(|k| k.category()).collect();
        dealt.sort_unstable();
        dealt.dedup();
        let mut sorted = listed.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(
            sorted.len(),
            listed.len(),
            "a group is listed twice: {listed:?}"
        );
        assert_eq!(sorted, dealt, "the legend and the deal disagree");
    }
    // ════════════════════════════════════════════════════════════════
    // The turtle: a tile size solved for the space, not written down
    // ════════════════════════════════════════════════════════════════

    #[test]
    fn every_tile_lands_inside_the_board_band() {
        // The old code pinned the tiles to `BOARD_OFFSET_X/Y` and a fixed 42x54
        // tile, so any window shorter than about 500 drew the bottom row of the
        // turtle underneath the help bar and any window narrower than 900 drew
        // the right-hand columns under the legend.
        for (w, h) in SIZES {
            let l = Layout::solve(w, h);
            for pos in turtle_layout() {
                let r = l.tile_rect(pos);
                assert!(
                    r.x >= l.board.x - 0.01
                        && r.right() <= l.board.right() + 0.01
                        && r.y >= l.board.y - 0.01
                        && r.bottom() <= l.board.bottom() + 0.01,
                    "at {w}x{h} the tile at {pos:?} is at ({}, {}) {}x{} outside the board \
                     ({}, {}) {}x{}",
                    r.x,
                    r.y,
                    r.w,
                    r.h,
                    l.board.x,
                    l.board.y,
                    l.board.w,
                    l.board.h
                );
            }
        }
    }

    #[test]
    fn the_turtle_box_is_exactly_the_tiles_it_contains() {
        // The bounding box is what the tile size is solved against, so a box
        // larger than the tiles wastes the difference on margin and a box
        // smaller than them puts tiles outside the board. The first draft
        // computed the box from `cols x rows + layers x offset`, which reserved
        // four layer-offsets on the left and top that no tile ever occupies:
        // the upper layers sit at the middle columns, so nothing is ever
        // staggered out past column 0.
        let l = Layout::solve(WINDOW_WIDTH, WINDOW_HEIGHT);
        let mut min_x = f32::INFINITY;
        let mut min_y = f32::INFINITY;
        let mut max_x = f32::NEG_INFINITY;
        let mut max_y = f32::NEG_INFINITY;
        for pos in turtle_layout() {
            let r = l.tile_rect(pos);
            min_x = min_x.min(r.x);
            min_y = min_y.min(r.y);
            max_x = max_x.max(r.right());
            max_y = max_y.max(r.bottom());
        }
        assert!(
            (min_x - l.turtle.x).abs() < 0.01,
            "left edge: {min_x} vs {}",
            l.turtle.x
        );
        assert!(
            (min_y - l.turtle.y).abs() < 0.01,
            "top edge: {min_y} vs {}",
            l.turtle.y
        );
        assert!(
            (max_x - l.turtle.right()).abs() < 0.01,
            "right edge: {max_x} vs {}",
            l.turtle.right()
        );
        assert!(
            (max_y - l.turtle.bottom()).abs() < 0.01,
            "bottom edge: {max_y} vs {}",
            l.turtle.bottom()
        );
    }

    #[test]
    fn the_turtle_is_centred_in_the_space_left_for_it() {
        // A window wider than the turtle needs must not pin it to one side
        // with all the slack on the other.
        for (w, h) in SIZES {
            let l = Layout::solve(w, h);
            if l.tile_w <= 0.0 {
                continue;
            }
            let inner = inset(l.board, l.pad);
            let left = l.turtle.x - inner.x;
            let right = inner.right() - l.turtle.right();
            assert!(
                (left - right).abs() < 0.5,
                "at {w}x{h} the turtle has {left} of slack on the left and {right} on the right"
            );
            let top = l.turtle.y - inner.y;
            let bottom = inner.bottom() - l.turtle.bottom();
            assert!(
                (top - bottom).abs() < 0.5,
                "at {w}x{h} the turtle has {top} of slack above and {bottom} below"
            );
        }
    }

    #[test]
    fn the_turtle_fits_inside_the_board_at_every_size() {
        // The tile is solved from whichever side has *less* room, and that is
        // the whole reason for the `min`. Centring cannot carry this claim --
        // a turtle too big for the board is still centred in it, with equal
        // slack of a negative sign on both sides -- so it is stated here.
        for (w, h) in SIZES.iter().copied().chain(SQUEEZED) {
            let l = Layout::solve(w, h);
            let inner = inset(l.board, l.pad);
            assert!(
                l.turtle.x >= inner.x - 0.01
                    && l.turtle.y >= inner.y - 0.01
                    && l.turtle.right() <= inner.right() + 0.01
                    && l.turtle.bottom() <= inner.bottom() + 0.01,
                "at {w}x{h} the turtle {:?} is outside the padded board {inner:?}",
                l.turtle
            );
        }
    }

    #[test]
    fn the_tile_grows_with_the_window() {
        // The whole point of solving for the size rather than fixing it at
        // 42x54. A constant would satisfy every containment test above.
        let small = Layout::solve(500.0, 400.0);
        let large = Layout::solve(1600.0, 1200.0);
        assert!(
            large.tile_w > small.tile_w * 1.5,
            "a tile is {} wide in a 500x400 window and {} in a 1600x1200 one",
            small.tile_w,
            large.tile_w
        );
    }

    #[test]
    fn a_tile_keeps_its_shape_at_every_size() {
        // Mahjong tiles are taller than they are wide; a tile that squashed to
        // the window's own aspect would be a domino at 1600x400.
        for (w, h) in SIZES {
            let l = Layout::solve(w, h);
            if l.tile_w <= 0.0 {
                continue;
            }
            assert!(
                (l.tile_h / l.tile_w - TILE_ASPECT).abs() < 0.001,
                "at {w}x{h} a tile is {}x{}, an aspect of {}",
                l.tile_w,
                l.tile_h,
                l.tile_h / l.tile_w
            );
        }
    }

    #[test]
    fn the_binding_side_is_whichever_runs_out_first() {
        // A wide, short window is limited by its height and a tall, narrow one
        // by its width. Taking only one of the two would overflow the other.
        let wide = Layout::solve(2000.0, 500.0);
        let inner_wide = inset(wide.board, wide.pad);
        assert!(
            wide.turtle.h <= inner_wide.h + 0.01 && wide.turtle.w < inner_wide.w - 1.0,
            "a 2000x500 window is not limited by its height: turtle {}x{} in {}x{}",
            wide.turtle.w,
            wide.turtle.h,
            inner_wide.w,
            inner_wide.h
        );
        let tall = Layout::solve(500.0, 2000.0);
        let inner_tall = inset(tall.board, tall.pad);
        assert!(
            tall.turtle.w <= inner_tall.w + 0.01 && tall.turtle.h < inner_tall.h - 1.0,
            "a 500x2000 window is not limited by its width: turtle {}x{} in {}x{}",
            tall.turtle.w,
            tall.turtle.h,
            inner_tall.w,
            inner_tall.h
        );
    }

    #[test]
    fn a_higher_layer_sits_up_and_to_the_left_of_the_one_below() {
        // The stagger is the only thing that makes a stack read as a stack.
        // Without it the cap tile would be drawn exactly over the tile it rests
        // on and the turtle would look flat.
        let l = Layout::solve(WINDOW_WIDTH, WINDOW_HEIGHT);
        let below = l.tile_rect(TilePos {
            layer: 0,
            row: 3,
            col: 6,
        });
        let above = l.tile_rect(TilePos {
            layer: 1,
            row: 3,
            col: 6,
        });
        assert!(
            above.x < below.x && above.y < below.y,
            "the tile on layer 1 is at ({}, {}) and the one under it at ({}, {})",
            above.x,
            above.y,
            below.x,
            below.y
        );
        let cap = l.tile_rect(TilePos {
            layer: 4,
            row: 3,
            col: 6,
        });
        assert!(
            (below.x - cap.x - 4.0 * l.tile_w * LAYER_OFFSET_SHARE).abs() < 0.01,
            "four layers of stagger is not four offsets: {} vs {}",
            below.x - cap.x,
            4.0 * l.tile_w * LAYER_OFFSET_SHARE
        );
    }

    #[test]
    fn neighbouring_tiles_on_one_layer_are_separated_by_a_gap() {
        // Without it the turtle is a solid slab and the player cannot see where
        // one tile ends and the next begins.
        let l = Layout::solve(WINDOW_WIDTH, WINDOW_HEIGHT);
        let a = l.tile_rect(TilePos {
            layer: 0,
            row: 0,
            col: 0,
        });
        let b = l.tile_rect(TilePos {
            layer: 0,
            row: 0,
            col: 1,
        });
        let gap = b.x - a.right();
        assert!(
            gap > 0.0,
            "adjacent tiles touch: one ends at {} and the next starts at {}",
            a.right(),
            b.x
        );
        assert!(
            (gap - l.tile_w * TILE_GAP_SHARE).abs() < 0.01,
            "the gap is {gap}, not the {} a tile's width asks for",
            l.tile_w * TILE_GAP_SHARE
        );

        // The rows need the same check and were not getting it. A tile is one
        // unit wide but `TILE_ASPECT` units tall, so a row pitch measured in
        // tile *widths* leaves the rows overlapping by the difference -- and
        // the sweep made exactly that substitution without a test noticing,
        // because every tile this test looked at was on the same row.
        let c = l.tile_rect(TilePos {
            layer: 0,
            row: 1,
            col: 0,
        });
        let row_gap = c.y - a.bottom();
        assert!(
            row_gap > 0.0,
            "the rows overlap: one ends at {} and the next starts at {}",
            a.bottom(),
            c.y
        );
        assert!(
            (row_gap - l.tile_w * TILE_GAP_SHARE).abs() < 0.01,
            "the row gap is {row_gap}, not the {} a tile's width asks for",
            l.tile_w * TILE_GAP_SHARE
        );
    }

    #[test]
    fn the_turtle_holds_the_whole_deal_with_nothing_stacked_on_a_shared_square() {
        // Two tiles at one (layer, row, col) would be drawn on top of each
        // other, and the lower of the two could never be reached by a click.
        let positions = turtle_layout();
        assert_eq!(positions.len(), LAYOUT_SIZE, "the turtle is not 144 tiles");
        let unique: HashSet<TilePos> = positions.iter().copied().collect();
        assert_eq!(
            unique.len(),
            positions.len(),
            "the turtle places two tiles on one square"
        );
    }

    #[test]
    fn the_turtle_is_a_pyramid_narrowing_towards_the_top() {
        // A layer no smaller than the one below it would leave the tiles under
        // it permanently covered, and the deal unwinnable from the first move.
        let positions = turtle_layout();
        let mut per_layer: HashMap<usize, usize> = HashMap::new();
        for pos in &positions {
            *per_layer.entry(pos.layer).or_insert(0) += 1;
        }
        let top = *per_layer.keys().max().expect("the turtle has no layers");
        for layer in 1..=top {
            let above = per_layer.get(&layer).copied().unwrap_or(0);
            let below = per_layer.get(&(layer - 1)).copied().unwrap_or(0);
            assert!(
                above > 0 && above < below,
                "layer {layer} has {above} tiles against {below} on layer {}",
                layer - 1
            );
        }

        // Counting is not narrowing, and the sweep said so: widening layer 2
        // to layer 1's full six columns still leaves it with fewer tiles,
        // because it spans fewer rows -- and a layer as wide as the one below
        // it covers that layer's edge tiles for the whole game. Check the
        // footprint, in both directions, not the population.
        let span = |layer: usize| {
            positions
                .iter()
                .filter(|p| p.layer == layer)
                .fold(None, |acc: Option<(usize, usize, usize, usize)>, p| {
                    Some(match acc {
                        None => (p.col, p.col, p.row, p.row),
                        Some((c0, c1, r0, r1)) => {
                            (c0.min(p.col), c1.max(p.col), r0.min(p.row), r1.max(p.row))
                        }
                    })
                })
                .expect("a layer with no tiles")
        };
        for layer in 1..=top {
            let (ac0, ac1, ar0, ar1) = span(layer);
            let (bc0, bc1, br0, br1) = span(layer - 1);
            assert!(
                ac0 >= bc0 && ac1 <= bc1 && ar0 >= br0 && ar1 <= br1,
                "layer {layer} spans cols {ac0}..={ac1} rows {ar0}..={ar1}, \
                 outside layer {}'s cols {bc0}..={bc1} rows {br0}..={br1}",
                layer - 1
            );
            // Both axes, not either: widening layer 2 to layer 1's full six
            // columns still leaves it two rows shorter, so "narrower in some
            // direction" was satisfied by the mutant that squared it off. The
            // deal nests strictly on both axes at every level -- 14x7, 5x5,
            // 3x3, 1x1, 0x0 -- which is what "nested strictly inside" in
            // `turtle_layout` means, so that is what is asserted.
            assert!(
                ac1 - ac0 < bc1 - bc0 && ar1 - ar0 < br1 - br0,
                "layer {layer} spans {}x{} against layer {}'s {}x{}, so it does not \
                 nest strictly inside it and the tiles under its edges stay covered",
                ac1 - ac0,
                ar1 - ar0,
                layer - 1,
                bc1 - bc0,
                br1 - br0
            );
        }
    }

    #[test]
    fn every_stacked_tile_rests_on_one_that_is_actually_there() {
        // A tile floating over an empty square would be unreachable in the
        // other direction: it covers nothing, so nothing is freed by taking it,
        // and the shape the player sees would not be the shape the rules play.
        let positions: HashSet<TilePos> = turtle_layout().into_iter().collect();
        for pos in &positions {
            if pos.layer == 0 {
                continue;
            }
            let under = TilePos {
                layer: pos.layer - 1,
                row: pos.row,
                col: pos.col,
            };
            assert!(
                positions.contains(&under),
                "the tile at {pos:?} rests on nothing"
            );
        }
    }
    // ════════════════════════════════════════════════════════════════
    // The tiles and the matching rule
    // ════════════════════════════════════════════════════════════════

    #[test]
    fn a_tile_matches_its_own_twin_and_nothing_else_in_its_suit() {
        assert!(TileKind::Bamboo(3).matches(TileKind::Bamboo(3)));
        assert!(!TileKind::Bamboo(3).matches(TileKind::Bamboo(4)));
        assert!(!TileKind::Bamboo(3).matches(TileKind::Circle(3)));
        assert!(TileKind::Wind(2).matches(TileKind::Wind(2)));
        assert!(!TileKind::Wind(2).matches(TileKind::Wind(3)));
        assert!(TileKind::Dragon(0).matches(TileKind::Dragon(0)));
        assert!(!TileKind::Dragon(0).matches(TileKind::Dragon(1)));
    }

    #[test]
    fn any_season_matches_any_other_season_and_the_same_for_flowers() {
        // The four seasons are one tile each, so without the group rule none of
        // them could ever be removed and every deal would end with eight tiles
        // stranded on the board.
        for a in 0..4 {
            for b in 0..4 {
                assert!(
                    TileKind::Season(a).matches(TileKind::Season(b)),
                    "season {a} does not match season {b}"
                );
                assert!(
                    TileKind::Flower(a).matches(TileKind::Flower(b)),
                    "flower {a} does not match flower {b}"
                );
            }
        }
    }

    #[test]
    fn a_season_does_not_match_a_flower() {
        // Both groups are wild, so a rule written as "either is wild" would
        // match them to each other. The rule is "wild, and the same group",
        // which is what the discriminant comparison says.
        for a in 0..4 {
            for b in 0..4 {
                assert!(
                    !TileKind::Season(a).matches(TileKind::Flower(b)),
                    "season {a} matched flower {b}"
                );
                assert!(
                    !TileKind::Flower(a).matches(TileKind::Season(b)),
                    "flower {a} matched season {b}"
                );
            }
        }
    }

    #[test]
    fn a_wild_tile_does_not_match_an_ordinary_one() {
        // The wild branch is only taken when *both* sides are wild; a season
        // against a bamboo falls through to equality, which it fails.
        assert!(!TileKind::Season(0).matches(TileKind::Bamboo(1)));
        assert!(!TileKind::Bamboo(1).matches(TileKind::Season(0)));
        assert!(!TileKind::Flower(2).matches(TileKind::Dragon(2)));
    }

    #[test]
    fn matching_is_symmetric_and_reflexive_across_the_whole_set() {
        // A rule that held one way round would make a pair removable by
        // clicking its two tiles in one order and not the other.
        let kinds = all_tile_kinds();
        for &a in &kinds {
            assert!(a.matches(a), "{a:?} does not match itself");
            for &b in &kinds {
                assert_eq!(
                    a.matches(b),
                    b.matches(a),
                    "{a:?} and {b:?} disagree about each other"
                );
            }
        }
    }

    #[test]
    fn the_wildcard_flag_says_exactly_which_tiles_match_a_stranger() {
        // The legend's asterisk is drawn from `wildcard`, so if the two ever
        // parted company the legend would promise a rule the game does not
        // play. This compares the flag against `matches` itself.
        let kinds = all_tile_kinds();
        for &a in &kinds {
            let matches_a_stranger = kinds.iter().any(|&b| b != a && a.matches(b));
            assert_eq!(
                a.wildcard(),
                matches_a_stranger,
                "{a:?} is marked wildcard={} but {}",
                a.wildcard(),
                if matches_a_stranger {
                    "matches a tile that is not its twin"
                } else {
                    "matches only itself"
                }
            );
        }
    }

    #[test]
    fn every_tile_in_the_deal_carries_a_label_of_its_own() {
        // `label` ends in a `_ => "??"` arm, so a tile added to the set without
        // a label would be drawn as "??" rather than failing to compile.
        let kinds = all_tile_kinds();
        let mut seen: HashMap<&str, TileKind> = HashMap::new();
        for &k in &kinds {
            let label = k.label();
            assert_ne!(label, "??", "{k:?} has no label of its own");
            assert!(!label.is_empty(), "{k:?} has an empty label");
            if let Some(other) = seen.insert(label, k) {
                panic!("{k:?} and {other:?} are both drawn as {label}");
            }
        }
    }

    #[test]
    fn the_groups_are_told_apart_by_colour() {
        // The legend's swatch is this colour, and it is the only thing that
        // distinguishes "W1" (a character) from "W" (the west wind) at a
        // glance. Two groups sharing one colour would make the swatch useless.
        let mut by_colour: HashMap<(u8, u8, u8, u8), &str> = HashMap::new();
        for &(_, kind) in &LEGEND_ITEMS {
            let c = kind.text_color();
            let key = (c.r, c.g, c.b, c.a);
            if let Some(other) = by_colour.insert(key, kind.category()) {
                assert_eq!(
                    other,
                    kind.category(),
                    "{} and {other} are drawn in the same colour",
                    kind.category()
                );
            }
        }
        assert_eq!(
            by_colour.len(),
            LEGEND_ITEMS.len(),
            "the seven groups share fewer than seven colours"
        );
    }

    #[test]
    fn the_deal_is_a_full_mahjong_set_of_a_hundred_and_forty_four() {
        let set = full_tile_set();
        assert_eq!(set.len(), 144, "the deal is not a full set");
        assert_eq!(
            set.len(),
            LAYOUT_SIZE,
            "the deal and the turtle are different sizes, so one of them is truncated"
        );
    }

    #[test]
    fn every_tile_dealt_has_a_partner_it_can_be_removed_with() {
        // A tile with no possible partner is a tile that can never leave the
        // board, so no deal containing one is winnable. Counting the base kinds
        // in fours is the usual way to state this; counting *matchable
        // partners* states it without repeating the construction.
        let set = full_tile_set();
        for (i, &tile) in set.iter().enumerate() {
            let partners = set
                .iter()
                .enumerate()
                .filter(|&(j, &other)| j != i && tile.matches(other))
                .count();
            assert!(
                partners > 0 && partners % 2 == 1,
                "{tile:?} has {partners} possible partners, so it cannot be paired off cleanly"
            );
        }
    }

    #[test]
    fn the_base_kinds_come_four_at_a_time_and_the_bonus_kinds_once() {
        // The traditional set: 34 base kinds x 4, plus one each of four
        // seasons and four flowers.
        let set = full_tile_set();
        let mut counts: HashMap<TileKind, usize> = HashMap::new();
        for &k in &set {
            *counts.entry(k).or_insert(0) += 1;
        }
        for &k in &base_tile_kinds() {
            assert_eq!(
                counts.get(&k).copied(),
                Some(4),
                "{k:?} is not dealt four times"
            );
        }
        for i in 0..4 {
            assert_eq!(
                counts.get(&TileKind::Season(i)).copied(),
                Some(1),
                "season {i} is not unique"
            );
            assert_eq!(
                counts.get(&TileKind::Flower(i)).copied(),
                Some(1),
                "flower {i} is not unique"
            );
        }
        assert_eq!(
            counts.len(),
            all_tile_kinds().len(),
            "a kind was never dealt"
        );
    }

    // ════════════════════════════════════════════════════════════════
    // The board: what is free, what pairs, what is left
    // ════════════════════════════════════════════════════════════════

    /// A board of exactly the tiles named, at the positions named.
    fn board_of(parts: &[(usize, usize, usize, TileKind)]) -> Board {
        let positions: Vec<TilePos> = parts
            .iter()
            .map(|&(layer, row, col, _)| TilePos { layer, row, col })
            .collect();
        let kinds: Vec<TileKind> = parts.iter().map(|&(_, _, _, k)| k).collect();
        Board::from_parts(&positions, &kinds)
    }

    #[test]
    fn a_tile_with_open_air_on_one_side_is_free() {
        let b = board_of(&[
            (0, 0, 0, TileKind::Bamboo(1)),
            (0, 0, 1, TileKind::Bamboo(2)),
            (0, 0, 2, TileKind::Bamboo(3)),
        ]);
        assert!(b.is_free(0), "the leftmost tile of a row is not free");
        assert!(!b.is_free(1), "a tile walled in on both sides is free");
        assert!(b.is_free(2), "the rightmost tile of a row is not free");
    }

    #[test]
    fn a_tile_with_another_on_top_of_it_is_not_free() {
        // Even with both sides open: the tile above has to come off first.
        let b = board_of(&[
            (0, 0, 0, TileKind::Bamboo(1)),
            (1, 0, 0, TileKind::Bamboo(2)),
        ]);
        assert!(!b.is_free(0), "a covered tile is free");
        assert!(b.is_free(1), "the covering tile is not free");
    }

    #[test]
    fn a_tile_is_only_blocked_by_a_neighbour_on_its_own_layer_and_row() {
        // A tile one row down or one layer up is not beside it, however close
        // its column. Getting this wrong walls in tiles that a player can see
        // are open.
        let b = board_of(&[
            (0, 0, 1, TileKind::Bamboo(1)),
            (0, 1, 0, TileKind::Bamboo(2)),
            (0, 1, 2, TileKind::Bamboo(3)),
            (1, 5, 0, TileKind::Bamboo(4)),
            (1, 5, 2, TileKind::Bamboo(5)),
        ]);
        assert!(b.is_free(0), "a tile was blocked by neighbours a row away");
    }

    #[test]
    fn a_removed_tile_blocks_nothing_and_is_itself_not_free() {
        // The whole game is the consequence of this: taking a pair opens up
        // whatever they were holding down.
        let mut b = board_of(&[
            (0, 0, 0, TileKind::Bamboo(1)),
            (0, 0, 1, TileKind::Bamboo(2)),
            (0, 0, 2, TileKind::Bamboo(3)),
        ]);
        assert!(
            !b.is_free(1),
            "the fixture is broken: the middle tile is free"
        );
        b.remove_pair(0, 2);
        assert!(
            b.is_free(1),
            "removing both neighbours did not free the tile"
        );
        assert!(!b.is_free(0), "a removed tile counts as free");
    }

    #[test]
    fn an_index_off_the_end_of_the_board_is_not_free() {
        // A click that arrives with a stale index -- after a new deal, say --
        // must be answered, not indexed with.
        let b = board_of(&[(0, 0, 0, TileKind::Bamboo(1))]);
        assert!(!b.is_free(1));
        assert!(!b.is_free(usize::MAX));
    }

    #[test]
    fn undoing_a_pair_puts_both_tiles_back_where_they_were() {
        let mut b = board_of(&[
            (0, 0, 0, TileKind::Bamboo(1)),
            (0, 0, 2, TileKind::Bamboo(1)),
        ]);
        assert_eq!(b.remaining(), 2);
        b.remove_pair(0, 1);
        assert_eq!(b.remaining(), 0);
        b.restore_pair(0, 1);
        assert_eq!(b.remaining(), 2, "undo did not restore the pair");
    }

    #[test]
    fn a_hint_names_two_free_tiles_that_actually_match() {
        // A hint that named a covered tile, or a pair that does not match,
        // would be worse than no hint: the player would click it and be told
        // the tile is not free.
        let b = game().board;
        let (a, c) = b.find_hint().expect("a fresh deal has no legal move");
        assert_ne!(a, c, "the hint names one tile twice");
        assert!(
            b.is_free(a) && b.is_free(c),
            "the hint names a tile that is not free"
        );
        assert!(
            b.tiles[a].kind.matches(b.tiles[c].kind),
            "the hint names {:?} and {:?}, which do not match",
            b.tiles[a].kind,
            b.tiles[c].kind
        );
    }

    #[test]
    fn a_board_with_no_matching_free_pair_offers_no_hint() {
        // Two free tiles that do not match, and nothing else.
        let b = board_of(&[
            (0, 0, 0, TileKind::Bamboo(1)),
            (0, 0, 2, TileKind::Circle(9)),
        ]);
        assert!(
            b.find_hint().is_none(),
            "a hint was found among tiles that do not match"
        );
        assert!(b.is_lost(), "a board with no move left is not lost");
        assert!(!b.is_won(), "a board with tiles on it is won");
    }

    #[test]
    fn a_matching_pair_that_is_buried_is_not_a_hint() {
        // The pair matches, but the covering tile has to come off first.
        // Offering it would send the player at a tile the rules refuse.
        let b = board_of(&[
            (0, 0, 0, TileKind::Bamboo(1)),
            (0, 0, 1, TileKind::Bamboo(1)),
            (1, 0, 0, TileKind::Circle(2)),
            (1, 0, 1, TileKind::Circle(3)),
        ]);
        assert!(
            b.find_hint().is_none(),
            "a buried pair was offered as a hint"
        );
    }

    #[test]
    fn an_empty_board_is_won_and_not_lost() {
        // `is_lost` asks for a board with tiles left *and* no move; without the
        // first half, the winning board would report both at once.
        let mut b = board_of(&[
            (0, 0, 0, TileKind::Bamboo(1)),
            (0, 0, 2, TileKind::Bamboo(1)),
        ]);
        b.remove_pair(0, 1);
        assert!(b.is_won(), "a cleared board is not won");
        assert!(!b.is_lost(), "a cleared board is also reported lost");
    }

    #[test]
    fn a_shuffle_keeps_every_tile_where_it_is_and_deals_it_a_new_face() {
        // Shuffling the *kinds* and not the positions is what lets a stuck
        // board be re-dealt without the turtle changing shape under the player.
        let mut g = game();
        let before: Vec<TilePos> = g.board.tiles.iter().map(|t| t.pos).collect();
        let faces_before: Vec<TileKind> = g.board.tiles.iter().map(|t| t.kind).collect();
        g.board.shuffle_remaining(&mut g.rng);
        let after: Vec<TilePos> = g.board.tiles.iter().map(|t| t.pos).collect();
        assert_eq!(before, after, "the shuffle moved the tiles");
        let faces_after: Vec<TileKind> = g.board.tiles.iter().map(|t| t.kind).collect();
        assert_ne!(faces_before, faces_after, "the shuffle changed nothing");
        let mut a: Vec<&str> = faces_before.iter().map(|k| k.label()).collect();
        let mut b: Vec<&str> = faces_after.iter().map(|k| k.label()).collect();
        a.sort_unstable();
        b.sort_unstable();
        assert_eq!(a, b, "the shuffle invented or lost a tile");
    }

    #[test]
    fn a_shuffle_leaves_the_removed_tiles_removed() {
        // Redealing the faces of tiles that are off the board would put them
        // back into circulation and break the count of what is left.
        let mut g = game();
        g.board.remove_pair(0, 1);
        let removed_before: Vec<bool> = g.board.tiles.iter().map(|t| t.removed).collect();
        let remaining = g.board.remaining();
        g.board.shuffle_remaining(&mut g.rng);
        let removed_after: Vec<bool> = g.board.tiles.iter().map(|t| t.removed).collect();
        assert_eq!(
            removed_before, removed_after,
            "the shuffle revived a removed tile"
        );
        assert_eq!(g.board.remaining(), remaining);
    }

    #[test]
    fn the_paint_order_runs_bottom_layer_first_and_skips_what_is_gone() {
        // Painted in any other order, a tile on layer 0 would be drawn over the
        // one resting on it, and the stack would read upside down. The hit test
        // reads this list backwards, so the order is also what decides which
        // tile a click on a stack reaches.
        let mut g = game();
        let order = g.board.paint_order();
        assert_eq!(order.len(), g.board.remaining());
        let mut last = 0usize;
        for &i in &order {
            let layer = g.board.tiles[i].pos.layer;
            assert!(
                layer >= last,
                "layer {layer} was painted after layer {last}"
            );
            last = layer;
        }
        let gone = order[0];
        g.board.remove_pair(gone, gone);
        assert!(
            !g.board.paint_order().contains(&gone),
            "a removed tile is still painted"
        );
    }
    // ════════════════════════════════════════════════════════════════
    // The drawing pass: what reaches the screen, and where
    // ════════════════════════════════════════════════════════════════

    /// Every string the frame draws, with the box it was drawn in.
    fn texts(f: &Frame<Target>) -> Vec<(String, f32, f32, f32, Option<f32>)> {
        f.commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text {
                    x,
                    y,
                    text,
                    font_size,
                    max_width,
                    ..
                } => Some((text.clone(), *x, *y, *font_size, *max_width)),
                _ => None,
            })
            .collect()
    }

    /// The one string containing `needle`, or a failure naming what was drawn.
    fn text_saying(f: &Frame<Target>, needle: &str) -> (String, f32, f32, f32, Option<f32>) {
        let all = texts(f);
        let mut hits = all.iter().filter(|(t, ..)| t.contains(needle));
        let first = hits
            .next()
            .unwrap_or_else(|| {
                panic!(
                    "nothing on screen says {needle:?}; what was drawn: {:?}",
                    all.iter().map(|(t, ..)| t.as_str()).collect::<Vec<_>>()
                )
            })
            .clone();
        assert!(
            hits.next().is_none(),
            "{needle:?} is drawn more than once, so a test naming it is ambiguous"
        );
        first
    }

    /// Every filled rectangle in the frame.
    fn fills(f: &Frame<Target>) -> Vec<(Rect, Color)> {
        f.commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::FillRect {
                    x,
                    y,
                    width,
                    height,
                    color,
                    ..
                } => Some((Rect::new(*x, *y, *width, *height), *color)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_frame_paints_a_face_for_every_tile_and_not_just_a_background() {
        // The window's own background fill covers every pixel, so "something is
        // drawn here" is true everywhere and proves nothing on its own. What
        // has to be true is that each live tile got a face and a label of its
        // own -- and a count of *commands* would not say that either, since one
        // tile drawn 144 times would satisfy it.
        let g = game();
        let f = g.draw(Mahjong::SIZE);
        let all = fills(&f);
        let background = all
            .first()
            .copied()
            .expect("the frame paints no background at all");
        assert_eq!(
            (background.0.w, background.0.h),
            Mahjong::SIZE,
            "the background is not the window"
        );
        // A face is a fill the size of a tile that is not the shadow behind
        // one. Selecting them by *colour* does not work and is worth recording
        // why: `TILE_SELECTED` and `TILE_HINT` are the same two hex values as
        // the circle and bamboo swatches in the legend, so a colour filter
        // counts two legend swatches as tiles.
        let l = Layout::solve(Mahjong::SIZE.0, Mahjong::SIZE.1);
        let faces: HashSet<(u32, u32)> = all
            .iter()
            .filter(|(r, c)| *c != TILE_SHADOW && r.w == l.tile_w && r.h == l.tile_h)
            .map(|(r, _)| (r.x.to_bits(), r.y.to_bits()))
            .collect();
        let expected: HashSet<(u32, u32)> = g
            .board
            .tiles
            .iter()
            .filter(|t| !t.removed)
            .map(|t| {
                let r = l.tile_rect(t.pos);
                (r.x.to_bits(), r.y.to_bits())
            })
            .collect();
        assert_eq!(
            faces.len(),
            g.board.remaining(),
            "{} faces were painted for {} live tiles, and no two at one place",
            faces.len(),
            g.board.remaining()
        );
        assert_eq!(faces, expected, "a face was painted somewhere no tile is");
        let labels = all_tile_kinds();
        let drawn = texts(&f)
            .iter()
            .filter(|(t, ..)| labels.iter().any(|k| k.label() == t))
            .count();
        assert_eq!(
            drawn,
            g.board.remaining(),
            "{drawn} labels were drawn for {} live tiles",
            g.board.remaining()
        );
    }

    #[test]
    fn the_frame_is_balanced_and_clips_to_the_window() {
        // An unbalanced clip stack leaks into whatever is drawn next.
        let g = game();
        for (w, h) in SIZES {
            let f = g.draw((w, h));
            assert!(f.is_balanced(), "the frame at {w}x{h} leaves a clip open");
        }
    }

    #[test]
    fn every_live_tile_is_painted_and_given_a_box_a_click_can_find() {
        // The old `main` built this board and dropped it; not one of these
        // rectangles ever existed. A tile with no hit box is a tile the player
        // can see and cannot click.
        let g = game();
        let f = g.draw(Mahjong::SIZE);
        let boxed: HashSet<usize> = f
            .hits()
            .iter()
            .filter_map(|(t, _)| match t {
                Target::Tile(i) => Some(*i),
                _ => None,
            })
            .collect();
        assert_eq!(
            boxed.len(),
            g.board.remaining(),
            "{} of {} live tiles have a hit box",
            boxed.len(),
            g.board.remaining()
        );
        for (i, tile) in g.board.tiles.iter().enumerate() {
            assert_eq!(
                boxed.contains(&i),
                !tile.removed,
                "tile {i} is removed={} and boxed={}",
                tile.removed,
                boxed.contains(&i)
            );
        }
    }

    #[test]
    fn a_removed_tile_is_neither_painted_nor_clickable() {
        // Without this the board would keep answering clicks on tiles the
        // player has already taken.
        let mut g = game();
        let hint = g.board.find_hint().expect("a fresh deal has no move");
        let before = texts(&g.draw(Mahjong::SIZE)).len();
        g.board.remove_pair(hint.0, hint.1);
        let f = g.draw(Mahjong::SIZE);
        assert_eq!(
            texts(&f).len(),
            before - 2,
            "removing two tiles did not remove two labels"
        );
        assert!(
            probe::rect_of_sized(&g, Target::Tile(hint.0), Mahjong::SIZE).is_none(),
            "a removed tile still answers a click"
        );
    }

    #[test]
    fn a_tiles_hit_box_is_the_rectangle_it_was_drawn_in() {
        // The old code computed the picture in `tile_screen_pos` and the hit
        // test in `tile_at_screen` -- two copies of one geometry, kept in step
        // by nothing but care. The box is recorded by the drawing pass now, so
        // this compares it against the layout that placed it.
        let g = game();
        for (w, h) in SIZES {
            let l = Layout::solve(w, h);
            if l.tile_w <= 0.0 {
                continue;
            }
            let f = g.draw((w, h));
            for (target, rect) in f.hits() {
                let Target::Tile(i) = target else { continue };
                let expected = l.tile_rect(g.board.tiles[*i].pos);
                assert!(
                    (rect.x - expected.x).abs() < 0.01
                        && (rect.y - expected.y).abs() < 0.01
                        && (rect.w - expected.w).abs() < 0.01
                        && (rect.h - expected.h).abs() < 0.01,
                    "at {w}x{h} tile {i} is drawn at {expected:?} and clicked at {rect:?}"
                );
            }
        }
    }

    #[test]
    fn a_click_on_a_stack_reaches_the_tile_on_top() {
        // The cap sits inside the footprint of every tile beneath it. A hit
        // test that answered with the first match would hand the click to the
        // bottom of the stack, which the rules then refuse as "not free" --
        // leaving the player unable to take the one tile that is.
        let g = game();
        let l = Layout::solve(WINDOW_WIDTH, WINDOW_HEIGHT);
        let cap = g
            .board
            .tiles
            .iter()
            .position(|t| t.pos.layer == 4)
            .expect("the turtle has no cap");
        let r = l.tile_rect(g.board.tiles[cap].pos);
        let (cx, cy) = r.centre();
        let under = g
            .board
            .tiles
            .iter()
            .enumerate()
            .filter(|&(i, t)| i != cap && t.pos.layer < 4 && l.tile_rect(t.pos).contains(cx, cy))
            .count();
        assert!(
            under > 0,
            "the fixture is broken: the cap covers no other tile"
        );
        assert_eq!(
            g.draw(Mahjong::SIZE).hit_test(cx, cy),
            Some(Target::Tile(cap)),
            "the click went through the cap to one of the {under} tiles under it"
        );
    }

    #[test]
    fn a_tiles_label_is_drawn_inside_the_tile_at_every_size() {
        // The old code drew every label at a fixed 16pt, which was fine only
        // for the fixed 42x54 tile it was designed beside. Once the tile
        // started moving the label spilled over its own edges.
        let g = game();
        for (w, h) in SIZES {
            let l = Layout::solve(w, h);
            if l.tile_w <= 0.0 {
                continue;
            }
            assert!(
                l.tile_font <= l.tile_h * 0.5 + 0.001,
                "at {w}x{h} a {}pt label is drawn in a tile {} tall",
                l.tile_font,
                l.tile_h
            );
            // Measure the labels the frame actually drew, not the ones it
            // could have drawn: a bound checked against `all_tile_kinds()`
            // holds even for a draw pass that puts no label on any tile.
            let drawn: Vec<String> = texts(&g.draw((w, h)))
                .into_iter()
                .filter(|(_, _, _, size, _)| *size == l.tile_font)
                .map(|(s, ..)| s)
                .collect();
            assert!(
                !drawn.is_empty(),
                "at {w}x{h} the tiles were painted with no labels on them"
            );
            for label in &drawn {
                let width = text::measure(label, l.tile_font, FontWeightHint::Bold);
                assert!(
                    width <= l.tile_w + 0.01,
                    "at {w}x{h} the label {label:?} measures {width} across a {} tile",
                    l.tile_w
                );
            }
        }
    }

    #[test]
    fn the_label_font_grows_with_the_tile_it_is_drawn_on() {
        // The bound above is one-sided: a label of 1pt satisfies it at every
        // size. This is the claim the fit is a *measurement* and not a floor.
        let small = Layout::solve(500.0, 400.0);
        let large = Layout::solve(1600.0, 1200.0);
        assert!(
            large.tile_font > small.tile_font * 1.5,
            "a label is {}pt on a {} tile and {}pt on a {} one",
            small.tile_font,
            small.tile_w,
            large.tile_font,
            large.tile_w
        );
    }

    #[test]
    fn a_selected_tile_is_painted_differently_from_an_unselected_one() {
        // Without it the player has no way to see which tile they picked, and
        // a mis-click is indistinguishable from a click that did nothing.
        let mut g = game();
        let (a, _) = g.board.find_hint().expect("a fresh deal has no move");
        let before = fills(&g.draw(Mahjong::SIZE));
        g.selected = Some(a);
        let after = fills(&g.draw(Mahjong::SIZE));
        let changed = before
            .iter()
            .zip(after.iter())
            .filter(|((_, c1), (_, c2))| c1 != c2)
            .count();
        assert_eq!(
            changed, 1,
            "selecting one tile repainted {changed} rectangles"
        );
    }

    #[test]
    fn a_hinted_pair_is_painted_differently_from_the_rest() {
        let mut g = game();
        let before = fills(&g.draw(Mahjong::SIZE));
        assert!(g.show_hint_pair(), "the hint key did nothing");
        let after = fills(&g.draw(Mahjong::SIZE));
        let changed = before
            .iter()
            .zip(after.iter())
            .filter(|((_, c1), (_, c2))| c1 != c2)
            .count();
        assert_eq!(
            changed, 2,
            "a hint of two tiles repainted {changed} of them"
        );
    }

    #[test]
    fn the_cursor_is_drawn_as_a_border_thick_enough_to_see_at_any_size() {
        // At the old fixed two pixels the border vanished on a large window and
        // swallowed the tile on a small one.
        let g = game();
        for (w, h) in SIZES {
            let l = Layout::solve(w, h);
            if l.tile_w <= 0.0 {
                continue;
            }
            let f = g.draw((w, h));
            let widths: Vec<f32> = f
                .commands()
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::Line { width, .. } => Some(*width),
                    _ => None,
                })
                .collect();
            assert_eq!(
                widths.len(),
                4,
                "at {w}x{h} the cursor is drawn with {} edges, not four",
                widths.len()
            );
            for bw in widths {
                assert!(
                    (1.0..=4.0).contains(&bw),
                    "at {w}x{h} the cursor border is {bw} pixels"
                );
                assert!(
                    bw < l.tile_w / 2.0,
                    "at {w}x{h} a {bw}-pixel border swallows a {} tile",
                    l.tile_w
                );
            }
        }
    }

    #[test]
    fn the_header_reports_what_is_left_what_was_played_and_what_can_be_played() {
        let g = game();
        let f = g.draw(Mahjong::SIZE);
        let (line, ..) = text_saying(&f, "Tiles:");
        assert!(
            line.contains(&format!("Tiles: {}", g.board.remaining())),
            "the header says {line:?} of a board with {} tiles",
            g.board.remaining()
        );
        assert!(
            line.contains("Moves: 0"),
            "a fresh game has been played: {line:?}"
        );
        assert!(
            line.contains(&format!("Free: {}", g.board.free_tiles().len())),
            "the header says {line:?} of a board with {} free tiles",
            g.board.free_tiles().len()
        );
    }

    #[test]
    fn the_counters_follow_the_board_rather_than_being_written_once() {
        let mut g = game();
        let before = g.board.remaining();
        let (a, b) = g.board.find_hint().expect("a fresh deal has no move");
        g.cursor.tile_idx = Some(a);
        assert!(g.try_select(a));
        assert!(g.try_select(b));
        let f = g.draw(Mahjong::SIZE);
        let (line, ..) = text_saying(&f, "Tiles:");
        assert!(
            line.contains(&format!("Tiles: {}", before - 2)),
            "after taking a pair the header still says {line:?}"
        );
        assert!(
            line.contains("Moves: 1"),
            "the move was not counted: {line:?}"
        );
    }

    #[test]
    fn the_title_and_the_key_hints_are_on_screen() {
        let g = game();
        let f = g.draw(Mahjong::SIZE);
        text_saying(&f, "Mahjong Solitaire");
        let (help, ..) = text_saying(&f, "N=New");
        for key in ["Z=Undo", "H=Hint", "S=Shuffle", "Esc=Deselect"] {
            assert!(
                help.contains(key),
                "the help bar does not mention {key}: {help:?}"
            );
        }
    }

    #[test]
    fn a_message_is_drawn_only_when_there_is_one_to_draw() {
        // An empty message box would still take a hit box, so a click on the
        // right half of the header would be answered as "the message".
        let mut g = game();
        assert!(
            g.message.is_none(),
            "a fresh game already has something to say"
        );
        assert!(
            probe::rect_of_sized(&g, Target::Message, Mahjong::SIZE).is_none(),
            "a box was recorded for a message that does not exist"
        );
        g.message = Some("Tile is not free");
        let f = g.draw(Mahjong::SIZE);
        text_saying(&f, "Tile is not free");
        assert!(
            probe::rect_of_sized(&g, Target::Message, Mahjong::SIZE).is_some(),
            "the message is drawn but cannot be found"
        );
    }

    #[test]
    fn the_counters_and_the_message_never_share_a_pixel() {
        // They used to be at fixed x offsets 40 and `BOARD_OFFSET_X + 400`, so
        // a long status line ran straight into the message.
        let mut g = game();
        g.message = Some("No moves left! S=shuffle, N=new");
        for (w, h) in SIZES {
            let status = probe::rect_of_sized(&g, Target::Status, (w, h));
            let message = probe::rect_of_sized(&g, Target::Message, (w, h));
            let (Some(s), Some(m)) = (status, message) else {
                continue;
            };
            assert!(
                s.intersect(m).is_none(),
                "at {w}x{h} the counters at {s:?} overlap the message at {m:?}"
            );
        }
    }

    #[test]
    fn every_line_of_text_is_told_how_wide_its_box_is() {
        // Without `max_width` the renderer has no licence to elide, so a string
        // longer than its box is drawn straight over its neighbour. This is the
        // fault the header had at a fixed offset, generalised.
        let mut g = game();
        g.message = Some("No moves left! S=shuffle, N=new");
        for (w, h) in SIZES {
            let f = g.draw((w, h));
            for (text, x, _, _, max) in texts(&f) {
                let max = max.unwrap_or_else(|| panic!("at {w}x{h} {text:?} has no width limit"));
                assert!(
                    max >= 0.0,
                    "at {w}x{h} {text:?} is given a negative width of {max}"
                );
                assert!(
                    x + max <= w + 0.01,
                    "at {w}x{h} {text:?} starts at {x} and is allowed {max}, past the window"
                );
            }
        }
    }

    #[test]
    fn the_legend_draws_one_row_for_each_group_and_a_note_for_the_asterisks() {
        let g = game();
        let f = g.draw(Mahjong::SIZE);
        text_saying(&f, "Legend");
        text_saying(&f, LEGEND_NOTE);
        for (i, &(codes, kind)) in LEGEND_ITEMS.iter().enumerate() {
            let (row, ..) = text_saying(&f, codes);
            assert_eq!(row, legend_label(codes, kind));
            assert!(
                probe::rect_of_sized(&g, Target::Legend(i), Mahjong::SIZE).is_some(),
                "legend row {i} was drawn without a box"
            );
        }
    }

    #[test]
    fn a_dropped_legend_draws_nothing_and_records_nothing() {
        // Half a legend -- a heading with no rows, or boxes with no text -- is
        // worse than none, because a click would land on a row that is not there.
        let g = game();
        let size = (360.0, 700.0);
        assert_eq!(
            Layout::solve(size.0, size.1).legend.w,
            0.0,
            "the fixture is broken"
        );
        let f = g.draw(size);
        for (text, ..) in texts(&f) {
            assert!(
                !text.contains("Legend") && !text.contains(LEGEND_NOTE),
                "a dropped legend still drew {text:?}"
            );
        }
        for i in 0..LEGEND_ITEMS.len() {
            assert!(
                probe::rect_of_sized(&g, Target::Legend(i), size).is_none(),
                "a dropped legend still records a box for row {i}"
            );
        }
    }

    #[test]
    fn every_legend_row_stays_inside_the_legend_column() {
        // Nine rows sharing the column evenly, so the note cannot run off the
        // bottom however short the window gets.
        let g = game();
        for (w, h) in SIZES {
            let l = Layout::solve(w, h);
            if l.legend.w <= 0.0 {
                continue;
            }
            for i in 0..LEGEND_ITEMS.len() {
                let Some(r) = probe::rect_of_sized(&g, Target::Legend(i), (w, h)) else {
                    panic!("at {w}x{h} legend row {i} is missing");
                };
                assert!(
                    r.y >= l.legend.y - 0.01 && r.bottom() <= l.legend.bottom() + 0.01,
                    "at {w}x{h} legend row {i} runs from {} to {} in a column {} to {}",
                    r.y,
                    r.bottom(),
                    l.legend.y,
                    l.legend.bottom()
                );
            }
        }
    }

    #[test]
    fn the_help_bar_is_painted_before_its_text_and_covers_the_bottom_strip() {
        // The strip used to be `height - 24.0` tall by a constant 30, which in
        // a window shorter than 54 pixels was drawn above its own top edge.
        for (w, h) in SIZES {
            let l = Layout::solve(w, h);
            if l.help.h <= 0.0 {
                continue;
            }
            let g = game();
            let f = g.draw((w, h));
            let strip = fills(&f)
                .into_iter()
                .find(|&(r, c)| c == MANTLE && r.w == w)
                .unwrap_or_else(|| panic!("at {w}x{h} the help bar has no background"));
            assert!(
                strip.0.y >= l.header.bottom() - 0.01,
                "at {w}x{h} the help bar at {} is drawn over the header",
                strip.0.y
            );
            assert!(
                (strip.0.bottom() - h).abs() < 0.01,
                "at {w}x{h} the help bar ends at {} rather than the bottom",
                strip.0.bottom()
            );
            // And it is as tall as the line it holds. Both claims above are
            // about where the strip is, and a strip of any height can be in
            // the right place: the sweep pinned the bar at a constant 30 and
            // the only test that noticed was the one about tile labels, which
            // saw it through the board it left behind rather than as a bar
            // whose text no longer fits.
            assert!(
                (strip.0.h - (l.small + l.pad * 2.0)).abs() < 0.01,
                "at {w}x{h} the help bar is {} tall around a {}pt line padded by {}",
                strip.0.h,
                l.small,
                l.pad
            );
        }
    }

    #[test]
    fn a_window_with_no_room_for_a_tile_draws_no_tiles_rather_than_zero_sized_ones() {
        // A zero-width tile is a hit box every click lands in at once, and a
        // divide by it is how the cursor arithmetic would produce infinities.
        let g = game();
        // Four pixels square: the padding alone consumes the whole window, so
        // the board's inner box has no width at all.
        let size = (4.0, 4.0);
        let l = Layout::solve(size.0, size.1);
        assert_eq!(
            l.tile_w, 0.0,
            "the fixture is broken: a 4x4 window fits a tile"
        );
        let f = g.draw(size);
        assert!(
            !f.hits().iter().any(|(t, _)| matches!(t, Target::Tile(_))),
            "a window with no room for a tile still recorded tile boxes"
        );
        // The hit boxes alone cannot carry this claim, and the sweep proved
        // it: deleting the guard in `draw_board` outright left this test
        // green, because `Frame::hit` drops an empty rect on its own -- so the
        // absence of a box is evidence about the toolkit, not about us. The
        // 144 zero-sized fills and 144 labels the guard exists to suppress
        // would all still have been pushed. Ask about the painting instead.
        assert!(
            !fills(&f).iter().any(|&(_, c)| c == TILE_BG
                || c == TILE_BG_FREE
                || c == TILE_SELECTED
                || c == TILE_HINT
                || c == TILE_SHADOW),
            "a window with no room for a tile still painted tiles"
        );
    }
    // ════════════════════════════════════════════════════════════════
    // Clicks: aimed by name at the box the drawing pass recorded
    // ════════════════════════════════════════════════════════════════

    #[test]
    fn clicking_a_free_tile_selects_it() {
        // The whole feature that did not exist: `main` dropped the board, so
        // this click had nowhere to arrive.
        let mut g = game();
        let (a, _) = g.board.find_hint().expect("a fresh deal has no move");
        assert_eq!(
            probe::click(&mut g, Target::Tile(a)),
            EventResult::Consumed,
            "a click on a free tile was ignored"
        );
        assert_eq!(g.selected, Some(a), "the click selected {:?}", g.selected);
    }

    #[test]
    fn clicking_a_matching_pair_takes_both_off_the_board() {
        let mut g = game();
        let (a, b) = g.board.find_hint().expect("a fresh deal has no move");
        let before = g.board.remaining();
        probe::click(&mut g, Target::Tile(a));
        probe::click(&mut g, Target::Tile(b));
        assert_eq!(
            g.board.remaining(),
            before - 2,
            "the pair is still on the board"
        );
        assert_eq!(
            g.selected, None,
            "a tile is still selected after the pair went"
        );
        assert_eq!(g.moves, 1, "the move was not counted");
        assert_eq!(g.undo_stack.len(), 1, "the move cannot be undone");
    }

    #[test]
    fn clicking_two_tiles_that_do_not_match_selects_the_second_and_says_so() {
        // Not "deselect both": the player almost always meant the second tile
        // as the start of a new attempt, and clearing it would cost them a
        // click every time they guessed wrong.
        let mut g = game();
        let free = g.board.free_tiles();
        let (a, b) = free
            .iter()
            .flat_map(|&i| free.iter().map(move |&j| (i, j)))
            .find(|&(i, j)| i != j && !g.board.tiles[i].kind.matches(g.board.tiles[j].kind))
            .expect("every free tile matches every other");
        probe::click(&mut g, Target::Tile(a));
        probe::click(&mut g, Target::Tile(b));
        assert_eq!(
            g.selected,
            Some(b),
            "the second tile of a bad guess is not selected"
        );
        assert_eq!(g.message, Some("Tiles don't match!"));
        assert_eq!(g.moves, 0, "a failed match was counted as a move");
    }

    #[test]
    fn clicking_the_selected_tile_again_puts_it_back() {
        let mut g = game();
        let (a, _) = g.board.find_hint().expect("a fresh deal has no move");
        probe::click(&mut g, Target::Tile(a));
        probe::click(&mut g, Target::Tile(a));
        assert_eq!(
            g.selected, None,
            "a tile cannot be deselected by clicking it"
        );
        assert_eq!(g.board.remaining(), 144, "a tile matched itself");
    }

    #[test]
    fn clicking_a_hemmed_in_tile_says_why_nothing_happened() {
        // Silence here is the worst answer: the player clicks, nothing moves,
        // and there is no way to learn that the tile is pinned rather than that
        // the click missed.
        //
        // "Not free" covers two different situations and only one of them is
        // reachable by clicking. A tile with neighbours on both sides is in
        // plain sight and takes its own click -- that is this test. A tile
        // *underneath* another one cannot be clicked at all, because the tile
        // on top is painted over it, which is what
        // `a_click_on_a_stack_reaches_the_tile_on_top` checks. The first draft
        // confused the two, took the first not-free tile it found (which is
        // hemmed in, not buried) and then asserted that something else took
        // the click.
        let mut g = game();
        let l = Layout::solve(WINDOW_WIDTH, WINDOW_HEIGHT);
        let hemmed = (0..g.board.tiles.len())
            .find(|&i| {
                let r = l.tile_rect(g.board.tiles[i].pos);
                !g.board.is_free(i)
                    && !g.board.tiles.iter().any(|t| {
                        t.pos.layer > g.board.tiles[i].pos.layer
                            && l.tile_rect(t.pos).contains(r.centre().0, r.centre().1)
                    })
            })
            .expect("every tile on a fresh deal is either free or buried");
        assert_eq!(
            probe::click(&mut g, Target::Tile(hemmed)),
            EventResult::Consumed,
            "the refusal was not worth repainting"
        );
        assert_eq!(g.message, Some("Tile is not free"));
        assert_eq!(g.selected, None, "a blocked tile was selected");
        // `try_select` answers "does the screen need repainting", not "was the
        // tile selected", so the *second* identical refusal is the one that
        // reports no change.
        assert!(
            !g.try_select(hemmed),
            "the same refusal twice asked for a second repaint"
        );
    }

    #[test]
    fn clicking_the_furniture_is_not_a_move() {
        // The title, the counters and the help bar record boxes so that a click
        // there is answered rather than falling through to the nearest tile.
        //
        // `Target::Board` is deliberately not in this list, and the reason is
        // worth keeping: the board's box spans the whole playing area and the
        // tiles are painted *over* it, so its centre belongs to a tile and
        // clicking "the board" by its rectangle's midpoint plays a tile
        // instead. Only its margin is its own; that is what the next case
        // checks.
        for target in [Target::Title, Target::Status, Target::Help] {
            let mut g = game();
            let before = g.board.remaining();
            assert_eq!(
                probe::click(&mut g, target),
                EventResult::Ignored,
                "a click on {target:?} was consumed"
            );
            assert_eq!(g.selected, None, "a click on {target:?} selected a tile");
            assert_eq!(
                g.board.remaining(),
                before,
                "a click on {target:?} took a pair"
            );
            assert_eq!(g.moves, 0, "a click on {target:?} counted as a move");
        }
    }

    #[test]
    fn clicking_the_boards_margin_lands_on_the_board_and_not_on_a_tile() {
        // The turtle is centred in the board with room to spare, so the board's
        // top-left corner is bare felt. A click there must be answered by the
        // board -- if it fell through to a tile the player would be playing
        // tiles by clicking empty space near them.
        let mut g = game();
        let l = g.layout();
        let (x, y) = (l.board.x + 1.0, l.board.y + 1.0);
        let f = g.draw(Mahjong::SIZE);
        assert_eq!(
            f.hit_test(x, y),
            Some(Target::Board),
            "the board's own corner is claimed by something else"
        );
        assert_eq!(
            g.click_at(x, y, MouseButton::Left, Mahjong::SIZE),
            EventResult::Ignored,
            "a click on bare felt was consumed"
        );
        assert_eq!(g.selected, None, "bare felt selected a tile");
        assert_eq!(g.board.remaining(), 144, "bare felt took a pair");
    }

    #[test]
    fn clicking_a_legend_row_is_not_a_move_either() {
        let mut g = game();
        for i in 0..LEGEND_ITEMS.len() {
            assert_eq!(
                probe::click(&mut g, Target::Legend(i)),
                EventResult::Ignored,
                "a click on legend row {i} was consumed"
            );
        }
        assert_eq!(g.board.remaining(), 144);
        assert_eq!(g.selected, None);
    }

    #[test]
    fn a_click_outside_every_box_does_nothing() {
        let mut g = game();
        assert_eq!(
            g.click_at(-5.0, -5.0, MouseButton::Left, Mahjong::SIZE),
            EventResult::Ignored
        );
        assert_eq!(
            g.click_at(
                WINDOW_WIDTH + 10.0,
                WINDOW_HEIGHT + 10.0,
                MouseButton::Left,
                Mahjong::SIZE
            ),
            EventResult::Ignored
        );
        assert_eq!(g.selected, None);
    }

    #[test]
    fn only_the_left_button_plays_a_tile() {
        // Answering all three meant a right-click removed a pair, which is a
        // move the player did not make.
        let mut g = game();
        let (a, _) = g.board.find_hint().expect("a fresh deal has no move");
        for button in [MouseButton::Right, MouseButton::Middle] {
            assert_eq!(
                probe::click_with(&mut g, Target::Tile(a), button),
                EventResult::Ignored,
                "the {button:?} button played a tile"
            );
            assert_eq!(g.selected, None);
        }
        assert_eq!(probe::click(&mut g, Target::Tile(a)), EventResult::Consumed);
    }

    #[test]
    fn a_click_moves_the_keyboard_cursor_to_the_tile_it_landed_on() {
        // Otherwise the next arrow press would jump back to wherever the
        // cursor had been left, which reads as the cursor teleporting.
        let mut g = game();
        let free = g.board.free_tiles();
        let target = *free
            .iter()
            .find(|&&i| Some(i) != g.cursor.tile_idx)
            .expect("there is only one free tile");
        probe::click(&mut g, Target::Tile(target));
        assert_eq!(g.cursor.tile_idx, Some(target));
    }

    #[test]
    fn a_click_is_read_against_the_size_the_frame_was_drawn_at() {
        // A click is a pair of window coordinates and means nothing without the
        // layout it was aimed at. Read against the *default* size, a click near
        // the edge of a large window would land on a different tile.
        let big = (1600.0, 1200.0);
        let g = game();
        let l = Layout::solve(big.0, big.1);
        let (a, _) = g.board.find_hint().expect("a fresh deal has no move");
        let (cx, cy) = l.tile_rect(g.board.tiles[a].pos).centre();
        assert!(
            cx > WINDOW_WIDTH
                || cy > WINDOW_HEIGHT
                || l.tile_w > 1.5 * Layout::solve(WINDOW_WIDTH, WINDOW_HEIGHT).tile_w,
            "the fixture is broken: the large window is not meaningfully different"
        );
        let mut g = game();
        assert_eq!(
            g.click_at(cx, cy, MouseButton::Left, big),
            EventResult::Consumed
        );
        assert_eq!(
            g.selected,
            Some(a),
            "the click was read against the wrong size"
        );
    }

    // ════════════════════════════════════════════════════════════════
    // Keys
    // ════════════════════════════════════════════════════════════════

    #[test]
    fn n_deals_a_new_game_from_a_new_seed() {
        let mut g = game();
        let (a, b) = g.board.find_hint().expect("a fresh deal has no move");
        probe::click(&mut g, Target::Tile(a));
        probe::click(&mut g, Target::Tile(b));
        let seed = g.seed;
        let faces: Vec<TileKind> = g.board.tiles.iter().map(|t| t.kind).collect();
        assert_eq!(press_key(&mut g, Key::N), EventResult::Consumed);
        assert_ne!(g.seed, seed, "the new game reused the old seed");
        assert_eq!(g.board.remaining(), 144, "the new deal is short of tiles");
        assert_eq!(g.moves, 0, "the move count survived the new deal");
        assert!(g.undo_stack.is_empty(), "the old game can still be undone");
        assert_eq!(g.status, GameStatus::Playing);
        assert_ne!(
            g.board.tiles.iter().map(|t| t.kind).collect::<Vec<_>>(),
            faces,
            "the new deal is the old deal"
        );
    }

    #[test]
    fn z_undoes_the_last_pair_and_says_so_when_there_is_none() {
        let mut g = game();
        let (a, b) = g.board.find_hint().expect("a fresh deal has no move");
        probe::click(&mut g, Target::Tile(a));
        probe::click(&mut g, Target::Tile(b));
        assert_eq!(press_key(&mut g, Key::Z), EventResult::Consumed);
        assert_eq!(g.board.remaining(), 144, "the pair did not come back");
        assert_eq!(g.moves, 0, "the move count did not come back down");
        assert_eq!(g.message, Some("Undo!"));
        // A second undo has nothing to undo, and must say so rather than
        // running the counter below zero.
        assert_eq!(press_key(&mut g, Key::Z), EventResult::Consumed);
        assert_eq!(g.message, Some("Nothing to undo"));
        assert_eq!(g.moves, 0);
        // ...and repeating it changes nothing, so the window is not asked to
        // redraw an identical frame.
        assert_eq!(press_key(&mut g, Key::Z), EventResult::Ignored);
    }

    #[test]
    fn h_shows_a_hint_and_the_hint_is_a_pair_the_rules_accept() {
        let mut g = game();
        assert_eq!(press_key(&mut g, Key::H), EventResult::Consumed);
        assert!(g.show_hint, "the hint was found but not shown");
        let (a, b) = g.hint.expect("the hint names no pair");
        assert!(g.board.is_free(a) && g.board.is_free(b));
        assert!(g.board.tiles[a].kind.matches(g.board.tiles[b].kind));
        assert_eq!(g.message, Some("Hint shown (green tiles)"));
    }

    #[test]
    fn h_says_so_when_there_is_nothing_to_hint() {
        let mut g = game();
        g.board = board_of(&[
            (0, 0, 0, TileKind::Bamboo(1)),
            (0, 0, 2, TileKind::Circle(9)),
        ]);
        assert_eq!(press_key(&mut g, Key::H), EventResult::Consumed);
        assert!(!g.show_hint, "a hint was shown for a board that has none");
        assert_eq!(g.message, Some("No valid pairs!"));
    }

    #[test]
    fn s_shuffles_a_stuck_board_back_into_play() {
        // The guard names `Won` rather than `!= Playing`, because a *lost*
        // board is exactly the one this key is for.
        let mut g = game();
        g.status = GameStatus::Lost;
        assert_eq!(press_key(&mut g, Key::S), EventResult::Consumed);
        assert_eq!(
            g.status,
            GameStatus::Playing,
            "a shuffled board is still reported lost"
        );
        assert_eq!(g.message, Some("Tiles shuffled!"));
    }

    #[test]
    fn s_does_nothing_to_a_board_that_has_already_been_cleared() {
        let mut g = game();
        g.status = GameStatus::Won;
        assert_eq!(press_key(&mut g, Key::S), EventResult::Ignored);
        assert_eq!(
            g.status,
            GameStatus::Won,
            "a won game was put back into play"
        );
    }

    #[test]
    fn the_arrows_walk_the_cursor_between_free_tiles() {
        let mut g = game();
        let start = g.cursor.tile_idx.expect("a fresh deal has no cursor");
        assert_eq!(press_key(&mut g, Key::Right), EventResult::Consumed);
        let moved = g.cursor.tile_idx.expect("the cursor vanished");
        assert_ne!(moved, start, "the right arrow did not move the cursor");
        assert!(
            g.board.is_free(moved),
            "the cursor landed on a covered tile"
        );
        let l = g.layout();
        assert!(
            l.tile_rect(g.board.tiles[moved].pos).x > l.tile_rect(g.board.tiles[start].pos).x,
            "the right arrow moved the cursor left"
        );
        assert_eq!(press_key(&mut g, Key::Left), EventResult::Consumed);
        // Left is not "undo right", and asserting that it was is how this test
        // first failed. The cursor walks the *free* tiles, so from tile 0 the
        // nearest playable tile rightwards is the layer-1 tile at (1,4) --
        // tile 1 is buried between its neighbours and is skipped -- and the
        // nearest playable tile leftwards of *that* is a third tile, (0,2,3),
        // not the one we came from. Only the direction is promised.
        let back = g.cursor.tile_idx.expect("the cursor vanished going back");
        assert!(g.board.is_free(back), "the cursor landed on a covered tile");
        assert!(
            l.tile_rect(g.board.tiles[back].pos).x < l.tile_rect(g.board.tiles[moved].pos).x,
            "the left arrow moved the cursor right"
        );
    }

    #[test]
    fn the_arrows_measure_in_tile_widths_so_the_same_press_picks_the_same_tile() {
        // Measured in pixels the weights would have been right at one window
        // size and silently wrong at every other.
        let mut small = game();
        let mut large = game();
        for key in [Key::Right, Key::Down, Key::Right, Key::Up] {
            small.resize(700.0, 500.0);
            press_key(&mut small, key);
            large.resize(1900.0, 1300.0);
            press_key(&mut large, key);
        }
        assert_eq!(
            small.cursor.tile_idx, large.cursor.tile_idx,
            "four arrow presses land on different tiles in different windows"
        );
    }

    #[test]
    fn an_arrow_at_the_edge_of_the_board_reports_that_nothing_changed() {
        // Otherwise a key held against the edge repaints an identical frame
        // for as long as it is held.
        let mut g = game();
        for _ in 0..40 {
            press_key(&mut g, Key::Left);
        }
        let settled = g.cursor.tile_idx;
        assert_eq!(
            press_key(&mut g, Key::Left),
            EventResult::Ignored,
            "the cursor is still moving after forty presses"
        );
        assert_eq!(g.cursor.tile_idx, settled);
    }

    #[test]
    fn a_window_too_small_to_draw_a_tile_leaves_the_cursor_where_it_is() {
        // Every tile is at the same point when the tile has no width, so
        // "the closest one in this direction" has no answer; jumping to an
        // arbitrary tile would be worse than sitting still.
        let mut g = game();
        let before = g.cursor.tile_idx;
        g.resize(4.0, 4.0);
        assert_eq!(g.layout().tile_w, 0.0, "the fixture is broken");
        assert_eq!(press_key(&mut g, Key::Right), EventResult::Ignored);
        assert_eq!(g.cursor.tile_idx, before);
    }

    #[test]
    fn enter_and_space_play_the_tile_under_the_cursor() {
        for key in [Key::Enter, Key::Space] {
            let mut g = game();
            let (a, b) = g.board.find_hint().expect("a fresh deal has no move");
            g.cursor.tile_idx = Some(a);
            assert_eq!(press_key(&mut g, key), EventResult::Consumed);
            assert_eq!(
                g.selected,
                Some(a),
                "{key:?} did not select the cursor's tile"
            );
            g.cursor.tile_idx = Some(b);
            assert_eq!(press_key(&mut g, key), EventResult::Consumed);
            assert_eq!(
                g.board.remaining(),
                142,
                "{key:?} did not complete the pair"
            );
        }
    }

    #[test]
    fn enter_with_no_cursor_does_nothing() {
        let mut g = game();
        g.cursor.tile_idx = None;
        assert_eq!(press_key(&mut g, Key::Enter), EventResult::Ignored);
        assert_eq!(g.selected, None);
    }

    #[test]
    fn escape_clears_the_selection_the_hint_and_the_message_together() {
        let mut g = game();
        let (a, _) = g.board.find_hint().expect("a fresh deal has no move");
        probe::click(&mut g, Target::Tile(a));
        press_key(&mut g, Key::H);
        assert_eq!(press_key(&mut g, Key::Escape), EventResult::Consumed);
        assert_eq!(g.selected, None);
        assert!(!g.show_hint);
        assert_eq!(g.message, None);
        // With nothing left to clear it reports no change, so the window is not
        // asked to redraw the same frame again.
        assert_eq!(press_key(&mut g, Key::Escape), EventResult::Ignored);
    }

    #[test]
    fn a_key_this_game_does_not_use_is_left_for_the_window() {
        // Swallowing it would break every shortcut the window itself owns.
        let mut g = game();
        for key in [Key::A, Key::Q, Key::F1, Key::Tab] {
            assert_eq!(
                press_key(&mut g, key),
                EventResult::Ignored,
                "{key:?} was swallowed"
            );
        }
    }

    #[test]
    fn a_key_release_is_not_a_key_press() {
        // Without the guard every keystroke would act twice -- once down and
        // once up -- so N would deal two games and Z would undo two moves.
        let mut g = game();
        let before = g.seed;
        let result = handle_event(&mut g, &Event::Key(probe::release(Key::N)));
        assert_eq!(result, EventResult::Ignored);
        assert_eq!(g.seed, before, "releasing N dealt a new game");
    }

    #[test]
    fn taking_the_last_pair_wins_the_game() {
        let mut g = game();
        g.board = board_of(&[
            (0, 0, 0, TileKind::Season(0)),
            (0, 0, 2, TileKind::Season(3)),
        ]);
        g.cursor.tile_idx = Some(0);
        assert!(g.try_select(0));
        assert!(g.try_select(1));
        assert_eq!(g.status, GameStatus::Won);
        let f = g.draw(Mahjong::SIZE);
        text_saying(&f, "YOU WIN!");
        text_saying(&f, "You win!");
    }

    #[test]
    fn a_game_that_is_over_stops_answering_clicks_on_tiles() {
        // Otherwise a stray click after the win would count a move against a
        // board that has none left to make.
        let mut g = game();
        g.status = GameStatus::Won;
        let (a, _) = g.board.find_hint().expect("a fresh deal has no move");
        assert!(!g.try_select(a), "a finished game accepted a move");
        assert_eq!(g.selected, None);
    }

    #[test]
    fn a_board_with_no_move_left_is_announced_rather_than_left_silent() {
        let mut g = game();
        g.board = board_of(&[
            (0, 0, 0, TileKind::Bamboo(1)),
            (0, 0, 2, TileKind::Circle(9)),
        ]);
        g.update_status();
        assert_eq!(g.status, GameStatus::Lost);
        let f = g.draw(Mahjong::SIZE);
        text_saying(&f, "No valid moves remain!");
        text_saying(&f, "No moves left!");
    }

    // ════════════════════════════════════════════════════════════════
    // The window itself
    // ════════════════════════════════════════════════════════════════

    #[test]
    fn the_app_names_itself_for_the_taskbar_and_asks_for_a_size_it_can_use() {
        let g = game();
        assert_eq!(g.title(), "Mahjong Solitaire");
        assert_eq!(g.app_id(), "mahjong");
        let (w, h) = g.initial_size();
        assert_eq!((w, h), (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32));
        let l = Layout::solve(f32_from_u32(w), f32_from_u32(h));
        assert!(
            l.tile_w > 10.0 && l.legend.w > 0.0,
            "the window opens at a size that cannot show a legend or a readable tile"
        );
    }

    #[test]
    fn the_close_button_ends_the_program() {
        let mut g = game();
        assert_eq!(g.on_event(&Event::CloseRequested), Response::Exit);
    }

    #[test]
    fn a_resize_is_remembered_so_the_next_click_is_read_against_it() {
        let mut g = game();
        assert_eq!(
            g.on_event(&Event::Resize {
                width: 1600,
                height: 1200
            }),
            Response::Redraw
        );
        assert_eq!((g.width, g.height), (1600.0, 1200.0));
        assert_eq!(g.layout().tile_w, Layout::solve(1600.0, 1200.0).tile_w);
    }

    #[test]
    fn a_render_records_the_size_it_was_drawn_at() {
        // `render` is the only place the window states its size on a frame that
        // is actually painted; a click arrives afterwards with nothing but
        // coordinates.
        let mut g = game();
        let tree = g.render(1234.0, 567.0);
        assert_eq!((g.width, g.height), (1234.0, 567.0));
        assert!(
            !tree.commands.is_empty(),
            "the render produced an empty tree"
        );
    }

    #[test]
    fn an_event_that_changes_nothing_does_not_ask_for_a_repaint() {
        // Redrawing on every event is how a game at rest burns a core.
        let mut g = game();
        assert_eq!(
            g.on_event(&Event::Key(probe::press(Key::A))),
            Response::Idle
        );
        assert_eq!(
            g.on_event(&Event::Mouse(MouseEvent {
                x: -1.0,
                y: -1.0,
                kind: MouseEventKind::Press(MouseButton::Left)
            })),
            Response::Idle
        );
    }

    #[test]
    fn an_event_that_changes_something_asks_for_a_repaint() {
        let mut g = game();
        assert_eq!(
            g.on_event(&Event::Key(probe::press(Key::H))),
            Response::Redraw
        );
    }

    #[test]
    fn two_games_from_one_seed_are_the_same_and_from_two_seeds_are_not() {
        // The deal used to be `with_seed(42)` in `new`, so every player on
        // every machine got the same 144 tiles in the same order for ever.
        let a: Vec<TileKind> = Mahjong::with_seed(7)
            .board
            .tiles
            .iter()
            .map(|t| t.kind)
            .collect();
        let b: Vec<TileKind> = Mahjong::with_seed(7)
            .board
            .tiles
            .iter()
            .map(|t| t.kind)
            .collect();
        let c: Vec<TileKind> = Mahjong::with_seed(8)
            .board
            .tiles
            .iter()
            .map(|t| t.kind)
            .collect();
        assert_eq!(a, b, "one seed dealt two different games");
        assert_ne!(a, c, "two seeds dealt the same game");
    }

    #[test]
    fn a_fresh_deal_is_playable_and_the_cursor_starts_on_a_tile_that_can_be_played() {
        // A cursor pointing at a covered tile would answer Enter with "Tile is
        // not free" before the player had touched anything.
        for seed in 0..12u64 {
            let g = Mahjong::with_seed(seed);
            assert_eq!(g.board.remaining(), 144, "seed {seed} dealt a short board");
            assert!(
                g.board.find_hint().is_some(),
                "seed {seed} deals a dead board"
            );
            let cursor = g.cursor.tile_idx.expect("seed {seed} deals no cursor");
            assert!(
                g.board.is_free(cursor),
                "seed {seed} starts the cursor on a covered tile"
            );
            assert_eq!(g.status, GameStatus::Playing);
        }
    }
}
