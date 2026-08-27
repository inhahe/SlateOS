#![allow(clippy::too_many_lines)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::cast_possible_wrap)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::similar_names)]
#![allow(clippy::struct_excessive_bools)]
#![allow(clippy::fn_params_excessive_bools)]

//! Slate OS Tetris — classic falling-blocks puzzle game.
//!
//! A 10x20 playfield, the seven tetrominoes with SRS rotation and wall kicks,
//! ghost piece, hold, a next-three preview off a 7-bag randomiser, lock delay,
//! T-spin detection, and scoring with level progression.
//!
//! # What this file used to be, and what a window changed
//!
//! `main` built a `TetrisApp` and dropped it. All of the above existed and
//! none of it ran: no window, so no gravity, no keys, and no way to see a
//! board that was being simulated for nobody. Wiring it to `oswindow` is what
//! made three further faults visible, each of which needed the app to actually
//! be played to find:
//!
//! * **The layout decided the window size, not the other way round.** The old
//!   `window_width()`/`window_height()` were *derived* from a constant 28-pixel
//!   cell and a constant 130-pixel sidebar, so there was exactly one size the
//!   program was correct at. A real window is whatever the user drags it to.
//!   Every measurement now comes from [`Layout`], which is rebuilt from the
//!   live size on every frame and never remembered.
//! * **Nothing was clickable.** The overlay said "Press P to resume" and that
//!   was the only affordance in the program — a pointer could do nothing at
//!   all. The CONTROLS panel already listed every action next to its key, so
//!   each row is now the button it was describing, and the overlay does what
//!   it says when clicked.
//! * **The piece fell once per tick rather than once per interval.** Fixed
//!   here as a proper accumulator; see [`TetrisApp::tick`].
//!
//! A fourth fault was in the game logic and is described on
//! [`TetrisApp::clear_lines`]: a multi-line clear left one of the cleared rows
//! behind as a solid line the player could never remove.

use guitk::color::Color;
#[cfg(test)]
use guitk::event::Modifiers;
use guitk::event::{Event, EventResult, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::frame::Rect;
use guitk::probe::Probe;
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::rng::{RandomSource, SeededRng, seed_from_system};
use guitk::style::CornerRadii;
use oswindow::app::{self, App, Response};
use std::process::ExitCode;
use std::time::Duration;

// ── Catppuccin Mocha palette ────────────────────────────────────────
const BASE: Color = Color::from_hex(0x1E1E2E);
const MANTLE: Color = Color::from_hex(0x181825);
const CRUST: Color = Color::from_hex(0x11111B);
const SURFACE0: Color = Color::from_hex(0x313244);
const SURFACE2: Color = Color::from_hex(0x585B70);
const TEXT_COLOR: Color = Color::from_hex(0xCDD6F4);
const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const BLUE: Color = Color::from_hex(0x89B4FA);
const GREEN: Color = Color::from_hex(0xA6E3A1);
const RED: Color = Color::from_hex(0xF38BA8);
const YELLOW: Color = Color::from_hex(0xF9E2AF);
const PEACH: Color = Color::from_hex(0xFAB387);
const LAVENDER: Color = Color::from_hex(0xB4BEFE);
const OVERLAY0: Color = Color::from_hex(0x6C7086);
const TEAL: Color = Color::from_hex(0x94E2D5);
const MAUVE: Color = Color::from_hex(0xCBA6F7);
const SKY: Color = Color::from_hex(0x89DCEB);

// ── Layout constants ────────────────────────────────────────────────
const FIELD_COLS: usize = 10;
const FIELD_ROWS: usize = 20;
/// Extra hidden rows above the visible field for piece spawning.
const HIDDEN_ROWS: usize = 4;
const TOTAL_ROWS: usize = FIELD_ROWS + HIDDEN_ROWS;

const CELL_GAP: f32 = 1.0;
const PADDING: f32 = 16.0;
const SIDEBAR_WIDTH: f32 = 130.0;

const HEADER_HEIGHT: f32 = 44.0;
const HEADER_FONT_SIZE: f32 = 16.0;
const LABEL_FONT_SIZE: f32 = 12.0;
const VALUE_FONT_SIZE: f32 = 18.0;
const TITLE_FONT_SIZE: f32 = 22.0;
const MINI_CELL: f32 = 14.0;
const MINI_GAP: f32 = 1.0;
const PREVIEW_BOX_HEIGHT: f32 = 64.0;

/// Lock delay in milliseconds — piece sits on a surface this long before locking.
const LOCK_DELAY_MS: u64 = 500;

// ── What a pointer can do ───────────────────────────────────────────

/// Something the player can ask the game to do.
///
/// One enum for both input paths. A key press names an `Action` and so does a
/// click, which is what keeps the two from drifting: there is no action the
/// keyboard can reach that the pointer cannot, because the CONTROLS panel is
/// built from this same list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    MoveLeft,
    MoveRight,
    SoftDrop,
    HardDrop,
    RotateCw,
    RotateCcw,
    Hold,
    Pause,
    Restart,
}

/// The CONTROLS panel: every action, with the key that also performs it.
///
/// This is the panel's contents *and* the pointer's repertoire. The old panel
/// was a list of eight strings drawn for information only, with "\u{2190}\u{2192}  Move"
/// as a single row; splitting it is what lets each row be the button it names.
const CONTROLS: [(Action, &str); 9] = [
    (Action::MoveLeft, "\u{2190}  Move left"),
    (Action::MoveRight, "\u{2192}  Move right"),
    (Action::SoftDrop, "\u{2193}  Soft drop"),
    (Action::HardDrop, "Space  Hard drop"),
    (Action::RotateCw, "\u{2191}/Z  Rotate CW"),
    (Action::RotateCcw, "X  Rotate CCW"),
    (Action::Hold, "C  Hold"),
    (Action::Pause, "P  Pause"),
    (Action::Restart, "R  Restart"),
];

/// Everything on screen a click can land on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// A row of the CONTROLS panel, which performs the action it names.
    Control(Action),
    /// The HOLD box. The same as `Control(Action::Hold)`, but a distinct
    /// target so a test can say which of the two affordances it reached.
    HoldBox,
    /// The pause / game-over message box, which does what its text says:
    /// resume when paused, restart when the game is over.
    Overlay,
}

/// A frame of this app's drawing, with the boxes a click can land in.
pub type Frame = guitk::frame::Frame<Target>;

/// Below this the board is too small to read and the sidebars are dropped so
/// it can have their room. Ten columns at nine pixels each.
const MIN_FIELD_W: f32 = 90.0;
/// A sidebar narrower than this cannot hold a label and a value, so it is not
/// worth the width it would take from the board.
const MIN_SIDEBAR_W: f32 = 64.0;

// ── Layout ──────────────────────────────────────────────────────────

/// Where everything goes, at the size the window is *now*.
///
/// Rebuilt from the live width and height on every frame and never stored on
/// the app. That is the whole point: the previous version computed the window
/// size from the layout constants, so the program was correct at exactly one
/// size and drew off the edge at every other. A resize is not an event this
/// app has to handle — it is simply the next frame's arguments.
#[derive(Clone, Copy, Debug)]
pub struct Layout {
    pub window: Rect,
    pub pad: f32,
    pub header: Rect,
    /// The playfield's drawn extent, at whatever cell size fits.
    pub field: Rect,
    pub cell: f32,
    pub gap: f32,
    /// `Rect::EMPTY` when the window is too narrow for sidebars.
    pub hold: Rect,
    pub stats: Rect,
    pub next: Rect,
    pub controls: Rect,
    pub overlay: Rect,
    /// Base font for small labels; everything else is a multiple of it.
    pub font: f32,
}

impl Layout {
    #[must_use]
    pub fn new(width: f32, height: f32) -> Self {
        let window = Rect::new(0.0, 0.0, width.max(1.0), height.max(1.0));
        // The padding is a *fraction* of the window and has no lower bound.
        // A floor of three pixels reads as harmless until the window is two
        // pixels wide, at which point the padding alone is wider than the
        // window and every rect measured from it starts outside.
        let pad = (window.w.min(window.h) * 0.03).min(PADDING);
        let font = (window.h / 46.0).clamp(7.0, LABEL_FONT_SIZE);

        let body_w = (window.w - pad * 2.0).max(0.0);
        // Capped by what is left after the padding, not by the window: the
        // 18-pixel floor is a legibility wish, and a window shorter than that
        // does not grant it by letting the bar hang off the bottom.
        let header_h = (window.h * 0.09)
            .clamp(18.0, HEADER_HEIGHT)
            .min((window.h - pad * 2.0).max(0.0));
        let header = Rect::new(pad, pad, body_w, header_h);
        let body_y = header.bottom() + pad;
        let body_h = (window.h - pad - body_y).max(0.0);

        // Sidebars get a share of the width, but only while what is left is
        // still a board worth looking at. Below that they go entirely and the
        // board keeps the room — a squeezed board is unplayable, whereas a
        // missing HOLD panel costs the player a reminder, not a move.
        let side = (body_w * 0.2).min(SIDEBAR_WIDTH);
        let sides = side >= MIN_SIDEBAR_W && body_w - side * 2.0 - pad * 2.0 >= MIN_FIELD_W;
        let (mid_x, mid_w) = if sides {
            (pad + side + pad, body_w - side * 2.0 - pad * 2.0)
        } else {
            (pad, body_w)
        };

        // One cell size for both axes, so the board stays square-celled at any
        // window shape; whichever axis is tighter is the one that decides.
        //
        // The gap is derived from the cell rather than fixed at a pixel, in
        // two passes because each depends on the other. A constant gap is a
        // constant that does not shrink: at a one-pixel cell the nine gaps
        // between ten columns are most of the board's width, and the field
        // ends up wider than the window that asked for it.
        let raw = (mid_w / FIELD_COLS as f32).min(body_h / FIELD_ROWS as f32);
        let gap = CELL_GAP.min(raw.max(0.0) * 0.1);
        let by_width = ((mid_w + gap) / FIELD_COLS as f32 - gap).max(0.0);
        let by_height = ((body_h + gap) / FIELD_ROWS as f32 - gap).max(0.0);
        let cell = by_width.min(by_height);
        let fw = FIELD_COLS as f32 * (cell + gap) - gap;
        let fh = FIELD_ROWS as f32 * (cell + gap) - gap;
        // Centred in the room it was given, never pushed out of it: the cell
        // was chosen so the board fits, and floating-point rounding is not a
        // reason to start the first column left of where the room begins.
        let field = Rect::new(
            mid_x + ((mid_w - fw) / 2.0).max(0.0),
            body_y + ((body_h - fh) / 2.0).max(0.0),
            fw,
            fh,
        );

        // The preview boxes are sized from the mini-cell they must hold, so a
        // short window shrinks them rather than letting them run past the
        // bottom edge with the stats and controls pushed off after them.
        // The 28-pixel floor is the smallest box a mini piece reads in, but it
        // is a preference and not a licence: a body shorter than that gets the
        // body, because a box taller than the space it sits in overhangs the
        // panel below and then the window.
        let preview = (body_h * 0.16)
            .clamp(28.0, PREVIEW_BOX_HEIGHT + 24.0)
            .min(body_h);
        let (hold, stats, next, controls) = if sides {
            let lx = pad;
            let rx = window.w - pad - side;
            (
                Rect::new(lx, body_y, side, preview),
                Rect::new(
                    lx,
                    body_y + preview + pad,
                    side,
                    (body_h - preview - pad).max(0.0),
                ),
                Rect::new(rx, body_y, side, (preview * 2.4).min(body_h)),
                Rect::new(
                    rx,
                    body_y + (preview * 2.4).min(body_h) + pad,
                    side,
                    (body_h - (preview * 2.4).min(body_h) - pad).max(0.0),
                ),
            )
        } else {
            (Rect::EMPTY, Rect::EMPTY, Rect::EMPTY, Rect::EMPTY)
        };

        let ow = (window.w * 0.7).clamp(90.0, 260.0).min(window.w);
        let oh = (window.h * 0.28).clamp(50.0, 100.0).min(window.h);
        let overlay = Rect::new((window.w - ow) / 2.0, (window.h - oh) / 2.0, ow, oh);

        Self {
            window,
            pad,
            header,
            field,
            cell,
            gap,
            hold,
            stats,
            next,
            controls,
            overlay,
            font,
        }
    }

    /// The drawn box of the playfield cell at (visible row, column).
    #[must_use]
    pub fn cell_rect(&self, row: usize, col: usize) -> Rect {
        let step = self.cell + self.gap;
        Rect::new(
            self.field.x + col as f32 * step,
            self.field.y + row as f32 * step,
            self.cell,
            self.cell,
        )
    }

    /// Height of one row in a labelled panel, and how many of them fit.
    fn rows_in(panel: Rect, count: usize, head: f32, min: f32) -> (f32, usize) {
        if panel.is_empty() || count == 0 {
            return (0.0, 0);
        }
        let room = (panel.h - head).max(0.0);
        let ideal = room / count as f32;
        if ideal >= min {
            return (ideal, count);
        }
        // Not enough room for all of them at a legible size, so show the ones
        // that fit rather than overlapping the lot into an unreadable smear.
        let fits = (room / min) as usize;
        (min, fits.min(count))
    }

    /// The five STATS rows: height of one, and how many the panel can show.
    #[must_use]
    pub fn stat_rows(&self) -> (f32, usize) {
        Self::rows_in(self.stats, 5, self.font * 2.0, self.font * 2.2)
    }

    /// The CONTROLS rows: height of one, and how many the panel can show.
    #[must_use]
    pub fn control_rows(&self) -> (f32, usize) {
        Self::rows_in(
            self.controls,
            CONTROLS.len(),
            self.font * 2.0,
            self.font * 1.5,
        )
    }

    /// The box of CONTROLS row `index`, or `Rect::EMPTY` if it does not fit.
    #[must_use]
    pub fn control_row(&self, index: usize) -> Rect {
        let (h, shown) = self.control_rows();
        if index >= shown {
            return Rect::EMPTY;
        }
        Rect::new(
            self.controls.x + 4.0,
            self.controls.y + self.font * 2.0 + index as f32 * h,
            (self.controls.w - 8.0).max(0.0),
            h,
        )
    }
}

// ── Piece types and shapes ──────────────────────────────────────────

/// The 7 standard tetrominoes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum PieceKind {
    I,
    O,
    T,
    S,
    Z,
    J,
    L,
}

impl PieceKind {
    const ALL: [Self; 7] = [
        Self::I,
        Self::O,
        Self::T,
        Self::S,
        Self::Z,
        Self::J,
        Self::L,
    ];

    /// Color for this piece kind.
    fn color(self) -> Color {
        match self {
            Self::I => SKY,
            Self::O => YELLOW,
            Self::T => MAUVE,
            Self::S => GREEN,
            Self::Z => RED,
            Self::J => BLUE,
            Self::L => PEACH,
        }
    }

    /// Get the 4 cells (row, col) offsets for this piece in the given rotation state.
    /// Rotation states: 0 = spawn, 1 = CW, 2 = 180, 3 = CCW.
    /// Offsets are relative to the piece's origin (top-left of bounding box).
    fn cells(self, rotation: u8) -> [(i8, i8); 4] {
        // SRS shapes using standard Tetris Guideline definitions.
        // Each cell is (row_offset, col_offset) within the bounding box.
        match self {
            Self::I => match rotation {
                0 => [(1, 0), (1, 1), (1, 2), (1, 3)],
                1 => [(0, 2), (1, 2), (2, 2), (3, 2)],
                2 => [(2, 0), (2, 1), (2, 2), (2, 3)],
                3 => [(0, 1), (1, 1), (2, 1), (3, 1)],
                _ => [(1, 0), (1, 1), (1, 2), (1, 3)],
            },
            Self::O => [(0, 0), (0, 1), (1, 0), (1, 1)],
            Self::T => match rotation {
                0 => [(0, 1), (1, 0), (1, 1), (1, 2)],
                1 => [(0, 1), (1, 1), (1, 2), (2, 1)],
                2 => [(1, 0), (1, 1), (1, 2), (2, 1)],
                3 => [(0, 1), (1, 0), (1, 1), (2, 1)],
                _ => [(0, 1), (1, 0), (1, 1), (1, 2)],
            },
            Self::S => match rotation {
                0 => [(0, 1), (0, 2), (1, 0), (1, 1)],
                1 => [(0, 1), (1, 1), (1, 2), (2, 2)],
                2 => [(1, 1), (1, 2), (2, 0), (2, 1)],
                3 => [(0, 0), (1, 0), (1, 1), (2, 1)],
                _ => [(0, 1), (0, 2), (1, 0), (1, 1)],
            },
            Self::Z => match rotation {
                0 => [(0, 0), (0, 1), (1, 1), (1, 2)],
                1 => [(0, 2), (1, 1), (1, 2), (2, 1)],
                2 => [(1, 0), (1, 1), (2, 1), (2, 2)],
                3 => [(0, 1), (1, 0), (1, 1), (2, 0)],
                _ => [(0, 0), (0, 1), (1, 1), (1, 2)],
            },
            Self::J => match rotation {
                0 => [(0, 0), (1, 0), (1, 1), (1, 2)],
                1 => [(0, 1), (0, 2), (1, 1), (2, 1)],
                2 => [(1, 0), (1, 1), (1, 2), (2, 2)],
                3 => [(0, 1), (1, 1), (2, 0), (2, 1)],
                _ => [(0, 0), (1, 0), (1, 1), (1, 2)],
            },
            Self::L => match rotation {
                0 => [(0, 2), (1, 0), (1, 1), (1, 2)],
                1 => [(0, 1), (1, 1), (2, 1), (2, 2)],
                2 => [(1, 0), (1, 1), (1, 2), (2, 0)],
                3 => [(0, 0), (0, 1), (1, 1), (2, 1)],
                _ => [(0, 2), (1, 0), (1, 1), (1, 2)],
            },
        }
    }

