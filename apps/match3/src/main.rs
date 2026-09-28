//! Slate OS Match-3 — Bejeweled-style puzzle game.
//!
//! Features an 8x8 grid of colored gems with match-3 mechanics,
//! gravity/cascading, special gems (line clear, color bomb),
//! three game modes (Classic, Timed, Moves), high score tracking,
//! a hint system, and automatic shuffle when no moves exist.
//! Randomness comes from the shared `randrange` crate, seeded from the
//! system so that two players do not get the same game.
//!
//! Every rectangle comes from the window's own size (`Layout::new`), and a
//! click is read against the frame the window is showing: the pass that
//! draws the board records a box for every cell and control, and
//! `Frame::hit_test` answers from those. It drew at one size -- 48-pixel
//! cells, a window computed from them, text at eyeballed offsets from the
//! middle of a cell -- so a larger window left the board in a corner and a
//! smaller one cut it off.

use gamechrome::Chrome;
use guitk::button::{Kind, State};
use guitk::color::Color;
#[cfg(test)]
use guitk::event::Modifiers;
use guitk::event::{Event, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::frame::Rect;
use guitk::palette::Palette;
use guitk::probe::Probe;
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::rng::{RandomSource, SeededRng, seed_from_system};
use guitk::style::CornerRadii;
use guitk::surface::Surface;
use guitk::text;
use guitk::theme::with_alpha;
use oswindow::app::{self, App, Response};
use std::process::ExitCode;
use std::time::Duration;

// ── Colours ─────────────────────────────────────────────────────────
//
// A gem's colour is its kind -- a player matches by it -- so the seven keep
// their colours in every theme; the board, the header, the marks on the board
// and the panels follow the user's palette (the operator's answer to C-Q16,
// §1422, and lane C's call for this game). It was all a copy of Catppuccin
// Mocha, dark on a light desktop.

// The seven gems.
const RUBY: Color = Color::from_hex(0xF38BA8);
const SAPPHIRE: Color = Color::from_hex(0x89B4FA);
const EMERALD: Color = Color::from_hex(0xA6E3A1);
const TOPAZ: Color = Color::from_hex(0xF9E2AF);
const AMBER: Color = Color::from_hex(0xFAB387);
const AMETHYST: Color = Color::from_hex(0xCBA6F7);
const AQUA: Color = Color::from_hex(0x94E2D5);
/// What is written on a gem -- its symbol, a special's marks: every gem is
/// pale, so near-black on all of them.
///
/// A neutral near-black and not Mocha's base (`1E1E2E`): the palette test
/// names this as the game's own colour and matches it on RGB, so Mocha's
/// base here would let a leftover Mocha base through in a light window.
const GEM_INK: Color = Color::from_hex(0x161616);

/// The colours the window draws in, from the user's palette.
#[derive(Clone, Copy, Debug)]
struct Colours {
    chrome: Chrome,
    /// The glow under the keyboard's cell, the gem picked up, and a hint.
    cursor: Color,
    selected: Color,
    hint: Color,
}

impl Colours {
    fn of(p: &Palette) -> Self {
        Self {
            chrome: Chrome::of(p),
            cursor: with_alpha(p.text, 80),
            selected: with_alpha(p.accent, 120),
            hint: p.green,
        }
    }
}

// ── Layout constants ────────────────────────────────────────────────
const GRID_SIZE: usize = 8;
/// How often the board is asked to advance itself.
///
/// 16 ms is one frame at 60 Hz, which is what the cascade animation and the
/// Timed-mode clock were written against: `handle_tick` takes the elapsed
/// milliseconds and both scale by it, so a slower tick is correct but visibly
/// steppy, and a faster one is work nobody can see.
const TICK: Duration = Duration::from_millis(16);

/// The window the game asks for. Any other size is laid out the same way:
/// every rectangle is [`Layout::new`]'s, from the window's own size.
const WINDOW_WIDTH: f32 = 480.0;
const WINDOW_HEIGHT: f32 = 640.0;

/// The share of the window's height the board keeps before the bands give
/// way: in a short window the key line goes first, then the controls, then
/// the header -- the keys do what the controls do, and the score is the one
/// thing a player cannot play without.
const BOARD_SHARE: f32 = 0.55;

/// What the controls' row offers, in order.
const CONTROLS: [(Target, &str); 5] = [
    (Target::Mode(GameMode::Classic), "Classic"),
    (Target::Mode(GameMode::Timed), "Timed"),
    (Target::Mode(GameMode::Moves), "Moves"),
    (Target::Hint, "Hint"),
    (Target::NewGame, "New game"),
];

/// The key line at the foot of the window.
const KEYS: &str =
    "Arrows move \u{b7} Enter swaps \u{b7} H hint \u{b7} N new game \u{b7} 1 2 3 mode";

/// Number of gem types.
const GEM_TYPE_COUNT: u8 = 7;

/// Idle milliseconds before showing a hint.
const HINT_DELAY_MS: u64 = 5000;

/// Timed mode duration in seconds.
const TIMED_MODE_SECONDS: u64 = 60;

/// Moves mode move count.
const MOVES_MODE_COUNT: u32 = 30;

/// Base score for a 3-match.
const SCORE_3: u32 = 100;

/// Base score for a 4-match.
const SCORE_4: u32 = 200;

/// Base score for a 5-match.
const SCORE_5: u32 = 500;

/// Cascade multiplier increase per chain level (1.5x).
/// Stored as fixed-point: 150 means 1.50x.
const CASCADE_MULTIPLIER_FP: u32 = 150;

/// Fixed-point base (100 = 1.0x).
const FP_BASE: u32 = 100;

// ── Randomness ──────────────────────────────────────────────────────

/// Seed used when the kernel's entropy source cannot be reached.
///
/// A per-crate constant rather than a shared one, so that two programs which
/// lose entropy on the same boot do not then produce correlated streams. The
/// bytes spell `MATCH3!!`.
const FALLBACK_SEED: u64 = 0x4D41_5443_4833_2121;

// This crate used to carry its own copy of the LCG that got copied into
// sixteen crates, reducing with `val % bound`. The generator's modulus is
// 2^64, so bit *k* of its state has period 2^(k+1): the low bits are a
// counter, not a draw, and any power-of-two bound reads only those.
//
// Two callers here, with opposite luck. Drawing a gem type used
// `next_bounded(7)`, and 7 is odd, so its remainder depends on all 64 bits and
// it escaped -- unlike `apps/simon`, whose four colours made the identical call
// deal a fixed 4-cycle for ever. But `shuffle_board` ran Fisher-Yates over an
// 8x8 board, so its bounds counted down 64, 63, ... 2 and every sixth of them
// was a power of two. Those swaps were not random; they were a fixed function
// of their position in the loop. See `apps/tetris` for what that does to a
// shuffle, measured.
//
// `randrange::below` is Lemire's method: it multiplies by the bound into 128
// bits and keeps the *top* half, so it reads the high bits and never the low
// ones, with a rejection step that makes it exactly uniform.

// ── Gem types ───────────────────────────────────────────────────────
/// The type/color of a gem on the board.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
enum GemType {
    Ruby = 0,
    Sapphire = 1,
    Emerald = 2,
    Topaz = 3,
    Amber = 4,
    Amethyst = 5,
    Aqua = 6,
}

impl GemType {
    fn from_index(i: u8) -> Self {
        match i {
            0 => GemType::Ruby,
            1 => GemType::Sapphire,
            2 => GemType::Emerald,
            3 => GemType::Topaz,
            4 => GemType::Amber,
            5 => GemType::Amethyst,
            _ => GemType::Aqua,
        }
    }

    fn color(self) -> Color {
        // `match`, not an index into a table of colours. Same lookup, except
        // that adding a variant to `GemType` fails to compile instead of
        // panicking the first time that colour is drawn.
        match self {
            Self::Ruby => RUBY,
            Self::Sapphire => SAPPHIRE,
            Self::Emerald => EMERALD,
            Self::Topaz => TOPAZ,
            Self::Amber => AMBER,
            Self::Amethyst => AMETHYST,
            Self::Aqua => AQUA,
        }
    }

    fn symbol(self) -> &'static str {
        match self {
            Self::Ruby => "\u{25C6}",
            Self::Sapphire => "\u{25CF}",
            Self::Emerald => "\u{25A0}",
            Self::Topaz => "\u{2605}",
            Self::Amber => "\u{25B2}",
            Self::Amethyst => "\u{2666}",
            Self::Aqua => "\u{2764}",
        }
    }
}

/// Special gem abilities created by larger matches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SpecialKind {
    /// Normal gem with no special ability.
    None,
    /// Line clear: clears the entire row or column (created by 4-match).
    LineClearH,
    LineClearV,
    /// Color bomb: clears all gems of one color (created by 5-match).
    ColorBomb,
}

/// A single gem on the board.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Gem {
    gem_type: GemType,
    special: SpecialKind,
}

impl Gem {
    fn new(gem_type: GemType) -> Self {
        Self {
            gem_type,
            special: SpecialKind::None,
        }
    }

    fn with_special(gem_type: GemType, special: SpecialKind) -> Self {
        Self { gem_type, special }
    }
}

// ── Board position ──────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Pos {
    row: usize,
    col: usize,
}

impl Pos {
    const fn new(row: usize, col: usize) -> Self {
        Self { row, col }
    }

    fn in_bounds(self) -> bool {
        self.row < GRID_SIZE && self.col < GRID_SIZE
    }

    /// Returns true if `other` is horizontally or vertically adjacent.
    fn is_adjacent(self, other: Pos) -> bool {
        let dr = self.row.abs_diff(other.row);
        let dc = self.col.abs_diff(other.col);
        (dr == 1 && dc == 0) || (dr == 0 && dc == 1)
    }
}

// ── Match info ──────────────────────────────────────────────────────
/// Describes a match found on the board.
#[derive(Clone, Debug)]
struct MatchInfo {
    /// All positions involved in this match.
    positions: Vec<Pos>,
    /// Whether this match is horizontal.
    horizontal: bool,
    /// Length of the match.
    length: usize,
}

impl MatchInfo {
    fn score(&self) -> u32 {
        match self.length {
            3 => SCORE_3,
            4 => SCORE_4,
            _ => SCORE_5,
        }
    }
}

// ── Game mode ───────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GameMode {
    /// Play until no valid moves remain.
    Classic,
    /// Play for a fixed time.
    Timed,
    /// Play with a fixed number of moves.
    Moves,
}

impl GameMode {
    fn label(self) -> &'static str {
        match self {
            GameMode::Classic => "Classic",
            GameMode::Timed => "Timed",
            GameMode::Moves => "Moves",
        }
    }
}

// ── Game state ──────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GameState {
    /// Waiting for player input.
    Idle,
    /// A gem is selected, waiting for swap target.
    Selected,
    /// Game is over.
    GameOver,
}

// ── High scores ─────────────────────────────────────────────────────
#[derive(Clone, Debug)]
struct HighScores {
    classic: u32,
    timed: u32,
    moves: u32,
}

impl HighScores {
    fn new() -> Self {
        Self {
            classic: 0,
            timed: 0,
            moves: 0,
        }
    }

    fn get(&self, mode: GameMode) -> u32 {
        match mode {
            GameMode::Classic => self.classic,
            GameMode::Timed => self.timed,
            GameMode::Moves => self.moves,
        }
    }

    fn update(&mut self, mode: GameMode, score: u32) {
        let current = match mode {
            GameMode::Classic => &mut self.classic,
            GameMode::Timed => &mut self.timed,
            GameMode::Moves => &mut self.moves,
        };
        if score > *current {
            *current = score;
        }
    }
}

// ── Layout ──────────────────────────────────────────────────────────

/// What a click can land on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Target {
    /// A cell of the board.
    Cell(Pos),
    /// A mode's button: a new game in it.
    Mode(GameMode),
    /// Show a move that makes a match.
    Hint,
    /// A new board in the same mode.
    NewGame,
    /// The game-over panel: a click on it starts the next game.
    GameOver,
}

type Frame = guitk::frame::Frame<Target>;

/// Every rectangle in the window, from the window's own size.
///
/// Built for each frame and each click and never stored, so the picture and
/// what a click is read against cannot disagree.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Layout {
    window: Rect,
    /// The band with the mode, the score, the best and what is left.
    header: Rect,
    /// The row of controls: the three modes, Hint, New game.
    controls: Rect,
    /// The board's frame, which the grid sits in.
    board: Rect,
    /// A cell's side, the gap between cells, and the frame's width.
    cell: f32,
    gap: f32,
    frame: f32,
    /// The key line; empty when the window has no room for it.
    footer: Rect,
    font: f32,
    small: f32,
    pad: f32,
}

impl Layout {
    fn new(width: f32, height: f32) -> Self {
        let w = width.max(1.0);
        let h = height.max(1.0);
        let pad = (w.min(h) * 0.025).clamp(3.0, 14.0);
        let font = (h / 34.0).clamp(8.0, 18.0);
        let small = (font - 3.0).max(7.0);

        // What each band would like, in [header, controls, footer] order,
        // and the order they give way in when the board would fall under
        // its share: the key line, the controls, then the header.
        let mut wants = [
            text::line_height(font, FontWeightHint::Bold) * 2.0 + pad * 2.0,
            (small * 2.2).max(12.0),
            text::line_height(small, FontWeightHint::Regular) + pad,
        ];
        let budget = (h - h * BOARD_SHARE - pad * 4.0).max(0.0);
        for i in [2, 1, 0] {
            if wants.iter().sum::<f32>() <= budget {
                break;
            }
            if let Some(band) = wants.get_mut(i) {
                *band = 0.0;
            }
        }
        let [header_h, controls_h, footer_h] = wants;
        let inner_w = (w - pad * 2.0).max(0.0);
        let header = if header_h > 0.0 {
            Rect::new(pad, pad, inner_w, header_h)
        } else {
            Rect::EMPTY
        };
        let top = if header_h > 0.0 { header.bottom() } else { 0.0 };
        let controls = if controls_h > 0.0 {
            Rect::new(pad, top + pad, inner_w, controls_h)
        } else {
            Rect::EMPTY
        };
        let top = if controls_h > 0.0 {
            controls.bottom()
        } else {
            top
        };
        let footer = if footer_h > 0.0 {
            Rect::new(pad, h - footer_h, inner_w, footer_h)
        } else {
            Rect::EMPTY
        };
        let bottom = if footer_h > 0.0 { footer.y } else { h };

        // The board: the largest square the rest holds, its cells a whole
        // number of pixels so every cell is the same size.
        let free = Rect::new(pad, top + pad, inner_w, (bottom - top - pad * 2.0).max(0.0));
        let side = free.w.min(free.h);
        let n = GRID_SIZE as f32;
        let gap = (side * 0.006).clamp(1.0, 3.0);
        let frame = gap * 2.0;
        let cell = ((side - frame * 2.0 - gap * (n - 1.0)) / n)
            .floor()
            .max(0.0);
        let board_side = if cell > 0.0 {
            cell * n + gap * (n - 1.0) + frame * 2.0
        } else {
            0.0
        };
        let board = Rect::new(
            free.x + (free.w - board_side) / 2.0,
            free.y + (free.h - board_side) / 2.0,
            board_side,
            board_side,
        );
        Self {
            window: Rect::new(0.0, 0.0, w, h),
            header,
            controls,
            board,
            cell,
            gap,
            frame,
            footer,
            font,
            small,
            pad,
        }
    }

