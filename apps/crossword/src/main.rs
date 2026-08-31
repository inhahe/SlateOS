//! Crossword -- a windowed crossword for SlateOS.
//!
//! Three built-in puzzles, an arrow-key cursor that is a cell of the grid, a
//! clue list every entry of which is clickable, a footer of buttons for check,
//! clear, reveal and back, and a clock that runs while the puzzle is open.
//!
//! # What wiring it found
//!
//! `main` was `let _app = CrosswordApp::new();` -- it parsed a puzzle, built
//! the clue list, dropped the lot and exited. No window opened.
//!
//! The picture was drawn from constants (`cell_size = 36.0` at `(20.0, 72.0)`,
//! a help card of exactly 360x140, a puzzle-list column at `x: 300.0`) while
//! the click that had to land on it was resolved by *a second copy of the same
//! constants* -- and the two copies had drifted: the drawing pass put the grid
//! at `grid_y = 72.0` and the click pass looked for it at `grid_y = 60.0`, so
//! every click landed twelve pixels above where the player aimed and the top
//! third of every cell selected the cell above it. There is one geometry now,
//! [`Layout`], derived from the live window, and the pass that paints a cell
//! records the box a click on it must land in.
//!
//! The clues did not match the grids. The puzzle definitions carried
//! hand-written clue *numbers* which the program then compared against numbers
//! it derived from the grid; the two disagreed in every puzzle, so half the
//! clues were anchored to the wrong word and eight words per puzzle had no
//! clue at all. Worse, the grids were not crosswords: the columns of "Easy
//! Start" spelled `CAB`, `AR`, `TES`, `SAPETRE`, `TE`, `OD`, `HA`, `AL`,
//! `EAN`, `BNS`, `ID`. Every grid is replaced with one in which every run of
//! two or more cells is a word in both directions, and a clue is now given by
//! *position* in the list of the grid's own word starts, so a number cannot
//! disagree with the grid it came from.
//!
//! The timer was a field nothing incremented. `load_puzzle` set `elapsed_secs`
//! to zero and `timer_running` to true, and no line anywhere added a second:
//! the clock in the corner read `00:00` for the whole puzzle and the end card
//! congratulated every player on a time of `00:00`.

use guitk::color::Color;
use guitk::event::{Event, EventResult, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::frame::{Frame, Rect};
use guitk::probe::Probe;
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::style::CornerRadii;
use guitk::text;
use guitk::wheel::Accumulator;
use oswindow::app::{self, App, Response};
use std::process::ExitCode;

// ── Catppuccin Mocha palette ────────────────────────────────────────
const BASE: Color = Color::from_hex(0x1E1E2E);
const MANTLE: Color = Color::from_hex(0x181825);
const CRUST: Color = Color::from_hex(0x11111B);
const SURFACE0: Color = Color::from_hex(0x313244);
const SURFACE1: Color = Color::from_hex(0x45475A);
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

// ── The window ──────────────────────────────────────────────────────
/// What the app asks the compositor for. It is a request, not a promise: every
/// rectangle in the frame comes from the width and height `render` is handed.
const WINDOW_WIDTH: f32 = 860.0;
const WINDOW_HEIGHT: f32 = 580.0;

const TITLE: &str = "Crossword";

/// A cell never grows past this, so a 5x5 grid in a very large window stays a
/// crossword rather than becoming five enormous squares.
const MAX_CELL: f32 = 96.0;

/// The share of the body's width the grid may take while the clue panel still
/// has somewhere to go.
const GRID_WIDTH_SHARE: f32 = 0.56;

/// A clue panel narrower than this holds no readable clue, so it is left out
/// and the grid takes the whole body instead.
const MIN_PANEL_WIDTH: f32 = 130.0;

/// How often the clock is asked what time it is.
const TICK_MS: u64 = 200;

const MILLIS_PER_SECOND: u64 = 1000;
const SECONDS_PER_MINUTE: u64 = 60;

// ── Layout ──────────────────────────────────────────────────────────
/// Every rectangle in the frame, derived from the window and the grid's shape.
///
/// Nothing here is a constant offset. The old code drew the grid at a fixed
/// `(20.0, 72.0)` with a fixed 36-pixel cell, put the clue panel at
/// `grid_x + width * cell + 20.0`, and sized the help card 360x280 -- so a
/// window smaller than the picture drew the picture outside the window, and a
/// larger one left most of itself empty.
#[derive(Clone, Copy, Debug)]
struct Layout {
    window: Rect,
    /// The bar carrying the puzzle name, the progress and the clock.
    header: Rect,
    /// The strip that names the word the cursor is in.
    banner: Rect,
    footer: Rect,
    /// The block of cells, exactly `cell * cols` by `cell * rows`.
    grid: Rect,
    /// The clue list. Empty when the window is too narrow to hold one.
    panel: Rect,
    cell: f32,
    pad: f32,
    title: f32,
    font: f32,
    small: f32,
}

impl Layout {
    fn solve(w: f32, h: f32, cols: usize, rows: usize) -> Self {
        let w = w.max(0.0);
        let h = h.max(0.0);
        let window = Rect::new(0.0, 0.0, w, h);
        let pad = (w.min(h) * 0.03).clamp(4.0, 18.0);
        let font = (h / 38.0).clamp(9.0, 17.0);
        let title = (font * 1.4).clamp(12.0, 23.0);
        let small = (font - 2.0).max(8.0);

        let header = Rect::new(0.0, 0.0, w, (title + pad * 1.6).min(h));
        let banner = Rect::new(
            0.0,
            header.bottom(),
            w,
            (font + pad * 1.1).min((h - header.h).max(0.0)),
        );
        let above = header.h + banner.h;
        let footer_h = (small * 2.4).min((h - above).max(0.0));
        let footer = Rect::new(0.0, h - footer_h, w, footer_h);

        // What the three bars leave, less a pad on every side.
        let body = Rect::new(
            pad,
            banner.bottom() + pad,
            (w - pad * 2.0).max(0.0),
            (footer.y - pad - (banner.bottom() + pad)).max(0.0),
        );

        // The cell size that fits a given width, and the height, and the cap.
        let fit = |avail: f32| {
            #[expect(
                clippy::cast_precision_loss,
                reason = "grid dimensions are single digits; exact in f32"
            )]
            let (c, r) = (cols.max(1) as f32, rows.max(1) as f32);
            (avail / c).min(body.h / r).clamp(0.0, MAX_CELL)
        };
        // Reserve a share for the panel, and give it back if what is left over
        // could not hold a clue anyway.
        let shared = fit(body.w * GRID_WIDTH_SHARE);
        #[expect(
            clippy::cast_precision_loss,
            reason = "grid dimensions are single digits; exact in f32"
        )]
        let grid_w = shared * cols.max(1) as f32;
        let cell = if body.w - grid_w - pad >= MIN_PANEL_WIDTH {
            shared
        } else {
            fit(body.w)
        };

        #[expect(
            clippy::cast_precision_loss,
            reason = "grid dimensions are single digits; exact in f32"
        )]
        let grid = Rect::new(
            body.x,
            body.y,
            cell * cols.max(1) as f32,
            cell * rows.max(1) as f32,
        );
        let panel_x = grid.right() + pad;
        let panel_w = (body.right() - panel_x).max(0.0);
        let panel = if panel_w >= MIN_PANEL_WIDTH {
            Rect::new(panel_x, body.y, panel_w, body.h)
        } else {
            Rect::EMPTY
        };

        Self {
            window,
            header,
            banner,
            footer,
            grid,
            panel,
            cell,
            pad,
            title,
            font,
            small,
        }
    }

    /// The square a cell of the grid occupies.
    ///
    /// Cells tile exactly and `Rect::contains` is half-open, so two of them can
    /// never both claim a pixel -- which is what makes recording the rectangle
    /// as the cell is painted a complete answer to "what did I click".
    #[expect(
        clippy::cast_precision_loss,
        reason = "grid dimensions are single digits; exact in f32"
    )]
    fn cell_rect(&self, row: usize, col: usize) -> Rect {
        Rect::new(
            self.cell.mul_add(col as f32, self.grid.x),
            self.cell.mul_add(row as f32, self.grid.y),
            self.cell,
            self.cell,
        )
    }

    /// The height of one row of the clue list.
    fn clue_row_h(&self) -> f32 {
        (self.small * 1.7).max(1.0)
    }

    /// How many clue rows the panel can show whole.
    fn clue_rows_visible(&self) -> usize {
        if self.panel.is_empty() {
            return 0;
        }
        // The heading takes the first row.
        let usable = (self.panel.h - self.clue_row_h()).max(0.0);
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a non-negative ratio of two small positive lengths"
        )]
        let n = (usable / self.clue_row_h()).floor() as usize;
        n
    }
}