    /// Bounding box size for this piece (rows, cols). I is 4x4, O is 2x2, rest are 3x3.
    fn bounding_size(self) -> (i8, i8) {
        match self {
            Self::I => (4, 4),
            Self::O => (2, 2),
            _ => (3, 3),
        }
    }
}

// ── SRS Wall Kick Data ──────────────────────────────────────────────

/// SRS wall kick offsets for J, L, S, T, Z pieces.
/// Each entry: (test_index, from_state, to_state) -> (col_offset, row_offset).
/// Tests are tried in order 0..4; first that passes wins.
fn wall_kick_data_jlstz(from: u8, to: u8) -> [(i8, i8); 5] {
    match (from, to) {
        (0, 1) => [(0, 0), (-1, 0), (-1, 1), (0, -2), (-1, -2)],
        (1, 0) => [(0, 0), (1, 0), (1, -1), (0, 2), (1, 2)],
        (1, 2) => [(0, 0), (1, 0), (1, -1), (0, 2), (1, 2)],
        (2, 1) => [(0, 0), (-1, 0), (-1, 1), (0, -2), (-1, -2)],
        (2, 3) => [(0, 0), (1, 0), (1, 1), (0, -2), (1, -2)],
        (3, 2) => [(0, 0), (-1, 0), (-1, -1), (0, 2), (-1, 2)],
        (3, 0) => [(0, 0), (-1, 0), (-1, -1), (0, 2), (-1, 2)],
        (0, 3) => [(0, 0), (1, 0), (1, 1), (0, -2), (1, -2)],
        _ => [(0, 0), (0, 0), (0, 0), (0, 0), (0, 0)],
    }
}

/// SRS wall kick offsets for I piece.
fn wall_kick_data_i(from: u8, to: u8) -> [(i8, i8); 5] {
    match (from, to) {
        (0, 1) => [(0, 0), (-2, 0), (1, 0), (-2, -1), (1, 2)],
        (1, 0) => [(0, 0), (2, 0), (-1, 0), (2, 1), (-1, -2)],
        (1, 2) => [(0, 0), (-1, 0), (2, 0), (-1, 2), (2, -1)],
        (2, 1) => [(0, 0), (1, 0), (-2, 0), (1, -2), (-2, 1)],
        (2, 3) => [(0, 0), (2, 0), (-1, 0), (2, 1), (-1, -2)],
        (3, 2) => [(0, 0), (-2, 0), (1, 0), (-2, -1), (1, 2)],
        (3, 0) => [(0, 0), (1, 0), (-2, 0), (1, -2), (-2, 1)],
        (0, 3) => [(0, 0), (-1, 0), (2, 0), (-1, 2), (2, -1)],
        _ => [(0, 0), (0, 0), (0, 0), (0, 0), (0, 0)],
    }
}

/// Get wall kick data for a piece rotation transition.
fn wall_kicks(kind: PieceKind, from: u8, to: u8) -> [(i8, i8); 5] {
    match kind {
        PieceKind::I => wall_kick_data_i(from, to),
        PieceKind::O => [(0, 0); 5], // O piece never needs kicks
        _ => wall_kick_data_jlstz(from, to),
    }
}

// ── Active piece state ──────────────────────────────────────────────

/// An active (falling) piece on the playfield.
///
/// Comparable because the pointer and the keyboard must be shown to leave the
/// board in the same state, and "the same state" is most of it this struct.
#[derive(Clone, Debug, PartialEq, Eq)]
struct ActivePiece {
    kind: PieceKind,
    /// Rotation state: 0 = spawn, 1 = CW, 2 = 180, 3 = CCW.
    rotation: u8,
    /// Row position of the piece's bounding box top-left corner.
    /// Row 0 is the topmost hidden row.
    row: i8,
    /// Column position of the piece's bounding box top-left corner.
    col: i8,
}

impl ActivePiece {
    /// Create a new piece at spawn position.
    fn spawn(kind: PieceKind) -> Self {
        // Spawn in the hidden rows, horizontally centered. Every kind starts
        // two rows into the hidden band, which is deep enough that the tallest
        // spawn shape (I, vertical after a rotation) is fully above the visible
        // field and shallow enough that the piece appears immediately.
        let (_, cols) = kind.bounding_size();
        let col = (FIELD_COLS as i8).saturating_sub(cols) / 2;
        Self {
            kind,
            rotation: 0,
            row: (HIDDEN_ROWS as i8).saturating_sub(2),
            col,
        }
    }

    /// Get the absolute (row, col) positions of this piece's 4 cells.
    fn absolute_cells(&self) -> [(i8, i8); 4] {
        let offsets = self.kind.cells(self.rotation);
        // `saturating_add` rather than `+`: a piece kicked hard against a wall
        // is tested at coordinates that may be off the board, and an
        // out-of-range cell must read as out of range rather than wrap into a
        // valid one on the opposite side.
        offsets.map(|(dr, dc)| (self.row.saturating_add(dr), self.col.saturating_add(dc)))
    }

    /// Return a copy of this piece moved by (dr, dc).
    fn moved(&self, dr: i8, dc: i8) -> Self {
        Self {
            kind: self.kind,
            rotation: self.rotation,
            row: self.row.saturating_add(dr),
            col: self.col.saturating_add(dc),
        }
    }

    /// Return a copy of this piece with a new rotation.
    fn with_rotation(&self, new_rotation: u8) -> Self {
        Self {
            kind: self.kind,
            rotation: new_rotation,
            row: self.row,
            col: self.col,
        }
    }
}

// ── Randomness ──────────────────────────────────────────────────────

/// Seed used when the kernel's entropy source cannot be reached.
///
/// A per-crate constant rather than a shared one, so that two programs which
/// lose entropy on the same boot do not then produce correlated streams. The
/// bytes spell `TETRIS!!`.
const FALLBACK_SEED: u64 = 0x5445_5452_4953_2121;

// This crate used to carry its own copy of the LCG that got copied into
// sixteen crates, reducing with `val % bound`. That is the broken reduction,
// and the 7-bag shuffle below is where it bit.
//
// The generator's modulus is 2^64, so bit *k* of its state has period 2^(k+1):
// the low bits are not weak, they are a counter. `fill_bag` runs Fisher-Yates
// over seven pieces, so its bounds count down 7, 6, 5, 4, 3, 2 -- and the
// bounds 4 and 2 read the low two bits and the low one bit respectively.
//
// Measured, not reasoned: `fill_bag` makes exactly six draws, and 6 mod 2 = 0,
// so the bound-2 draw at `i == 1` returned **0 in every bag, for ever** --
// `self.bag.swap(1, 0)` was performed unconditionally rather than half the
// time. And 6 mod 4 = 2, so the bound-4 draw at `i == 3` alternated 2, 0, 2, 0
// across successive bags. Two of the six shuffle steps in every tetris bag
// were not shuffle steps; they were a fixed function of how many bags had gone
// before. The bag still held all seven pieces -- that is what made it survive
// review -- but the order it dealt them in was substantially predetermined.
//
// `randrange::below` is Lemire's method: it multiplies by the bound into 128
// bits and keeps the *top* half, so it reads the high bits and never the low
// ones, with a rejection step that makes it exactly uniform.

// ── 7-bag randomizer ────────────────────────────────────────────────

/// Generates pieces using the 7-bag system: shuffle all 7 piece kinds,
/// deal them in order, refill the bag when empty.
struct BagRandomizer {
    rng: SeededRng,
    bag: Vec<PieceKind>,
}

impl BagRandomizer {
    fn new(seed: u64) -> Self {
        let mut this = Self {
            rng: SeededRng::new(seed),
            bag: Vec::new(),
        };
        this.fill_bag();
        this
    }

    fn fill_bag(&mut self) {
        self.bag = PieceKind::ALL.to_vec();
        // Was a hand-rolled Fisher-Yates over `next_bounded(i + 1)`. Same
        // algorithm, but drawing through the shared generator: see the note by
        // `FALLBACK_SEED` for what the old reduction did to two of these six
        // steps.
        self.rng.shuffle(&mut self.bag);
    }

    fn next_piece(&mut self) -> PieceKind {
        if self.bag.is_empty() {
            self.fill_bag();
        }
        // Pop from the end (most efficient for Vec).
        self.bag.pop().unwrap_or(PieceKind::T)
    }
}

// ── T-spin detection ────────────────────────────────────────────────

/// Whether the cell at (row, col) blocks a T-spin corner.
///
/// Off the board counts as occupied, which is what makes a T-spin against a
/// wall or the floor detectable at all — the wall is as solid as a block, and
/// the rule that names the spin does not distinguish them.
fn corner_is_blocked(field: &[Option<Color>], row: i8, col: i8) -> bool {
    let (Ok(r), Ok(c)) = (usize::try_from(row), usize::try_from(col)) else {
        return true;
    };
    if r >= TOTAL_ROWS || c >= FIELD_COLS {
        return true;
    }
    r.checked_mul(FIELD_COLS)
        .and_then(|base| base.checked_add(c))
        .and_then(|idx| field.get(idx))
        .is_some_and(Option::is_some)
}

/// Check the 4 corners of the T piece's 3x3 bounding box for occupied cells.
fn count_t_corners(field: &[Option<Color>], row: i8, col: i8) -> usize {
    [(0, 0), (0, 2), (2, 0), (2, 2)]
        .into_iter()
        .filter(|&(dr, dc)| {
            corner_is_blocked(field, row.saturating_add(dr), col.saturating_add(dc))
        })
        .count()
}

/// Check the two "front" corners of the T piece in its current rotation.
/// Front corners are the two corners adjacent to the flat side.
fn count_t_front_corners(field: &[Option<Color>], row: i8, col: i8, rotation: u8) -> usize {
    let front_corners: [(i8, i8); 2] = match rotation {
        0 => [(0, 0), (0, 2)], // top-left, top-right
        1 => [(0, 2), (2, 2)], // top-right, bottom-right
        2 => [(2, 0), (2, 2)], // bottom-left, bottom-right
        3 => [(0, 0), (2, 0)], // top-left, bottom-left
        _ => [(0, 0), (0, 2)],
    };
    front_corners
        .into_iter()
        .filter(|&(dr, dc)| {
            corner_is_blocked(field, row.saturating_add(dr), col.saturating_add(dc))
        })
        .count()
}

/// T-spin classification.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TSpinKind {
    None,
    Mini,
    Full,
}

// ── Scoring ─────────────────────────────────────────────────────────

/// Compute the score for a line clear.
fn line_clear_score(lines: usize, level: u32, tspin: TSpinKind) -> u32 {
    let base = match (lines, tspin) {
        (1, TSpinKind::None) => 100,
        (2, TSpinKind::None) => 300,
        (3, TSpinKind::None) => 500,
        (4, TSpinKind::None) => 800, // Tetris
        (0, TSpinKind::Mini) => 100,
        (1, TSpinKind::Mini) => 200,
        (2, TSpinKind::Mini) => 400, // T-spin mini double (rare)
        (0, TSpinKind::Full) => 400,
        (1, TSpinKind::Full) => 800,
        (2, TSpinKind::Full) => 1200,
        (3, TSpinKind::Full) => 1600,
        _ => u32::try_from(lines).unwrap_or(0).saturating_mul(100),
    };
    // Saturating rather than wrapping: a score that rolled over to nothing
    // after a long game would read as a fresh start, which is worse than a
    // score that sticks at the top of the range and is visibly stuck.
    base.saturating_mul(level)
}

/// Soft drop score: 1 point per cell dropped.
fn soft_drop_score(cells: u32) -> u32 {
    cells
}

/// Hard drop score: 2 points per cell dropped.
fn hard_drop_score(cells: u32) -> u32 {
    cells.saturating_mul(2)
}

// ── Game state ──────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GameStatus {
    Playing,
    Paused,
    GameOver,
}

/// Main Tetris application state.
struct TetrisApp {
    /// The playfield: `TOTAL_ROWS` * `FIELD_COLS`. `None` = empty, `Some(color)` = occupied.
    field: Vec<Option<Color>>,
    /// Currently active (falling) piece.
    current_piece: Option<ActivePiece>,
    /// Held piece kind (swap with C).
    hold_piece: Option<PieceKind>,
    /// Whether hold has already been used this turn (can only hold once per piece).
    hold_used: bool,
    /// Piece generator (7-bag randomizer).
    piece_gen: BagRandomizer,
    /// Upcoming pieces buffer (kept filled for preview).
    preview_queue: Vec<PieceKind>,

    /// Game status.
    status: GameStatus,
    /// Current score.
    score: u32,
    /// Current level (starts at 1).
    level: u32,
    /// Total lines cleared.
    lines_cleared: u32,
    /// Total pieces placed (locked into field).
    pieces_placed: u32,
    /// Elapsed game time in milliseconds.
    elapsed_ms: u64,

    /// Accumulated gravity time in ms since last gravity step.
    gravity_accum_ms: u64,
    /// Lock delay tracking: ms since piece first landed on a surface.
    lock_accum_ms: u64,
    /// Whether the piece is currently resting on a surface.
    piece_on_surface: bool,
    /// Number of lock delay resets (moves/rotations while on surface).
    lock_resets: u32,
    /// Maximum lock resets before forced lock.
    max_lock_resets: u32,

    /// Whether the last successful rotation was a kick (for T-spin detection).
    last_move_was_rotation: bool,
    /// Whether the last rotation used a wall kick.
    last_rotation_was_kick: bool,

    /// RNG seed (stored for reproducibility in tests).
    seed: u64,

    /// The window's current width and height. The only thing remembered about
    /// the window: everything else is derived from these two on each frame.
    width: f32,
    height: f32,
}

impl TetrisApp {
    fn new() -> Self {
        // Was `with_seed(42)`: every player, on every machine, got the same
        // piece order from the first bag onwards. The `u64` form is used and
        // not the generator form because this app *stores* its seed -- restart
        // is `with_seed(self.seed + 1)`, and an app holding a generator instead
        // would have to reseed one generator from another's output, which
        // silently correlates the two.
        Self::with_seed(seed_from_system(FALLBACK_SEED))
    }

    fn with_seed(seed: u64) -> Self {
        let mut piece_gen = BagRandomizer::new(seed);

        // Fill the preview queue with upcoming pieces.
        let mut preview_queue = Vec::new();
        for _ in 0..3 {
            preview_queue.push(piece_gen.next_piece());
        }

        let mut app = Self {
            field: vec![None; TOTAL_ROWS * FIELD_COLS],
            current_piece: None,
            hold_piece: None,
            hold_used: false,
            piece_gen,
            preview_queue,
            status: GameStatus::Playing,
            score: 0,
            level: 1,
            lines_cleared: 0,
            pieces_placed: 0,
            elapsed_ms: 0,
            gravity_accum_ms: 0,
            lock_accum_ms: 0,
            piece_on_surface: false,
            lock_resets: 0,
            max_lock_resets: 15,
            last_move_was_rotation: false,
            last_rotation_was_kick: false,
            seed,
            // Replaced by the first `Resize` or `render`, whichever the window
            // sends first. This is a starting guess and never a measurement.
            width: 640.0,
            height: 720.0,
        };

        app.spawn_piece();
        app
    }

    // ── Field access ────────────────────────────────────────────────

    /// The flat index of (row, col), or `None` if the pair names no cell.
    ///
    /// One place computes the stride so one place can be wrong about it, and
    /// so both accessors agree about what is off the board — a `get` that
    /// clamped and a `set` that dropped would silently disagree about the
    /// edges.
    fn field_index(row: usize, col: usize) -> Option<usize> {
        if row >= TOTAL_ROWS || col >= FIELD_COLS {
            return None;
        }
        row.checked_mul(FIELD_COLS)?.checked_add(col)
    }

    fn field_get(&self, row: usize, col: usize) -> Option<Color> {
        Self::field_index(row, col).and_then(|idx| self.field.get(idx).copied().flatten())
    }

    fn field_set(&mut self, row: usize, col: usize, value: Option<Color>) {
        if let Some(cell) = Self::field_index(row, col).and_then(|idx| self.field.get_mut(idx)) {
            *cell = value;
        }
    }

    // ── Collision detection ─────────────────────────────────────────

