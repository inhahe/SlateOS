//! SlateOS Word Search — find hidden words in a grid of letters.
//!
//! Eight placement directions, five word categories, three difficulties, a
//! running clock, five hints, and a board you can either walk with the arrow
//! keys or drag across with the pointer.
//!
//! # What this file used to be
//!
//! It drew a word search and never showed one. `main` built a `WordSearchApp`,
//! bound it to `_app` and returned; nothing opened a window, so no frame was
//! ever submitted and no event ever arrived. Four consequences followed from
//! that one missing line, and every one of them had a test suite passing over
//! it:
//!
//! * **There was no mouse handling of any kind** — no `Event::Mouse` arm
//!   anywhere in the file — in a game whose natural gesture is to drag from a
//!   word's first letter to its last.
//! * **The clock never ran.** `elapsed_secs` was a field, drawn in the header
//!   as `MM:SS`, and incremented by nothing: there was no `Event::Tick` arm
//!   either. The timer read `00:00` for the length of every game.
//! * **`format_time` was dead code.** The header formatted the clock inline
//!   with its own `format!`, so the tested function and the drawn string were
//!   two implementations of one rule and only the untested one shipped. Four
//!   tests covered the copy nobody called.
//! * **A hint stayed lit forever.** `HintHighlight::ticks` counted down from
//!   10 and nothing decremented it, so the renderer's `ticks > 0` test could
//!   never be false.
//!
//! The `#![allow(dead_code)]` at the top of the file is what let the last two
//! ship in silence. It is gone.
//!
//! # Shape
//!
//! [`Layout`] is derived from the live window size on every frame and never
//! stored on the model, so the board is a board and not a picture of one at
//! 780x600. Every control the renderer paints it also records with
//! [`Frame::hit`](guitk::frame::Frame::hit), which is what lets a test click a
//! button by name and what lets the pointer find a cell.

use guitk::color::Color;
use guitk::event::{
    Event, EventResult, Key, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseEventKind,
};
use guitk::frame::Rect;
use guitk::probe::Probe;
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::style::CornerRadii;
use guitk::text;
use oswindow::app::{self, App, Response};
use randrange::{RandomSource, SeededRng, seed_from_system};
use std::cmp::Ordering;
use std::process::ExitCode;
use std::time::Duration;

/// The frame this program draws into, with its own control identifiers.
pub type Frame = guitk::frame::Frame<Target>;

// ── Catppuccin Mocha palette ───────────────────────────────────────────────
const BASE: Color = Color::from_hex(0x1E1E2E);
const MANTLE: Color = Color::from_hex(0x181825);
const SURFACE0: Color = Color::from_hex(0x313244);
const SURFACE1: Color = Color::from_hex(0x45475A);
const TEXT_COLOR: Color = Color::from_hex(0xCDD6F4);
const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const BLUE: Color = Color::from_hex(0x89B4FA);
const GREEN: Color = Color::from_hex(0xA6E3A1);
const RED: Color = Color::from_hex(0xF38BA8);
const YELLOW: Color = Color::from_hex(0xF9E2AF);
const PEACH: Color = Color::from_hex(0xFAB387);
const TEAL: Color = Color::from_hex(0x94E2D5);
const MAUVE: Color = Color::from_hex(0xCBA6F7);
const OVERLAY0: Color = Color::from_hex(0x6C7086);
const LAVENDER: Color = Color::from_hex(0xB4BEFE);

// ── Window and clock ───────────────────────────────────────────────────────

/// The size the window asks for, and the size the tests draw at unless they
/// say otherwise.
pub const WINDOW_WIDTH: f32 = 820.0;
/// See [`WINDOW_WIDTH`].
pub const WINDOW_HEIGHT: f32 = 620.0;

/// How often the clock is woken while a game is being played.
///
/// A second, because a second is what the clock displays; waking any faster
/// would redraw a string that had not changed.
pub const CLOCK_MS: u64 = 1_000;

/// How often the clock is woken while a hint is lit.
///
/// Finer than [`CLOCK_MS`] on purpose. A hint lasts [`HINT_MS`], and a
/// countdown sampled once a second can only expire on a second boundary — so
/// at a one-second interval a 2.5-second hint would last three. The interval a
/// thing is measured with is part of how long it lasts.
pub const HINT_STEP_MS: u64 = 100;

/// How long a hint stays lit.
pub const HINT_MS: u64 = 2_500;

/// How many hints a game starts with.
pub const MAX_HINTS: usize = 5;

/// The keys the board answers, drawn along the bottom.
///
/// A slice and not an array: the length of this table is a fact about the
/// table, and writing it into the type turns "someone added a key and forgot
/// the bar" into a compile error only if they also touched the count — which
/// is the one edit nobody forgets. As a slice it can go wrong the way it
/// really goes wrong, and a test is what stops it.
pub const SHORTCUTS: &[(&str, &str)] = &[
    ("Arrows", "move"),
    ("Enter", "mark"),
    ("Esc", "cancel"),
    ("H", "hint"),
    ("D", "level"),
    ("C", "words"),
    ("F2", "new"),
];

/// The keys the board answers while a selection is open.
pub const SELECTING_SHORTCUTS: &[(&str, &str)] = &[
    ("Arrows", "to end"),
    ("Enter", "confirm"),
    ("Esc", "cancel"),
];

// ── Directions ─────────────────────────────────────────────────────────────

/// One coordinate's contribution to a direction: back one, still, or on one.
///
/// An enum rather than the `-1 | 0 | 1` this was, because every use of it was
/// `(start as i32 + delta * i as i32) as usize` — three casts and a signed
/// multiply to express "the `i`th cell along", in a file whose indices are all
/// unsigned and whose grid is at most twenty wide. [`Step::from`] says the same
/// thing in one `checked_` call and answers `None` for the step that runs off
/// the top or left edge instead of wrapping to `usize::MAX`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// Towards row 0 / column 0.
    Back,
    /// Not at all — the still axis of a horizontal or vertical word.
    Stay,
    /// Away from the origin.
    Fwd,
}

impl Step {
    /// Where `i` steps from `at` lands, or `None` if that is off the low end
    /// or past `usize::MAX`.
    #[must_use]
    pub fn from(self, at: usize, i: usize) -> Option<usize> {
        match self {
            Self::Back => at.checked_sub(i),
            Self::Stay => Some(at),
            Self::Fwd => at.checked_add(i),
        }
    }

    /// The step that undoes this one.
    #[must_use]
    pub fn reversed(self) -> Self {
        match self {
            Self::Back => Self::Fwd,
            Self::Stay => Self::Stay,
            Self::Fwd => Self::Back,
        }
    }
}

/// The eight directions a word may be laid along.
pub const DIRECTIONS: [(Step, Step); 8] = [
    (Step::Stay, Step::Fwd),  // right
    (Step::Stay, Step::Back), // left
    (Step::Fwd, Step::Stay),  // down
    (Step::Back, Step::Stay), // up
    (Step::Fwd, Step::Fwd),   // down-right
    (Step::Fwd, Step::Back),  // down-left
    (Step::Back, Step::Fwd),  // up-right
    (Step::Back, Step::Back), // up-left
];

// ── Word categories ────────────────────────────────────────────────────────

/// Which list of words a puzzle is built from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Category {
    Animals,
    Colors,
    Food,
    Science,
    Geography,
}

impl Category {
    /// Every category, in the order `next` cycles through them.
    pub const ALL: [Category; 5] = [
        Category::Animals,
        Category::Colors,
        Category::Food,
        Category::Science,
        Category::Geography,
    ];

    /// The name shown on the category chip.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Animals => "Animals",
            Self::Colors => "Colors",
            Self::Food => "Food",
            Self::Science => "Science",
            Self::Geography => "Geography",
        }
    }

    /// The accent the category chip is drawn in.
    #[must_use]
    pub fn color(self) -> Color {
        match self {
            Self::Animals => GREEN,
            Self::Colors => MAUVE,
            Self::Food => PEACH,
            Self::Science => TEAL,
            Self::Geography => YELLOW,
        }
    }

    /// The words a puzzle in this category is drawn from.
    ///
    /// Every entry is capital ASCII, which is what lets the grid be a `Vec<u8>`
    /// and a word's byte length be its letter count. That is an assumption, so
    /// a test checks it rather than the file merely asserting it here.
    #[must_use]
    pub fn words(self) -> &'static [&'static str] {
        match self {
            Self::Animals => &[
                "TIGER", "EAGLE", "SHARK", "HORSE", "WHALE", "SNAKE", "PANDA", "ZEBRA", "CAMEL",
                "OTTER", "FALCON", "PARROT", "RABBIT", "TURTLE", "MONKEY", "LIZARD", "SALMON",
                "DOLPHIN", "GIRAFFE", "PENGUIN", "JAGUAR", "COYOTE", "BADGER", "BISON", "CRANE",
                "RAVEN", "VIPER", "MOOSE", "KOALA", "LLAMA",
            ],
            Self::Colors => &[
                "AZURE", "CORAL", "GREEN", "IVORY", "KHAKI", "LILAC", "MAUVE", "OLIVE", "PEACH",
                "ROUGE", "TAUPE", "AMBER", "BLACK", "BROWN", "CREAM", "EBONY", "FROST", "GREY",
                "HAZEL", "LEMON", "MELON", "PEARL", "PLUM", "RUBY", "SAGE", "SAND", "TEAL",
                "WHEAT", "WHITE", "WINE",
            ],
            Self::Food => &[
                "BREAD", "CHEESE", "GRAPE", "LEMON", "MELON", "OLIVE", "PEACH", "PIZZA", "SALAD",
                "STEAK", "SUSHI", "TACOS", "TOAST", "MANGO", "CREPE", "PASTA", "CURRY", "BACON",
                "BERRY", "CANDY", "CHIPS", "DONUT", "HONEY", "JUICE", "MAPLE", "ONION", "RICE",
                "SOUP", "BASIL", "THYME",
            ],
            Self::Science => &[
                "ATOM", "CELL", "FORCE", "LASER", "ORBIT", "PRISM", "QUARK", "SOLAR", "VAPOR",
                "XENON", "DIODE", "FIELD", "GAMMA", "HELIX", "IONIC", "JOULE", "KELVIN", "LOGIC",
                "MOLAR", "NERVE", "OPTIC", "PHASE", "RADAR", "SIGMA", "TESLA", "ALLOY", "DECAY",
                "FLORA", "GENES", "HERTZ",
            ],
            Self::Geography => &[
                "DELTA", "FJORD", "RIDGE", "BASIN", "CLIFF", "DUNES", "GORGE", "PLAIN", "RIVER",
                "TIDAL", "ATLAS", "COAST", "GROVE", "MARSH", "OASIS", "PEAKS", "SHOAL", "TROPIC",
                "VALLEY", "BAYOU", "CANAL", "GULLY", "ISLAND", "NORTH", "SOUTH", "OCEAN", "POLAR",
                "STEPPE", "TUNDRA", "CREEK",
            ],
        }
    }

    /// The next category the `C` key and the category chip move to.
    #[must_use]
    pub fn next(self) -> Self {
        match self {
            Self::Animals => Self::Colors,
            Self::Colors => Self::Food,
            Self::Food => Self::Science,
            Self::Science => Self::Geography,
            Self::Geography => Self::Animals,
        }
    }
}

// ── Difficulty ─────────────────────────────────────────────────────────────

/// How big the board is and how many words hide on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Difficulty {
    Easy,
    Medium,
    Hard,
}

impl Difficulty {
    /// Every difficulty, in the order `next` cycles through them.
    pub const ALL: [Difficulty; 3] = [Difficulty::Easy, Difficulty::Medium, Difficulty::Hard];

    /// The board's side, in cells.
    #[must_use]
    pub fn grid_size(self) -> usize {
        match self {
            Self::Easy => 10,
            Self::Medium => 15,
            Self::Hard => 20,
        }
    }

    /// How many words a puzzle at this difficulty tries to place.
    #[must_use]
    pub fn word_count(self) -> usize {
        match self {
            Self::Easy => 8,
            Self::Medium => 10,
            Self::Hard => 12,
        }
    }

    /// The name shown on the difficulty chip.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Easy => "Easy",
            Self::Medium => "Medium",
            Self::Hard => "Hard",
        }
    }

    /// The accent the difficulty chip is drawn in.
    #[must_use]
    pub fn color(self) -> Color {
        match self {
            Self::Easy => GREEN,
            Self::Medium => YELLOW,
            Self::Hard => RED,
        }
    }

    /// The next difficulty the `D` key and the difficulty chip move to.
    #[must_use]
    pub fn next(self) -> Self {
        match self {
            Self::Easy => Self::Medium,
            Self::Medium => Self::Hard,
            Self::Hard => Self::Easy,
        }
    }
}

// ── A word on the board ────────────────────────────────────────────────────

/// A word that was placed on the grid, with where it starts and which way it
/// runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlacedWord {
    /// The word itself, capital ASCII.
    pub word: String,
    /// `(row, column)` of its first letter.
    pub start: (usize, usize),
    /// Which way its letters run.
    pub dir: (Step, Step),
    /// Whether the player has found it.
    pub found: bool,
}

impl PlacedWord {
    /// The cells this word occupies, first letter first.
    ///
    /// The direction is stored as a pair of [`Step`]s rather than an index into
    /// [`DIRECTIONS`], so there is no lookup that can fail and no bound to
    /// re-check: an index would need a guard here that the constructor already
    /// keeps, which is one guard too many (`known-issues.md` lesson 51).
    #[must_use]
    pub fn cells(&self) -> Vec<(usize, usize)> {
        let (dr, dc) = self.dir;
        (0..self.word.len())
            .map_while(|i| Some((dr.from(self.start.0, i)?, dc.from(self.start.1, i)?)))
            .collect()
    }
}

// ── Game state ─────────────────────────────────────────────────────────────

/// Whether there is anything left to find.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameStatus {
    Playing,
    Won,
}

/// Whether a word is being marked out, and where it was started.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Selection {
    /// Nothing is being marked.
    None,
    /// A start cell has been chosen; the cursor is the other end.
    From(usize, usize),
}

/// A letter lit by a hint, and how long it has left.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hint {
    pub row: usize,
    pub col: usize,
    /// Milliseconds of light remaining.
    pub remaining_ms: u64,
}

/// Every control the renderer paints, and the name a click on it is answered
/// by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// A cell of the board, by `(row, column)`.
    Cell(usize, usize),
    /// A row of the word list, by its index into the puzzle's words.
    Word(usize),
    /// The difficulty chip.
    Difficulty,
    /// The category chip.
    Category,
    /// The hint chip.
    HintButton,
    /// The new-game chip.
    NewGame,
}

/// Everything the board can be asked to do, from a key or from the pointer.
///
/// One enum for both so that "click the hint chip" and "press H" are the same
/// operation rather than two copies of it, and so a mutation of the rule shows
/// up whichever way a test drives it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Move the cursor one cell.
    Move(Step, Step),
    /// Put the cursor on a specific cell.
    Goto(usize, usize),
    /// Enter: start marking at the cursor, or finish marking there.
    Anchor,
    /// Pointer press: start marking at a cell.
    Begin(usize, usize),
    /// Pointer drag: move the far end of the mark.
    Extend(usize, usize),
    /// Pointer release: finish a mark the pointer started.
    Finish,
    /// Throw the current mark away.
    Cancel,
    /// Light the first letter of the first word still missing.
    UseHint,
    /// Light the first letter of a named word.
    HintFor(usize),
    /// Start a new game one difficulty on.
    CycleDifficulty,
    /// Start a new game one category on.
    CycleCategory,
    /// Start a new game at a named difficulty.
    SetDifficulty(Difficulty),
    /// Start a new game at the same settings.
    NewGame,
}

/// The whole game.
pub struct WordSearchApp {
    /// The letters, row-major, `grid_size` wide. `0` means "not filled yet".
    grid: Vec<u8>,
    grid_size: usize,
    placed_words: Vec<PlacedWord>,
    difficulty: Difficulty,
    category: Category,
    cursor: (usize, usize),
    selection: Selection,
    /// Whether the pointer is mid-drag. `Selection` alone cannot say: a
    /// selection opened with Enter must not be finished by a stray release.
    dragging: bool,
    status: GameStatus,
    /// The clock, in milliseconds, because that is the unit ticks arrive in.
    /// Seconds are derived for display and never stored — a counter bumped by
    /// one per tick is a rate with no denominator, which is how the mixer's
    /// meters came to have four.
    elapsed_ms: u64,
    hints_remaining: usize,
    rng: SeededRng,
    hint: Option<Hint>,
    seed: u64,
    /// The size the last frame was drawn at, which is the size the next click
    /// is read against.
    size: (f32, f32),
}