// ── Direction ───────────────────────────────────────────────────────
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Direction {
    Across,
    Down,
}

impl Direction {
    fn other(self) -> Self {
        match self {
            Self::Across => Self::Down,
            Self::Down => Self::Across,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Across => "Across",
            Self::Down => "Down",
        }
    }

    /// One step along this direction, as `(row, col)`.
    fn step(self) -> (isize, isize) {
        match self {
            Self::Across => (0, 1),
            Self::Down => (1, 0),
        }
    }

    /// One step against this direction.
    ///
    /// Spelled out rather than negating [`Self::step`]: the workspace denies
    /// `clippy::arithmetic_side_effects`, and `-dr` on an `isize` is an
    /// arithmetic operation that can overflow while a constant cannot.
    fn back(self) -> (isize, isize) {
        match self {
            Self::Across => (0, -1),
            Self::Down => (-1, 0),
        }
    }

    /// A step along this direction, or against it when `forward` is false.
    fn step_toward(self, forward: bool) -> (isize, isize) {
        if forward { self.step() } else { self.back() }
    }
}

// ── Cell ────────────────────────────────────────────────────────────
#[derive(Debug, Clone)]
struct Cell {
    /// The letter the grid says belongs here.
    solution: char,
    /// What the player has put here.
    entry: Option<char>,
    /// The number printed in the corner; zero when the cell starts no word.
    number: u16,
    /// Whether this letter was given away rather than worked out.
    revealed: bool,
}

impl Cell {
    fn new(solution: char) -> Self {
        Self {
            solution,
            entry: None,
            number: 0,
            revealed: false,
        }
    }

    /// Whether this cell holds its answer.
    ///
    /// Note that this already implies the cell is *filled*: a cell whose entry
    /// equals its solution has an entry. The old completion test asked
    /// `all_filled && all_correct`, and no board could ever distinguish the two
    /// halves -- deleting the first changed nothing observable (lesson 92).
    fn is_correct(&self) -> bool {
        self.entry == Some(self.solution)
    }
}

// ── Clue ────────────────────────────────────────────────────────────
#[derive(Debug, Clone)]
struct Clue {
    /// Taken from the cell the word starts in, never written by hand.
    number: u16,
    direction: Direction,
    text: &'static str,
    row: usize,
    col: usize,
    len: usize,
}

// ── Puzzle definitions ──────────────────────────────────────────────
/// A puzzle as it is written down.
///
/// The clue lists are in **grid order** and carry no numbers: the `i`-th
/// across clue belongs to the `i`-th across word the grid starts, reading the
/// grid left-to-right and top-to-bottom. The old definitions carried a number
/// per clue which the program matched against the numbering it computed from
/// the grid, and in all three puzzles the two disagreed -- a clue whose number
/// named a *down* start was hung on whatever cell happened to carry that
/// number, and the words the tables never mentioned got no clue at all.
struct PuzzleDef {
    name: &'static str,
    width: usize,
    height: usize,
    /// Row-major, `#` for a black square and a letter for a playable one.
    grid: &'static str,
    across: &'static [&'static str],
    down: &'static [&'static str],
}

/// Three 5x5 puzzles in which **every** run of two or more cells is a word,
/// down as well as across. The grids that were here before were built from
/// across answers alone and their columns spelled `SAPETRE`, `HALLILA` and
/// `FSIVRNA`; `every_run_of_two_or_more_is_a_word` is the test that would not
/// have let them in.
const PUZZLES: &[PuzzleDef] = &[
    PuzzleDef {
        name: "Warm Up",
        width: 5,
        height: 5,
        grid: "\
SPA#S\
TAR#P\
ELITE\
A#SEE\
K#END",
        across: &[
            "Place for a massage and a soak",
            "Cover with pitch",
            "The pick of the crop",
            "Take in with the eyes",
            "Where the road stops",
        ],
        down: &[
            "Cut of beef for the grill",
            "Close friend",
            "Get up, or come about",
            "How fast a thing is going",
            "Fingers on two hands",
        ],
    },
    PuzzleDef {
        name: "Centre Stage",
        width: 5,
        height: 5,
        grid: "\
COG#P\
EAR#L\
DRAMA\
A#CAT\
R#EYE",
        across: &[
            "Toothed wheel in a machine",
            "What a whisper is aimed at",
            "A play, or a fuss",
            "Animal that purrs",
            "A needle's is for thread",
        ],
        down: &[
            "Fragrant wood of a lined chest",
            "What a rower pulls",
            "Elegance of movement",
            "Dinner is served on it",
            "Month between April and June",
        ],
    },
    PuzzleDef {
        name: "Around the House",
        width: 5,
        height: 5,
        grid: "\
BOW#T\
AIR#H\
SLIDE\
I#SUM\
N#TOE",
        across: &[
            "Ribbon tied in a loop",
            "What a lung is for",
            "Playground chute",
            "What the additions come to",
            "The foot's smallest digit",
        ],
        down: &[
            "Bowl fixed to a bathroom wall",
            "It floats on water",
            "Where a watch is worn",
            "The idea a set of things share",
            "A pair performing together",
        ],
    },
];

/// Written where a puzzle's clue list is shorter than its grid's word count,
/// so the gap is visible in the panel rather than silently dropping the word.
/// `every_word_in_every_puzzle_has_a_clue` asserts no puzzle produces one.
const MISSING_CLUE: &str = "(no clue)";

// ── The view ────────────────────────────────────────────────────────
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum View {
    PuzzleSelect,
    Playing,
    Completed,
}