    /// Check if a piece position is valid (no collisions with walls or placed blocks).
    fn is_valid_position(&self, piece: &ActivePiece) -> bool {
        piece.absolute_cells().into_iter().all(|(r, c)| {
            let Ok(col) = usize::try_from(c) else {
                return false;
            };
            if col >= FIELD_COLS || r >= TOTAL_ROWS as i8 {
                return false;
            }
            // Above the field is legal — that is where pieces spawn, and a
            // piece is only rejected up there by a block, never by the ceiling.
            let Ok(row) = usize::try_from(r) else {
                return true;
            };
            self.field_get(row, col).is_none()
        })
    }

    // ── Piece spawning ──────────────────────────────────────────────

    /// Spawn the next piece from the preview queue.
    fn spawn_piece(&mut self) {
        if self.preview_queue.is_empty() {
            self.refill_preview();
        }
        // Take the first piece from the preview queue.
        let kind = self.preview_queue.remove(0);
        // Refill the queue.
        self.preview_queue.push(self.piece_gen.next_piece());

        let piece = ActivePiece::spawn(kind);
        if self.is_valid_position(&piece) {
            self.current_piece = Some(piece);
            self.piece_on_surface = false;
            self.lock_accum_ms = 0;
            self.lock_resets = 0;
            self.gravity_accum_ms = 0;
            self.hold_used = false;
            self.last_move_was_rotation = false;
            self.last_rotation_was_kick = false;
        } else {
            // Can't spawn -> game over
            self.current_piece = Some(piece);
            self.status = GameStatus::GameOver;
        }
    }

    fn refill_preview(&mut self) {
        while self.preview_queue.len() < 3 {
            self.preview_queue.push(self.piece_gen.next_piece());
        }
    }

    // ── Movement ────────────────────────────────────────────────────

    /// Try to move the current piece by (dr, dc). Returns true if successful.
    fn try_move(&mut self, dr: i8, dc: i8) -> bool {
        if self.status != GameStatus::Playing {
            return false;
        }
        let piece = match &self.current_piece {
            Some(p) => p.moved(dr, dc),
            None => return false,
        };
        if self.is_valid_position(&piece) {
            self.current_piece = Some(piece);
            self.last_move_was_rotation = false;
            // Reset lock delay on successful horizontal move while on surface
            if dc != 0 && self.piece_on_surface && self.lock_resets < self.max_lock_resets {
                self.lock_accum_ms = 0;
                self.lock_resets = self.lock_resets.saturating_add(1);
            }
            // Update surface status
            self.update_surface_status();
            true
        } else {
            false
        }
    }

    /// Check and update whether the piece is on a surface.
    fn update_surface_status(&mut self) {
        if let Some(piece) = &self.current_piece {
            let below = piece.moved(1, 0);
            self.piece_on_surface = !self.is_valid_position(&below);
        }
    }

    /// Try to rotate the current piece. `clockwise = true` for CW, false for CCW.
    fn try_rotate(&mut self, clockwise: bool) -> bool {
        if self.status != GameStatus::Playing {
            return false;
        }
        let piece = match &self.current_piece {
            Some(p) => p.clone(),
            None => return false,
        };

        // O piece doesn't rotate
        if piece.kind == PieceKind::O {
            return true;
        }

        let from = piece.rotation;
        // Rotation states are 0..=3, so masking the low two bits *is* the
        // modulo and cannot be told apart from it -- and unlike `%` it needs no
        // proof that the addition did not overflow first.
        let to = if clockwise {
            from.wrapping_add(1) & 3
        } else {
            from.wrapping_add(3) & 3
        };

        let kicks = wall_kicks(piece.kind, from, to);
        let rotated = piece.with_rotation(to);

        for (kick_idx, (dc, dr)) in kicks.iter().enumerate() {
            // Wall kick offsets: (col_offset, row_offset). Row is inverted
            // because SRS defines positive Y as up, but our grid has row 0 at top.
            let test = ActivePiece {
                kind: rotated.kind,
                rotation: rotated.rotation,
                row: rotated.row.saturating_sub(*dr),
                col: rotated.col.saturating_add(*dc),
            };
            if self.is_valid_position(&test) {
                self.current_piece = Some(test);
                self.last_move_was_rotation = true;
                self.last_rotation_was_kick = kick_idx > 0;
                // Reset lock delay on successful rotation while on surface
                if self.piece_on_surface && self.lock_resets < self.max_lock_resets {
                    self.lock_accum_ms = 0;
                    self.lock_resets = self.lock_resets.saturating_add(1);
                }
                self.update_surface_status();
                return true;
            }
        }

        false
    }

    /// Soft drop: move piece down one row. Returns true if moved.
    fn soft_drop(&mut self) -> bool {
        if self.try_move(1, 0) {
            self.score = self.score.saturating_add(soft_drop_score(1));
            self.gravity_accum_ms = 0;
            true
        } else {
            false
        }
    }

    /// Hard drop: instantly drop piece to lowest valid position and lock.
    fn hard_drop(&mut self) {
        if self.status != GameStatus::Playing {
            return;
        }
        if self.current_piece.is_none() {
            return;
        }

        let mut cells_dropped: u32 = 0;
        while self.try_move(1, 0) {
            cells_dropped = cells_dropped.saturating_add(1);
        }
        self.score = self.score.saturating_add(hard_drop_score(cells_dropped));
        self.lock_piece();
    }

    /// Ghost piece position: the lowest valid position for the current piece.
    fn ghost_row(&self) -> Option<i8> {
        let piece = self.current_piece.as_ref()?;
        let mut test = piece.clone();
        while self.is_valid_position(&test.moved(1, 0)) {
            test = test.moved(1, 0);
        }
        Some(test.row)
    }

    // ── Hold piece ──────────────────────────────────────────────────

    /// Hold the current piece. Swap with held piece if one exists.
    fn hold_piece(&mut self) {
        if self.status != GameStatus::Playing || self.hold_used {
            return;
        }
        if let Some(piece) = self.current_piece.take() {
            let current_kind = piece.kind;
            if let Some(held_kind) = self.hold_piece.take() {
                // Swap: spawn the previously held piece
                self.hold_piece = Some(current_kind);
                let new_piece = ActivePiece::spawn(held_kind);
                if self.is_valid_position(&new_piece) {
                    self.current_piece = Some(new_piece);
                } else {
                    self.status = GameStatus::GameOver;
                }
            } else {
                // First hold: put current piece in hold, spawn next
                self.hold_piece = Some(current_kind);
                self.spawn_piece();
            }
            self.hold_used = true;
            self.piece_on_surface = false;
            self.lock_accum_ms = 0;
            self.lock_resets = 0;
            self.last_move_was_rotation = false;
            self.last_rotation_was_kick = false;
            self.update_surface_status();
        }
    }

    // ── Locking and line clearing ───────────────────────────────────

    /// Lock the current piece into the field.
    fn lock_piece(&mut self) {
        let piece = match self.current_piece.take() {
            Some(p) => p,
            None => return,
        };

        // Detect T-spin before locking
        let tspin = self.detect_tspin(&piece);

        let color = piece.kind.color();
        for (r, c) in piece.absolute_cells() {
            // `field_set` drops anything off the board, so a piece locked with
            // a cell above the ceiling loses that cell rather than wrapping it
            // somewhere it would be a surprise.
            if let (Ok(row), Ok(col)) = (usize::try_from(r), usize::try_from(c)) {
                self.field_set(row, col, Some(color));
            }
        }

        self.pieces_placed = self.pieces_placed.saturating_add(1);
        self.piece_on_surface = false;
        self.lock_accum_ms = 0;
        self.lock_resets = 0;

        // Clear lines
        let lines = self.clear_lines();

        // Score
        if lines > 0 || tspin != TSpinKind::None {
            self.score = self
                .score
                .saturating_add(line_clear_score(lines, self.level, tspin));
        }
        self.lines_cleared = self
            .lines_cleared
            .saturating_add(u32::try_from(lines).unwrap_or(0));

        // Level up every 10 lines. The level only ever rises: it is derived
        // from a total that only rises, but stating that as a `max` rather
        // than a comparison means a future change to the formula cannot make
        // the level fall mid-game.
        let new_level = (self.lines_cleared / 10).saturating_add(1);
        self.level = self.level.max(new_level);

        // Spawn next piece
        self.spawn_piece();
    }

    /// Detect if the last move was a T-spin.
    fn detect_tspin(&self, piece: &ActivePiece) -> TSpinKind {
        if piece.kind != PieceKind::T || !self.last_move_was_rotation {
            return TSpinKind::None;
        }

        let corners = count_t_corners(&self.field, piece.row, piece.col);
        if corners < 3 {
            return TSpinKind::None;
        }

        let front_corners =
            count_t_front_corners(&self.field, piece.row, piece.col, piece.rotation);
        if front_corners >= 2 {
            TSpinKind::Full
        } else if self.last_rotation_was_kick {
            // T-spin mini: 3+ corners occupied but fewer than 2 front corners,
            // and the rotation used a wall kick.
            TSpinKind::Mini
        } else {
            TSpinKind::None
        }
    }

    /// Clear completed lines and return the count.
    ///
    /// The field is rebuilt rather than shifted in place, and that is the whole
    /// point of the function's shape. The obvious implementation — collect the
    /// full rows' indices, then for each one shift everything above it down a
    /// row — is wrong for a reason that only shows up on a multi-line clear:
    /// **the first shift moves the rows that the remaining indices name.** With
    /// rows 21 and 23 full, clearing 23 slides row 21's contents down to 22, so
    /// the recorded index 21 now points at an innocent row; that row is deleted
    /// instead and the full one survives as a solid line the player can never
    /// get rid of. Every double, triple and tetris left one behind, and the
    /// board silently filled from the bottom.
    ///
    /// The tests missed it because they asserted the returned *count*, which
    /// was right the whole time — the count is taken before any shifting, so it
    /// cannot see the damage the shifting does.
    ///
    /// Keeping the surviving rows in order and pushing them to the bottom of a
    /// fresh field has no such coupling: nothing is indexed after it has moved,
    /// because nothing moves until every row has been classified.
    fn clear_lines(&mut self) -> usize {
        let mut kept: Vec<Option<Color>> = Vec::with_capacity(self.field.len());
        let mut kept_rows = 0usize;

        for row in 0..TOTAL_ROWS {
            if (0..FIELD_COLS).all(|col| self.field_get(row, col).is_some()) {
                continue;
            }
            for col in 0..FIELD_COLS {
                kept.push(self.field_get(row, col));
            }
            kept_rows = kept_rows.saturating_add(1);
        }

        let cleared = TOTAL_ROWS.saturating_sub(kept_rows);
        if cleared == 0 {
            return 0;
        }

        // The blank rows go on top, so the survivors keep their order and land
        // as far down as the cleared count allows — which is what "everything
        // above falls" means once it is stated as a whole-field property rather
        // than as a sequence of row moves.
        let mut field = vec![None; cleared.saturating_mul(FIELD_COLS)];
        field.extend(kept);
        self.field = field;

        cleared
    }

    // ── Gravity and timing ──────────────────────────────────────────

    /// Gravity interval in milliseconds for the current level.
    fn gravity_interval_ms(&self) -> u64 {
        // Level 1 = 1000ms, level 10 ~ 100ms, level 20 ~ 17ms
        // Formula: (0.8 - (level-1) * 0.007)^(level-1) seconds, converted to ms.
        // Simplified: use a lookup-like formula.
        let lvl = self.level.min(30) as f64;
        let seconds = (0.8 - (lvl - 1.0) * 0.007).max(0.01).powf(lvl - 1.0);
        let ms = (seconds * 1000.0) as u64;
        ms.max(10) // Minimum 10ms
    }

    /// Advance the game by the time that actually passed.
    ///
    /// `dt_ms` is what the *loop* reports elapsed, not what the app asked for.
    /// Gravity is driven from an accumulator rather than from a tick count, so
    /// a slow frame drops the piece by the distance the wall clock says it
    /// should have fallen instead of by one row per delivery — a game whose
    /// speed tracked the frame rate would be a different game on every machine,
    /// and a slower one whenever something else on the desktop was busy.
    ///
    /// Returns whether anything the player can see changed, so a heartbeat that
    /// moved nothing does not cost a repaint. During play the clock reads to
    /// the second, so that is at worst once a second rather than once a tick.
    fn tick(&mut self, dt_ms: u64) -> bool {
        if self.status != GameStatus::Playing {
            return false;
        }

        let before = self.elapsed_ms / 1000;
        self.elapsed_ms = self.elapsed_ms.saturating_add(dt_ms);
        let mut changed = self.elapsed_ms / 1000 != before;

        if self.current_piece.is_none() {
            self.spawn_piece();
            return true;
        }

        // Gravity. The loop is bounded by the board rather than by `dt_ms`:
        // `try_move` fails as soon as the piece reaches a surface, so even a
        // multi-second stall cannot spin here for longer than the board is tall.
        self.gravity_accum_ms = self.gravity_accum_ms.saturating_add(dt_ms);
        let interval = self.gravity_interval_ms();
        while self.gravity_accum_ms >= interval {
            self.gravity_accum_ms = self.gravity_accum_ms.saturating_sub(interval);
            if !self.try_move(1, 0) {
                break;
            }
            changed = true;
        }

        // Ask the board whether the piece is resting rather than inferring it
        // from a failed gravity step. The two differ for a piece that *spawns*
        // onto a surface: no gravity step has failed yet, so the inference says
        // it is still falling and the lock delay does not start for another
        // whole gravity interval. On a nearly-full board at level 1 that was an
        // extra second of a piece sitting visibly still and not locking.
        self.update_surface_status();

        if self.piece_on_surface {
            self.lock_accum_ms = self.lock_accum_ms.saturating_add(dt_ms);
            if self.lock_accum_ms >= LOCK_DELAY_MS || self.lock_resets >= self.max_lock_resets {
                self.lock_piece();
                changed = true;
            }
        }

        changed
    }

    // ── Input handling ──────────────────────────────────────────────

    /// Perform an action, whichever input asked for it.
    ///
    /// Returns whether anything the player can see changed, so a key that did
    /// nothing — a move into a wall, hold used twice on one piece — does not
    /// cost a repaint.
    fn apply(&mut self, action: Action) -> bool {
        match action {
            Action::Pause => match self.status {
                GameStatus::Playing => {
                    self.status = GameStatus::Paused;
                    true
                }
                GameStatus::Paused => {
                    self.status = GameStatus::Playing;
                    true
                }
                // Pause is not an escape from a finished game; the message box
                // says "restart" and that is the only way out.
                GameStatus::GameOver => false,
            },
            Action::Restart => {
                if self.status == GameStatus::GameOver {
                    // A new seed, not the same one: replaying the identical
                    // piece order after a loss is a different game from the
                    // one the player thinks they are starting.
                    *self = Self::with_seed(self.seed.wrapping_add(1));
                    true
                } else {
                    false
                }
            }
            _ if self.status != GameStatus::Playing => false,
            Action::MoveLeft => self.try_move(0, -1),
            Action::MoveRight => self.try_move(0, 1),
            Action::SoftDrop => self.soft_drop(),
            Action::HardDrop => {
                self.hard_drop();
                true
            }
            Action::RotateCw => self.try_rotate(true),
            Action::RotateCcw => self.try_rotate(false),
            Action::Hold => {
                if self.hold_used || self.current_piece.is_none() {
                    false
                } else {
                    self.hold_piece();
                    true
                }
            }
        }
    }

    /// The action a key press names, if any.
    fn action_for_key(key: Key) -> Option<Action> {
        Some(match key {
            Key::Left => Action::MoveLeft,
            Key::Right => Action::MoveRight,
            Key::Down => Action::SoftDrop,
            Key::Space => Action::HardDrop,
            Key::Up | Key::Z => Action::RotateCw,
            Key::X => Action::RotateCcw,
            Key::C => Action::Hold,
            Key::P => Action::Pause,
            Key::R => Action::Restart,
            _ => return None,
        })
    }

    /// Handle a key event.
    fn handle_key(&mut self, event: &KeyEvent) -> EventResult {
        if !event.pressed {
            return EventResult::Ignored;
        }
        // Ctrl and Alt combinations belong to the window and the desktop, not
        // to the board: Ctrl-R is "reload" everywhere else and must not be
        // silently eaten here as "restart".
        if event.modifiers.ctrl || event.modifiers.alt || event.modifiers.super_key {
            return EventResult::Ignored;
        }
        match Self::action_for_key(event.key) {
            // The action is consumed whether or not it changed anything: a
            // left arrow into a wall is still the game's key, and letting it
            // fall through to the desktop would move the focus instead.
            Some(action) => {
                self.apply(action);
                EventResult::Consumed
            }
            None => EventResult::Ignored,
        }
    }

    /// What a click at (x, y) would hit, at the size the window is now.
    #[must_use]
    pub fn target_at(&self, x: f32, y: f32) -> Option<Target> {
        self.frame(self.width, self.height).hit_test(x, y)
    }

    fn handle_mouse(&mut self, mouse: &MouseEvent) -> EventResult {
        if !matches!(mouse.kind, MouseEventKind::Press(MouseButton::Left)) {
            return EventResult::Ignored;
        }
        let Some(target) = self.target_at(mouse.x, mouse.y) else {
            return EventResult::Ignored;
        };
        let action = match target {
            Target::Control(action) => action,
            Target::HoldBox => Action::Hold,
            // The box says what it does. Doing anything else on a click would
            // make the label a lie, and there is no second thing it could mean.
            Target::Overlay => match self.status {
                GameStatus::GameOver => Action::Restart,
                _ => Action::Pause,
            },
        };
        self.apply(action);
        EventResult::Consumed
    }