impl WordSearchApp {
    /// A game seeded from the system, falling back to a fixed seed where there
    /// is no entropy source to ask.
    #[must_use]
    pub fn new() -> Self {
        Self::with_seed(seed_from_system(0x5EED_0FAB_0A5D_u64))
    }

    /// A game with a chosen seed, so a test can name the puzzle it means.
    #[must_use]
    pub fn with_seed(seed: u64) -> Self {
        let mut app = Self {
            grid: Vec::new(),
            grid_size: Difficulty::Medium.grid_size(),
            placed_words: Vec::new(),
            difficulty: Difficulty::Medium,
            category: Category::Animals,
            cursor: (0, 0),
            selection: Selection::None,
            dragging: false,
            status: GameStatus::Playing,
            elapsed_ms: 0,
            hints_remaining: MAX_HINTS,
            rng: SeededRng::new(seed),
            hint: None,
            seed,
            size: (WINDOW_WIDTH, WINDOW_HEIGHT),
        };
        app.generate_puzzle();
        app
    }

    // ── What the outside can ask ───────────────────────────────────────────

    /// The board's side, in cells.
    #[must_use]
    pub fn grid_size(&self) -> usize {
        self.grid_size
    }

    /// The letter at `(row, col)`, or `None` off the board. `0` is a cell no
    /// word and no filler has reached yet, which only happens mid-generation.
    #[must_use]
    pub fn letter(&self, row: usize, col: usize) -> Option<u8> {
        // The column bound is the load-bearing one: without it, column
        // `grid_size` of row 0 is column 0 of row 1 -- a perfectly valid index
        // into a flat grid, and a silently wrong letter. A row past the end
        // needs no separate check, because it runs off the end of the vector
        // and `get` says so. Two checks of one bound is one check and one
        // place a fault can hide (lesson 51).
        if col >= self.grid_size {
            return None;
        }
        let index = row.checked_mul(self.grid_size)?.checked_add(col)?;
        self.grid.get(index).copied()
    }

    /// The words hidden in this puzzle, in the order the list draws them.
    #[must_use]
    pub fn words(&self) -> &[PlacedWord] {
        &self.placed_words
    }

    /// The `i`th hidden word.
    #[must_use]
    pub fn word_at_index(&self, i: usize) -> Option<&PlacedWord> {
        self.placed_words.get(i)
    }

    /// How many words have been found.
    #[must_use]
    pub fn found_count(&self) -> usize {
        self.placed_words.iter().filter(|w| w.found).count()
    }

    /// How many words are hidden in this puzzle.
    #[must_use]
    pub fn total_words(&self) -> usize {
        self.placed_words.len()
    }

    /// Where the cursor is.
    #[must_use]
    pub fn cursor(&self) -> (usize, usize) {
        self.cursor
    }

    /// What is being marked out, if anything.
    #[must_use]
    pub fn selection(&self) -> Selection {
        self.selection
    }

    /// Whether the pointer is mid-drag.
    #[must_use]
    pub fn dragging(&self) -> bool {
        self.dragging
    }

    /// Whether anything is left to find.
    #[must_use]
    pub fn status(&self) -> GameStatus {
        self.status
    }

    /// The current difficulty.
    #[must_use]
    pub fn difficulty(&self) -> Difficulty {
        self.difficulty
    }

    /// The current category.
    #[must_use]
    pub fn category(&self) -> Category {
        self.category
    }

    /// How many hints are left.
    #[must_use]
    pub fn hints_remaining(&self) -> usize {
        self.hints_remaining
    }

    /// The lit hint, if one is lit.
    #[must_use]
    pub fn hint(&self) -> Option<Hint> {
        self.hint
    }

    /// The clock, in whole seconds — derived from the milliseconds ticks
    /// arrive in, never counted separately.
    #[must_use]
    pub fn elapsed_secs(&self) -> u64 {
        self.elapsed_ms / 1_000
    }

    /// The clock, in milliseconds.
    #[must_use]
    pub fn elapsed_ms(&self) -> u64 {
        self.elapsed_ms
    }

    /// The cells the mark currently covers, empty when nothing is marked or
    /// when the two ends do not lie on a line a word could.
    #[must_use]
    pub fn marked_cells(&self) -> Vec<(usize, usize)> {
        match self.selection {
            Selection::None => Vec::new(),
            Selection::From(r, c) => cells_between((r, c), self.cursor).unwrap_or_default(),
        }
    }

    /// Whether `(row, col)` belongs to a word already found.
    ///
    /// Derived from the words rather than kept alongside them. A parallel
    /// `found_cells: Vec<_>` is a second copy of a fact the words already
    /// carry, and a copy is a thing that can disagree — the whole reason the
    /// clock here is milliseconds and the seconds are computed.
    #[must_use]
    pub fn is_found_cell(&self, row: usize, col: usize) -> bool {
        self.placed_words
            .iter()
            .any(|w| w.found && w.cells().contains(&(row, col)))
    }

    // ── Puzzle generation ──────────────────────────────────────────────────

    /// Throw the board away and build a new one at the given settings.
    pub fn new_game(&mut self, difficulty: Difficulty, category: Category) {
        self.difficulty = difficulty;
        self.category = category;
        self.cursor = (0, 0);
        self.selection = Selection::None;
        self.dragging = false;
        self.status = GameStatus::Playing;
        self.elapsed_ms = 0;
        self.hints_remaining = MAX_HINTS;
        self.hint = None;
        // A fresh generator per puzzle, so a seed names a board rather than a
        // point in a stream whose position depends on how much was drawn from
        // it before.
        self.seed = self.seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
        self.rng = SeededRng::new(self.seed);
        self.generate_puzzle();
    }

    fn generate_puzzle(&mut self) {
        let size = self.difficulty.grid_size();
        self.grid_size = size;
        self.grid = vec![0u8; size.saturating_mul(size)];
        self.placed_words.clear();

        let all = self.category.words();
        let wanted = self.difficulty.word_count();

        let mut order: Vec<usize> = (0..all.len()).collect();
        self.rng.shuffle(&mut order);

        for &i in &order {
            if self.placed_words.len() >= wanted {
                break;
            }
            let Some(&word) = all.get(i) else { continue };
            // There is no `if word.len() > size { continue }` here, and there
            // used to be. `try_place_word` already answers exactly the same
            // thing for a word that fits nowhere: `can_place` rejects every one
            // of the eight directions at every start, `spots` comes back empty,
            // and `rng.below(0)` returns 0 without drawing — so the two paths
            // agree down to the random stream, and the guard could not change a
            // board even in principle. The invariant it was quietly assuming --
            // that no word is longer than the smallest board -- is asserted
            // where it belongs, as a fact about the word lists themselves
            // (`every_category_word_is_capital_ascii_so_a_byte_is_a_letter`).
            self.try_place_word(word);
        }

        // Everything the words did not reach becomes filler.
        for cell in &mut self.grid {
            if *cell == 0 {
                *cell = random_letter(&mut self.rng);
            }
        }
    }

    /// Put `word` somewhere it fits, chosen uniformly among every place it
    /// fits. Answers whether it went anywhere.
    fn try_place_word(&mut self, word: &str) -> bool {
        let bytes = word.as_bytes();
        let mut spots: Vec<((usize, usize), (Step, Step))> = Vec::new();
        for dir in DIRECTIONS {
            for row in 0..self.grid_size {
                for col in 0..self.grid_size {
                    if self.can_place(bytes, (row, col), dir) {
                        spots.push(((row, col), dir));
                    }
                }
            }
        }

        let Some(&(start, dir)) = spots.get(self.rng.below(spots.len())) else {
            return false;
        };

        for (i, &ch) in bytes.iter().enumerate() {
            let (Some(r), Some(c)) = (dir.0.from(start.0, i), dir.1.from(start.1, i)) else {
                continue;
            };
            self.set_letter(r, c, ch);
        }

        self.placed_words.push(PlacedWord {
            word: String::from(word),
            start,
            dir,
            found: false,
        });
        true
    }

    /// Whether `word` fits at `at` running along `dir`: every cell on the
    /// board, and every cell either empty or already carrying the right
    /// letter.
    fn can_place(&self, word: &[u8], at: (usize, usize), dir: (Step, Step)) -> bool {
        if word.is_empty() {
            return false;
        }
        word.iter().enumerate().all(|(i, &ch)| {
            let (Some(r), Some(c)) = (dir.0.from(at.0, i), dir.1.from(at.1, i)) else {
                return false;
            };
            match self.letter(r, c) {
                Some(existing) => existing == 0 || existing == ch,
                None => false,
            }
        })
    }

    fn set_letter(&mut self, row: usize, col: usize, ch: u8) -> bool {
        if col >= self.grid_size {
            return false;
        }
        let Some(index) = row
            .checked_mul(self.grid_size)
            .and_then(|i| i.checked_add(col))
        else {
            return false;
        };
        let Some(slot) = self.grid.get_mut(index) else {
            return false;
        };
        *slot = ch;
        true
    }

    // ── Marking a word ─────────────────────────────────────────────────────

    /// The index of the word lying exactly between `from` and `to`, in either
    /// direction.
    ///
    /// Matched on **cells**, not on spelling. This used to check both — that
    /// the letters under the mark spelled the word, *and* that the cells were
    /// the word's own cells — which is two checks of one invariant, because
    /// standing on a word's cells implies reading its letters. The spelling
    /// check was the redundant copy, and with it there a mutation of the cell
    /// comparison changed nothing any test could see. See lesson 51.
    ///
    /// It also used to skip words already found. That was a third copy of the
    /// same story: two distinct words cannot occupy the same cells, so the only
    /// thing the skip could ever suppress is re-finding a word that is already
    /// found — which changes no count, no colour and no status, because
    /// [`Self::mark_found`] is idempotent. A guard nothing can reach is a place
    /// for a fault to hide, so it is gone.
    #[must_use]
    pub fn word_between(&self, from: (usize, usize), to: (usize, usize)) -> Option<usize> {
        let cells = cells_between(from, to)?;
        let backwards: Vec<(usize, usize)> = cells.iter().rev().copied().collect();
        self.placed_words.iter().position(|placed| {
            let own = placed.cells();
            own == cells || own == backwards
        })
    }

    fn mark_found(&mut self, index: usize) {
        let Some(placed) = self.placed_words.get_mut(index) else {
            return;
        };
        placed.found = true;
        if self.placed_words.iter().all(|w| w.found) {
            self.status = GameStatus::Won;
        }
    }

    fn confirm(&mut self) -> EventResult {
        let Selection::From(r, c) = self.selection else {
            return EventResult::Ignored;
        };
        if let Some(index) = self.word_between((r, c), self.cursor) {
            self.mark_found(index);
        }
        self.selection = Selection::None;
        EventResult::Consumed
    }

    // ── Hints ──────────────────────────────────────────────────────────────

    /// Light the first letter of word `index`, spending a hint.
    ///
    /// Naming the word is the pointer's gesture — click it in the list — and it
    /// is also what stops the hint being useless twice over: the keyboard's `H`
    /// used to be the *only* way to ask, and it always revealed the first
    /// unfound word in placement order, so a second hint on the same word said
    /// exactly what the first had.
    ///
    /// It used to open with `self.status == GameStatus::Won || ..`, and the
    /// mutation sweep proved that disjunct unreachable: `Won` is set in exactly
    /// one place, when *every* word is found, so a won board has no unfound
    /// word for the `placed.found` test below to admit. Deleting the
    /// asked-for-a-hint-after-winning case failed no test — not because the
    /// suite is thin, but because the case cannot arise
    /// (`known-issues.md` lesson 51). The behaviour it named is still asserted;
    /// it is now guaranteed by the one check that does the work.
    fn hint_for(&mut self, index: usize) -> EventResult {
        if self.hints_remaining == 0 {
            return EventResult::Ignored;
        }
        let Some(placed) = self.placed_words.get(index) else {
            return EventResult::Ignored;
        };
        if placed.found {
            return EventResult::Ignored;
        }
        let Some(&(row, col)) = placed.cells().first() else {
            return EventResult::Ignored;
        };
        self.hint = Some(Hint {
            row,
            col,
            remaining_ms: HINT_MS,
        });
        self.hints_remaining = self.hints_remaining.saturating_sub(1);
        EventResult::Consumed
    }

    // ── The clock ──────────────────────────────────────────────────────────

    /// Advance the clock and the lit hint by `elapsed_ms` of wall time.
    ///
    /// Both take the elapsed time as an argument rather than counting calls,
    /// so the interval the window happens to wake at cannot change how fast
    /// the game's clock runs.
    pub fn tick(&mut self, elapsed_ms: u64) -> EventResult {
        let mut moved = EventResult::Ignored;
        if self.status == GameStatus::Playing {
            self.elapsed_ms = self.elapsed_ms.saturating_add(elapsed_ms);
            moved = EventResult::Consumed;
        }
        if let Some(hint) = &mut self.hint {
            hint.remaining_ms = hint.remaining_ms.saturating_sub(elapsed_ms);
            if hint.remaining_ms == 0 {
                self.hint = None;
            }
            moved = EventResult::Consumed;
        }
        moved
    }

    /// Whether anything on screen is still moving on its own.
    #[must_use]
    pub fn animating(&self) -> bool {
        self.hint.is_some() || self.status == GameStatus::Playing
    }

    // ── Doing things ───────────────────────────────────────────────────────