    /// The cell at `pos`.
    fn cell_rect(&self, pos: Pos) -> Rect {
        let step = self.cell + self.gap;
        Rect::new(
            self.board.x + self.frame + pos.col as f32 * step,
            self.board.y + self.frame + pos.row as f32 * step,
            self.cell,
            self.cell,
        )
    }

    /// A cell's corner radius, scaled with the cell.
    fn radius(&self) -> f32 {
        (self.cell * 0.125).min(6.0)
    }

    /// The `index`th of `count` evenly spaced buttons filling `row`.
    fn nth_of(row: Rect, count: usize, index: usize) -> Rect {
        let n = count.max(1) as f32;
        let gap = (row.w * 0.012).min(6.0);
        let bw = ((row.w - gap * (n - 1.0)) / n).max(0.0);
        Rect::new(row.x + index as f32 * (bw + gap), row.y, bw, row.h.max(0.0))
    }
}

// ── Drawing helpers ─────────────────────────────────────────────────

/// A filled rectangle; nothing when it is empty.
fn fill(f: &mut Frame, r: Rect, color: Color, radius: f32) {
    if r.w <= 0.0 || r.h <= 0.0 {
        return;
    }
    f.push(RenderCommand::FillRect {
        x: r.x,
        y: r.y,
        width: r.w,
        height: r.h,
        color,
        corner_radii: CornerRadii::all(radius),
    });
}

/// `r` shrunk by `by` on every side.
fn inset(r: Rect, by: f32) -> Rect {
    Rect::new(
        r.x + by,
        r.y + by,
        (r.w - by * 2.0).max(0.0),
        (r.h - by * 2.0).max(0.0),
    )
}

/// `r` grown by `by` on every side.
fn grow(r: Rect, by: f32) -> Rect {
    Rect::new(r.x - by, r.y - by, r.w + by * 2.0, r.h + by * 2.0)
}

/// `s` from (`x`, `y`), its top left, cut with an ellipsis at `max` wide.
fn text_left(
    f: &mut Frame,
    (x, y): (f32, f32),
    s: &str,
    (size, weight): (f32, FontWeightHint),
    color: Color,
    max: f32,
) {
    if max <= 0.0 {
        return;
    }
    f.push(RenderCommand::Text {
        x,
        y,
        text: s.to_string(),
        color,
        font_size: size,
        font_weight: weight,
        max_width: Some(max),
        overflow: TextOverflow::Ellipsis,
    });
}

/// `s` ending at `right`, cut with an ellipsis at `max` wide.
fn text_right(
    f: &mut Frame,
    (right, y): (f32, f32),
    s: &str,
    (size, weight): (f32, FontWeightHint),
    color: Color,
    max: f32,
) {
    let w = text::measure(s, size, weight).min(max);
    text_left(f, (right - w, y), s, (size, weight), color, max);
}

/// `s` centred in `r`, measured rather than placed at a guessed offset, and
/// cut with an ellipsis if `r` is narrower than it.
fn centred(f: &mut Frame, r: Rect, s: &str, (size, weight): (f32, FontWeightHint), color: Color) {
    let w = text::measure(s, size, weight).min(r.w);
    let h = text::line_height(size, weight);
    let x = r.x + (r.w - w).max(0.0) / 2.0;
    let y = r.y + (r.h - h).max(0.0) / 2.0;
    text_left(f, (x, y), s, (size, weight), color, r.w);
}

/// A gem in the cell `r`: its body in its own colour, its symbol centred on
/// it, and a special's marks -- each scaled with the cell. The symbol stood
/// at eyeballed offsets from the middle, right for one font at one size.
fn draw_gem(f: &mut Frame, l: &Layout, r: Rect, gem: Gem) {
    let inset_by = (l.cell * 0.08).max(1.0);
    let body = inset(r, inset_by);
    fill(f, body, gem.gem_type.color(), l.radius());
    let size = (l.cell * 0.46).max(6.0);
    centred(
        f,
        r,
        gem.gem_type.symbol(),
        (size, FontWeightHint::Bold),
        GEM_INK,
    );
    let line = (l.cell * 0.04).max(1.0);
    let (cx, cy) = r.centre();
    match gem.special {
        SpecialKind::LineClearH => f.push(RenderCommand::Line {
            x1: body.x + inset_by * 0.5,
            y1: cy,
            x2: body.right() - inset_by * 0.5,
            y2: cy,
            color: GEM_INK,
            width: line,
        }),
        SpecialKind::LineClearV => f.push(RenderCommand::Line {
            x1: cx,
            y1: body.y + inset_by * 0.5,
            x2: cx,
            y2: body.bottom() - inset_by * 0.5,
            color: GEM_INK,
            width: line,
        }),
        SpecialKind::ColorBomb => {
            // Four dots in the body's corners.
            let dot = (l.cell * 0.06).max(1.0);
            for (x, y) in [
                (body.x + dot, body.y + dot),
                (body.right() - dot, body.y + dot),
                (body.x + dot, body.bottom() - dot),
                (body.right() - dot, body.bottom() - dot),
            ] {
                fill(
                    f,
                    Rect::new(x - dot, y - dot, dot * 2.0, dot * 2.0),
                    GEM_INK,
                    dot,
                );
            }
        }
        SpecialKind::None => {}
    }
}

/// The key line at the foot, when the window has room for one.
fn draw_footer(f: &mut Frame, l: &Layout, c: &Colours) {
    if l.footer.is_empty() {
        return;
    }
    centred(
        f,
        l.footer,
        KEYS,
        (l.small, FontWeightHint::Regular),
        c.chrome.dim,
    );
}

// ── Main app struct ─────────────────────────────────────────────────
struct Match3 {
    /// The 8x8 board of gems. `board[row][col]`.
    board: [[Option<Gem>; GRID_SIZE]; GRID_SIZE],
    /// Current game state.
    state: GameState,
    /// Current game mode.
    mode: GameMode,
    /// Current score.
    score: u32,
    /// Current cascade chain level (0 = no cascade).
    chain_level: u32,
    /// Cursor position (for keyboard navigation).
    cursor: Pos,
    /// Currently selected gem position (for swap).
    selected: Option<Pos>,
    /// Hint position pair (source, target).
    hint: Option<(Pos, Pos)>,
    /// Milliseconds since last user action (for hint timer).
    idle_ms: u64,
    /// Whether the hint is currently visible.
    hint_visible: bool,
    /// Remaining time in milliseconds (Timed mode).
    time_remaining_ms: u64,
    /// Remaining moves (Moves mode).
    moves_remaining: u32,
    /// High scores per mode.
    high_scores: HighScores,
    /// RNG state.
    rng: SeededRng,
    /// Animation pulse counter.
    pulse_counter: u32,
    /// Total elapsed time in ms.
    total_elapsed_ms: u64,
    /// The user's colours, replaced whenever the theme changes. Seeded from
    /// the defaults; the framework calls `App::theme_changed` before the
    /// first frame.
    palette: Palette,
    /// The window's size, as the window last set it: the size the next
    /// click is read against.
    width: f32,
    height: f32,
}

impl Match3 {
    fn new() -> Self {
        // Was `with_seed(42)`: every player, on every machine, got the same
        // opening board and the same gems falling into it. The `u64` form is
        // used and not the generator form because this app *stores* its seed --
        // `new_game` draws the next one from the live generator, and an app
        // holding a generator instead would have to reseed one generator from
        // another's output, which silently correlates the two.
        Self::with_seed(seed_from_system(FALLBACK_SEED))
    }

    fn with_seed(seed: u64) -> Self {
        let mut app = Self {
            board: [[None; GRID_SIZE]; GRID_SIZE],
            state: GameState::Idle,
            mode: GameMode::Classic,
            score: 0,
            chain_level: 0,
            cursor: Pos::new(0, 0),
            selected: None,
            hint: None,
            idle_ms: 0,
            hint_visible: false,
            time_remaining_ms: TIMED_MODE_SECONDS * 1000,
            moves_remaining: MOVES_MODE_COUNT,
            high_scores: HighScores::new(),
            rng: SeededRng::new(seed),
            pulse_counter: 0,
            total_elapsed_ms: 0,
            palette: Palette::for_mode(false),
            width: WINDOW_WIDTH,
            height: WINDOW_HEIGHT,
        };
        app.fill_board_no_matches();
        app
    }

    // ── Board initialization ────────────────────────────────────────

    /// Fill the board with random gems, ensuring no initial matches.
    fn fill_board_no_matches(&mut self) {
        for row in 0..GRID_SIZE {
            for col in 0..GRID_SIZE {
                let gem = self.random_gem_no_match(row, col);
                self.set_gem(row, col, Some(gem));
            }
        }
    }

    /// Generate a random gem for (row, col) that does not create a match.
    fn random_gem_no_match(&mut self, row: usize, col: usize) -> Gem {
        loop {
            let gem_type = GemType::from_index(self.rng.below(GEM_TYPE_COUNT as usize) as u8);
            let gem = Gem::new(gem_type);
            // Check horizontal: if two to the left are the same type, skip.
            if col >= 2
                && let (Some(a), Some(b)) = (
                    self.get_gem(row, col.saturating_sub(1)),
                    self.get_gem(row, col.saturating_sub(2)),
                )
                && a.gem_type == gem_type
                && b.gem_type == gem_type
            {
                continue;
            }
            // Check vertical: if two above are the same type, skip.
            if row >= 2
                && let (Some(a), Some(b)) = (
                    self.get_gem(row.saturating_sub(1), col),
                    self.get_gem(row.saturating_sub(2), col),
                )
                && a.gem_type == gem_type
                && b.gem_type == gem_type
            {
                continue;
            }
            return gem;
        }
    }

    /// Generate a random gem type.
    fn random_gem(&mut self) -> Gem {
        let gem_type = GemType::from_index(self.rng.below(GEM_TYPE_COUNT as usize) as u8);
        Gem::new(gem_type)
    }

    // ── Match detection ─────────────────────────────────────────────

    /// Find all matches on the board. Returns a list of match groups.
    fn find_matches(&self) -> Vec<MatchInfo> {
        let mut matches = Vec::new();

        // Horizontal matches.
        for row in 0..GRID_SIZE {
            let mut col = 0;
            while col < GRID_SIZE {
                if let Some(gem) = self.get_gem(row, col) {
                    let gem_type = gem.gem_type;
                    let start = col;
                    while col < GRID_SIZE {
                        if let Some(g) = self.get_gem(row, col) {
                            if g.gem_type == gem_type {
                                col = col.saturating_add(1);
                            } else {
                                break;
                            }
                        } else {
                            break;
                        }
                    }
                    let length = col.saturating_sub(start);
                    if length >= 3 {
                        let positions: Vec<Pos> = (start..col).map(|c| Pos::new(row, c)).collect();
                        matches.push(MatchInfo {
                            positions,
                            horizontal: true,
                            length,
                        });
                    }
                } else {
                    col = col.saturating_add(1);
                }
            }
        }

        // Vertical matches.
        for col in 0..GRID_SIZE {
            let mut row = 0;
            while row < GRID_SIZE {
                if let Some(gem) = self.get_gem(row, col) {
                    let gem_type = gem.gem_type;
                    let start = row;
                    while row < GRID_SIZE {
                        if let Some(g) = self.get_gem(row, col) {
                            if g.gem_type == gem_type {
                                row = row.saturating_add(1);
                            } else {
                                break;
                            }
                        } else {
                            break;
                        }
                    }
                    let length = row.saturating_sub(start);
                    if length >= 3 {
                        let positions: Vec<Pos> = (start..row).map(|r| Pos::new(r, col)).collect();
                        matches.push(MatchInfo {
                            positions,
                            horizontal: false,
                            length,
                        });
                    }
                } else {
                    row = row.saturating_add(1);
                }
            }
        }

        matches
    }

    /// Check if a specific swap would create any match.
    fn swap_creates_match(&mut self, a: Pos, b: Pos) -> bool {
        // Perform swap.
        let tmp = self.get_gem(a.row, a.col);
        self.set_gem(a.row, a.col, self.get_gem(b.row, b.col));
        self.set_gem(b.row, b.col, tmp);

        let has_match = !self.find_matches().is_empty();

        // Undo swap.
        let tmp = self.get_gem(a.row, a.col);
        self.set_gem(a.row, a.col, self.get_gem(b.row, b.col));
        self.set_gem(b.row, b.col, tmp);

        has_match
    }

    // ── Match processing ────────────────────────────────────────────

    /// Remove matched gems and award score. Returns true if any matches were removed.
    fn process_matches(&mut self) -> bool {
        let matches = self.find_matches();
        if matches.is_empty() {
            return false;
        }

        // Calculate score with cascade multiplier.
        let multiplier = self.cascade_multiplier();
        for m in &matches {
            let base = m.score();
            let scored = base.saturating_mul(multiplier) / FP_BASE;
            self.score = self.score.saturating_add(scored);
        }

        // Create special gems and mark positions for removal.
        let mut to_remove = Vec::new();
        for m in &matches {
            // Determine if a special gem should be created.
            let special_pos = self.determine_special_gem(m);
            for &pos in &m.positions {
                // Handle existing special gems being matched.
                if let Some(gem) = self.get_gem(pos.row, pos.col) {
                    match gem.special {
                        SpecialKind::LineClearH => {
                            // Clear entire row.
                            for c in 0..GRID_SIZE {
                                let p = Pos::new(pos.row, c);
                                if !to_remove.contains(&p) {
                                    to_remove.push(p);
                                }
                            }
                        }
                        SpecialKind::LineClearV => {
                            // Clear entire column.
                            for r in 0..GRID_SIZE {
                                let p = Pos::new(r, pos.col);
                                if !to_remove.contains(&p) {
                                    to_remove.push(p);
                                }
                            }
                        }
                        SpecialKind::ColorBomb => {
                            // Clear all gems of the matched type.
                            let target_type = gem.gem_type;
                            for r in 0..GRID_SIZE {
                                for c in 0..GRID_SIZE {
                                    if let Some(g) = self.get_gem(r, c)
                                        && g.gem_type == target_type
                                    {
                                        let p = Pos::new(r, c);
                                        if !to_remove.contains(&p) {
                                            to_remove.push(p);
                                        }
                                    }
                                }
                            }
                        }
                        SpecialKind::None => {}
                    }
                }
                if !to_remove.contains(&pos) {
                    to_remove.push(pos);
                }
            }

            // Place special gem if one was determined.
            if let Some((pos, special)) = special_pos
                && let Some(gem) = self.get_gem(pos.row, pos.col)
            {
                self.set_gem(
                    pos.row,
                    pos.col,
                    Some(Gem::with_special(gem.gem_type, special)),
                );
                // Remove this position from the removal list so the special gem survives.
                to_remove.retain(|&p| p != pos);
            }
        }

        // Remove matched gems.
        for pos in &to_remove {
            self.set_gem(pos.row, pos.col, None);
        }

        true
    }