    /// Adopt a new window size. Nothing is recomputed here — the size is all
    /// the layout needs, and it is read fresh on the next frame.
    fn resize(&mut self, width: f32, height: f32) {
        self.width = width.max(1.0);
        self.height = height.max(1.0);
    }

    // ── Rendering ───────────────────────────────────────────────────

    /// Everything the player sees, at the size the window is now.
    ///
    /// The layout is built here and thrown away when the frame ends. Nothing
    /// about where things are is stored on the app, so there is no stale copy
    /// of it to go wrong after a resize — the previous version's problem was
    /// exactly the reverse, with the window size derived from fixed layout
    /// constants and therefore correct at one size only.
    #[must_use]
    pub fn frame(&self, width: f32, height: f32) -> Frame {
        let l = Layout::new(width, height);
        let mut f = Frame::new(l.window.w, l.window.h);

        fill(&mut f, l.window, BASE, 8.0);
        self.draw_header(&mut f, &l);
        self.draw_field(&mut f, &l);
        // Both sidebars stand or fall together, and `hold` is empty exactly
        // when the window was too narrow to give them room.
        if !l.hold.is_empty() {
            self.draw_left(&mut f, &l);
            self.draw_right(&mut f, &l);
        }
        self.draw_overlay(&mut f, &l);

        f
    }

    /// Title, score, level and lines, spread across the header bar.
    fn draw_header(&self, f: &mut Frame, l: &Layout) {
        fill(f, l.header, MANTLE, 6.0);
        let inset = (l.header.h * 0.25).max(2.0);
        // Four even slots rather than the old fixed 150/350-pixel offsets,
        // which overlapped as soon as the window was narrower than the one
        // width they were measured for.
        let slot = l.header.w / 4.0;
        let title = (l.header.h * 0.5).clamp(9.0, TITLE_FONT_SIZE);
        let small = (l.header.h * 0.36).clamp(7.0, HEADER_FONT_SIZE);
        let room = (slot - inset).max(0.0);

        label(
            f,
            l.header.x + inset,
            l.header.y + (l.header.h - title) / 2.0,
            "TETRIS",
            LAVENDER,
            title,
            FontWeightHint::Bold,
            room,
        );
        let fields = [
            (format!("Score: {}", self.score), TEXT_COLOR),
            (format!("Level: {}", self.level), TEAL),
            (format!("Lines: {}", self.lines_cleared), GREEN),
        ];
        for (i, (text, color)) in fields.into_iter().enumerate() {
            label(
                f,
                l.header.x + slot * i.saturating_add(1) as f32,
                l.header.y + (l.header.h - small) / 2.0,
                &text,
                color,
                small,
                FontWeightHint::Bold,
                room,
            );
        }
    }

    /// The playfield: border, grid, placed blocks, ghost, and the live piece.
    fn draw_field(&self, f: &mut Frame, l: &Layout) {
        stroke(
            f,
            Rect::new(
                l.field.x - 2.0,
                l.field.y - 2.0,
                l.field.w + 4.0,
                l.field.h + 4.0,
            ),
            SURFACE2,
            2.0,
            2.0,
        );
        fill(f, l.field, CRUST, 0.0);

        // The grid is a reading aid, not decoration: below a couple of pixels
        // per cell the lines are most of the ink and the board is harder to
        // read with them than without.
        if l.cell >= 4.0 {
            let step = l.cell + l.gap;
            let grid = Color::rgba(49, 50, 68, 60);
            for col in 1..FIELD_COLS {
                let x = l.field.x + col as f32 * step - l.gap / 2.0;
                f.push(RenderCommand::Line {
                    x1: x,
                    y1: l.field.y,
                    x2: x,
                    y2: l.field.bottom(),
                    color: grid,
                    width: 0.5,
                });
            }
            for row in 1..FIELD_ROWS {
                let y = l.field.y + row as f32 * step - l.gap / 2.0;
                f.push(RenderCommand::Line {
                    x1: l.field.x,
                    y1: y,
                    x2: l.field.right(),
                    y2: y,
                    color: grid,
                    width: 0.5,
                });
            }
        }

        for row in HIDDEN_ROWS..TOTAL_ROWS {
            for col in 0..FIELD_COLS {
                if let Some(color) = self.field_get(row, col) {
                    block(f, l.cell_rect(row.saturating_sub(HIDDEN_ROWS), col), color);
                }
            }
        }

        // The ghost is drawn only when it is somewhere else: an outline on top
        // of the piece itself is a smudge, not a hint.
        if self.status == GameStatus::Playing
            && let (Some(piece), Some(ghost_row)) = (&self.current_piece, self.ghost_row())
            && ghost_row > piece.row
        {
            let c = piece.kind.color();
            let faint = Color::rgba(c.r, c.g, c.b, 50);
            let ghost = ActivePiece {
                kind: piece.kind,
                rotation: piece.rotation,
                row: ghost_row,
                col: piece.col,
            };
            for (r, c) in ghost.absolute_cells() {
                if let Some(rect) = visible_cell(l, r, c) {
                    stroke(f, rect, faint, 1.5, 3.0);
                }
            }
        }

        if self.status == GameStatus::Playing
            && let Some(piece) = &self.current_piece
        {
            let color = piece.kind.color();
            for (r, c) in piece.absolute_cells() {
                if let Some(rect) = visible_cell(l, r, c) {
                    block(f, rect, color);
                }
            }
        }
    }

    /// The left column: the HOLD box, which is also a button, and STATS.
    fn draw_left(&self, f: &mut Frame, l: &Layout) {
        fill(f, l.hold, MANTLE, 6.0);
        // Clicking the box does what C does. The panel was already showing the
        // held piece; it costs nothing for it to also be the way to swap.
        f.hit(Target::HoldBox, l.hold);
        let head = l.font * 2.0;
        label(
            f,
            l.hold.x + 4.0,
            l.hold.y + 2.0,
            "HOLD",
            SUBTEXT0,
            l.font,
            FontWeightHint::Bold,
            (l.hold.w - 8.0).max(0.0),
        );
        if let Some(kind) = self.hold_piece {
            // Dimmed once used, because the box still shows a piece the player
            // cannot have this turn and nothing else on screen says so.
            let alpha = if self.hold_used { 100 } else { 255 };
            mini_piece(
                f,
                Rect::new(
                    l.hold.x + 4.0,
                    l.hold.y + head,
                    (l.hold.w - 8.0).max(0.0),
                    (l.hold.h - head - 4.0).max(0.0),
                ),
                kind,
                alpha,
            );
        }

        if l.stats.is_empty() {
            return;
        }
        fill(f, l.stats, MANTLE, 6.0);
        label(
            f,
            l.stats.x + 4.0,
            l.stats.y + 2.0,
            "STATS",
            SUBTEXT0,
            l.font,
            FontWeightHint::Bold,
            (l.stats.w - 8.0).max(0.0),
        );
        let (row_h, shown) = l.stat_rows();
        let value = (row_h * 0.5).clamp(8.0, VALUE_FONT_SIZE);
        let room = (l.stats.w - 8.0).max(0.0);
        let rows: [(&str, String, Color); 5] = [
            ("Score", format!("{}", self.score), YELLOW),
            ("Level", format!("{}", self.level), TEAL),
            ("Lines", format!("{}", self.lines_cleared), GREEN),
            ("Pieces", format!("{}", self.pieces_placed), BLUE),
            ("Time", self.format_time(), LAVENDER),
        ];
        for (i, (name, text, color)) in rows.iter().enumerate().take(shown) {
            let y = l.stats.y + head + i as f32 * row_h;
            label(
                f,
                l.stats.x + 4.0,
                y,
                name,
                OVERLAY0,
                l.font,
                FontWeightHint::Regular,
                room,
            );
            label(
                f,
                l.stats.x + 4.0,
                y + l.font * 1.1,
                text,
                *color,
                value,
                FontWeightHint::Bold,
                room,
            );
        }
    }

    /// The right column: the NEXT previews and the CONTROLS buttons.
    fn draw_right(&self, f: &mut Frame, l: &Layout) {
        let head = l.font * 2.0;
        fill(f, l.next, MANTLE, 6.0);
        label(
            f,
            l.next.x + 4.0,
            l.next.y + 2.0,
            "NEXT",
            SUBTEXT0,
            l.font,
            FontWeightHint::Bold,
            (l.next.w - 8.0).max(0.0),
        );
        let each = (l.next.h - head - 4.0).max(0.0) / 3.0;
        for (i, kind) in self.preview_queue.iter().take(3).enumerate() {
            mini_piece(
                f,
                Rect::new(
                    l.next.x + 4.0,
                    l.next.y + head + i as f32 * each,
                    (l.next.w - 8.0).max(0.0),
                    each,
                ),
                *kind,
                255,
            );
        }

        if l.controls.is_empty() {
            return;
        }
        fill(f, l.controls, MANTLE, 6.0);
        label(
            f,
            l.controls.x + 4.0,
            l.controls.y + 2.0,
            "CONTROLS",
            SUBTEXT0,
            l.font,
            FontWeightHint::Bold,
            (l.controls.w - 8.0).max(0.0),
        );
        let (row_h, shown) = l.control_rows();
        let size = (row_h * 0.62).clamp(6.0, LABEL_FONT_SIZE);
        for (i, (action, text)) in CONTROLS.iter().enumerate().take(shown) {
            let row = l.control_row(i);
            if row.is_empty() {
                continue;
            }
            // The hit box is the whole row, not the glyphs: a player aiming at
            // "C  Hold" is aiming at the line, and a target the width of the
            // text would miss on either side of it.
            f.hit(Target::Control(*action), row);
            label(
                f,
                row.x + 2.0,
                row.y + (row.h - size) / 2.0,
                text,
                OVERLAY0,
                size,
                FontWeightHint::Regular,
                (row.w - 4.0).max(0.0),
            );
        }
    }

    /// The pause / game-over message box, drawn over everything else.
    ///
    /// Last, so its hit box wins: `hit_test` walks the recorded boxes in
    /// reverse, and a message box that a click passes straight through to the
    /// board behind it would be a lie about being modal.
    fn draw_overlay(&self, f: &mut Frame, l: &Layout) {
        let Some((title, subtitle)) = (match self.status {
            GameStatus::Paused => Some(("PAUSED", "Click or press P to resume")),
            GameStatus::GameOver => Some(("GAME OVER", "Click or press R to restart")),
            GameStatus::Playing => None,
        }) else {
            return;
        };

        fill(f, l.window, Color::rgba(17, 17, 27, 180), 0.0);
        fill(f, l.overlay, SURFACE0, 10.0);
        stroke(f, l.overlay, LAVENDER, 2.0, 10.0);
        f.hit(Target::Overlay, l.overlay);

        let big = (l.overlay.h * 0.26).clamp(10.0, 24.0);
        let small = (l.overlay.h * 0.15).clamp(7.0, 14.0);
        centred(
            f,
            l.overlay,
            l.overlay.y + l.overlay.h * 0.20,
            title,
            TEXT_COLOR,
            big,
            FontWeightHint::Bold,
        );
        centred(
            f,
            l.overlay,
            l.overlay.y + l.overlay.h * 0.58,
            subtitle,
            SUBTEXT0,
            small,
            FontWeightHint::Regular,
        );
    }

    /// Format elapsed time as mm:ss.
    fn format_time(&self) -> String {
        let total_seconds = self.elapsed_ms / 1000;
        let minutes = total_seconds / 60;
        let seconds = total_seconds % 60;
        format!("{minutes:02}:{seconds:02}")
    }
}

// ── Drawing helpers ─────────────────────────────────────────────────

fn fill(f: &mut Frame, rect: Rect, color: Color, radius: f32) {
    f.push(RenderCommand::FillRect {
        x: rect.x,
        y: rect.y,
        width: rect.w,
        height: rect.h,
        color,
        corner_radii: CornerRadii::all(radius),
    });
}

fn stroke(f: &mut Frame, rect: Rect, color: Color, line_width: f32, radius: f32) {
    f.push(RenderCommand::StrokeRect {
        x: rect.x,
        y: rect.y,
        width: rect.w,
        height: rect.h,
        color,
        line_width,
        corner_radii: CornerRadii::all(radius),
    });
}

fn label(
    f: &mut Frame,
    x: f32,
    y: f32,
    text: &str,
    color: Color,
    font_size: f32,
    font_weight: FontWeightHint,
    max_width: f32,
) {
    f.push(RenderCommand::Text {
        x,
        y,
        text: String::from(text),
        color,
        font_size,
        font_weight,
        max_width: Some(max_width),
        overflow: TextOverflow::Ellipsis,
    });
}

/// Text placed near the middle of `area`.
///
/// The width is estimated from the character count rather than measured — the
/// renderer owns the font and this code does not. It is only ever used for the
/// two lines in the message box, where being a few pixels off centre is
/// invisible and a wrong *layout* would not be.
fn centred(
    f: &mut Frame,
    area: Rect,
    y: f32,
    text: &str,
    color: Color,
    font_size: f32,
    font_weight: FontWeightHint,
) {
    let est = text.chars().count() as f32 * font_size * 0.55;
    label(
        f,
        area.x + ((area.w - est) / 2.0).max(0.0),
        y,
        text,
        color,
        font_size,
        font_weight,
        (area.w - 8.0).max(0.0),
    );
}

/// One filled cell, with a highlight along its top half.
fn block(f: &mut Frame, rect: Rect, color: Color) {
    fill(f, rect, color, (rect.w * 0.12).clamp(0.0, 3.0));
    // Skipped when the cell is too small to hold both: a 3-pixel cell with a
    // 1-pixel inset on every side is a highlight and nothing else.
    if rect.w < 6.0 || rect.h < 6.0 {
        return;
    }
    f.push(RenderCommand::FillRect {
        x: rect.x + 1.0,
        y: rect.y + 1.0,
        width: rect.w - 2.0,
        height: rect.h / 2.0 - 1.0,
        color: Color::rgba(255, 255, 255, 30),
        corner_radii: CornerRadii {
            top_left: 2.0,
            top_right: 2.0,
            bottom_right: 0.0,
            bottom_left: 0.0,
        },
    });
}

/// The drawn box of a piece cell, or `None` when it is off the field or in the
/// hidden rows above it, where pieces spawn and must not be seen.
fn visible_cell(l: &Layout, row: i8, col: i8) -> Option<Rect> {
    let (Ok(r), Ok(c)) = (usize::try_from(row), usize::try_from(col)) else {
        return None;
    };
    if r < HIDDEN_ROWS || r >= TOTAL_ROWS || c >= FIELD_COLS {
        return None;
    }
    Some(l.cell_rect(r.saturating_sub(HIDDEN_ROWS), c))
}

/// A piece drawn small and centred in `area`, for the HOLD and NEXT boxes.
///
/// Sized from the box rather than from a constant, so a short window shrinks
/// the previews instead of letting them run over the panel below.
fn mini_piece(f: &mut Frame, area: Rect, kind: PieceKind, alpha: u8) {
    if area.is_empty() {
        return;
    }
    let cells = kind.cells(0);
    let min_r = cells.iter().fold(i8::MAX, |a, (r, _)| a.min(*r));
    let max_r = cells.iter().fold(i8::MIN, |a, (r, _)| a.max(*r));
    let min_c = cells.iter().fold(i8::MAX, |a, (_, c)| a.min(*c));
    let max_c = cells.iter().fold(i8::MIN, |a, (_, c)| a.max(*c));
    let rows = f32::from(max_r.saturating_sub(min_r).saturating_add(1));
    let cols = f32::from(max_c.saturating_sub(min_c).saturating_add(1));

    let cell = (area.w / cols).min(area.h / rows).clamp(1.0, MINI_CELL);
    let gap = (cell * 0.08).clamp(0.0, MINI_GAP);
    let step = cell + gap;
    let ox = area.x + (area.w - (cols * step - gap)) / 2.0;
    let oy = area.y + (area.h - (rows * step - gap)) / 2.0;

    let base = kind.color();
    let color = if alpha < 255 {
        Color::rgba(base.r, base.g, base.b, alpha)
    } else {
        base
    };
    for (dr, dc) in cells {
        f.push(RenderCommand::FillRect {
            x: ox + f32::from(dc.saturating_sub(min_c)) * step,
            y: oy + f32::from(dr.saturating_sub(min_r)) * step,
            width: cell,
            height: cell,
            color,
            corner_radii: CornerRadii::all((cell * 0.14).clamp(0.0, 2.0)),
        });
    }
}

// ── Window wiring ───────────────────────────────────────────────────