    /// Carry out one action. Every key and every click funnels through here.
    #[allow(
        clippy::too_many_lines,
        reason = "one arm per action reads better in one place than split across \
                  helpers that each have one caller"
    )]
    pub fn apply(&mut self, action: Action) -> EventResult {
        match action {
            Action::Move(dr, dc) => {
                let (Some(r), Some(c)) = (dr.from(self.cursor.0, 1), dc.from(self.cursor.1, 1))
                else {
                    return EventResult::Ignored;
                };
                self.apply(Action::Goto(r, c))
            }
            Action::Goto(row, col) => {
                // The one bound the cursor has. `Move` does not repeat it --
                // it delegates here -- so there is exactly one place that
                // decides what is on the board.
                if row >= self.grid_size || col >= self.grid_size {
                    return EventResult::Ignored;
                }
                if self.cursor == (row, col) {
                    return EventResult::Ignored;
                }
                self.cursor = (row, col);
                EventResult::Consumed
            }
            Action::Anchor => {
                if self.status == GameStatus::Won {
                    return EventResult::Ignored;
                }
                match self.selection {
                    Selection::None => {
                        self.selection = Selection::From(self.cursor.0, self.cursor.1);
                        EventResult::Consumed
                    }
                    Selection::From(..) => self.confirm(),
                }
            }
            Action::Begin(row, col) => {
                if self.status == GameStatus::Won {
                    return EventResult::Ignored;
                }
                if row >= self.grid_size || col >= self.grid_size {
                    return EventResult::Ignored;
                }
                self.cursor = (row, col);
                self.selection = Selection::From(row, col);
                self.dragging = true;
                EventResult::Consumed
            }
            Action::Extend(row, col) => {
                // A drag only moves the far end while a drag is in progress.
                // Without this the pointer would drag the cursor around
                // whenever it crossed the board, selection or no selection.
                if !self.dragging {
                    return EventResult::Ignored;
                }
                self.apply(Action::Goto(row, col))
            }
            Action::Finish => {
                if !self.dragging {
                    return EventResult::Ignored;
                }
                self.dragging = false;
                self.confirm()
            }
            Action::Cancel => {
                if self.selection == Selection::None && !self.dragging {
                    return EventResult::Ignored;
                }
                self.selection = Selection::None;
                self.dragging = false;
                EventResult::Consumed
            }
            Action::UseHint => {
                let Some(index) = self.placed_words.iter().position(|w| !w.found) else {
                    return EventResult::Ignored;
                };
                self.hint_for(index)
            }
            Action::HintFor(index) => self.hint_for(index),
            Action::CycleDifficulty => {
                self.new_game(self.difficulty.next(), self.category);
                EventResult::Consumed
            }
            Action::CycleCategory => {
                self.new_game(self.difficulty, self.category.next());
                EventResult::Consumed
            }
            Action::SetDifficulty(difficulty) => {
                self.new_game(difficulty, self.category);
                EventResult::Consumed
            }
            Action::NewGame => {
                self.new_game(self.difficulty, self.category);
                EventResult::Consumed
            }
        }
    }

    /// What a key means, or `None` for the keys this program does not answer.
    #[must_use]
    pub fn action_for_key(&self, key: &KeyEvent) -> Option<Action> {
        if !key.pressed {
            return None;
        }
        if key.modifiers.ctrl {
            return match key.key {
                Key::Num1 => Some(Action::SetDifficulty(Difficulty::Easy)),
                Key::Num2 => Some(Action::SetDifficulty(Difficulty::Medium)),
                Key::Num3 => Some(Action::SetDifficulty(Difficulty::Hard)),
                _ => None,
            };
        }
        if key.modifiers != Modifiers::NONE {
            return None;
        }
        match key.key {
            Key::Up => Some(Action::Move(Step::Back, Step::Stay)),
            Key::Down => Some(Action::Move(Step::Fwd, Step::Stay)),
            Key::Left => Some(Action::Move(Step::Stay, Step::Back)),
            Key::Right => Some(Action::Move(Step::Stay, Step::Fwd)),
            Key::Enter => Some(Action::Anchor),
            Key::Escape => Some(Action::Cancel),
            Key::H => Some(Action::UseHint),
            Key::D => Some(Action::CycleDifficulty),
            Key::C => Some(Action::CycleCategory),
            Key::F2 => Some(Action::NewGame),
            _ => None,
        }
    }

    fn handle_key(&mut self, key: &KeyEvent) -> EventResult {
        match self.action_for_key(key) {
            Some(action) => self.apply(action),
            None => EventResult::Ignored,
        }
    }

    fn handle_mouse(&mut self, event: &MouseEvent) -> EventResult {
        let hit = self
            .frame(self.size.0, self.size.1)
            .hit_test(event.x, event.y);
        match event.kind {
            MouseEventKind::Press(MouseButton::Left) => match hit {
                Some(Target::Cell(row, col)) => self.apply(Action::Begin(row, col)),
                Some(Target::Word(index)) => self.apply(Action::HintFor(index)),
                Some(Target::Difficulty) => self.apply(Action::CycleDifficulty),
                Some(Target::Category) => self.apply(Action::CycleCategory),
                Some(Target::HintButton) => self.apply(Action::UseHint),
                Some(Target::NewGame) => self.apply(Action::NewGame),
                None => EventResult::Ignored,
            },
            MouseEventKind::Move => match hit {
                Some(Target::Cell(row, col)) => self.apply(Action::Extend(row, col)),
                _ => EventResult::Ignored,
            },
            MouseEventKind::Release(MouseButton::Left) => self.apply(Action::Finish),
            _ => EventResult::Ignored,
        }
    }

    /// Remember the size the window is now, so the next click is read against
    /// the frame the player is actually looking at.
    pub fn resize(&mut self, width: f32, height: f32) {
        self.size = (width.max(1.0), height.max(1.0));
    }

    /// The size the last frame was drawn at.
    #[must_use]
    pub fn size(&self) -> (f32, f32) {
        self.size
    }
}

impl Default for WordSearchApp {
    fn default() -> Self {
        Self::new()
    }
}

// ── Free-standing rules ────────────────────────────────────────────────────

/// A random capital ASCII letter.
///
/// A free function rather than a generator method because an alphabet is this
/// game's unit, not the generator's.
fn random_letter(rng: &mut SeededRng) -> u8 {
    // 26 fits in a `u8` and `below` never returns its bound, so the sum is at
    // most b'Z'. `saturating_add` says so without an `unwrap`.
    let offset = u8::try_from(rng.below(26)).unwrap_or(0);
    b'A'.saturating_add(offset)
}

/// The cells from `from` to `to` inclusive, or `None` when the two do not lie
/// on a line a word could be laid along.
///
/// Horizontal, vertical, or exactly 45 degrees — the same eight directions
/// [`DIRECTIONS`] offers, which is not a coincidence but the point.
#[must_use]
pub fn cells_between(from: (usize, usize), to: (usize, usize)) -> Option<Vec<(usize, usize)>> {
    let (sr, sc) = from;
    let (er, ec) = to;
    if from == to {
        // One cell is a line of length one. It matches no word -- every word
        // is at least three letters -- but it is a legal thing for a player
        // mid-drag to be pointing at, and answering `None` here would make the
        // preview flicker off every time the drag returned to its own start.
        return Some(vec![from]);
    }

    let dr = er.abs_diff(sr);
    let dc = ec.abs_diff(sc);
    let steps = if dr == 0 {
        dc
    } else if dc == 0 || dr == dc {
        dr
    } else {
        return None;
    };

    let mut cells = Vec::with_capacity(steps.saturating_add(1));
    for i in 0..=steps {
        cells.push((walk(sr, er, i), walk(sc, ec, i)));
    }
    Some(cells)
}

/// `start` moved `i` places towards `end`, stopping at `end`'s side of the
/// axis. Unsigned throughout, so there is no cast to get the sign of wrong.
///
/// The three cases are three cases and not two. `end == start` is the still
/// axis of a horizontal or vertical line, and folding it in with `end < start`
/// walks it *backwards*: the rows of the line from `(2, 3)` to `(2, 7)` came
/// out `2, 1, 0, 0, 0`, so every horizontal mark not on row 0 previewed a
/// staircase running off the top of the board -- and no word laid along a row
/// below the first could be marked at all. Written as `end > start` / `else`
/// it looks like a two-way choice about direction; it is a three-way one, and
/// the third way is "do not move".
fn walk(start: usize, end: usize, i: usize) -> usize {
    match end.cmp(&start) {
        Ordering::Greater => start.saturating_add(i),
        Ordering::Less => start.saturating_sub(i),
        Ordering::Equal => start,
    }
}

/// Seconds as `MM:SS`.
///
/// One implementation, and the header calls it. It used to be a free function
/// with four tests and no callers, while the header formatted the clock inline
/// with its own `format!` — so the tested copy and the drawn copy were two
/// implementations of one rule, and the tests covered the one that never
/// reached a screen.
#[must_use]
pub fn format_time(secs: u64) -> String {
    format!("{:02}:{:02}", secs / 60, secs % 60)
}

// ── Layout ─────────────────────────────────────────────────────────────────

/// Which band gives up its height first when the window is too short for all
/// of them. The board is not in the list: a word search with no board is not a
/// smaller word search, it is a blank window.
const BAND_DROP_ORDER: [usize; 2] = [1, 0];

/// How much of the window's height the board insists on keeping.
const BOARD_SHARE: f32 = 0.55;

/// How many chips sit in the header.
const CHIPS: usize = 4;

/// Where everything goes, derived from the live window size on every frame and
/// never stored on the model.
///
/// The old file had no layout at all: `render` computed a `total_w` and
/// `total_h` from the grid's cell count and painted a background that size,
/// regardless of how big the window actually was. Nothing in it could answer
/// "which cell is under the pointer", which is one reason there was no pointer
/// handling to ask.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub window: Rect,
    /// The strip along the top holding the title, the clock and the chips.
    pub header: Rect,
    /// The area the board is centred in.
    pub board: Rect,
    /// The column the word list runs down.
    pub list: Rect,
    /// The shortcut strip along the bottom.
    pub footer: Rect,
    /// The side of one cell.
    pub cell: f32,
    /// The space between two cells.
    pub gap: f32,
    /// The board's side in cells, so `cell_rect` cannot invent one.
    pub size: usize,
    /// How many words the list has, so `word_row` cannot invent one.
    pub words: usize,
    pub font: f32,
    pub big: f32,
    pub pad: f32,
}

impl Layout {
    /// The layout for a window of the given size holding a `size`-square board
    /// and a list of `words` words.
    #[must_use]
    #[allow(
        clippy::cast_precision_loss,
        reason = "a board is at most 20 cells and a list at most 12 words; both \
                  are exact in f32"
    )]
    pub fn new(width: f32, height: f32, size: usize, words: usize) -> Self {
        let w = width.max(1.0);
        let h = height.max(1.0);
        let font = (h / 42.0).clamp(7.0, 15.0);
        let big = (font * 1.5).clamp(10.0, 24.0);
        // A margin may never be more than a quarter of the side it is taken
        // from: a two-pixel floor is wider than a 1x1 window, and a margin that
        // does not fit inside the thing it is a margin of puts the content it
        // indents outside the window.
        let pad = (w.min(h) * 0.015).clamp(2.0, 14.0).min(w.min(h) / 4.0);

        // What each band would like, in [header, footer] order.
        let mut wants = [(h * 0.11).clamp(26.0, 58.0), (h * 0.055).clamp(16.0, 28.0)];
        let budget = (h - h * BOARD_SHARE - pad * 2.0).max(0.0);
        for &i in &BAND_DROP_ORDER {
            if wants.iter().sum::<f32>() <= budget {
                break;
            }
            if let Some(band) = wants.get_mut(i) {
                *band = 0.0;
            }
        }
        let [head_h, foot_h] = wants;

        // A dropped band is `Rect::EMPTY`, not a full-width strip nought pixels
        // tall. Both look the same to a fill, but only one of them looks the
        // same to a reader asking "is this band gone, or merely thin?"
        let header = if head_h > 0.0 {
            Rect::new(0.0, 0.0, w, head_h)
        } else {
            Rect::EMPTY
        };
        let footer = if foot_h > 0.0 {
            Rect::new(0.0, h - foot_h, w, foot_h)
        } else {
            Rect::EMPTY
        };

        // From the heights, not from `header.bottom()`: a dropped band's bottom
        // is zero, which is right by accident today and wrong the moment
        // `BAND_DROP_ORDER` is reordered.
        let middle = Rect::new(
            pad,
            head_h + pad,
            (w - pad * 2.0).max(0.0),
            (h - foot_h - head_h - pad * 2.0).max(0.0),
        );

        // The list gets a fixed share of the width until that share stops being
        // wide enough to read a word in, at which point it goes entirely and
        // the board takes the room. A list too narrow for its words is worse
        // than no list: it is a column of ellipses.
        let want_list = (middle.w * 0.24).clamp(0.0, 190.0);
        let list_w = if want_list >= big * 3.0 {
            want_list
        } else {
            0.0
        };
        let split = if list_w > 0.0 { pad } else { 0.0 };
        let board = Rect::new(
            middle.x,
            middle.y,
            (middle.w - list_w - split).max(0.0),
            middle.h,
        );
        let list = if list_w > 0.0 {
            Rect::new(board.right() + split, middle.y, list_w, middle.h)
        } else {
            Rect::EMPTY
        };

        // Square cells, so a diagonal is a diagonal. The gap may take at most
        // half the board's shorter side, which leaves the other half to be
        // divided among the cells and keeps the last cell's far edge on the
        // board's.
        let side = board.w.min(board.h);
        let gaps = size.saturating_sub(1) as f32;
        let gap = (side * 0.006).clamp(1.0, 4.0).min(if gaps > 0.0 {
            side / (2.0 * gaps)
        } else {
            f32::INFINITY
        });
        let cell = if size == 0 {
            0.0
        } else {
            ((side - gap * gaps) / size as f32).max(0.0)
        };

        Self {
            window: Rect::new(0.0, 0.0, w, h),
            header,
            board,
            list,
            footer,
            cell,
            gap,
            size,
            words,
            font,
            big,
            pad,
        }
    }

    /// The square the cells actually occupy, centred in [`Layout::board`].
    #[must_use]
    #[allow(
        clippy::cast_precision_loss,
        reason = "a board is at most 20 cells, which is exact in f32"
    )]
    pub fn grid_rect(&self) -> Rect {
        if self.size == 0 || self.cell <= 0.0 {
            return Rect::EMPTY;
        }
        let side = self.cell * self.size as f32 + self.gap * self.size.saturating_sub(1) as f32;
        Rect::new(
            self.board.x + (self.board.w - side) / 2.0,
            self.board.y + (self.board.h - side) / 2.0,
            side,
            side,
        )
    }

    /// The box cell `(row, col)` is drawn in, or [`Rect::EMPTY`] for a cell
    /// this board does not have.
    #[must_use]
    #[allow(
        clippy::cast_precision_loss,
        reason = "a board is at most 20 cells, which is exact in f32"
    )]
    pub fn cell_rect(&self, row: usize, col: usize) -> Rect {
        if row >= self.size || col >= self.size || self.cell <= 0.0 {
            return Rect::EMPTY;
        }
        let grid = self.grid_rect();
        let pitch = self.cell + self.gap;
        Rect::new(
            grid.x + col as f32 * pitch,
            grid.y + row as f32 * pitch,
            self.cell,
            self.cell,
        )
    }

    /// The `i`th chip in the header, or [`Rect::EMPTY`] when there is no
    /// header to put it in.
    #[must_use]
    #[allow(
        clippy::cast_precision_loss,
        reason = "there are four chips, which is exact in f32"
    )]
    pub fn chip(&self, i: usize) -> Rect {
        if self.header.is_empty() || i >= CHIPS {
            return Rect::EMPTY;
        }
        let inset = self.pad;
        let gap = (inset * 0.5).min(6.0);
        let usable = (self.header.w * 0.58 - inset * 2.0).max(0.0);
        let span = usable - gap * (CHIPS.saturating_sub(1)) as f32;
        if span <= 0.0 {
            return Rect::EMPTY;
        }
        let cw = span / CHIPS as f32;
        let x0 = self.header.right() - inset - usable;
        Rect::new(
            x0 + i as f32 * (cw + gap),
            self.header.y + inset * 0.5,
            cw,
            (self.header.h - inset).max(0.0),
        )
    }

    /// The `i`th row of the word list, or [`Rect::EMPTY`] for a word this
    /// puzzle does not have.
    #[must_use]
    #[allow(
        clippy::cast_precision_loss,
        reason = "a list holds at most twelve words, which is exact in f32"
    )]
    pub fn word_row(&self, i: usize) -> Rect {
        if self.list.is_empty() || i >= self.words {
            return Rect::EMPTY;
        }
        let head = (self.list.h * 0.10).min(self.big * 1.6);
        let body = (self.list.h - head).max(0.0);
        let row_h = (body / self.words as f32).min(self.big * 1.7);
        if row_h <= 0.0 {
            return Rect::EMPTY;
        }
        Rect::new(
            self.list.x,
            self.list.y + head + i as f32 * row_h,
            self.list.w,
            row_h,
        )
    }

    /// Whether a band has room to say anything at all.
    #[must_use]
    pub fn shows(&self, band: Rect) -> bool {
        !band.is_empty() && band.h >= self.font
    }
}

// ── Drawing ────────────────────────────────────────────────────────────────

fn fill(f: &mut Frame, r: Rect, color: Color, radius: f32) {
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

fn stroke(f: &mut Frame, r: Rect, color: Color, line_width: f32, radius: f32) {
    if r.is_empty() {
        return;
    }
    f.push(RenderCommand::StrokeRect {
        x: r.x,
        y: r.y,
        width: r.w,
        height: r.h,
        color,
        line_width,
        corner_radii: CornerRadii::all(radius),
    });
}

#[allow(
    clippy::too_many_arguments,
    reason = "a text command has this many parts; naming them in a struct would \
              move the argument list rather than shorten it"
)]
fn label(
    f: &mut Frame,
    x: f32,
    y: f32,
    s: &str,
    size: f32,
    color: Color,
    weight: FontWeightHint,
    max_width: Option<f32>,
) {
    if size <= 0.0 || s.is_empty() || max_width.is_some_and(|w| w <= 0.0) {
        return;
    }
    f.push(RenderCommand::Text {
        x,
        y,
        text: s.to_string(),
        color,
        font_size: size,
        font_weight: weight,
        max_width,
        overflow: TextOverflow::Ellipsis,
    });
}