    /// Determine if a match should create a special gem and where.
    fn determine_special_gem(&self, m: &MatchInfo) -> Option<(Pos, SpecialKind)> {
        if m.length == 4 {
            // 4-match: line clear gem at the center of the match.
            // `get`, because `length` and `positions.len()` are two facts
            // that are only equal by construction, and the construction is in
            // another function.
            let pos = *m.positions.get(m.length / 2)?;
            let special = if m.horizontal {
                SpecialKind::LineClearH
            } else {
                SpecialKind::LineClearV
            };
            Some((pos, special))
        } else if m.length >= 5 {
            // 5+ match: color bomb at the center.
            // `get`, because `length` and `positions.len()` are two facts
            // that are only equal by construction, and the construction is in
            // another function.
            let pos = *m.positions.get(m.length / 2)?;
            Some((pos, SpecialKind::ColorBomb))
        } else {
            None
        }
    }

    /// Calculate cascade multiplier as fixed-point (100 = 1.0x).
    fn cascade_multiplier(&self) -> u32 {
        let mut mult = FP_BASE;
        for _ in 0..self.chain_level {
            mult = mult.saturating_mul(CASCADE_MULTIPLIER_FP) / FP_BASE;
        }
        mult
    }

    /// Apply gravity: gems fall down to fill empty spaces.
    /// Returns true if any gems moved.
    fn apply_gravity(&mut self) -> bool {
        let mut moved = false;
        for col in 0..GRID_SIZE {
            // Compact column: move gems down to fill gaps.
            let mut write = GRID_SIZE;
            for read in (0..GRID_SIZE).rev() {
                if self.get_gem(read, col).is_some() {
                    write = write.saturating_sub(1);
                    if write != read {
                        self.set_gem(write, col, self.get_gem(read, col));
                        self.set_gem(read, col, None);
                        moved = true;
                    }
                }
            }
        }
        moved
    }

    /// Fill empty spaces at the top with new random gems.
    fn fill_empty_spaces(&mut self) {
        for col in 0..GRID_SIZE {
            for row in 0..GRID_SIZE {
                if self.get_gem(row, col).is_none() {
                    let gem = self.random_gem();
                    self.set_gem(row, col, Some(gem));
                }
            }
        }
    }

    /// Run the full cascade loop: match -> remove -> gravity -> fill -> repeat.
    /// Returns total score delta from cascades.
    fn run_cascade(&mut self) {
        self.chain_level = 0;
        loop {
            if !self.process_matches() {
                break;
            }
            self.chain_level = self.chain_level.saturating_add(1);
            self.apply_gravity();
            self.fill_empty_spaces();
        }
        self.chain_level = 0;
    }

    // ── Move validation ─────────────────────────────────────────────

    /// Find all valid moves on the board.
    fn find_valid_moves(&mut self) -> Vec<(Pos, Pos)> {
        let mut moves = Vec::new();
        // Check horizontal swaps.
        for row in 0..GRID_SIZE {
            for col in 0..GRID_SIZE - 1 {
                let a = Pos::new(row, col);
                let b = Pos::new(row, col.saturating_add(1));
                if self.swap_creates_match(a, b) {
                    moves.push((a, b));
                }
            }
        }
        // Check vertical swaps.
        for row in 0..GRID_SIZE - 1 {
            for col in 0..GRID_SIZE {
                let a = Pos::new(row, col);
                let b = Pos::new(row.saturating_add(1), col);
                if self.swap_creates_match(a, b) {
                    moves.push((a, b));
                }
            }
        }
        moves
    }

    /// Check if any valid moves exist.
    fn has_valid_moves(&mut self) -> bool {
        // Check horizontal swaps.
        for row in 0..GRID_SIZE {
            for col in 0..GRID_SIZE - 1 {
                let a = Pos::new(row, col);
                let b = Pos::new(row, col.saturating_add(1));
                if self.swap_creates_match(a, b) {
                    return true;
                }
            }
        }
        // Check vertical swaps.
        for row in 0..GRID_SIZE - 1 {
            for col in 0..GRID_SIZE {
                let a = Pos::new(row, col);
                let b = Pos::new(row.saturating_add(1), col);
                if self.swap_creates_match(a, b) {
                    return true;
                }
            }
        }
        false
    }

    /// Shuffle the board until valid moves exist.
    fn shuffle_board(&mut self) {
        let mut attempts: usize = 0;
        loop {
            // Fisher-Yates shuffle of all gems.
            let mut gems: Vec<Option<Gem>> = Vec::new();
            for row in 0..GRID_SIZE {
                for col in 0..GRID_SIZE {
                    gems.push(self.get_gem(row, col));
                }
            }
            self.rng.shuffle(&mut gems);
            // Place back on board.
            for row in 0..GRID_SIZE {
                for col in 0..GRID_SIZE {
                    let at = row.saturating_mul(GRID_SIZE).saturating_add(col);
                    self.set_gem(row, col, gems.get(at).copied().flatten());
                }
            }
            // Remove any existing matches first.
            while self.process_matches() {
                self.apply_gravity();
                self.fill_empty_spaces();
            }
            if self.has_valid_moves() {
                break;
            }
            attempts = attempts.saturating_add(1);
            if attempts > 100 {
                // Fallback: regenerate board from scratch.
                self.fill_board_no_matches();
                break;
            }
        }
    }

    // ── Hint system ─────────────────────────────────────────────────

    /// Find one valid move to use as a hint.
    fn find_hint(&mut self) -> Option<(Pos, Pos)> {
        let moves = self.find_valid_moves();
        if moves.is_empty() {
            None
        } else {
            let idx = self.rng.below(moves.len());
            moves.get(idx).copied()
        }
    }

    /// Update the hint timer and compute hint if needed.
    fn update_hint(&mut self, elapsed_ms: u64) {
        if self.state != GameState::Idle {
            self.hint_visible = false;
            return;
        }
        self.idle_ms = self.idle_ms.saturating_add(elapsed_ms);
        if self.idle_ms >= HINT_DELAY_MS && !self.hint_visible {
            self.hint = self.find_hint();
            self.hint_visible = true;
        }
    }

    /// Reset the idle timer (called on user action).
    fn reset_idle(&mut self) {
        self.idle_ms = 0;
        self.hint_visible = false;
        self.hint = None;
    }

    // ── Swap logic ──────────────────────────────────────────────────

    /// Attempt to swap two adjacent gems. Returns true if the swap was valid.
    /// Remove the bomb at `bomb` and every gem of `target` colour, and score it.
    ///
    /// Split out of `try_swap`, where it appeared twice verbatim -- once for a
    /// bomb in either position -- so that the two copies could not drift apart,
    /// and so that the clear can be observed on its own. It cannot be observed
    /// through `try_swap`: that goes on to `fill_empty_spaces`, which draws
    /// fresh gems into the holes, and a refilled cell may perfectly well hold
    /// the colour that was just cleared. A test that inspects the board
    /// afterwards is testing the refill, not the detonation, and passes or
    /// fails on which gems the generator happened to deal.
    fn detonate_color_bomb(&mut self, bomb: Pos, target: GemType) {
        self.set_gem(bomb.row, bomb.col, None);
        for r in 0..GRID_SIZE {
            for c in 0..GRID_SIZE {
                if let Some(g) = self.get_gem(r, c)
                    && g.gem_type == target
                {
                    self.set_gem(r, c, None);
                }
            }
        }
        self.score = self.score.saturating_add(SCORE_5 * 2);
    }

    /// Let gems fall into the holes a detonation left, refill, and cascade.
    fn settle_after_detonation(&mut self) {
        self.apply_gravity();
        self.fill_empty_spaces();
        self.run_cascade();
        self.consume_move();
    }

    fn try_swap(&mut self, a: Pos, b: Pos) -> bool {
        if !a.in_bounds() || !b.in_bounds() || !a.is_adjacent(b) {
            return false;
        }
        if self.get_gem(a.row, a.col).is_none() || self.get_gem(b.row, b.col).is_none() {
            return false;
        }

        // Check if either gem is a color bomb being swapped with a regular gem.
        let gem_a = self.get_gem(a.row, a.col);
        let gem_b = self.get_gem(b.row, b.col);
        if let (Some(ga), Some(gb)) = (gem_a, gem_b) {
            if ga.special == SpecialKind::ColorBomb && gb.special != SpecialKind::ColorBomb {
                self.detonate_color_bomb(a, gb.gem_type);
                self.settle_after_detonation();
                return true;
            }
            if gb.special == SpecialKind::ColorBomb && ga.special != SpecialKind::ColorBomb {
                self.detonate_color_bomb(b, ga.gem_type);
                self.settle_after_detonation();
                return true;
            }
        }

        // Check if swap creates a match.
        if !self.swap_creates_match(a, b) {
            return false;
        }

        // Perform the swap.
        let tmp = self.get_gem(a.row, a.col);
        self.set_gem(a.row, a.col, self.get_gem(b.row, b.col));
        self.set_gem(b.row, b.col, tmp);

        // Run cascade.
        self.run_cascade();

        // Consume a move in Moves mode.
        self.consume_move();

        // Check if no valid moves remain.
        if !self.has_valid_moves() {
            if self.mode == GameMode::Classic {
                self.end_game();
            } else {
                self.shuffle_board();
            }
        }

        true
    }

    /// Consume one move (for Moves mode tracking).
    fn consume_move(&mut self) {
        if self.mode == GameMode::Moves {
            self.moves_remaining = self.moves_remaining.saturating_sub(1);
            if self.moves_remaining == 0 {
                self.end_game();
            }
        }
    }

    /// End the current game.
    fn end_game(&mut self) {
        self.state = GameState::GameOver;
        self.high_scores.update(self.mode, self.score);
    }

    /// Start a new game with the current mode.
    fn new_game(&mut self) {
        let mode = self.mode;
        let high_scores = self.high_scores.clone();
        let palette = self.palette;
        let seed = self.rng.next_u64();
        *self = Self::with_seed(seed);
        self.mode = mode;
        self.high_scores = high_scores;
        // The window's colours, like the scores, outlive a game.
        self.palette = palette;
        self.time_remaining_ms = TIMED_MODE_SECONDS * 1000;
        self.moves_remaining = MOVES_MODE_COUNT;
    }

    /// Switch to a different game mode and start a new game.
    fn switch_mode(&mut self, mode: GameMode) {
        self.mode = mode;
        self.new_game();
    }

    // ── Window ──────────────────────────────────────────────────────

    /// Record the size the window is now, which is the size the next click
    /// is read against.
    fn resize(&mut self, width: f32, height: f32) {
        self.width = width.max(1.0);
        self.height = height.max(1.0);
    }

    /// What a click at (`x`, `y`) lands on, read from the frame the window
    /// is showing. The boxes were recorded by the pass that drew them, so
    /// there is no second copy of the geometry to get wrong.
    fn target_at(&self, x: f32, y: f32) -> Option<Target> {
        self.frame(self.width, self.height).hit_test(x, y)
    }

    // ── Drawing ─────────────────────────────────────────────────────

    /// The whole window at `width` x `height`, with a box recorded for
    /// everything a click can do.
    fn frame(&self, width: f32, height: f32) -> Frame {
        let l = Layout::new(width, height);
        let c = Colours::of(&self.palette);
        let mut f = Frame::new(l.window.w, l.window.h);
        fill(&mut f, l.window, c.chrome.page, 0.0);
        self.draw_header(&mut f, &l, &c);
        self.draw_controls(&mut f, &l, &c);
        self.draw_board(&mut f, &l, &c);
        draw_footer(&mut f, &l, &c);
        if self.state == GameState::GameOver {
            self.draw_game_over(&mut f, &l, &c);
        }
        f
    }

    /// The mode and the score on the left, the best and what is left of
    /// the game on the right, each cut with an ellipsis before it reaches
    /// the other half.
    fn draw_header(&self, f: &mut Frame, l: &Layout, c: &Colours) {
        let r = l.header;
        if r.is_empty() {
            return;
        }
        fill(f, r, c.chrome.raised, (r.h * 0.18).min(8.0));
        // Every word here is written for the raised band, not the page: the
        // page's inks were 3.6:1 (the mode, the best score) and 4.1:1 (the
        // grey) on it in a light theme.
        let on = c.chrome.on(c.chrome.raised);
        let inner = inset(r, l.pad);
        let half = (inner.w / 2.0 - l.pad / 2.0).max(0.0);
        let line = text::line_height(l.font, FontWeightHint::Bold);
        let top = inner.y + (inner.h - line * 2.0).max(0.0) / 2.0;
        let bold = FontWeightHint::Bold;
        let regular = FontWeightHint::Regular;
        let mode = format!("Mode: {}", self.mode.label());
        text_left(f, (inner.x, top), &mode, (l.font, bold), on.title, half);
        let score = format!("Score: {}", self.score);
        text_left(
            f,
            (inner.x, top + line),
            &score,
            (l.font, bold),
            on.text,
            half,
        );
        let best = format!("Best: {}", self.high_scores.get(self.mode));
        text_right(
            f,
            (inner.right(), top),
            &best,
            (l.font, regular),
            on.even,
            half,
        );
        let (limit, colour, weight) = match self.mode {
            GameMode::Timed => {
                let secs = self.time_remaining_ms / 1000;
                let colour = if secs <= 10 { on.bad } else { on.good };
                (format!("Time: {secs}s"), colour, bold)
            }
            GameMode::Moves => {
                let colour = if self.moves_remaining <= 5 {
                    on.bad
                } else {
                    on.key
                };
                (format!("Moves: {}", self.moves_remaining), colour, bold)
            }
            GameMode::Classic => ("No limit".to_string(), on.dim, regular),
        };
        text_right(
            f,
            (inner.right(), top + line),
            &limit,
            (l.font, weight),
            colour,
            half,
        );
    }

    /// The modes, Hint and New game: the toolkit's buttons, the mode being
    /// played the primary one. Hint is switched off once the game is over.
    fn draw_controls(&self, f: &mut Frame, l: &Layout, c: &Colours) {
        let r = l.controls;
        if r.is_empty() {
            return;
        }
        let over = self.state == GameState::GameOver;
        for (i, (target, label)) in CONTROLS.iter().enumerate() {
            let b = Layout::nth_of(r, CONTROLS.len(), i);
            let (kind, disabled) = match *target {
                Target::Mode(mode) if mode == self.mode => (Kind::Primary, false),
                Target::Hint => (Kind::Plain, over),
                _ => (Kind::Plain, false),
            };
            let state = State {
                disabled,
                ..State::default()
            };
            gamechrome::button(
                f,
                &self.palette,
                (b.x, b.y, b.w, b.h),
                label,
                l.small,
                kind,
                state,
                c.chrome.page,
            );
            if !disabled {
                f.hit(*target, b);
            }
        }
    }