/// One body for every event, whoever delivers it.
///
/// [`App::on_event`] and the [`Probe`] impl both call this, which is what
/// makes a test that clicks a CONTROLS row a test of the shipped program
/// rather than of a parallel implementation written for the test.
fn handle_event(app: &mut TetrisApp, event: &Event) -> EventResult {
    match event {
        Event::Key(key) => app.handle_key(key),
        Event::Mouse(mouse) => app.handle_mouse(mouse),
        Event::Resize { width, height } => {
            app.resize(*width as f32, *height as f32);
            EventResult::Consumed
        }
        // A window that comes back after being hidden should not be handed a
        // gravity debt for the time it was away, and the player should not
        // return to a board that fell while they were not looking.
        Event::FocusOut => {
            if app.status == GameStatus::Playing {
                app.status = GameStatus::Paused;
                EventResult::Consumed
            } else {
                EventResult::Ignored
            }
        }
        Event::Tick { elapsed_ms } => {
            if app.tick(*elapsed_ms) {
                EventResult::Consumed
            } else {
                EventResult::Ignored
            }
        }
        _ => EventResult::Ignored,
    }
}

impl App for TetrisApp {
    fn on_event(&mut self, event: &Event) -> Response {
        match handle_event(self, event) {
            EventResult::Consumed => Response::Redraw,
            EventResult::Ignored => Response::Idle,
        }
    }

    fn render(&mut self, width: f32, height: f32) -> RenderTree {
        self.resize(width, height);
        self.frame(width, height).into_tree()
    }

    fn title(&self) -> String {
        String::from("Tetris")
    }

    fn app_id(&self) -> String {
        String::from("tetris")
    }

    fn initial_size(&self) -> (u32, u32) {
        (640, 720)
    }

    /// How often the game wants to be woken.
    ///
    /// Not the gravity interval. Gravity is one second at level 1 and a
    /// hundredth of that at level 20, but the *lock delay* is 500 ms and the
    /// clock in STATS reads in whole seconds, so a tick that arrived only when
    /// a piece was due to fall would leave both visibly late. Asking for a
    /// steady 50 ms and advancing by the `elapsed_ms` each tick actually
    /// carries keeps every timer in the app to within one frame, and costs
    /// nothing while paused because the interval is dropped entirely.
    fn tick_interval(&self) -> Option<Duration> {
        match self.status {
            GameStatus::Playing => Some(Duration::from_millis(50)),
            GameStatus::Paused | GameStatus::GameOver => None,
        }
    }
}

impl Probe for TetrisApp {
    type Target = Target;
    type Outcome = EventResult;

    const SIZE: (f32, f32) = (640.0, 720.0);

    fn draw(&self, size: (f32, f32)) -> Frame {
        self.frame(size.0, size.1)
    }