/// A string centred in `r`, both ways.
fn centred_in(f: &mut Frame, r: Rect, s: &str, size: f32, color: Color, weight: FontWeightHint) {
    if r.is_empty() || size <= 0.0 {
        return;
    }
    let w = text::measure(s, size, weight);
    let line_h = text::line_height(size, weight);
    // Centring moves the start left, so the width to fit in has to be measured
    // from the start actually chosen and not from the box's -- passing the
    // box's whole width from a start half a box to its left puts the ellipsis
    // point half a box past the right edge, which is a promise to clip that
    // clips nothing. A string too long to centre starts *at* the box.
    let x = (r.x + (r.w - w) / 2.0).max(r.x);
    label(
        f,
        x,
        r.y + (r.h - line_h) / 2.0,
        s,
        size,
        color,
        weight,
        Some((r.right() - x).max(0.0)),
    );
}

/// A string starting at the left of `r`, vertically centred and clipped to it.
fn left_in(f: &mut Frame, r: Rect, s: &str, size: f32, color: Color, weight: FontWeightHint) {
    if r.is_empty() || size <= 0.0 {
        return;
    }
    let line_h = text::line_height(size, weight);
    label(
        f,
        r.x,
        r.y + (r.h - line_h) / 2.0,
        s,
        size,
        color,
        weight,
        Some(r.w),
    );
}

/// A chip: a rounded box, a caption, and a hit box the size of the box.
///
/// The hit box is recorded only when the chip has room to be drawn. A hit box
/// on a control the renderer skipped is a control a test can find and a player
/// cannot, which is the wrong way round.
fn chip(f: &mut Frame, r: Rect, target: Target, s: &str, size: f32, accent: Color) {
    if r.is_empty() {
        return;
    }
    fill(f, r, SURFACE0, 5.0);
    f.hit(target, r);
    centred_in(f, r, s, size, accent, FontWeightHint::Bold);
}

impl WordSearchApp {
    /// The whole window, and every hit box in it.
    #[must_use]
    pub fn frame(&self, width: f32, height: f32) -> Frame {
        let l = Layout::new(width, height, self.grid_size, self.placed_words.len());
        let mut f = Frame::new(width, height);
        fill(&mut f, l.window, BASE, 0.0);

        self.draw_header(&mut f, &l);
        self.draw_board(&mut f, &l);
        self.draw_list(&mut f, &l);
        self.draw_footer(&mut f, &l);
        f
    }

    fn draw_header(&self, f: &mut Frame, l: &Layout) {
        if !l.shows(l.header) {
            return;
        }
        fill(f, l.header, MANTLE, 0.0);

        let inset = l.pad;
        let left = Rect::new(
            l.header.x + inset,
            l.header.y,
            (l.chip(0).x - l.header.x - inset * 2.0).max(0.0),
            l.header.h,
        );
        if !left.is_empty() {
            let top = Rect::new(left.x, left.y, left.w, left.h / 2.0);
            let bottom = Rect::new(left.x, left.y + left.h / 2.0, left.w, left.h / 2.0);
            left_in(f, top, "Word Search", l.big, BLUE, FontWeightHint::Bold);
            let clock = format_time(self.elapsed_secs());
            let found = format!(
                "{}  {}/{} found",
                clock,
                self.found_count(),
                self.total_words()
            );
            let color = if self.status == GameStatus::Won {
                GREEN
            } else {
                LAVENDER
            };
            left_in(f, bottom, &found, l.font, color, FontWeightHint::Regular);
        }

        chip(
            f,
            l.chip(0),
            Target::Difficulty,
            self.difficulty.label(),
            l.font,
            self.difficulty.color(),
        );
        chip(
            f,
            l.chip(1),
            Target::Category,
            self.category.label(),
            l.font,
            self.category.color(),
        );
        chip(
            f,
            l.chip(2),
            Target::HintButton,
            &format!("Hint {}", self.hints_remaining),
            l.font,
            if self.hints_remaining > 0 {
                PEACH
            } else {
                OVERLAY0
            },
        );
        chip(f, l.chip(3), Target::NewGame, "New", l.font, TEAL);
    }

    fn draw_board(&self, f: &mut Frame, l: &Layout) {
        let marked = self.marked_cells();
        for row in 0..self.grid_size {
            for col in 0..self.grid_size {
                let r = l.cell_rect(row, col);
                if r.is_empty() {
                    continue;
                }

                let lit = self.hint.is_some_and(|h| h.row == row && h.col == col);
                let found = self.is_found_cell(row, col);
                let marking = marked.contains(&(row, col));
                let anchored = self.selection == Selection::From(row, col);

                let bg = if lit {
                    YELLOW
                } else if found {
                    Color::rgba(166, 227, 161, 40)
                } else if anchored {
                    MAUVE
                } else if marking {
                    Color::rgba(137, 180, 250, 60)
                } else {
                    SURFACE0
                };
                fill(f, r, bg, (l.cell * 0.12).min(4.0));
                f.hit(Target::Cell(row, col), r);

                if self.cursor == (row, col) {
                    stroke(
                        f,
                        r,
                        BLUE,
                        (l.cell * 0.06).clamp(1.0, 2.5),
                        (l.cell * 0.12).min(4.0),
                    );
                }

                let Some(ch) = self.letter(row, col) else {
                    continue;
                };
                if ch == 0 {
                    continue;
                }
                let color = if lit {
                    BASE
                } else if found {
                    GREEN
                } else if marking {
                    BLUE
                } else {
                    TEXT_COLOR
                };
                let weight = if lit || found || marking {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                };
                centred_in(
                    f,
                    r,
                    &String::from(char::from(ch)),
                    (l.cell * 0.56).max(1.0),
                    color,
                    weight,
                );
            }
        }
    }

    fn draw_list(&self, f: &mut Frame, l: &Layout) {
        if !l.shows(l.list) {
            return;
        }
        let head = Rect::new(
            l.list.x,
            l.list.y,
            l.list.w,
            (l.list.h * 0.10).min(l.big * 1.6),
        );
        left_in(
            f,
            head,
            "Words to find",
            l.font,
            SUBTEXT0,
            FontWeightHint::Bold,
        );

        for (i, placed) in self.placed_words.iter().enumerate() {
            let r = l.word_row(i);
            if r.is_empty() {
                continue;
            }
            let (color, weight) = if placed.found {
                (OVERLAY0, FontWeightHint::Light)
            } else {
                (TEXT_COLOR, FontWeightHint::Regular)
            };
            if placed.found {
                fill(f, r, SURFACE1, 3.0);
            }
            f.hit(Target::Word(i), r);
            left_in(f, r, &placed.word, l.font, color, weight);

            if placed.found {
                // The rule is as long as the word it strikes through, measured
                // in the weight the word was drawn at and clamped to the room
                // the word itself was clipped to -- otherwise a word wider than
                // the column gets a rule running out past the letters the
                // renderer actually drew.
                let drawn = text::measure(&placed.word, l.font, weight).min(r.w);
                let mid = r.y + r.h / 2.0;
                f.push(RenderCommand::Line {
                    x1: r.x,
                    y1: mid,
                    x2: r.x + drawn,
                    y2: mid,
                    color: GREEN,
                    width: (l.font * 0.1).clamp(1.0, 2.0),
                });
            }
        }
    }

    fn draw_footer(&self, f: &mut Frame, l: &Layout) {
        if !l.shows(l.footer) {
            return;
        }
        fill(f, l.footer, MANTLE, 0.0);
        let table = if self.selection == Selection::None {
            SHORTCUTS
        } else {
            SELECTING_SHORTCUTS
        };
        let text_line = table
            .iter()
            .map(|(k, what)| format!("{k}:{what}"))
            .collect::<Vec<_>>()
            .join("   ");
        let inner = Rect::new(
            l.footer.x + l.pad,
            l.footer.y,
            (l.footer.w - l.pad * 2.0).max(0.0),
            l.footer.h,
        );
        left_in(
            f,
            inner,
            &text_line,
            l.font,
            OVERLAY0,
            FontWeightHint::Regular,
        );
    }
}

// ── The window ─────────────────────────────────────────────────────────────

/// Route one event to the handler that answers it.
///
/// A free function so a test can send the events [`Probe`] does not model —
/// a pointer `Move`, a `Release`, a `Tick` — through exactly the path the
/// window uses.
pub fn handle_event(app: &mut WordSearchApp, event: &Event) -> EventResult {
    match event {
        Event::Key(key) => app.handle_key(key),
        Event::Mouse(mouse) => app.handle_mouse(mouse),
        Event::Tick { elapsed_ms } => app.tick(*elapsed_ms),
        Event::Resize { width, height } => {
            #[allow(
                clippy::cast_precision_loss,
                reason = "a window dimension is far below f32's exact-integer range"
            )]
            app.resize(*width as f32, *height as f32);
            EventResult::Consumed
        }
        _ => EventResult::Ignored,
    }
}

impl App for WordSearchApp {
    fn title(&self) -> String {
        "Word Search".to_string()
    }

    fn app_id(&self) -> String {
        "wordsearch".to_string()
    }

    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "both constants are small positive integers written as f32"
    )]
    fn initial_size(&self) -> (u32, u32) {
        (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)
    }

    /// Asked after every event, so the clock gets a wake-up exactly while
    /// there is something for it to move. Leaving this at the default gets no
    /// ticks at all — which is what this program did, with a timer field on the
    /// model and a `MM:SS` in the header and nothing on earth to advance it.
    fn tick_interval(&self) -> Option<Duration> {
        if self.hint.is_some() {
            Some(Duration::from_millis(HINT_STEP_MS))
        } else if self.status == GameStatus::Playing {
            Some(Duration::from_millis(CLOCK_MS))
        } else {
            None
        }
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
        // against -- that is the whole point of storing it.
        self.resize(width, height);
        self.frame(width, height).into_tree()
    }
}

impl Probe for WordSearchApp {
    type Target = Target;
    type Outcome = EventResult;
    const SIZE: (f32, f32) = (WINDOW_WIDTH, WINDOW_HEIGHT);

    fn draw(&self, size: (f32, f32)) -> Frame {
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

fn main() -> ExitCode {
    let mut app = WordSearchApp::new();
    app::launch("wordsearch", &mut app)
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_cmp,
    clippy::arithmetic_side_effects,
    clippy::cast_precision_loss,
    reason = "a test that cannot panic cannot fail, and a test that avoids \
              indexing says less about the thing it indexes"
)]
mod tests {
    use super::*;
    use guitk::probe::{self, ctrl, press, press_with};

    // ── Helpers ────────────────────────────────────────────────────────────

    const SIZE: (f32, f32) = (WINDOW_WIDTH, WINDOW_HEIGHT);

    /// Every window shape worth asking a layout question at: the default, the
    /// degenerate, the very wide, the very tall, and a few in between.
    const SIZES: [(f32, f32); 9] = [
        (WINDOW_WIDTH, WINDOW_HEIGHT),
        (1.0, 1.0),
        (2.0, 400.0),
        (400.0, 2.0),
        (320.0, 240.0),
        (1920.0, 1080.0),
        (600.0, 200.0),
        (200.0, 600.0),
        (1000.0, 1000.0),
    ];

    fn game(seed: u64) -> WordSearchApp {
        WordSearchApp::with_seed(seed)
    }

    fn tick(a: &mut WordSearchApp, ms: u64) -> EventResult {
        handle_event(a, &Event::Tick { elapsed_ms: ms })
    }

    fn key(a: &mut WordSearchApp, k: Key) -> EventResult {
        handle_event(a, &Event::Key(press(k)))
    }

    fn mouse(a: &mut WordSearchApp, x: f32, y: f32, kind: MouseEventKind) -> EventResult {
        handle_event(a, &Event::Mouse(MouseEvent { x, y, kind }))
    }

    fn layout_of(a: &WordSearchApp) -> Layout {
        Layout::new(a.size().0, a.size().1, a.grid_size(), a.total_words())
    }

    fn cell_point(a: &WordSearchApp, (row, col): (usize, usize)) -> (f32, f32) {
        let r = layout_of(a).cell_rect(row, col);
        assert!(
            !r.is_empty(),
            "cell {row},{col} is not drawn at {:?}",
            a.size()
        );
        r.centre()
    }

    /// Press on `from`, move to `to`, release — the gesture a player makes.
    fn drag(a: &mut WordSearchApp, from: (usize, usize), to: (usize, usize)) {
        let (x, y) = cell_point(a, from);
        mouse(a, x, y, MouseEventKind::Press(MouseButton::Left));
        let (x, y) = cell_point(a, to);
        mouse(a, x, y, MouseEventKind::Move);
        mouse(a, x, y, MouseEventKind::Release(MouseButton::Left));
    }

    /// Walk the cursor to `(row, col)` with the arrow keys alone.
    fn walk_to(a: &mut WordSearchApp, (row, col): (usize, usize)) {
        for _ in 0..a.grid_size() * 2 {
            let (r, c) = a.cursor();
            if (r, c) == (row, col) {
                return;
            }
            if r > row {
                key(a, Key::Up);
            } else if r < row {
                key(a, Key::Down);
            } else if c > col {
                key(a, Key::Left);
            } else {
                key(a, Key::Right);
            }
        }
        assert_eq!(a.cursor(), (row, col), "the cursor never arrived");
    }

    /// Mark a word out with the keyboard: walk to its first cell, Enter, walk
    /// to its last, Enter.
    fn spell_out(a: &mut WordSearchApp, cells: &[(usize, usize)]) {
        let first = *cells.first().unwrap();
        let last = *cells.last().unwrap();
        walk_to(a, first);
        key(a, Key::Enter);
        walk_to(a, last);
        key(a, Key::Enter);
    }