    /// The board: its frame, the cells, the gems, and the glows for the
    /// keyboard's cell, the gem picked up and a hint.
    fn draw_board(&self, f: &mut Frame, l: &Layout, c: &Colours) {
        if l.board.is_empty() || l.cell <= 0.0 {
            return;
        }
        let radius = l.radius();
        fill(f, l.board, c.chrome.raised, (radius + l.frame).min(10.0));
        let over = self.state == GameState::GameOver;
        for (row, cells) in self.board.iter().enumerate() {
            for (col, gem) in cells.iter().enumerate() {
                let pos = Pos::new(row, col);
                let r = l.cell_rect(pos);
                let shade = if row.saturating_add(col) % 2 == 0 {
                    c.chrome.lit
                } else {
                    c.chrome.raised
                };
                fill(f, r, shade, radius);
                if let Some(gem) = gem {
                    draw_gem(f, l, r, *gem);
                }
                // A cell takes a click while the game is on; over, the
                // panel above it answers.
                if !over {
                    f.hit(Target::Cell(pos), r);
                }
            }
        }
        if !over {
            fill(
                f,
                grow(l.cell_rect(self.cursor), l.gap),
                c.cursor,
                radius + l.gap,
            );
        }
        if let Some(sel) = self.selected {
            let glow = l.gap * 1.5;
            fill(f, grow(l.cell_rect(sel), glow), c.selected, radius + glow);
        }
        if self.hint_visible
            && let Some((a, b)) = self.hint
        {
            // A glow that pulses once a second.
            let phase = (self.pulse_counter % 60) as f32 / 60.0 * std::f32::consts::TAU;
            let alpha = (40.0 + phase.sin() * 40.0) as u8;
            let glow = l.gap * 1.5;
            for pos in [a, b] {
                fill(
                    f,
                    grow(l.cell_rect(pos), glow),
                    with_alpha(c.hint, alpha),
                    radius + glow,
                );
            }
        }
    }

    /// The game over, on the toolkit's panel over the veiled board: the
    /// panel is sized to its words and takes the click that starts the
    /// next game.
    fn draw_game_over(&self, f: &mut Frame, l: &Layout, c: &Colours) {
        fill(f, l.board, c.chrome.veil, (l.radius() + l.frame).min(10.0));
        // On the panel the chrome's roles read as they are: the palette inks
        // its text colours for its own panel (`Palette::ink`). Over the veil
        // alone the words sat on whichever gem was beneath -- the best score
        // at 4.1:1 over a teal one in a light theme.
        let bold = FontWeightHint::Bold;
        let regular = FontWeightHint::Regular;
        let lines = [
            ("GAME OVER".to_string(), l.font * 1.6, bold, c.chrome.bad),
            (
                format!("Final score: {}", self.score),
                l.font,
                regular,
                c.chrome.text,
            ),
            (
                format!("Best: {}", self.high_scores.get(self.mode)),
                l.font,
                regular,
                c.chrome.even,
            ),
            (
                "Click here, or N, for a new game".to_string(),
                l.small,
                regular,
                c.chrome.dim,
            ),
        ];
        let gap = l.pad * 0.5;
        let widest = lines
            .iter()
            .map(|(s, size, weight, _)| text::measure(s, *size, *weight))
            .fold(0.0_f32, f32::max);
        let total: f32 = lines
            .iter()
            .map(|(_, size, weight, _)| text::line_height(*size, *weight))
            .sum::<f32>()
            + gap * (lines.len() as f32 - 1.0);
        let (cx, cy) = l.board.centre();
        let block = Rect::new(
            cx - widest / 2.0 - l.pad,
            cy - total / 2.0 - l.pad,
            widest + l.pad * 2.0,
            total + l.pad * 2.0,
        );
        // Cut to the window, and not drawn under a point, where the
        // toolkit's border would reach outside it.
        let Some(panel) = block.intersect(l.window) else {
            return;
        };
        if panel.w >= 1.0 && panel.h >= 1.0 {
            self.palette.push_surface(
                f,
                panel.x,
                panel.y,
                panel.w,
                panel.h,
                l.pad * 0.6,
                Surface::Panel,
            );
        }
        let mut y = cy - total / 2.0;
        for (s, size, weight, colour) in &lines {
            let h = text::line_height(*size, *weight);
            centred(
                f,
                Rect::new(panel.x, y, panel.w, h),
                s,
                (*size, *weight),
                *colour,
            );
            y += h + gap;
        }
        f.hit(Target::GameOver, panel);
    }

    // ── Event handling ──────────────────────────────────────────────

    fn handle_event(&mut self, event: &Event) {
        match event {
            Event::Key(ke) if ke.pressed => {
                self.handle_key(ke.key);
            }
            Event::Mouse(me) => self.handle_mouse(me),
            Event::Tick { elapsed_ms } => self.handle_tick(*elapsed_ms),
            _ => {}
        }
    }

    fn handle_key(&mut self, key: Key) {
        self.reset_idle();

        match key {
            Key::N => {
                self.new_game();
                return;
            }
            Key::Num1 => {
                self.switch_mode(GameMode::Classic);
                return;
            }
            Key::Num2 => {
                self.switch_mode(GameMode::Timed);
                return;
            }
            Key::Num3 => {
                self.switch_mode(GameMode::Moves);
                return;
            }
            Key::H => {
                self.show_hint();
                return;
            }
            _ => {}
        }

        if self.state == GameState::GameOver {
            return;
        }

        match key {
            Key::Left if self.cursor.col > 0 => {
                self.cursor.col = self.cursor.col.saturating_sub(1);
            }
            Key::Right if self.cursor.col < GRID_SIZE - 1 => {
                self.cursor.col = self.cursor.col.saturating_add(1);
            }
            Key::Up if self.cursor.row > 0 => {
                self.cursor.row = self.cursor.row.saturating_sub(1);
            }
            Key::Down if self.cursor.row < GRID_SIZE - 1 => {
                self.cursor.row = self.cursor.row.saturating_add(1);
            }
            Key::Enter | Key::Space => {
                self.select_or_swap(self.cursor);
            }
            Key::Escape => {
                self.selected = None;
                self.state = GameState::Idle;
            }
            _ => {}
        }
    }

    /// A left press, answered by what it lands on in the frame the window
    /// is showing.
    fn handle_mouse(&mut self, me: &MouseEvent) {
        if !matches!(me.kind, MouseEventKind::Press(MouseButton::Left)) {
            return;
        }
        self.reset_idle();
        match self.target_at(me.x, me.y) {
            Some(Target::Cell(pos)) if self.state != GameState::GameOver => {
                self.select_or_swap(pos);
            }
            Some(Target::Mode(mode)) => self.switch_mode(mode),
            Some(Target::Hint) => self.show_hint(),
            Some(Target::NewGame | Target::GameOver) => self.new_game(),
            _ => {}
        }
    }

    /// Show a move that makes a match, while the game is on.
    fn show_hint(&mut self) {
        if self.state != GameState::GameOver {
            self.hint = self.find_hint();
            self.hint_visible = true;
        }
    }

    fn select_or_swap(&mut self, pos: Pos) {
        match self.state {
            GameState::Idle => {
                self.selected = Some(pos);
                self.state = GameState::Selected;
            }
            GameState::Selected => {
                if let Some(sel) = self.selected {
                    if sel == pos {
                        // Deselect.
                        self.selected = None;
                        self.state = GameState::Idle;
                    } else if sel.is_adjacent(pos) {
                        // Attempt swap.
                        self.try_swap(sel, pos);
                        self.selected = None;
                        self.state = GameState::Idle;
                    } else {
                        // Select new gem.
                        self.selected = Some(pos);
                    }
                }
            }
            GameState::GameOver => {}
        }
    }

    fn handle_tick(&mut self, elapsed_ms: u64) {
        self.total_elapsed_ms = self.total_elapsed_ms.saturating_add(elapsed_ms);
        self.pulse_counter = self.pulse_counter.wrapping_add(1);
        self.update_hint(elapsed_ms);

        if self.state != GameState::GameOver && self.mode == GameMode::Timed {
            self.time_remaining_ms = self.time_remaining_ms.saturating_sub(elapsed_ms);
            if self.time_remaining_ms == 0 {
                self.end_game();
            }
        }
    }

    // ── Board access ─────────────────────────────────────────────────────
    //
    // The only two places that reason about the board's bounds. They were
    // `(for testing)` helpers sitting beside forty direct
    // `self.get_gem(row, col)` subscripts, which is the arrangement that
    // produced 127 of this crate's clippy `indexing_slicing` warnings -- the
    // whole of the tree's remaining total, in one file. A bounds test written
    // next to each subscript is a test somebody forgets; two accessors are two
    // places to be right.

    /// The gem at `(row, col)`, or `None` -- for an empty cell and for a
    /// coordinate off the board alike.
    ///
    /// The two are deliberately one answer. Every caller is either inside a
    /// `0..GRID_SIZE` loop or holding a `Pos` the input path has already
    /// vetted, so an out-of-range read is a bug and not an input; what it must
    /// not do is end the player's game with a panic. An empty cell is the
    /// nearest true thing to say about a square that is not there.
    fn get_gem(&self, row: usize, col: usize) -> Option<Gem> {
        self.board
            .get(row)
            .and_then(|r| r.get(col))
            .copied()
            .flatten()
    }

    /// Put `gem` at `(row, col)`, or do nothing if that is off the board.
    ///
    /// `debug_assert` rather than a silent drop *and* rather than a panic: a
    /// write off the board is a logic error worth failing a test over, and
    /// worth surviving in a released game.
    fn set_gem(&mut self, row: usize, col: usize, gem: Option<Gem>) {
        debug_assert!(
            row < GRID_SIZE && col < GRID_SIZE,
            "write off the board at ({row}, {col})"
        );
        if let Some(cell) = self.board.get_mut(row).and_then(|r| r.get_mut(col)) {
            *cell = gem;
        }
    }
}

/// Test support: a board set up by hand, and a count of what is on it. The
/// game itself only ever fills a board by dealing, so these live behind
/// `cfg(test)` rather than widening what the program can do.
#[cfg(test)]
impl Match3 {
    /// Clear the board.
    fn clear_board(&mut self) {
        self.board = [[None; GRID_SIZE]; GRID_SIZE];
    }

    /// Count non-empty cells.
    fn gem_count(&self) -> usize {
        self.board.iter().flatten().filter(|g| g.is_some()).count()
    }
}

#[cfg(test)]
impl GemType {
    /// Where the kind stands in the order `from_index` reads.
    fn index(self) -> usize {
        self as usize
    }
}

/// What to do about this program's first command-line argument.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArgVerdict {
    /// No argument: get on with it.
    Run,
    Help,
    Version,
    /// Anything else. This program has no options.
    Refuse,
}

/// Classify the first argument, if there is one.
///
/// Filed by lane B on 2026-09-12: this program accepted
/// `--zzq-not-an-option` and exited 0, which to anything reading an exit
/// status says the option was honoured. It was not -- nothing here had ever
/// looked at `argv`. That is the more common shape of what lane B swept out
/// of `userspace/`: not a parse loop with a careless `_ => {}` arm, but no
/// parse at all.
///
/// Takes `&OsStr`, not `&str`, and that is the point of the signature. An
/// argument can hold any byte; decoding it first would mean an undecodable
/// option name arriving here as replacement characters, which is a string
/// nobody passed. Undecodable is simply not one of the two options this
/// program has, so it is refused like any other.
///
/// `--help` and `--version` are answered rather than refused, so that the
/// refusal is a statement about the interface rather than the absence of one.
fn classify_argument(first: Option<&std::ffi::OsStr>) -> ArgVerdict {
    match first.and_then(std::ffi::OsStr::to_str) {
        None if first.is_none() => ArgVerdict::Run,
        Some("--help" | "-h") => ArgVerdict::Help,
        Some("--version" | "-V") => ArgVerdict::Version,
        _ => ArgVerdict::Refuse,
    }
}

/// Act on [`classify_argument`], exiting for every verdict but `Run`.
/// Takes the first argument that is *not* `--display ADDR`, because that one
/// is the strap's and every application accepts it.
fn refuse_arguments(first: Option<&std::ffi::OsStr>) {
    match classify_argument(first) {
        ArgVerdict::Run => {}
        ArgVerdict::Help => {
            println!("usage: match3");
            println!();
            println!("A match-three puzzle game. Takes no options.");
            std::process::exit(0);
        }
        ArgVerdict::Version => {
            println!("match3 {}", env!("CARGO_PKG_VERSION"));
            std::process::exit(0);
        }
        ArgVerdict::Refuse => {
            let bad = first.unwrap_or_default();
            // The raw bytes, escaped -- not `Display` and not `to_string_lossy`.
            // An option name can hold any byte, and a lossy conversion prints
            // U+FFFD where the undecodable ones were: a string nobody passed.
            // `escape_ascii` renders those as \xNN, so the message names
            // exactly what arrived and stays printable.
            eprintln!(
                "match3: unrecognized option: '{}'",
                bad.as_encoded_bytes().escape_ascii()
            );
            eprintln!("usage: match3");
            std::process::exit(2);
        }
    }
}

impl App for Match3 {
    fn theme_changed(&mut self, palette: &Palette) {
        self.palette = *palette;
    }

    fn title(&self) -> String {
        "Match 3".to_string()
    }

    fn app_id(&self) -> String {
        "match3".to_string()
    }

    fn initial_size(&self) -> (u32, u32) {
        // A size to open at, not the only one it can draw: every rectangle
        // is laid out from the size the window has.
        (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)
    }

    fn tick_interval(&self) -> Option<Duration> {
        // The board animates without input: cascades fall, the hint timer
        // counts, and Timed mode runs down. Returning `None` here would make
        // all three advance only when the player happened to move.
        Some(TICK)
    }

    fn on_event(&mut self, event: &Event) -> Response {
        if matches!(event, Event::CloseRequested) {
            return Response::Exit;
        }
        self.handle_event(event);
        // `Redraw` unconditionally, and it is a statement about this program
        // rather than laziness. `handle_event` returns `()`, so there is no
        // honest way to answer "did that change the picture" from here -- and
        // for a board mid-cascade the answer is yes even for an event it
        // ignored. Claiming `Idle` on a guess would freeze a falling column
        // until the player clicked.
        Response::Redraw
    }

    fn render(&mut self, width: f32, height: f32) -> RenderTree {
        // The size the frame is drawn at is the size the next click is read
        // against -- that is why it is stored here.
        self.resize(width, height);
        self.frame(width, height).into_tree()
    }
}

impl Probe for Match3 {
    type Target = Target;
    type Outcome = ();
    const SIZE: (f32, f32) = (WINDOW_WIDTH, WINDOW_HEIGHT);

    fn draw(&self, size: (f32, f32)) -> Frame {
        self.frame(size.0, size.1)
    }