    fn click_at(&mut self, x: f32, y: f32, button: MouseButton, size: (f32, f32)) -> EventResult {
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

    fn key_at(&mut self, key: &KeyEvent, size: (f32, f32)) -> EventResult {
        self.resize(size.0, size.1);
        handle_event(self, &Event::Key(key.clone()))
    }
}

fn main() -> ExitCode {
    let mut app = TetrisApp::new();
    app::launch("tetris", &mut app)
}

// ═══════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════
#[cfg(test)]
mod tests {
    // A test that indexes out of range should fail loudly and point at the line
    // that did it -- that is the diagnosis. The defensive lints exist to keep
    // panics out of code that runs on a user's data, which this is not.
    #![allow(
        clippy::indexing_slicing,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::float_cmp,
        clippy::arithmetic_side_effects
    )]

    use super::*;

    // ── Helper functions ────────────────────────────────────────────

    fn make_key_event(key: Key) -> KeyEvent {
        KeyEvent {
            key,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        }
    }

    fn press_key(app: &mut TetrisApp, key: Key) {
        let event = make_key_event(key);
        app.handle_key(&event);
    }

    /// Fill a specific row in the field with a color, leaving one column empty.
    fn fill_row_except(app: &mut TetrisApp, row: usize, except_col: usize) {
        for col in 0..FIELD_COLS {
            if col != except_col {
                app.field_set(row, col, Some(BLUE));
            }
        }
    }

    /// Fill a specific row completely.
    fn fill_row(app: &mut TetrisApp, row: usize) {
        for col in 0..FIELD_COLS {
            app.field_set(row, col, Some(BLUE));
        }
    }

    /// Create an app with no current piece for field manipulation tests.
    fn app_no_piece() -> TetrisApp {
        let mut app = TetrisApp::with_seed(42);
        app.current_piece = None;
        app
    }

    // ── Piece shape tests ───────────────────────────────────────────

    #[test]
    fn test_i_piece_shape_rotation_0() {
        let cells = PieceKind::I.cells(0);
        // I piece rotation 0: horizontal bar in second row
        assert_eq!(cells, [(1, 0), (1, 1), (1, 2), (1, 3)]);
    }

    #[test]
    fn test_i_piece_shape_rotation_1() {
        let cells = PieceKind::I.cells(1);
        // I piece rotation 1: vertical bar in third column
        assert_eq!(cells, [(0, 2), (1, 2), (2, 2), (3, 2)]);
    }

    #[test]
    fn test_i_piece_shape_rotation_2() {
        let cells = PieceKind::I.cells(2);
        assert_eq!(cells, [(2, 0), (2, 1), (2, 2), (2, 3)]);
    }

    #[test]
    fn test_i_piece_shape_rotation_3() {
        let cells = PieceKind::I.cells(3);
        assert_eq!(cells, [(0, 1), (1, 1), (2, 1), (3, 1)]);
    }

    #[test]
    fn test_o_piece_shape_all_rotations() {
        // O piece is the same in all rotations
        let expected = [(0, 0), (0, 1), (1, 0), (1, 1)];
        for rot in 0..4 {
            assert_eq!(PieceKind::O.cells(rot), expected, "O piece rotation {rot}");
        }
    }

    #[test]
    fn test_t_piece_all_rotations() {
        assert_eq!(PieceKind::T.cells(0), [(0, 1), (1, 0), (1, 1), (1, 2)]);
        assert_eq!(PieceKind::T.cells(1), [(0, 1), (1, 1), (1, 2), (2, 1)]);
        assert_eq!(PieceKind::T.cells(2), [(1, 0), (1, 1), (1, 2), (2, 1)]);
        assert_eq!(PieceKind::T.cells(3), [(0, 1), (1, 0), (1, 1), (2, 1)]);
    }

    #[test]
    fn test_s_piece_all_rotations() {
        assert_eq!(PieceKind::S.cells(0), [(0, 1), (0, 2), (1, 0), (1, 1)]);
        assert_eq!(PieceKind::S.cells(1), [(0, 1), (1, 1), (1, 2), (2, 2)]);
        assert_eq!(PieceKind::S.cells(2), [(1, 1), (1, 2), (2, 0), (2, 1)]);
        assert_eq!(PieceKind::S.cells(3), [(0, 0), (1, 0), (1, 1), (2, 1)]);
    }

    #[test]
    fn test_z_piece_all_rotations() {
        assert_eq!(PieceKind::Z.cells(0), [(0, 0), (0, 1), (1, 1), (1, 2)]);
        assert_eq!(PieceKind::Z.cells(1), [(0, 2), (1, 1), (1, 2), (2, 1)]);
        assert_eq!(PieceKind::Z.cells(2), [(1, 0), (1, 1), (2, 1), (2, 2)]);
        assert_eq!(PieceKind::Z.cells(3), [(0, 1), (1, 0), (1, 1), (2, 0)]);
    }

    #[test]
    fn test_j_piece_all_rotations() {
        assert_eq!(PieceKind::J.cells(0), [(0, 0), (1, 0), (1, 1), (1, 2)]);
        assert_eq!(PieceKind::J.cells(1), [(0, 1), (0, 2), (1, 1), (2, 1)]);
        assert_eq!(PieceKind::J.cells(2), [(1, 0), (1, 1), (1, 2), (2, 2)]);
        assert_eq!(PieceKind::J.cells(3), [(0, 1), (1, 1), (2, 0), (2, 1)]);
    }

    #[test]
    fn test_l_piece_all_rotations() {
        assert_eq!(PieceKind::L.cells(0), [(0, 2), (1, 0), (1, 1), (1, 2)]);
        assert_eq!(PieceKind::L.cells(1), [(0, 1), (1, 1), (2, 1), (2, 2)]);
        assert_eq!(PieceKind::L.cells(2), [(1, 0), (1, 1), (1, 2), (2, 0)]);
        assert_eq!(PieceKind::L.cells(3), [(0, 0), (0, 1), (1, 1), (2, 1)]);
    }

    #[test]
    fn test_piece_bounding_sizes() {
        assert_eq!(PieceKind::I.bounding_size(), (4, 4));
        assert_eq!(PieceKind::O.bounding_size(), (2, 2));
        assert_eq!(PieceKind::T.bounding_size(), (3, 3));
        assert_eq!(PieceKind::S.bounding_size(), (3, 3));
        assert_eq!(PieceKind::Z.bounding_size(), (3, 3));
        assert_eq!(PieceKind::J.bounding_size(), (3, 3));
        assert_eq!(PieceKind::L.bounding_size(), (3, 3));
    }

    #[test]
    fn test_piece_colors_are_distinct() {
        let colors: Vec<Color> = PieceKind::ALL.iter().map(|k| k.color()).collect();
        for i in 0..colors.len() {
            for j in (i + 1)..colors.len() {
                assert_ne!(
                    colors[i],
                    colors[j],
                    "Pieces {:?} and {:?} share a color",
                    PieceKind::ALL[i],
                    PieceKind::ALL[j]
                );
            }
        }
    }

    // ── Collision detection tests ───────────────────────────────────

    #[test]
    fn test_valid_position_at_spawn() {
        let app = TetrisApp::with_seed(42);
        assert!(app.current_piece.is_some());
        assert!(app.is_valid_position(app.current_piece.as_ref().unwrap()));
    }

    #[test]
    fn test_collision_with_left_wall() {
        let app = TetrisApp::with_seed(42);
        if let Some(piece) = &app.current_piece {
            // Move far left until collision
            let mut test = piece.clone();
            test.col = -5;
            assert!(!app.is_valid_position(&test));
        }
    }

    #[test]
    fn test_collision_with_right_wall() {
        let app = TetrisApp::with_seed(42);
        if let Some(piece) = &app.current_piece {
            let mut test = piece.clone();
            test.col = FIELD_COLS as i8 + 1;
            assert!(!app.is_valid_position(&test));
        }
    }

    #[test]
    fn test_collision_with_floor() {
        let app = TetrisApp::with_seed(42);
        if let Some(piece) = &app.current_piece {
            let mut test = piece.clone();
            test.row = TOTAL_ROWS as i8;
            assert!(!app.is_valid_position(&test));
        }
    }

    #[test]
    fn test_collision_with_placed_block() {
        let mut app = TetrisApp::with_seed(42);
        // Place a block in the middle of the field
        app.field_set(TOTAL_ROWS - 1, 5, Some(RED));
        let piece = ActivePiece {
            kind: PieceKind::O,
            rotation: 0,
            row: (TOTAL_ROWS - 2) as i8,
            col: 5,
        };
        assert!(!app.is_valid_position(&piece));
    }

    #[test]
    fn test_no_collision_with_empty_field() {
        let app = TetrisApp::with_seed(42);
        let piece = ActivePiece {
            kind: PieceKind::O,
            rotation: 0,
            row: 10,
            col: 4,
        };
        assert!(app.is_valid_position(&piece));
    }

    // ── Movement tests ──────────────────────────────────────────────

    #[test]
    fn test_move_left() {
        let mut app = TetrisApp::with_seed(42);
        let orig_col = app.current_piece.as_ref().unwrap().col;
        assert!(app.try_move(0, -1));
        assert_eq!(app.current_piece.as_ref().unwrap().col, orig_col - 1);
    }

    #[test]
    fn test_move_right() {
        let mut app = TetrisApp::with_seed(42);
        let orig_col = app.current_piece.as_ref().unwrap().col;
        assert!(app.try_move(0, 1));
        assert_eq!(app.current_piece.as_ref().unwrap().col, orig_col + 1);
    }

    #[test]
    fn test_move_down() {
        let mut app = TetrisApp::with_seed(42);
        let orig_row = app.current_piece.as_ref().unwrap().row;
        assert!(app.try_move(1, 0));
        assert_eq!(app.current_piece.as_ref().unwrap().row, orig_row + 1);
    }

    #[test]
    fn test_cannot_move_through_wall() {
        let mut app = TetrisApp::with_seed(42);
        // Move all the way left
        for _ in 0..20 {
            app.try_move(0, -1);
        }
        let col = app.current_piece.as_ref().unwrap().col;
        // Should not be able to move further left
        assert!(!app.try_move(0, -1) || app.current_piece.as_ref().unwrap().col >= 0);
        // Actually verify position is at wall
        let final_col = app.current_piece.as_ref().unwrap().col;
        assert!(final_col <= col);
    }

    #[test]
    fn test_cannot_move_when_paused() {
        let mut app = TetrisApp::with_seed(42);
        app.status = GameStatus::Paused;
        let orig_col = app.current_piece.as_ref().unwrap().col;
        assert!(!app.try_move(0, -1));
        assert_eq!(app.current_piece.as_ref().unwrap().col, orig_col);
    }

    #[test]
    fn test_cannot_move_when_game_over() {
        let mut app = TetrisApp::with_seed(42);
        app.status = GameStatus::GameOver;
        assert!(!app.try_move(0, -1));
    }

    // ── Rotation tests ──────────────────────────────────────────────

    #[test]
    fn test_rotate_clockwise() {
        let mut app = TetrisApp::with_seed(42);
        if let Some(piece) = &app.current_piece {
            let orig_rot = piece.rotation;
            if piece.kind != PieceKind::O {
                assert!(app.try_rotate(true));
                assert_eq!(
                    app.current_piece.as_ref().unwrap().rotation,
                    (orig_rot + 1) % 4
                );
            }
        }
    }

    #[test]
    fn test_rotate_counterclockwise() {
        let mut app = TetrisApp::with_seed(42);
        if let Some(piece) = &app.current_piece {
            let orig_rot = piece.rotation;
            if piece.kind != PieceKind::O {
                assert!(app.try_rotate(false));
                assert_eq!(
                    app.current_piece.as_ref().unwrap().rotation,
                    (orig_rot + 3) % 4
                );
            }
        }
    }

    #[test]
    fn test_full_rotation_cycle() {
        let mut app = TetrisApp::with_seed(42);
        // Move piece down a bit to have room
        for _ in 0..5 {
            app.try_move(1, 0);
        }
        if app.current_piece.as_ref().unwrap().kind != PieceKind::O {
            for _ in 0..4 {
                assert!(app.try_rotate(true));
            }
            // After 4 CW rotations, should be back to 0
            assert_eq!(app.current_piece.as_ref().unwrap().rotation, 0);
        }
    }

    #[test]
    fn test_o_piece_rotation_unchanged() {
        let mut app = TetrisApp::with_seed(42);
        // Force an O piece
        app.current_piece = Some(ActivePiece {
            kind: PieceKind::O,
            rotation: 0,
            row: 10,
            col: 4,
        });
        let cells_before = app.current_piece.as_ref().unwrap().absolute_cells();
        app.try_rotate(true);
        let cells_after = app.current_piece.as_ref().unwrap().absolute_cells();
        assert_eq!(cells_before, cells_after);
    }

    #[test]
    fn test_wall_kick_near_left_wall() {
        let mut app = TetrisApp::with_seed(42);
        // Place an I piece near the left wall in vertical orientation
        app.current_piece = Some(ActivePiece {
            kind: PieceKind::I,
            rotation: 1, // vertical
            row: 10,
            col: -1, // partially off-screen
        });
        // Try to rotate - should wall kick
        let result = app.try_rotate(true);
        // The rotation may or may not succeed depending on wall kick data,
        // but at least it shouldn't panic.
        let _ = result;
    }

    #[test]
    fn test_wall_kick_near_right_wall() {
        let mut app = TetrisApp::with_seed(42);
        app.current_piece = Some(ActivePiece {
            kind: PieceKind::T,
            rotation: 0,
            row: 10,
            col: (FIELD_COLS - 3) as i8,
        });
        // Should be able to rotate with kicks
        let original_rot = app.current_piece.as_ref().unwrap().rotation;
        app.try_rotate(true);
        // Verify rotation happened (or piece is still valid)
        assert!(app.is_valid_position(app.current_piece.as_ref().unwrap()));
        let new_rot = app.current_piece.as_ref().unwrap().rotation;
        // For T piece at right wall, rotation 0->1 should succeed with or without kick
        assert_ne!(original_rot, new_rot);
    }

    #[test]
    fn test_rotation_blocked_when_no_valid_kick() {
        let mut app = TetrisApp::with_seed(42);
        // Create a very tight space where rotation is impossible
        // Fill most of the field, leaving only a small gap
        for row in 8..TOTAL_ROWS {
            for col in 0..FIELD_COLS {
                if !(row == 10 && (col == 4 || col == 5)) {
                    app.field_set(row, col, Some(BLUE));
                }
            }
        }
        // Place a piece in the tight gap
        app.current_piece = Some(ActivePiece {
            kind: PieceKind::I,
            rotation: 0,
            row: 8,
            col: 3,
        });
        // This rotation should fail because there's no room
        let rot_before = app.current_piece.as_ref().unwrap().rotation;
        let result = app.try_rotate(true);
        if !result {
            assert_eq!(app.current_piece.as_ref().unwrap().rotation, rot_before);
        }
    }

    // ── Soft drop tests ─────────────────────────────────────────────

    #[test]
    fn test_soft_drop_moves_down() {
        let mut app = TetrisApp::with_seed(42);
        let orig_row = app.current_piece.as_ref().unwrap().row;
        assert!(app.soft_drop());
        assert_eq!(app.current_piece.as_ref().unwrap().row, orig_row + 1);
    }

    #[test]
    fn test_soft_drop_scores_one_per_cell() {
        let mut app = TetrisApp::with_seed(42);
        let score_before = app.score;
        app.soft_drop();
        assert_eq!(app.score, score_before + 1);
    }

    #[test]
    fn test_soft_drop_multiple() {
        let mut app = TetrisApp::with_seed(42);
        let score_before = app.score;
        for _ in 0..5 {
            app.soft_drop();
        }
        assert_eq!(app.score, score_before + 5);
    }

    // ── Hard drop tests ─────────────────────────────────────────────

    #[test]
    fn test_hard_drop_locks_piece() {
        let mut app = TetrisApp::with_seed(42);
        let kind = app.current_piece.as_ref().unwrap().kind;
        app.hard_drop();
        // After hard drop, the piece should be locked and a new piece spawned
        // (or the same piece reference changed)
        assert!(app.pieces_placed >= 1);
        // The current piece should be a new one (or game over)
        if app.status == GameStatus::Playing {
            // New piece was spawned
            assert!(app.current_piece.is_some());
        }
        let _ = kind;
    }

    #[test]
    fn test_hard_drop_scores_two_per_cell() {
        let mut app = TetrisApp::with_seed(42);
        let orig_row = app.current_piece.as_ref().unwrap().row;
        let ghost = app.ghost_row().unwrap();
        let expected_cells = (ghost - orig_row) as u32;
        let score_before = app.score;
        app.hard_drop();
        assert_eq!(app.score, score_before + expected_cells * 2);
    }

    #[test]
    fn test_hard_drop_piece_reaches_bottom() {
        let mut app = TetrisApp::with_seed(42);
        app.hard_drop();
        // At least one cell in the bottom portion of the field should be filled
        let mut found = false;
        for row in (TOTAL_ROWS - 4)..TOTAL_ROWS {
            for col in 0..FIELD_COLS {
                if app.field_get(row, col).is_some() {
                    found = true;
                }
            }
        }
        assert!(found, "Hard drop should place piece near bottom of field");
    }

    // ── Ghost piece tests ───────────────────────────────────────────

    #[test]
    fn test_ghost_row_exists() {
        let app = TetrisApp::with_seed(42);
        assert!(app.ghost_row().is_some());
    }

    #[test]
    fn test_ghost_row_below_current() {
        let app = TetrisApp::with_seed(42);
        let piece_row = app.current_piece.as_ref().unwrap().row;
        let ghost = app.ghost_row().unwrap();
        assert!(ghost >= piece_row);
    }

    #[test]
    fn test_ghost_row_is_valid_position() {
        let app = TetrisApp::with_seed(42);
        let piece = app.current_piece.as_ref().unwrap();
        let ghost = app.ghost_row().unwrap();
        let ghost_piece = ActivePiece {
            kind: piece.kind,
            rotation: piece.rotation,
            row: ghost,
            col: piece.col,
        };
        assert!(app.is_valid_position(&ghost_piece));
        // One row below should NOT be valid
        let below = ghost_piece.moved(1, 0);
        assert!(!app.is_valid_position(&below));
    }

    // ── Line clearing tests ─────────────────────────────────────────

    #[test]
    fn test_clear_single_line() {
        let mut app = app_no_piece();
        let bottom_row = TOTAL_ROWS - 1;
        fill_row(&mut app, bottom_row);
        let cleared = app.clear_lines();
        assert_eq!(cleared, 1);
        // Row should now be empty
        for col in 0..FIELD_COLS {
            assert!(app.field_get(bottom_row, col).is_none());
        }
    }

    #[test]
    fn test_clear_double_line() {
        let mut app = app_no_piece();
        fill_row(&mut app, TOTAL_ROWS - 1);
        fill_row(&mut app, TOTAL_ROWS - 2);
        let cleared = app.clear_lines();
        assert_eq!(cleared, 2);
    }

    #[test]
    fn test_clear_triple_line() {
        let mut app = app_no_piece();
        fill_row(&mut app, TOTAL_ROWS - 1);
        fill_row(&mut app, TOTAL_ROWS - 2);
        fill_row(&mut app, TOTAL_ROWS - 3);
        let cleared = app.clear_lines();
        assert_eq!(cleared, 3);
    }

    #[test]
    fn test_clear_tetris() {
        let mut app = app_no_piece();
        for i in 0..4 {
            fill_row(&mut app, TOTAL_ROWS - 1 - i);
        }
        let cleared = app.clear_lines();
        assert_eq!(cleared, 4);
    }

    #[test]
    fn test_clear_line_shifts_above_down() {
        let mut app = app_no_piece();
        // Place a block above the line to clear
        let above_row = TOTAL_ROWS - 3;
        app.field_set(above_row, 5, Some(GREEN));
        // Fill the bottom two rows
        fill_row(&mut app, TOTAL_ROWS - 1);
        fill_row(&mut app, TOTAL_ROWS - 2);
        app.clear_lines();
        // The block that was at above_row should now be at above_row + 2
        assert!(app.field_get(above_row + 2, 5).is_some());
        assert!(app.field_get(above_row, 5).is_none());
    }

    #[test]
    fn test_no_clear_incomplete_row() {
        let mut app = app_no_piece();
        fill_row_except(&mut app, TOTAL_ROWS - 1, 5);
        let cleared = app.clear_lines();
        assert_eq!(cleared, 0);
    }

    #[test]
    fn test_clear_non_contiguous_rows() {
        let mut app = app_no_piece();
        // Fill bottom row and row 3 from bottom (leaving gap in between)
        fill_row(&mut app, TOTAL_ROWS - 1);
        fill_row(&mut app, TOTAL_ROWS - 3);
        // Put something in the middle row to make it incomplete
        fill_row_except(&mut app, TOTAL_ROWS - 2, 3);
        let cleared = app.clear_lines();
        assert_eq!(cleared, 2);
    }

    /// Every row of the field, as `#` for occupied and `.` for empty, so a
    /// clear can be asserted against the board it leaves rather than against
    /// the number it returns. The count was never the bug; the board was.
    fn board(app: &TetrisApp) -> Vec<String> {
        (0..TOTAL_ROWS)
            .map(|row| {
                (0..FIELD_COLS)
                    .map(|col| {
                        if app.field_get(row, col).is_some() {
                            '#'
                        } else {
                            '.'
                        }
                    })
                    .collect()
            })
            .collect()
    }

    /// Regression: `clear_lines` used to walk a list of full-row indices and
    /// shift the field down once per entry. The first shift moved the rows the
    /// *later* indices named, so the second entry deleted an innocent row and
    /// left the full one behind as a solid line no play could ever remove.
    /// Every double, triple and tetris leaked one, and the board filled from
    /// the bottom until the game ended on its own.
    #[test]
    fn a_multi_line_clear_leaves_no_full_row_behind() {
        for gap in [0usize, 1, 2, 3] {
            let mut app = app_no_piece();
            let low = TOTAL_ROWS - 1;
            let high = low - 1 - gap;
            fill_row(&mut app, low);
            fill_row(&mut app, high);

            let cleared = app.clear_lines();
            assert_eq!(cleared, 2, "two full rows with a gap of {gap} between them");

            for row in 0..TOTAL_ROWS {
                assert!(
                    (0..FIELD_COLS).any(|col| app.field_get(row, col).is_none()),
                    "row {row} is still full after the clear (gap {gap}); board {:?}",
                    board(&app)
                );
            }
        }
    }

    /// The survivors must keep their order and their contents, not merely their
    /// count — a rebuild that dropped or reordered a row would pass the "no
    /// full row left" check above while scrambling the stack the player built.
    #[test]
    fn the_rows_that_survive_a_clear_keep_their_order_and_fall_together() {
        let mut app = app_no_piece();
        // Three markers in distinct columns, interleaved with two full rows, so
        // a shift that is off by one row shows up as the wrong column order.
        app.field_set(TOTAL_ROWS - 6, 0, Some(GREEN));
        fill_row(&mut app, TOTAL_ROWS - 5);
        app.field_set(TOTAL_ROWS - 4, 1, Some(BLUE));
        fill_row(&mut app, TOTAL_ROWS - 3);
        app.field_set(TOTAL_ROWS - 2, 2, Some(RED));

        assert_eq!(app.clear_lines(), 2);

        // Two rows went, so the survivors close up from the top: the marker
        // that was three rows above the lowest full row is now directly above
        // the next marker, and the columns still read 0, 1, 2 downwards.
        assert_eq!(
            app.field_get(TOTAL_ROWS - 4, 0),
            Some(GREEN),
            "board {:?}",
            board(&app)
        );
        assert_eq!(
            app.field_get(TOTAL_ROWS - 3, 1),
            Some(BLUE),
            "board {:?}",
            board(&app)
        );
        assert_eq!(
            app.field_get(TOTAL_ROWS - 2, 2),
            Some(RED),
            "board {:?}",
            board(&app)
        );

        // And nothing was invented above them.
        for row in 0..TOTAL_ROWS - 4 {
            for col in 0..FIELD_COLS {
                assert_eq!(
                    app.field_get(row, col),
                    None,
                    "row {row} col {col} should be empty; board {:?}",
                    board(&app)
                );
            }
        }
    }

    /// A tetris is four *contiguous* rows, which the old shift-in-place also
    /// got wrong — contiguity does not save it, because the indices still move.
    #[test]
    fn a_tetris_empties_all_four_rows_and_drops_what_sat_on_them() {
        let mut app = app_no_piece();
        app.field_set(TOTAL_ROWS - 5, 7, Some(MAUVE));
        for i in 0..4 {
            fill_row(&mut app, TOTAL_ROWS - 1 - i);
        }

        assert_eq!(app.clear_lines(), 4);

        assert_eq!(
            app.field_get(TOTAL_ROWS - 1, 7),
            Some(MAUVE),
            "the block above a tetris falls all four rows; board {:?}",
            board(&app)
        );
        for row in 0..TOTAL_ROWS {
            let occupied = (0..FIELD_COLS)
                .filter(|&col| app.field_get(row, col).is_some())
                .count();
            let expected = usize::from(row == TOTAL_ROWS - 1);
            assert_eq!(
                occupied,
                expected,
                "row {row} holds {occupied} blocks; board {:?}",
                board(&app)
            );
        }
    }

    /// The whole board full is the extreme case of the same rebuild: every row
    /// goes, nothing survives, and the field must come back empty rather than
    /// keeping a copy of the last row it shifted.
    #[test]
    fn clearing_every_row_at_once_empties_the_field() {
        let mut app = app_no_piece();
        for row in 0..TOTAL_ROWS {
            fill_row(&mut app, row);
        }
        assert_eq!(app.clear_lines(), TOTAL_ROWS);
        assert_eq!(
            app.field.len(),
            TOTAL_ROWS * FIELD_COLS,
            "field keeps its size"
        );
        assert!(app.field.iter().all(Option::is_none), "field is empty");
    }

    // ── Scoring tests ───────────────────────────────────────────────

    #[test]
    fn test_single_line_score() {
        assert_eq!(line_clear_score(1, 1, TSpinKind::None), 100);
    }

    #[test]
    fn test_double_line_score() {
        assert_eq!(line_clear_score(2, 1, TSpinKind::None), 300);
    }

    #[test]
    fn test_triple_line_score() {
        assert_eq!(line_clear_score(3, 1, TSpinKind::None), 500);
    }

    #[test]
    fn test_tetris_score() {
        assert_eq!(line_clear_score(4, 1, TSpinKind::None), 800);
    }

    #[test]
    fn test_score_scales_with_level() {
        assert_eq!(line_clear_score(1, 5, TSpinKind::None), 500);
        assert_eq!(line_clear_score(4, 3, TSpinKind::None), 2400);
    }

    #[test]
    fn test_tspin_single_score() {
        assert_eq!(line_clear_score(1, 1, TSpinKind::Full), 800);
    }

    #[test]
    fn test_tspin_double_score() {
        assert_eq!(line_clear_score(2, 1, TSpinKind::Full), 1200);
    }

    #[test]
    fn test_tspin_triple_score() {
        assert_eq!(line_clear_score(3, 1, TSpinKind::Full), 1600);
    }

    #[test]
    fn test_tspin_mini_score() {
        assert_eq!(line_clear_score(1, 1, TSpinKind::Mini), 200);
    }

    #[test]
    fn test_tspin_no_clear_score() {
        assert_eq!(line_clear_score(0, 1, TSpinKind::Full), 400);
    }

    #[test]
    fn test_soft_drop_score_fn() {
        assert_eq!(soft_drop_score(5), 5);
    }

    #[test]
    fn test_hard_drop_score_fn() {
        assert_eq!(hard_drop_score(10), 20);
    }

    // ── Level progression tests ─────────────────────────────────────

    #[test]
    fn test_initial_level() {
        let app = TetrisApp::with_seed(42);
        assert_eq!(app.level, 1);
    }

    #[test]
    fn test_level_up_after_10_lines() {
        let mut app = app_no_piece();
        app.lines_cleared = 9;
        app.level = 1;
        // Simulate clearing one more line (bypassing lock_piece for unit test)
        fill_row(&mut app, TOTAL_ROWS - 1);
        let cleared = app.clear_lines();
        assert_eq!(cleared, 1);
        app.lines_cleared += cleared as u32;
        let new_level = app.lines_cleared / 10 + 1;
        if new_level > app.level {
            app.level = new_level;
        }
        assert_eq!(app.level, 2);
    }

    #[test]
    fn test_level_increases_speed() {
        let app1 = TetrisApp::with_seed(42);
        let mut app10 = TetrisApp::with_seed(42);
        app10.level = 10;
        assert!(app10.gravity_interval_ms() < app1.gravity_interval_ms());
    }

    #[test]
    fn test_gravity_interval_minimum() {
        let mut app = TetrisApp::with_seed(42);
        app.level = 100;
        assert!(app.gravity_interval_ms() >= 10);
    }

    // ── Hold piece tests ────────────────────────────────────────────

    #[test]
    fn test_hold_piece_first_time() {
        let mut app = TetrisApp::with_seed(42);
        let first_kind = app.current_piece.as_ref().unwrap().kind;
        app.hold_piece();
        assert_eq!(app.hold_piece, Some(first_kind));
        assert!(app.hold_used);
        // A new piece should have been spawned
        assert!(app.current_piece.is_some());
    }

    #[test]
    fn test_hold_piece_swap() {
        let mut app = TetrisApp::with_seed(42);
        let first_kind = app.current_piece.as_ref().unwrap().kind;
        app.hold_piece(); // Hold first piece, spawn second
        let second_kind = app.current_piece.as_ref().unwrap().kind;
        assert_ne!(first_kind, second_kind); // likely different with seed 42

        // Can't hold again until next piece
        app.hold_used = false; // simulate new piece (normally done by spawn)
        app.hold_piece(); // Swap back
        assert_eq!(app.hold_piece, Some(second_kind));
        assert_eq!(app.current_piece.as_ref().unwrap().kind, first_kind);
    }

    #[test]
    fn test_cannot_hold_twice_same_piece() {
        let mut app = TetrisApp::with_seed(42);
        app.hold_piece();
        let held = app.hold_piece;
        let current_kind = app.current_piece.as_ref().unwrap().kind;
        app.hold_piece(); // Should do nothing — hold_used is true
        assert_eq!(app.hold_piece, held);
        assert_eq!(app.current_piece.as_ref().unwrap().kind, current_kind);
    }

    // ── 7-bag randomizer tests ──────────────────────────────────────

    #[test]
    fn test_bag_contains_all_seven_pieces() {
        let mut bag = BagRandomizer::new(123);
        let mut seen = [false; 7];
        for _ in 0..7 {
            let piece = bag.next_piece();
            let idx = PieceKind::ALL.iter().position(|k| *k == piece).unwrap();
            assert!(!seen[idx], "Duplicate piece in first bag: {piece:?}");
            seen[idx] = true;
        }
        assert!(
            seen.iter().all(|&s| s),
            "Not all pieces appeared in first bag"
        );
    }

    #[test]
    fn test_bag_refills_after_seven() {
        let mut bag = BagRandomizer::new(456);
        // Drain first bag
        for _ in 0..7 {
            bag.next_piece();
        }
        // Second bag should also contain all 7
        let mut seen = [false; 7];
        for _ in 0..7 {
            let piece = bag.next_piece();
            let idx = PieceKind::ALL.iter().position(|k| *k == piece).unwrap();
            seen[idx] = true;
        }
        assert!(seen.iter().all(|&s| s), "Second bag missing pieces");
    }

    #[test]
    fn test_bag_deterministic_with_same_seed() {
        let mut bag1 = BagRandomizer::new(789);
        let mut bag2 = BagRandomizer::new(789);
        for _ in 0..21 {
            assert_eq!(bag1.next_piece(), bag2.next_piece());
        }
    }

    #[test]
    fn test_bag_different_seeds_differ() {
        let mut bag1 = BagRandomizer::new(100);
        let mut bag2 = BagRandomizer::new(200);
        let mut same = true;
        for _ in 0..14 {
            if bag1.next_piece() != bag2.next_piece() {
                same = false;
            }
        }
        assert!(!same, "Different seeds should produce different sequences");
    }

    #[test]
    fn test_preview_queue_initially_has_three() {
        let app = TetrisApp::with_seed(42);
        assert_eq!(app.preview_queue.len(), 3);
    }

    #[test]
    fn test_preview_queue_maintained_after_spawn() {
        let mut app = TetrisApp::with_seed(42);
        app.hard_drop(); // Lock current piece and spawn new one
        if app.status == GameStatus::Playing {
            assert_eq!(app.preview_queue.len(), 3);
        }
    }

    // ── Game over detection tests ───────────────────────────────────

    #[test]
    fn test_game_over_when_field_full() {
        let mut app = TetrisApp::with_seed(42);
        // Fill the top visible rows completely
        for row in HIDDEN_ROWS..HIDDEN_ROWS + 4 {
            for col in 0..FIELD_COLS {
                app.field_set(row, col, Some(RED));
            }
        }
        // Also fill hidden rows
        for row in 0..HIDDEN_ROWS {
            for col in 0..FIELD_COLS {
                app.field_set(row, col, Some(RED));
            }
        }
        // Try to spawn a new piece
        app.current_piece = None;
        app.spawn_piece();
        assert_eq!(app.status, GameStatus::GameOver);
    }

    #[test]
    fn test_initial_status_is_playing() {
        let app = TetrisApp::with_seed(42);
        assert_eq!(app.status, GameStatus::Playing);
    }

    // ── Lock delay tests ────────────────────────────────────────────

    #[test]
    fn test_lock_delay_not_immediate() {
        let mut app = TetrisApp::with_seed(42);
        // Drop piece to the bottom
        while app.try_move(1, 0) {}
        // Piece should be on surface but not yet locked
        app.update_surface_status();
        assert!(app.piece_on_surface);
        // Small tick should not lock
        app.tick(100);
        if app.status == GameStatus::Playing {
            assert!(app.current_piece.is_some() || app.pieces_placed > 0);
        }
    }

    #[test]
    fn test_lock_delay_expires() {
        let mut app = TetrisApp::with_seed(42);
        // Move piece to bottom
        while app.try_move(1, 0) {}
        app.update_surface_status();
        assert!(app.piece_on_surface);
        let pieces_before = app.pieces_placed;
        // Tick past the lock delay
        app.tick(LOCK_DELAY_MS + 100);
        // Piece should now be locked
        assert!(app.pieces_placed > pieces_before || app.status == GameStatus::GameOver);
    }

    #[test]
    fn test_lock_delay_resets_on_move() {
        let mut app = TetrisApp::with_seed(42);
        // Move piece to bottom
        while app.try_move(1, 0) {}
        app.update_surface_status();
        assert!(app.piece_on_surface);
        app.lock_accum_ms = 400; // Almost expired
        // Move sideways (if possible) should reset lock delay
        if app.try_move(0, -1) {
            assert!(app.lock_accum_ms < 400 || !app.piece_on_surface);
        }
    }

    // ── Pause tests ─────────────────────────────────────────────────

    #[test]
    fn test_pause_toggle() {
        let mut app = TetrisApp::with_seed(42);
        assert_eq!(app.status, GameStatus::Playing);
        press_key(&mut app, Key::P);
        assert_eq!(app.status, GameStatus::Paused);
        press_key(&mut app, Key::P);
        assert_eq!(app.status, GameStatus::Playing);
    }

    #[test]
    fn test_no_movement_while_paused() {
        let mut app = TetrisApp::with_seed(42);
        press_key(&mut app, Key::P);
        let col = app.current_piece.as_ref().unwrap().col;
        press_key(&mut app, Key::Left);
        assert_eq!(app.current_piece.as_ref().unwrap().col, col);
    }

    #[test]
    fn test_tick_does_nothing_while_paused() {
        let mut app = TetrisApp::with_seed(42);
        let time_before = app.elapsed_ms;
        app.status = GameStatus::Paused;
        app.tick(1000);
        assert_eq!(app.elapsed_ms, time_before);
    }

    // ── Gravity / tick tests ────────────────────────────────────────

    #[test]
    fn test_gravity_moves_piece_down() {
        let mut app = TetrisApp::with_seed(42);
        let row_before = app.current_piece.as_ref().unwrap().row;
        // Tick for enough time to trigger gravity at level 1
        let interval = app.gravity_interval_ms();
        app.tick(interval + 1);
        let row_after = app.current_piece.as_ref().unwrap().row;
        assert!(row_after > row_before, "Gravity should move piece down");
    }

    #[test]
    fn test_gravity_faster_at_higher_level() {
        let mut app = TetrisApp::with_seed(42);
        let interval_l1 = app.gravity_interval_ms();
        app.level = 5;
        let interval_l5 = app.gravity_interval_ms();
        app.level = 10;
        let interval_l10 = app.gravity_interval_ms();
        assert!(interval_l5 < interval_l1);
        assert!(interval_l10 < interval_l5);
    }

    // ── T-spin detection tests ──────────────────────────────────────

    #[test]
    fn test_t_corners_empty_field() {
        let field = vec![None; TOTAL_ROWS * FIELD_COLS];
        let corners = count_t_corners(&field, 10, 4);
        assert_eq!(corners, 0);
    }

    #[test]
    fn test_t_corners_all_occupied() {
        let mut field = vec![None; TOTAL_ROWS * FIELD_COLS];
        // Set all four corners of a 3x3 bounding box
        field[10 * FIELD_COLS + 4] = Some(BLUE); // (10, 4) top-left
        field[10 * FIELD_COLS + 6] = Some(BLUE); // (10, 6) top-right
        field[12 * FIELD_COLS + 4] = Some(BLUE); // (12, 4) bottom-left
        field[12 * FIELD_COLS + 6] = Some(BLUE); // (12, 6) bottom-right
        let corners = count_t_corners(&field, 10, 4);
        assert_eq!(corners, 4);
    }

    #[test]
    fn test_t_corners_at_left_wall() {
        let field = vec![None; TOTAL_ROWS * FIELD_COLS];
        // At col=-1, left corners are out of bounds (count as occupied)
        let corners = count_t_corners(&field, 10, -1);
        assert_eq!(corners, 2); // two left corners are OOB
    }

    #[test]
    fn test_tspin_detection_none_without_rotation() {
        let mut app = TetrisApp::with_seed(42);
        app.current_piece = Some(ActivePiece {
            kind: PieceKind::T,
            rotation: 0,
            row: 10,
            col: 4,
        });
        app.last_move_was_rotation = false;
        let piece = app.current_piece.as_ref().unwrap().clone();
        let tspin = app.detect_tspin(&piece);
        assert_eq!(tspin, TSpinKind::None);
    }

    // ── Rendering tests ─────────────────────────────

    /// The size the app opens at, for tests that do not care which size.
    const SIZE: (f32, f32) = TetrisApp::SIZE;

    #[test]
    fn test_render_returns_commands() {
        let app = TetrisApp::with_seed(42);
        assert!(!app.frame(SIZE.0, SIZE.1).commands().is_empty());
    }

    #[test]
    fn test_render_pause_overlay() {
        let mut app = TetrisApp::with_seed(42);
        app.status = GameStatus::Paused;
        let paused = app.frame(SIZE.0, SIZE.1).commands().len();
        // Should have more commands than when playing (overlay)
        let playing = TetrisApp::with_seed(42)
            .frame(SIZE.0, SIZE.1)
            .commands()
            .len();
        assert!(paused > playing);
    }

    #[test]
    fn test_render_game_over_overlay() {
        let mut app = TetrisApp::with_seed(42);
        app.status = GameStatus::GameOver;
        let over = app.frame(SIZE.0, SIZE.1).commands().len();
        let playing = TetrisApp::with_seed(42)
            .frame(SIZE.0, SIZE.1)
            .commands()
            .len();
        assert!(over > playing);
    }

    #[test]
    fn test_render_ghost_piece() {
        let app = TetrisApp::with_seed(42);
        let f = app.frame(SIZE.0, SIZE.1);
        // Ghost piece generates StrokeRect commands — count them
        let stroke_count = f
            .commands()
            .iter()
            .filter(|c| matches!(c, RenderCommand::StrokeRect { .. }))
            .count();
        // Should have at least 4 (ghost cells) + 1 (field border)
        assert!(
            stroke_count >= 5,
            "Expected ghost piece stroke rects, found {stroke_count}"
        );
    }

    #[test]
    fn test_render_field_background() {
        let app = TetrisApp::with_seed(42);
        let f = app.frame(SIZE.0, SIZE.1);
        // First command should be the background fill
        assert!(matches!(
            f.commands().first(),
            Some(RenderCommand::FillRect { .. })
        ));
    }

    #[test]
    fn test_format_time() {
        let mut app = TetrisApp::with_seed(42);
        app.elapsed_ms = 0;
        assert_eq!(app.format_time(), "00:00");
        app.elapsed_ms = 65_000;
        assert_eq!(app.format_time(), "01:05");
        app.elapsed_ms = 3_661_000;
        assert_eq!(app.format_time(), "61:01");
    }

    // ── Event handling tests ────────────────────────────────────────

    #[test]
    fn test_handle_event_key() {
        let mut app = TetrisApp::with_seed(42);
        let event = Event::Key(make_key_event(Key::Left));
        let col_before = app.current_piece.as_ref().unwrap().col;
        handle_event(&mut app, &event);
        assert_eq!(app.current_piece.as_ref().unwrap().col, col_before - 1);
    }

    #[test]
    fn test_handle_event_tick() {
        let mut app = TetrisApp::with_seed(42);
        let elapsed_before = app.elapsed_ms;
        handle_event(&mut app, &Event::Tick { elapsed_ms: 100 });
        assert_eq!(app.elapsed_ms, elapsed_before + 100);
    }

    #[test]
    fn test_key_release_ignored() {
        let mut app = TetrisApp::with_seed(42);
        let col_before = app.current_piece.as_ref().unwrap().col;
        let event = KeyEvent {
            key: Key::Left,
            pressed: false, // Release, not press
            modifiers: Modifiers::NONE,
            text: String::new(),
        };
        app.handle_key(&event);
        assert_eq!(app.current_piece.as_ref().unwrap().col, col_before);
    }

    #[test]
    fn test_up_key_rotates_cw() {
        let mut app = TetrisApp::with_seed(42);
        let rot_before = app.current_piece.as_ref().unwrap().rotation;
        let kind = app.current_piece.as_ref().unwrap().kind;
        press_key(&mut app, Key::Up);
        if kind != PieceKind::O {
            assert_eq!(
                app.current_piece.as_ref().unwrap().rotation,
                (rot_before + 1) % 4
            );
        }
    }

    #[test]
    fn test_z_key_rotates_cw() {
        let mut app = TetrisApp::with_seed(42);
        let rot_before = app.current_piece.as_ref().unwrap().rotation;
        let kind = app.current_piece.as_ref().unwrap().kind;
        press_key(&mut app, Key::Z);
        if kind != PieceKind::O {
            assert_eq!(
                app.current_piece.as_ref().unwrap().rotation,
                (rot_before + 1) % 4
            );
        }
    }

    #[test]
    fn test_x_key_rotates_ccw() {
        let mut app = TetrisApp::with_seed(42);
        let rot_before = app.current_piece.as_ref().unwrap().rotation;
        let kind = app.current_piece.as_ref().unwrap().kind;
        press_key(&mut app, Key::X);
        if kind != PieceKind::O {
            assert_eq!(
                app.current_piece.as_ref().unwrap().rotation,
                (rot_before + 3) % 4
            );
        }
    }

    #[test]
    fn test_space_key_hard_drops() {
        let mut app = TetrisApp::with_seed(42);
        let pieces_before = app.pieces_placed;
        press_key(&mut app, Key::Space);
        assert!(app.pieces_placed > pieces_before);
    }

    #[test]
    fn test_c_key_holds() {
        let mut app = TetrisApp::with_seed(42);
        assert!(app.hold_piece.is_none());
        press_key(&mut app, Key::C);
        assert!(app.hold_piece.is_some());
    }

    #[test]
    fn test_down_key_soft_drops() {
        let mut app = TetrisApp::with_seed(42);
        let row_before = app.current_piece.as_ref().unwrap().row;
        press_key(&mut app, Key::Down);
        assert_eq!(app.current_piece.as_ref().unwrap().row, row_before + 1);
    }

    // ── The 7-bag ───────────────────────────────────────────────────

    // The generator's own contract -- determinism under a seed, divergence
    // under two, staying inside its bound -- used to be tested here against the
    // local `Lcg`. It is now tested once, against the shared implementation, in
    // `randrange`. Sixteen crates each testing their own copy is sixteen
    // chances to test a copy that has quietly drifted from the one being
    // shipped. What replaces those tests is about the bag, which is what a
    // player actually experiences.

    /// Deal `bags` bags and return the piece each position of each bag held.
    fn dealt_bags(seed: u64, bags: usize) -> Vec<Vec<usize>> {
        let mut bag_gen = BagRandomizer::new(seed);
        (0..bags)
            .map(|_| {
                (0..PieceKind::ALL.len())
                    .map(|_| {
                        let kind = bag_gen.next_piece();
                        PieceKind::ALL.iter().position(|k| *k == kind).unwrap_or(0)
                    })
                    .collect()
            })
            .collect()
    }

    /// Every piece must be able to arrive at every position in the bag.
    ///
    /// This is the test that catches the reduction bug described by
    /// `FALLBACK_SEED`, and it is the only cheap one that does. The obvious
    /// check -- "the bags are not all the same" -- passes on the broken code:
    /// 200 broken bags contain 177 distinct orders, which looks perfectly
    /// healthy. The defect is not a lack of variety, it is a *hole* in the
    /// variety. Filling the 7x7 table of (piece, position) counts over 7000
    /// broken bags left six cells at exactly zero -- one piece could never be
    /// dealt in three of the seven slots -- while that same piece took one
    /// particular slot in 46% of bags instead of the expected 14%.
    #[test]
    fn every_piece_reaches_every_position_in_the_bag() {
        let kinds = PieceKind::ALL.len();
        let bags = 700;
        let mut counts = vec![vec![0usize; kinds]; kinds];
        for bag in dealt_bags(0x1234_5678, bags) {
            for (pos, piece) in bag.iter().enumerate() {
                counts[*piece][pos] += 1;
            }
        }
        for (piece, row) in counts.iter().enumerate() {
            for (pos, count) in row.iter().enumerate() {
                assert!(
                    *count > 0,
                    "piece {piece} was never dealt at position {pos} in {bags} bags;                      the table is {counts:?}"
                );
            }
        }
    }

    /// Every bag must still hold each of the seven pieces exactly once.
    ///
    /// The whole point of a 7-bag is that it bounds the drought between two
    /// copies of the same piece. A shuffle that lost or duplicated a piece
    /// would break that guarantee without breaking the test above.
    #[test]
    fn every_bag_holds_each_piece_exactly_once() {
        for (n, bag) in dealt_bags(99, 50).into_iter().enumerate() {
            let mut sorted = bag.clone();
            sorted.sort_unstable();
            let expected: Vec<usize> = (0..PieceKind::ALL.len()).collect();
            assert_eq!(sorted, expected, "bag {n} was not a permutation: {bag:?}");
        }
    }

    /// A fresh game must take its seed from the kernel, not from a literal.
    ///
    /// Phrased as "which seed", not as "two fresh games differ", because a host
    /// test build has no SlateOS kernel: `seed_from_system` correctly takes its
    /// fallback and two fresh games are then identical, exactly as they were
    /// under the old hardcoded `42`. A variety check would therefore pass on
    /// the broken code and fail on the fixed code, which is backwards.
    #[cfg(not(unix))]
    #[test]
    fn a_fresh_game_is_seeded_by_the_system_and_not_by_a_literal() {
        let fresh = TetrisApp::new().seed;
        assert_eq!(
            fresh, FALLBACK_SEED,
            "a fresh game did not use the crate's fallback seed"
        );
        assert_ne!(fresh, 42, "a fresh game is still seeded by the old literal");
    }

    // ── Integration tests ───────────────────────────────────────────

    #[test]
    fn test_full_game_sequence() {
        let mut app = TetrisApp::with_seed(42);
        assert_eq!(app.status, GameStatus::Playing);

        // Drop a few pieces
        for _ in 0..5 {
            app.hard_drop();
            if app.status != GameStatus::Playing {
                break;
            }
        }

        assert!(app.pieces_placed >= 5 || app.status == GameStatus::GameOver);
        assert!(app.score > 0);
    }

    #[test]
    fn test_restart_from_game_over() {
        let mut app = TetrisApp::with_seed(42);
        app.status = GameStatus::GameOver;
        press_key(&mut app, Key::R);
        assert_eq!(app.status, GameStatus::Playing);
        assert_eq!(app.score, 0);
        assert_eq!(app.level, 1);
        assert_eq!(app.lines_cleared, 0);
        assert_eq!(app.pieces_placed, 0);
    }

    #[test]
    fn test_main_fn_does_not_panic() {
        // Ensure main() creates app without panicking
        let _app = TetrisApp::new();
    }

    #[test]
    fn test_active_piece_absolute_cells() {
        let piece = ActivePiece {
            kind: PieceKind::O,
            rotation: 0,
            row: 5,
            col: 3,
        };
        let cells = piece.absolute_cells();
        assert_eq!(cells, [(5, 3), (5, 4), (6, 3), (6, 4)]);
    }

    #[test]
    fn test_active_piece_moved() {
        let piece = ActivePiece {
            kind: PieceKind::T,
            rotation: 0,
            row: 5,
            col: 3,
        };
        let moved = piece.moved(2, -1);
        assert_eq!(moved.row, 7);
        assert_eq!(moved.col, 2);
        assert_eq!(moved.kind, PieceKind::T);
        assert_eq!(moved.rotation, 0);
    }

    #[test]
    fn test_active_piece_with_rotation() {
        let piece = ActivePiece {
            kind: PieceKind::T,
            rotation: 0,
            row: 5,
            col: 3,
        };
        let rotated = piece.with_rotation(2);
        assert_eq!(rotated.rotation, 2);
        assert_eq!(rotated.row, 5);
        assert_eq!(rotated.col, 3);
    }

    #[test]
    fn test_field_get_out_of_bounds() {
        let app = TetrisApp::with_seed(42);
        assert!(app.field_get(TOTAL_ROWS, 0).is_none());
        assert!(app.field_get(0, FIELD_COLS).is_none());
    }

    #[test]
    fn test_field_set_and_get() {
        let mut app = TetrisApp::with_seed(42);
        app.field_set(10, 5, Some(RED));
        assert_eq!(app.field_get(10, 5), Some(RED));
        app.field_set(10, 5, None);
        assert_eq!(app.field_get(10, 5), None);
    }

    #[test]
    fn test_pieces_placed_increments() {
        let mut app = TetrisApp::with_seed(42);
        assert_eq!(app.pieces_placed, 0);
        app.hard_drop();
        assert!(app.pieces_placed >= 1);
    }

    #[test]
    fn test_elapsed_time_increments() {
        let mut app = TetrisApp::with_seed(42);
        app.tick(500);
        assert_eq!(app.elapsed_ms, 500);
        app.tick(500);
        assert_eq!(app.elapsed_ms, 1000);
    }

    // ── Window, layout and pointer tests ────────────────────────────

    /// Every action the CONTROLS panel lists, in order.
    fn listed_actions() -> Vec<Action> {
        CONTROLS.iter().map(|(a, _)| *a).collect()
    }

    #[test]
    fn the_panel_offers_every_action_the_keyboard_does() {
        // The panel is the pointer's whole repertoire, so an action reachable
        // by key and missing from the table is an action the mouse cannot
        // perform. Checking it from the key side means adding a key without a
        // row fails here rather than silently shrinking what a click can do.
        for key in [
            Key::Left,
            Key::Right,
            Key::Down,
            Key::Space,
            Key::Up,
            Key::Z,
            Key::X,
            Key::C,
            Key::P,
            Key::R,
        ] {
            let action = TetrisApp::action_for_key(key)
                .unwrap_or_else(|| panic!("{key:?} names no action at all"));
            assert!(
                listed_actions().contains(&action),
                "{key:?} performs {action:?}, which no CONTROLS row offers"
            );
        }
    }

    #[test]
    fn every_control_row_is_the_button_it_names() {
        // Each row is clicked on a board arranged so the action it names has a
        // visible effect, and the effect is compared against the same action
        // performed by its key. Anything a row does that its key does not is a
        // second implementation, which is exactly what one `Action` prevents.
        for action in listed_actions() {
            let mut clicked = TetrisApp::with_seed(7);
            let mut keyed = TetrisApp::with_seed(7);

            let outcome = guitk::probe::click(&mut clicked, Target::Control(action));
            assert_eq!(
                outcome,
                EventResult::Consumed,
                "the row for {action:?} did not take the click"
            );
            keyed.apply(action);

            assert_eq!(
                clicked.current_piece, keyed.current_piece,
                "clicking {action:?} moved the piece somewhere its key does not"
            );
            assert_eq!(
                clicked.status, keyed.status,
                "status differs for {action:?}"
            );
            assert_eq!(clicked.score, keyed.score, "score differs for {action:?}");
            assert_eq!(
                clicked.hold_piece, keyed.hold_piece,
                "hold differs for {action:?}"
            );
        }
    }

    #[test]
    fn clicking_the_hold_box_is_the_same_as_pressing_c() {
        let mut clicked = TetrisApp::with_seed(11);
        let mut keyed = TetrisApp::with_seed(11);
        assert_eq!(
            guitk::probe::click(&mut clicked, Target::HoldBox),
            EventResult::Consumed
        );
        press_key(&mut keyed, Key::C);
        assert_eq!(clicked.hold_piece, keyed.hold_piece);
        assert!(clicked.hold_piece.is_some(), "nothing was held");
        assert_eq!(clicked.current_piece, keyed.current_piece);
    }

    #[test]
    fn the_message_box_does_what_its_text_says() {
        let mut paused = TetrisApp::with_seed(3);
        paused.status = GameStatus::Paused;
        assert_eq!(
            guitk::probe::click(&mut paused, Target::Overlay),
            EventResult::Consumed
        );
        assert_eq!(paused.status, GameStatus::Playing, "the box said resume");

        let mut over = TetrisApp::with_seed(3);
        over.status = GameStatus::GameOver;
        over.score = 999;
        assert_eq!(
            guitk::probe::click(&mut over, Target::Overlay),
            EventResult::Consumed
        );
        assert_eq!(over.status, GameStatus::Playing, "the box said restart");
        assert_eq!(over.score, 0, "a restart that keeps the score is not one");
    }

    #[test]
    fn the_message_box_is_not_there_while_the_game_is_running() {
        let app = TetrisApp::with_seed(3);
        assert!(
            guitk::probe::rect_of(&app, Target::Overlay).is_none(),
            "a clickable box with nothing drawn in it"
        );
    }

    #[test]
    fn a_click_on_nothing_is_left_for_whoever_wants_it() {
        let mut app = TetrisApp::with_seed(3);
        // The middle of the board: drawn, but not a control. A game that
        // consumed it would stop a drag of the window from starting there.
        let (x, y) = Layout::new(TetrisApp::SIZE.0, TetrisApp::SIZE.1)
            .field
            .centre();
        assert_eq!(
            app.click_at(x, y, MouseButton::Left, TetrisApp::SIZE),
            EventResult::Ignored
        );
    }

    #[test]
    fn a_ctrl_or_alt_combination_belongs_to_the_desktop() {
        // Ctrl-R is "reload" in every other program on the screen. Eating it
        // here as "restart" would make the game the only thing that does.
        for modifiers in [
            Modifiers {
                ctrl: true,
                ..Modifiers::NONE
            },
            Modifiers {
                alt: true,
                ..Modifiers::NONE
            },
            Modifiers {
                super_key: true,
                ..Modifiers::NONE
            },
        ] {
            let mut app = TetrisApp::with_seed(5);
            app.status = GameStatus::GameOver;
            let event = KeyEvent {
                key: Key::R,
                pressed: true,
                modifiers,
                text: String::new(),
            };
            assert_eq!(
                handle_event(&mut app, &Event::Key(event)),
                EventResult::Ignored
            );
            assert_eq!(
                app.status,
                GameStatus::GameOver,
                "{modifiers:?} restarted the game"
            );
        }
    }

    #[test]
    fn the_layout_stays_inside_the_window_at_every_size() {
        for (w, h) in [
            (120.0_f32, 120.0_f32),
            (200.0, 400.0),
            (320.0, 240.0),
            (640.0, 720.0),
            (1024.0, 768.0),
            (2560.0, 1440.0),
            (400.0, 2000.0),
            (2000.0, 400.0),
            (1.0, 1.0),
        ] {
            let l = Layout::new(w, h);
            let window = l.window;
            for (name, rect) in [
                ("header", l.header),
                ("field", l.field),
                ("hold", l.hold),
                ("stats", l.stats),
                ("next", l.next),
                ("controls", l.controls),
                ("overlay", l.overlay),
            ] {
                if rect.is_empty() {
                    continue;
                }
                assert!(
                    rect.x >= -0.01
                        && rect.y >= -0.01
                        && rect.right() <= window.w + 0.01
                        && rect.bottom() <= window.h + 0.01,
                    "{name} {rect:?} runs outside a {w}x{h} window"
                );
            }
            assert!(
                l.cell.is_finite() && l.cell >= 0.0 && l.gap >= 0.0,
                "nonsense cell {} / gap {} at {w}x{h}",
                l.cell,
                l.gap
            );
            // A one-pixel window has no board and correctly says so with a
            // zero cell and an empty field. Anything a compositor would
            // plausibly hand us must still be playable.
            if w >= 100.0 && h >= 100.0 {
                assert!(l.cell > 0.0, "no board at {w}x{h}");
            }
            assert!(l.font >= 7.0, "unreadable font at {w}x{h}");
        }
    }

    #[test]
    fn a_narrow_window_gives_the_sidebars_room_to_the_board() {
        // A squeezed board is unplayable; a missing HOLD panel costs the
        // player a reminder and not a move, so the panels are what goes.
        let wide = Layout::new(640.0, 720.0);
        let narrow = Layout::new(200.0, 720.0);
        assert!(!wide.hold.is_empty(), "the sidebars should fit at 640");
        assert!(narrow.hold.is_empty(), "the sidebars should not fit at 200");
        assert!(narrow.controls.is_empty());
        assert!(
            narrow.field.w > 200.0 * 0.8,
            "the board did not take the room the sidebars gave up: {:?}",
            narrow.field
        );
    }

    #[test]
    fn the_cells_stay_square_whatever_shape_the_window_is() {
        for (w, h) in [(640.0_f32, 720.0_f32), (2000.0, 400.0), (300.0, 1600.0)] {
            let l = Layout::new(w, h);
            let a = l.cell_rect(0, 0);
            let b = l.cell_rect(1, 1);
            assert!(
                (a.w - a.h).abs() < 0.01,
                "cell {a:?} is not square at {w}x{h}"
            );
            assert!(
                (b.x - a.x - (b.y - a.y)).abs() < 0.01,
                "the column step and the row step differ at {w}x{h}"
            );
        }
    }

    #[test]
    fn a_panel_shows_fewer_rows_rather_than_an_unreadable_smear() {
        let roomy = Layout::new(640.0, 900.0);
        let (_, roomy_rows) = roomy.control_rows();
        assert_eq!(
            roomy_rows,
            CONTROLS.len(),
            "all nine should fit at 900 tall"
        );

        let cramped = Layout::new(640.0, 150.0);
        let (h, shown) = cramped.control_rows();
        assert!(shown < CONTROLS.len(), "150 tall cannot hold all nine");
        assert!(h >= cramped.font * 1.5, "the rows shrank below legibility");
        for i in shown..CONTROLS.len() {
            assert!(
                cramped.control_row(i).is_empty(),
                "row {i} has a box but is not shown"
            );
        }
    }

    #[test]
    fn the_buttons_follow_the_window_when_it_is_resized() {
        // The point of rebuilding the layout every frame: a hit box computed
        // at the old size is a click that lands on the wrong thing.
        let small = (520.0_f32, 640.0_f32);
        let large = (1100.0_f32, 900.0_f32);

        let app = TetrisApp::with_seed(9);
        let at_small = app
            .frame(small.0, small.1)
            .rect_of(|t| *t == Target::Control(Action::Restart))
            .expect("no restart row at the small size");
        let at_large = app
            .frame(large.0, large.1)
            .rect_of(|t| *t == Target::Control(Action::Restart))
            .expect("no restart row at the large size");
        assert!(
            (at_small.x - at_large.x).abs() > 1.0,
            "the row did not move with the window: {at_small:?} vs {at_large:?}"
        );

        let mut app = TetrisApp::with_seed(9);
        app.status = GameStatus::GameOver;
        let (x, y) = at_large.centre();
        assert_eq!(
            app.click_at(x, y, MouseButton::Left, large),
            EventResult::Consumed
        );
        assert_eq!(
            app.status,
            GameStatus::Playing,
            "the click missed the row at the size it was drawn for"
        );
    }

    #[test]
    fn a_resize_event_is_what_the_next_frame_is_drawn_at() {
        let mut app = TetrisApp::with_seed(9);
        handle_event(
            &mut app,
            &Event::Resize {
                width: 900,
                height: 500,
            },
        );
        let f = app.frame(app.width, app.height);
        assert!((f.width - 900.0).abs() < 0.01);
        assert!((f.height - 500.0).abs() < 0.01);
        // `target_at` must agree with what was drawn, or a hit test answers
        // for a window that is no longer on screen.
        let row = f
            .rect_of(|t| *t == Target::Control(Action::Pause))
            .expect("no pause row");
        let (x, y) = row.centre();
        assert_eq!(app.target_at(x, y), Some(Target::Control(Action::Pause)));
    }

    // ── Gravity timing tests ────────────────────────────────────────

    #[test]
    fn the_piece_falls_once_per_interval_and_not_once_per_tick() {
        // The old `tick` stepped gravity on every event it was handed, so the
        // fall speed was the compositor's frame rate rather than the level.
        let mut app = TetrisApp::with_seed(13);
        let start = app.current_piece.as_ref().expect("no piece").row;
        let interval = app.gravity_interval_ms();
        assert!(interval > 200, "level 1 should be slow: {interval}ms");

        for _ in 0..10 {
            app.tick(interval / 20);
        }
        assert_eq!(
            app.current_piece.as_ref().expect("no piece").row,
            start,
            "half an interval of ticks moved the piece"
        );

        app.tick(interval);
        assert_eq!(
            app.current_piece.as_ref().expect("no piece").row,
            start.saturating_add(1),
            "a whole interval did not move it exactly one row"
        );
    }

    #[test]
    fn a_long_tick_is_worth_every_interval_it_covers() {
        // A window that was busy for a second owes the player a second of
        // gravity, not one row. `Event::Tick` carries what really elapsed.
        let mut app = TetrisApp::with_seed(13);
        let start = app.current_piece.as_ref().expect("no piece").row;
        let interval = app.gravity_interval_ms();
        app.tick(interval.saturating_mul(3));
        assert_eq!(
            app.current_piece.as_ref().expect("no piece").row,
            start.saturating_add(3)
        );
    }

    #[test]
    fn the_clock_counts_the_time_the_ticks_carry() {
        let mut app = TetrisApp::with_seed(13);
        for _ in 0..40 {
            app.tick(25);
        }
        assert_eq!(app.elapsed_ms, 1000);
        assert_eq!(app.format_time(), "00:01");
    }

    #[test]
    fn the_game_asks_for_ticks_only_while_it_is_playing() {
        // A paused game that keeps asking to be woken is a paused game that
        // keeps the CPU busy for nothing.
        let mut app = TetrisApp::with_seed(13);
        assert_eq!(app.tick_interval(), Some(Duration::from_millis(50)));
        app.status = GameStatus::Paused;
        assert_eq!(app.tick_interval(), None);
        app.status = GameStatus::GameOver;
        assert_eq!(app.tick_interval(), None);
    }

    #[test]
    fn losing_focus_pauses_rather_than_letting_the_board_fall_unwatched() {
        let mut app = TetrisApp::with_seed(13);
        assert_eq!(
            handle_event(&mut app, &Event::FocusOut),
            EventResult::Consumed
        );
        assert_eq!(app.status, GameStatus::Paused);
        // Already over is already over: focus is not a way back in.
        let mut over = TetrisApp::with_seed(13);
        over.status = GameStatus::GameOver;
        assert_eq!(
            handle_event(&mut over, &Event::FocusOut),
            EventResult::Ignored
        );
        assert_eq!(over.status, GameStatus::GameOver);
    }

    #[test]
    fn pause_is_not_a_way_out_of_a_finished_game() {
        let mut app = TetrisApp::with_seed(13);
        app.status = GameStatus::GameOver;
        assert!(!app.apply(Action::Pause));
        assert_eq!(app.status, GameStatus::GameOver);
    }

    #[test]
    fn a_key_that_changes_nothing_is_still_the_games_key() {
        // A left arrow into the wall must not fall through to the desktop,
        // which would move the window focus instead of doing nothing.
        let mut app = TetrisApp::with_seed(13);
        for _ in 0..12 {
            app.apply(Action::MoveLeft);
        }
        assert!(
            !app.apply(Action::MoveLeft),
            "the wall should have stopped it"
        );
        assert_eq!(
            handle_event(&mut app, &Event::Key(make_key_event(Key::Left))),
            EventResult::Consumed
        );
    }
}