    fn strings(a: &WordSearchApp, size: (f32, f32)) -> Vec<String> {
        a.frame(size.0, size.1)
            .commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    fn says(a: &WordSearchApp, size: (f32, f32), needle: &str) -> bool {
        strings(a, size).iter().any(|s| s.contains(needle))
    }

    fn colour_of(a: &WordSearchApp, size: (f32, f32), needle: &str) -> Option<Color> {
        a.frame(size.0, size.1)
            .commands()
            .iter()
            .find_map(|c| match c {
                RenderCommand::Text { text, color, .. } if text.contains(needle) => Some(*color),
                _ => None,
            })
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 0.01
    }

    /// The cells of the first word still unfound.
    fn first_unfound(a: &WordSearchApp) -> Vec<(usize, usize)> {
        a.words()
            .iter()
            .find(|w| !w.found)
            .map(PlacedWord::cells)
            .unwrap()
    }

    // ── Steps and directions ───────────────────────────────────────────────

    #[test]
    fn a_step_walks_the_way_it_names_and_stops_at_the_edge() {
        assert_eq!(Step::Fwd.from(5, 3), Some(8));
        assert_eq!(Step::Back.from(5, 3), Some(2));
        assert_eq!(Step::Stay.from(5, 3), Some(5));
        // Off the low end is `None`, not a wrap to `usize::MAX` -- the whole
        // reason this is an enum and not an `i32` delta.
        assert_eq!(Step::Back.from(2, 3), None);
        assert_eq!(Step::Back.from(0, 1), None);
        assert_eq!(Step::Fwd.from(usize::MAX, 1), None);
        // `Stay` ignores the distance rather than clamping it.
        assert_eq!(Step::Stay.from(0, usize::MAX), Some(0));
    }

    #[test]
    fn reversing_a_step_undoes_it() {
        assert_eq!(Step::Back.reversed(), Step::Fwd);
        assert_eq!(Step::Fwd.reversed(), Step::Back);
        assert_eq!(Step::Stay.reversed(), Step::Stay);
        for step in [Step::Back, Step::Stay, Step::Fwd] {
            assert_eq!(step.reversed().reversed(), step);
            // Two steps apart on opposite sides of the same origin.
            let out = step.from(9, 2);
            let back = step.reversed().from(9, 2);
            assert_eq!(step == Step::Stay, out == back);
        }
    }

    #[test]
    fn the_eight_directions_are_eight_different_directions() {
        assert_eq!(DIRECTIONS.len(), 8);
        for (i, a) in DIRECTIONS.iter().enumerate() {
            for (j, b) in DIRECTIONS.iter().enumerate() {
                assert!(i == j || a != b, "directions {i} and {j} are the same");
            }
            assert_ne!(
                *a,
                (Step::Stay, Step::Stay),
                "standing still is not a direction"
            );
        }
        // Every direction's opposite is also offered, so a word can be hidden
        // backwards as readily as forwards.
        for d in DIRECTIONS {
            let opposite = (d.0.reversed(), d.1.reversed());
            assert!(DIRECTIONS.contains(&opposite), "{d:?} has no opposite");
        }
    }

    #[test]
    fn a_words_cells_run_from_its_start_along_its_direction() {
        let w = PlacedWord {
            word: String::from("TIGER"),
            start: (2, 3),
            dir: (Step::Fwd, Step::Back),
            found: false,
        };
        assert_eq!(w.cells(), vec![(2, 3), (3, 2), (4, 1), (5, 0)]);
        // The walk stops where it runs off the board rather than wrapping: a
        // five-letter word starting at column 3 going left has four cells.
        assert_eq!(w.cells().len(), 4);

        let straight = PlacedWord {
            word: String::from("CAT"),
            start: (0, 0),
            dir: (Step::Stay, Step::Fwd),
            found: false,
        };
        assert_eq!(straight.cells(), vec![(0, 0), (0, 1), (0, 2)]);
    }

    // ── Categories and difficulties ────────────────────────────────────────

    #[test]
    fn every_category_word_is_capital_ascii_so_a_byte_is_a_letter() {
        for category in Category::ALL {
            for word in category.words() {
                assert!(
                    word.bytes().all(|b| b.is_ascii_uppercase()),
                    "{} has {word}, which is not capital ASCII",
                    category.label()
                );
                assert_eq!(
                    word.len(),
                    word.chars().count(),
                    "{word} has more bytes than letters"
                );
                assert!(word.len() >= 3, "{word} is too short to hide");
                assert!(
                    word.len() <= Difficulty::Easy.grid_size(),
                    "{word} will not fit on the smallest board"
                );
            }
        }
    }

    #[test]
    fn every_category_has_enough_words_for_the_hardest_board() {
        let wanted = Difficulty::Hard.word_count();
        for category in Category::ALL {
            assert!(
                category.words().len() >= wanted,
                "{} has {} words but Hard wants {wanted}",
                category.label(),
                category.words().len()
            );
        }
    }

    #[test]
    fn cycling_categories_visits_every_one_and_returns() {
        let mut seen = Vec::new();
        let mut c = Category::Animals;
        for _ in 0..Category::ALL.len() {
            assert!(
                !seen.contains(&c),
                "{} came round twice too early",
                c.label()
            );
            seen.push(c);
            c = c.next();
        }
        assert_eq!(c, Category::Animals, "the cycle does not close");
        assert_eq!(seen.len(), Category::ALL.len());
        for category in Category::ALL {
            assert!(
                seen.contains(&category),
                "{} is unreachable",
                category.label()
            );
        }
    }

    #[test]
    fn every_category_has_its_own_name_and_its_own_accent() {
        for a in Category::ALL {
            for b in Category::ALL {
                if a == b {
                    continue;
                }
                assert_ne!(a.label(), b.label(), "two categories share a name");
                assert_ne!(a.color(), b.color(), "two categories share an accent");
                assert_ne!(a.words(), b.words(), "two categories share a word list");
            }
            assert!(!a.label().is_empty());
        }
    }

    #[test]
    fn every_category_offers_the_same_number_of_words_and_none_of_them_twice() {
        // `assert_ne!(a.words(), b.words())` above is a weaker claim than it
        // looks, and the mutation sweep said so: pasting fifteen animals onto
        // the front of the colour list leaves the two lists unequal, so nothing
        // failed. Inequality is not distinctness -- a list can be another list
        // plus something. The fact that does pin it comes from outside the
        // program: all five lists are the same length, so no category is a
        // poorer draw than another, and a category that has quietly absorbed a
        // second list is a category with the wrong number of words in it.
        let sizes: Vec<usize> = Category::ALL.iter().map(|c| c.words().len()).collect();
        assert!(
            sizes.iter().all(|&n| n == 30),
            "the categories offer {sizes:?} words, not thirty each"
        );
        for category in Category::ALL {
            let mut seen: Vec<&str> = Vec::new();
            for &word in category.words() {
                assert!(
                    !seen.contains(&word),
                    "{} lists {word} twice",
                    category.label()
                );
                seen.push(word);
            }
        }
    }

    #[test]
    fn a_harder_board_is_bigger_and_hides_more() {
        let sizes: Vec<usize> = Difficulty::ALL.iter().map(|d| d.grid_size()).collect();
        let counts: Vec<usize> = Difficulty::ALL.iter().map(|d| d.word_count()).collect();
        assert!(
            sizes.windows(2).all(|w| w[0] < w[1]),
            "{sizes:?} is not increasing"
        );
        assert!(
            counts.windows(2).all(|w| w[0] < w[1]),
            "{counts:?} is not increasing"
        );
        // A board must have room for the words it promises: a word may take a
        // whole row, so the row count is the honest ceiling.
        for d in Difficulty::ALL {
            assert!(
                d.word_count() <= d.grid_size(),
                "{} is overcrowded",
                d.label()
            );
        }
    }

    #[test]
    fn cycling_difficulty_visits_every_one_and_returns() {
        let mut seen = Vec::new();
        let mut d = Difficulty::Easy;
        for _ in 0..Difficulty::ALL.len() {
            assert!(!seen.contains(&d));
            seen.push(d);
            d = d.next();
        }
        assert_eq!(d, Difficulty::Easy);
        for difficulty in Difficulty::ALL {
            assert!(
                seen.contains(&difficulty),
                "{} is unreachable",
                difficulty.label()
            );
            for other in Difficulty::ALL {
                if other != difficulty {
                    assert_ne!(difficulty.label(), other.label());
                    assert_ne!(difficulty.color(), other.color());
                }
            }
        }
    }

    // ── Lines between cells ────────────────────────────────────────────────

    #[test]
    fn a_line_between_two_cells_is_only_a_line_at_the_eight_angles() {
        assert_eq!(
            cells_between((0, 0), (0, 3)).unwrap(),
            vec![(0, 0), (0, 1), (0, 2), (0, 3)]
        );
        assert_eq!(
            cells_between((3, 0), (0, 0)).unwrap(),
            vec![(3, 0), (2, 0), (1, 0), (0, 0)]
        );
        assert_eq!(
            cells_between((0, 0), (2, 2)).unwrap(),
            vec![(0, 0), (1, 1), (2, 2)]
        );
        assert_eq!(
            cells_between((2, 0), (0, 2)).unwrap(),
            vec![(2, 0), (1, 1), (0, 2)]
        );
        // A knight's move is not a direction any word runs along.
        assert_eq!(cells_between((0, 0), (1, 2)), None);
        assert_eq!(cells_between((0, 0), (2, 1)), None);
        assert_eq!(cells_between((5, 5), (0, 3)), None);
    }

    #[test]
    fn a_cell_is_a_line_of_one() {
        // Not `None`: a drag that returns to its own start is still a drag, and
        // answering `None` would blink the preview off at the moment it did.
        assert_eq!(cells_between((4, 7), (4, 7)).unwrap(), vec![(4, 7)]);
    }

    #[test]
    fn a_line_and_its_reverse_are_the_same_cells_in_the_other_order() {
        for &(from, to) in &[
            ((1usize, 1usize), (1usize, 6usize)),
            ((6, 1), (1, 1)),
            ((0, 0), (4, 4)),
            ((4, 0), (0, 4)),
        ] {
            let there = cells_between(from, to).unwrap();
            let back = cells_between(to, from).unwrap();
            assert_eq!(there.len(), back.len());
            let reversed: Vec<_> = back.iter().rev().copied().collect();
            assert_eq!(there, reversed, "{from:?}..{to:?} is not its own reverse");
            assert_eq!(*there.first().unwrap(), from);
            assert_eq!(*there.last().unwrap(), to);
        }
    }

    #[test]
    fn every_cell_of_a_line_steps_exactly_one_from_the_last() {
        let line = cells_between((2, 9), (9, 2)).unwrap();
        assert_eq!(line.len(), 8);
        for pair in line.windows(2) {
            let (ar, ac) = pair[0];
            let (br, bc) = pair[1];
            assert_eq!(ar.abs_diff(br), 1, "{pair:?} skips a row");
            assert_eq!(ac.abs_diff(bc), 1, "{pair:?} skips a column");
        }
    }

    // ── The clock's format ─────────────────────────────────────────────────

    #[test]
    fn the_clock_reads_minutes_and_seconds_with_both_digits() {
        assert_eq!(format_time(0), "00:00");
        assert_eq!(format_time(9), "00:09");
        assert_eq!(format_time(59), "00:59");
        assert_eq!(format_time(60), "01:00");
        assert_eq!(format_time(61), "01:01");
        assert_eq!(format_time(599), "09:59");
        assert_eq!(format_time(600), "10:00");
        assert_eq!(format_time(3_599), "59:59");
        // Past an hour the minutes keep counting rather than wrapping: this is
        // a stopwatch, not a clock face.
        assert_eq!(format_time(3_600), "60:00");
        assert_eq!(format_time(7_265), "121:05");
    }

    #[test]
    fn the_header_shows_the_clock_the_ticks_wound() {
        let mut a = game(11);
        assert!(says(&a, SIZE, "00:00"), "{:?}", strings(&a, SIZE));
        tick(&mut a, 65_000);
        assert_eq!(a.elapsed_secs(), 65);
        assert!(says(&a, SIZE, "01:05"), "{:?}", strings(&a, SIZE));
        assert!(!says(&a, SIZE, "00:00"));
    }

    // ── Generation ─────────────────────────────────────────────────────────

    #[test]
    fn a_new_board_is_full_of_letters_with_no_holes() {
        for seed in [1u64, 2, 3, 99, 12_345] {
            let a = game(seed);
            let n = a.grid_size();
            assert_eq!(n, Difficulty::Medium.grid_size());
            for row in 0..n {
                for col in 0..n {
                    let ch = a
                        .letter(row, col)
                        .expect("every cell on the board has a letter");
                    assert!(ch.is_ascii_uppercase(), "cell {row},{col} holds {ch:#x}");
                }
            }
        }
    }

    #[test]
    fn every_hidden_word_can_actually_be_read_off_the_board() {
        for seed in [7u64, 8, 21, 400, 55_555] {
            let a = game(seed);
            assert!(a.total_words() > 0, "seed {seed} hid nothing");
            for placed in a.words() {
                let cells = placed.cells();
                assert_eq!(
                    cells.len(),
                    placed.word.len(),
                    "{} runs off the board",
                    placed.word
                );
                let read: Vec<u8> = cells
                    .iter()
                    .map(|&(r, c)| a.letter(r, c).expect("a word's cell is on the board"))
                    .collect();
                assert_eq!(
                    read,
                    placed.word.as_bytes(),
                    "{} is not on the board where it says it is",
                    placed.word
                );
                assert!(!placed.found, "a fresh board has nothing found");
            }
        }
    }

    #[test]
    fn the_same_seed_is_the_same_puzzle_and_a_different_seed_is_not() {
        let a = game(1234);
        let b = game(1234);
        let c = game(1235);
        let read = |g: &WordSearchApp| -> Vec<u8> {
            (0..g.grid_size())
                .flat_map(|r| (0..g.grid_size()).map(move |c| (r, c)))
                .map(|(r, c)| g.letter(r, c).unwrap())
                .collect()
        };
        assert_eq!(read(&a), read(&b), "one seed gave two boards");
        assert_ne!(read(&a), read(&c), "two seeds gave one board");
    }

    #[test]
    fn one_named_seed_names_one_named_board() {
        // Every term of the test above goes through `with_seed`, so it is
        // silent about `with_seed` itself: replacing the seed with `seed ^ 1`
        // keeps equal seeds equal and unequal seeds unequal, and the sweep duly
        // walked straight past it. That is `known-issues.md` lesson 52 -- a
        // test built out of the thing it is testing cannot fail -- and the way
        // out of it is a statement from outside the program. This is that
        // statement: one literal board, written down, for one seed. It is a
        // characterisation test and it is meant to be brittle. If a change to
        // generation breaks it, the honest response is to check the change was
        // intended and paste in the new row -- not to weaken the claim, because
        // "the same seed gives the same puzzle across builds" is the whole
        // content of the word "seeded".
        let a = WordSearchApp::with_seed(1234);
        // Named, so that a change of default settings fails here saying which
        // one moved, rather than as an unexplained row of different letters.
        assert_eq!(a.difficulty(), Difficulty::Medium);
        assert_eq!(a.category(), Category::Animals);
        let row: String = (0..a.grid_size())
            .map(|c| char::from(a.letter(0, c).unwrap_or(b'?')))
            .collect();
        assert_eq!(row, "MGZXYREUHXFEVUR", "seed 1234's first row moved");
    }

    #[test]
    fn no_two_hidden_words_are_the_same_word() {
        for seed in [3u64, 30, 300, 3_000] {
            let a = game(seed);
            let mut seen: Vec<&str> = Vec::new();
            for placed in a.words() {
                assert!(
                    !seen.contains(&placed.word.as_str()),
                    "{} was hidden twice",
                    placed.word
                );
                seen.push(&placed.word);
            }
        }
    }

    #[test]
    fn a_harder_puzzle_really_is_a_bigger_board_with_more_words() {
        for difficulty in Difficulty::ALL {
            let mut a = game(77);
            a.apply(Action::SetDifficulty(difficulty));
            assert_eq!(a.difficulty(), difficulty);
            assert_eq!(a.grid_size(), difficulty.grid_size());
            assert_eq!(
                a.total_words(),
                difficulty.word_count(),
                "{} placed {} of {}",
                difficulty.label(),
                a.total_words(),
                difficulty.word_count()
            );
        }
    }

    #[test]
    fn words_are_hidden_in_more_than_one_direction() {
        // A generator that always ran left-to-right would still pass every
        // "the word is where it says" test, and would be a much worse puzzle.
        let mut directions: Vec<(Step, Step)> = Vec::new();
        for seed in 0..12u64 {
            for placed in game(seed).words() {
                if !directions.contains(&placed.dir) {
                    directions.push(placed.dir);
                }
            }
        }
        assert_eq!(
            directions.len(),
            DIRECTIONS.len(),
            "only {} of the eight directions were ever used: {directions:?}",
            directions.len()
        );
    }

    // ── Reading the board ──────────────────────────────────────────────────

    #[test]
    fn a_column_past_the_end_is_not_the_next_rows_first() {
        let a = game(5);
        let n = a.grid_size();
        // Without the column bound this is `grid[n]`, a perfectly valid index
        // that reads row 1's first letter -- a silently wrong answer rather
        // than no answer.
        assert_eq!(
            a.letter(0, n),
            None,
            "column {n} of row 0 answered something"
        );
        assert_eq!(a.letter(0, n + 1), None);
        assert!(a.letter(1, 0).is_some(), "row 1 column 0 is a real cell");
        // A row past the end runs off the end of the grid, which `get` catches.
        assert_eq!(a.letter(n, 0), None);
        assert_eq!(a.letter(usize::MAX, 0), None);
        assert_eq!(a.letter(usize::MAX, usize::MAX), None);
        // The corners are on the board.
        assert!(a.letter(0, 0).is_some());
        assert!(a.letter(n - 1, n - 1).is_some());
    }

    // ── Walking the cursor ─────────────────────────────────────────────────

    #[test]
    fn the_arrow_keys_walk_the_cursor_one_cell_at_a_time() {
        let mut a = game(9);
        assert_eq!(a.cursor(), (0, 0));
        assert_eq!(key(&mut a, Key::Right), EventResult::Consumed);
        assert_eq!(a.cursor(), (0, 1));
        key(&mut a, Key::Down);
        assert_eq!(a.cursor(), (1, 1));
        key(&mut a, Key::Left);
        assert_eq!(a.cursor(), (1, 0));
        key(&mut a, Key::Up);
        assert_eq!(a.cursor(), (0, 0));
    }

    #[test]
    fn the_cursor_stops_at_every_edge_rather_than_wrapping() {
        let mut a = game(9);
        let n = a.grid_size();
        // Off the top and the left is `None` from `Step::from`, and the answer
        // must be the same as off the bottom and the right, which is the bound
        // in `Goto`.
        assert_eq!(key(&mut a, Key::Up), EventResult::Ignored);
        assert_eq!(key(&mut a, Key::Left), EventResult::Ignored);
        assert_eq!(a.cursor(), (0, 0));

        walk_to(&mut a, (n - 1, n - 1));
        assert_eq!(key(&mut a, Key::Down), EventResult::Ignored);
        assert_eq!(key(&mut a, Key::Right), EventResult::Ignored);
        assert_eq!(a.cursor(), (n - 1, n - 1));
    }

    #[test]
    fn a_move_that_changes_nothing_is_not_a_redraw() {
        // `Goto` refuses a move to where the cursor already is, which is what
        // keeps a held-down arrow key at the edge from repainting the window
        // sixty times a second for no change.
        let mut a = game(9);
        assert_eq!(a.apply(Action::Goto(3, 4)), EventResult::Consumed);
        assert_eq!(a.apply(Action::Goto(3, 4)), EventResult::Ignored);
        assert_eq!(a.cursor(), (3, 4));
    }

    #[test]
    fn the_cursor_cannot_be_sent_off_the_board_by_name_either() {
        let mut a = game(9);
        let n = a.grid_size();
        assert_eq!(a.apply(Action::Goto(n, 0)), EventResult::Ignored);
        assert_eq!(a.apply(Action::Goto(0, n)), EventResult::Ignored);
        assert_eq!(
            a.apply(Action::Goto(usize::MAX, usize::MAX)),
            EventResult::Ignored
        );
        assert_eq!(a.cursor(), (0, 0));
        assert_eq!(a.apply(Action::Goto(n - 1, n - 1)), EventResult::Consumed);
    }

    // ── Marking with the keyboard ──────────────────────────────────────────

    #[test]
    fn enter_anchors_and_enter_again_confirms() {
        let mut a = game(13);
        assert_eq!(a.selection(), Selection::None);
        walk_to(&mut a, (2, 2));
        assert_eq!(key(&mut a, Key::Enter), EventResult::Consumed);
        assert_eq!(a.selection(), Selection::From(2, 2));
        // The cursor is free to move while the anchor stays put.
        key(&mut a, Key::Right);
        assert_eq!(a.selection(), Selection::From(2, 2));
        assert_eq!(a.cursor(), (2, 3));
        key(&mut a, Key::Enter);
        assert_eq!(
            a.selection(),
            Selection::None,
            "confirming leaves nothing marked"
        );
    }

    #[test]
    fn a_word_marked_from_its_first_letter_to_its_last_is_found() {
        let mut a = game(13);
        let cells = first_unfound(&a);
        let word = a.words()[0].word.clone();
        assert_eq!(a.found_count(), 0);
        spell_out(&mut a, &cells);
        assert_eq!(a.found_count(), 1, "{word} was not found");
        assert!(a.words()[0].found);
        for &(r, c) in &cells {
            assert!(a.is_found_cell(r, c), "{r},{c} of {word} is not lit");
        }
    }

    #[test]
    fn a_word_marked_backwards_is_the_same_word() {
        let mut a = game(13);
        let mut cells = first_unfound(&a);
        cells.reverse();
        spell_out(&mut a, &cells);
        assert_eq!(a.found_count(), 1, "a word can only be read one way round");
    }

    #[test]
    fn marking_a_line_that_spells_nothing_finds_nothing() {
        let mut a = game(13);
        // Two adjacent cells: no word is two letters long, so whatever is
        // under them, this is not a word.
        walk_to(&mut a, (0, 0));
        key(&mut a, Key::Enter);
        key(&mut a, Key::Right);
        assert_eq!(key(&mut a, Key::Enter), EventResult::Consumed);
        assert_eq!(a.found_count(), 0, "a two-cell mark found a word");
        assert_eq!(a.selection(), Selection::None, "the mark was not cleared");
        assert_eq!(a.status(), GameStatus::Playing);
    }

    #[test]
    fn a_mark_whose_ends_are_not_on_a_line_finds_nothing() {
        let mut a = game(13);
        walk_to(&mut a, (0, 0));
        key(&mut a, Key::Enter);
        walk_to(&mut a, (1, 3)); // a knight's move away
        assert_eq!(
            a.marked_cells(),
            Vec::new(),
            "a crooked mark previewed cells"
        );
        key(&mut a, Key::Enter);
        assert_eq!(a.found_count(), 0);
    }

    #[test]
    fn escape_throws_a_mark_away_and_finds_nothing() {
        let mut a = game(13);
        let cells = first_unfound(&a);
        walk_to(&mut a, cells[0]);
        key(&mut a, Key::Enter);
        assert_eq!(key(&mut a, Key::Escape), EventResult::Consumed);
        assert_eq!(a.selection(), Selection::None);
        // Reaching the far end now marks nothing, because nothing is anchored.
        walk_to(&mut a, *cells.last().unwrap());
        assert_eq!(a.found_count(), 0);
        // And escape with nothing to cancel is not a redraw.
        assert_eq!(key(&mut a, Key::Escape), EventResult::Ignored);
    }

    #[test]
    fn the_marked_cells_are_the_line_between_the_anchor_and_the_cursor() {
        let mut a = game(13);
        assert_eq!(
            a.marked_cells(),
            Vec::new(),
            "nothing is marked before anything is"
        );
        walk_to(&mut a, (4, 4));
        key(&mut a, Key::Enter);
        assert_eq!(a.marked_cells(), vec![(4, 4)]);
        key(&mut a, Key::Right);
        key(&mut a, Key::Right);
        assert_eq!(a.marked_cells(), vec![(4, 4), (4, 5), (4, 6)]);
        key(&mut a, Key::Up);
        assert_eq!(
            a.marked_cells(),
            Vec::new(),
            "a mark one row up and two columns across is not a line"
        );
        key(&mut a, Key::Up);
        assert_eq!(a.marked_cells(), vec![(4, 4), (3, 5), (2, 6)]);
    }

    #[test]
    fn finding_every_word_wins_and_stops_the_clock() {
        let mut a = game(13);
        let all: Vec<Vec<(usize, usize)>> = a.words().iter().map(PlacedWord::cells).collect();
        assert!(all.len() >= 2);
        for cells in &all {
            assert_eq!(
                a.status(),
                GameStatus::Playing,
                "won before every word was found"
            );
            spell_out(&mut a, cells);
        }
        assert_eq!(a.status(), GameStatus::Won);
        assert_eq!(a.found_count(), a.total_words());

        let stopped = a.elapsed_ms();
        assert_eq!(
            tick(&mut a, 5_000),
            EventResult::Ignored,
            "the clock ran on after the win"
        );
        assert_eq!(a.elapsed_ms(), stopped);
        assert!(!a.animating());
        // And nothing more can be marked.
        assert_eq!(a.apply(Action::Anchor), EventResult::Ignored);
        assert_eq!(a.apply(Action::Begin(0, 0)), EventResult::Ignored);
    }

    // ── Marking with the pointer ───────────────────────────────────────────

    #[test]
    fn dragging_across_a_word_finds_it() {
        let mut a = game(21);
        let cells = first_unfound(&a);
        drag(&mut a, cells[0], *cells.last().unwrap());
        assert_eq!(a.found_count(), 1);
        assert!(!a.dragging(), "the drag is still open after the release");
        assert_eq!(a.selection(), Selection::None);
    }

    #[test]
    fn a_press_anchors_where_it_lands_and_moves_the_cursor_there() {
        let mut a = game(21);
        let (x, y) = cell_point(&a, (3, 5));
        assert_eq!(
            mouse(&mut a, x, y, MouseEventKind::Press(MouseButton::Left)),
            EventResult::Consumed
        );
        assert_eq!(a.cursor(), (3, 5));
        assert_eq!(a.selection(), Selection::From(3, 5));
        assert!(a.dragging());
    }

    #[test]
    fn the_pointer_only_drags_the_far_end_while_a_button_is_down() {
        // `MouseEventKind::Move` carries no button state, so the model tracks
        // its own drag flag. Without it, merely crossing the board would haul
        // the cursor around.
        let mut a = game(21);
        let (x, y) = cell_point(&a, (6, 6));
        assert_eq!(
            mouse(&mut a, x, y, MouseEventKind::Move),
            EventResult::Ignored
        );
        assert_eq!(a.cursor(), (0, 0), "a hover moved the cursor");
        assert_eq!(a.selection(), Selection::None);

        let (px, py) = cell_point(&a, (6, 0));
        mouse(&mut a, px, py, MouseEventKind::Press(MouseButton::Left));
        assert_eq!(
            mouse(&mut a, x, y, MouseEventKind::Move),
            EventResult::Consumed
        );
        assert_eq!(a.cursor(), (6, 6), "a drag did not move the cursor");
    }

    #[test]
    fn a_release_with_no_drag_open_does_nothing() {
        let mut a = game(21);
        let (x, y) = cell_point(&a, (2, 2));
        assert_eq!(
            mouse(&mut a, x, y, MouseEventKind::Release(MouseButton::Left)),
            EventResult::Ignored
        );
        // A selection opened with Enter is not something a stray release may
        // finish -- that is the state the drag flag exists to tell apart.
        key(&mut a, Key::Enter);
        assert_eq!(a.selection(), Selection::From(0, 0));
        assert_eq!(
            mouse(&mut a, x, y, MouseEventKind::Release(MouseButton::Left)),
            EventResult::Ignored
        );
        assert_eq!(
            a.selection(),
            Selection::From(0, 0),
            "a release ate the anchor"
        );
    }

    #[test]
    fn escape_abandons_a_drag_as_well_as_an_anchor() {
        let mut a = game(21);
        let cells = first_unfound(&a);
        let (x, y) = cell_point(&a, cells[0]);
        mouse(&mut a, x, y, MouseEventKind::Press(MouseButton::Left));
        assert!(a.dragging());
        assert_eq!(key(&mut a, Key::Escape), EventResult::Consumed);
        assert!(!a.dragging());
        // The release that follows now finds nothing.
        let (x, y) = cell_point(&a, *cells.last().unwrap());
        mouse(&mut a, x, y, MouseEventKind::Move);
        mouse(&mut a, x, y, MouseEventKind::Release(MouseButton::Left));
        assert_eq!(a.found_count(), 0);
    }

    #[test]
    fn only_the_left_button_marks() {
        let mut a = game(21);
        let (x, y) = cell_point(&a, (4, 4));
        for button in [MouseButton::Right, MouseButton::Middle] {
            assert_eq!(
                mouse(&mut a, x, y, MouseEventKind::Press(button)),
                EventResult::Ignored
            );
        }
        assert_eq!(a.selection(), Selection::None);
        assert_eq!(
            mouse(&mut a, x, y, MouseEventKind::Scroll { dx: 0.0, dy: 3.0 }),
            EventResult::Ignored
        );
        assert_eq!(a.cursor(), (0, 0));
    }

    #[test]
    fn a_click_off_every_control_does_nothing() {
        let mut a = game(21);
        assert_eq!(probe::click_background(&mut a), EventResult::Ignored);
        assert_eq!(a.selection(), Selection::None);
        assert_eq!(a.cursor(), (0, 0));
    }

    // ── Hints ──────────────────────────────────────────────────────────────

    #[test]
    fn a_hint_lights_the_first_letter_of_a_word_and_costs_one() {
        let mut a = game(31);
        assert_eq!(a.hints_remaining(), MAX_HINTS);
        assert_eq!(a.hint(), None);
        assert_eq!(key(&mut a, Key::H), EventResult::Consumed);
        let lit = a.hint().expect("H lit nothing");
        assert_eq!((lit.row, lit.col), first_unfound(&a)[0]);
        assert_eq!(lit.remaining_ms, HINT_MS);
        assert_eq!(a.hints_remaining(), MAX_HINTS - 1);
    }

    #[test]
    fn a_hint_can_be_asked_for_by_name_rather_than_always_the_same_word() {
        // The old `H` was the only way to ask and always revealed the first
        // unfound word, so a second hint said what the first had. Clicking a
        // word in the list names the word.
        let mut a = game(31);
        let second = a.words()[1].cells()[0];
        assert_eq!(probe::click(&mut a, Target::Word(1)), EventResult::Consumed);
        let lit = a.hint().expect("clicking a word lit nothing");
        assert_eq!((lit.row, lit.col), second);
        assert_ne!(
            (lit.row, lit.col),
            a.words()[0].cells()[0],
            "the list is one button"
        );
    }

    #[test]
    fn a_hint_fades_on_its_own_and_the_board_goes_back_to_normal() {
        let mut a = game(31);
        key(&mut a, Key::H);
        let lit = a.hint().unwrap();
        assert_eq!(tick(&mut a, HINT_STEP_MS), EventResult::Consumed);
        assert_eq!(a.hint().unwrap().remaining_ms, HINT_MS - HINT_STEP_MS);
        assert_eq!(
            a.hint().unwrap().row,
            lit.row,
            "the hint moved while it burned"
        );

        tick(&mut a, HINT_MS);
        assert_eq!(a.hint(), None, "the hint never went out");
        // The old file counted a `ticks` field down from ten and decremented it
        // nowhere, so `ticks > 0` was permanently true.
        for _ in 0..50 {
            tick(&mut a, HINT_STEP_MS);
        }
        assert_eq!(a.hint(), None);
    }

    #[test]
    fn a_hint_burns_down_at_wall_clock_speed_not_per_wake_up() {
        let mut a = game(31);
        key(&mut a, Key::H);
        let mut fast = a.hint().unwrap().remaining_ms;
        for _ in 0..10 {
            tick(&mut a, 50);
        }
        fast -= 500;
        assert_eq!(a.hint().unwrap().remaining_ms, fast);

        let mut b = game(31);
        key(&mut b, Key::H);
        tick(&mut b, 500);
        assert_eq!(
            b.hint().unwrap().remaining_ms,
            a.hint().unwrap().remaining_ms,
            "ten small ticks and one big one left different amounts of hint"
        );
    }

    #[test]
    fn hints_run_out_and_the_chip_says_so() {
        let mut a = game(31);
        for left in (0..MAX_HINTS).rev() {
            assert_eq!(key(&mut a, Key::H), EventResult::Consumed);
            assert_eq!(a.hints_remaining(), left);
            tick(&mut a, HINT_MS);
        }
        assert_eq!(a.hints_remaining(), 0);
        assert_eq!(
            key(&mut a, Key::H),
            EventResult::Ignored,
            "a sixth hint was given"
        );
        assert_eq!(a.hint(), None);
        assert!(says(&a, SIZE, "Hint 0"), "{:?}", strings(&a, SIZE));
        assert_eq!(
            probe::click(&mut a, Target::HintButton),
            EventResult::Ignored,
            "the chip gave a hint the keyboard would not"
        );
    }

    #[test]
    fn a_hint_is_never_spent_on_a_word_already_found() {
        let mut a = game(31);
        let cells = first_unfound(&a);
        spell_out(&mut a, &cells);
        assert!(a.words()[0].found);
        assert_eq!(
            a.apply(Action::HintFor(0)),
            EventResult::Ignored,
            "a found word took a hint"
        );
        assert_eq!(a.hints_remaining(), MAX_HINTS);
        assert_eq!(a.hint(), None);
        // And `H` skips it rather than pointing at it again.
        key(&mut a, Key::H);
        let lit = a.hint().unwrap();
        assert!(
            !cells.contains(&(lit.row, lit.col)),
            "H pointed at a found word"
        );
    }

    #[test]
    fn a_hint_for_a_word_this_puzzle_does_not_have_is_refused() {
        let mut a = game(31);
        let past_the_end = a.total_words();
        assert_eq!(a.apply(Action::HintFor(past_the_end)), EventResult::Ignored);
        assert_eq!(a.apply(Action::HintFor(usize::MAX)), EventResult::Ignored);
        assert_eq!(a.hints_remaining(), MAX_HINTS);
    }

    #[test]
    fn a_won_board_gives_no_more_hints() {
        let mut a = game(31);
        let all: Vec<Vec<(usize, usize)>> = a.words().iter().map(PlacedWord::cells).collect();
        for cells in &all {
            spell_out(&mut a, cells);
        }
        assert_eq!(a.status(), GameStatus::Won);
        assert_eq!(key(&mut a, Key::H), EventResult::Ignored);
        assert_eq!(a.apply(Action::HintFor(0)), EventResult::Ignored);
        assert_eq!(a.hints_remaining(), MAX_HINTS);
    }

    // ── The clock ──────────────────────────────────────────────────────────

    #[test]
    fn the_clock_counts_the_time_it_is_given_not_the_ticks_it_gets() {
        let mut a = game(41);
        assert_eq!(a.elapsed_ms(), 0);
        assert_eq!(tick(&mut a, 250), EventResult::Consumed);
        assert_eq!(a.elapsed_ms(), 250);
        assert_eq!(a.elapsed_secs(), 0, "a quarter second is not a second");
        tick(&mut a, 750);
        assert_eq!(a.elapsed_secs(), 1);
        tick(&mut a, 1_999);
        assert_eq!(a.elapsed_ms(), 2_999);
        assert_eq!(a.elapsed_secs(), 2);

        let mut b = game(41);
        for _ in 0..4 {
            tick(&mut b, 750);
        }
        assert_eq!(b.elapsed_ms(), 3_000, "four 750ms ticks are three seconds");
        assert_eq!(b.elapsed_secs(), 3);
        assert_eq!(
            a.elapsed_secs(),
            2,
            "one millisecond short is still two seconds"
        );
    }

    #[test]
    fn a_new_game_puts_the_clock_and_the_hints_back() {
        let mut a = game(41);
        tick(&mut a, 90_000);
        key(&mut a, Key::H);
        walk_to(&mut a, (2, 2));
        key(&mut a, Key::Enter);
        assert_eq!(key(&mut a, Key::F2), EventResult::Consumed);
        assert_eq!(a.elapsed_ms(), 0);
        assert_eq!(a.hints_remaining(), MAX_HINTS);
        assert_eq!(a.hint(), None);
        assert_eq!(a.cursor(), (0, 0));
        assert_eq!(a.selection(), Selection::None);
        assert!(!a.dragging());
        assert_eq!(a.found_count(), 0);
        assert_eq!(a.status(), GameStatus::Playing);
    }

    #[test]
    fn a_new_game_at_the_same_settings_is_a_different_board() {
        let mut a = game(41);
        let before: Vec<String> = a.words().iter().map(|w| w.word.clone()).collect();
        let starts: Vec<(usize, usize)> = a.words().iter().map(|w| w.start).collect();
        key(&mut a, Key::F2);
        let after: Vec<String> = a.words().iter().map(|w| w.word.clone()).collect();
        let after_starts: Vec<(usize, usize)> = a.words().iter().map(|w| w.start).collect();
        assert_eq!(
            a.difficulty(),
            Difficulty::Medium,
            "F2 changed the settings"
        );
        assert_eq!(a.category(), Category::Animals);
        assert!(
            before != after || starts != after_starts,
            "a new game handed back the same puzzle"
        );
    }

    #[test]
    fn a_tick_that_moves_nothing_is_not_a_redraw() {
        let mut a = game(41);
        let all: Vec<Vec<(usize, usize)>> = a.words().iter().map(PlacedWord::cells).collect();
        for cells in &all {
            spell_out(&mut a, cells);
        }
        assert_eq!(tick(&mut a, 1_000), EventResult::Ignored);
        // Unless a hint is still burning, which is its own reason to repaint.
        assert_eq!(a.apply(Action::HintFor(0)), EventResult::Ignored);
    }

    #[test]
    fn a_hint_still_burns_on_a_board_whose_clock_has_stopped() {
        let mut a = game(41);
        key(&mut a, Key::H);
        let all: Vec<Vec<(usize, usize)>> = a.words().iter().map(PlacedWord::cells).collect();
        for cells in &all {
            spell_out(&mut a, cells);
        }
        assert_eq!(a.status(), GameStatus::Won);
        assert!(a.hint().is_some(), "the win blew the hint out");
        assert_eq!(
            tick(&mut a, 100),
            EventResult::Consumed,
            "a burning hint is a redraw"
        );
        assert!(a.animating());
        tick(&mut a, HINT_MS);
        assert_eq!(a.hint(), None);
        assert!(!a.animating());
    }

    // ── Difficulty and category ────────────────────────────────────────────

    #[test]
    fn d_and_c_start_a_new_game_one_step_on() {
        let mut a = game(51);
        assert_eq!(a.difficulty(), Difficulty::Medium);
        assert_eq!(a.category(), Category::Animals);
        assert_eq!(key(&mut a, Key::D), EventResult::Consumed);
        assert_eq!(a.difficulty(), Difficulty::Hard);
        assert_eq!(
            a.category(),
            Category::Animals,
            "D changed the category too"
        );
        assert_eq!(key(&mut a, Key::C), EventResult::Consumed);
        assert_eq!(a.category(), Category::Colors);
        assert_eq!(
            a.difficulty(),
            Difficulty::Hard,
            "C changed the difficulty too"
        );
        assert_eq!(a.grid_size(), Difficulty::Hard.grid_size());
    }

    #[test]
    fn a_new_category_hides_words_from_that_category() {
        let mut a = game(51);
        for _ in 0..Category::ALL.len() {
            let category = a.category();
            for placed in a.words() {
                assert!(
                    category.words().contains(&placed.word.as_str()),
                    "{} is not a {} word",
                    placed.word,
                    category.label()
                );
            }
            key(&mut a, Key::C);
        }
        assert_eq!(
            a.category(),
            Category::Animals,
            "the C key does not come round"
        );
    }

    #[test]
    fn ctrl_and_a_digit_pick_a_difficulty_outright() {
        let mut a = game(51);
        for (k, expected) in [
            (Key::Num3, Difficulty::Hard),
            (Key::Num1, Difficulty::Easy),
            (Key::Num2, Difficulty::Medium),
        ] {
            assert_eq!(
                handle_event(&mut a, &Event::Key(ctrl(k))),
                EventResult::Consumed
            );
            assert_eq!(a.difficulty(), expected);
            assert_eq!(a.grid_size(), expected.grid_size());
        }
        // The same digit without Ctrl is not a difficulty.
        assert_eq!(key(&mut a, Key::Num1), EventResult::Ignored);
        assert_eq!(a.difficulty(), Difficulty::Medium);
        // And Ctrl with a key that is not a digit is nothing.
        assert_eq!(
            handle_event(&mut a, &Event::Key(ctrl(Key::H))),
            EventResult::Ignored
        );
        assert_eq!(a.hints_remaining(), MAX_HINTS, "Ctrl-H spent a hint");
    }

    #[test]
    fn the_chips_do_what_the_keys_do() {
        for (target, key_pressed) in [
            (Target::Difficulty, Key::D),
            (Target::Category, Key::C),
            (Target::NewGame, Key::F2),
        ] {
            let mut clicked = game(61);
            let mut typed = game(61);
            assert_eq!(probe::click(&mut clicked, target), EventResult::Consumed);
            assert_eq!(key(&mut typed, key_pressed), EventResult::Consumed);
            assert_eq!(clicked.difficulty(), typed.difficulty(), "{target:?}");
            assert_eq!(clicked.category(), typed.category(), "{target:?}");
            let a: Vec<&str> = clicked.words().iter().map(|w| w.word.as_str()).collect();
            let b: Vec<&str> = typed.words().iter().map(|w| w.word.as_str()).collect();
            assert_eq!(a, b, "{target:?} built a different puzzle from its key");
        }
    }

    #[test]
    fn a_key_this_program_does_not_answer_is_left_alone() {
        let mut a = game(61);
        for k in [Key::Tab, Key::Space, Key::Q, Key::Z, Key::Num1] {
            assert_eq!(key(&mut a, k), EventResult::Ignored, "{k:?} did something");
        }
        // A key going up is not a key going down.
        let mut release = press(Key::Enter);
        release.pressed = false;
        assert_eq!(
            handle_event(&mut a, &Event::Key(release)),
            EventResult::Ignored
        );
        assert_eq!(
            a.selection(),
            Selection::None,
            "a key release anchored a mark"
        );
        // Nor is Alt-Enter Enter.
        let alt = press_with(
            Key::Enter,
            Modifiers {
                alt: true,
                ..Modifiers::NONE
            },
        );
        assert_eq!(handle_event(&mut a, &Event::Key(alt)), EventResult::Ignored);
    }

    // ── Layout ─────────────────────────────────────────────────────────────

    #[test]
    fn every_band_stays_inside_the_window_at_every_size() {
        for (w, h) in SIZES {
            let l = Layout::new(w, h, 15, 10);
            assert_eq!(l.window, Rect::new(0.0, 0.0, w.max(1.0), h.max(1.0)));
            for (name, band) in [
                ("header", l.header),
                ("board", l.board),
                ("list", l.list),
                ("footer", l.footer),
            ] {
                if band.is_empty() {
                    continue;
                }
                assert!(
                    band.x >= -0.01,
                    "{name} starts left of the window at {w}x{h}"
                );
                assert!(band.y >= -0.01, "{name} starts above the window at {w}x{h}");
                assert!(
                    band.right() <= l.window.right() + 0.01,
                    "{name} runs past the right edge at {w}x{h}"
                );
                assert!(
                    band.bottom() <= l.window.bottom() + 0.01,
                    "{name} runs past the bottom at {w}x{h}"
                );
            }
        }
    }

    #[test]
    fn the_bands_do_not_sit_on_top_of_each_other() {
        for (w, h) in SIZES {
            let l = Layout::new(w, h, 15, 10);
            for (an, a) in [("header", l.header), ("board", l.board), ("list", l.list)] {
                for (bn, b) in [("board", l.board), ("list", l.list), ("footer", l.footer)] {
                    if an == bn || a.is_empty() || b.is_empty() {
                        continue;
                    }
                    assert!(
                        a.intersect(b).is_none(),
                        "{an} overlaps {bn} at {w}x{h}: {a:?} vs {b:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_window_too_short_for_everything_drops_the_footer_before_the_header() {
        // The title, the clock and the chips are the controls; the shortcut
        // strip is a reminder. A board with no chips is unplayable.
        let tall = Layout::new(800.0, 600.0, 15, 10);
        assert!(!tall.header.is_empty() && !tall.footer.is_empty());

        let short = Layout::new(800.0, 90.0, 15, 10);
        assert!(
            short.footer.is_empty(),
            "the footer outlived the header's room"
        );
        assert!(
            !short.header.is_empty(),
            "the header went before the footer"
        );

        let tiny = Layout::new(800.0, 20.0, 15, 10);
        assert!(tiny.footer.is_empty() && tiny.header.is_empty());
        // The board is never dropped: a word search with no board is a blank
        // window, not a smaller word search.
        assert!(!tiny.board.is_empty(), "the board was dropped");
    }

    #[test]
    fn a_dropped_band_is_empty_rather_than_a_strip_of_no_height() {
        let short = Layout::new(800.0, 90.0, 15, 10);
        assert_eq!(short.footer, Rect::EMPTY);
        assert!(!short.shows(short.footer));
        // Which is a different thing from a band that is merely thin.
        let thin = Layout::new(800.0, 600.0, 15, 10);
        assert!(thin.shows(thin.footer));
        assert!(!thin.shows(Rect::new(0.0, 0.0, 100.0, thin.font / 2.0)));
    }

    #[test]
    fn the_board_keeps_its_share_of_a_short_window() {
        for h in [200.0f32, 300.0, 600.0, 1080.0] {
            let l = Layout::new(900.0, h, 15, 10);
            assert!(
                l.board.h >= h * BOARD_SHARE - l.pad * 2.0 - 0.01,
                "the board got {} of {h}",
                l.board.h
            );
        }
    }

    #[test]
    fn the_word_list_goes_entirely_rather_than_becoming_a_column_of_ellipses() {
        let wide = Layout::new(900.0, 600.0, 15, 10);
        assert!(!wide.list.is_empty(), "a wide window has room for the list");
        assert!(wide.list.w >= wide.big * 3.0);

        let narrow = Layout::new(120.0, 600.0, 15, 10);
        assert_eq!(
            narrow.list,
            Rect::EMPTY,
            "a narrow window kept a useless list"
        );
        // And the board takes the room the list gave up.
        assert!(
            narrow.board.w > wide.board.w / wide.window.w * narrow.window.w * 0.9,
            "the board did not take the list's room"
        );
        assert_eq!(narrow.word_row(0), Rect::EMPTY);
    }

    #[test]
    fn cells_are_square_and_the_last_one_ends_on_the_board() {
        for (w, h) in SIZES {
            for size in [10usize, 15, 20] {
                let l = Layout::new(w, h, size, 10);
                let grid = l.grid_rect();
                if grid.is_empty() {
                    continue;
                }
                assert!(
                    close(grid.w, grid.h),
                    "the grid is not square at {w}x{h}/{size}"
                );
                let first = l.cell_rect(0, 0);
                let last = l.cell_rect(size - 1, size - 1);
                assert!(
                    close(first.w, first.h),
                    "a cell is not square at {w}x{h}/{size}"
                );
                assert!(close(first.x, grid.x) && close(first.y, grid.y));
                assert!(
                    close(last.right(), grid.right()) && close(last.bottom(), grid.bottom()),
                    "the last cell does not finish on the grid at {w}x{h}/{size}"
                );
                assert!(
                    l.board.intersect(grid).is_some(),
                    "the grid is not inside the board at {w}x{h}/{size}"
                );
                assert!(
                    grid.w <= l.board.w.min(l.board.h) + 0.01,
                    "the grid is wider than the board at {w}x{h}/{size}"
                );
            }
        }
    }

    #[test]
    fn the_grid_is_centred_in_the_room_it_is_given() {
        let l = Layout::new(900.0, 700.0, 15, 10);
        let grid = l.grid_rect();
        assert!(close(grid.x - l.board.x, l.board.right() - grid.right()));
        assert!(close(grid.y - l.board.y, l.board.bottom() - grid.bottom()));
    }

    #[test]
    fn no_two_cells_share_a_pixel_and_the_gaps_are_equal() {
        let l = Layout::new(900.0, 700.0, 15, 10);
        for row in 0..15 {
            for col in 0..14 {
                let a = l.cell_rect(row, col);
                let b = l.cell_rect(row, col + 1);
                assert!(
                    a.intersect(b).is_none(),
                    "cells {row},{col} and {row},{col}+1 overlap"
                );
                assert!(close(b.x - a.right(), l.gap), "the gap is not the gap");
            }
        }
        for row in 0..14 {
            let a = l.cell_rect(row, 3);
            let b = l.cell_rect(row + 1, 3);
            assert!(close(b.y - a.bottom(), l.gap));
        }
    }

    #[test]
    fn a_cell_this_board_does_not_have_has_no_box() {
        let l = Layout::new(900.0, 700.0, 15, 10);
        assert_eq!(l.cell_rect(15, 0), Rect::EMPTY);
        assert_eq!(l.cell_rect(0, 15), Rect::EMPTY);
        assert_eq!(l.cell_rect(usize::MAX, usize::MAX), Rect::EMPTY);
        assert!(!l.cell_rect(14, 14).is_empty());
        // A board of no cells has no grid at all rather than a grid of nothing.
        let none = Layout::new(900.0, 700.0, 0, 0);
        assert_eq!(none.grid_rect(), Rect::EMPTY);
        assert_eq!(none.cell_rect(0, 0), Rect::EMPTY);
    }

    #[test]
    fn the_chips_sit_in_the_header_side_by_side_without_touching() {
        let l = Layout::new(900.0, 700.0, 15, 10);
        let mut last: Option<Rect> = None;
        for i in 0..CHIPS {
            let c = l.chip(i);
            assert!(!c.is_empty(), "chip {i} has no box");
            assert!(
                l.header.intersect(c).is_some_and(|r| close(r.w, c.w)),
                "chip {i} hangs outside the header"
            );
            if let Some(prev) = last {
                assert!(
                    prev.intersect(c).is_none(),
                    "chips {i} and {} overlap",
                    i - 1
                );
                assert!(c.x > prev.right(), "the chips are out of order");
                assert!(close(c.w, prev.w), "the chips are different widths");
            }
            last = Some(c);
        }
        assert_eq!(l.chip(CHIPS), Rect::EMPTY, "there is a fifth chip");
    }

    #[test]
    fn there_is_no_chip_where_there_is_no_header() {
        let short = Layout::new(800.0, 20.0, 15, 10);
        assert!(short.header.is_empty());
        for i in 0..CHIPS {
            assert_eq!(short.chip(i), Rect::EMPTY, "chip {i} outlived the header");
        }
        // Nor where the header is too narrow to hold four of them.
        let narrow = Layout::new(4.0, 700.0, 15, 10);
        for i in 0..CHIPS {
            assert_eq!(narrow.chip(i), Rect::EMPTY);
        }
    }

    #[test]
    fn the_word_rows_run_down_the_list_in_order_without_overlapping() {
        let l = Layout::new(900.0, 700.0, 15, 10);
        let mut last: Option<Rect> = None;
        for i in 0..10 {
            let r = l.word_row(i);
            assert!(!r.is_empty(), "word row {i} has no box");
            assert!(r.y >= l.list.y - 0.01, "row {i} is above the list");
            assert!(
                r.bottom() <= l.list.bottom() + 0.01,
                "row {i} is below the list"
            );
            if let Some(prev) = last {
                assert!(
                    prev.intersect(r).is_none(),
                    "rows {i} and {} overlap",
                    i - 1
                );
                assert!(r.y > prev.y);
            }
            last = Some(r);
        }
        assert_eq!(l.word_row(10), Rect::EMPTY, "there is an eleventh word");
        assert_eq!(l.word_row(usize::MAX), Rect::EMPTY);
    }

    #[test]
    fn a_longer_list_still_fits_the_same_column() {
        for words in [8usize, 10, 12] {
            let l = Layout::new(900.0, 700.0, 15, words);
            let last = l.word_row(words - 1);
            assert!(!last.is_empty(), "{words} words did not fit");
            assert!(
                last.bottom() <= l.list.bottom() + 0.01,
                "{words} words overflowed"
            );
        }
    }

    // ── Drawing ────────────────────────────────────────────────────────────

    #[test]
    fn the_background_is_the_window_and_not_the_board() {
        // `render` used to paint a rectangle sized from the grid's cell count,
        // so a window bigger than 780x600 showed bare compositor behind it.
        for (w, h) in SIZES {
            let a = game(71);
            let f = a.frame(w, h);
            let first = f.commands().first().expect("an empty frame");
            match *first {
                RenderCommand::FillRect {
                    x,
                    y,
                    width,
                    height,
                    ..
                } => {
                    assert!(
                        close(x, 0.0) && close(y, 0.0),
                        "the background is offset at {w}x{h}"
                    );
                    assert!(
                        width >= w.max(1.0) - 0.01 && height >= h.max(1.0) - 0.01,
                        "the background is {width}x{height} in a {w}x{h} window"
                    );
                }
                ref other => panic!("the frame does not open with a background: {other:?}"),
            }
            assert!(f.is_balanced(), "clips are unbalanced at {w}x{h}");
        }
    }

    #[test]
    fn every_cell_of_the_board_is_clickable_and_answers_for_itself() {
        let a = game(71);
        let f = a.frame(SIZE.0, SIZE.1);
        let l = layout_of(&a);
        for row in 0..a.grid_size() {
            for col in 0..a.grid_size() {
                let (x, y) = l.cell_rect(row, col).centre();
                assert_eq!(
                    f.hit_test(x, y),
                    Some(Target::Cell(row, col)),
                    "the middle of {row},{col} is not {row},{col}"
                );
            }
        }
    }

    #[test]
    fn every_letter_on_the_board_is_drawn() {
        let a = game(71);
        let drawn = strings(&a, SIZE);
        let n = a.grid_size();
        let mut single: Vec<&String> = drawn.iter().filter(|s| s.len() == 1).collect();
        assert_eq!(
            single.len(),
            n * n,
            "{} letters drawn for {} cells",
            single.len(),
            n * n
        );
        single.sort();
        let mut want: Vec<String> = (0..n)
            .flat_map(|r| (0..n).map(move |c| (r, c)))
            .map(|(r, c)| String::from(char::from(a.letter(r, c).unwrap())))
            .collect();
        want.sort();
        assert_eq!(single.into_iter().cloned().collect::<Vec<_>>(), want);
    }

    #[test]
    fn every_word_in_the_list_is_named_and_clickable() {
        let a = game(71);
        let drawn = strings(&a, SIZE);
        for (i, placed) in a.words().iter().enumerate() {
            assert!(
                drawn.contains(&placed.word),
                "{} is not in the list",
                placed.word
            );
            assert!(
                probe::is_visible(&a, Target::Word(i)),
                "{} cannot be clicked",
                placed.word
            );
        }
        assert!(!probe::is_visible(&a, Target::Word(a.total_words())));
    }

    #[test]
    fn a_found_word_is_struck_through_and_a_missing_one_is_not() {
        let mut a = game(71);
        let rules = |g: &WordSearchApp| {
            g.frame(SIZE.0, SIZE.1)
                .commands()
                .iter()
                .filter(|c| matches!(c, RenderCommand::Line { .. }))
                .count()
        };
        assert_eq!(rules(&a), 0, "an untouched list is already struck through");
        let cells = first_unfound(&a);
        spell_out(&mut a, &cells);
        assert_eq!(rules(&a), 1, "the found word was not struck through");
    }

    #[test]
    fn a_found_word_is_greyed_and_its_letters_go_green() {
        let mut a = game(71);
        let word = a.words()[0].word.clone();
        let before = colour_of(&a, SIZE, &word).expect("the word is not in the list");
        // Before anything is found, nothing is found. This test used to look
        // only at the after, and the sweep showed what that missed: dropping
        // the `w.found &&` from `is_found_cell` makes every hidden word's
        // letters green from the first frame -- the whole puzzle solved on
        // sight -- and every assertion below still held. A test that only
        // watches a value change cannot see a value that started wrong.
        for row in 0..a.grid_size() {
            for col in 0..a.grid_size() {
                assert!(
                    !a.is_found_cell(row, col),
                    "({row}, {col}) is found on a board where nothing has been found"
                );
            }
        }
        let cells = first_unfound(&a);
        spell_out(&mut a, &cells);
        let after = colour_of(&a, SIZE, &word).expect("the word left the list");
        assert_ne!(
            before, after,
            "finding {word} changed nothing about the list"
        );
        assert_eq!(after, OVERLAY0);
        for &(r, c) in &cells {
            assert!(a.is_found_cell(r, c));
        }
    }

    #[test]
    fn the_lit_hint_is_the_only_cell_painted_yellow() {
        let count_yellow = |g: &WordSearchApp| {
            g.frame(SIZE.0, SIZE.1)
                .commands()
                .iter()
                .filter(|c| matches!(c, RenderCommand::FillRect { color, .. } if *color == YELLOW))
                .count()
        };
        let mut a = game(71);
        assert_eq!(
            count_yellow(&a),
            0,
            "something was lit before a hint was asked for"
        );
        key(&mut a, Key::H);
        assert_eq!(count_yellow(&a), 1);
        tick(&mut a, HINT_MS);
        assert_eq!(
            count_yellow(&a),
            0,
            "the hint is still lit after it went out"
        );
    }

    #[test]
    fn the_footer_tells_the_player_what_the_keys_do_and_changes_mid_mark() {
        let mut a = game(71);
        for (k, what) in SHORTCUTS {
            assert!(says(&a, SIZE, k), "the footer never mentions {k}");
            assert!(says(&a, SIZE, what), "the footer never says what {k} does");
        }
        assert!(says(&a, SIZE, "H:hint"));

        key(&mut a, Key::Enter);
        assert!(says(&a, SIZE, "Enter:confirm"), "{:?}", strings(&a, SIZE));
        assert!(
            !says(&a, SIZE, "H:hint"),
            "the mid-mark footer offers a hint key"
        );
        for (k, what) in SELECTING_SHORTCUTS {
            assert!(says(&a, SIZE, &format!("{k}:{what}")));
        }
        key(&mut a, Key::Escape);
        assert!(says(&a, SIZE, "H:hint"), "the footer did not come back");
    }

    #[test]
    fn the_shortcut_tables_name_keys_the_program_answers() {
        assert!(SHORTCUTS.len() >= SELECTING_SHORTCUTS.len());
        for table in [SHORTCUTS, SELECTING_SHORTCUTS] {
            assert!(!table.is_empty());
            for (i, (k, what)) in table.iter().enumerate() {
                assert!(!k.is_empty() && !what.is_empty());
                for (j, (other, _)) in table.iter().enumerate() {
                    assert!(i == j || k != other, "{k} is listed twice");
                }
            }
        }
        // Every key the footer promises really does something.
        let mut a = game(71);
        for k in [Key::Enter, Key::Escape, Key::H, Key::D, Key::C, Key::F2] {
            let mut fresh = game(71);
            if k == Key::Escape {
                key(&mut fresh, Key::Enter);
            }
            assert_eq!(
                key(&mut fresh, k),
                EventResult::Consumed,
                "{k:?} does nothing"
            );
        }
        assert_eq!(
            key(&mut a, Key::Right),
            EventResult::Consumed,
            "Arrows do nothing"
        );
    }

    #[test]
    fn the_header_counts_what_is_found_out_of_what_there_is() {
        let mut a = game(71);
        let total = a.total_words();
        assert!(
            says(&a, SIZE, &format!("0/{total} found")),
            "{:?}",
            strings(&a, SIZE)
        );
        let cells = first_unfound(&a);
        spell_out(&mut a, &cells);
        assert!(
            says(&a, SIZE, &format!("1/{total} found")),
            "{:?}",
            strings(&a, SIZE)
        );
    }

    #[test]
    fn winning_turns_the_header_line_green() {
        let mut a = game(71);
        let playing = colour_of(&a, SIZE, "found").expect("no count in the header");
        assert_eq!(playing, LAVENDER);
        let all: Vec<Vec<(usize, usize)>> = a.words().iter().map(PlacedWord::cells).collect();
        for cells in &all {
            spell_out(&mut a, cells);
        }
        assert_eq!(
            colour_of(&a, SIZE, "found"),
            Some(GREEN),
            "the win is not announced"
        );
    }

    #[test]
    fn the_chips_say_the_settings_they_change() {
        let mut a = game(71);
        for _ in 0..Difficulty::ALL.len() {
            assert!(
                says(&a, SIZE, a.difficulty().label()),
                "the chip does not name the level"
            );
            assert!(
                says(&a, SIZE, a.category().label()),
                "the chip does not name the category"
            );
            assert_eq!(
                colour_of(&a, SIZE, a.difficulty().label()),
                Some(a.difficulty().color())
            );
            key(&mut a, Key::D);
        }
        assert!(says(&a, SIZE, "New"));
        assert!(says(&a, SIZE, "Word Search"));
    }

    #[test]
    fn a_window_with_no_room_still_draws_something_and_crashes_at_no_size() {
        for (w, h) in SIZES {
            let a = game(71);
            let f = a.frame(w, h);
            assert!(!f.commands().is_empty(), "{w}x{h} drew nothing at all");
            assert!(f.is_balanced());
            // Every hit box that exists is inside the window it was drawn for.
            for (target, r) in f.hits() {
                assert!(
                    r.x >= -0.01 && r.y >= -0.01 && r.right() <= w.max(1.0) + 0.01,
                    "{target:?} is outside a {w}x{h} window: {r:?}"
                );
            }
        }
    }

    #[test]
    fn a_click_is_read_against_the_window_the_player_is_looking_at() {
        // The model remembers the size it last drew at, so a click in a resized
        // window lands on the cell that is under it there and not under it in a
        // 820x620 one.
        let mut a = game(81);
        let small = (400u32, 300u32);
        assert_eq!(
            handle_event(
                &mut a,
                &Event::Resize {
                    width: small.0,
                    height: small.1
                }
            ),
            EventResult::Consumed
        );
        assert_eq!(a.size(), (400.0, 300.0));
        let (x, y) = cell_point(&a, (7, 7));
        mouse(&mut a, x, y, MouseEventKind::Press(MouseButton::Left));
        assert_eq!(
            a.cursor(),
            (7, 7),
            "the click was read against the wrong window"
        );

        // The same point in the default window is a different cell.
        let mut b = game(81);
        b.resize(WINDOW_WIDTH, WINDOW_HEIGHT);
        mouse(&mut b, x, y, MouseEventKind::Press(MouseButton::Left));
        assert_ne!(
            b.cursor(),
            (7, 7),
            "two window sizes read one click the same way"
        );
    }

    #[test]
    fn a_window_is_never_smaller_than_one_pixel_to_the_model() {
        let mut a = game(81);
        a.resize(0.0, 0.0);
        assert_eq!(a.size(), (1.0, 1.0));
        a.resize(-50.0, -50.0);
        assert_eq!(a.size(), (1.0, 1.0));
        // And a frame at that size is still a frame.
        assert!(!a.frame(a.size().0, a.size().1).commands().is_empty());
    }

    // ── The window ─────────────────────────────────────────────────────────

    #[test]
    fn the_window_asks_for_ticks_exactly_while_something_is_moving() {
        let mut a = game(91);
        assert_eq!(a.tick_interval(), Some(Duration::from_millis(CLOCK_MS)));
        // A burning hint needs a finer wake-up than the clock does.
        key(&mut a, Key::H);
        assert_eq!(a.tick_interval(), Some(Duration::from_millis(HINT_STEP_MS)));
        const { assert!(HINT_STEP_MS < CLOCK_MS) };
        tick(&mut a, HINT_MS);
        assert_eq!(a.tick_interval(), Some(Duration::from_millis(CLOCK_MS)));

        let all: Vec<Vec<(usize, usize)>> = a.words().iter().map(PlacedWord::cells).collect();
        for cells in &all {
            spell_out(&mut a, cells);
        }
        assert_eq!(a.tick_interval(), None, "a won board still wakes up");
        assert_eq!(a.tick_interval().is_some(), a.animating());
    }

    #[test]
    fn an_event_that_changes_something_redraws_and_one_that_does_not_idles() {
        let mut a = game(91);
        assert!(matches!(
            a.on_event(&Event::Key(press(Key::Right))),
            Response::Redraw
        ));
        assert!(matches!(
            a.on_event(&Event::Key(press(Key::Tab))),
            Response::Idle
        ));
        assert!(matches!(a.on_event(&Event::CloseRequested), Response::Exit));
        // The events this program does not model are not errors.
        for event in [
            Event::Moved { x: 10, y: 10 },
            Event::FocusIn,
            Event::FocusOut,
            Event::ScaleChanged { scale: 2.0 },
        ] {
            assert_eq!(
                handle_event(&mut a, &event),
                EventResult::Ignored,
                "{event:?}"
            );
        }
    }

    #[test]
    fn rendering_records_the_size_the_next_click_is_read_against() {
        let mut a = game(91);
        let tree = a.render(500.0, 400.0);
        assert_eq!(a.size(), (500.0, 400.0));
        assert!(!tree.commands.is_empty());
        // Which is the same frame `draw` hands a probe.
        assert_eq!(
            a.draw((500.0, 400.0)).commands().len(),
            tree.commands.len(),
            "the probe and the window see different frames"
        );
    }

    #[test]
    fn the_window_names_itself_and_opens_at_a_size_the_board_fits_in() {
        let a = game(91);
        assert_eq!(a.title(), "Word Search");
        assert_eq!(a.app_id(), "wordsearch");
        assert_eq!(a.initial_size(), (820, 620));
        let (w, h) = a.initial_size();
        let l = Layout::new(w as f32, h as f32, Difficulty::Hard.grid_size(), 12);
        assert!(
            !l.grid_rect().is_empty(),
            "the hardest board does not fit the window"
        );
        assert!(!l.list.is_empty() && !l.header.is_empty() && !l.footer.is_empty());
    }

    #[test]
    fn a_default_game_is_a_real_game() {
        let a = WordSearchApp::default();
        assert_eq!(a.grid_size(), Difficulty::Medium.grid_size());
        assert_eq!(a.total_words(), Difficulty::Medium.word_count());
        assert_eq!(a.status(), GameStatus::Playing);
        assert_eq!(a.size(), (WINDOW_WIDTH, WINDOW_HEIGHT));
        assert!(a.letter(0, 0).is_some());
    }

    #[test]
    fn every_control_the_window_draws_can_be_reached_by_name() {
        let a = game(91);
        let names = probe::control_names(&a);
        for wanted in [
            "Cell",
            "Word",
            "Difficulty",
            "Category",
            "HintButton",
            "NewGame",
        ] {
            assert!(
                names.iter().any(|n| n == wanted),
                "{wanted} is drawn nowhere; the window offers {names:?}"
            );
        }
        for target in [
            Target::Difficulty,
            Target::Category,
            Target::HintButton,
            Target::NewGame,
            Target::Word(0),
            Target::Cell(0, 0),
        ] {
            assert!(probe::is_visible(&a, target), "{target:?} is not on screen");
        }
    }
}