    fn click_at(&mut self, x: f32, y: f32, button: MouseButton, size: (f32, f32)) {
        self.resize(size.0, size.1);
        self.handle_event(&Event::Mouse(MouseEvent {
            x,
            y,
            kind: MouseEventKind::Press(button),
        }));
    }

    fn key_at(&mut self, key: &KeyEvent, size: (f32, f32)) {
        self.resize(size.0, size.1);
        self.handle_event(&Event::Key(key.clone()));
    }
}

fn main() -> ExitCode {
    // Parsed rather than indexed, so `--display` reaches the connection
    // instead of being refused as an argument this game does not take. That
    // option belongs to every application equally and is not part of what
    // `refuse_arguments` is about.
    let args = match app::ArgsOs::from_env() {
        Ok(args) => args,
        Err(e) => {
            eprintln!("match3: {e}");
            return ExitCode::from(2);
        }
    };
    // The argument as it was given, bytes and all: the refusal escapes what
    // is not text rather than the parser refusing it first by another name.
    refuse_arguments(args.rest.first().map(std::ffi::OsString::as_os_str));
    let mut game = Match3::new();
    app::launch_with("match3", args.display.as_deref(), &mut game)
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

    #[test]
    fn an_option_this_program_does_not_have_is_not_silently_accepted() {
        use std::ffi::OsStr;
        assert_eq!(classify_argument(None), ArgVerdict::Run);
        assert_eq!(
            classify_argument(Some(OsStr::new("--help"))),
            ArgVerdict::Help
        );
        assert_eq!(classify_argument(Some(OsStr::new("-h"))), ArgVerdict::Help);
        assert_eq!(
            classify_argument(Some(OsStr::new("--version"))),
            ArgVerdict::Version
        );
        assert_eq!(
            classify_argument(Some(OsStr::new("-V"))),
            ArgVerdict::Version
        );
        assert_eq!(
            classify_argument(Some(OsStr::new("--zzq-not-an-option"))),
            ArgVerdict::Refuse,
            "the option lane B's sweep uses"
        );
        // An empty argument is still an argument, and `match3` takes none.
        assert_eq!(classify_argument(Some(OsStr::new(""))), ArgVerdict::Refuse);
    }

    use super::*;
    use guitk::probe;

    /// The seven gems' own colours, in `GemType` order.
    const GEM_COLORS: [Color; 7] = [RUBY, SAPPHIRE, EMERALD, TOPAZ, AMBER, AMETHYST, AQUA];

    /// The window sizes the layout tests draw at: the one it asks for,
    /// wider, taller, cramped, a sliver, and a big one.
    const WINDOWS: [(f32, f32); 7] = [
        (WINDOW_WIDTH, WINDOW_HEIGHT),
        (900.0, 600.0),
        (480.0, 1000.0),
        (320.0, 360.0),
        (200.0, 120.0),
        (1600.0, 1200.0),
        (60.0, 60.0),
    ];

    /// **The window is drawn in the user's colours**, light or dark -- a
    /// board with a gem picked up and a hint showing, and a game over --
    /// with only the gems' own colours and their ink not the palette's (the
    /// operator's C-Q16).
    #[test]
    fn the_window_is_drawn_in_the_users_colours() {
        let mut derived: Vec<Color> = GEM_COLORS.to_vec();
        derived.push(GEM_INK);
        for (light, cards) in LOOKS {
            let p = palette(light, cards);
            // The header's words, written for it, and the controls' faces
            // and labels: the toolkit's blends of the palette.
            let chrome = gamechrome::Chrome::of(&p);
            let mut derived = derived.clone();
            derived.extend(chrome.on(chrome.raised).inks());
            for kind in [Kind::Plain, Kind::Primary] {
                derived.extend(gamechrome::button_colours(&p, kind, chrome.page));
            }
            for (what, f) in every_look(&p) {
                appearance::palette_check::assert_drawn_from(
                    &p,
                    f.commands(),
                    &derived,
                    &format!("match3, {what}, light: {light}, cards: {cards}"),
                );
            }
        }
    }

    /// The palette for a light or a dark theme, in the bordered look (the
    /// default) or the card look.
    fn palette(light: bool, cards: bool) -> Palette {
        let mut p = Palette::for_mode(light);
        p.set_surface_style(if cards {
            guitk::palette::SurfaceStyle::Cards
        } else {
            guitk::palette::SurfaceStyle::Borders
        });
        p
    }

    /// `(light, cards)`: both themes, in both surface looks.
    const LOOKS: [(bool, bool); 4] = [(false, false), (true, false), (false, true), (true, true)];

    /// Every state the window shows, drawn in `p`'s colours: a gem picked up
    /// and a hint shown in each mode, the game over, and the same in a
    /// cramped window and a wide one.
    fn every_look(p: &Palette) -> Vec<(&'static str, Frame)> {
        let mut game = Match3::with_seed(7);
        game.theme_changed(p);
        game.selected = Some(Pos::new(2, 2));
        game.hint = Some((Pos::new(3, 3), Pos::new(3, 4)));
        game.hint_visible = true;
        let (w, h) = (WINDOW_WIDTH, WINDOW_HEIGHT);
        let playing = game.frame(w, h);
        let cramped = game.frame(320.0, 360.0);
        let wide = game.frame(900.0, 600.0);
        game.mode = GameMode::Timed;
        game.time_remaining_ms = 5_000;
        let timed = game.frame(w, h);
        game.mode = GameMode::Moves;
        game.moves_remaining = 3;
        let moves = game.frame(w, h);
        game.state = GameState::GameOver;
        let over = game.frame(w, h);
        let over_cramped = game.frame(320.0, 360.0);
        vec![
            ("playing", playing),
            ("timed", timed),
            ("moves", moves),
            ("over", over),
            ("cramped", cramped),
            ("wide", wide),
            ("over, cramped", over_cramped),
        ]
    }

    /// **Every text reads on what is drawn under it**, in either theme and
    /// either surface look (`gamechrome::legibility`: each run held to
    /// WCAG's floor for its size against the fills under it).
    #[test]
    fn every_text_reads_on_what_is_under_it_in_either_theme() {
        let mut bad = Vec::new();
        for (look, p) in gamechrome::legibility::looks() {
            // A switched-off button's label is exempt, as WCAG exempts an
            // inactive control: Hint, once the game is over.
            let off = guitk::button::paint(
                &p,
                Kind::Plain,
                State {
                    disabled: true,
                    ..State::default()
                },
                Colours::of(&p).chrome.page,
            );
            let exempt = |r: &gamechrome::legibility::Read| {
                r.ink == off.ink && (r.ground == off.lower || r.ground == off.upper)
            };
            for (what, f) in every_look(&p) {
                for r in gamechrome::legibility::illegible(f.commands(), p.base, exempt) {
                    bad.push(format!(
                        "{what}, {look}: {:?} {:.2}:1 on {:?}",
                        r.text,
                        r.ratio(),
                        r.ground
                    ));
                }
            }
        }
        assert!(bad.is_empty(), "match3: {bad:#?}");
    }

    /// **A new game keeps the user's colours**, as it keeps the scores.
    #[test]
    fn a_new_game_keeps_the_users_colours() {
        let light = Palette::for_mode(true);
        let mut game = Match3::with_seed(7);
        game.theme_changed(&light);
        game.new_game();
        assert_eq!(game.palette, light);
    }

    /// **Every gem's marks read on it**: the symbol and a special's lines and
    /// dots, near-black on every gem. The lines were the text colour --
    /// near-white -- and the colour bomb's dots the topaz's own yellow.
    #[test]
    fn every_gems_marks_read_on_it() {
        for gem in GEM_COLORS {
            let ratio = guitk::theme::contrast_ratio(GEM_INK, gem);
            assert!(ratio >= 4.5, "the marks are {ratio:.2}:1 on {gem:?}");
        }
    }

    // ── Helper functions ────────────────────────────────────────────

    fn make_gem(t: u8) -> Gem {
        Gem::new(GemType::from_index(t))
    }

    fn make_special_gem(t: u8, special: SpecialKind) -> Gem {
        Gem::with_special(GemType::from_index(t), special)
    }

    /// Create a game with a fully empty board for test setups.
    fn empty_game() -> Match3 {
        let mut game = Match3::with_seed(1);
        game.clear_board();
        game
    }

    /// Fill the board with alternating gems so no matches exist.
    fn fill_no_matches(game: &mut Match3) {
        for row in 0..GRID_SIZE {
            for col in 0..GRID_SIZE {
                // Use a pattern that never creates 3-in-a-row.
                let t = ((row * 2 + col) % GEM_TYPE_COUNT as usize) as u8;
                game.board[row][col] = Some(make_gem(t));
            }
        }
    }

    // ── Seeding and the shuffle ─────────────────────────────────────

    // The generator's own contract -- determinism under a seed, divergence
    // under two, staying inside its bound -- used to be tested here against the
    // local `Rng`. It is now tested once, against the shared implementation, in
    // `randrange`. Sixteen crates each testing their own copy is sixteen
    // chances to test a copy that has quietly drifted from the one being
    // shipped. What replaces those tests is about the board.

    /// A shuffle must move gems, and must not lose or invent any.
    ///
    /// `shuffle_board` reshuffles in place and then clears any matches the new
    /// arrangement created, so the gem *multiset* is allowed to change -- what
    /// must not change is that every cell still holds a gem and the board is
    /// still playable.
    #[test]
    fn a_shuffle_leaves_a_full_playable_board() {
        let mut app = Match3::with_seed(0x5EED_1234);
        for round in 0..8 {
            app.shuffle_board();
            for row in 0..GRID_SIZE {
                for col in 0..GRID_SIZE {
                    assert!(
                        app.board[row][col].is_some(),
                        "round {round} left a hole at ({row}, {col})"
                    );
                }
            }
        }
    }

    /// Shuffling must depend on the generator, not on the board's position.
    #[test]
    fn a_shuffle_follows_the_generator_it_was_given() {
        let boards: Vec<_> = (0..8u64)
            .map(|seed| {
                let mut app = Match3::with_seed(seed);
                app.shuffle_board();
                app.board
            })
            .collect();
        let mut distinct = 0;
        for (i, b) in boards.iter().enumerate() {
            if !boards[..i].iter().any(|o| o == b) {
                distinct += 1;
            }
        }
        assert!(distinct > 1, "eight seeds produced one shuffled board");
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
        let fresh = Match3::new().board;
        assert!(
            fresh == Match3::with_seed(FALLBACK_SEED).board,
            "a fresh game did not use the crate's fallback seed"
        );
        assert!(
            fresh != Match3::with_seed(42).board,
            "a fresh game is still seeded by the old hardcoded literal"
        );
    }

    // ── GemType tests ───────────────────────────────────────────────

    #[test]
    fn test_gem_type_roundtrip() {
        for i in 0..GEM_TYPE_COUNT {
            let gt = GemType::from_index(i);
            assert_eq!(gt.index(), i as usize);
        }
    }

    #[test]
    fn test_gem_type_color_distinct() {
        let colors: Vec<Color> = (0..GEM_TYPE_COUNT)
            .map(|i| GemType::from_index(i).color())
            .collect();
        for i in 0..colors.len() {
            for j in (i + 1)..colors.len() {
                assert_ne!(colors[i], colors[j]);
            }
        }
    }

    #[test]
    fn test_gem_type_symbol_distinct() {
        let syms: Vec<&str> = (0..GEM_TYPE_COUNT)
            .map(|i| GemType::from_index(i).symbol())
            .collect();
        for i in 0..syms.len() {
            for j in (i + 1)..syms.len() {
                assert_ne!(syms[i], syms[j]);
            }
        }
    }

    #[test]
    fn test_gem_type_out_of_range() {
        // Values >= 6 should map to Aqua.
        assert_eq!(GemType::from_index(7), GemType::Aqua);
        assert_eq!(GemType::from_index(100), GemType::Aqua);
    }

    // ── Pos tests ───────────────────────────────────────────────────

    #[test]
    fn test_pos_in_bounds() {
        assert!(Pos::new(0, 0).in_bounds());
        assert!(Pos::new(7, 7).in_bounds());
        assert!(!Pos::new(8, 0).in_bounds());
        assert!(!Pos::new(0, 8).in_bounds());
    }

    #[test]
    fn test_pos_adjacent_horizontal() {
        assert!(Pos::new(3, 4).is_adjacent(Pos::new(3, 5)));
        assert!(Pos::new(3, 4).is_adjacent(Pos::new(3, 3)));
    }

    #[test]
    fn test_pos_adjacent_vertical() {
        assert!(Pos::new(3, 4).is_adjacent(Pos::new(4, 4)));
        assert!(Pos::new(3, 4).is_adjacent(Pos::new(2, 4)));
    }

    #[test]
    fn test_pos_not_adjacent_diagonal() {
        assert!(!Pos::new(3, 4).is_adjacent(Pos::new(4, 5)));
        assert!(!Pos::new(3, 4).is_adjacent(Pos::new(2, 3)));
    }

    #[test]
    fn test_pos_not_adjacent_same() {
        assert!(!Pos::new(3, 4).is_adjacent(Pos::new(3, 4)));
    }

    #[test]
    fn test_pos_not_adjacent_far() {
        assert!(!Pos::new(0, 0).is_adjacent(Pos::new(2, 0)));
        assert!(!Pos::new(0, 0).is_adjacent(Pos::new(0, 2)));
    }

    // ── Board initialization tests ──────────────────────────────────

    #[test]
    fn test_new_game_fills_board() {
        let game = Match3::new();
        assert_eq!(game.gem_count(), GRID_SIZE * GRID_SIZE);
    }

    #[test]
    fn test_new_game_no_initial_matches() {
        let game = Match3::new();
        let matches = game.find_matches();
        assert!(matches.is_empty(), "New board should have no matches");
    }

    #[test]
    fn test_deterministic_seed() {
        let g1 = Match3::with_seed(123);
        let g2 = Match3::with_seed(123);
        for row in 0..GRID_SIZE {
            for col in 0..GRID_SIZE {
                assert_eq!(g1.board[row][col], g2.board[row][col]);
            }
        }
    }

    #[test]
    fn test_different_seeds_different_boards() {
        let g1 = Match3::with_seed(1);
        let g2 = Match3::with_seed(2);
        let mut same = 0;
        for row in 0..GRID_SIZE {
            for col in 0..GRID_SIZE {
                if g1.board[row][col] == g2.board[row][col] {
                    same += 1;
                }
            }
        }
        // Should not be identical (statistically nearly impossible).
        assert!(same < GRID_SIZE * GRID_SIZE);
    }

    // ── Match detection tests ───────────────────────────────────────

    #[test]
    fn test_find_horizontal_match_3() {
        let mut game = empty_game();
        fill_no_matches(&mut game);
        // Place 3 rubies in a row.
        game.board[0][0] = Some(make_gem(0));
        game.board[0][1] = Some(make_gem(0));
        game.board[0][2] = Some(make_gem(0));
        let matches = game.find_matches();
        assert!(!matches.is_empty());
        let m = &matches[0];
        assert!(m.horizontal);
        assert_eq!(m.length, 3);
    }

    #[test]
    fn test_find_vertical_match_3() {
        let mut game = empty_game();
        fill_no_matches(&mut game);
        game.board[0][0] = Some(make_gem(0));
        game.board[1][0] = Some(make_gem(0));
        game.board[2][0] = Some(make_gem(0));
        let matches = game.find_matches();
        assert!(!matches.is_empty());
        let has_vertical = matches.iter().any(|m| !m.horizontal && m.length == 3);
        assert!(has_vertical);
    }

    #[test]
    fn test_find_match_4() {
        let mut game = empty_game();
        fill_no_matches(&mut game);
        game.board[3][2] = Some(make_gem(1));
        game.board[3][3] = Some(make_gem(1));
        game.board[3][4] = Some(make_gem(1));
        game.board[3][5] = Some(make_gem(1));
        let matches = game.find_matches();
        let has_4 = matches.iter().any(|m| m.length == 4);
        assert!(has_4);
    }

    #[test]
    fn test_find_match_5() {
        let mut game = empty_game();
        fill_no_matches(&mut game);
        game.board[5][1] = Some(make_gem(2));
        game.board[5][2] = Some(make_gem(2));
        game.board[5][3] = Some(make_gem(2));
        game.board[5][4] = Some(make_gem(2));
        game.board[5][5] = Some(make_gem(2));
        let matches = game.find_matches();
        let has_5 = matches.iter().any(|m| m.length >= 5);
        assert!(has_5);
    }

    #[test]
    fn test_no_match_with_2() {
        let mut game = empty_game();
        fill_no_matches(&mut game);
        game.board[0][0] = Some(make_gem(0));
        game.board[0][1] = Some(make_gem(0));
        // Only 2 in a row: no match.
        let matches = game.find_matches();
        let has_match_at_row0 = matches
            .iter()
            .any(|m| m.positions.iter().any(|p| p.row == 0 && p.col <= 1));
        assert!(!has_match_at_row0);
    }

    #[test]
    fn test_empty_board_no_matches() {
        let game = empty_game();
        assert!(game.find_matches().is_empty());
    }

    // ── Match scoring tests ─────────────────────────────────────────

    #[test]
    fn test_score_3_match() {
        let m = MatchInfo {
            positions: vec![Pos::new(0, 0), Pos::new(0, 1), Pos::new(0, 2)],
            horizontal: true,
            length: 3,
        };
        assert_eq!(m.score(), SCORE_3);
    }

    #[test]
    fn test_score_4_match() {
        let m = MatchInfo {
            positions: vec![
                Pos::new(0, 0),
                Pos::new(0, 1),
                Pos::new(0, 2),
                Pos::new(0, 3),
            ],
            horizontal: true,
            length: 4,
        };
        assert_eq!(m.score(), SCORE_4);
    }

    #[test]
    fn test_score_5_match() {
        let m = MatchInfo {
            positions: vec![
                Pos::new(0, 0),
                Pos::new(0, 1),
                Pos::new(0, 2),
                Pos::new(0, 3),
                Pos::new(0, 4),
            ],
            horizontal: true,
            length: 5,
        };
        assert_eq!(m.score(), SCORE_5);
    }

    // ── Cascade multiplier tests ────────────────────────────────────

    #[test]
    fn test_cascade_multiplier_base() {
        let game = Match3::new();
        assert_eq!(game.cascade_multiplier(), FP_BASE);
    }

    #[test]
    fn test_cascade_multiplier_level_1() {
        let mut game = Match3::new();
        game.chain_level = 1;
        assert_eq!(game.cascade_multiplier(), CASCADE_MULTIPLIER_FP);
    }

    #[test]
    fn test_cascade_multiplier_level_2() {
        let mut game = Match3::new();
        game.chain_level = 2;
        // 1.5 * 1.5 = 2.25 -> 225 in FP.
        assert_eq!(game.cascade_multiplier(), 225);
    }

    // ── Gravity tests ───────────────────────────────────────────────

    #[test]
    fn test_gravity_single_gap() {
        let mut game = empty_game();
        // Place a gem at row 0, gap at row 1, gem at row 2.
        game.board[0][0] = Some(make_gem(0));
        game.board[1][0] = None;
        game.board[2][0] = Some(make_gem(1));

        let moved = game.apply_gravity();
        assert!(moved);
        // Gravity compacts gems to the bottom of the GRID_SIZE-tall column, so the
        // two gems settle in the last two rows and everything above is empty.
        for row in 0..GRID_SIZE - 2 {
            assert!(game.board[row][0].is_none());
        }
        assert!(game.board[GRID_SIZE - 2][0].is_some());
        assert!(game.board[GRID_SIZE - 1][0].is_some());
    }

    #[test]
    fn test_gravity_multiple_gaps() {
        let mut game = empty_game();
        game.board[0][3] = Some(make_gem(0));
        // rows 1-6 empty
        game.board[7][3] = Some(make_gem(1));

        game.apply_gravity();
        // gem from row 0 should fall to row 6.
        assert!(game.board[6][3].is_some());
        assert!(game.board[7][3].is_some());
        // rows 0-5 should be empty.
        for r in 0..6 {
            assert!(game.board[r][3].is_none());
        }
    }

    #[test]
    fn test_gravity_no_gaps() {
        let mut game = empty_game();
        for r in 0..GRID_SIZE {
            game.board[r][0] = Some(make_gem((r % 7) as u8));
        }
        let moved = game.apply_gravity();
        assert!(!moved);
    }

    #[test]
    fn test_gravity_preserves_order() {
        let mut game = empty_game();
        game.board[0][0] = Some(make_gem(0));
        game.board[2][0] = Some(make_gem(1));
        game.board[4][0] = Some(make_gem(2));

        game.apply_gravity();
        // Should be stacked at bottom in original order.
        assert_eq!(game.board[5][0].unwrap().gem_type, GemType::Ruby);
        assert_eq!(game.board[6][0].unwrap().gem_type, GemType::Sapphire);
        assert_eq!(game.board[7][0].unwrap().gem_type, GemType::Emerald);
    }

    // ── Fill empty spaces tests ─────────────────────────────────────

    #[test]
    fn test_fill_empty_spaces() {
        let mut game = empty_game();
        game.fill_empty_spaces();
        assert_eq!(game.gem_count(), GRID_SIZE * GRID_SIZE);
    }

    #[test]
    fn test_fill_preserves_existing() {
        let mut game = empty_game();
        let gem = make_gem(3);
        game.board[4][4] = Some(gem);
        game.fill_empty_spaces();
        assert_eq!(game.board[4][4], Some(gem));
        assert_eq!(game.gem_count(), GRID_SIZE * GRID_SIZE);
    }

    // ── Swap tests ──────────────────────────────────────────────────

    #[test]
    fn test_swap_creates_match_detection() {
        let mut game = empty_game();
        fill_no_matches(&mut game);
        // Set up so swapping (0,2) and (0,3) creates a 3-match.
        game.board[0][0] = Some(make_gem(0));
        game.board[0][1] = Some(make_gem(0));
        game.board[0][2] = Some(make_gem(1));
        game.board[0][3] = Some(make_gem(0));

        let a = Pos::new(0, 2);
        let b = Pos::new(0, 3);
        assert!(game.swap_creates_match(a, b));
    }

    #[test]
    fn test_swap_no_match() {
        let mut game = empty_game();
        fill_no_matches(&mut game);

        // Two different gems, swapping won't create a match.
        let a = Pos::new(3, 3);
        let b = Pos::new(3, 4);
        // May or may not create a match depending on fill_no_matches pattern.
        // Just verify it doesn't panic.
        let _ = game.swap_creates_match(a, b);
    }

    #[test]
    fn test_try_swap_non_adjacent() {
        let mut game = Match3::new();
        let result = game.try_swap(Pos::new(0, 0), Pos::new(0, 2));
        assert!(!result);
    }

    #[test]
    fn test_try_swap_out_of_bounds() {
        let mut game = Match3::new();
        let result = game.try_swap(Pos::new(0, 0), Pos::new(8, 0));
        assert!(!result);
    }

    #[test]
    fn test_try_swap_same_position() {
        let mut game = Match3::new();
        let result = game.try_swap(Pos::new(3, 3), Pos::new(3, 3));
        assert!(!result);
    }

    #[test]
    fn test_try_swap_valid_match() {
        let mut game = empty_game();
        fill_no_matches(&mut game);
        // Set up guaranteed match.
        game.board[4][0] = Some(make_gem(5));
        game.board[4][1] = Some(make_gem(5));
        game.board[4][2] = Some(make_gem(3));
        game.board[4][3] = Some(make_gem(5));

        let old_score = game.score;
        let result = game.try_swap(Pos::new(4, 2), Pos::new(4, 3));
        assert!(result);
        assert!(game.score > old_score);
    }

    // ── Game mode tests ─────────────────────────────────────────────

    #[test]
    fn test_classic_mode_label() {
        assert_eq!(GameMode::Classic.label(), "Classic");
    }

    #[test]
    fn test_timed_mode_label() {
        assert_eq!(GameMode::Timed.label(), "Timed");
    }

    #[test]
    fn test_moves_mode_label() {
        assert_eq!(GameMode::Moves.label(), "Moves");
    }

    #[test]
    fn test_initial_mode_classic() {
        let game = Match3::new();
        assert_eq!(game.mode, GameMode::Classic);
    }

    #[test]
    fn test_switch_mode() {
        let mut game = Match3::new();
        game.switch_mode(GameMode::Timed);
        assert_eq!(game.mode, GameMode::Timed);
        assert_eq!(game.time_remaining_ms, TIMED_MODE_SECONDS * 1000);
    }

    #[test]
    fn test_switch_mode_moves() {
        let mut game = Match3::new();
        game.switch_mode(GameMode::Moves);
        assert_eq!(game.mode, GameMode::Moves);
        assert_eq!(game.moves_remaining, MOVES_MODE_COUNT);
    }

    // ── Game state tests ────────────────────────────────────────────

    #[test]
    fn test_initial_state_idle() {
        let game = Match3::new();
        assert_eq!(game.state, GameState::Idle);
    }

    #[test]
    fn test_initial_score_zero() {
        let game = Match3::new();
        assert_eq!(game.score, 0);
    }

    #[test]
    fn test_end_game_updates_state() {
        let mut game = Match3::new();
        game.score = 500;
        game.end_game();
        assert_eq!(game.state, GameState::GameOver);
    }

    #[test]
    fn test_end_game_updates_high_score() {
        let mut game = Match3::new();
        game.score = 500;
        game.end_game();
        assert_eq!(game.high_scores.get(GameMode::Classic), 500);
    }

    #[test]
    fn test_high_score_preserved_across_games() {
        let mut game = Match3::new();
        game.score = 1000;
        game.end_game();
        game.new_game();
        assert_eq!(game.high_scores.get(GameMode::Classic), 1000);
        assert_eq!(game.score, 0);
    }

    #[test]
    fn test_high_score_per_mode() {
        let mut game = Match3::new();
        game.mode = GameMode::Classic;
        game.score = 100;
        game.end_game();

        game.new_game();
        game.mode = GameMode::Timed;
        game.score = 200;
        game.end_game();

        assert_eq!(game.high_scores.get(GameMode::Classic), 100);
        assert_eq!(game.high_scores.get(GameMode::Timed), 200);
        assert_eq!(game.high_scores.get(GameMode::Moves), 0);
    }

    // ── New game tests ──────────────────────────────────────────────

    #[test]
    fn test_new_game_resets_score() {
        let mut game = Match3::new();
        game.score = 500;
        game.new_game();
        assert_eq!(game.score, 0);
    }

    #[test]
    fn test_new_game_resets_state() {
        let mut game = Match3::new();
        game.state = GameState::GameOver;
        game.new_game();
        assert_eq!(game.state, GameState::Idle);
    }

    #[test]
    fn test_new_game_refills_board() {
        let mut game = Match3::new();
        game.clear_board();
        game.new_game();
        assert_eq!(game.gem_count(), GRID_SIZE * GRID_SIZE);
    }

    #[test]
    fn test_new_game_preserves_mode() {
        let mut game = Match3::new();
        game.switch_mode(GameMode::Moves);
        game.new_game();
        assert_eq!(game.mode, GameMode::Moves);
    }

    // ── Selection and cursor tests ──────────────────────────────────

    #[test]
    fn test_select_gem() {
        let mut game = Match3::new();
        game.select_or_swap(Pos::new(3, 3));
        assert_eq!(game.state, GameState::Selected);
        assert_eq!(game.selected, Some(Pos::new(3, 3)));
    }

    #[test]
    fn test_deselect_same_gem() {
        let mut game = Match3::new();
        game.select_or_swap(Pos::new(3, 3));
        game.select_or_swap(Pos::new(3, 3));
        assert_eq!(game.state, GameState::Idle);
        assert_eq!(game.selected, None);
    }

    #[test]
    fn test_select_different_non_adjacent() {
        let mut game = Match3::new();
        game.select_or_swap(Pos::new(0, 0));
        game.select_or_swap(Pos::new(5, 5));
        // Should reselect the new gem.
        assert_eq!(game.state, GameState::Selected);
        assert_eq!(game.selected, Some(Pos::new(5, 5)));
    }

    #[test]
    fn test_cursor_move_right() {
        let mut game = Match3::new();
        game.cursor = Pos::new(0, 0);
        game.handle_key(Key::Right);
        assert_eq!(game.cursor.col, 1);
    }

    #[test]
    fn test_cursor_move_down() {
        let mut game = Match3::new();
        game.cursor = Pos::new(0, 0);
        game.handle_key(Key::Down);
        assert_eq!(game.cursor.row, 1);
    }

    #[test]
    fn test_cursor_bounded_left() {
        let mut game = Match3::new();
        game.cursor = Pos::new(0, 0);
        game.handle_key(Key::Left);
        assert_eq!(game.cursor.col, 0);
    }

    #[test]
    fn test_cursor_bounded_up() {
        let mut game = Match3::new();
        game.cursor = Pos::new(0, 0);
        game.handle_key(Key::Up);
        assert_eq!(game.cursor.row, 0);
    }

    #[test]
    fn test_cursor_bounded_right() {
        let mut game = Match3::new();
        game.cursor = Pos::new(0, GRID_SIZE - 1);
        game.handle_key(Key::Right);
        assert_eq!(game.cursor.col, GRID_SIZE - 1);
    }

    #[test]
    fn test_cursor_bounded_down() {
        let mut game = Match3::new();
        game.cursor = Pos::new(GRID_SIZE - 1, 0);
        game.handle_key(Key::Down);
        assert_eq!(game.cursor.row, GRID_SIZE - 1);
    }

    // ── Keyboard event tests ────────────────────────────────────────

    #[test]
    fn test_key_n_new_game() {
        let mut game = Match3::new();
        game.score = 999;
        game.handle_event(&Event::Key(KeyEvent {
            key: Key::N,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: "n".to_string(),
        }));
        assert_eq!(game.score, 0);
    }

    #[test]
    fn test_key_1_classic_mode() {
        let mut game = Match3::new();
        game.mode = GameMode::Timed;
        game.handle_event(&Event::Key(KeyEvent {
            key: Key::Num1,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: "1".to_string(),
        }));
        assert_eq!(game.mode, GameMode::Classic);
    }

    #[test]
    fn test_key_2_timed_mode() {
        let mut game = Match3::new();
        game.handle_event(&Event::Key(KeyEvent {
            key: Key::Num2,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: "2".to_string(),
        }));
        assert_eq!(game.mode, GameMode::Timed);
    }

    #[test]
    fn test_key_3_moves_mode() {
        let mut game = Match3::new();
        game.handle_event(&Event::Key(KeyEvent {
            key: Key::Num3,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: "3".to_string(),
        }));
        assert_eq!(game.mode, GameMode::Moves);
    }

    #[test]
    fn test_key_escape_deselects() {
        let mut game = Match3::new();
        game.select_or_swap(Pos::new(2, 2));
        assert_eq!(game.state, GameState::Selected);
        game.handle_key(Key::Escape);
        assert_eq!(game.state, GameState::Idle);
        assert_eq!(game.selected, None);
    }

    #[test]
    fn test_key_released_ignored() {
        let mut game = Match3::new();
        game.handle_event(&Event::Key(KeyEvent {
            key: Key::N,
            pressed: false,
            modifiers: Modifiers::NONE,
            text: String::new(),
        }));
        // Should not create a new game (key release ignored).
        // Can't easily verify no-op, but at least no panic.
    }

    // ── Mouse event tests ───────────────────────────────────────────

    #[test]
    fn test_mouse_click_selects() {
        let mut game = Match3::new();
        probe::click(&mut game, Target::Cell(Pos::new(2, 3)));
        assert_eq!(game.state, GameState::Selected);
        assert_eq!(game.selected, Some(Pos::new(2, 3)));
    }

    #[test]
    fn test_mouse_click_outside_grid() {
        let mut game = Match3::new();
        game.handle_event(&Event::Mouse(MouseEvent {
            x: 0.0,
            y: 0.0,
            kind: MouseEventKind::Press(MouseButton::Left),
        }));
        // Clicking outside the grid should not select anything.
        assert_eq!(game.state, GameState::Idle);
    }

    #[test]
    fn test_mouse_right_click_ignored() {
        let mut game = Match3::new();
        probe::click_with(&mut game, Target::Cell(Pos::new(2, 3)), MouseButton::Right);
        assert_eq!(game.state, GameState::Idle);
    }

    /// **The controls do what they say**: a mode's button starts a game in
    /// it, Hint shows a move, New game deals a new board in the same mode.
    #[test]
    fn the_controls_do_what_they_say() {
        let mut game = Match3::with_seed(5);
        for mode in [GameMode::Timed, GameMode::Moves, GameMode::Classic] {
            game.score = 40;
            probe::click(&mut game, Target::Mode(mode));
            assert_eq!(game.mode, mode);
            assert_eq!(game.score, 0, "{mode:?} did not start a new game");
        }
        assert!(!game.hint_visible);
        probe::click(&mut game, Target::Hint);
        assert!(
            game.hint_visible && game.hint.is_some(),
            "Hint showed nothing"
        );
        let before = game.board;
        game.mode = GameMode::Moves;
        probe::click(&mut game, Target::NewGame);
        assert_ne!(game.board, before, "New game kept the board");
        assert_eq!(game.mode, GameMode::Moves, "New game changed the mode");
    }

    /// **The mode being played is the primary button**, and the others
    /// plain -- the one way the row says which mode this is.
    #[test]
    fn the_mode_being_played_is_the_primary_button() {
        let mut game = Match3::with_seed(5);
        game.mode = GameMode::Timed;
        let p = game.palette;
        let primary = guitk::button::paint(&p, Kind::Primary, State::default(), p.base);
        let f = game.frame(WINDOW_WIDTH, WINDOW_HEIGHT);
        let face = |target: Target| {
            let r = f.rect_of(|t| *t == target).expect("the button is drawn");
            f.commands().iter().find_map(|cmd| match cmd {
                RenderCommand::FillRect { x, y, color, .. }
                    if (x - r.x).abs() < 0.5 && (y - r.y).abs() < 0.5 =>
                {
                    Some(*color)
                }
                _ => None,
            })
        };
        let primary_face = [primary.lower, primary.upper];
        let timed = face(Target::Mode(GameMode::Timed)).expect("Timed has a face");
        assert!(
            primary_face.contains(&timed),
            "the mode played is not the primary button"
        );
        let classic = face(Target::Mode(GameMode::Classic)).expect("Classic has a face");
        assert!(
            !primary_face.contains(&classic),
            "a mode not played looks chosen"
        );
    }

    /// **A game over takes a click on its panel as the next game**, and
    /// Hint is switched off: there is no move to show.
    #[test]
    fn a_click_on_the_game_over_panel_starts_the_next_game() {
        let mut game = Match3::with_seed(5);
        game.end_game();
        assert!(
            !probe::is_visible(&game, Target::Hint),
            "Hint is live on a finished game"
        );
        assert!(
            !probe::is_visible(&game, Target::Cell(Pos::new(0, 0))),
            "the board takes clicks under the panel"
        );
        probe::click(&mut game, Target::GameOver);
        assert_ne!(
            game.state,
            GameState::GameOver,
            "the panel's click did nothing"
        );
    }

    // ── Hint system tests ───────────────────────────────────────────

    #[test]
    fn test_hint_not_visible_initially() {
        let game = Match3::new();
        assert!(!game.hint_visible);
    }

    #[test]
    fn test_hint_appears_after_delay() {
        let mut game = Match3::new();
        game.update_hint(HINT_DELAY_MS);
        assert!(game.hint_visible);
    }

    #[test]
    fn test_hint_not_visible_before_delay() {
        let mut game = Match3::new();
        game.update_hint(HINT_DELAY_MS - 1);
        assert!(!game.hint_visible);
    }

    #[test]
    fn test_hint_reset_on_action() {
        let mut game = Match3::new();
        game.update_hint(HINT_DELAY_MS);
        assert!(game.hint_visible);
        game.reset_idle();
        assert!(!game.hint_visible);
        assert_eq!(game.idle_ms, 0);
    }

    #[test]
    fn test_key_h_shows_hint() {
        let mut game = Match3::new();
        game.handle_key(Key::H);
        assert!(game.hint_visible);
    }

    // ── Timed mode tests ────────────────────────────────────────────

    #[test]
    fn test_timed_mode_countdown() {
        let mut game = Match3::new();
        game.switch_mode(GameMode::Timed);
        let initial = game.time_remaining_ms;
        game.handle_tick(1000);
        assert_eq!(game.time_remaining_ms, initial - 1000);
    }

    #[test]
    fn test_timed_mode_game_over() {
        let mut game = Match3::new();
        game.switch_mode(GameMode::Timed);
        game.handle_tick(TIMED_MODE_SECONDS * 1000 + 1);
        assert_eq!(game.state, GameState::GameOver);
        assert_eq!(game.time_remaining_ms, 0);
    }

    #[test]
    fn test_timed_mode_not_game_over_early() {
        let mut game = Match3::new();
        game.switch_mode(GameMode::Timed);
        game.handle_tick(1000);
        assert_eq!(game.state, GameState::Idle);
    }

    // ── Moves mode tests ────────────────────────────────────────────

    #[test]
    fn test_moves_mode_initial_count() {
        let mut game = Match3::new();
        game.switch_mode(GameMode::Moves);
        assert_eq!(game.moves_remaining, MOVES_MODE_COUNT);
    }

    #[test]
    fn test_moves_consume_move() {
        let mut game = Match3::new();
        game.switch_mode(GameMode::Moves);
        game.consume_move();
        assert_eq!(game.moves_remaining, MOVES_MODE_COUNT - 1);
    }

    #[test]
    fn test_moves_game_over_at_zero() {
        let mut game = Match3::new();
        game.switch_mode(GameMode::Moves);
        game.moves_remaining = 1;
        game.consume_move();
        assert_eq!(game.state, GameState::GameOver);
    }

    #[test]
    fn test_classic_mode_no_move_consume() {
        let mut game = Match3::new();
        game.mode = GameMode::Classic;
        game.moves_remaining = 30;
        game.consume_move();
        // Classic mode does not decrement moves.
        assert_eq!(game.moves_remaining, 30);
    }

    // ── Special gem tests ───────────────────────────────────────────

    #[test]
    fn test_special_gem_4_match_horizontal() {
        let m = MatchInfo {
            positions: vec![
                Pos::new(0, 0),
                Pos::new(0, 1),
                Pos::new(0, 2),
                Pos::new(0, 3),
            ],
            horizontal: true,
            length: 4,
        };
        let game = Match3::new();
        let result = game.determine_special_gem(&m);
        assert!(result.is_some());
        let (_, special) = result.unwrap();
        assert_eq!(special, SpecialKind::LineClearH);
    }

    #[test]
    fn test_special_gem_4_match_vertical() {
        let m = MatchInfo {
            positions: vec![
                Pos::new(0, 0),
                Pos::new(1, 0),
                Pos::new(2, 0),
                Pos::new(3, 0),
            ],
            horizontal: false,
            length: 4,
        };
        let game = Match3::new();
        let result = game.determine_special_gem(&m);
        assert!(result.is_some());
        let (_, special) = result.unwrap();
        assert_eq!(special, SpecialKind::LineClearV);
    }

    #[test]
    fn test_special_gem_5_match_color_bomb() {
        let m = MatchInfo {
            positions: vec![
                Pos::new(0, 0),
                Pos::new(0, 1),
                Pos::new(0, 2),
                Pos::new(0, 3),
                Pos::new(0, 4),
            ],
            horizontal: true,
            length: 5,
        };
        let game = Match3::new();
        let result = game.determine_special_gem(&m);
        assert!(result.is_some());
        let (_, special) = result.unwrap();
        assert_eq!(special, SpecialKind::ColorBomb);
    }

    #[test]
    fn test_no_special_gem_3_match() {
        let m = MatchInfo {
            positions: vec![Pos::new(0, 0), Pos::new(0, 1), Pos::new(0, 2)],
            horizontal: true,
            length: 3,
        };
        let game = Match3::new();
        let result = game.determine_special_gem(&m);
        assert!(result.is_none());
    }

    // ── Process matches tests ───────────────────────────────────────

    #[test]
    fn test_process_matches_removes_gems() {
        let mut game = empty_game();
        fill_no_matches(&mut game);
        game.board[0][0] = Some(make_gem(0));
        game.board[0][1] = Some(make_gem(0));
        game.board[0][2] = Some(make_gem(0));

        let had_match = game.process_matches();
        assert!(had_match);
        // The matched gems should be removed.
        assert!(
            game.board[0][0].is_none() || game.board[0][1].is_none() || game.board[0][2].is_none()
        );
    }

    #[test]
    fn test_process_matches_awards_score() {
        let mut game = empty_game();
        fill_no_matches(&mut game);
        game.board[0][0] = Some(make_gem(0));
        game.board[0][1] = Some(make_gem(0));
        game.board[0][2] = Some(make_gem(0));

        game.process_matches();
        assert!(game.score > 0);
    }

    #[test]
    fn test_process_matches_none() {
        let mut game = empty_game();
        fill_no_matches(&mut game);
        let had_match = game.process_matches();
        assert!(!had_match);
    }

    // ── Where a click lands ─────────────────────────────────────────

    /// **Every cell is taken where it is drawn**, in every window: the box a
    /// click is read against is the cell's rectangle, and a click in its
    /// middle picks that gem up.
    #[test]
    fn every_cell_is_clicked_where_it_is_drawn() {
        for (w, h) in WINDOWS {
            let l = Layout::new(w, h);
            if l.cell < 1.0 {
                continue;
            }
            for row in 0..GRID_SIZE {
                for col in 0..GRID_SIZE {
                    let pos = Pos::new(row, col);
                    let mut game = Match3::with_seed(3);
                    let r = probe::rect_of_sized(&game, Target::Cell(pos), (w, h))
                        .expect("every cell is drawn");
                    assert_eq!(r, l.cell_rect(pos), "{w}x{h}: {pos:?}");
                    let (x, y) = r.centre();
                    game.click_at(x, y, MouseButton::Left, (w, h));
                    assert_eq!(game.selected, Some(pos), "{w}x{h}: {pos:?}");
                }
            }
        }
    }

    /// **A click is read against the size the window was last drawn at**:
    /// the frame the window shows, not the size it opened at.
    #[test]
    fn a_click_is_read_against_the_size_the_window_was_drawn_at() {
        let mut game = Match3::with_seed(3);
        let (w, h) = (1600.0, 1200.0);
        let _ = App::render(&mut game, w, h);
        let (x, y) = Layout::new(w, h).cell_rect(Pos::new(7, 7)).centre();
        game.handle_event(&Event::Mouse(MouseEvent {
            x,
            y,
            kind: MouseEventKind::Press(MouseButton::Left),
        }));
        assert_eq!(game.selected, Some(Pos::new(7, 7)));
    }

    /// **A click between cells or off the board picks nothing up.**
    #[test]
    fn a_click_off_the_cells_picks_nothing_up() {
        let l = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        let a = l.cell_rect(Pos::new(0, 0));
        for (x, y) in [
            (a.right() + l.gap / 2.0, a.y + a.h / 2.0),
            (a.x + a.w / 2.0, a.bottom() + l.gap / 2.0),
            (l.board.x + 0.5, l.board.y + 0.5),
            (1.0, WINDOW_HEIGHT - 1.0),
        ] {
            let mut game = Match3::with_seed(3);
            game.click_at(x, y, MouseButton::Left, (WINDOW_WIDTH, WINDOW_HEIGHT));
            assert_eq!(
                game.state,
                GameState::Idle,
                "a click at ({x}, {y}) picked a gem up"
            );
        }
    }

    // ── Render tests ────────────────────────────────────────────────

    #[test]
    fn test_render_produces_commands() {
        let game = Match3::new();
        assert!(
            !game
                .frame(WINDOW_WIDTH, WINDOW_HEIGHT)
                .commands()
                .is_empty()
        );
    }

    #[test]
    fn test_render_game_over_overlay() {
        let mut game = Match3::new();
        game.end_game();
        let f = game.frame(WINDOW_WIDTH, WINDOW_HEIGHT);
        assert!(
            f.rect_of(|t| *t == Target::GameOver).is_some(),
            "no game-over panel"
        );
        let idle = Match3::new().frame(WINDOW_WIDTH, WINDOW_HEIGHT);
        assert!(f.commands().len() > idle.commands().len());
    }

    // ── HighScores tests ────────────────────────────────────────────

    #[test]
    fn test_high_scores_initial() {
        let hs = HighScores::new();
        assert_eq!(hs.get(GameMode::Classic), 0);
        assert_eq!(hs.get(GameMode::Timed), 0);
        assert_eq!(hs.get(GameMode::Moves), 0);
    }

    #[test]
    fn test_high_scores_update() {
        let mut hs = HighScores::new();
        hs.update(GameMode::Classic, 100);
        assert_eq!(hs.get(GameMode::Classic), 100);
    }

    #[test]
    fn test_high_scores_only_higher() {
        let mut hs = HighScores::new();
        hs.update(GameMode::Classic, 100);
        hs.update(GameMode::Classic, 50);
        assert_eq!(hs.get(GameMode::Classic), 100);
    }

    // ── Layout ──────────────────────────────────────────────────────

    /// **The board fits every window**: square, whole cells of one size,
    /// inside the window and clear of the bands, in every size -- it drew at
    /// one size, in a corner of a large window and cut off in a small one.
    #[test]
    fn the_board_fits_every_window() {
        for (w, h) in WINDOWS {
            let l = Layout::new(w, h);
            assert!(
                l.cell >= 0.0 && l.cell == l.cell.floor(),
                "{w}x{h}: cell {}",
                l.cell
            );
            if l.cell == 0.0 {
                continue;
            }
            let b = l.board;
            assert!((b.w - b.h).abs() < 0.01, "{w}x{h}: the board is not square");
            assert!(
                b.x >= 0.0 && b.y >= 0.0 && b.right() <= w + 0.01 && b.bottom() <= h + 0.01,
                "{w}x{h}: the board {b:?} leaves the window"
            );
            for band in [l.header, l.controls, l.footer] {
                if !band.is_empty() {
                    assert!(
                        band.bottom() <= b.y + 0.01 || band.y >= b.bottom() - 0.01,
                        "{w}x{h}: {band:?} overlaps the board {b:?}"
                    );
                }
            }
            let last = l.cell_rect(Pos::new(GRID_SIZE - 1, GRID_SIZE - 1));
            assert!(
                last.right() <= b.right() + 0.01 && last.bottom() <= b.bottom() + 0.01,
                "{w}x{h}: the last cell leaves the board"
            );
        }
    }

    /// **A larger window gets a larger board**, not the same one in a
    /// corner of it.
    #[test]
    fn a_larger_window_gets_a_larger_board() {
        let small = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        let large = Layout::new(1600.0, 1200.0);
        assert!(
            large.cell > small.cell * 1.5,
            "{} then {}",
            small.cell,
            large.cell
        );
        // Centred across the window.
        let b = large.board;
        assert!(
            (b.x - (1600.0 - b.right())).abs() < 1.0,
            "{b:?} is not centred"
        );
    }

    /// **In a short window the bands give way in order**: the key line
    /// first, then the controls, then the header -- the board keeps its
    /// share of the height.
    #[test]
    fn in_a_short_window_the_bands_give_way_in_order() {
        let full = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        assert!(!full.header.is_empty() && !full.controls.is_empty() && !full.footer.is_empty());
        let mut seen = Vec::new();
        for h in (60..=640).rev().step_by(4) {
            let l = Layout::new(WINDOW_WIDTH, h as f32);
            let shown = [
                !l.header.is_empty(),
                !l.controls.is_empty(),
                !l.footer.is_empty(),
            ];
            if seen.last() != Some(&shown) {
                seen.push(shown);
            }
        }
        assert_eq!(
            seen,
            vec![
                [true, true, true],
                [true, true, false],
                [true, false, false],
                [false, false, false]
            ],
            "the bands gave way in another order"
        );
    }

    /// **Nothing is drawn outside the window**, in every size and state.
    #[test]
    fn nothing_is_drawn_outside_the_window() {
        let mut game = Match3::with_seed(5);
        game.selected = Some(Pos::new(0, 0));
        game.hint = Some((Pos::new(7, 6), Pos::new(7, 7)));
        game.hint_visible = true;
        for over in [false, true] {
            if over {
                game.state = GameState::GameOver;
            }
            for (w, h) in WINDOWS {
                for cmd in game.frame(w, h).commands() {
                    if let RenderCommand::FillRect {
                        x,
                        y,
                        width,
                        height,
                        ..
                    } = cmd
                    {
                        // The glows reach a few points past their cell by
                        // design; the board's frame leaves room for them.
                        let slack = Layout::new(w, h).gap * 2.0;
                        assert!(
                            *x >= -slack
                                && *y >= -slack
                                && x + width <= w + slack
                                && y + height <= h + slack,
                            "{w}x{h}: {x},{y} {width}x{height} leaves the window"
                        );
                    }
                }
            }
        }
    }

    /// **A gem's symbol is centred on it**, at every cell size: measured,
    /// rather than placed at an offset from the middle that was right for
    /// one font at one size.
    #[test]
    fn a_gems_symbol_is_centred_on_it() {
        let game = Match3::with_seed(5);
        for (w, h) in [
            (WINDOW_WIDTH, WINDOW_HEIGHT),
            (1600.0, 1200.0),
            (320.0, 360.0),
        ] {
            let l = Layout::new(w, h);
            let f = game.frame(w, h);
            let cell = l.cell_rect(Pos::new(4, 4));
            let gem = game.get_gem(4, 4).expect("a full board");
            let symbol = gem.gem_type.symbol();
            let (x, y, size) = f
                .commands()
                .iter()
                .find_map(|cmd| match cmd {
                    RenderCommand::Text {
                        x,
                        y,
                        text,
                        font_size,
                        ..
                    } if text == symbol && cell.contains(*x + 0.5, *y + 0.5) => {
                        Some((*x, *y, *font_size))
                    }
                    _ => None,
                })
                .expect("the symbol is drawn in its cell");
            let tw = text::measure(symbol, size, FontWeightHint::Bold);
            let th = text::line_height(size, FontWeightHint::Bold);
            let (cx, cy) = cell.centre();
            assert!(
                (x + tw / 2.0 - cx).abs() < 0.5,
                "{w}x{h}: off-centre across"
            );
            assert!((y + th / 2.0 - cy).abs() < 0.5, "{w}x{h}: off-centre down");
        }
    }

    // ── Tick tests ──────────────────────────────────────────────────

    #[test]
    fn test_tick_increments_total_elapsed() {
        let mut game = Match3::new();
        game.handle_tick(100);
        assert_eq!(game.total_elapsed_ms, 100);
        game.handle_tick(200);
        assert_eq!(game.total_elapsed_ms, 300);
    }

    #[test]
    fn test_tick_increments_pulse() {
        let mut game = Match3::new();
        let initial = game.pulse_counter;
        game.handle_tick(16);
        assert_eq!(game.pulse_counter, initial + 1);
    }

    // ── Shuffle tests ───────────────────────────────────────────────

    #[test]
    fn test_shuffle_produces_valid_moves() {
        let mut game = Match3::new();
        game.shuffle_board();
        assert!(game.has_valid_moves());
    }

    #[test]
    fn test_shuffle_preserves_gem_count() {
        let mut game = Match3::new();
        let count_before = game.gem_count();
        game.shuffle_board();
        assert_eq!(game.gem_count(), count_before);
    }

    // ── Gem equality tests ──────────────────────────────────────────

    #[test]
    fn test_gem_equality() {
        let a = make_gem(0);
        let b = make_gem(0);
        assert_eq!(a, b);
    }

    #[test]
    fn test_gem_inequality() {
        let a = make_gem(0);
        let b = make_gem(1);
        assert_ne!(a, b);
    }

    #[test]
    fn test_gem_special_inequality() {
        let a = make_gem(0);
        let b = make_special_gem(0, SpecialKind::LineClearH);
        assert_ne!(a, b);
    }

    // ── Run cascade tests ───────────────────────────────────────────

    #[test]
    fn test_run_cascade_clears_matches() {
        let mut game = empty_game();
        fill_no_matches(&mut game);
        game.board[7][0] = Some(make_gem(0));
        game.board[7][1] = Some(make_gem(0));
        game.board[7][2] = Some(make_gem(0));

        game.run_cascade();
        // After cascade, board should be full (filled empty spaces).
        assert_eq!(game.gem_count(), GRID_SIZE * GRID_SIZE);
        assert!(game.score > 0);
    }

    #[test]
    fn test_run_cascade_resets_chain_level() {
        let mut game = Match3::new();
        game.chain_level = 5;
        game.run_cascade();
        assert_eq!(game.chain_level, 0);
    }

    // ── Color bomb swap tests ───────────────────────────────────────

    /// Swapping a colour bomb must clear every gem of the swapped colour.
    ///
    /// This used to assert on `board[0][0]` *after* `try_swap` returned, which
    /// tested the refill rather than the detonation: `try_swap` finishes by
    /// filling the holes from the generator, and a refilled cell may perfectly
    /// well hold the colour that was just cleared. It passed under the old
    /// hand-rolled LCG because that generator happened not to deal an Emerald
    /// into that cell, and failed the moment the crate moved to `randrange` --
    /// a false alarm, but a fair one, since a test that depends on which gem a
    /// generator deals is not testing what its name claims.
    ///
    /// So it now checks the detonation directly, which is the step the name is
    /// about, and the refill is checked separately below.
    #[test]
    fn test_color_bomb_swap_clears_color() {
        let mut game = empty_game();
        fill_no_matches(&mut game);
        game.board[3][3] = Some(make_special_gem(0, SpecialKind::ColorBomb));
        game.board[3][4] = Some(make_gem(2));
        game.board[0][0] = Some(make_gem(2));
        game.board[5][5] = Some(make_gem(2));

        let old_score = game.score;
        game.detonate_color_bomb(Pos::new(3, 3), GemType::Emerald);

        assert!(game.score > old_score, "detonating scored nothing");
        assert!(game.board[3][3].is_none(), "the colour bomb survived");
        for r in 0..GRID_SIZE {
            for c in 0..GRID_SIZE {
                assert!(
                    game.board[r][c].is_none_or(|g| g.gem_type != GemType::Emerald),
                    "an Emerald survived at ({r}, {c})"
                );
            }
        }
    }

    /// The swap must go through the detonation and leave a full board behind.
    ///
    /// The counterpart to the test above: that one checks that the right gems
    /// are removed, this one that the holes are then filled. Together they
    /// cover what the single old assertion was reaching for, without either of
    /// them depending on which gems the generator deals.
    #[test]
    fn test_color_bomb_swap_leaves_a_full_board() {
        let mut game = empty_game();
        fill_no_matches(&mut game);
        game.board[3][3] = Some(make_special_gem(0, SpecialKind::ColorBomb));
        game.board[3][4] = Some(make_gem(2));

        let old_score = game.score;
        assert!(game.try_swap(Pos::new(3, 3), Pos::new(3, 4)));
        assert!(game.score > old_score);
        for r in 0..GRID_SIZE {
            for c in 0..GRID_SIZE {
                assert!(
                    game.board[r][c].is_some(),
                    "the refill left a hole at ({r}, {c})"
                );
            }
        }
    }

    // ── Comprehensive integration test ──────────────────────────────

    #[test]
    fn test_full_game_loop() {
        let mut game = Match3::with_seed(42);
        // Play through a few moves with keyboard.
        game.handle_event(&Event::Key(KeyEvent {
            key: Key::Right,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        }));
        game.handle_event(&Event::Key(KeyEvent {
            key: Key::Enter,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        }));
        game.handle_event(&Event::Key(KeyEvent {
            key: Key::Right,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        }));
        game.handle_event(&Event::Key(KeyEvent {
            key: Key::Enter,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        }));
        // Tick forward.
        game.handle_event(&Event::Tick { elapsed_ms: 100 });
        // Render should not panic.
        assert!(
            !game
                .frame(WINDOW_WIDTH, WINDOW_HEIGHT)
                .commands()
                .is_empty()
        );
    }
}

