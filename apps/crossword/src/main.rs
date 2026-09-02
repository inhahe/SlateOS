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

    /// Where the line between column `i - 1` and column `i` falls.
    ///
    /// One function for both sides of a boundary. A cell whose width was
    /// `self.cell` in its own right would end a rounding step away from where
    /// the next cell starts, because `grid.x + cell * n` and
    /// `(grid.x + cell * (n - 1)) + cell` are the same number in arithmetic and
    /// not always the same `f32`, and the sliver that leaves is a strip of the
    /// picture two cells both claim. Taking both edges from here and the width
    /// as their difference makes the boundary a single value, so there is
    /// nothing to disagree.
    #[expect(
        clippy::cast_precision_loss,
        reason = "grid dimensions are single digits; exact in f32"
    )]
    fn edge_x(&self, col: usize) -> f32 {
        self.cell.mul_add(col as f32, self.grid.x)
    }

    /// Where the line between row `i - 1` and row `i` falls. See [`edge_x`].
    ///
    /// [`edge_x`]: Layout::edge_x
    #[expect(
        clippy::cast_precision_loss,
        reason = "grid dimensions are single digits; exact in f32"
    )]
    fn edge_y(&self, row: usize) -> f32 {
        self.cell.mul_add(row as f32, self.grid.y)
    }

    /// The square a cell of the grid occupies.
    ///
    /// Cells tile and `Rect::contains` is half-open, so two of them can never
    /// both claim a pixel -- which is what makes recording the rectangle as the
    /// cell is painted a complete answer to "what did I click".
    fn cell_rect(&self, row: usize, col: usize) -> Rect {
        let x = self.edge_x(col);
        let y = self.edge_y(row);
        Rect::new(
            x,
            y,
            self.edge_x(col.saturating_add(1)) - x,
            self.edge_y(row.saturating_add(1)) - y,
        )
    }

    /// The height of one row of the clue list.
    fn clue_row_h(&self) -> f32 {
        (self.small * 1.7).max(1.0)
    }

    /// How many rows of the clue panel fit on it whole.
    ///
    /// Rows, not clues: the direction headings are rows of the same list, so
    /// the panel's capacity is measured in the units it is filled in. Counting
    /// clues and then drawing a heading among them spends a row nothing
    /// budgeted for, and the end of the list becomes unreachable.
    fn clue_rows_visible(&self) -> usize {
        if self.panel.is_empty() {
            return 0;
        }
        let usable = self.panel.h.max(0.0);
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

    /// The letter that goes after a clue number: `7A`, `7D`.
    ///
    /// The panel scrolls, so the heading that says which half of the list a
    /// row belongs to is often not on the screen with it. Two clues can share
    /// a number -- one across, one down -- and without this the two rows read
    /// the same.
    fn initial(self) -> char {
        match self {
            Self::Across => 'A',
            Self::Down => 'D',
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

/// One row of the scrolling clue panel.
///
/// A heading is a row like any other so that the scroll arithmetic and the
/// drawing agree about how much list there is. See [`Crossword::panel_rows`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PanelRow {
    /// "Across" or "Down", above the clues that go that way.
    Heading(Direction),
    /// A clue, by its index into `clues`.
    Clue(usize),
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
        // No `to_ascii_uppercase` here: `letter_of` is the only source of a
        // typed letter and it answers in the case the grid is written in, so a
        // second normalisation at the store is a rule written twice with one
        // copy nothing can reach (lesson 51). The case lives in `letter_of`,
        // and `a_letter_is_stored_upper_case_however_it_is_typed` holds it.
        let Some(cell) = self.cell_mut(self.cursor.0, self.cursor.1) else {
            return;
        };
        cell.entry = Some(ch);
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

    /// Every row of the clue panel, in the order it is drawn -- the direction
    /// headings among the clues, not beside them.
    ///
    /// The panel scrolls by rows, so a heading it draws has to be a row it
    /// counts. It used to draw "Down" in the middle of the list out of a
    /// budget that had only ever counted clues, which cost the list one row
    /// of reach: at the bottom of the scroll the last clue was pushed off the
    /// panel and no scroll position existed that would bring it back.
    fn panel_rows(&self) -> Vec<PanelRow> {
        let mut rows = Vec::with_capacity(self.clues.len().saturating_add(2));
        let mut heading: Option<Direction> = None;
        for (i, clue) in self.clues.iter().enumerate() {
            if heading != Some(clue.direction) {
                heading = Some(clue.direction);
                rows.push(PanelRow::Heading(clue.direction));
            }
            rows.push(PanelRow::Clue(i));
        }
        rows
    }

    /// Which row of the panel clue `index` is drawn on.
    fn row_of_clue(&self, index: usize) -> Option<usize> {
        self.panel_rows()
            .iter()
            .position(|row| *row == PanelRow::Clue(index))
    }

    /// Scroll the panel so that clue `index` is on it.
    fn show_clue(&mut self, index: usize) {
        let visible = self.layout().clue_rows_visible();
        let Some(row) = self.row_of_clue(index) else {
            return;
        };
        if visible == 0 {
            return;
        }
        if row < self.clue_scroll {
            // Take the heading above it along when there is one: a clue
            // scrolled to the very top of a panel whose heading is one row
            // off it is a row that does not say which list it belongs to.
            self.clue_scroll = if matches!(
                self.panel_rows().get(row.saturating_sub(1)),
                Some(PanelRow::Heading(_))
            ) {
                row.saturating_sub(1)
            } else {
                row
            };
        } else if row >= self.clue_scroll.saturating_add(visible) {
            self.clue_scroll = row.saturating_sub(visible.saturating_sub(1));
        }
    }

    /// The furthest the clue list may be scrolled, so the last row is the last
    /// clue rather than a screen of nothing.
    fn max_scroll(&self) -> usize {
        let visible = self.layout().clue_rows_visible();
        self.panel_rows().len().saturating_sub(visible)
    }

    fn scroll_clues(&mut self, rows: isize) -> bool {
        let before = self.clue_scroll;
        // The clamp is written once, at the store. It used to be here *and* in
        // the branch above, and a duplicated clamp is a clamp one of whose
        // copies no test can reach (lesson 92): deleting the outer one changed
        // nothing observable, so nothing was holding it.
        let moved = if rows >= 0 {
            self.clue_scroll.saturating_add(rows.unsigned_abs())
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

    /// Move the menu highlight, which cannot leave the list.
    ///
    /// The bound is here and nowhere else. `Key::Down` used to clamp its own
    /// argument as well, which left this one unreachable: deleting it changed
    /// nothing any test could see (lesson 92).
    fn select(&mut self, index: usize) {
        self.selected_puzzle = index.min(PUZZLES.len().saturating_sub(1));
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
                self.select(self.selected_puzzle.saturating_add(1));
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
        // The heading owns the strip above the first row, and is centred in a
        // band cut to the window rather than written at a fixed `l.pad` from
        // its top. The fixed offset was a fault, not a stylistic choice: an
        // 18pt-tall window still gets a 12.6pt title, whose line is 17.3, and
        // `4 + 17.3` is outside the window it was drawn for.
        let head = Rect::new(
            l.window.x,
            l.window.y,
            l.window.w,
            (l.pad + l.title * 2.2).min(l.window.h),
        );
        if let Some(ty) = centre_line(head, text::line_height(l.title, FontWeightHint::Bold)) {
            centred(
                f,
                head.x,
                head.w,
                ty,
                heading,
                TEXT_COLOR,
                l.title,
                FontWeightHint::Bold,
            );
        }

        let row_h = (l.title * 2.0).max(1.0);
        let mut y = head.bottom();
        for (i, def) in PUZZLES.iter().enumerate() {
            let r = Rect::new(l.pad, y, (l.window.w - l.pad * 2.0).max(0.0), row_h);
            if r.bottom() > l.window.h - l.pad {
                break;
            }
            let on = i == self.selected_puzzle;
            fill(f, r, if on { SURFACE1 } else { SURFACE0 }, l.pad * 0.4);

            // The dimensions sit at the right-hand end, measured, and the name
            // gets what is left. Both were previously free to run the whole
            // width of the row, so a long puzzle name was drawn straight
            // through the "15x15" beside it and out over the row's edge.
            let size = format!("{}x{}", def.width, def.height);
            let dims_w = text::measure(&size, l.small, FontWeightHint::Regular)
                .min((r.w - l.pad * 2.0).max(0.0));
            let dims_x = r.right() - l.pad - dims_w;
            if let Some(y) = centre_line(r, text::line_height(l.font, FontWeightHint::Bold)) {
                text_at(
                    f,
                    r.x + l.pad,
                    y,
                    def.name,
                    if on { BLUE } else { TEXT_COLOR },
                    l.font,
                    FontWeightHint::Bold,
                    dims_x - l.pad * 0.5 - (r.x + l.pad),
                );
            }
            if let Some(y) = centre_line(r, text::line_height(l.small, FontWeightHint::Regular)) {
                text_at(
                    f,
                    dims_x,
                    y,
                    &size,
                    SUBTEXT0,
                    l.small,
                    FontWeightHint::Regular,
                    dims_w,
                );
            }
            f.hit(Target::PuzzleRow(i), r);
            y += row_h + l.pad * 0.5;
        }

        // The hint's own strip, at the foot of the window. Measured as a band
        // and refused when it does not fit, rather than tested with `y +
        // l.small <= l.window.h` -- which compared the *font size* against the
        // window's height and so let a run a third of a line taller than it
        // measured through, and which asked about the wrong thing anyway: what
        // matters is whether the hint's strip clears the last row drawn above
        // it, not whether it clears the bottom of the window.
        let line = text::line_height(l.small, FontWeightHint::Regular);
        let strip = Rect::new(
            l.pad,
            l.window.h - l.pad - line,
            (l.window.w - l.pad * 2.0).max(0.0),
            line,
        );
        if strip.y >= y
            && let Some(ty) = centre_line(strip, line)
        {
            text_at(
                f,
                strip.x,
                ty,
                "Up/Down to choose, Enter to start -- or click a puzzle",
                OVERLAY0,
                l.small,
                FontWeightHint::Regular,
                strip.w,
            );
        }
    }

    fn draw_header(&self, f: &mut Frame<Target>, l: &Layout) {
        fill(f, l.header, MANTLE, 0.0);
        if l.header.is_empty() {
            return;
        }
        // The readouts are laid out from the right edge of the window they are
        // in. They used to sit at `width - 100.0` and `width - 220.0`, which
        // are widths used as coordinates: in a 200-pixel window both were off
        // the left edge.
        //
        // They are placed before the name, not after it, because the name is
        // the thing that has to give way. A puzzle name is arbitrary text and
        // the readouts are not, so the readouts take what they measure and the
        // name is cut to whatever is left -- which is the only ordering that
        // cannot end with a name drawn through a clock.
        let (filled, total) = self.filled_count();
        let readouts = [
            (self.format_time(), TEXT_COLOR),
            (format!("{filled}/{total}"), SUBTEXT0),
        ];
        let mut x = l.header.right() - l.pad;
        for (s, colour) in readouts {
            let w = text::measure(&s, l.font, FontWeightHint::Regular);
            x -= w;
            if x < l.pad {
                x += w;
                break;
            }
            if let Some(y) =
                centre_line(l.header, text::line_height(l.font, FontWeightHint::Regular))
            {
                text_at(f, x, y, &s, colour, l.font, FontWeightHint::Regular, w);
            }
            x -= l.pad;
        }

        if let Some(y) = centre_line(l.header, text::line_height(l.title, FontWeightHint::Bold)) {
            text_at(
                f,
                l.pad,
                y,
                self.puzzle_name(),
                TEXT_COLOR,
                l.title,
                FontWeightHint::Bold,
                x - l.pad * 0.5 - l.pad,
            );
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
        // straight off the right-hand edge. The vertical bound is newer: the
        // old `(banner.h - l.font) / 2.0` centred the *font size* in the band,
        // and a run is `line_height` tall, which is a third as much again, so
        // the strip overflowed top and bottom by a sixth of a line whenever
        // the band was tight. `centre_line` centres the run and refuses when
        // the run does not fit.
        let line = text::line_height(l.font, FontWeightHint::Regular);
        if let Some(y) = centre_line(l.banner, line) {
            text_at(
                f,
                l.pad,
                y,
                &s,
                YELLOW,
                l.font,
                FontWeightHint::Regular,
                (l.banner.w - l.pad * 2.0).max(0.0),
            );
        }
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
                    // The number sits in the cell's top-left corner rather
                    // than in the middle of it, so it gets a corner band of
                    // its own instead of `centre_line`. The band matters
                    // because the size has a 6pt *floor*: below a cell of
                    // about 8pt the floor is taller than the corner it is
                    // being written into, and a floor is not a bound.
                    let size = (l.cell * 0.24).max(6.0);
                    let corner = Rect::new(
                        r.x + l.cell * 0.08,
                        r.y + l.cell * 0.05,
                        (r.w - l.cell * 0.16).max(0.0),
                        (r.h - l.cell * 0.05).max(0.0),
                    );
                    if text::line_height(size, FontWeightHint::Regular) <= corner.h {
                        text_at(
                            f,
                            corner.x,
                            corner.y,
                            &n,
                            if on_cursor { CRUST } else { OVERLAY0 },
                            size,
                            FontWeightHint::Regular,
                            corner.w,
                        );
                    }
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
                    // middle of the cell as it was before -- and vertically
                    // centred on the height of the *line*, not on the font
                    // size, which is the smaller number and left every letter
                    // sitting low in its cell.
                    if let Some(y) = centre_line(r, text::line_height(size, FontWeightHint::Bold)) {
                        centred(f, r.x, r.w, y, &s, colour, size, FontWeightHint::Bold);
                    }
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

        // One list, one budget. The headings used to be drawn from a second
        // copy of this code that ran before the loop, and again inside it when
        // the direction changed, out of a row count that had never included
        // them.
        let rows = self.panel_rows();
        let first = self.clue_scroll.min(self.max_scroll());
        let mut y = l.panel.y;

        for row in rows.iter().skip(first).take(visible) {
            let r = Rect::new(l.panel.x, y, l.panel.w, row_h);
            if r.bottom() > l.panel.bottom() {
                break;
            }
            y += row_h;
            let index = match *row {
                PanelRow::Heading(dir) => {
                    // A heading is a row like any other and is centred in it.
                    // It used to be written at `r.y`, which is the row's top
                    // edge, so it sat proud of its row by however much the row
                    // was taller than the line -- and hung out of the panel
                    // entirely when the row was shorter.
                    if let Some(ty) =
                        centre_line(r, text::line_height(l.small, FontWeightHint::Bold))
                    {
                        text_at(
                            f,
                            r.x,
                            ty,
                            dir.label(),
                            LAVENDER,
                            l.small,
                            FontWeightHint::Bold,
                            r.w,
                        );
                    }
                    continue;
                }
                PanelRow::Clue(index) => index,
            };
            let Some(clue) = self.clues.get(index) else {
                continue;
            };
            let on = current == Some(index);
            if on {
                fill(f, r, SURFACE0, l.small * 0.3);
            }
            let weight = if on {
                FontWeightHint::Bold
            } else {
                FontWeightHint::Regular
            };
            // Centred on the line, not on the font size: `(row_h - l.small)`
            // is a third of a line too much room, and the difference put every
            // clue a sixth of a line below where it belonged -- and out of the
            // bottom of the panel on the last row of a tight one.
            if let Some(ty) = centre_line(r, text::line_height(l.small, weight)) {
                text_at(
                    f,
                    r.x + l.pad * 0.3,
                    ty,
                    // The direction goes in the label because the heading that
                    // would otherwise say it scrolls away: 7 Across and 7 Down
                    // are both "7", and a panel showing one of them without
                    // its heading would not say which.
                    &format!("{}{}. {}", clue.number, clue.direction.initial(), clue.text),
                    if on { YELLOW } else { SUBTEXT0 },
                    l.small,
                    weight,
                    // The renderer is the only thing that knows how wide the
                    // text it draws will be, so it is the only thing that may
                    // cut it. This used to be cut at `(w / 7.0) - 3` *bytes*
                    // -- a guessed advance, and a byte offset that aborts the
                    // process the first time a clue holds an accented letter.
                    (r.w - l.pad * 0.6).max(0.0),
                );
            }
            f.hit(Target::ClueRow(index), r);
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
        // The button strip is the footer less a gap, centred in it. Written
        // through `centre_line` rather than by hand so that the one place in
        // this file that turns a band and a height into a top edge is the one
        // place that has to be right; `h` is `footer.h` minus a non-negative
        // number, so the refusal cannot fire here, and the `let else` says so
        // rather than pretending the arithmetic is unconditional.
        let h = (l.footer.h - gap).max(0.0);
        let Some(y) = centre_line(l.footer, h) else {
            return;
        };
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
            // `cy - size / 2.0` centred the font size in the button and left
            // the label low in it by a sixth of a line; a 7pt floor on `size`
            // in a footer that can be shorter than 9pt put it out of the
            // window's bottom edge altogether. The label is dropped rather
            // than drawn outside the button that is meant to contain it.
            if let Some(ty) = centre_line(r, text::line_height(size, FontWeightHint::Bold)) {
                centred(
                    f,
                    r.x,
                    r.w,
                    ty,
                    label,
                    if on { TEXT_COLOR } else { SUBTEXT0 },
                    size,
                    FontWeightHint::Bold,
                );
            }
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
        // The heading owns the band the rows begin after, and is centred in
        // it rather than written at `card.y + l.pad`. In a window shorter than
        // the card's natural height the card is cut to the window, and the
        // fixed `l.pad` offset then put the heading below the card's own
        // bottom edge; a band that is cut with the card cannot do that.
        let head = Rect::new(card.x, card.y, card.w, (l.title * 1.8).min(card.h));
        if let Some(ty) = centre_line(head, text::line_height(l.title, FontWeightHint::Bold)) {
            centred(
                f,
                card.x,
                card.w,
                ty,
                heading,
                TEXT_COLOR,
                l.title,
                FontWeightHint::Bold,
            );
        }

        // Two columns, each with a width of its own. The key column is the
        // widest key; the description gets what is left of the card. Both were
        // unbounded, and in a card cut down to a narrow window both ran out of
        // the right-hand edge of the thing that is meant to contain them.
        let key_x = card.x + l.pad;
        let desc_x = key_x + key_w + l.pad;
        let room = |x: f32| (card.right() - l.pad - x).max(0.0);
        let key_limit = key_w.min(room(key_x));
        let desc_limit = room(desc_x);
        let step = l.small * 1.8;
        // The taller of the two faces, because both are drawn on this line and
        // a bound that holds for one of them is not a bound.
        let line = text::line_height(l.small, FontWeightHint::Bold)
            .max(text::line_height(l.small, FontWeightHint::Regular));
        let mut y = head.bottom();
        for (key, desc) in &rows {
            // `y + l.small` measured the *font size* against the card's floor,
            // which is a third of a line short: the last row that this let
            // through was drawn a sixth of a line below the card.
            if y + line > card.bottom() {
                break;
            }
            text_at(
                f,
                key_x,
                y,
                key,
                BLUE,
                l.small,
                FontWeightHint::Bold,
                key_limit,
            );
            text_at(
                f,
                desc_x,
                y,
                desc,
                SUBTEXT0,
                l.small,
                FontWeightHint::Regular,
                desc_limit,
            );
            y += step;
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
            // Against the height of the line, not the font size. `y + size`
            // let through a row whose descender was already past the card's
            // bottom edge, which is what the card is for.
            if y + text::line_height(size, weight) > card.bottom() {
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

/// Where a run of `size` points starts if it is to sit centred in `band`, or
/// `None` when the band is too short to hold one.
///
/// The shape this replaces is `band.y + (band.h - size) / 2.0`, written twelve
/// times across this file. It reads like a bound and is not one: it is an
/// *offset*, and when the band is shorter than the run the slack goes negative
/// and the run is placed above the band's top, spilling out of both ends at
/// once. Nothing in the resulting picture says which band the fault is in --
/// the run is simply somewhere it does not belong, overlapping whatever is
/// above and below.
///
/// `size` is a **line height**, not a font size, and the distinction is the
/// whole point of the function. Every one of the twelve sites passed a font
/// size, because that is the number in scope; but the renderer places a run by
/// its top-left corner and the run then occupies a full line height, which for
/// this face is 1.33 times the font size. A band sized to hold exactly one line
/// therefore appeared to have a third of a line spare, and every "centred" run
/// in the program sat about a sixth of a line low -- consistently enough to
/// look like a design choice rather than a bug.
///
/// Returning `Option` rather than clamping is deliberate. A run that will not
/// fit is not improved by being moved to the top of a band it overflows; the
/// caller has to decide, and every caller here decides not to draw it. See the
/// `C-CENTRING-IS-NOT-A-BOUND` campaign in `known-issues.md`.
fn centre_line(band: Rect, size: f32) -> Option<f32> {
    if band.is_empty() || size <= 0.0 || size > band.h {
        return None;
    }
    Some(band.y + (band.h - size) / 2.0)
}

/// One run of text, cut to `limit`.
///
/// The limit is not optional, and that is the fix. This function used to push
/// `max_width: None` with `overflow: Clip`, which together mean "draw the whole
/// string wherever it reaches" -- so every label, name, heading and hint in the
/// program was free to run past the right-hand edge of whatever it was in, and
/// several did. `None` is not a wide limit, it is the *absence* of one, and a
/// renderer given no limit has nothing to cut against; the `Clip` beside it
/// looked like a bound and clipped to nothing at all.
///
/// Making the limit a required argument rather than defaulting it means a new
/// call site cannot be written without answering the question, which is the
/// only version of this that stays fixed.
fn text_at(
    f: &mut Frame<Target>,
    x: f32,
    y: f32,
    s: &str,
    color: Color,
    font_size: f32,
    font_weight: FontWeightHint,
    limit: f32,
) {
    if s.is_empty() || font_size <= 0.0 || limit <= 0.0 {
        return;
    }
    f.push(RenderCommand::Text {
        x,
        y,
        text: String::from(s),
        color,
        font_size,
        font_weight,
        max_width: Some(limit),
        overflow: TextOverflow::Ellipsis,
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
/// The limit is measured from where the run *starts*, not from `x`. A centred
/// run is already inset by half the slack, so handing it the box's full width
/// would let it end half the slack past the box's right edge -- and the wider
/// the string, the further past, which is the opposite of what a limit is for.
/// The start is also floored at `x`, so a string wider than its box begins at
/// the left edge and is ellipsised rather than being centred into a negative
/// inset and hanging off both sides.
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
    let start = (x + (w - measured) / 2.0).max(x);
    text_at(f, start, y, s, color, size, weight, x + w - start);
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

// ═══════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════
#[cfg(test)]
mod tests {
    // A test that indexes out of range should fail loudly and point at the
    // line that did it -- that is the diagnosis. The defensive lints exist to
    // keep panics out of code that runs on a user's data, which this is not.
    #![expect(
        clippy::indexing_slicing,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::float_cmp,
        reason = "test code: a panic is a diagnosis"
    )]

    use super::*;
    use guitk::probe::{click_sized, ctrl, is_visible_sized, press, rect_of_sized, shift};
    use std::collections::BTreeMap;

    // ── Fixtures ───────────────────────────────────────────────────

    /// The windows every geometric claim is checked at.
    ///
    /// The grid is square, so the *shorter* side is the one that pays for it,
    /// and a list of near-4:3 windows would let width and height take turns
    /// being the binding constraint without ever making that visible. This
    /// list spans both orders, both extremes and the degenerate cases: wider
    /// than tall, taller than wide, exactly square, one so short the three
    /// bars have already spent the height, and one so narrow the clue panel
    /// cannot exist at all.
    /// The last four are the slivers, and they are here for a reason the
    /// others cannot cover. `pad` follows the window's *shorter* side and the
    /// type sizes follow its *height*, so a tall narrow window drives them
    /// apart -- and the bands are all `type + k * pad` tall. 133x600 is the
    /// case where the header ends up shorter than the title it carries; the
    /// three around it are its neighbours on both sides of that boundary, so a
    /// change that moves the boundary shows up as a failure rather than as a
    /// probe that quietly stops probing.
    const SIZES: [(f32, f32); 18] = [
        (860.0, 580.0),
        (320.0, 240.0),
        (240.0, 320.0),
        (400.0, 400.0),
        (1280.0, 800.0),
        (800.0, 1280.0),
        (1920.0, 1080.0),
        (200.0, 900.0),
        (900.0, 200.0),
        (140.0, 140.0),
        (900.0, 60.0),
        (60.0, 900.0),
        (133.0, 600.0),
        (160.0, 900.0),
        (110.0, 1000.0),
        (900.0, 26.0),
        (900.0, 18.0),
        (900.0, 14.0),
    ];

    /// An app with puzzle `index` open, driven to the size a probe uses.
    fn playing(index: usize) -> Crossword {
        let mut app = Crossword::new();
        app.load_puzzle(index);
        assert_eq!(
            app.view,
            View::Playing,
            "the fixture failed to open puzzle {index}"
        );
        app
    }

    /// The answers each grid is supposed to spell, in grid order.
    ///
    /// This table is the test that the grids are crosswords. The three that
    /// were here before were built from their across answers alone, and their
    /// *columns* spelled `SAPETRE`, `HALLILA` and `FSIVRNA` -- nobody had ever
    /// read them downwards, because nothing ever asked. Writing the intended
    /// words down here means a grid that stops spelling them says so.
    const ANSWERS: [(&str, &[&str], &[&str]); 3] = [
        (
            "Warm Up",
            &["SPA", "TAR", "ELITE", "SEE", "END"],
            &["STEAK", "PAL", "ARISE", "SPEED", "TEN"],
        ),
        (
            "Centre Stage",
            &["COG", "EAR", "DRAMA", "CAT", "EYE"],
            &["CEDAR", "OAR", "GRACE", "PLATE", "MAY"],
        ),
        (
            "Around the House",
            &["BOW", "AIR", "SLIDE", "SUM", "TOE"],
            &["BASIN", "OIL", "WRIST", "THEME", "DUO"],
        ),
    ];

    /// The letters of the word a clue points at.
    fn answer_of(app: &Crossword, clue: &Clue) -> String {
        app.word_cells(clue.row, clue.col, clue.direction)
            .into_iter()
            .map(|(r, c)| app.cell(r, c).unwrap().solution)
            .collect()
    }

    /// Every rectangle the frame filled.
    fn painted_rects(app: &Crossword, size: (f32, f32)) -> Vec<Rect> {
        app.draw(size)
            .commands()
            .iter()
            .filter_map(|c| match *c {
                RenderCommand::FillRect {
                    x,
                    y,
                    width,
                    height,
                    ..
                } => Some(Rect::new(x, y, width, height)),
                _ => None,
            })
            .collect()
    }

    /// Every run of text the frame painted, with the width it was bounded to.
    /// Every screen the app can be on, at one window size.
    ///
    /// The whole-frame properties -- nothing outside the window, nothing with
    /// no area -- have to hold on all of them, and building the list twice is
    /// how one of them ends up tested on three screens and not the fourth.
    fn every_screen(size: (f32, f32)) -> Vec<(&'static str, Crossword)> {
        let mut menu = Crossword::new();
        menu.size = size;
        let mut game = playing(0);
        game.size = size;
        let mut helped = playing(1);
        helped.size = size;
        helped.show_help = true;
        let mut done = playing(2);
        done.size = size;
        done.view = View::Completed;
        vec![
            ("menu", menu),
            ("playing", game),
            ("help", helped),
            ("completed", done),
        ]
    }

    fn painted_text(app: &Crossword, size: (f32, f32)) -> Vec<(String, Option<f32>)> {
        app.draw(size)
            .commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text {
                    text, max_width, ..
                } => Some((text.clone(), *max_width)),
                _ => None,
            })
            .collect()
    }

    /// A shape of the app's state that two routes to the same action must
    /// agree on, so "the button and the key do the same thing" is one
    /// comparison rather than a list of fields somebody will forget to extend.
    #[derive(Debug, PartialEq, Eq)]
    struct Snapshot {
        view: View,
        check_mode: bool,
        show_help: bool,
        cursor: (usize, usize),
        entries: Vec<Option<char>>,
        revealed: usize,
    }

    fn snapshot(app: &Crossword) -> Snapshot {
        Snapshot {
            view: app.view,
            check_mode: app.check_mode,
            show_help: app.show_help,
            cursor: app.cursor,
            entries: app
                .cells
                .iter()
                .map(|c| c.as_ref().and_then(|c| c.entry))
                .collect(),
            revealed: app.revealed_count(),
        }
    }

    // ── The layout follows the window ──────────────────────────────

    #[test]
    fn the_layout_follows_the_window_rather_than_a_constant() {
        // Both windows have to sit in the band where the quantity compared is
        // free to move, or the comparison is between two *clamped* values and
        // passes against a program that ignores the window entirely. `font` is
        // clamped to 9 below 342px tall and to 17 above 646, and `pad` to 4
        // below 134px and to 18 above 600 on the shorter side.
        let small = Layout::solve(400.0, 400.0, 5, 5);
        let large = Layout::solve(560.0, 560.0, 5, 5);

        assert!(
            large.cell > small.cell,
            "a bigger window has to draw a bigger cell: {} vs {}",
            large.cell,
            small.cell
        );
        assert!(
            large.font > small.font,
            "a bigger window has to use a bigger font: {} vs {}",
            large.font,
            small.font
        );
        assert!(
            large.pad > small.pad,
            "a bigger window has to leave a bigger margin: {} vs {}",
            large.pad,
            small.pad
        );
        assert!(
            large.header.h > small.header.h,
            "the header is a function of the window, not a constant"
        );
    }

    #[test]
    fn a_panel_too_narrow_to_read_is_left_out_rather_than_drawn_narrow() {
        // The rule only governs windows that leave a *usable-looking but
        // useless* gap beside the grid, and 220x700 is not one of them: there
        // the grid takes the whole body and the gap is zero, so dropping the
        // panel and keeping it look identical. A gap in the band appears when
        // the grid is limited by the window's height instead of its width --
        // widening the grid then does not consume the gap, and whatever is
        // left over is left over. That is the regime this scans for, and
        // `entered` is the guard that says the scan reached it (lesson 90).
        let mut entered = false;
        let mut w = 200.0_f32;
        while w <= 900.0 {
            let mut h = 160.0_f32;
            while h <= 700.0 {
                let l = Layout::solve(w, h, 5, 5);
                assert!(
                    l.panel.is_empty() || l.panel.w >= MIN_PANEL_WIDTH,
                    "{w}x{h}: a {}-pixel clue panel holds no readable clue",
                    l.panel.w
                );
                if l.panel.is_empty() && l.window.right() - l.pad - l.grid.right() > 1.0 {
                    entered = true;
                }
                h += 10.0;
            }
            w += 10.0;
        }
        assert!(
            entered,
            "no window in the scan leaves a gap too narrow to hold a panel, \
             so the scan says nothing about what happens when one does"
        );
    }

    #[test]
    fn the_grid_fills_the_body_when_the_window_has_no_room_for_a_panel() {
        // The old grid was 36 pixels a cell whatever the window was: a 5x5
        // puzzle occupied 180 pixels of an 1920-pixel window and 180 of a
        // 140-pixel one, which is to say it was drawn outside the second.
        let wide = Layout::solve(1200.0, 700.0, 5, 5);
        assert!(
            !wide.panel.is_empty(),
            "a 1200x700 window has room for a clue panel"
        );

        let narrow = Layout::solve(220.0, 700.0, 5, 5);
        assert!(
            narrow.panel.is_empty(),
            "a 220-pixel window cannot hold a readable clue panel, so it \
             should not pretend to: {:?}",
            narrow.panel
        );
        assert!(
            narrow.grid.w > wide.grid.w * 0.0,
            "the grid has some width in a narrow window"
        );
        // Handing the panel's share back to the grid is the point of dropping
        // it, so the grid must be wider than the share it would have had.
        let shared = narrow.grid.w;
        let reserved = (220.0 - narrow.pad * 2.0) * GRID_WIDTH_SHARE;
        assert!(
            shared > reserved,
            "dropping the panel should give its width to the grid: \
             grid {shared} vs reserved share {reserved}"
        );
    }

    #[test]
    fn a_cell_stops_growing_before_the_grid_swallows_the_monitor() {
        // A 5x5 puzzle on a 1080-line screen has room for a 180-pixel cell,
        // which is a crossword the size of a dinner plate. The cap is what
        // keeps a small puzzle looking like one.
        let big = Layout::solve(1920.0, 1080.0, 5, 5);
        assert!(big.cell <= MAX_CELL, "a cell grew to {}", big.cell);
        assert!(big.cell > 0.0, "and it still has to be drawn");
        // The cap must not be what decides every window, or this test is a
        // statement about a constant rather than about the layout.
        let ordinary = Layout::solve(400.0, 400.0, 5, 5);
        assert!(
            ordinary.cell < MAX_CELL,
            "the cap should not bind at 400x400: {}",
            ordinary.cell
        );
    }

    #[test]
    fn the_grid_is_square_and_its_cells_tile_it_exactly() {
        for (w, h) in SIZES {
            let l = Layout::solve(w, h, 5, 5);
            assert_eq!(l.grid.w, l.cell * 5.0, "grid width at {w}x{h}");
            assert_eq!(l.grid.h, l.cell * 5.0, "grid height at {w}x{h}");
            for row in 0..5 {
                for col in 0..5 {
                    let r = l.cell_rect(row, col);
                    // Not `== l.cell`: the width is the distance between two
                    // boundaries, and a boundary is what has to agree, not a
                    // width computed twice. It is the next two assertions that
                    // are exact.
                    assert!(
                        (r.w - l.cell).abs() < 0.01 && (r.h - l.cell).abs() < 0.01,
                        "cell {row},{col} is {r:?}, not about {} square, at {w}x{h}",
                        l.cell
                    );
                    if col > 0 {
                        assert_eq!(
                            l.cell_rect(row, col - 1).right(),
                            r.x,
                            "cell {row},{col} and its left neighbour must touch exactly at {w}x{h}"
                        );
                    }
                    if row > 0 {
                        assert_eq!(
                            l.cell_rect(row - 1, col).bottom(),
                            r.y,
                            "cell {row},{col} and the one above must touch exactly at {w}x{h}"
                        );
                    }
                }
            }
            let last = l.cell_rect(4, 4);
            assert!(
                (last.right() - l.grid.right()).abs() < 0.01
                    && (last.bottom() - l.grid.bottom()).abs() < 0.01,
                "the last cell has to end where the grid does at {w}x{h}: {last:?} in {:?}",
                l.grid
            );
        }
    }

    // ── Every pass stays inside the band it owns ───────────────────

    /// Boxes the layout would never hand a pass, so that a bound is checked
    /// where it is meant to bite rather than only where it was never going to.
    ///
    /// Both axes: the vertical squeezes are what `centre_line` is for, and the
    /// horizontal ones are what the `limit` argument to `text_at` is for. A run
    /// bounded on one axis and not the other leaves the band by the other one.
    fn squeezes(r: Rect) -> Vec<Rect> {
        let mut out = vec![r];
        let mut push_h = |h: f32| {
            if h < r.h {
                out.push(Rect::new(r.x, r.y, r.w, h));
            }
        };
        for h in [0.0, 1.0, 3.0, 7.0, 13.0, 26.0] {
            push_h(h);
        }
        for k in 1..16_u8 {
            push_h(r.h * f32::from(k) / 16.0);
        }
        let mut push_w = |w: f32| {
            if w < r.w {
                out.push(Rect::new(r.x, r.y, w, r.h));
            }
        };
        for w in [0.0, 1.0, 5.0, 30.0, 120.0] {
            push_w(w);
        }
        for k in 1..16_u8 {
            push_w(r.w * f32::from(k) / 16.0);
        }
        out
    }

    type Pass = fn(&Crossword, &mut Frame<Target>, &Layout);
    type Band = fn(&mut Layout) -> &mut Rect;
    type Region = fn(&Layout) -> Rect;

    /// Every drawing pass and the region it is entitled to paint in.
    ///
    /// `menu`, `help` and `completed` own the **window**: each of the three
    /// covers the screen -- the menu is a screen in its own right, and the
    /// other two scrim what is behind them before drawing a card, so the window
    /// really is the region they paint. That means this test says nothing about
    /// the help card's or the end card's own box, which is what
    /// `every_run_a_card_draws_stays_inside_the_card` below is for.
    const PASSES: [(&str, Pass, Region); 7] = [
        ("menu", Crossword::draw_menu, |l| l.window),
        ("header", Crossword::draw_header, |l| l.header),
        ("banner", Crossword::draw_banner, |l| l.banner),
        ("grid", Crossword::draw_grid, |l| l.grid),
        ("panel", Crossword::draw_panel, |l| l.panel),
        ("footer", Crossword::draw_footer, |l| l.footer),
        ("help", Crossword::draw_help, |l| l.window),
        // `draw_completed` is absent on purpose: it is `View::Completed`'s
        // whole screen and is covered by the `window` sweep in
        // `nothing_is_painted_outside_the_window`, and by the card test below.
    ];

    /// The bands a test may replace with one the layout would not have given.
    ///
    /// The **grid** is not among them, and that is not an oversight. Every cell
    /// is placed from `l.cell` rather than from `l.grid.h`, so a grid squeezed
    /// on its own is a grid the cells were never solved for: the overrun would
    /// be the test's own doing and would say nothing about the drawing. The
    /// grid's containment is checked from real layouts, where `grid.h` and
    /// `cell` agree because `Layout::solve` derived them together.
    const SQUEEZABLE: [(&str, Band, Pass); 4] = [
        ("header", |l| &mut l.header, Crossword::draw_header),
        ("banner", |l| &mut l.banner, Crossword::draw_banner),
        ("panel", |l| &mut l.panel, Crossword::draw_panel),
        ("footer", |l| &mut l.footer, Crossword::draw_footer),
    ];

    /// Every run of type, as the box the renderer may fill with it.
    ///
    /// The height is `text::line_height`, not the font size: a run's *top-left*
    /// corner is what is placed, so it occupies a whole line below `y`. The
    /// difference is a third of the size, and measuring runs by their font size
    /// is precisely the mistake this app's twelve centrings all made -- a test
    /// that repeated it would pass against the code that has it.
    fn inked(f: &Frame<Target>) -> Vec<(String, Rect)> {
        f.commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text {
                    text,
                    x,
                    y,
                    max_width,
                    font_size,
                    font_weight,
                    ..
                } => {
                    let w =
                        max_width.unwrap_or_else(|| text::measure(text, *font_size, *font_weight));
                    Some((
                        text.clone(),
                        Rect::new(*x, *y, w, text::line_height(*font_size, *font_weight)),
                    ))
                }
                _ => None,
            })
            .collect()
    }

    /// Every rectangle a frame filled.
    fn filled(f: &Frame<Target>) -> Vec<Rect> {
        f.commands()
            .iter()
            .filter_map(|c| match *c {
                RenderCommand::FillRect {
                    x,
                    y,
                    width,
                    height,
                    ..
                } => Some(Rect::new(x, y, width, height)),
                _ => None,
            })
            .collect()
    }

    /// A box with no area is inside anything: it is a thing that was not drawn.
    fn inside(outer: Rect, inner: Rect) -> bool {
        inner.is_empty()
            || (inner.x >= outer.x - 0.01
                && inner.y >= outer.y - 0.01
                && inner.right() <= outer.right() + 0.01
                && inner.bottom() <= outer.bottom() + 0.01)
    }

    fn check_containment(state: &str, pass: &str, region: Rect, f: &Frame<Target>) {
        for r in filled(f) {
            assert!(
                inside(region, r),
                "{state}: the {pass} pass, given {region:?}, painted {r:?}"
            );
        }
        for (s, r) in inked(f) {
            assert!(
                inside(region, r),
                "{state}: the {pass} pass, given {region:?}, inked {s:?} at {r:?}"
            );
        }
        // The containment assertion above cannot see this one, and that is not
        // a gap to be argued away: `inked` gives a run bounded to zero a box of
        // no width, and a box of no area is inside everything. So a run placed
        // where there is no room passes containment precisely *because* it has
        // nowhere to go. What is wrong with it is the same thing that is wrong
        // with a fill of no area -- it is a command the renderer is entitled to
        // treat however it likes, and an `Ellipsis` into zero width may well be
        // an ellipsis drawn outside the band. `text_at` refuses it; this is
        // where that refusal is checked, on every screen and every squeeze.
        for c in f.commands() {
            if let RenderCommand::Text {
                text,
                max_width: Some(limit),
                ..
            } = c
            {
                assert!(
                    *limit > 0.0,
                    "{state}: the {pass} pass gave {text:?} a width of {limit} \
                     to fit in, which is no room at all"
                );
            }
        }
        for (target, rect) in f.hits() {
            assert!(
                inside(region, *rect),
                "{state}: the {pass} pass, given {region:?}, hit-boxed {target:?} at {rect:?}"
            );
        }
    }

    #[test]
    fn centre_line_refuses_a_band_it_cannot_fill_rather_than_going_negative() {
        let band = Rect::new(10.0, 100.0, 80.0, 20.0);
        assert_eq!(
            centre_line(band, 20.0),
            Some(100.0),
            "a run exactly as tall as its band sits at the band's top"
        );
        assert_eq!(
            centre_line(band, 10.0),
            Some(105.0),
            "and one half as tall sits half the slack down"
        );
        assert_eq!(
            centre_line(band, 20.001),
            None,
            "a run taller than its band has nowhere to go, and the answer is \
             not a negative offset above the band's top edge"
        );
        assert_eq!(
            centre_line(band, 0.0),
            None,
            "a run of no height is not a run"
        );
        assert_eq!(
            centre_line(Rect::EMPTY, 1.0),
            None,
            "a band with no area holds nothing"
        );
        assert_eq!(
            centre_line(Rect::new(0.0, 0.0, 0.0, 50.0), 10.0),
            None,
            "and a band with height but no width is still a band with no area"
        );
    }

    #[test]
    fn text_at_refuses_a_run_with_no_room_to_draw_it() {
        // Both halves, because a refusal that refuses everything is not a
        // refusal. The three inputs below are the three ways there is nothing
        // to draw: no text, no size, no width. Each must produce no command;
        // the fourth case must produce exactly one.
        for (why, s, size, limit) in [
            ("no text", "", 12.0, 100.0),
            ("no size", "Across", 0.0, 100.0),
            ("no width", "Across", 12.0, 0.0),
            ("a negative width", "Across", 12.0, -40.0),
        ] {
            let mut f: Frame<Target> = Frame::new(200.0, 200.0);
            text_at(
                &mut f,
                10.0,
                10.0,
                s,
                TEXT_COLOR,
                size,
                FontWeightHint::Regular,
                limit,
            );
            assert!(
                f.commands().is_empty(),
                "a run with {why} must not reach the renderer, yet {:?} did",
                f.commands()
            );
        }
        let mut f: Frame<Target> = Frame::new(200.0, 200.0);
        text_at(
            &mut f,
            10.0,
            10.0,
            "Across",
            TEXT_COLOR,
            12.0,
            FontWeightHint::Regular,
            100.0,
        );
        assert!(
            matches!(
                f.commands(),
                [RenderCommand::Text {
                    max_width: Some(100.0),
                    overflow: TextOverflow::Ellipsis,
                    ..
                }]
            ),
            "a run with room in it is one bounded command, not {:?}",
            f.commands()
        );
    }

    #[test]
    fn a_run_in_a_band_the_drawing_filled_is_centred_in_it_and_not_merely_inside_it() {
        // Containment is a *weaker* property than centring, and this app has
        // bands with slack in them. A clue row is `small * 1.7` tall and a line
        // is about `small * 1.33`, so a run centred on the font size instead of
        // on the line still lands inside its row -- a sixth of a line low, and
        // invisible to every containment sweep above, because nothing has left
        // anything. The same holds for a footer button. Where the drawing fills
        // a box and then writes in it, the fill *is* the band, and the run and
        // the fill must share a centre line; that is the assertion containment
        // cannot make.
        let mut checked = 0_u32;
        let mut per: BTreeMap<&str, u32> = BTreeMap::new();
        for (w, h) in SIZES {
            let mut app = playing(0);
            app.size = (w, h);
            app.check_mode = true;
            // A freshly-opened puzzle has no letters in it, and a grid with no
            // letters draws no runs a cell can be off-centre in -- so without
            // this the grid pass contributed nothing and the test said so only
            // through the counter. Revealing fills every cell of the word the
            // cursor is in, which is the state a player is in most of the time
            // anyway.
            app.reveal_word();
            let l = Layout::solve(w, h, app.width, app.height);
            for (name, pass) in [
                ("panel", Crossword::draw_panel as Pass),
                ("footer", Crossword::draw_footer as Pass),
                ("grid", Crossword::draw_grid as Pass),
            ] {
                let mut f = Frame::new(w, h);
                pass(&app, &mut f, &l);
                for band in filled(&f) {
                    for (s, r) in inked(&f) {
                        // Paired by the run's *centre point*, not by whole
                        // containment. A cell's fill is its box inset by a
                        // pixel, so the letter -- bounded to the cell -- is a
                        // hair wider than the fill and no containment test
                        // pairs the two. An inset does not move a centre,
                        // which is the thing being compared, so pairing by the
                        // centre asks the question the band was drawn to
                        // answer.
                        let (rx, ry) = r.centre();
                        if rx < band.x || rx > band.right() || ry < band.y || ry > band.bottom() {
                            continue;
                        }
                        // A cell's *number* is the one run in this app that is
                        // deliberately not centred: it sits in the top-left
                        // corner of its cell, which is where a crossword puts
                        // its numbers. It has a corner band of its own and the
                        // containment sweep checks it there; skipping it here
                        // is not excusing it. Digits tell it from the letter,
                        // which is a single capital.
                        if s.chars().all(|c| c.is_ascii_digit()) {
                            continue;
                        }
                        let (_, by) = band.centre();
                        assert!(
                            (ry - by).abs() < 0.02,
                            "{name} at {w}x{h}: {s:?} is centred on {ry} in a band                              centred on {by} ({band:?}); a run centred on its font                              size rather than on its line lands exactly here --                              inside the band, and off its middle"
                        );
                        checked += 1;
                        *per.entry(name).or_insert(0_u32) += 1;
                    }
                }
            }
        }
        assert!(
            checked > 40,
            "only {checked} runs were paired with the box that was filled for \
             them; this test says nothing about a pass whose fill it never found"
        );
        // Per pass, not just in total. A fixture change that stops one of the
        // three producing runs at all -- an unrevealed grid draws no letters,
        // which is exactly how this test first passed while saying nothing
        // about a cell -- would otherwise hide behind the other two.
        for name in ["panel", "footer", "grid"] {
            let n = per.get(name).copied().unwrap_or(0);
            assert!(
                n > 5,
                "the {name} pass contributed {n} centred runs; a pass this test \
                 never sees a run from is a pass it does not cover"
            );
        }
    }

    #[test]
    fn no_pass_paints_outside_the_region_it_owns() {
        for (w, h) in SIZES {
            for (state, app) in every_screen((w, h)) {
                let l = Layout::solve(w, h, app.width, app.height);
                for (name, pass, region) in PASSES {
                    let mut f = Frame::new(w, h);
                    pass(&app, &mut f, &l);
                    check_containment(&format!("{state} at {w}x{h}"), name, region(&l), &f);
                }
            }
        }
    }

    #[test]
    fn no_pass_paints_outside_a_band_squeezed_below_anything_the_layout_hands_out() {
        // The window list on its own is not enough. `Layout::solve` mostly
        // gives each band room for the type it carries, so a bound checked only
        // against the layout's own answers is checked where it was never going
        // to bite. The counter below is what says this sweep reached the cases
        // it exists for: bands with area, too short for the line they carry.
        let mut tight = 0_u32;
        for (w, h) in SIZES {
            for (state, app) in every_screen((w, h)) {
                let base = Layout::solve(w, h, app.width, app.height);
                for (name, band, pass) in SQUEEZABLE {
                    for squeezed in squeezes(*band(&mut base.clone())) {
                        let mut l = base;
                        *band(&mut l) = squeezed;
                        let mut f = Frame::new(w, h);
                        pass(&app, &mut f, &l);
                        check_containment(
                            &format!("{state} at {w}x{h} with {name} squeezed to {squeezed:?}"),
                            name,
                            squeezed,
                            &f,
                        );
                        if !squeezed.is_empty()
                            && squeezed.h < text::line_height(l.small, FontWeightHint::Regular)
                        {
                            tight += 1;
                        }
                    }
                }
            }
        }
        assert!(
            tight > 50,
            "only {tight} squeezed bands were shorter than the line they carry; \
             a sweep that never squeezes a band past its own type checks nothing"
        );
    }

    #[test]
    fn a_header_too_short_for_its_title_drops_the_name_rather_than_spilling_it() {
        // Not a hypothetical, and not squeezed by the test: `Layout::solve`
        // itself produces this. `header.h` is `title + pad * 1.6` and a run is
        // `line_height` -- about `1.33 * title` -- tall, so the band is shorter
        // than the run it carries whenever `title > 4.85 * pad`. `pad` follows
        // the *shorter* side of the window and `title` the *taller*, so a tall
        // narrow window separates them: at 133x600 the band is 28.5 and the
        // run 29.4. Before `centre_line` the title was drawn at a negative
        // offset from the band's top and spilled out of both of its ends.
        let (w, h) = (133.0, 600.0);
        let app = playing(0);
        let l = Layout::solve(w, h, app.width, app.height);
        assert!(
            l.header.h < text::line_height(l.title, FontWeightHint::Bold),
            "this probe is pointless unless the header really is too short: \
             {} against {}",
            l.header.h,
            text::line_height(l.title, FontWeightHint::Bold)
        );
        let mut f = Frame::new(w, h);
        app.draw_header(&mut f, &l);
        let names: Vec<String> = inked(&f)
            .into_iter()
            .map(|(s, _)| s)
            .filter(|s| s.contains(app.puzzle_name()))
            .collect();
        assert!(
            names.is_empty(),
            "the title does not fit this header and must not be drawn in it, \
             yet {names:?} was"
        );
        check_containment("the 133x600 header", "header", l.header, &f);
    }

    #[test]
    fn every_run_a_card_draws_stays_inside_the_card() {
        // The help and end cards own the window in `PASSES`, because both scrim
        // it. That makes the window sweep silent about the card itself, which
        // is the box that actually holds the type -- and the box that used to
        // be a fixed 360x280 whatever the window was. The card is found the
        // only way that cannot re-derive a fault: it is the largest thing the
        // pass filled.
        //
        // Both counters below matter. `cards` says the card was found at all --
        // a run of `continue`s would otherwise turn this into a test of
        // nothing. `runs` says the cards had type in them: a card located
        // correctly and drawn empty passes every assertion here and is exactly
        // the failure a size sweep is meant to catch.
        let mut cards = 0_u32;
        let mut runs = 0_u32;
        for (w, h) in SIZES {
            for (name, mut app, help) in
                [("help", playing(1), true), ("completed", playing(2), false)]
            {
                app.size = (w, h);
                app.show_help = help;
                let l = Layout::solve(w, h, app.width, app.height);
                let mut f = Frame::new(w, h);
                if help {
                    app.draw_help(&mut f, &l);
                } else {
                    app.draw_completed(&mut f, &l);
                }
                // The scrim is the whole window; the card is the next largest.
                let mut boxes = filled(&f);
                boxes.sort_by(|a, b| {
                    (b.w * b.h)
                        .partial_cmp(&(a.w * a.h))
                        .unwrap_or(core::cmp::Ordering::Equal)
                });
                let Some(card) = boxes.into_iter().find(|r| r.w < w || r.h < h) else {
                    // Every fill covers the window: there is no card to be
                    // outside of, which a window with no room for one is
                    // entitled to produce.
                    continue;
                };
                cards += 1;
                for (s, r) in inked(&f) {
                    runs += 1;
                    assert!(
                        inside(card, r),
                        "the {name} card at {w}x{h} is {card:?} and inked {s:?} at {r:?}"
                    );
                }
            }
        }
        assert!(
            cards > 20,
            "only {cards} of the sizes drew a card at all; this sweep has to \
             find one at nearly every size or it is testing the `continue`"
        );
        assert!(
            runs > 100,
            "only {runs} runs were checked against a card; a card drawn empty \
             passes every assertion above and is the fault this exists to find"
        );
    }

    #[test]
    fn nothing_is_painted_outside_the_window() {
        for (w, h) in SIZES {
            let window = Rect::new(0.0, 0.0, w, h);
            for (name, app) in every_screen((w, h)) {
                for r in painted_rects(&app, (w, h)) {
                    assert!(
                        r.x >= window.x - 0.51
                            && r.y >= window.y - 0.51
                            && r.right() <= window.right() + 0.51
                            && r.bottom() <= window.bottom() + 0.51,
                        "{name} painted {r:?} outside a {w}x{h} window"
                    );
                }
            }
        }
    }

    #[test]
    fn nothing_with_no_area_is_painted() {
        // `fill` refuses an empty rectangle. Without that refusal a window too
        // narrow for a clue panel still paints the panel's background -- a
        // zero-wide or negative-wide fill, which is inside the window and so
        // invisible to the test above, and which a renderer is entitled to
        // treat however it likes.
        for (w, h) in SIZES {
            for (name, app) in every_screen((w, h)) {
                for r in painted_rects(&app, (w, h)) {
                    assert!(
                        r.w > 0.0 && r.h > 0.0,
                        "{name} painted {r:?}, which has no area, in a {w}x{h} window"
                    );
                }
            }
        }
    }

    #[test]
    fn the_help_card_is_drawn_inside_the_window_that_needs_it() {
        // It was a fixed 360x280 box at `width / 2.0 - 180.0`: in a 200-pixel
        // window it started at -80 and ran to 280, which is to say the help
        // was drawn entirely outside the window whose user had asked for it.
        for (w, h) in SIZES {
            let mut app = playing(0);
            app.size = (w, h);
            app.show_help = true;
            let with_help = painted_rects(&app, (w, h));
            app.show_help = false;
            let without = painted_rects(&app, (w, h));
            assert!(
                with_help.len() > without.len(),
                "the help card has to be painted at {w}x{h}"
            );
            for r in with_help {
                assert!(
                    r.right() <= w + 0.51 && r.bottom() <= h + 0.51 && r.x >= -0.51 && r.y >= -0.51,
                    "the help card put {r:?} outside a {w}x{h} window"
                );
            }
        }
    }

    // ── The click lands where the picture is ───────────────────────

    #[test]
    fn a_click_selects_the_cell_it_was_painted_in() {
        // The fault this replaces: the drawing pass put the grid at
        // `grid_y = 72.0` and the click pass looked for it at `grid_y = 60.0`,
        // so a click was resolved against a grid twelve pixels above the one
        // on the screen and the top third of every cell selected its
        // neighbour. Clicking the *painted* box is the only thing that catches
        // that, because a test that re-derives the coordinates re-derives the
        // bug along with them.
        for (w, h) in SIZES {
            let mut app = playing(0);
            app.size = (w, h);
            let l = Layout::solve(w, h, app.width, app.height);
            if l.cell < 8.0 {
                continue;
            }
            for row in 0..app.height {
                for col in 0..app.width {
                    if !app.playable(row, col) {
                        continue;
                    }
                    let r = l.cell_rect(row, col);
                    // The top edge and the middle: the drift showed up first
                    // at the top of a cell, which is why it is checked there.
                    for (px, py) in [r.centre(), (r.x + r.w / 2.0, r.y + 1.0)] {
                        app.cursor = (0, 0);
                        app.direction = Direction::Across;
                        app.click_at(px, py, MouseButton::Left, (w, h));
                        assert_eq!(
                            app.cursor,
                            (row, col),
                            "a click at ({px}, {py}) in a {w}x{h} window must \
                             select the cell painted there"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn no_two_hit_boxes_claim_the_same_pixel() {
        for (w, h) in SIZES {
            let mut app = playing(0);
            app.size = (w, h);
            let hits = app.draw((w, h)).hits().to_vec();
            for (i, (ta, ra)) in hits.iter().enumerate() {
                for (tb, rb) in hits.iter().skip(i.saturating_add(1)) {
                    assert!(
                        ra.intersect(*rb).is_none(),
                        "{ta:?} at {ra:?} and {tb:?} at {rb:?} overlap in a \
                         {w}x{h} window, so a click on the overlap is \
                         resolved by draw order rather than by aim"
                    );
                }
            }
        }
    }

    #[test]
    fn clicking_the_cell_the_cursor_is_already_on_turns_the_word() {
        let mut app = playing(0);
        app.cursor = (2, 2);
        app.direction = Direction::Across;
        click_sized(
            &mut app,
            Target::Cell(2, 2),
            MouseButton::Left,
            Crossword::SIZE,
        );
        assert_eq!(app.direction, Direction::Down, "the second click turns");
        click_sized(
            &mut app,
            Target::Cell(2, 2),
            MouseButton::Left,
            Crossword::SIZE,
        );
        assert_eq!(app.direction, Direction::Across, "and turns back");
    }

    #[test]
    fn clicking_a_clue_moves_the_cursor_to_the_word_it_names() {
        let mut app = playing(0);
        app.size = SHORT;
        for index in 0..app.clues.len() {
            app.clue_scroll = 0;
            app.show_clue(index);
            assert!(
                is_visible_sized(&app, Target::ClueRow(index), SHORT),
                "clue {index} is on no screen the panel can reach"
            );
            click_sized(&mut app, Target::ClueRow(index), MouseButton::Left, SHORT);
            let clue = &app.clues[index];
            assert_eq!(
                app.cursor,
                (clue.row, clue.col),
                "clicking clue {index} must put the cursor on its first cell"
            );
            assert_eq!(app.direction, clue.direction, "and face its direction");
        }
    }

    #[test]
    fn clicking_a_menu_row_opens_that_puzzle() {
        for index in 0..PUZZLES.len() {
            let mut app = Crossword::new();
            click_sized(
                &mut app,
                Target::PuzzleRow(index),
                MouseButton::Left,
                Crossword::SIZE,
            );
            assert_eq!(app.view, View::Playing, "row {index} has to start a game");
            assert_eq!(
                app.puzzle_name(),
                PUZZLES[index].name,
                "row {index} has to open the puzzle it is labelled with"
            );
        }
    }

    #[test]
    fn a_click_on_nothing_puts_the_help_away_and_otherwise_does_nothing() {
        let mut app = playing(0);
        app.show_help = true;
        let before = app.cursor;
        // A corner of the window with no control in it.
        let outcome = app.click_at(
            Crossword::SIZE.0 - 1.0,
            Crossword::SIZE.1 - 1.0,
            MouseButton::Left,
            Crossword::SIZE,
        );
        assert_eq!(outcome, EventResult::Consumed);
        assert!(!app.show_help, "a click off the card dismisses it");
        assert_eq!(app.cursor, before, "and moves nothing else");

        let outcome = app.click_at(
            Crossword::SIZE.0 - 1.0,
            Crossword::SIZE.1 - 1.0,
            MouseButton::Left,
            Crossword::SIZE,
        );
        assert_eq!(
            outcome,
            EventResult::Ignored,
            "with no card up the same click is nothing at all"
        );
    }

    // ── The puzzles are crosswords ─────────────────────────────────

    #[test]
    fn every_grid_spells_the_words_it_is_supposed_to() {
        for (index, (name, across, down)) in ANSWERS.into_iter().enumerate() {
            let app = playing(index);
            assert_eq!(app.puzzle_name(), name, "puzzle {index} is out of order");

            for dir in [Direction::Across, Direction::Down] {
                let want: &[&str] = if dir == Direction::Across {
                    across
                } else {
                    down
                };
                let got: Vec<String> = app
                    .clues
                    .iter()
                    .filter(|c| c.direction == dir)
                    .map(|c| answer_of(&app, c))
                    .collect();
                assert_eq!(
                    got,
                    want,
                    "{name} {} answers, in grid order",
                    dir.label().to_lowercase()
                );
            }
        }
    }

    #[test]
    fn every_run_of_two_or_more_cells_is_a_word_with_a_clue() {
        // The columns of the old grids -- `SAPETRE`, `HALLILA`, `FSIVRNA` --
        // existed because the down direction had no words in it at all: the
        // grids were laid out from their across answers and nobody looked
        // sideways. A word here is a maximal run of two or more playable
        // cells, and every one of them must be a clue.
        for index in 0..PUZZLES.len() {
            let app = playing(index);
            let name = app.puzzle_name();
            for dir in [Direction::Across, Direction::Down] {
                for row in 0..app.height {
                    for col in 0..app.width {
                        if !app.starts_word(row, col, dir) {
                            continue;
                        }
                        let cells = app.word_cells(row, col, dir);
                        assert!(
                            cells.len() >= 2,
                            "{name}: a one-cell run at ({row}, {col}) {dir:?} \
                             was numbered as a word"
                        );
                        let clue = app
                            .clues
                            .iter()
                            .find(|c| c.direction == dir && (c.row, c.col) == (row, col))
                            .unwrap_or_else(|| {
                                panic!("{name}: the {dir:?} word at ({row}, {col}) has no clue")
                            });
                        assert_eq!(
                            clue.len,
                            cells.len(),
                            "{name}: the clue at ({row}, {col}) {dir:?} states \
                             a length the grid does not have"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn a_word_with_no_clue_written_for_it_says_so_in_the_panel() {
        // The clue tables are hand-written and the words are read off the
        // grid, so a table can be short. It used to lose the extra words in
        // silence. `MISSING_CLUE` is what a player sees instead -- and this is
        // the only test that reaches that arm, since all three shipped tables
        // are complete and `every_word_in_every_puzzle_has_a_clue` is what
        // keeps them that way.
        static SHORT_OF_CLUES: PuzzleDef = PuzzleDef {
            name: "short",
            width: 5,
            height: 5,
            grid: PUZZLES[0].grid,
            across: &["Only one across clue was written"],
            down: &[],
        };
        let mut app = playing(0);
        app.number_and_clue(&SHORT_OF_CLUES);
        let missing = app.clues.iter().filter(|c| c.text == MISSING_CLUE).count();
        assert_eq!(
            missing,
            app.clues.len().saturating_sub(1),
            "every word but the one clue that was written is missing its clue"
        );
        let drawn = painted_text(&app, Crossword::SIZE);
        assert!(
            drawn.iter().any(|(s, _)| s.contains(MISSING_CLUE)),
            "a word with no clue has to be visible as such in the panel"
        );
    }

    #[test]
    fn every_word_in_every_puzzle_has_a_clue() {
        // A clue list shorter than the grid's word count used to lose the
        // extra words in silence -- eight per puzzle. `MISSING_CLUE` makes
        // that visible in the panel, and this makes it a failure here.
        for index in 0..PUZZLES.len() {
            let app = playing(index);
            for clue in &app.clues {
                assert_ne!(
                    clue.text,
                    MISSING_CLUE,
                    "{}: the {:?} word at ({}, {}) has no clue written for it",
                    app.puzzle_name(),
                    clue.direction,
                    clue.row,
                    clue.col
                );
            }
            for dir in [Direction::Across, Direction::Down] {
                let def = &PUZZLES[index];
                let texts = if dir == Direction::Across {
                    def.across
                } else {
                    def.down
                };
                let words = app.clues.iter().filter(|c| c.direction == dir).count();
                assert_eq!(
                    texts.len(),
                    words,
                    "{}: {} {dir:?} clues written for {words} words in the grid",
                    def.name,
                    texts.len()
                );
            }
        }
    }

    #[test]
    fn a_clue_carries_the_text_written_for_the_word_it_is_on() {
        // The lists are in grid order and carry no numbers, so the only thing
        // tying a clue to its word is the position it was read from. Both
        // lists are five long in all three puzzles, so a pair handed to the
        // wrong directions would leave every count right and every word
        // clued -- and every clue on the wrong word.
        for index in 0..PUZZLES.len() {
            let app = playing(index);
            let def = &PUZZLES[index];
            for dir in [Direction::Across, Direction::Down] {
                let texts = if dir == Direction::Across {
                    def.across
                } else {
                    def.down
                };
                let got: Vec<&str> = app
                    .clues
                    .iter()
                    .filter(|c| c.direction == dir)
                    .map(|c| c.text)
                    .collect();
                assert_eq!(got, texts, "{}: the {dir:?} clues, in grid order", def.name);
            }
        }
    }

    #[test]
    fn every_clue_number_is_the_number_on_the_cell_it_starts_at() {
        // The definitions used to carry hand-written numbers which the program
        // compared against numbering it derived from the grid; the two
        // disagreed in all three puzzles. A clue's number is now read off the
        // grid, and this is the statement of that.
        for index in 0..PUZZLES.len() {
            let app = playing(index);
            for clue in &app.clues {
                let cell = app.cell(clue.row, clue.col).unwrap();
                assert_eq!(
                    clue.number,
                    cell.number,
                    "{}: clue {:?} at ({}, {}) claims a number its cell does \
                     not carry",
                    app.puzzle_name(),
                    clue.direction,
                    clue.row,
                    clue.col
                );
                assert!(clue.number > 0, "a word's first cell must be numbered");
            }
        }
    }

    #[test]
    fn the_numbers_run_from_one_in_reading_order() {
        for index in 0..PUZZLES.len() {
            let app = playing(index);
            let mut expected = 0u16;
            for row in 0..app.height {
                for col in 0..app.width {
                    let starts = [Direction::Across, Direction::Down]
                        .into_iter()
                        .any(|d| app.starts_word(row, col, d));
                    let number = app.cell(row, col).map_or(0, |c| c.number);
                    if starts {
                        expected = expected.saturating_add(1);
                        assert_eq!(
                            number,
                            expected,
                            "{}: ({row}, {col}) starts a word, so it is number \
                             {expected}",
                            app.puzzle_name()
                        );
                    } else {
                        assert_eq!(
                            number,
                            0,
                            "{}: ({row}, {col}) starts nothing and must carry \
                             no number",
                            app.puzzle_name()
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn a_cell_that_starts_a_word_both_ways_carries_one_number() {
        let app = playing(0);
        let across = app.clues.iter().find(|c| (c.row, c.col) == (0, 0)).unwrap();
        let down = app
            .clues
            .iter()
            .find(|c| (c.row, c.col) == (0, 0) && c.direction == Direction::Down)
            .unwrap();
        assert_eq!(
            across.number, down.number,
            "1 Across and 1 Down start in the same square"
        );
        assert_eq!(across.number, 1, "and it is the first square numbered");
    }

    #[test]
    fn a_coordinate_off_the_board_is_off_the_board() {
        // `index` is the one place that answers "is this on the grid". Both
        // halves of its bound are load-bearing and neither is reachable
        // through a key or a click, so this asks it directly: a column past
        // the right edge would otherwise wrap onto the next row, and a row
        // past the bottom would run off the end of a board that a future
        // puzzle might not have sized to its `cells` vector.
        let app = playing(0);
        assert_eq!(app.index(0, 0), Some(0));
        assert_eq!(
            app.index(1, 0),
            Some(app.width),
            "the second row starts one width along"
        );
        assert_eq!(
            app.index(app.height, 0),
            None,
            "a row past the bottom is off the board"
        );
        assert_eq!(
            app.index(0, app.width),
            None,
            "a column past the right edge is off the board, not the next row"
        );
    }

    #[test]
    fn every_grid_is_the_size_it_says_it_is() {
        for def in PUZZLES {
            assert_eq!(
                def.grid.chars().count(),
                def.width * def.height,
                "{}: the grid string is not {}x{}",
                def.name,
                def.width,
                def.height
            );
            for ch in def.grid.chars() {
                assert!(
                    ch == '#' || ch.is_ascii_uppercase(),
                    "{}: {ch:?} is neither a black square nor a letter",
                    def.name
                );
            }
        }
    }

    // ── The clock ──────────────────────────────────────────────────

    #[test]
    fn the_clock_advances_while_the_puzzle_is_open() {
        // It did not before. `load_puzzle` set `elapsed_secs = 0` and
        // `timer_running = true`, and no line in the file ever added a second,
        // so the readout was `00:00` for the whole puzzle and the end card
        // congratulated every player on solving it instantly.
        let mut app = playing(0);
        assert_eq!(app.format_time(), "00:00", "a fresh puzzle starts at zero");
        for _ in 0..5 {
            app.handle_event(&Event::Tick { elapsed_ms: 1000 });
        }
        assert_eq!(
            app.elapsed_secs(),
            5,
            "five seconds of ticks is five seconds"
        );
        assert_eq!(app.format_time(), "00:05");
        for _ in 0..115 {
            app.handle_event(&Event::Tick { elapsed_ms: 1000 });
        }
        assert_eq!(app.format_time(), "02:00", "and it carries into minutes");
    }

    #[test]
    fn the_clock_is_stopped_in_the_menu_and_on_the_end_card() {
        for view in [View::PuzzleSelect, View::Completed] {
            let mut app = playing(0);
            app.view = view;
            let outcome = app.handle_event(&Event::Tick { elapsed_ms: 5000 });
            assert_eq!(app.elapsed_ms, 0, "the clock must not run in {view:?}");
            assert_eq!(
                outcome,
                EventResult::Ignored,
                "and a tick in {view:?} is not worth a repaint"
            );
        }
    }

    #[test]
    fn the_readout_shows_the_clock_rather_than_a_constant() {
        let mut app = playing(0);
        app.handle_event(&Event::Tick { elapsed_ms: 91_000 });
        let drawn = painted_text(&app, Crossword::SIZE);
        assert!(
            drawn.iter().any(|(s, _)| s == "01:31"),
            "the header has to print the time the clock holds; it printed {:?}",
            drawn.iter().map(|(s, _)| s).collect::<Vec<_>>()
        );
    }

    #[test]
    fn the_app_asks_for_a_tick() {
        // Without one the clock cannot move however well `handle_tick` works.
        let app = playing(0);
        assert_eq!(
            app.tick_interval(),
            Some(std::time::Duration::from_millis(TICK_MS)),
            "an app with a clock in it has to ask to be ticked"
        );
    }

    // ── Typing ─────────────────────────────────────────────────────

    #[test]
    fn typing_a_letter_fills_the_cell_and_moves_along_the_word() {
        let mut app = playing(0);
        app.cursor = (0, 0);
        app.direction = Direction::Across;
        app.key_at(&press(Key::S), Crossword::SIZE);
        assert_eq!(app.cell(0, 0).unwrap().entry, Some('S'));
        assert_eq!(app.cursor, (0, 1), "and the cursor moves on");
    }

    #[test]
    fn typing_stops_at_the_end_of_a_word_rather_than_jumping_the_black_square() {
        // `advance_cursor` used to scan for the next playable cell in the row,
        // which stepped straight over the black square and carried on typing
        // into a different word.
        let mut app = playing(0);
        app.cursor = (0, 2);
        app.direction = Direction::Across;
        assert!(
            !app.playable(0, 3),
            "the fixture needs a black square at (0, 3)"
        );
        assert!(app.playable(0, 4), "and a playable cell beyond it");
        app.key_at(&press(Key::A), Crossword::SIZE);
        assert_eq!(app.cell(0, 2).unwrap().entry, Some('A'));
        assert_eq!(
            app.cursor,
            (0, 2),
            "the last cell of a word is where typing stops"
        );
    }

    #[test]
    fn typing_over_a_letter_that_was_given_away_makes_it_the_players() {
        // The old code left `revealed` set, so the end card went on counting a
        // letter against the player long after they had typed it themselves.
        let mut app = playing(0);
        app.cursor = (0, 0);
        app.reveal_letter();
        assert_eq!(app.revealed_count(), 1);
        app.cursor = (0, 0);
        app.key_at(&press(Key::S), Crossword::SIZE);
        assert_eq!(
            app.revealed_count(),
            0,
            "a letter the player typed is the player's"
        );
    }

    #[test]
    fn backspace_clears_this_cell_then_steps_back() {
        let mut app = playing(0);
        app.cursor = (0, 0);
        app.direction = Direction::Across;
        app.key_at(&press(Key::S), Crossword::SIZE);
        app.key_at(&press(Key::P), Crossword::SIZE);
        assert_eq!(app.cursor, (0, 2));

        app.key_at(&press(Key::Backspace), Crossword::SIZE);
        assert_eq!(app.cursor, (0, 1), "an empty cell steps back");
        assert_eq!(
            app.cell(0, 1).unwrap().entry,
            None,
            "and clears what it finds"
        );

        app.cursor = (0, 0);
        app.key_at(&press(Key::Backspace), Crossword::SIZE);
        assert_eq!(app.cell(0, 0).unwrap().entry, None, "a full cell empties");
        assert_eq!(app.cursor, (0, 0), "without moving");
    }

    #[test]
    fn a_letter_is_stored_upper_case_however_it_is_typed() {
        let mut app = playing(0);
        app.cursor = (0, 0);
        app.key_at(&press(Key::S), Crossword::SIZE);
        assert_eq!(app.cell(0, 0).unwrap().entry, Some('S'));
    }

    #[test]
    fn a_key_that_types_nothing_is_not_treated_as_a_letter() {
        // `key_to_char` used to answer `'\0'` for "not a letter", which every
        // caller then had to remember to compare against.
        assert_eq!(letter_of(Key::A), Some('A'));
        assert_eq!(letter_of(Key::Z), Some('Z'));
        for key in [Key::F5, Key::Home, Key::Enter, Key::Num1] {
            assert_eq!(letter_of(key), None, "{key:?} types no letter");
        }

        let mut app = playing(0);
        app.cursor = (0, 0);
        let outcome = app.key_at(&press(Key::F5), Crossword::SIZE);
        assert_eq!(outcome, EventResult::Ignored);
        assert_eq!(app.cell(0, 0).unwrap().entry, None, "and fills nothing");
    }

    // ── The cursor ─────────────────────────────────────────────────

    #[test]
    fn an_arrow_turns_the_cursor_the_way_it_moved() {
        // The highlighted word, the banner and what typing does next all
        // follow `direction`, so an arrow that moves up the board while
        // leaving the cursor reading across selects a word the player did not
        // point at.
        for (key, face) in [
            (Key::Up, Direction::Down),
            (Key::Down, Direction::Down),
            (Key::Left, Direction::Across),
            (Key::Right, Direction::Across),
        ] {
            for start in [Direction::Across, Direction::Down] {
                let mut app = playing(0);
                app.cursor = (2, 2);
                app.direction = start;
                let before = app.cursor;
                app.key_at(&press(key), Crossword::SIZE);
                assert_ne!(before, app.cursor, "{key:?} has to move from (2, 2)");
                assert_eq!(
                    app.direction, face,
                    "{key:?} moved {face:?} and must leave the cursor reading that way"
                );
            }
        }
    }

    #[test]
    fn the_four_arrows_are_the_same_code_and_each_undoes_the_other() {
        // They were four hand-written loops, and `Key::Up`'s bounds guard was
        // in the match arm while `Key::Down`'s was in the loop, so the two
        // were not the same code in any sense a reader could check.
        let mut app = playing(0);
        for (there, back, from) in [
            (Key::Right, Key::Left, (2, 0)),
            (Key::Left, Key::Right, (2, 4)),
            (Key::Down, Key::Up, (0, 0)),
            (Key::Up, Key::Down, (4, 0)),
        ] {
            app.cursor = from;
            app.key_at(&press(there), Crossword::SIZE);
            assert_ne!(app.cursor, from, "{there:?} from {from:?} must move");
            let moved = app.cursor;
            app.key_at(&press(back), Crossword::SIZE);
            assert_eq!(
                app.cursor, from,
                "{back:?} must undo {there:?}: {from:?} -> {moved:?} -> {:?}",
                app.cursor
            );
        }
    }

    #[test]
    fn an_arrow_crosses_a_black_square_where_typing_would_not() {
        let mut app = playing(0);
        app.cursor = (0, 2);
        assert!(
            !app.playable(0, 3),
            "the fixture needs a black square at (0, 3)"
        );
        app.key_at(&press(Key::Right), Crossword::SIZE);
        assert_eq!(app.cursor, (0, 4), "the arrows walk the grid, not the word");
    }

    #[test]
    fn an_arrow_at_the_edge_stays_put_rather_than_wrapping() {
        let mut app = playing(0);
        for (key, at) in [
            (Key::Left, (2, 0)),
            (Key::Right, (2, 4)),
            (Key::Up, (0, 0)),
            (Key::Down, (4, 0)),
        ] {
            app.cursor = at;
            app.direction = Direction::Across;
            let outcome = app.key_at(&press(key), Crossword::SIZE);
            assert_eq!(
                app.cursor, at,
                "{key:?} at the edge {at:?} has nowhere to go"
            );
            assert_eq!(outcome, EventResult::Ignored, "and is not worth a repaint");
            // Nor does it turn: an arrow that could not move has not chosen a
            // word, and turning would select a different word in place.
            assert_eq!(
                app.direction,
                Direction::Across,
                "{key:?} at the edge {at:?} moved nothing, so it turns nothing"
            );
        }
    }

    #[test]
    fn space_turns_the_word_under_the_cursor() {
        let mut app = playing(0);
        app.cursor = (0, 0);
        app.direction = Direction::Across;
        app.key_at(&press(Key::Space), Crossword::SIZE);
        assert_eq!(app.direction, Direction::Down);
        app.key_at(&press(Key::Space), Crossword::SIZE);
        assert_eq!(app.direction, Direction::Across);
    }

    // ── The clue list ──────────────────────────────────────────────

    #[test]
    fn tab_and_shift_tab_walk_the_clue_list_in_opposite_directions() {
        // `move_to_next_clue(_reverse: bool)` took the argument and ignored
        // it, so Shift+Tab was Tab.
        let mut app = playing(0);
        let n = app.clues.len();
        assert!(n > 2, "the fixture needs a list worth cycling");

        app.go_to_clue(0);
        let mut seen = Vec::new();
        for _ in 0..n {
            seen.push(app.current_clue().unwrap());
            app.key_at(&press(Key::Tab), Crossword::SIZE);
        }
        let mut sorted = seen.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), n, "Tab must reach every clue exactly once");
        assert_eq!(
            app.current_clue(),
            Some(0),
            "and wrap back to where it started"
        );

        let mut backwards = Vec::new();
        for _ in 0..n {
            app.key_at(&shift(Key::Tab), Crossword::SIZE);
            backwards.push(app.current_clue().unwrap());
        }
        seen.reverse();
        assert_eq!(
            backwards, seen,
            "Shift+Tab must walk the list the other way"
        );
    }

    /// A window with a clue panel too short for the whole list.
    ///
    /// The default 860x580 window shows all ten clues at once, so a scrolling
    /// test written against it asserts nothing about scrolling: it passes
    /// against a program whose wheel does nothing, because there is nothing
    /// for the wheel to do (lesson 90). Everything about the scrollbar is
    /// checked here instead, and `the_short_window_really_is_too_short` is
    /// what keeps this size in the regime it was chosen for.
    const SHORT: (f32, f32) = (900.0, 200.0);

    #[test]
    fn the_short_window_really_is_too_short_for_the_clue_list() {
        let mut app = playing(0);
        app.size = SHORT;
        let l = Layout::solve(SHORT.0, SHORT.1, app.width, app.height);
        assert!(
            !l.panel.is_empty(),
            "the fixture needs a panel, or there is nothing to scroll"
        );
        assert!(
            l.clue_rows_visible() < app.panel_rows().len(),
            "the fixture needs a panel shorter than the list: {} rows on the \
             screen for a list of {}",
            l.clue_rows_visible(),
            app.panel_rows().len()
        );
        assert!(app.max_scroll() > 0);
    }

    #[test]
    fn a_heading_is_a_row_of_the_list_it_heads() {
        // The row budget counted clues and the drawing put a heading among
        // them, so the list was one row longer than anything had scrolled for
        // and its last clue could not be reached. The two headings are rows
        // now, and this is what says so.
        let app = playing(0);
        let rows = app.panel_rows();
        assert_eq!(
            rows.len(),
            app.clues.len().saturating_add(2),
            "ten clues and an Across and a Down heading: {rows:?}"
        );
        assert_eq!(rows.first(), Some(&PanelRow::Heading(Direction::Across)));
        assert!(
            rows.contains(&PanelRow::Heading(Direction::Down)),
            "the down clues need a heading too"
        );
        for (i, _) in app.clues.iter().enumerate() {
            let row = app
                .row_of_clue(i)
                .unwrap_or_else(|| panic!("clue {i} is on no row"));
            assert_eq!(rows[row], PanelRow::Clue(i));
        }
    }

    #[test]
    fn the_panel_draws_every_row_it_has_room_for_and_no_more() {
        // The count on the screen is what the scroll arithmetic is computed
        // from, so a panel that drew fewer rows than `clue_rows_visible` says
        // would leave the end of the list unreachable however the limit was
        // computed.
        let mut app = playing(0);
        app.size = SHORT;
        let l = Layout::solve(SHORT.0, SHORT.1, app.width, app.height);
        let visible = l.clue_rows_visible();
        for scroll in 0..=app.max_scroll() {
            app.clue_scroll = scroll;
            let drawn = app
                .draw(SHORT)
                .hits()
                .iter()
                .filter(|(t, _)| matches!(t, Target::ClueRow(_)))
                .count();
            let headings = app
                .panel_rows()
                .iter()
                .skip(scroll)
                .take(visible)
                .filter(|r| matches!(r, PanelRow::Heading(_)))
                .count();
            assert_eq!(
                drawn.saturating_add(headings),
                visible.min(app.panel_rows().len().saturating_sub(scroll)),
                "at scroll {scroll} the panel drew {drawn} clues and \
                 {headings} headings out of {visible} rows"
            );
        }
    }

    #[test]
    fn every_clue_can_be_scrolled_onto_the_panel() {
        // The panel drew `.take(8)` from a scroll offset no event ever
        // changed, so a puzzle's ninth clue was simply not on the screen.
        for index in 0..PUZZLES.len() {
            let mut app = playing(index);
            app.size = SHORT;
            let mut reached = vec![false; app.clues.len()];
            for scroll in 0..=app.max_scroll() {
                app.clue_scroll = scroll;
                for (target, _) in app.draw(SHORT).hits() {
                    if let Target::ClueRow(i) = *target {
                        reached[i] = true;
                    }
                }
            }
            for (i, ok) in reached.iter().enumerate() {
                assert!(
                    *ok,
                    "{}: clue {i} is on no screen the panel can scroll to",
                    app.puzzle_name()
                );
            }
        }
    }

    #[test]
    fn the_wheel_scrolls_the_clue_list_and_stops_at_both_ends() {
        let mut app = playing(0);
        app.size = SHORT;
        assert!(
            app.max_scroll() > 0,
            "the fixture needs a list longer than the panel"
        );

        // One notch away from the user scrolls towards the end of the list.
        app.scroll_at(0.0, 0.0, -1.0, SHORT);
        assert!(app.clue_scroll > 0, "one notch has to move the list");
        for _ in 0..20 {
            app.scroll_at(0.0, 0.0, -1.0, SHORT);
        }
        assert_eq!(
            app.clue_scroll,
            app.max_scroll(),
            "the list stops at its last screen rather than scrolling into space"
        );
        for _ in 0..20 {
            app.scroll_at(0.0, 0.0, 1.0, SHORT);
        }
        assert_eq!(app.clue_scroll, 0, "and at the top");
    }

    #[test]
    fn a_fraction_of_a_notch_is_kept_rather_than_rounded_away() {
        // A trackpad sends fractions of a notch. Rounding each event on its
        // own throws them away and the list never moves at all.
        let mut app = playing(0);
        app.size = SHORT;
        let tenth = -0.1;
        for _ in 0..3 {
            app.scroll_at(0.0, 0.0, tenth, SHORT);
        }
        assert_eq!(
            app.clue_scroll, 0,
            "three tenths of a notch is less than a row, so nothing moves yet"
        );
        for _ in 0..37 {
            app.scroll_at(0.0, 0.0, tenth, SHORT);
        }
        assert!(
            app.clue_scroll > 0,
            "forty tenths of a notch have to add up to something"
        );
    }

    #[test]
    fn moving_the_cursor_scrolls_the_clue_it_lands_on_into_view() {
        let mut app = playing(0);
        app.size = SHORT;
        let last = app.clues.len().saturating_sub(1);
        app.clue_scroll = 0;
        assert!(
            !is_visible_sized(&app, Target::ClueRow(last), SHORT),
            "the fixture needs a clue that starts off the screen"
        );
        app.go_to_clue(last);
        assert!(
            is_visible_sized(&app, Target::ClueRow(last), SHORT),
            "the clue the cursor is in has to be on the screen"
        );
        app.go_to_clue(0);
        assert!(
            is_visible_sized(&app, Target::ClueRow(0), SHORT),
            "and so does the one it comes back to"
        );
    }

    // ── The buttons ────────────────────────────────────────────────

    #[test]
    fn the_buttons_and_the_keys_do_the_same_thing() {
        // The footer used to be one line of text naming eight keystrokes,
        // drawn in the one strip of the window that exists to be clicked. Now
        // every verb has both routes, and this is what keeps them one verb.
        for button in BUTTONS {
            let mut clicked = playing(0);
            clicked.cursor = (2, 1);
            let mut typed = playing(0);
            typed.cursor = (2, 1);

            click_sized(
                &mut clicked,
                Target::Button(button),
                MouseButton::Left,
                Crossword::SIZE,
            );

            let event = match button {
                Button::Check => ctrl(Key::C),
                Button::Clear => ctrl(Key::U),
                Button::RevealLetter => ctrl(Key::R),
                Button::RevealWord => ctrl(Key::W),
                Button::Help => press(Key::F1),
                Button::Menu => press(Key::Escape),
            };
            typed.key_at(&event, Crossword::SIZE);

            assert_eq!(
                snapshot(&clicked),
                snapshot(&typed),
                "{button:?}: the button and {} must be one verb",
                button.key_hint()
            );
        }
    }

    #[test]
    fn every_button_is_drawn_where_a_click_can_reach_it() {
        let app = playing(0);
        let l = Layout::solve(Crossword::SIZE.0, Crossword::SIZE.1, app.width, app.height);
        for button in BUTTONS {
            let r = rect_of_sized(&app, Target::Button(button), Crossword::SIZE)
                .unwrap_or_else(|| panic!("{button:?} is not on the screen at the default size"));
            assert!(!r.is_empty(), "{button:?} has no area to click");
            // Whole, not overlapping. `intersect(..).is_some()` is satisfied by
            // a button hanging half out of the strip, and half a button drawn
            // over the grid is a button drawn over the grid.
            assert!(
                r.x >= l.footer.x
                    && r.y >= l.footer.y
                    && r.right() <= l.footer.right()
                    && r.bottom() <= l.footer.bottom(),
                "{button:?} at {r:?} is not wholly inside the footer {:?}",
                l.footer
            );
        }
    }

    #[test]
    fn a_button_that_will_not_fit_is_left_out_rather_than_drawn_off_the_edge() {
        let mut app = playing(0);
        let narrow = (150.0, 600.0);
        app.size = narrow;
        let l = Layout::solve(narrow.0, narrow.1, app.width, app.height);
        let drawn: Vec<Button> = app
            .draw(narrow)
            .hits()
            .iter()
            .filter_map(|(t, _)| match *t {
                Target::Button(b) => Some(b),
                _ => None,
            })
            .collect();
        assert!(
            drawn.len() < BUTTONS.len(),
            "a 150-pixel window cannot hold all six buttons"
        );
        assert_eq!(
            drawn,
            BUTTONS[..drawn.len()],
            "the ones that fit are the first ones, in order"
        );
        for button in &drawn {
            let r = rect_of_sized(&app, Target::Button(*button), narrow).unwrap();
            assert!(
                r.right() <= l.footer.right(),
                "{button:?} at {r:?} runs past the footer {:?}",
                l.footer
            );
        }
    }

    // ── Checking, revealing, finishing ─────────────────────────────

    #[test]
    fn check_marks_the_wrong_letters_and_nothing_else() {
        let mut app = playing(0);
        app.cursor = (0, 0);
        app.key_at(&press(Key::S), Crossword::SIZE);
        app.cursor = (0, 1);
        app.key_at(&press(Key::Z), Crossword::SIZE);

        assert!(!app.is_wrong(0, 1), "nothing is wrong before it is checked");
        app.key_at(&ctrl(Key::C), Crossword::SIZE);
        assert!(app.is_wrong(0, 1), "Z is not the letter that goes there");
        assert!(!app.is_wrong(0, 0), "S is");
        assert!(!app.is_wrong(2, 2), "an empty cell is not yet wrong");
    }

    #[test]
    fn clearing_the_marks_leaves_the_letters_alone() {
        // The mark used to be a `flagged_wrong` field on every cell, set by
        // the check and cleared at four separate call sites, so it could
        // disagree with the letter it was marking. It is derived now.
        let mut app = playing(0);
        app.cursor = (0, 1);
        app.key_at(&press(Key::Z), Crossword::SIZE);
        app.key_at(&ctrl(Key::C), Crossword::SIZE);
        assert!(app.is_wrong(0, 1));

        app.key_at(&ctrl(Key::U), Crossword::SIZE);
        assert!(!app.is_wrong(0, 1), "the mark is gone");
        assert_eq!(
            app.cell(0, 1).unwrap().entry,
            Some('Z'),
            "and the letter is not"
        );
    }

    #[test]
    fn a_mark_follows_the_letter_it_marks() {
        let mut app = playing(0);
        app.cursor = (0, 1);
        app.key_at(&press(Key::Z), Crossword::SIZE);
        app.key_at(&ctrl(Key::C), Crossword::SIZE);
        assert!(app.is_wrong(0, 1));

        app.cursor = (0, 1);
        app.key_at(&press(Key::P), Crossword::SIZE);
        assert!(
            !app.is_wrong(0, 1),
            "typing the right letter unmarks the cell without a second check"
        );
    }

    #[test]
    fn revealing_a_word_fills_all_of_it_and_only_it() {
        let mut app = playing(0);
        // Row 0, not row 2: row 2 is `ELITE`, a word that fills its whole row,
        // and on it "reveal the word" and "reveal the row" are the same
        // instruction. Row 0 is `SPA#S`, where the two differ (lesson 90).
        app.cursor = (0, 0);
        app.direction = Direction::Across;
        let word = app.current_word();
        let playable_in_row = (0..app.width).filter(|&c| app.playable(0, c)).count();
        assert!(
            word.len() < playable_in_row,
            "the fixture needs a row with more cells in it than the word has"
        );
        app.key_at(&ctrl(Key::W), Crossword::SIZE);
        for (r, c) in &word {
            let cell = app.cell(*r, *c).unwrap();
            assert_eq!(cell.entry, Some(cell.solution), "({r}, {c}) is filled in");
            assert!(cell.revealed, "and marked as given away");
        }
        assert_eq!(app.revealed_count(), word.len(), "and nothing else is");
    }

    #[test]
    fn the_puzzle_ends_when_the_last_letter_goes_in_and_not_before() {
        // The old test was `all_filled && all_correct`, and a board that
        // distinguishes the two does not exist: a cell whose entry equals its
        // solution has an entry. Deleting the first half changed nothing
        // observable, which is a condition no test could reach (lesson 92).
        let mut app = playing(0);
        let cells: Vec<(usize, usize)> = (0..app.height)
            .flat_map(|r| (0..app.width).map(move |c| (r, c)))
            .filter(|&(r, c)| app.playable(r, c))
            .collect();

        for &(r, c) in cells.iter().take(cells.len().saturating_sub(1)) {
            let solution = app.cell(r, c).unwrap().solution;
            app.cursor = (r, c);
            app.enter_letter(solution);
        }
        assert_eq!(
            app.view,
            View::Playing,
            "one empty cell is not a finished puzzle"
        );
        assert!(!app.is_solved());

        let (r, c) = cells[cells.len().saturating_sub(1)];
        let solution = app.cell(r, c).unwrap().solution;
        app.cursor = (r, c);
        app.enter_letter(solution);
        assert!(app.is_solved(), "every cell holds its answer");
        assert_eq!(app.view, View::Completed, "so the puzzle is over");
    }

    #[test]
    fn a_full_grid_of_wrong_letters_is_not_a_finished_puzzle() {
        let mut app = playing(0);
        for r in 0..app.height {
            for c in 0..app.width {
                if let Some(cell) = app.cell_mut(r, c) {
                    // A letter that is never the solution anywhere in the grid.
                    cell.entry = Some('Q');
                }
            }
        }
        app.settle();
        assert!(!app.is_solved(), "filled is not solved");
        assert_eq!(app.view, View::Playing);
    }

    #[test]
    fn an_empty_board_is_not_a_solved_one() {
        // `all(..)` over nothing is true, so a program with no puzzle loaded
        // would otherwise open on its own congratulations card.
        let app = Crossword::new();
        assert!(app.cells.is_empty());
        assert!(
            !app.is_solved(),
            "a program with no puzzle has solved nothing"
        );
    }

    #[test]
    fn escape_puts_the_help_away_before_it_leaves_the_puzzle() {
        let mut app = playing(0);
        app.show_help = true;
        app.key_at(&press(Key::Escape), Crossword::SIZE);
        assert!(!app.show_help);
        assert_eq!(app.view, View::Playing, "the first Escape closes the card");
        app.key_at(&press(Key::Escape), Crossword::SIZE);
        assert_eq!(app.view, View::PuzzleSelect, "the second leaves the puzzle");
    }

    #[test]
    fn the_end_card_is_a_way_back_to_the_menu_by_click_and_by_key() {
        let mut clicked = playing(0);
        clicked.view = View::Completed;
        click_sized(
            &mut clicked,
            Target::Button(Button::Menu),
            MouseButton::Left,
            Crossword::SIZE,
        );
        assert_eq!(clicked.view, View::PuzzleSelect);

        let mut typed = playing(0);
        typed.view = View::Completed;
        typed.key_at(&press(Key::Enter), Crossword::SIZE);
        assert_eq!(typed.view, View::PuzzleSelect);
    }

    // ── The text ───────────────────────────────────────────────────

    #[test]
    fn a_clue_is_handed_to_the_renderer_whole_and_bounded_by_width() {
        // The panel used to cut a clue at `(w / 7.0) - 3` *bytes*: a guessed
        // advance, and a byte offset into a `&str`, which aborts the process
        // the first time a clue holds an accented letter. The renderer is the
        // only thing that knows how wide the text it draws will be, so it is
        // the only thing that may cut it.
        let app = playing(0);
        let drawn = painted_text(&app, Crossword::SIZE);
        let whole: Vec<String> = app
            .clues
            .iter()
            .map(|c| format!("{}{}. {}", c.number, c.direction.initial(), c.text))
            .collect();
        let mut rows = 0usize;
        for (s, max) in &drawn {
            // A clue row is the painted text that opens `<number><A|D>. `.
            // The clock reads "00:00" and would otherwise pass for one.
            let after = s.trim_start_matches(|ch: char| ch.is_ascii_digit());
            if after.len() == s.len() || !(after.starts_with("A. ") || after.starts_with("D. ")) {
                continue;
            }
            rows = rows.saturating_add(1);
            assert!(
                whole.iter().any(|w| w == s),
                "the panel drew {s:?}, which is no clue of this puzzle whole"
            );
            let bound = max.expect("a clue is bounded by the panel it is in");
            assert!(
                bound > 0.0,
                "a clue bounded to nothing is a clue nobody reads"
            );
        }
        assert!(rows > 0, "the panel has to draw some clues");
        assert!(
            drawn.iter().any(|(s, _)| s.starts_with("1A. ")),
            "the panel has to draw the first clue"
        );
    }

    #[test]
    fn a_clue_with_an_accent_in_it_is_drawn_rather_than_aborting() {
        // The old byte-offset cut panicked on the first non-ASCII clue. This
        // is the test that says the cut is the renderer's job now.
        let mut app = playing(0);
        app.clues[0].text = "Café frappé — a naïve piñata, 100 ½ Ω";
        // The panel row, specifically. `contains("piñata")` over the whole
        // frame is also satisfied by the banner, which draws the same clue by
        // another route -- so a cut panel row would pass unnoticed.
        let want = format!("1A. {}", app.clues[0].text);
        let mut reached = 0usize;
        for (w, h) in SIZES {
            app.size = (w, h);
            let drawn = painted_text(&app, (w, h));
            // A 900x60 window has a panel with no room for a row in it.
            if Layout::solve(w, h, app.width, app.height).clue_rows_visible() == 0 {
                continue;
            }
            assert!(
                drawn.iter().any(|(s, _)| *s == want),
                "the accented clue has to reach the renderer whole at {w}x{h}"
            );
            reached = reached.saturating_add(1);
        }
        assert!(reached > 0, "no window in SIZES draws a clue row at all");
    }

    #[test]
    fn the_banner_names_the_word_the_cursor_is_in() {
        let mut app = playing(0);
        app.go_to_clue(0);
        let clue = app.clues[0].clone();
        let drawn = painted_text(&app, Crossword::SIZE);
        let want = format!(
            "{} {} ({}): {}",
            clue.number,
            clue.direction.label(),
            clue.len,
            clue.text
        );
        assert!(
            drawn.iter().any(|(s, _)| *s == want),
            "the banner should read {want:?}; it drew {:?}",
            drawn.iter().map(|(s, _)| s).collect::<Vec<_>>()
        );
        let (_, bound) = drawn.iter().find(|(s, _)| *s == want).unwrap();
        assert!(
            bound.is_some(),
            "a clue is arbitrary text, so the banner has to bound it"
        );
    }

    #[test]
    fn a_heading_is_centred_by_measuring_it_rather_than_by_a_literal() {
        // Every heading used to be centred by subtracting half of one
        // particular string at one particular size: `width / 2.0 - 100.0`,
        // `cx - 60.0`, `bx + bw / 2.0 - 30.0`. Two windows whose widths differ
        // by `d` must move the heading by `d / 2`, whatever the string is.
        let mut narrow = Crossword::new();
        narrow.size = (600.0, 600.0);
        let mut wide = Crossword::new();
        wide.size = (900.0, 600.0);

        let x_of = |app: &Crossword, size: (f32, f32)| {
            app.draw(size)
                .commands()
                .iter()
                .find_map(|c| match c {
                    RenderCommand::Text { x, text, .. } if text == "Crossword Puzzles" => Some(*x),
                    _ => None,
                })
                .expect("the menu draws its heading")
        };
        let a = x_of(&narrow, (600.0, 600.0));
        let b = x_of(&wide, (900.0, 600.0));
        assert!(
            (b - a - 150.0).abs() < 0.51,
            "300 more pixels of window moves a centred heading 150 right: \
             {a} then {b}"
        );
    }

    // ── The menu ───────────────────────────────────────────────────

    #[test]
    fn the_menu_arrows_stay_inside_the_list() {
        let mut app = Crossword::new();
        app.key_at(&press(Key::Up), Crossword::SIZE);
        assert_eq!(app.selected_puzzle, 0, "the first row has nothing above it");
        for _ in 0..10 {
            app.key_at(&press(Key::Down), Crossword::SIZE);
        }
        assert_eq!(
            app.selected_puzzle,
            PUZZLES.len().saturating_sub(1),
            "and the last has nothing below it"
        );
        app.key_at(&press(Key::Enter), Crossword::SIZE);
        assert_eq!(app.view, View::Playing);
        assert_eq!(app.puzzle_name(), PUZZLES[PUZZLES.len() - 1].name);
    }

    #[test]
    fn opening_a_puzzle_starts_it_from_the_beginning() {
        let mut app = playing(0);
        app.cursor = (2, 2);
        app.handle_event(&Event::Tick { elapsed_ms: 9000 });
        app.check_mode = true;
        app.clue_scroll = 2;
        app.cursor = (0, 0);
        app.reveal_letter();

        app.load_puzzle(1);
        assert_eq!(app.elapsed_ms, 0, "a new puzzle starts a new clock");
        assert!(!app.check_mode, "and no marks");
        assert_eq!(app.clue_scroll, 0, "and the top of the clue list");
        assert_eq!(app.revealed_count(), 0, "and nothing given away");
        assert_eq!(app.direction, Direction::Across);
        assert_eq!(
            app.cursor,
            (0, 0),
            "and the cursor on the first playable cell"
        );
    }

    #[test]
    fn a_puzzle_that_does_not_exist_leaves_the_app_where_it_was() {
        let mut app = Crossword::new();
        app.load_puzzle(PUZZLES.len());
        assert_eq!(
            app.view,
            View::PuzzleSelect,
            "there is no fourth puzzle to open"
        );
        assert!(app.cells.is_empty());
    }

    #[test]
    fn a_close_request_closes_the_window() {
        let mut app = playing(0);
        assert!(matches!(
            app.on_event(&Event::CloseRequested),
            Response::Exit
        ));
    }

    #[test]
    fn a_move_asks_for_a_redraw_and_a_dead_key_does_not() {
        // A program that answers `Redraw` to everything repaints the screen
        // for every mouse move; one that answers `Idle` to a move leaves the
        // last frame on the screen. Both are visible, so both are here.
        let mut app = playing(0);
        assert!(matches!(
            app.on_event(&Event::Key(press(Key::S))),
            Response::Redraw
        ));
        assert!(
            matches!(app.on_event(&Event::Key(press(Key::F5))), Response::Idle),
            "a key the puzzle does not use asks for nothing"
        );
    }

    #[test]
    fn render_records_the_window_it_was_given() {
        // The click handler resolves against `self.size`, which nothing but
        // `render` sets. A `render` that did not record it would resolve every
        // click against whatever size the app happened to start at -- which is
        // the shape of the fault this whole app had.
        let mut app = playing(0);
        app.render(1000.0, 700.0);
        assert_eq!(app.size, (1000.0, 700.0));
    }

    #[test]
    fn the_frame_survives_a_window_with_no_area_in_it() {
        // `render` is handed whatever the compositor has, including a window
        // mid-resize with a zero or negative dimension.
        for (w, h) in [(0.0, 0.0), (0.0, 600.0), (600.0, 0.0), (-40.0, -40.0)] {
            let mut app = playing(0);
            app.size = (w, h);
            let frame = app.draw((w, h));
            for r in frame.hits() {
                assert!(!r.1.is_empty(), "a hit box with no area: {:?}", r.1);
            }
        }
    }
}