// ── What a click can land on ────────────────────────────────────────
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    /// A playable cell of the grid, by row and column.
    Cell(usize, usize),
    /// A row of the clue list, by index into `clues`.
    ClueRow(usize),
    /// A row of the puzzle menu.
    PuzzleRow(usize),
    Button(Button),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Button {
    Check,
    Clear,
    RevealLetter,
    RevealWord,
    Help,
    Menu,
}

impl Button {
    fn label(self) -> &'static str {
        match self {
            Self::Check => "Check",
            Self::Clear => "Clear",
            Self::RevealLetter => "Letter",
            Self::RevealWord => "Word",
            Self::Help => "Help",
            Self::Menu => "Menu",
        }
    }

    /// The key that does the same thing, for the help card.
    fn key_hint(self) -> &'static str {
        match self {
            Self::Check => "Ctrl+C",
            Self::Clear => "Ctrl+U",
            Self::RevealLetter => "Ctrl+R",
            Self::RevealWord => "Ctrl+W",
            Self::Help => "F1",
            Self::Menu => "Esc",
        }
    }
}

/// The footer's buttons, in the order they are laid out.
const BUTTONS: [Button; 6] = [
    Button::Check,
    Button::Clear,
    Button::RevealLetter,
    Button::RevealWord,
    Button::Help,
    Button::Menu,
];

// ── The app ─────────────────────────────────────────────────────────
struct Crossword {
    view: View,
    /// Which row of the menu is highlighted.
    selected_puzzle: usize,
    /// Which puzzle is loaded; the name and size are read from it rather than
    /// copied out of it.
    puzzle: usize,
    width: usize,
    height: usize,
    /// Row-major. `None` is a black square.
    cells: Vec<Option<Cell>>,
    /// Across clues by number, then down clues by number.
    clues: Vec<Clue>,
    cursor: (usize, usize),
    direction: Direction,
    /// The clock. It is milliseconds because that is what a tick carries; the
    /// old `elapsed_secs` was a whole number of seconds that nothing ever
    /// advanced.
    elapsed_ms: u64,
    check_mode: bool,
    show_help: bool,
    clue_scroll: usize,
    scroll: Accumulator,
    /// The window the last frame was drawn in, so a click can be resolved
    /// against the picture the player is actually looking at.
    size: (f32, f32),
}

impl Crossword {
    fn new() -> Self {
        Self {
            view: View::PuzzleSelect,
            selected_puzzle: 0,
            puzzle: 0,
            width: 0,
            height: 0,
            cells: Vec::new(),
            clues: Vec::new(),
            cursor: (0, 0),
            direction: Direction::Across,
            elapsed_ms: 0,
            check_mode: false,
            show_help: false,
            clue_scroll: 0,
            scroll: Accumulator::default(),
            size: (WINDOW_WIDTH, WINDOW_HEIGHT),
        }
    }