/// The wiring itself, driven through the real strap against a stand-in
/// compositor.
///
/// Every other test in this file exercises the game's own model. None of them
/// would notice if the `App` impl were deleted, or if `initial_size` returned
/// `(0, 0)`, or if `render` handed back an empty tree -- the program would
/// compile, launch, and show nothing. That is precisely the failure
/// `TD-NO-APP-CONNECTS-TO-THE-COMPOSITOR` is about, and it is invisible to a
/// model test by construction.
///
/// So this opens a window *through the trait* -- the title and the size are
/// the ones the game reports, not ones the test chose -- scripts real input,
/// and asserts frames came out and the close request was obeyed.
///
/// **Proved able to fail rather than assumed to be.** Replacing the body of
/// `App::render` with an empty `RenderTree` -- the shape of a wiring that
/// compiles and shows nothing -- makes this report *the game submitted 2
/// frame(s) for this window and every one was empty, so the window would be
/// blank*, while all 120 of the model tests keep passing.
#[cfg(test)]
mod reaches_a_window {
    // A failure here should point at the line that failed, which is what a
    // panicking assertion does. The defensive lints exist to keep panics out
    // of code that runs on a user's data; this is a harness. Same list, and
    // the same reason, as `mod tests` above.
    #![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

    use super::*;
    use oswindow::InputEvent;
    use oswindow::testing;

    #[test]
    fn the_game_opens_a_window_draws_and_closes() {
        let (mut events, desktop) = testing::desktop();
        let mut game = Match3::new();

        let window = app::open(&mut events, &game).expect("the compositor should grant a window");

        {
            let mut desk = desktop.borrow_mut();
            // A tick, because this game animates without input and a frame
            // that only ever follows a click would look frozen.
            desk.script.push_back(vec![InputEvent::new(
                window,
                Event::Tick { elapsed_ms: 16 },
            )]);
            desk.script
                .push_back(vec![InputEvent::new(window, Event::CloseRequested)]);
        }

        app::drive(&mut events, window, &mut game).expect("the loopback connection cannot fail");

        let desk = desktop.borrow();
        // NEGATIVE CONTROL FIRST. `submitted` being non-empty is what makes
        // the command count below mean "the game drew" rather than "nothing
        // ran and the filter found nothing".
        assert!(
            !desk.submitted.is_empty(),
            "no frame was submitted at all, so this test cannot say anything \
             about what was drawn"
        );
        let commands: usize = desk
            .submitted
            .iter()
            .filter(|(w, _)| *w == window)
            .map(|(_, n)| *n)
            .sum();
        assert!(
            commands > 0,
            "the game submitted {} frame(s) for this window and every one was \
             empty, so the window would be blank",
            desk.submitted.len()
        );
    }

    /// The window the game asks for shows everything: the header, the
    /// controls, the key line and a board of real cells.
    ///
    /// `initial_size` is the one part of the `App` impl a compositor obeys
    /// without question, and a wrong answer there is not a crash -- it is a
    /// window with the playfield clipped or its chrome given away.
    #[test]
    fn the_window_it_asks_for_shows_everything() {
        let game = Match3::new();
        let (w, h) = game.initial_size();
        assert_eq!((w, h), (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32));
        let l = Layout::new(w as f32, h as f32);
        assert!(!l.header.is_empty() && !l.controls.is_empty() && !l.footer.is_empty());
        assert!(
            l.cell >= 40.0,
            "cells of {} at the size it asks for",
            l.cell
        );
    }
}