    fn def(&self) -> Option<&'static PuzzleDef> {
        PUZZLES.get(self.puzzle)
    }

    fn puzzle_name(&self) -> &'static str {
        self.def().map_or("", |d| d.name)
    }

    // ── Loading ────────────────────────────────────────────────────

    fn load_puzzle(&mut self, index: usize) {
        let Some(def) = PUZZLES.get(index) else {
            return;
        };
        self.puzzle = index;
        self.selected_puzzle = index;
        self.width = def.width;
        self.height = def.height;

        let letters: Vec<char> = def.grid.chars().collect();
        self.cells = (0..def.width.saturating_mul(def.height))
            .map(|i| match letters.get(i) {
                Some(&ch) if ch != '#' => Some(Cell::new(ch)),
                _ => None,
            })
            .collect();

        self.number_and_clue(def);

        self.cursor = self.first_playable().unwrap_or((0, 0));
        self.direction = Direction::Across;
        self.elapsed_ms = 0;
        self.check_mode = false;
        self.show_help = false;
        self.clue_scroll = 0;
        self.view = View::Playing;
    }

    /// Number every cell that starts a word and hang a clue on each.
    ///
    /// The number a clue carries is read off the grid here; it is never
    /// written down beside the clue text, which is what let the two disagree.
    fn number_and_clue(&mut self, def: &'static PuzzleDef) {
        let mut starts: Vec<(u16, usize, usize, Direction)> = Vec::new();
        let mut number: u16 = 0;
        for row in 0..self.height {
            for col in 0..self.width {
                let across = self.starts_word(row, col, Direction::Across);
                let down = self.starts_word(row, col, Direction::Down);
                if !across && !down {
                    continue;
                }
                number = number.saturating_add(1);
                if let Some(cell) = self.cell_mut(row, col) {
                    cell.number = number;
                }
                if across {
                    starts.push((number, row, col, Direction::Across));
                }
                if down {
                    starts.push((number, row, col, Direction::Down));
                }
            }
        }

        self.clues.clear();
        for dir in [Direction::Across, Direction::Down] {
            let texts = match dir {
                Direction::Across => def.across,
                Direction::Down => def.down,
            };
            for (i, &(number, row, col, _)) in
                starts.iter().filter(|(_, _, _, d)| *d == dir).enumerate()
            {
                self.clues.push(Clue {
                    number,
                    direction: dir,
                    text: texts.get(i).copied().unwrap_or(MISSING_CLUE),
                    row,
                    col,
                    len: self.word_cells(row, col, dir).len(),
                });
            }
        }
    }

    // ── Reading the grid ───────────────────────────────────────────

    /// The one place that turns a coordinate into an index.
    ///
    /// Every reader used to do its own bounds reasoning -- `word_length`
    /// computed `r * width + c` before it checked whether `r` was on the board,
    /// `load_puzzle` reached for `cells[idx.wrapping_sub(1)]` and relied on a
    /// short circuit to the left of it to keep the subtraction from wrapping,
    /// and `cell_at` had its own copy of the comparison. Off the board is
    /// `None` here, once.
    fn index(&self, row: usize, col: usize) -> Option<usize> {
        if row >= self.height || col >= self.width {
            return None;
        }
        row.checked_mul(self.width)?.checked_add(col)
    }

    fn cell(&self, row: usize, col: usize) -> Option<&Cell> {
        self.cells.get(self.index(row, col)?)?.as_ref()
    }

    fn cell_mut(&mut self, row: usize, col: usize) -> Option<&mut Cell> {
        let idx = self.index(row, col)?;
        self.cells.get_mut(idx)?.as_mut()
    }

    fn playable(&self, row: usize, col: usize) -> bool {
        self.cell(row, col).is_some()
    }

    fn first_playable(&self) -> Option<(usize, usize)> {
        (0..self.height)
            .flat_map(|r| (0..self.width).map(move |c| (r, c)))
            .find(|&(r, c)| self.playable(r, c))
    }

    /// Whether a word of two or more cells begins at `(row, col)` in `dir`.
    ///
    /// This is the whole of the numbering rule and the whole of the clue-start
    /// rule, in one place and in one copy per direction rather than two
    /// hand-expanded ones.
    fn starts_word(&self, row: usize, col: usize, dir: Direction) -> bool {
        if !self.playable(row, col) {
            return false;
        }
        !self.offset_playable(row, col, dir.back()) && self.offset_playable(row, col, dir.step())
    }

    /// Whether the cell one `step` away from `(row, col)` is playable.
    fn offset_playable(&self, row: usize, col: usize, step: (isize, isize)) -> bool {
        self.offset(row, col, step)
            .is_some_and(|(r, c)| self.playable(r, c))
    }

    /// `(row, col)` moved by `step`, or `None` off the board.
    fn offset(&self, row: usize, col: usize, step: (isize, isize)) -> Option<(usize, usize)> {
        let r = row.checked_add_signed(step.0)?;
        let c = col.checked_add_signed(step.1)?;
        (r < self.height && c < self.width).then_some((r, c))
    }

    /// The first cell of the word `(row, col)` lies in, along `dir`.
    fn word_start(&self, row: usize, col: usize, dir: Direction) -> (usize, usize) {
        let (mut r, mut c) = (row, col);
        while let Some((pr, pc)) = self.offset(r, c, dir.back()) {
            if !self.playable(pr, pc) {
                break;
            }
            (r, c) = (pr, pc);
        }
        (r, c)
    }

    /// Every cell of the word `(row, col)` lies in, along `dir`.
    ///
    /// The old code walked a word in four places -- `word_length`,
    /// `reveal_word`, `cells_in_current_word` and the render's highlight --
    /// each with its own loop and its own bounds test.
    fn word_cells(&self, row: usize, col: usize, dir: Direction) -> Vec<(usize, usize)> {
        let (mut r, mut c) = self.word_start(row, col, dir);
        let mut out = Vec::new();
        while self.playable(r, c) {
            out.push((r, c));
            let Some(next) = self.offset(r, c, dir.step()) else {
                break;
            };
            (r, c) = next;
        }
        out
    }

    /// The cells of the word the cursor is in, in the direction it faces.
    fn current_word(&self) -> Vec<(usize, usize)> {
        self.word_cells(self.cursor.0, self.cursor.1, self.direction)
    }

    /// Whether a cell should be shown as wrong.
    ///
    /// Derived, not stored. `flagged_wrong` used to be a field on every cell,
    /// set by `check_answers` and cleared by four separate call sites, so the
    /// mark could disagree with the letter it was marking.
    fn is_wrong(&self, row: usize, col: usize) -> bool {
        self.check_mode
            && self
                .cell(row, col)
                .is_some_and(|cell| cell.entry.is_some() && !cell.is_correct())
    }

    fn filled_count(&self) -> (usize, usize) {
        let total = self.cells.iter().flatten().count();
        let filled = self
            .cells
            .iter()
            .flatten()
            .filter(|cell| cell.entry.is_some())
            .count();
        (filled, total)
    }

    fn revealed_count(&self) -> usize {
        self.cells.iter().flatten().filter(|c| c.revealed).count()
    }

    /// Whether every cell holds its answer.
    fn is_solved(&self) -> bool {
        !self.cells.is_empty() && self.cells.iter().flatten().all(Cell::is_correct)
    }

    // ── The clock ──────────────────────────────────────────────────

    fn elapsed_secs(&self) -> u64 {
        self.elapsed_ms / MILLIS_PER_SECOND
    }

    fn format_time(&self) -> String {
        let secs = self.elapsed_secs();
        let (m, s) = (secs / SECONDS_PER_MINUTE, secs % SECONDS_PER_MINUTE);
        format!("{m:02}:{s:02}")
    }

    /// The clock runs while a puzzle is open and stops when it is solved.
    fn handle_tick(&mut self, elapsed_ms: u64) -> EventResult {
        if self.view != View::Playing {
            return EventResult::Ignored;
        }
        self.elapsed_ms = self.elapsed_ms.saturating_add(elapsed_ms);
        EventResult::Consumed
    }

    // ── The cursor ─────────────────────────────────────────────────

    /// Move to the next playable cell in `dir`, crossing black squares.
    ///
    /// The four arrow keys used to be four hand-written loops, each with its
    /// own bounds test and its own `if cursor == 0 { return }` guard in front
    /// of a subtraction -- and `Key::Up`'s guard was in the match arm while
    /// `Key::Down`'s was in the loop, so the two directions were not the same
    /// code in any sense a reader could check.
    fn arrow(&mut self, step: (isize, isize), face: Direction) -> bool {
        let (mut r, mut c) = self.cursor;
        while let Some((nr, nc)) = self.offset(r, c, step) {
            (r, c) = (nr, nc);
            if self.playable(r, c) {
                self.cursor = (r, c);
                self.direction = face;
                return true;
            }
        }
        false
    }

    /// One step along the current word, or `None` at either end of it.
    ///
    /// Typing stops at the end of a word; the arrows cross to the next one.
    /// The old `advance_cursor` scanned to the next playable cell in the row,
    /// which jumped the black square and carried on typing into a different
    /// word.
    fn step_in_word(&self, forward: bool) -> Option<(usize, usize)> {
        self.offset(
            self.cursor.0,
            self.cursor.1,
            self.direction.step_toward(forward),
        )
        .filter(|&(r, c)| self.playable(r, c))
    }

    // ── Playing ────────────────────────────────────────────────────

    fn enter_letter(&mut self, ch: char) {
        let upper = ch.to_ascii_uppercase();
        let Some(cell) = self.cell_mut(self.cursor.0, self.cursor.1) else {
            return;
        };
        cell.entry = Some(upper);
        // A letter the player typed is the player's, even where one was given
        // away before. The old code left `revealed` set, so the end card went
        // on counting a letter the player had since worked out for themselves.
        cell.revealed = false;
        if let Some(next) = self.step_in_word(true) {
            self.cursor = next;
        }
        self.settle();
    }

    fn delete_letter(&mut self) {
        if self
            .cell(self.cursor.0, self.cursor.1)
            .is_some_and(|c| c.entry.is_some())
        {
            self.clear_cell();
            return;
        }
        if let Some(prev) = self.step_in_word(false) {
            self.cursor = prev;
            self.clear_cell();
        }
    }

    fn clear_cell(&mut self) {
        if let Some(cell) = self.cell_mut(self.cursor.0, self.cursor.1) {
            cell.entry = None;
            cell.revealed = false;
        }
    }

    fn check_answers(&mut self) {
        self.check_mode = true;
    }

    fn clear_checks(&mut self) {
        self.check_mode = false;
    }

    fn reveal_letter(&mut self) {
        if let Some(cell) = self.cell_mut(self.cursor.0, self.cursor.1) {
            cell.entry = Some(cell.solution);
            cell.revealed = true;
        }
        self.settle();
    }

    fn reveal_word(&mut self) {
        for (r, c) in self.current_word() {
            if let Some(cell) = self.cell_mut(r, c) {
                cell.entry = Some(cell.solution);
                cell.revealed = true;
            }
        }
        self.settle();
    }

    /// End the puzzle when it is solved.
    fn settle(&mut self) {
        if self.is_solved() {
            self.view = View::Completed;
        }
    }

    fn toggle_direction(&mut self) {
        self.direction = self.direction.other();
    }

    // ── The clues ──────────────────────────────────────────────────

    /// The index in `clues` of the word the cursor is in.
    fn current_clue(&self) -> Option<usize> {
        let (r, c) = self.word_start(self.cursor.0, self.cursor.1, self.direction);
        let number = self.cell(r, c)?.number;
        self.clues
            .iter()
            .position(|cl| cl.number == number && cl.direction == self.direction)
    }

    /// Put the cursor on the first cell of clue `index`.
    fn go_to_clue(&mut self, index: usize) {
        let Some(clue) = self.clues.get(index) else {
            return;
        };
        self.cursor = (clue.row, clue.col);
        self.direction = clue.direction;
        self.show_clue(index);
    }

    /// The next clue in the list, wrapping. `forward` is Tab; the reverse is
    /// Shift+Tab, which the old `move_to_next_clue(_reverse: bool)` took an
    /// argument for and then ignored.
    fn cycle_clue(&mut self, forward: bool) {
        let len = self.clues.len();
        if len == 0 {
            return;
        }
        let step = if forward { 1 } else { len.saturating_sub(1) };
        let raw = self.current_clue().unwrap_or(0).saturating_add(step);
        let Some(next) = raw.checked_rem(len) else {
            return;
        };
        self.go_to_clue(next);
    }

    /// Scroll the panel so that clue `index` is on it.
    fn show_clue(&mut self, index: usize) {
        let visible = self.layout().clue_rows_visible();
        if visible == 0 {
            return;
        }
        if index < self.clue_scroll {
            self.clue_scroll = index;
        } else if index >= self.clue_scroll.saturating_add(visible) {
            self.clue_scroll = index.saturating_sub(visible.saturating_sub(1));
        }
    }

    /// The furthest the clue list may be scrolled, so the last row is the last
    /// clue rather than a screen of nothing.
    fn max_scroll(&self) -> usize {
        let visible = self.layout().clue_rows_visible();
        self.clues.len().saturating_sub(visible)
    }

    fn scroll_clues(&mut self, rows: isize) -> bool {
        let before = self.clue_scroll;
        let moved = if rows >= 0 {
            self.clue_scroll
                .saturating_add(rows.unsigned_abs())
                .min(self.max_scroll())
        } else {
            self.clue_scroll.saturating_sub(rows.unsigned_abs())
        };
        self.clue_scroll = moved.min(self.max_scroll());
        self.clue_scroll != before
    }

    // ── The menu ───────────────────────────────────────────────────

    fn go_to_menu(&mut self) {
        self.view = View::PuzzleSelect;
        self.show_help = false;
    }

    fn select(&mut self, index: usize) {
        if index < PUZZLES.len() {
            self.selected_puzzle = index;
        }
    }

    // ── Events ─────────────────────────────────────────────────────

    fn layout(&self) -> Layout {
        Layout::solve(self.size.0, self.size.1, self.width, self.height)
    }

    fn handle_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::Tick { elapsed_ms } => self.handle_tick(*elapsed_ms),
            Event::Key(key) => self.handle_key(key),
            Event::Mouse(mouse) => self.handle_mouse(mouse),
            _ => EventResult::Ignored,
        }
    }

    fn handle_mouse(&mut self, mouse: &MouseEvent) -> EventResult {
        match mouse.kind {
            MouseEventKind::Press(MouseButton::Left) => self.handle_click(mouse.x, mouse.y),
            MouseEventKind::Scroll { dy, .. } => {
                let rows = self.scroll.rows(dy);
                if rows == 0 {
                    return EventResult::Ignored;
                }
                if self.scroll_clues(rows) {
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            _ => EventResult::Ignored,
        }
    }

    /// Resolve a click against the boxes the last frame recorded.
    ///
    /// The old `handle_grid_click` re-derived the geometry from its own copies
    /// of `cell_size`, `grid_x` and `grid_y` -- and its `grid_y` was `60.0`
    /// while the drawing pass used `72.0`, so every click was resolved against
    /// a grid twelve pixels above the one on the screen.
    fn handle_click(&mut self, x: f32, y: f32) -> EventResult {
        let Some(target) = self.frame(self.size.0, self.size.1).hit_test(x, y) else {
            // A click on the help card's dimmed backdrop puts it away, which is
            // the only thing a click outside every box means while it is up.
            if self.show_help {
                self.show_help = false;
                return EventResult::Consumed;
            }
            return EventResult::Ignored;
        };
        self.activate(target)
    }

    fn activate(&mut self, target: Target) -> EventResult {
        match target {
            Target::Cell(row, col) => {
                if self.cursor == (row, col) {
                    self.toggle_direction();
                } else {
                    self.cursor = (row, col);
                }
                if let Some(index) = self.current_clue() {
                    self.show_clue(index);
                }
                EventResult::Consumed
            }
            Target::ClueRow(index) => {
                self.go_to_clue(index);
                EventResult::Consumed
            }
            Target::PuzzleRow(index) => {
                self.select(index);
                self.load_puzzle(index);
                EventResult::Consumed
            }
            Target::Button(button) => {
                self.press(button);
                EventResult::Consumed
            }
        }
    }

    /// What a button does. Every one of these is also a key, and
    /// `the_buttons_and_the_keys_do_the_same_thing` asserts they agree.
    fn press(&mut self, button: Button) {
        match button {
            Button::Check => self.check_answers(),
            Button::Clear => self.clear_checks(),
            Button::RevealLetter => self.reveal_letter(),
            Button::RevealWord => self.reveal_word(),
            Button::Help => self.show_help = !self.show_help,
            Button::Menu => self.go_to_menu(),
        }
    }

    fn handle_key(&mut self, key: &KeyEvent) -> EventResult {
        match self.view {
            View::PuzzleSelect => self.key_in_menu(key),
            View::Playing => self.key_in_puzzle(key),
            View::Completed => {
                if matches!(key.key, Key::Enter | Key::Escape) {
                    self.go_to_menu();
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
        }
    }

    fn key_in_menu(&mut self, key: &KeyEvent) -> EventResult {
        match key.key {
            Key::Up => {
                self.select(self.selected_puzzle.saturating_sub(1));
                EventResult::Consumed
            }
            Key::Down => {
                self.select(
                    self.selected_puzzle
                        .saturating_add(1)
                        .min(PUZZLES.len().saturating_sub(1)),
                );
                EventResult::Consumed
            }
            Key::Enter => {
                self.load_puzzle(self.selected_puzzle);
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    fn key_in_puzzle(&mut self, key: &KeyEvent) -> EventResult {
        if key.modifiers.ctrl {
            let button = match key.key {
                Key::C => Button::Check,
                Key::U => Button::Clear,
                Key::R => Button::RevealLetter,
                Key::W => Button::RevealWord,
                _ => return EventResult::Ignored,
            };
            self.press(button);
            return EventResult::Consumed;
        }

        let handled = match key.key {
            Key::Up => self.arrow((-1, 0), Direction::Down),
            Key::Down => self.arrow((1, 0), Direction::Down),
            Key::Left => self.arrow((0, -1), Direction::Across),
            Key::Right => self.arrow((0, 1), Direction::Across),
            Key::Space => {
                self.toggle_direction();
                true
            }
            Key::Tab => {
                self.cycle_clue(!key.modifiers.shift);
                true
            }
            Key::Backspace => {
                self.delete_letter();
                true
            }
            Key::F1 => {
                self.press(Button::Help);
                true
            }
            Key::Escape => {
                if self.show_help {
                    self.show_help = false;
                } else {
                    self.go_to_menu();
                }
                true
            }
            other => match letter_of(other) {
                Some(ch) => {
                    self.enter_letter(ch);
                    true
                }
                None => false,
            },
        };
        if handled {
            EventResult::Consumed
        } else {
            EventResult::Ignored
        }
    }
}

/// The letter a key types, or `None` for a key that types nothing.
///
/// This used to answer `'\0'` for "not a letter" and the caller asked
/// `is_ascii_alphabetic` of the answer -- a sentinel character standing in for
/// an absence the type system can carry.
fn letter_of(key: Key) -> Option<char> {
    let letters = [
        (Key::A, 'A'),
        (Key::B, 'B'),
        (Key::C, 'C'),
        (Key::D, 'D'),
        (Key::E, 'E'),
        (Key::F, 'F'),
        (Key::G, 'G'),
        (Key::H, 'H'),
        (Key::I, 'I'),
        (Key::J, 'J'),
        (Key::K, 'K'),
        (Key::L, 'L'),
        (Key::M, 'M'),
        (Key::N, 'N'),
        (Key::O, 'O'),
        (Key::P, 'P'),
        (Key::Q, 'Q'),
        (Key::R, 'R'),
        (Key::S, 'S'),
        (Key::T, 'T'),
        (Key::U, 'U'),
        (Key::V, 'V'),
        (Key::W, 'W'),
        (Key::X, 'X'),
        (Key::Y, 'Y'),
        (Key::Z, 'Z'),
    ];
    letters
        .into_iter()
        .find_map(|(k, ch)| (k == key).then_some(ch))
}

// ── Drawing ─────────────────────────────────────────────────────────

impl Crossword {
    fn frame(&self, width: f32, height: f32) -> Frame<Target> {
        let mut f = Frame::new(width, height);
        let l = Layout::solve(width, height, self.width, self.height);
        fill(&mut f, l.window, BASE, 0.0);
        match self.view {
            View::PuzzleSelect => self.draw_menu(&mut f, &l),
            View::Playing => {
                self.draw_header(&mut f, &l);
                self.draw_banner(&mut f, &l);
                self.draw_grid(&mut f, &l);
                self.draw_panel(&mut f, &l);
                self.draw_footer(&mut f, &l);
                if self.show_help {
                    self.draw_help(&mut f, &l);
                }
            }
            View::Completed => self.draw_completed(&mut f, &l),
        }
        f
    }

    /// The puzzle menu. Every row is a button now; it used to be a list you
    /// could only reach with the arrow keys, drawn with its size column pinned
    /// at `x: 300.0` whatever the window was.
    fn draw_menu(&self, f: &mut Frame<Target>, l: &Layout) {
        let heading = "Crossword Puzzles";
        centred(
            f,
            l.window.x,
            l.window.w,
            l.pad,
            heading,
            TEXT_COLOR,
            l.title,
            FontWeightHint::Bold,
        );

        let row_h = (l.title * 2.0).max(1.0);
        let mut y = l.pad + l.title * 2.2;
        for (i, def) in PUZZLES.iter().enumerate() {
            let r = Rect::new(l.pad, y, (l.window.w - l.pad * 2.0).max(0.0), row_h);
            if r.bottom() > l.window.h - l.pad {
                break;
            }
            let on = i == self.selected_puzzle;
            fill(f, r, if on { SURFACE1 } else { SURFACE0 }, l.pad * 0.4);
            text_at(
                f,
                r.x + l.pad,
                r.y + (row_h - l.font) / 2.0,
                def.name,
                if on { BLUE } else { TEXT_COLOR },
                l.font,
                FontWeightHint::Bold,
            );
            let size = format!("{}x{}", def.width, def.height);
            let w = text::measure(&size, l.small, FontWeightHint::Regular);
            text_at(
                f,
                r.right() - l.pad - w,
                r.y + (row_h - l.small) / 2.0,
                &size,
                SUBTEXT0,
                l.small,
                FontWeightHint::Regular,
            );
            f.hit(Target::PuzzleRow(i), r);
            y += row_h + l.pad * 0.5;
        }

        let hint = "Up/Down to choose, Enter to start -- or click a puzzle";
        if y + l.small <= l.window.h {
            text_at(
                f,
                l.pad,
                l.window.h - l.pad - l.small,
                hint,
                OVERLAY0,
                l.small,
                FontWeightHint::Regular,
            );
        }
    }

    fn draw_header(&self, f: &mut Frame<Target>, l: &Layout) {
        fill(f, l.header, MANTLE, 0.0);
        if l.header.is_empty() {
            return;
        }
        let y = l.header.y + (l.header.h - l.title) / 2.0;
        text_at(
            f,
            l.pad,
            y,
            self.puzzle_name(),
            TEXT_COLOR,
            l.title,
            FontWeightHint::Bold,
        );

        // The readouts are laid out from the right edge of the window they are
        // in. They used to sit at `width - 100.0` and `width - 220.0`, which
        // are widths used as coordinates: in a 200-pixel window both were off
        // the left edge.
        let (filled, total) = self.filled_count();
        let right = [
            (self.format_time(), TEXT_COLOR),
            (format!("{filled}/{total}"), SUBTEXT0),
        ];
        let mut x = l.header.right() - l.pad;
        for (s, colour) in right {
            let w = text::measure(&s, l.font, FontWeightHint::Regular);
            x -= w;
            if x < l.pad {
                break;
            }
            text_at(
                f,
                x,
                l.header.y + (l.header.h - l.font) / 2.0,
                &s,
                colour,
                l.font,
                FontWeightHint::Regular,
            );
            x -= l.pad;
        }
    }

    /// The strip naming the word the cursor is in.
    fn draw_banner(&self, f: &mut Frame<Target>, l: &Layout) {
        fill(f, l.banner, CRUST, 0.0);
        if l.banner.is_empty() {
            return;
        }
        let Some(clue) = self.current_clue().and_then(|i| self.clues.get(i)) else {
            return;
        };
        let s = format!(
            "{} {} ({}): {}",
            clue.number,
            clue.direction.label(),
            clue.len,
            clue.text
        );
        // Bounded to the window: a clue is arbitrary text, and unbounded it ran
        // straight off the right-hand edge.
        f.push(RenderCommand::Text {
            x: l.pad,
            y: l.banner.y + (l.banner.h - l.font) / 2.0,
            text: s,
            font_size: l.font,
            color: YELLOW,
            font_weight: FontWeightHint::Regular,
            max_width: Some((l.banner.w - l.pad * 2.0).max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });
    }

    /// The grid, and the box a click on each cell must land in.
    fn draw_grid(&self, f: &mut Frame<Target>, l: &Layout) {
        if l.grid.is_empty() || l.cell <= 0.0 {
            return;
        }
        let word = self.current_word();
        for row in 0..self.height {
            for col in 0..self.width {
                let r = l.cell_rect(row, col);
                let Some(cell) = self.cell(row, col) else {
                    fill(f, r, CRUST, 0.0);
                    continue;
                };

                let on_cursor = self.cursor == (row, col);
                let bg = if on_cursor {
                    BLUE
                } else if word.contains(&(row, col)) {
                    SURFACE1
                } else {
                    SURFACE0
                };
                fill(
                    f,
                    Rect::new(
                        r.x + 1.0,
                        r.y + 1.0,
                        (r.w - 2.0).max(0.0),
                        (r.h - 2.0).max(0.0),
                    ),
                    bg,
                    l.cell * 0.06,
                );

                if cell.number > 0 {
                    let n = cell.number.to_string();
                    text_at(
                        f,
                        r.x + l.cell * 0.08,
                        r.y + l.cell * 0.05,
                        &n,
                        if on_cursor { CRUST } else { OVERLAY0 },
                        (l.cell * 0.24).max(6.0),
                        FontWeightHint::Regular,
                    );
                }

                if let Some(entry) = cell.entry {
                    let colour = if self.is_wrong(row, col) {
                        RED
                    } else if cell.revealed {
                        TEAL
                    } else if on_cursor {
                        CRUST
                    } else {
                        TEXT_COLOR
                    };
                    let size = l.cell * 0.5;
                    let s = entry.to_string();
                    // Measured, not centred by subtracting six pixels from the
                    // middle of the cell as it was before.
                    let w = text::measure(&s, size, FontWeightHint::Bold);
                    let (cx, cy) = r.centre();
                    text_at(
                        f,
                        cx - w / 2.0,
                        cy - size / 2.0,
                        &s,
                        colour,
                        size,
                        FontWeightHint::Bold,
                    );
                }

                // The box a click on this cell lands in *is* the cell that was
                // just painted, so the picture and the hit test cannot drift.
                f.hit(Target::Cell(row, col), r);
            }
        }

        f.push(RenderCommand::StrokeRect {
            x: l.grid.x,
            y: l.grid.y,
            width: l.grid.w,
            height: l.grid.h,
            color: OVERLAY0,
            line_width: (l.cell * 0.05).clamp(1.0, 4.0),
            corner_radii: CornerRadii::ZERO,
        });
    }

    /// The clue list: every clue, scrollable, and every row a hit box.
    ///
    /// The old panel drew `.take(8)` of each direction from a scroll offset
    /// that no event ever changed, so a puzzle with a ninth clue simply hid it.
    fn draw_panel(&self, f: &mut Frame<Target>, l: &Layout) {
        if l.panel.is_empty() {
            return;
        }
        let row_h = l.clue_row_h();
        let visible = l.clue_rows_visible();
        let current = self.current_clue();

        let first = self.clue_scroll.min(self.max_scroll());
        let mut heading: Option<Direction> = None;
        let mut y = l.panel.y;

        // The heading names whichever direction the first visible row is in,
        // so a list scrolled into the down clues still says so.
        if let Some(clue) = self.clues.get(first) {
            heading = Some(clue.direction);
            text_at(
                f,
                l.panel.x,
                y,
                clue.direction.label(),
                LAVENDER,
                l.small,
                FontWeightHint::Bold,
            );
        }
        y += row_h;

        for (i, clue) in self.clues.iter().enumerate().skip(first).take(visible) {
            if heading != Some(clue.direction) {
                heading = Some(clue.direction);
                text_at(
                    f,
                    l.panel.x,
                    y,
                    clue.direction.label(),
                    LAVENDER,
                    l.small,
                    FontWeightHint::Bold,
                );
                y += row_h;
                if y + row_h > l.panel.bottom() {
                    break;
                }
            }
            let r = Rect::new(l.panel.x, y, l.panel.w, row_h);
            if r.bottom() > l.panel.bottom() {
                break;
            }
            let on = current == Some(i);
            if on {
                fill(f, r, SURFACE0, l.small * 0.3);
            }
            f.push(RenderCommand::Text {
                x: r.x + l.pad * 0.3,
                y: r.y + (row_h - l.small) / 2.0,
                text: format!("{}. {}", clue.number, clue.text),
                font_size: l.small,
                color: if on { YELLOW } else { SUBTEXT0 },
                font_weight: if on {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                // The renderer is the only thing that knows how wide the text
                // it draws will be, so it is the only thing that may cut it.
                // This used to be cut at `(w / 7.0) - 3` *bytes* -- a guessed
                // advance, and a byte offset that aborts the process the first
                // time a clue holds an accented letter.
                max_width: Some((r.w - l.pad * 0.6).max(0.0)),
                overflow: TextOverflow::Ellipsis,
            });
            f.hit(Target::ClueRow(i), r);
            y += row_h;
        }
    }

    /// The footer, and the verbs in it.
    ///
    /// It used to be one line of text naming eight keystrokes, drawn in the one
    /// strip of the window that exists to be clicked.
    fn draw_footer(&self, f: &mut Frame<Target>, l: &Layout) {
        fill(f, l.footer, MANTLE, 0.0);
        if l.footer.is_empty() {
            return;
        }
        let size = (l.small * 0.95).max(7.0);
        let gap = l.pad * 0.4;
        let h = (l.footer.h - gap).max(0.0);
        let y = l.footer.y + (l.footer.h - h) / 2.0;
        let mut x = l.pad;
        for button in BUTTONS {
            let label = button.label();
            let w = text::measure(label, size, FontWeightHint::Bold) + l.pad;
            // A button that would not fit whole is left out rather than drawn
            // off the edge of the window.
            if x + w > l.footer.right() - l.pad {
                break;
            }
            let r = Rect::new(x, y, w, h);
            let on = button == Button::Check && self.check_mode
                || button == Button::Help && self.show_help;
            fill(f, r, if on { SURFACE1 } else { SURFACE0 }, h * 0.25);
            let (cx, cy) = r.centre();
            text_at(
                f,
                cx - text::measure(label, size, FontWeightHint::Bold) / 2.0,
                cy - size / 2.0,
                label,
                if on { TEXT_COLOR } else { SUBTEXT0 },
                size,
                FontWeightHint::Bold,
            );
            f.hit(Target::Button(button), r);
            x += w + gap;
        }
    }

    /// The help card, sized from the lines it holds.
    ///
    /// It was a fixed 360x280 box at `width / 2.0 - 180.0`, with its rows at
    /// literal offsets inside it, so in any window narrower than 360 the help
    /// was drawn outside the window that needed it.
    fn draw_help(&self, f: &mut Frame<Target>, l: &Layout) {
        fill(f, l.window, Color::rgba(0, 0, 0, 180), 0.0);

        let mut rows: Vec<(String, String)> = vec![
            (String::from("Arrows"), String::from("Move the cursor")),
            (String::from("A-Z"), String::from("Type a letter")),
            (String::from("Backspace"), String::from("Rub one out")),
            (String::from("Space"), String::from("Turn the cursor")),
            (String::from("Tab / Shift+Tab"), String::from("Next clue")),
        ];
        rows.extend(
            BUTTONS
                .iter()
                .map(|b| (String::from(b.key_hint()), String::from(b.label()))),
        );

        let key_w = rows.iter().fold(0.0f32, |acc, (k, _)| {
            acc.max(text::measure(k, l.small, FontWeightHint::Bold))
        });
        let desc_w = rows.iter().fold(0.0f32, |acc, (_, d)| {
            acc.max(text::measure(d, l.small, FontWeightHint::Regular))
        });
        let heading = "Help";
        let inner =
            (key_w + desc_w + l.pad).max(text::measure(heading, l.title, FontWeightHint::Bold));
        #[expect(clippy::cast_precision_loss, reason = "a dozen rows; exact in f32")]
        let rows_h = rows.len() as f32 * l.small * 1.8;
        let card_w = (inner + l.pad * 2.0).min(l.window.w);
        let card_h = (rows_h + l.title * 2.2 + l.pad).min(l.window.h);
        let card = Rect::new(
            (l.window.w - card_w) / 2.0,
            (l.window.h - card_h) / 2.0,
            card_w,
            card_h,
        );
        fill(f, card, MANTLE, l.pad * 0.6);
        centred(
            f,
            card.x,
            card.w,
            card.y + l.pad,
            heading,
            TEXT_COLOR,
            l.title,
            FontWeightHint::Bold,
        );

        let mut y = card.y + l.title * 1.8;
        for (key, desc) in &rows {
            if y + l.small > card.bottom() {
                break;
            }
            text_at(
                f,
                card.x + l.pad,
                y,
                key,
                BLUE,
                l.small,
                FontWeightHint::Bold,
            );
            text_at(
                f,
                card.x + l.pad + key_w + l.pad,
                y,
                desc,
                SUBTEXT0,
                l.small,
                FontWeightHint::Regular,
            );
            y += l.small * 1.8;
        }
        // The card itself swallows clicks so the grid behind it is not played
        // by a player reaching for a line of the help.
        f.hit(Target::Button(Button::Help), card);
    }

    /// The card shown when the puzzle is solved, sized to the window it is in.
    fn draw_completed(&self, f: &mut Frame<Target>, l: &Layout) {
        fill(f, l.window, CRUST, 0.0);
        let revealed = self.revealed_count();
        let time = format!("Time: {}", self.format_time());
        let helped = if revealed > 0 {
            format!("{revealed} letter(s) revealed")
        } else {
            String::from("Solved unaided")
        };
        let lines: [(&str, f32, Color, FontWeightHint); 5] = [
            ("Puzzle Complete!", l.title, GREEN, FontWeightHint::Bold),
            (self.puzzle_name(), l.font, TEXT_COLOR, FontWeightHint::Bold),
            (&time, l.font, TEXT_COLOR, FontWeightHint::Regular),
            (&helped, l.small, PEACH, FontWeightHint::Regular),
            (
                "Press Enter for the menu",
                l.small,
                OVERLAY0,
                FontWeightHint::Regular,
            ),
        ];

        let widest = lines.iter().fold(0.0f32, |acc, (s, size, _, weight)| {
            acc.max(text::measure(s, *size, *weight))
        });
        let text_h = lines
            .iter()
            .fold(0.0f32, |acc, (_, size, _, _)| acc + size * 1.9);
        let card_w = (widest + l.pad * 2.0).min(l.window.w);
        let card_h = (text_h + l.pad * 2.0).min(l.window.h);
        let card = Rect::new(
            (l.window.w - card_w) / 2.0,
            (l.window.h - card_h) / 2.0,
            card_w,
            card_h,
        );
        fill(f, card, SURFACE0, l.pad * 0.5);

        let mut y = card.y + l.pad;
        for (s, size, colour, weight) in lines {
            if y + size > card.bottom() {
                break;
            }
            centred(f, card.x, card.w, y, s, colour, size, weight);
            y += size * 1.9;
        }
        f.hit(Target::Button(Button::Menu), card);
    }
}

// ── Drawing helpers ─────────────────────────────────────────────────

/// A filled rectangle, skipped when there is nothing to fill.
fn fill(f: &mut Frame<Target>, r: Rect, color: Color, radius: f32) {
    if r.is_empty() {
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

/// One run of text.
fn text_at(
    f: &mut Frame<Target>,
    x: f32,
    y: f32,
    s: &str,
    color: Color,
    font_size: f32,
    font_weight: FontWeightHint,
) {
    f.push(RenderCommand::Text {
        x,
        y,
        text: String::from(s),
        color,
        font_size,
        font_weight,
        max_width: None,
        overflow: TextOverflow::Clip,
    });
}

/// A run of text centred in `[x, x + w)`, by measuring it.
///
/// Every heading in this program used to be centred by subtracting a literal:
/// `width / 2.0 - 100.0` for the menu title, `cx - 60.0` and `cx - 80.0` on the
/// end card, `bx + bw / 2.0 - 30.0` for the word "Help". Each was half of one
/// particular string at one particular size, in a program that links
/// `guitk::text`.
///
/// The weight is a parameter because the caller has to measure with the same
/// one it paints with. The end card sizes itself from a table whose rows carry
/// a weight each -- a bold heading over regular body text -- so a `centred`
/// that always measured `Bold` would centre the regular rows against a width
/// they do not have.
fn centred(
    f: &mut Frame<Target>,
    x: f32,
    w: f32,
    y: f32,
    s: &str,
    color: Color,
    size: f32,
    weight: FontWeightHint,
) {
    let measured = text::measure(s, size, weight);
    text_at(f, x + (w - measured) / 2.0, y, s, color, size, weight);
}

// ── The window ──────────────────────────────────────────────────────

impl App for Crossword {
    fn title(&self) -> String {
        String::from(TITLE)
    }

    fn app_id(&self) -> String {
        String::from("crossword")
    }

    fn initial_size(&self) -> (u32, u32) {
        (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)
    }

    fn tick_interval(&self) -> Option<std::time::Duration> {
        // Without a tick the clock in the corner never moves, which is exactly
        // what the program did before: `elapsed_secs` was set to zero on load
        // and nothing in the file ever added to it.
        Some(std::time::Duration::from_millis(TICK_MS))
    }

    fn on_event(&mut self, event: &Event) -> Response {
        if matches!(event, Event::CloseRequested) {
            return Response::Exit;
        }
        match self.handle_event(event) {
            EventResult::Consumed => Response::Redraw,
            EventResult::Ignored => Response::Idle,
        }
    }

    fn render(&mut self, width: f32, height: f32) -> RenderTree {
        self.size = (width, height);
        self.frame(width, height).into_tree()
    }
}

impl Probe for Crossword {
    type Target = Target;
    type Outcome = EventResult;
    const SIZE: (f32, f32) = (WINDOW_WIDTH, WINDOW_HEIGHT);

    fn draw(&self, size: (f32, f32)) -> Frame<Target> {
        self.frame(size.0, size.1)
    }

    fn click_at(&mut self, x: f32, y: f32, button: MouseButton, size: (f32, f32)) -> EventResult {
        self.size = size;
        self.handle_event(&Event::Mouse(MouseEvent {
            x,
            y,
            kind: MouseEventKind::Press(button),
        }))
    }

    fn key_at(&mut self, key: &KeyEvent, size: (f32, f32)) -> EventResult {
        self.size = size;
        self.handle_event(&Event::Key(key.clone()))
    }

    fn scroll_at(&mut self, x: f32, y: f32, dy: f32, size: (f32, f32)) -> Option<EventResult> {
        self.size = size;
        Some(self.handle_event(&Event::Mouse(MouseEvent {
            x,
            y,
            kind: MouseEventKind::Scroll { dx: 0.0, dy },
        })))
    }
}

fn main() -> ExitCode {
    let mut app = Crossword::new();
    app::launch("crossword", &mut app)
}
