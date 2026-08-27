//! Tower of Hanoi — move the stack, one disk at a time, never a big disk onto
//! a small one.
//!
//! Three pegs, three to eight disks, and a shortest solution of exactly
//! `2^n - 1` moves. The program can also solve the position it is in — from
//! *any* legal position, not just the opening one — and will step through that
//! solution on a clock so you can watch how it goes.
//!
//! ## What wiring this up found
//!
//! `main` built a `TowersOfHanoi`, dropped it and exited, so no peg ever
//! reached a screen and no key or click ever arrived. Underneath that the
//! puzzle was not merely unpolished, it was **unplayable**, in six distinct
//! ways:
//!
//! 1. **Every key fired twice**, because the handler matched
//!    `Event::Key(KeyEvent { key, modifiers, .. })` and never read `pressed`.
//!    On a three-peg puzzle that is fatal rather than annoying:
//!    - `Left`/`Right` moved *two* pegs per press, and both are clamped at the
//!      ends — so from peg 1 a press of Right landed on peg 3 and from peg 3 a
//!      press of Left landed on peg 1. **The middle peg could not be selected
//!      from either end**, and every Hanoi solution needs the middle peg. The
//!      puzzle was unsolvable from the keyboard.
//!    - `Enter` picked a disk up on the press and put it straight back down on
//!      the release — `try_place` onto the peg you took it from just cancels —
//!      so `Enter` did nothing at all, ever.
//!    - `Z` undid two moves per press, `H` opened the help panel on the press
//!      and closed it on the release (so help never appeared), and `Up`/`Down`
//!      stepped the disk count by two, which is why **5 and 7 disks could not
//!      be reached** and their best-score rows read `---` forever.
//! 2. **The mouse hit test never looked at `y`.** It tested `x` against
//!    `[60, 660]` and nothing else, so a click on the title "Tower of Hanoi",
//!    on the move counter, or on the "Solved in N moves!" line — all inside
//!    that band — picked up or dropped a disk. Hit boxes are now recorded by
//!    the same pass that draws the pegs, so a column's clickable area cannot
//!    drift from the pixels naming it.
//! 3. **The layout was a constant.** `render(width, height)` used its two
//!    arguments for the background rectangle and for nothing else. Pegs were
//!    always 200px wide starting at x=60; the best-scores panel was pinned at
//!    x=690 with a fixed 160x220, so it fell off the right edge of any window
//!    narrower than 850px, and the help panel below it ran to y=530, so it fell
//!    off the bottom of any window shorter than that. Meanwhile the click bands
//!    stayed at 60/200 whatever the window did, so on a resized window the pegs
//!    you clicked were not the pegs you saw.
//! 4. **`&&` binds tighter than `||`.** The disk-count guard read
//!    `num_disks < MAX_DISKS && state != Playing || (moves == 0 && ...)`, which
//!    parses as `(a && b) || c` — the bound on the disk count applied to only
//!    one of the two branches. `set_disks` range-checks, so this silently did
//!    nothing rather than corrupting anything, but a guard that does not say
//!    what it means is a guard nobody can maintain.
//! 5. **Picking a disk up did not take it off its peg.** `try_pickup` recorded
//!    `(peg, disk)` and left the disk on the stack, so a held disk was drawn
//!    twice — floating above the peg you were aiming at *and* still stacked on
//!    the peg it had left — and `can_place` was still being asked about a top
//!    disk that was supposed to be in your hand.
//! 6. **The only measure of your progress was a constant.** The header showed
//!    `moves / 2^n - 1`, the length of a perfect solve from the *opening*
//!    position. One wrong move in and that figure is not what is left to do,
//!    it is a number about a game you are no longer playing. The status line
//!    now reports the shortest completion **from where you actually are**,
//!    which is also what drives the new hint and auto-solve.
//!
//! That last one is the interesting algorithm here, so it is tested the way
//! tic-tac-toe's search is: `remaining` and `next_optimal_move` are checked
//! against a breadth-first search over the whole state space — every one of the
//! `3^n` positions, for every disk count the game offers — computed
//! independently of the code under test.

use guitk::color::Color;
use guitk::event::{Event, EventResult, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::frame::Rect;
use guitk::probe::Probe;
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::style::CornerRadii;
use guitk::text;
use oswindow::app::{self, App, Response};
use std::process::ExitCode;
use std::time::Duration;

// ── Catppuccin Mocha, only the entries this program paints with ──
const COL_BASE: Color = Color::from_hex(0x1E1E2E);
const COL_CRUST: Color = Color::from_hex(0x11111B);
const COL_SURFACE0: Color = Color::from_hex(0x313244);
const COL_SURFACE1: Color = Color::from_hex(0x45475A);
const COL_TEXT: Color = Color::from_hex(0xCDD6F4);
const COL_SUBTEXT: Color = Color::from_hex(0xA6ADC8);
const COL_OVERLAY: Color = Color::from_hex(0x6C7086);
const COL_BLUE: Color = Color::from_hex(0x89B4FA);
const COL_GREEN: Color = Color::from_hex(0xA6E3A1);
const COL_YELLOW: Color = Color::from_hex(0xF9E2AF);
const COL_LAVENDER: Color = Color::from_hex(0xB4BEFE);

/// Drawn over the window behind the help sheet, and over a solved puzzle.
const COL_SCRIM: Color = Color::rgba(0x1E, 0x1E, 0x2E, 158);
const COL_VEIL: Color = Color::rgba(0x11, 0x11, 0x1B, 214);

/// Disk colours, smallest first. A disk keeps its colour whatever peg it is on,
/// which is the only cue that tells you two stacks apart at a glance.
const DISK_COLORS: [Color; 8] = [
    Color::from_hex(0xF38BA8),
    Color::from_hex(0xFAB387),
    Color::from_hex(0xF9E2AF),
    Color::from_hex(0xA6E3A1),
    Color::from_hex(0x94E2D5),
    Color::from_hex(0x89B4FA),
    Color::from_hex(0xCBA6F7),
    Color::from_hex(0xB4BEFE),
];

pub const PEGS: usize = 3;
pub const MIN_DISKS: usize = 3;
pub const MAX_DISKS: usize = 8;
/// How many disk counts the best-score table has a row for: 3 through 8.
pub const DISK_CHOICES: usize = MAX_DISKS - MIN_DISKS + 1;
/// Every solve ends with the whole stack on the right-hand peg.
pub const GOAL_PEG: usize = 2;

const WINDOW_WIDTH: f32 = 760.0;
const WINDOW_HEIGHT: f32 = 620.0;

/// How long the auto-solver waits between moves.
///
/// Slow enough to follow with your eyes — the point of watching a solve is to
/// see the recursion, and a solve that finishes in one frame teaches nothing.
const SOLVE_STEP_MS: u64 = 320;

/// The tick the solver is paced with, asked for only while it is running.
const TICK_MS: u64 = 16;

const HELP_TITLE: &str = "How to play";
const HELP_ROWS: [(&str, &str); 9] = [
    ("Goal", "Move every disk to peg 3, largest at the bottom"),
    (
        "Rule",
        "One disk at a time, never a bigger one onto a smaller",
    ),
    (
        "Left/Right",
        "Choose a peg; 1, 2 and 3 jump straight to one",
    ),
    (
        "Enter",
        "Lift the top disk, or drop the one you are holding",
    ),
    ("Click", "The same, on the peg you click"),
    ("Esc", "Put the disk you are holding back"),
    ("Z", "Take back a move"),
    ("A / S", "One best move / solve it and watch"),
    ("Up/Down", "More or fewer disks, before you start"),
];

// ── Layout ─────────────────────────────────────────────────────────────────

/// The share of the window's height the pegs keep no matter what.
const BOARD_SHARE: f32 = 0.5;

/// Which band goes first when they do not all fit: scores, header, controls.
///
/// Bands are dropped whole rather than shrunk together, because a band scaled
/// to four pixels costs the pegs four pixels and shows nothing. The scores go
/// first — they are a record of games already finished. The status line goes
/// last: how many moves you have made and how many are left is the only chrome
/// you cannot play without.
const BAND_DROP_ORDER: [usize; 4] = [3, 0, 2, 1];

/// Every rectangle in the window, derived from the window's own size.
///
/// Built fresh on every frame and never remembered. A layout stored on the
/// model is a layout that can disagree with the window it is drawn in — which
/// is exactly how a click on the title bar came to pick up a disk.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub window: Rect,
    pub header: Rect,
    pub info: Rect,
    /// The three peg columns, side by side.
    pub board: Rect,
    /// Disk count, undo and solve.
    pub controls: Rect,
    /// One cell per disk count, showing the best solve recorded for it.
    pub scores: Rect,
    pub help: Rect,
    pub font: f32,
    pub small: f32,
    pub pad: f32,
}

impl Layout {
    /// The layout for a window of the given size.
    #[must_use]
    pub fn new(width: f32, height: f32) -> Self {
        let w = width.max(1.0);
        let h = height.max(1.0);
        let font = (h / 38.0).clamp(8.0, 17.0);
        let small = (font - 2.0).max(7.0);
        let pad = (w.min(h) * 0.02).clamp(2.0, 10.0);

        // What each band would like, in [header, info, controls, scores] order.
        let mut wants = [
            (h * 0.09).clamp(24.0, 46.0),
            (h * 0.055).clamp(16.0, 28.0),
            (h * 0.08).clamp(22.0, 40.0),
            (h * 0.07).clamp(18.0, 34.0),
        ];
        // What is left for chrome once the board has its share *and* the gap
        // that separates the board from the chrome above and below it. The
        // padding has to come out of this side: charging it to the board turns
        // a promised half-window into 46% of a 160px window, which is where a
        // three-disk stack stops fitting between the pegs' feet and the tops.
        let budget = (h - h * BOARD_SHARE - pad * 2.0).max(0.0);
        for &i in &BAND_DROP_ORDER {
            if wants.iter().sum::<f32>() <= budget {
                break;
            }
            if let Some(band) = wants.get_mut(i) {
                *band = 0.0;
            }
        }
        let [hdr_h, inf_h, ctl_h, sco_h] = wants;

        let header = Rect::new(0.0, 0.0, w, hdr_h);
        let info = Rect::new(0.0, header.bottom(), w, inf_h);
        // The two lower bands are stacked up from the bottom edge, so dropping
        // either one gives its height straight back to the pegs.
        let scores = if sco_h > 0.0 {
            Rect::new(0.0, h - sco_h, w, sco_h)
        } else {
            Rect::EMPTY
        };
        let lower = if sco_h > 0.0 { scores.y } else { h };
        let controls = if ctl_h > 0.0 {
            Rect::new(0.0, lower - ctl_h, w, ctl_h)
        } else {
            Rect::EMPTY
        };

        let top = info.bottom();
        let bottom = if ctl_h > 0.0 { controls.y } else { lower };
        let board = Rect::new(
            pad,
            top + pad,
            (w - pad * 2.0).max(0.0),
            (bottom - top - pad * 2.0).max(0.0),
        );

        let help_w = (w * 0.92).min(470.0);
        let help_h = (h * 0.92).min(330.0);
        let help = Rect::new((w - help_w) / 2.0, (h - help_h) / 2.0, help_w, help_h);

        Self {
            window: Rect::new(0.0, 0.0, w, h),
            header,
            info,
            board,
            controls,
            scores,
            help,
            font,
            small,
            pad,
        }
    }

    /// The `index`th of `count` evenly-spaced buttons filling `row`.
    fn nth_of(row: Rect, count: usize, index: usize) -> Rect {
        let n = count.max(1) as f32;
        let gap = (row.w * 0.01).min(6.0);
        let bw = ((row.w - gap * (n + 1.0)) / n).max(0.0);
        Rect::new(
            row.x + gap + index as f32 * (bw + gap),
            row.y,
            bw,
            row.h.max(0.0),
        )
    }

    /// One peg column, `index` counting left to right. The whole column is the
    /// hit box: clicking anywhere above a peg aims at that peg.
    #[must_use]
    pub fn peg(&self, index: usize) -> Rect {
        let col = self.board.w / PEGS as f32;
        Rect::new(
            self.board.x + index.min(PEGS.saturating_sub(1)) as f32 * col,
            self.board.y,
            col,
            self.board.h.max(0.0),
        )
    }

    /// The line the disks stack up from, inside a peg column.
    #[must_use]
    pub fn base_y(&self) -> f32 {
        self.board.bottom() - (self.board.h * 0.08).clamp(4.0, 16.0)
    }

    /// How tall one disk is, for a puzzle of `disks` disks.
    ///
    /// Sized so the full stack fits the column with room above it for the disk
    /// you are carrying — the old code fixed this at 20px, so eight disks ran
    /// 160px up a column that might be 90px tall.
    #[must_use]
    pub fn disk_h(&self, disks: usize) -> f32 {
        let room = (self.base_y() - self.board.y) * 0.78;
        (room / disks.max(1) as f32).clamp(2.0, 34.0)
    }

    /// The two header buttons — new game, help — left to right.
    #[must_use]
    pub fn header_button(&self, index: usize) -> Rect {
        let group_w = (self.header.w * 0.42).min(210.0);
        let row = Rect::new(
            (self.header.right() - self.pad - group_w).max(self.header.x),
            self.header.y + self.header.h * 0.15,
            group_w,
            (self.header.h * 0.7).max(0.0),
        );
        Self::nth_of(row, 2, index)
    }

    /// One of the four control buttons: fewer, more, undo, solve.
    #[must_use]
    pub fn control(&self, index: usize) -> Rect {
        let row = Rect::new(
            self.controls.x + self.pad,
            self.controls.y + self.controls.h * 0.12,
            (self.controls.w - self.pad * 2.0).max(0.0),
            (self.controls.h * 0.76).max(0.0),
        );
        Self::nth_of(row, 5, index)
    }

    /// One cell of the best-score strip, one per disk count.
    #[must_use]
    pub fn score_cell(&self, index: usize) -> Rect {
        Self::nth_of(self.scores, DISK_CHOICES, index)
    }

    #[must_use]
    pub fn shows_header(&self) -> bool {
        self.header.h >= 14.0 && self.header.w >= 90.0
    }
    #[must_use]
    pub fn shows_info(&self) -> bool {
        self.info.h >= 10.0 && self.info.w >= 90.0
    }
    #[must_use]
    pub fn shows_controls(&self) -> bool {
        self.controls.h >= 14.0 && self.controls.w >= 220.0
    }
    #[must_use]
    pub fn shows_scores(&self) -> bool {
        self.scores.h >= 10.0 && self.scores.w >= 260.0
    }
}

// ── Model ──────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameState {
    Playing,
    Solved,
}

/// What the window can ask the puzzle to do.
///
/// Every route in — a key, a click, the solver's clock — goes through `apply`,
/// so there is one place that decides whether a move is legal and one place
/// that counts it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Aim at a peg without touching its disks.
    Select(usize),
    /// Lift the top disk off a peg, or drop the one you are holding onto it —
    /// whichever applies. This is what both a click and `Enter` do.
    Touch(usize),
    /// Put the disk you are holding back where it came from.
    Cancel,
    Undo,
    NewGame,
    /// Start over with a different number of disks.
    SetDisks(usize),
    /// Play one move of the shortest completion from here.
    Step,
    /// Start or stop playing that completion out on a clock.
    ToggleSolve,
    ToggleHelp,
}

/// Everything in the window a click can land on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// A whole peg column, from the top of the board to the base.
    Peg(usize),
    Fewer,
    More,
    Undo,
    Solve,
    NewGame,
    Help,
    /// The help sheet itself, over the whole window: while it is up, nothing
    /// behind it is clickable.
    HelpSheet,
}

/// The frame type this program records its hit boxes into.
pub type Frame = guitk::frame::Frame<Target>;

// ── Solver ─────────────────────────────────────────────────────────────────

/// Where every disk is: `spots[d]` holds the peg carrying the disk of size
/// `d + 1`, smallest first. Entries past the puzzle's disk count are ignored.
///
/// The pegs themselves are stacks, which is the right shape for drawing and
/// the wrong shape for searching: the only thing the solution depends on is
/// which peg each disk sits on, because their order within a peg is forced.
pub type Spots = [u8; MAX_DISKS];

/// `2^k`, saturating rather than panicking on a shift nobody can reach.
fn pow2(k: u32) -> u32 {
    1_u32.checked_shl(k).unwrap_or(u32::MAX)
}

/// The peg that is neither `a` nor `b`. Meaningless unless they differ.
fn spare_peg(a: u8, b: u8) -> u8 {
    3_u8.saturating_sub(a).saturating_sub(b)
}

/// Fewest moves that would gather disks `1..=disks` onto peg `dest`.
///
/// The recursion is the puzzle's own: to put the largest disk where it belongs
/// you must first clear every smaller disk onto the one peg that is in neither
/// place, which costs the same problem one size down, and then rebuild that
/// tower on top — `2^(n-1) - 1` moves, since by then it is a full tower on a
/// single peg. If the largest disk is already home it costs nothing and the
/// question shrinks by one.
///
/// Written as a loop rather than a recursion because each step only ever makes
/// one recursive call, so the stack frame would carry nothing.
#[must_use]
pub fn cost_to(spots: &Spots, disks: usize, dest: u8) -> u32 {
    let mut total: u32 = 0;
    let mut n = disks.min(MAX_DISKS);
    let mut dest = dest;
    while n > 0 {
        let Some(&here) = spots.get(n.saturating_sub(1)) else {
            break;
        };
        if here == dest {
            n = n.saturating_sub(1);
            continue;
        }
        // One move for this disk, plus `2^(n-1) - 1` to rebuild the tower that
        // had to get out of its way: `2^(n-1)` in total.
        total = total.saturating_add(pow2(n.saturating_sub(1) as u32));
        dest = spare_peg(here, dest);
        n = n.saturating_sub(1);
    }
    total
}

/// The first move of a shortest completion, as `(from, to)`.
///
/// `None` once everything is already on `dest`. Follows the same recursion as
/// `cost_to` and stops at the first disk that can actually move: the largest
/// disk that is out of place, once everything smaller is on the peg that has
/// to hold it while that move happens.
#[must_use]
pub fn next_move_to(spots: &Spots, disks: usize, dest: u8) -> Option<(usize, usize)> {
    let mut n = disks.min(MAX_DISKS);
    let mut dest = dest;
    while n > 0 {
        let &here = spots.get(n.saturating_sub(1))?;
        if here == dest {
            n = n.saturating_sub(1);
            continue;
        }
        let aux = spare_peg(here, dest);
        let smaller = n.saturating_sub(1);
        if spots.iter().take(smaller).all(|&p| p == aux) {
            return Some((here as usize, dest as usize));
        }
        dest = aux;
        n = smaller;
    }
    None
}

// ── Puzzle ─────────────────────────────────────────────────────────────────

const NO_DISKS: &[u8] = &[];

/// The whole puzzle: three stacks, the disk in your hand, the running record,
/// and the size of the window it was last drawn in.
#[derive(Clone)]
pub struct Towers {
    /// Disk sizes on each peg, bottom to top; 1 is the smallest.
    pegs: [Vec<u8>; PEGS],
    disks: usize,
    moves: u32,
    /// Which peg the keyboard is aiming at.
    cursor: usize,
    /// The disk you are carrying and the peg it came from.
    ///
    /// It is genuinely off that peg while you hold it — the old code left it on
    /// the stack, so it was drawn twice and `can_place` was still answering
    /// questions about a disk that was supposed to be in your hand.
    held: Option<(usize, u8)>,
    state: GameState,
    /// Fewest moves recorded for each disk count, 3 through 8.
    best: [Option<u32>; DISK_CHOICES],
    /// Whether a hint or the solver has touched this attempt.
    assisted: bool,
    solving: bool,
    /// Milliseconds until the solver's next move; zero when it is not running.
    step_ms: u64,
    /// Every move made, so `Undo` can walk back up them.
    undo_stack: Vec<(usize, usize)>,
    show_help: bool,
    width: f32,
    height: f32,
}

impl Default for Towers {
    fn default() -> Self {
        Self::new()
    }
}

impl Towers {
    #[must_use]
    pub fn new() -> Self {
        let mut app = Self {
            pegs: [Vec::new(), Vec::new(), Vec::new()],
            disks: 4,
            moves: 0,
            cursor: 0,
            held: None,
            state: GameState::Playing,
            best: [None; DISK_CHOICES],
            assisted: false,
            solving: false,
            step_ms: 0,
            undo_stack: Vec::new(),
            show_help: false,
            width: WINDOW_WIDTH,
            height: WINDOW_HEIGHT,
        };
        app.reset();
        app
    }

    /// Rebuild the opening stack, keeping the disk count and the record.
    pub fn reset(&mut self) {
        self.pegs = [Vec::new(), Vec::new(), Vec::new()];
        if let Some(first) = self.pegs.get_mut(0) {
            for size in (1..=self.disks.min(MAX_DISKS) as u8).rev() {
                first.push(size);
            }
        }
        self.moves = 0;
        self.held = None;
        self.state = GameState::Playing;
        self.assisted = false;
        self.solving = false;
        self.step_ms = 0;
        self.undo_stack.clear();
        self.cursor = 0;
    }

    /// Start over with `n` disks. Out-of-range counts are refused rather than
    /// clamped: a `+` at the top of the range should do nothing, not silently
    /// restart the puzzle you were already on.
    pub fn set_disks(&mut self, n: usize) -> bool {
        if !(MIN_DISKS..=MAX_DISKS).contains(&n) || n == self.disks {
            return false;
        }
        self.disks = n;
        self.reset();
        true
    }

    // ── Reading the position ──

    #[must_use]
    pub fn peg(&self, index: usize) -> &[u8] {
        self.pegs.get(index).map_or(NO_DISKS, Vec::as_slice)
    }
    #[must_use]
    pub fn top(&self, index: usize) -> Option<u8> {
        self.peg(index).last().copied()
    }
    #[must_use]
    pub fn disks(&self) -> usize {
        self.disks
    }
    #[must_use]
    pub fn moves(&self) -> u32 {
        self.moves
    }
    #[must_use]
    pub fn cursor(&self) -> usize {
        self.cursor
    }
    #[must_use]
    pub fn held(&self) -> Option<(usize, u8)> {
        self.held
    }
    #[must_use]
    pub fn state(&self) -> GameState {
        self.state
    }
    #[must_use]
    pub fn best(&self) -> [Option<u32>; DISK_CHOICES] {
        self.best
    }
    #[must_use]
    pub fn assisted(&self) -> bool {
        self.assisted
    }
    #[must_use]
    pub fn solving(&self) -> bool {
        self.solving
    }
    #[must_use]
    pub fn show_help(&self) -> bool {
        self.show_help
    }
    #[must_use]
    pub fn playing(&self) -> bool {
        self.state == GameState::Playing
    }
    #[must_use]
    pub fn undo_depth(&self) -> usize {
        self.undo_stack.len()
    }

    /// The row of the best-score table this disk count belongs in.
    #[must_use]
    pub fn best_row(&self) -> usize {
        self.disks
            .saturating_sub(MIN_DISKS)
            .min(DISK_CHOICES.saturating_sub(1))
    }

    /// The shortest solve there is, from the opening position.
    #[must_use]
    pub fn min_moves(&self) -> u32 {
        pow2(self.disks.min(MAX_DISKS) as u32).saturating_sub(1)
    }

    /// Where every disk is, for the solver.
    ///
    /// A disk in your hand counts as being on the peg it came from — that is
    /// the position you would be in if you put it back, and it is the only
    /// answer that makes "moves left" a number about a legal position.
    #[must_use]
    pub fn spots(&self) -> Spots {
        let mut spots: Spots = [0; MAX_DISKS];
        for (p, stack) in self.pegs.iter().enumerate() {
            for &size in stack {
                if let Some(slot) = spots.get_mut(usize::from(size).saturating_sub(1)) {
                    *slot = p as u8;
                }
            }
        }
        if let Some((from, size)) = self.held
            && let Some(slot) = spots.get_mut(usize::from(size).saturating_sub(1))
        {
            *slot = from as u8;
        }
        spots
    }

    /// Fewest moves that would finish the puzzle from here.
    ///
    /// This is the figure the old header did not have. It showed
    /// `moves / 2^n - 1` — the length of a perfect solve from the *opening*
    /// position — which stops describing your game the moment you stray from
    /// the shortest line.
    #[must_use]
    pub fn remaining(&self) -> u32 {
        cost_to(&self.spots(), self.disks, GOAL_PEG as u8)
    }

    /// The first move of a shortest completion from here.
    #[must_use]
    pub fn next_optimal(&self) -> Option<(usize, usize)> {
        next_move_to(&self.spots(), self.disks, GOAL_PEG as u8)
    }

    /// Whether `disk` may go on `peg`: only onto an empty peg, or onto a disk
    /// strictly larger than itself.
    #[must_use]
    pub fn can_place(&self, peg: usize, disk: u8) -> bool {
        if peg >= PEGS {
            return false;
        }
        self.top(peg).is_none_or(|t| disk < t)
    }

    /// The sentence the status line shows.
    #[must_use]
    pub fn status(&self) -> String {
        if self.state == GameState::Solved {
            let n = self.moves;
            if self.assisted {
                return format!("Solved in {} moves with help — not recorded", n);
            }
            if n == self.min_moves() {
                return format!("Solved in {} moves — the shortest there is", n);
            }
            return format!(
                "Solved in {} moves — {} is the shortest",
                n,
                self.min_moves()
            );
        }
        if let Some((_, size)) = self.held {
            return format!("Holding disk {} — choose a peg", size);
        }
        let left = self.remaining();
        if self.solving {
            return format!("Solving — {} moves to go", left);
        }
        format!("{} moves so far — {} to go at best", self.moves, left)
    }
}

// ── Playing ────────────────────────────────────────────────────────────────

impl Towers {
    /// The peg one step left or right of the cursor, stopping at the end
    /// rather than wrapping.
    ///
    /// One step, once, per press. The old handler acted on the release too, so
    /// an arrow moved two pegs — and with only three pegs and a clamp at each
    /// end, that put the middle peg out of reach from either side. Every Hanoi
    /// solution needs the middle peg.
    #[must_use]
    pub fn neighbour(&self, step: i32) -> usize {
        let last = PEGS.saturating_sub(1) as i32;
        (self.cursor.min(PEGS.saturating_sub(1)) as i32)
            .saturating_add(step)
            .clamp(0, last) as usize
    }

    /// Whether the disk count may be changed right now: before you have moved
    /// anything, or once the puzzle is finished.
    ///
    /// Written with the parentheses the original guard needed. `&&` binds
    /// tighter than `||`, so `n < MAX && state != Playing || (moves == 0 && …)`
    /// is `(n < MAX && state != Playing) || (moves == 0 && …)` — the bound on
    /// the disk count applied to only one of the two branches.
    #[must_use]
    pub fn may_change_disks(&self) -> bool {
        !self.playing() || (self.moves == 0 && self.held.is_none())
    }

    /// Lift the top disk off `peg` into your hand.
    ///
    /// The disk really leaves the peg: while it is held it is on no stack at
    /// all, so nothing can be dropped on top of where it used to be and the
    /// win check cannot count it twice.
    pub fn grab(&mut self, peg: usize) -> bool {
        if !self.playing() || self.held.is_some() {
            return false;
        }
        let Some(stack) = self.pegs.get_mut(peg) else {
            return false;
        };
        let Some(disk) = stack.pop() else {
            return false;
        };
        self.held = Some((peg, disk));
        true
    }

    /// Drop the disk you are holding onto `peg`.
    ///
    /// Dropping it back where it came from is a change of mind, not a move, so
    /// it is not counted and not recorded for undo. Dropping it somewhere it
    /// cannot legally go leaves it in your hand.
    pub fn drop_on(&mut self, peg: usize) -> bool {
        let Some((from, disk)) = self.held else {
            return false;
        };
        if peg == from {
            self.cancel();
            return true;
        }
        if !self.can_place(peg, disk) {
            return false;
        }
        self.held = None;
        self.land(from, peg, disk);
        true
    }

    /// Put the disk you are holding back where it came from.
    pub fn cancel(&mut self) -> bool {
        let Some((from, disk)) = self.held.take() else {
            return false;
        };
        if let Some(stack) = self.pegs.get_mut(from) {
            stack.push(disk);
        }
        true
    }

    /// Lift from, or drop onto, `peg` — whichever applies.
    pub fn touch(&mut self, peg: usize) -> bool {
        if peg >= PEGS || !self.playing() {
            return false;
        }
        // Aim first, so a refused peg still shows where you asked: reaching for
        // a disk that cannot go there moves the marker and does nothing else,
        // which reads as "not that one".
        let aimed = self.cursor != peg;
        if aimed {
            self.cursor = peg;
        }
        let acted = if self.held.is_some() {
            self.drop_on(peg)
        } else {
            self.grab(peg)
        };
        acted || aimed
    }

    /// The one place a move is counted, recorded for undo, and checked for a
    /// finished puzzle.
    fn land(&mut self, from: usize, to: usize, disk: u8) {
        if let Some(stack) = self.pegs.get_mut(to) {
            stack.push(disk);
        }
        self.moves = self.moves.saturating_add(1);
        self.undo_stack.push((from, to));
        self.check_solved();
    }

    /// Take back the last move.
    ///
    /// Refused while you are holding a disk — there is no sensible answer to
    /// "undo" with a disk in mid-air — and refused once the puzzle is done,
    /// which is a finished record rather than a position.
    pub fn undo(&mut self) -> bool {
        if !self.playing() || self.held.is_some() {
            return false;
        }
        let Some(&(from, to)) = self.undo_stack.last() else {
            return false;
        };
        let Some(disk) = self.pegs.get_mut(to).and_then(Vec::pop) else {
            return false;
        };
        if let Some(stack) = self.pegs.get_mut(from) {
            stack.push(disk);
        }
        self.undo_stack.pop();
        self.moves = self.moves.saturating_sub(1);
        self.cursor = from;
        true
    }

    fn check_solved(&mut self) {
        if self.held.is_some() || self.peg(GOAL_PEG).len() != self.disks {
            return;
        }
        self.state = GameState::Solved;
        self.solving = false;
        self.step_ms = 0;
        // A solve you were shown is not a solve you found. Recording it would
        // fill every row of the table with `2^n - 1` on the first afternoon and
        // leave the table with nothing left to say.
        if self.assisted {
            return;
        }
        let row = self.best_row();
        if let Some(slot) = self.best.get_mut(row) {
            *slot = Some(match *slot {
                Some(previous) => previous.min(self.moves),
                None => self.moves,
            });
        }
    }

    /// Play one move of the shortest completion from here.
    ///
    /// A disk in your hand goes back first: the solution is a fact about a
    /// position, and a position is not something you can be halfway through.
    pub fn step_once(&mut self) -> bool {
        if !self.playing() {
            return false;
        }
        let put_back = self.cancel();
        let Some((from, to)) = self.next_optimal() else {
            return put_back;
        };
        let Some(disk) = self.pegs.get_mut(from).and_then(Vec::pop) else {
            return put_back;
        };
        self.assisted = true;
        self.cursor = to;
        self.land(from, to, disk);
        true
    }

    /// Start or stop playing the solution out on a clock.
    pub fn toggle_solve(&mut self) -> bool {
        if self.solving {
            self.solving = false;
            self.step_ms = 0;
            return true;
        }
        if !self.playing() || self.next_optimal().is_none() {
            return false;
        }
        self.cancel();
        self.solving = true;
        // Zero, not `SOLVE_STEP_MS`: the first move lands on the next tick, so
        // pressing the button visibly does something straight away.
        self.step_ms = 0;
        true
    }

    /// Stop the solver, returning whether it had been running.
    ///
    /// Every hand-made action calls this: a solver still stepping while you
    /// move disks yourself would be two players sharing one board.
    fn interrupt(&mut self) -> bool {
        let was = self.solving;
        self.solving = false;
        self.step_ms = 0;
        was
    }

    /// Age the solver's clock by `elapsed_ms` and play its move when the wait
    /// is spent. Returns true if anything changed, which is what tells the
    /// window whether the frame is worth redrawing.
    ///
    /// Ageing by the reported interval rather than by counting ticks keeps the
    /// pace the same whatever rate the compositor settles on.
    pub fn advance(&mut self, elapsed_ms: u64) -> bool {
        if !self.solving {
            return false;
        }
        self.step_ms = self.step_ms.saturating_sub(elapsed_ms.max(1));
        if self.step_ms > 0 {
            return true;
        }
        if !self.step_once() {
            self.solving = false;
            return true;
        }
        if self.solving {
            self.step_ms = SOLVE_STEP_MS;
        }
        true
    }

    /// Whether `action` would do anything, so the window can grey out a control
    /// instead of offering one that silently refuses.
    #[must_use]
    pub fn enabled(&self, action: Action) -> bool {
        match action {
            Action::Select(p) => p < PEGS && p != self.cursor,
            Action::Touch(p) => {
                self.playing()
                    && p < PEGS
                    && match self.held {
                        Some((from, disk)) => from == p || self.can_place(p, disk),
                        None => self.top(p).is_some(),
                    }
            }
            Action::Cancel => self.held.is_some(),
            Action::Undo => self.playing() && self.held.is_none() && !self.undo_stack.is_empty(),
            Action::NewGame => {
                self.moves > 0 || self.held.is_some() || !self.playing() || self.solving
            }
            Action::SetDisks(n) => {
                (MIN_DISKS..=MAX_DISKS).contains(&n) && n != self.disks && self.may_change_disks()
            }
            Action::Step => self.playing() && self.next_optimal().is_some(),
            Action::ToggleSolve => {
                self.solving || (self.playing() && self.next_optimal().is_some())
            }
            Action::ToggleHelp => true,
        }
    }

    /// The one place a move is made. Returns whether the puzzle changed.
    pub fn apply(&mut self, action: Action) -> bool {
        match action {
            Action::Select(p) => {
                if p >= PEGS || p == self.cursor {
                    return false;
                }
                self.cursor = p;
                true
            }
            Action::Touch(p) => {
                let stopped = self.interrupt();
                self.touch(p) || stopped
            }
            Action::Cancel => {
                let stopped = self.interrupt();
                self.cancel() || stopped
            }
            Action::Undo => {
                let stopped = self.interrupt();
                self.undo() || stopped
            }
            Action::NewGame => {
                self.reset();
                true
            }
            Action::SetDisks(n) => {
                if !self.may_change_disks() {
                    return false;
                }
                self.set_disks(n)
            }
            Action::Step => {
                let stopped = self.interrupt();
                self.step_once() || stopped
            }
            Action::ToggleSolve => self.toggle_solve(),
            Action::ToggleHelp => {
                self.show_help = !self.show_help;
                true
            }
        }
    }
}

/// Test support: reaching a named position without playing into it.
///
/// The app itself only ever arrives at a position by making legal moves, so
/// these live behind `cfg(test)` rather than widening the public surface with a
/// way to set up a position that nothing in the program needs.
#[cfg(test)]
impl Towers {
    /// Set the disk count regardless of whether the puzzle would allow it.
    fn force_disks(&mut self, n: usize) {
        self.disks = n.clamp(MIN_DISKS, MAX_DISKS);
        self.reset();
    }

    /// Put every disk where `spots` says, largest at the bottom of each peg.
    fn set_spots(&mut self, spots: &Spots) {
        self.pegs = [Vec::new(), Vec::new(), Vec::new()];
        self.held = None;
        for size in (1..=self.disks as u8).rev() {
            let Some(&peg) = spots.get(usize::from(size).saturating_sub(1)) else {
                continue;
            };
            if let Some(stack) = self.pegs.get_mut(usize::from(peg)) {
                stack.push(size);
            }
        }
        self.state = GameState::Playing;
        self.undo_stack.clear();
    }

    /// Mark this attempt unassisted again, for a test that scripts a solve.
    fn clear_assist(&mut self) {
        self.assisted = false;
    }
}

// ── Window ─────────────────────────────────────────────────────────────────

impl Towers {
    /// Record the size the window is now, which is the size the next click will
    /// be read against.
    pub fn resize(&mut self, width: f32, height: f32) {
        self.width = width.max(1.0);
        self.height = height.max(1.0);
    }

    #[must_use]
    pub fn width(&self) -> f32 {
        self.width
    }
    #[must_use]
    pub fn height(&self) -> f32 {
        self.height
    }

    /// What a click at (`x`, `y`) would land on, read from the frame the window
    /// is actually showing.
    ///
    /// This replaces a hit test that compared `x` against two constants and
    /// never looked at `y` at all — which is how clicking the title picked up a
    /// disk. There is no second copy of the geometry to get wrong, because the
    /// boxes were recorded by the pass that drew them.
    #[must_use]
    pub fn target_at(&self, x: f32, y: f32) -> Option<Target> {
        self.frame(self.width, self.height).hit_test(x, y)
    }
}

// ── Drawing ────────────────────────────────────────────────────────────────

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
        corner_radii: if radius > 0.0 {
            CornerRadii::all(radius)
        } else {
            CornerRadii::ZERO
        },
    });
}

fn label(
    f: &mut Frame,
    x: f32,
    y: f32,
    body: &str,
    size: f32,
    color: Color,
    weight: FontWeightHint,
    max_width: Option<f32>,
) {
    if body.is_empty() || size <= 0.0 {
        return;
    }
    f.push(RenderCommand::Text {
        x,
        y,
        text: body.to_string(),
        color,
        font_size: size,
        font_weight: weight,
        max_width,
        overflow: TextOverflow::Ellipsis,
    });
}

/// A label centred in a horizontal span, clamped so a string wider than the
/// span starts at the span's left edge instead of overhanging to its left.
fn centred_in(
    f: &mut Frame,
    left: f32,
    span: f32,
    cy: f32,
    body: &str,
    size: f32,
    color: Color,
    weight: FontWeightHint,
) {
    label(
        f,
        text::center_x(body, left + span / 2.0, size, weight).max(left),
        cy - text::line_height(size, weight) / 2.0,
        body,
        size,
        color,
        weight,
        Some(span.max(0.0)),
    );
}

/// A ring drawn as four bars rather than a stroked rectangle, because a stroke
/// is centred on the edge and would bleed half its width into the neighbouring
/// column — and so into its hit box.
fn ring(f: &mut Frame, r: Rect, thickness: f32, color: Color) {
    if r.w <= 0.0 || r.h <= 0.0 || thickness <= 0.0 {
        return;
    }
    let t = thickness.min(r.w / 2.0).min(r.h / 2.0);
    fill(f, Rect::new(r.x, r.y, r.w, t), color, 0.0);
    fill(f, Rect::new(r.x, r.bottom() - t, r.w, t), color, 0.0);
    fill(
        f,
        Rect::new(r.x, r.y + t, t, (r.h - t * 2.0).max(0.0)),
        color,
        0.0,
    );
    fill(
        f,
        Rect::new(r.right() - t, r.y + t, t, (r.h - t * 2.0).max(0.0)),
        color,
        0.0,
    );
}

/// One button: a filled pill with a centred caption, and its hit box.
fn button(f: &mut Frame, r: Rect, body: &str, size: f32, live: bool, target: Target) {
    if r.w <= 0.0 || r.h <= 0.0 {
        return;
    }
    fill(
        f,
        r,
        if live { COL_SURFACE0 } else { COL_CRUST },
        (r.h * 0.28).min(8.0),
    );
    centred_in(
        f,
        r.x,
        r.w,
        r.y + r.h / 2.0,
        body,
        size,
        if live { COL_TEXT } else { COL_OVERLAY },
        FontWeightHint::Bold,
    );
    f.hit(target, r);
}

/// The colour of the disk of size `size`, which never changes with the peg it
/// is on: it is the only cue that tells two stacks apart at a glance.
#[must_use]
pub fn disk_color(size: u8) -> Color {
    let index = usize::from(size)
        .saturating_sub(1)
        .checked_rem(DISK_COLORS.len())
        .unwrap_or(0);
    DISK_COLORS.get(index).copied().unwrap_or(COL_TEXT)
}

impl Towers {
    /// The layout of this window. A pure function of its size — nothing about
    /// the position changes where anything goes.
    #[must_use]
    pub fn layout(&self, width: f32, height: f32) -> Layout {
        Layout::new(width, height)
    }

    /// How wide the disk of size `size` is drawn in a column `col_w` across.
    fn disk_w(&self, col_w: f32, size: u8) -> f32 {
        let widest = col_w * 0.84;
        let narrowest = (col_w * 0.26).min(widest);
        let span = self.disks.max(2).saturating_sub(1) as f32;
        let frac = (f32::from(size.saturating_sub(1)) / span).clamp(0.0, 1.0);
        narrowest + frac * (widest - narrowest)
    }

    /// The whole window, and every hit box in it, in one pass.
    ///
    /// `Frame::hit_test` scans the recorded boxes in reverse, so anything drawn
    /// later wins the click over what it covers. That is why the help sheet is
    /// painted last.
    #[must_use]
    pub fn frame(&self, width: f32, height: f32) -> Frame {
        let l = self.layout(width, height);
        let mut f = Frame::new(l.window.w, l.window.h);
        fill(&mut f, l.window, COL_BASE, 0.0);

        if l.shows_header() {
            self.draw_header(&mut f, &l);
        }
        if l.shows_info() {
            self.draw_info(&mut f, &l);
        }
        self.draw_board(&mut f, &l);
        if !self.playing() {
            self.draw_banner(&mut f, &l);
        }
        if l.shows_controls() {
            self.draw_controls(&mut f, &l);
        }
        if l.shows_scores() {
            self.draw_scores(&mut f, &l);
        }
        if self.show_help {
            draw_help(&mut f, &l);
        }
        f
    }

    fn draw_header(&self, f: &mut Frame, l: &Layout) {
        let cy = l.header.y + l.header.h / 2.0;
        let first = l.header_button(0);
        let title_span = (first.x - l.pad * 2.0 - l.header.x).max(0.0);
        label(
            f,
            l.header.x + l.pad,
            cy - text::line_height(l.font, FontWeightHint::Bold) / 2.0,
            "Tower of Hanoi",
            l.font,
            COL_LAVENDER,
            FontWeightHint::Bold,
            Some(title_span),
        );

        let captions = ["New game", "Help"];
        let targets = [Target::NewGame, Target::Help];
        let live = [self.enabled(Action::NewGame), true];
        for i in 0..2 {
            button(
                f,
                l.header_button(i),
                captions.get(i).copied().unwrap_or(""),
                l.small,
                live.get(i).copied().unwrap_or(true),
                targets.get(i).copied().unwrap_or(Target::Help),
            );
        }
    }

    fn draw_info(&self, f: &mut Frame, l: &Layout) {
        let colour = if self.state == GameState::Solved {
            if self.assisted { COL_YELLOW } else { COL_GREEN }
        } else if self.held.is_some() || self.solving {
            COL_YELLOW
        } else {
            COL_SUBTEXT
        };
        centred_in(
            f,
            l.info.x + l.pad,
            (l.info.w - l.pad * 2.0).max(0.0),
            l.info.y + l.info.h / 2.0,
            &self.status(),
            l.font,
            colour,
            FontWeightHint::Bold,
        );
    }

    fn draw_board(&self, f: &mut Frame, l: &Layout) {
        if l.board.w <= 0.0 || l.board.h <= 0.0 {
            return;
        }
        fill(f, l.board, COL_CRUST, (l.board.h * 0.04).min(12.0));

        let base_y = l.base_y();
        let disk_h = l.disk_h(self.disks);
        let plinth = (l.board.h * 0.025).clamp(2.0, 9.0);
        let label_h = (l.board.h * 0.12).min(l.small * 1.6);

        for p in 0..PEGS {
            let col = l.peg(p);
            let aimed = p == self.cursor;
            if aimed {
                let inset = Rect::new(
                    col.x + 2.0,
                    col.y + 2.0,
                    (col.w - 4.0).max(0.0),
                    (col.h - 4.0).max(0.0),
                );
                fill(f, inset, COL_BASE, (col.w * 0.04).min(10.0));
                ring(f, inset, (col.w * 0.012).max(1.5), COL_LAVENDER);
            }

            centred_in(
                f,
                col.x,
                col.w,
                col.y + label_h / 2.0,
                &format!("{}", p.saturating_add(1)),
                l.small,
                if aimed { COL_YELLOW } else { COL_OVERLAY },
                FontWeightHint::Bold,
            );

            // Rod and plinth, sized from the stack that has to fit on them
            // rather than from the 200px-wide peg the old code assumed.
            let cx = col.x + col.w / 2.0;
            let rod_w = (col.w * 0.03).clamp(2.0, 9.0);
            let rod_top = (base_y - disk_h * self.disks as f32 - disk_h * 0.5).max(col.y + label_h);
            fill(
                f,
                Rect::new(
                    cx - rod_w / 2.0,
                    rod_top,
                    rod_w,
                    (base_y - rod_top).max(0.0),
                ),
                COL_SURFACE1,
                rod_w / 2.0,
            );
            let plinth_w = col.w * 0.9;
            fill(
                f,
                Rect::new(cx - plinth_w / 2.0, base_y, plinth_w, plinth),
                COL_SURFACE1,
                plinth / 2.0,
            );

            let gap = (disk_h * 0.12).min(3.0);
            for (i, &size) in self.peg(p).iter().enumerate() {
                let dw = self.disk_w(col.w, size);
                let dy = base_y - (i as f32 + 1.0) * disk_h;
                let r = Rect::new(cx - dw / 2.0, dy, dw, (disk_h - gap).max(1.0));
                fill(f, r, disk_color(size), (r.h * 0.35).min(6.0));
                // The number only when the disk is tall enough to hold it;
                // a 4px disk with a 12px numeral on it is not a label.
                if r.h >= l.small + 2.0 {
                    centred_in(
                        f,
                        r.x,
                        r.w,
                        r.y + r.h / 2.0,
                        &format!("{}", size),
                        l.small,
                        COL_CRUST,
                        FontWeightHint::Bold,
                    );
                }
            }

            // The whole column, so a click anywhere above a peg aims at it —
            // and, just as importantly, a click on the header or the status
            // line does not, because those are not in here.
            f.hit(Target::Peg(p), col);
        }

        // The disk in your hand, over the peg you are aiming at. It is not on
        // any stack, so this is the only place it is drawn.
        if let Some((_, size)) = self.held {
            let col = l.peg(self.cursor);
            let dw = self.disk_w(col.w, size);
            let r = Rect::new(
                col.x + (col.w - dw) / 2.0,
                col.y + label_h,
                dw,
                (disk_h - (disk_h * 0.12).min(3.0)).max(1.0),
            );
            fill(f, r, disk_color(size), (r.h * 0.35).min(6.0));
            ring(f, r, (r.h * 0.12).max(1.0), COL_TEXT);
        }
    }

    /// The result, over the finished puzzle.
    fn draw_banner(&self, f: &mut Frame, l: &Layout) {
        let h = (l.board.h * 0.24).clamp(0.0, 56.0);
        let w = (l.board.w * 0.8).min(420.0);
        if h <= 0.0 || w <= 0.0 {
            return;
        }
        let r = Rect::new(
            l.board.x + (l.board.w - w) / 2.0,
            l.board.y + (l.board.h - h) / 2.0,
            w,
            h,
        );
        fill(f, r, COL_VEIL, (h * 0.3).min(12.0));
        centred_in(
            f,
            r.x,
            r.w,
            r.y + r.h * 0.36,
            &self.status(),
            l.small,
            if self.assisted { COL_YELLOW } else { COL_GREEN },
            FontWeightHint::Bold,
        );
        centred_in(
            f,
            r.x,
            r.w,
            r.y + r.h * 0.74,
            "N for a new game, Up or Down to change the disk count",
            (l.small - 1.0).max(6.0),
            COL_SUBTEXT,
            FontWeightHint::Regular,
        );
    }

    fn draw_controls(&self, f: &mut Frame, l: &Layout) {
        let fewer = self.disks.saturating_sub(1);
        let more = self.disks.saturating_add(1);
        let solve = if self.solving { "Stop" } else { "Solve" };
        button(
            f,
            l.control(0),
            "Fewer",
            l.small,
            self.enabled(Action::SetDisks(fewer)),
            Target::Fewer,
        );

        // The readout between the two buttons is not a control, so it records
        // no hit box: clicking it must do nothing rather than doing whichever
        // of its neighbours happens to be drawn underneath.
        let mid = l.control(1);
        fill(f, mid, COL_CRUST, (mid.h * 0.28).min(8.0));
        centred_in(
            f,
            mid.x,
            mid.w,
            mid.y + mid.h / 2.0,
            &format!("{} disks", self.disks),
            l.small,
            COL_SUBTEXT,
            FontWeightHint::Regular,
        );

        button(
            f,
            l.control(2),
            "More",
            l.small,
            self.enabled(Action::SetDisks(more)),
            Target::More,
        );
        button(
            f,
            l.control(3),
            "Undo",
            l.small,
            self.enabled(Action::Undo),
            Target::Undo,
        );
        button(
            f,
            l.control(4),
            solve,
            l.small,
            self.enabled(Action::ToggleSolve),
            Target::Solve,
        );
    }

    fn draw_scores(&self, f: &mut Frame, l: &Layout) {
        for i in 0..DISK_CHOICES {
            let r = l.score_cell(i);
            if r.w <= 0.0 || r.h <= 0.0 {
                continue;
            }
            let disks = MIN_DISKS.saturating_add(i);
            let here = disks == self.disks;
            fill(f, r, COL_CRUST, (r.h * 0.22).min(7.0));
            let best = self.best.get(i).copied().flatten();
            let shortest = pow2(disks as u32).saturating_sub(1);
            let body = match best {
                Some(n) if n == shortest => format!("{}: {} best", disks, n),
                Some(n) => format!("{}: {}", disks, n),
                None => format!("{}: —", disks),
            };
            centred_in(
                f,
                r.x,
                r.w,
                r.y + r.h / 2.0,
                &body,
                (l.small - 1.0).max(6.0),
                if here {
                    COL_YELLOW
                } else if best.is_some() {
                    COL_SUBTEXT
                } else {
                    COL_OVERLAY
                },
                if here {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
            );
        }
    }
}

fn draw_help(f: &mut Frame, l: &Layout) {
    // Dim the whole window first, then the panel on top of it, so the sheet
    // reads as in front of the puzzle rather than part of it.
    fill(f, l.window, COL_SCRIM, 0.0);
    let p = l.help;
    fill(f, p, COL_VEIL, 10.0);

    let pad = (p.w * 0.05).clamp(6.0, 18.0);
    let inner = (p.w - pad * 2.0).max(0.0);
    let title_h = text::line_height(l.font, FontWeightHint::Bold);
    label(
        f,
        p.x + pad,
        p.y + pad,
        HELP_TITLE,
        l.font,
        COL_YELLOW,
        FontWeightHint::Bold,
        Some(inner),
    );

    // Rows share whatever is left below the title, so the sheet cannot write
    // past its own foot however short the window is.
    let top = p.y + pad + title_h + pad / 2.0;
    let room = (p.bottom() - pad - top).max(0.0);
    let step = room / HELP_ROWS.len() as f32;
    let key_span = (inner * 0.26).min(96.0);
    for (i, (k, v)) in HELP_ROWS.iter().enumerate() {
        let y = top + i as f32 * step;
        if y + l.small > p.bottom() - pad {
            break;
        }
        label(
            f,
            p.x + pad,
            y,
            k,
            l.small,
            COL_BLUE,
            FontWeightHint::Bold,
            Some(key_span),
        );
        label(
            f,
            p.x + pad + key_span,
            y,
            v,
            l.small,
            COL_TEXT,
            FontWeightHint::Regular,
            Some((inner - key_span).max(0.0)),
        );
    }

    // Over the whole window, not just the panel: while the sheet is up, nothing
    // behind it is clickable.
    f.hit(Target::HelpSheet, l.window);
}

// ── Input ──────────────────────────────────────────────────────────────────

impl Towers {
    fn handle_key(&mut self, ev: &KeyEvent) -> EventResult {
        // The fault that broke every key in this file, in one line. A release
        // is not a second press. Acting on both moved the cursor two pegs at a
        // time — putting the middle peg out of reach and the puzzle out of
        // reach with it — and made `Enter` lift a disk and put it straight back
        // down, so it did nothing at all.
        if !ev.pressed {
            return EventResult::Ignored;
        }
        let m = ev.modifiers;
        if m.ctrl || m.alt || m.super_key {
            return EventResult::Ignored;
        }

        if self.show_help {
            // The sheet is modal: it takes every key, and a few of them close
            // it. Letting the rest through would mean playing blind.
            if matches!(ev.key, Key::H | Key::Escape | Key::Enter | Key::Space) {
                self.apply(Action::ToggleHelp);
            }
            return EventResult::Consumed;
        }

        let action = match ev.key {
            Key::Left => Some(Action::Select(self.neighbour(-1))),
            Key::Right => Some(Action::Select(self.neighbour(1))),
            Key::Num1 => Some(Action::Select(0)),
            Key::Num2 => Some(Action::Select(1)),
            Key::Num3 => Some(Action::Select(2)),
            Key::Enter | Key::Space => Some(if self.playing() {
                Action::Touch(self.cursor)
            } else {
                Action::NewGame
            }),
            Key::Escape => Some(Action::Cancel),
            Key::Z => Some(Action::Undo),
            Key::N => Some(Action::NewGame),
            Key::Up => Some(Action::SetDisks(self.disks.saturating_add(1))),
            Key::Down => Some(Action::SetDisks(self.disks.saturating_sub(1))),
            Key::A => Some(Action::Step),
            Key::S => Some(Action::ToggleSolve),
            Key::H => Some(Action::ToggleHelp),
            _ => None,
        };

        match action {
            Some(a) => {
                // Consumed even when the puzzle refuses it: the key belongs to
                // this window either way, and a refused `Enter` must not reach
                // whatever is behind it.
                self.apply(a);
                EventResult::Consumed
            }
            None => EventResult::Ignored,
        }
    }

    fn handle_mouse(&mut self, ev: &MouseEvent) -> EventResult {
        if !matches!(ev.kind, MouseEventKind::Press(MouseButton::Left)) {
            return EventResult::Ignored;
        }
        if self.show_help {
            // Anywhere at all dismisses the sheet, including outside it.
            self.apply(Action::ToggleHelp);
            return EventResult::Consumed;
        }
        let Some(target) = self.target_at(ev.x, ev.y) else {
            return EventResult::Ignored;
        };
        match target {
            Target::Peg(p) => {
                if self.playing() {
                    self.apply(Action::Touch(p));
                } else {
                    // A finished puzzle is a record, not a position: clicking a
                    // peg on it starts the next one rather than doing nothing,
                    // which is what every player tries first.
                    self.apply(Action::NewGame);
                }
            }
            Target::Fewer => {
                self.apply(Action::SetDisks(self.disks.saturating_sub(1)));
            }
            Target::More => {
                self.apply(Action::SetDisks(self.disks.saturating_add(1)));
            }
            Target::Undo => {
                self.apply(Action::Undo);
            }
            Target::Solve => {
                self.apply(Action::ToggleSolve);
            }
            Target::NewGame => {
                self.apply(Action::NewGame);
            }
            Target::Help => {
                self.apply(Action::ToggleHelp);
            }
            Target::HelpSheet => {}
        }
        // Consumed either way: a click that lands on a control the puzzle is
        // refusing should stop there, not fall through to the pegs.
        EventResult::Consumed
    }
}

/// The one body both the window and the test probe drive, so what a click does
/// in a test is what it does on a screen.
pub fn handle_event(app: &mut Towers, event: &Event) -> EventResult {
    match event {
        Event::Key(ev) => app.handle_key(ev),
        Event::Mouse(ev) => app.handle_mouse(ev),
        Event::Resize { width, height } => {
            app.resize(*width as f32, *height as f32);
            EventResult::Consumed
        }
        // The clock the auto-solver runs on. Without it the only way to show a
        // solution would be to play the whole thing inside one event handler,
        // which draws exactly one frame: the finished puzzle.
        Event::Tick { elapsed_ms } => {
            if app.advance(*elapsed_ms) {
                EventResult::Consumed
            } else {
                EventResult::Ignored
            }
        }
        _ => EventResult::Ignored,
    }
}

impl App for Towers {
    fn title(&self) -> String {
        "Tower of Hanoi".to_string()
    }

    fn app_id(&self) -> String {
        "towers".to_string()
    }

    fn initial_size(&self) -> (u32, u32) {
        (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)
    }

    /// Ticks are asked for only while the solver is running.
    ///
    /// A puzzle waiting for you needs no frames, and one that asks for 60 a
    /// second regardless is one that keeps a laptop awake to draw the same
    /// pixels.
    fn tick_interval(&self) -> Option<Duration> {
        if self.solving {
            Some(Duration::from_millis(TICK_MS))
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
        // against — that is the whole point of storing it here.
        self.resize(width, height);
        self.frame(width, height).into_tree()
    }
}

impl Probe for Towers {
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
    let mut game = Towers::new();
    app::launch("towers", &mut game)
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::expect_used,
    clippy::arithmetic_side_effects
)]
mod tests {
    use super::*;
    use guitk::event::Modifiers;
    use guitk::probe;
    use std::collections::{HashMap, VecDeque};

    // ── Helpers ────────────────────────────────────────────────────────────

    /// The window sizes every layout claim is checked at: tiny, short, narrow,
    /// square, wide, and larger than any of them.
    const WINDOWS: &[(f32, f32)] = &[
        (120.0, 90.0),
        (200.0, 160.0),
        (320.0, 240.0),
        (400.0, 900.0),
        (760.0, 620.0),
        (900.0, 500.0),
        (1280.0, 720.0),
        (1920.0, 1080.0),
        (2560.0, 1440.0),
    ];

    fn game() -> Towers {
        Towers::new()
    }

    /// A puzzle whose window is a given size, as the compositor would set it.
    fn windowed(w: f32, h: f32) -> Towers {
        let mut g = game();
        g.resize(w, h);
        g
    }

    fn release(key: Key) -> KeyEvent {
        KeyEvent {
            key,
            modifiers: Modifiers::default(),
            pressed: false,
            text: String::new(),
        }
    }

    /// A press followed by its release — what a real keyboard sends.
    fn tap(g: &mut Towers, key: Key) {
        probe::key(g, &probe::press(key));
        probe::key(g, &release(key));
    }

    fn click(g: &mut Towers, target: Target) -> EventResult {
        probe::click(g, target)
    }

    /// Click a raw point, as the compositor reports it.
    fn poke(g: &mut Towers, x: f32, y: f32, size: (f32, f32)) -> EventResult {
        g.click_at(x, y, MouseButton::Left, size)
    }

    /// Every string the frame draws at a given size.
    fn texts(g: &Towers, w: f32, h: f32) -> Vec<String> {
        g.frame(w, h)
            .commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    fn says(g: &Towers, needle: &str) -> bool {
        texts(g, g.width(), g.height())
            .iter()
            .any(|t| t.contains(needle))
    }

    /// Every block of ink the frame lays down at a given size: filled and
    /// stroked rectangles as themselves, and a run of text as the point it
    /// starts from (its extent is the font's business, not the layout's).
    fn painted(g: &Towers, w: f32, h: f32) -> Vec<Rect> {
        g.frame(w, h)
            .commands()
            .iter()
            .filter_map(|c| match *c {
                RenderCommand::FillRect {
                    x,
                    y,
                    width,
                    height,
                    ..
                }
                | RenderCommand::StrokeRect {
                    x,
                    y,
                    width,
                    height,
                    ..
                } => Some(Rect::new(x, y, width, height)),
                RenderCommand::Text { x, y, .. } => Some(Rect::new(x, y, 0.0, 0.0)),
                _ => None,
            })
            .collect()
    }

    /// Which chrome bands the layout kept, in drop order.
    fn bands(l: &Layout) -> [bool; 4] {
        [
            l.shows_header(),
            l.shows_info(),
            l.shows_controls(),
            l.shows_scores(),
        ]
    }

    /// Run the solver's clock until it stops, or give up loudly.
    fn settle(g: &mut Towers) {
        for _ in 0..40_000 {
            if !g.solving() {
                return;
            }
            handle_event(g, &Event::Tick { elapsed_ms: 40 });
        }
        panic!("the solver never finished");
    }

    /// Drive `g` to the solved position along the shortest line, by hand,
    /// without touching the hint or the solver.
    fn solve_by_hand(g: &mut Towers) {
        for _ in 0..1024 {
            let Some((from, to)) = g.next_optimal() else {
                return;
            };
            assert!(g.apply(Action::Touch(from)), "could not lift from {from}");
            assert!(g.apply(Action::Touch(to)), "could not drop on {to}");
        }
        panic!("the shortest line never ended");
    }

    // ── Fault 1: a release is not a second press ──────────────────────────

    #[test]
    fn an_arrow_release_does_not_move_the_cursor_again() {
        let mut g = game();
        assert_eq!(g.cursor(), 0);
        tap(&mut g, Key::Right);
        assert_eq!(g.cursor(), 1, "one press of Right moves exactly one peg");
    }

    #[test]
    fn the_middle_peg_is_reachable_from_both_ends() {
        // The whole puzzle turned on this. With the release firing too, Right
        // from peg 1 landed on peg 3 and Left from peg 3 landed on peg 1, so
        // peg 2 could not be selected at all — and every Hanoi solution needs
        // it. The game was unsolvable from the keyboard.
        let mut g = game();
        tap(&mut g, Key::Right);
        assert_eq!(g.cursor(), 1);

        let mut g = game();
        tap(&mut g, Key::Right);
        tap(&mut g, Key::Right);
        assert_eq!(g.cursor(), 2);
        tap(&mut g, Key::Left);
        assert_eq!(g.cursor(), 1, "Left from the far peg stops in the middle");
    }

    #[test]
    fn enter_lifts_a_disk_and_leaves_it_lifted() {
        // Press-and-release used to lift the disk and put it straight back:
        // dropping onto the peg you took it from is a change of mind, so
        // `Enter` did nothing at all, ever.
        let mut g = game();
        tap(&mut g, Key::Enter);
        assert_eq!(g.held(), Some((0, 1)), "Enter leaves the disk in hand");
        assert_eq!(g.peg(0), [4, 3, 2]);
    }

    #[test]
    fn enter_twice_lifts_then_drops() {
        let mut g = game();
        tap(&mut g, Key::Enter);
        tap(&mut g, Key::Right);
        tap(&mut g, Key::Enter);
        assert_eq!(g.held(), None);
        assert_eq!(g.peg(1), [1]);
        assert_eq!(g.moves(), 1);
    }

    #[test]
    fn undo_takes_back_one_move_per_press() {
        let mut g = game();
        for (from, to) in [(0, 1), (0, 2)] {
            g.apply(Action::Touch(from));
            g.apply(Action::Touch(to));
        }
        assert_eq!(g.moves(), 2);
        tap(&mut g, Key::Z);
        assert_eq!(g.moves(), 1, "Z undoes one move, not two");
    }

    #[test]
    fn help_stays_up_after_the_key_is_let_go() {
        let mut g = game();
        assert!(!g.show_help());
        tap(&mut g, Key::H);
        assert!(
            g.show_help(),
            "H opened help and the release closed it again"
        );
        tap(&mut g, Key::H);
        assert!(!g.show_help());
    }

    #[test]
    fn every_disk_count_can_be_reached_from_the_keyboard() {
        // Stepping two at a time made 5 and 7 unreachable, so their rows in the
        // best-score table read nothing at all however long you played.
        let mut g = game();
        let mut seen = vec![g.disks()];
        for _ in 0..MAX_DISKS {
            tap(&mut g, Key::Up);
            seen.push(g.disks());
        }
        for _ in 0..MAX_DISKS * 2 {
            tap(&mut g, Key::Down);
            seen.push(g.disks());
        }
        for n in MIN_DISKS..=MAX_DISKS {
            assert!(seen.contains(&n), "disk count {n} is unreachable");
        }
    }

    #[test]
    fn the_disk_count_stops_at_both_ends() {
        let mut g = game();
        for _ in 0..12 {
            tap(&mut g, Key::Up);
        }
        assert_eq!(g.disks(), MAX_DISKS);
        for _ in 0..12 {
            tap(&mut g, Key::Down);
        }
        assert_eq!(g.disks(), MIN_DISKS);
    }

    // ── Fault 2: the hit test never looked at y ───────────────────────────

    #[test]
    fn a_click_on_the_title_does_not_touch_a_disk() {
        let mut g = game();
        let size = (g.width(), g.height());
        let l = g.layout(size.0, size.1);
        let outcome = poke(
            &mut g,
            l.header.x + l.header.w * 0.05,
            l.header.y + l.header.h / 2.0,
            size,
        );
        assert_eq!(outcome, EventResult::Ignored);
        assert_eq!(g.held(), None);
        assert_eq!(g.peg(0), [4, 3, 2, 1]);
    }

    #[test]
    fn a_click_on_the_status_line_does_not_touch_a_disk() {
        let mut g = game();
        let size = (g.width(), g.height());
        let l = g.layout(size.0, size.1);
        poke(
            &mut g,
            l.info.x + l.info.w / 2.0,
            l.info.y + l.info.h / 2.0,
            size,
        );
        assert_eq!(g.held(), None);
        assert_eq!(g.moves(), 0);
    }

    #[test]
    fn a_click_on_a_peg_lifts_its_top_disk() {
        let mut g = game();
        assert_eq!(click(&mut g, Target::Peg(0)), EventResult::Consumed);
        assert_eq!(g.held(), Some((0, 1)));
    }

    #[test]
    fn the_peg_columns_do_not_overlap_and_cover_the_board() {
        for &(w, h) in WINDOWS {
            let g = windowed(w, h);
            let l = g.layout(w, h);
            let mut edge = l.board.x;
            for p in 0..PEGS {
                let col = l.peg(p);
                assert!(
                    (col.x - edge).abs() < 0.01,
                    "{w}x{h}: peg {p} starts at {} not {edge}",
                    col.x
                );
                edge = col.right();
            }
            assert!(
                (edge - l.board.right()).abs() < 0.01,
                "{w}x{h}: the columns leave a strip of board uncovered"
            );
        }
    }

    #[test]
    fn a_click_is_read_against_the_size_the_frame_was_drawn_at() {
        let mut g = windowed(1280.0, 720.0);
        let r = probe::rect_of_sized(&g, Target::Peg(2), (1280.0, 720.0)).unwrap();
        poke(&mut g, r.x + r.w / 2.0, r.y + r.h / 2.0, (1280.0, 720.0));
        assert_eq!(g.cursor(), 2, "the click landed on the peg it was drawn on");
    }

    #[test]
    fn every_control_is_reachable_by_a_click_in_a_normal_window() {
        let mut g = windowed(1024.0, 768.0);
        for target in [
            Target::Peg(0),
            Target::Peg(1),
            Target::Peg(2),
            Target::Fewer,
            Target::More,
            Target::Undo,
            Target::Solve,
            Target::NewGame,
            Target::Help,
        ] {
            assert!(
                probe::is_visible_sized(&g, target, (1024.0, 768.0)),
                "{} cannot be clicked",
                probe::variant_name(target)
            );
        }
        // The disk-count readout is not a control and records no hit box, so a
        // click on it must do nothing rather than doing whichever neighbour is
        // drawn underneath.
        let l = g.layout(1024.0, 768.0);
        let mid = l.control(1);
        let before = g.disks();
        poke(
            &mut g,
            mid.x + mid.w / 2.0,
            mid.y + mid.h / 2.0,
            (1024.0, 768.0),
        );
        assert_eq!(g.disks(), before);
    }

    #[test]
    fn nothing_is_painted_outside_the_window() {
        for &(w, h) in WINDOWS {
            let mut g = windowed(w, h);
            // With the help sheet up as well: it is the widest thing drawn.
            for _ in 0..2 {
                for r in painted(&g, w, h) {
                    assert!(
                        r.x >= -0.01
                            && r.y >= -0.01
                            && r.right() <= w + 0.01
                            && r.bottom() <= h + 0.01,
                        "{w}x{h}: ink at ({}, {}) {}x{} falls off the window",
                        r.x,
                        r.y,
                        r.w,
                        r.h
                    );
                }
                g.apply(Action::ToggleHelp);
            }
        }
    }

    #[test]
    fn the_board_keeps_its_share_of_every_window() {
        // The old build pinned the scores panel at x = 690 and ran the help
        // sheet down to y = 530, so both vanished from any window smaller than
        // the one they were measured in.  Chrome now yields to the board.
        for &(w, h) in WINDOWS {
            let g = windowed(w, h);
            let l = g.layout(w, h);
            assert!(
                l.board.h >= h * BOARD_SHARE - 0.01,
                "{w}x{h}: the board is down to {} of {h}",
                l.board.h
            );
            assert!(
                (l.board.w - (w - l.pad * 2.0)).abs() < 0.01 && l.pad <= 10.0,
                "{w}x{h}: the board is {} wide inside a {w} window",
                l.board.w
            );
        }
    }

    #[test]
    fn a_band_is_dropped_whole_and_never_comes_back() {
        // Half a row of buttons is worse than no row: a band is either at its
        // full height or absent.  These widths are all wide enough for every
        // row; the narrow case is `a_row_too_narrow_to_read_is_not_drawn`.
        for &w in &[280.0_f32, 760.0, 1920.0] {
            let mut seen = [false; 4];
            let mut height = 60.0_f32;
            while height <= 1200.0 {
                let g = windowed(w, height);
                let l = g.layout(w, height);
                let now = bands(&l);
                for (i, (&kept, was)) in now.iter().zip(seen.iter_mut()).enumerate() {
                    assert!(
                        kept || !*was,
                        "{w}x{height}: band {i} came back as the window grew"
                    );
                    *was = kept;
                }
                if l.shows_header() {
                    assert!(l.header.h >= 24.0, "{w}x{height}: half a header");
                }
                if l.shows_controls() {
                    assert!(l.controls.h >= 22.0, "{w}x{height}: half a control row");
                }
                height += 20.0;
            }
            assert_eq!(seen, [true; 4], "{w}: a tall window should keep everything");
        }
    }

    #[test]
    fn a_row_too_narrow_to_read_is_not_drawn() {
        // Height is not the only thing a row needs.  Six score cells squeezed
        // into 200 px are six illegible slivers, so the row goes rather than
        // shrinks — and its buttons stop answering clicks with it.
        let tall = 1000.0_f32;
        let wide = windowed(760.0, tall);
        assert!(wide.layout(760.0, tall).shows_scores());
        assert!(wide.layout(760.0, tall).shows_controls());

        let mut narrow = windowed(200.0, tall);
        let l = narrow.layout(200.0, tall);
        assert!(!l.shows_scores(), "six score cells do not fit in 200 px");
        assert!(
            !l.shows_controls(),
            "the control row does not fit in 200 px"
        );
        for target in [Target::Fewer, Target::More, Target::Undo, Target::Solve] {
            assert!(
                !probe::is_visible_sized(&narrow, target, (200.0, tall)),
                "{} answers clicks though it is not drawn",
                probe::variant_name(target)
            );
        }
        // The pegs are what is left, and they still work.
        let before = narrow.cursor();
        assert_eq!(click(&mut narrow, Target::Peg(2)), EventResult::Consumed);
        assert_ne!(narrow.cursor(), before);
    }

    #[test]
    fn the_bands_go_in_the_stated_order() {
        // Least important first: scores, then the title, then the controls.
        // The status line is the last thing to go, because it is the only
        // place the puzzle says what it wants.
        let mut order = Vec::new();
        let mut prev = [true; 4];
        let mut height = 1200.0_f32;
        while height >= 60.0 {
            let g = windowed(760.0, height);
            let now = bands(&g.layout(760.0, height));
            for (i, (&kept, &was)) in now.iter().zip(prev.iter()).enumerate() {
                if was && !kept {
                    order.push(i);
                }
            }
            prev = now;
            height -= 4.0;
        }
        // Spelled out rather than read back from `BAND_DROP_ORDER`: a test that
        // compares the behaviour against the constant that produced it agrees
        // with any order at all.  3 = the best-score row, which is a record of
        // games already finished; 0 = the title, which says what the window
        // already says; 2 = the buttons, all of which have keys.  1 = the
        // status line goes last, because it is the only place the puzzle says
        // how it is going.
        assert_eq!(
            order,
            [3, 0, 2, 1]
                .iter()
                .copied()
                .take(order.len())
                .collect::<Vec<_>>(),
            "bands left in an order other than least-useful-first"
        );
        assert!(order.len() >= 3, "too few bands were dropped: {order:?}");
    }

    #[test]
    fn the_puzzle_is_still_playable_in_a_window_too_small_for_the_chrome() {
        let mut g = windowed(180.0, 120.0);
        let l = g.layout(180.0, 120.0);
        assert!(!l.shows_scores(), "the scores should be the first to go");
        for p in 0..PEGS {
            assert!(
                probe::is_visible_sized(&g, Target::Peg(p), (180.0, 120.0)),
                "peg {p} is unreachable in a small window"
            );
        }
        // And a whole game can be played through the pegs alone.
        g.apply(Action::SetDisks(3));
        solve_by_hand(&mut g);
        assert_eq!(g.state(), GameState::Solved);
    }

    // ── Fault 5: a held disk is off its peg ───────────────────────────────

    #[test]
    fn holding_a_disk_takes_it_off_its_peg() {
        let mut g = game();
        g.apply(Action::Touch(0));
        assert_eq!(g.held(), Some((0, 1)));
        assert_eq!(g.peg(0), [4, 3, 2], "the held disk is on no stack");
        assert_eq!(g.top(0), Some(2), "the peg's top is what is really on it");
    }

    #[test]
    fn a_held_disk_can_be_put_back_without_costing_a_move() {
        let mut g = game();
        g.apply(Action::Touch(0));
        assert!(g.apply(Action::Cancel));
        assert_eq!(g.held(), None);
        assert_eq!(g.peg(0), [4, 3, 2, 1]);
        assert_eq!(g.moves(), 0);
        assert_eq!(g.undo_depth(), 0);
    }

    #[test]
    fn dropping_back_onto_the_peg_it_came_from_is_not_a_move() {
        let mut g = game();
        g.apply(Action::Touch(0));
        g.apply(Action::Touch(0));
        assert_eq!(g.held(), None);
        assert_eq!(g.moves(), 0);
        assert_eq!(g.peg(0), [4, 3, 2, 1]);
    }

    #[test]
    fn a_disk_cannot_be_dropped_on_a_smaller_one() {
        let mut g = game();
        g.apply(Action::Touch(0)); // lift 1
        g.apply(Action::Touch(1)); // drop on peg 2
        g.apply(Action::Touch(0)); // lift 2
        assert_eq!(g.held(), Some((0, 2)));
        assert!(!g.drop_on(1), "2 must not go on 1");
        assert_eq!(g.held(), Some((0, 2)), "it stays in your hand");
        assert_eq!(g.moves(), 1);
    }

    #[test]
    fn a_refused_peg_still_moves_the_marker() {
        let mut g = game();
        g.apply(Action::Touch(0));
        g.apply(Action::Touch(1));
        g.apply(Action::Touch(0)); // holding 2
        assert!(g.apply(Action::Touch(1)), "aiming counts as a change");
        assert_eq!(g.cursor(), 1);
        assert_eq!(g.held(), Some((0, 2)));
    }

    // ── Fault 4: the disk-count guard ─────────────────────────────────────

    #[test]
    fn the_disk_count_guard_says_what_it_means() {
        let mut g = game();
        assert!(g.may_change_disks(), "nothing has moved yet");
        g.apply(Action::Touch(0));
        assert!(!g.may_change_disks(), "a disk is in the air");
        g.apply(Action::Touch(1));
        assert!(!g.may_change_disks(), "a move has been made");
        assert!(!g.apply(Action::SetDisks(6)));
        assert_eq!(g.disks(), 4);
        solve_by_hand(&mut g);
        assert!(g.may_change_disks(), "the puzzle is finished");
        assert!(g.apply(Action::SetDisks(6)));
        assert_eq!(g.disks(), 6);
    }

    #[test]
    fn changing_the_disk_count_starts_over() {
        let mut g = game();
        assert!(g.apply(Action::SetDisks(6)));
        assert_eq!(g.peg(0), [6, 5, 4, 3, 2, 1]);
        assert_eq!(g.moves(), 0);
        assert_eq!(g.remaining(), 63);
    }

    #[test]
    fn a_disk_count_outside_the_range_is_refused_not_clamped() {
        let mut g = game();
        assert!(!g.set_disks(2));
        assert!(!g.set_disks(9));
        assert_eq!(g.disks(), 4);
    }

    // ── Fault 6: progress is measured from here, not from the start ───────

    #[test]
    fn the_status_line_counts_from_where_you_are_not_from_the_start() {
        // The figure the old build showed was `2^n - 1 - moves`: a countdown
        // of moves made, which says nothing about the position in front of
        // you.  This one is the shortest completion from where you are, so a
        // move that goes the wrong way does not shorten it.
        let mut g = game();
        assert_eq!(g.remaining(), 15);
        // Four disks: the shortest line opens with the smallest disk onto the
        // middle peg.  Sending it to the far peg instead wastes the move.
        g.apply(Action::Touch(0));
        g.apply(Action::Touch(2));
        assert_eq!(g.moves(), 1);
        assert_eq!(
            g.remaining(),
            15,
            "a move off the shortest line leaves the work undone"
        );
        assert_eq!(
            g.moves() + g.remaining(),
            16,
            "the wasted move shows up as one more than the best possible"
        );
        assert!(says(&g, "15 to go"));

        // The move that is on the shortest line does shorten it.
        let mut h = game();
        h.apply(Action::Touch(0));
        h.apply(Action::Touch(1));
        assert_eq!(h.remaining(), 14);
        assert_eq!(h.moves() + h.remaining(), h.min_moves());
    }

    #[test]
    fn the_opening_position_needs_exactly_two_to_the_n_minus_one() {
        let mut g = game();
        for n in MIN_DISKS..=MAX_DISKS {
            g.force_disks(n);
            assert_eq!(g.remaining(), g.min_moves());
            assert_eq!(g.min_moves(), (1_u32 << n) - 1);
        }
    }

    // ── The solver, against a search that shares no code with it ──────────

    /// Every position `n` disks can be in, one legal move apart.
    fn neighbours(state: &Spots, n: usize) -> Vec<Spots> {
        let mut out = Vec::new();
        for from in 0..PEGS as u8 {
            // Only the smallest disk on a peg can leave it, and scanning by
            // increasing size finds it.
            let Some(disk) = (0..n).find(|&d| state[d] == from) else {
                continue;
            };
            for to in 0..PEGS as u8 {
                if to == from || (0..disk).any(|d| state[d] == to) {
                    continue;
                }
                let mut next = *state;
                next[disk] = to;
                out.push(next);
            }
        }
        out
    }

    /// The fewest moves from every position to the finished one, worked out by
    /// breadth-first search backwards from it.
    ///
    /// This is the oracle. It knows nothing about towers of Hanoi beyond the
    /// two rules — one disk at a time, never onto a smaller one — and every
    /// move is reversible, so a search out from the goal gives the distance
    /// back to it. If `cost_to` and this ever disagree, `cost_to` is wrong.
    fn distances(n: usize) -> HashMap<Spots, u32> {
        let mut goal: Spots = [0; MAX_DISKS];
        for slot in goal.iter_mut().take(n) {
            *slot = GOAL_PEG as u8;
        }
        let mut dist: HashMap<Spots, u32> = HashMap::new();
        dist.insert(goal, 0);
        let mut queue = VecDeque::new();
        queue.push_back(goal);
        while let Some(state) = queue.pop_front() {
            let d = dist[&state];
            for next in neighbours(&state, n) {
                if let std::collections::hash_map::Entry::Vacant(slot) = dist.entry(next) {
                    slot.insert(d + 1);
                    queue.push_back(next);
                }
            }
        }
        dist
    }

    fn at(n: usize, state: &Spots) -> Towers {
        let mut g = Towers::new();
        g.force_disks(n);
        g.set_spots(state);
        g
    }

    #[test]
    fn every_position_is_reachable_from_every_other() {
        // If this fails the oracle is not seeing the whole space, and the two
        // tests below are checking a subset without saying so.
        for n in MIN_DISKS..=MAX_DISKS {
            assert_eq!(
                distances(n).len(),
                3_usize.pow(n as u32),
                "{n} disks: the search did not reach every position"
            );
        }
    }

    #[test]
    fn the_moves_left_figure_matches_the_search_at_every_position() {
        for n in MIN_DISKS..=MAX_DISKS {
            for (state, &d) in &distances(n) {
                assert_eq!(
                    cost_to(state, n, GOAL_PEG as u8),
                    d,
                    "{n} disks, position {state:?}"
                );
            }
        }
    }

    #[test]
    fn the_moves_left_figure_is_what_the_puzzle_reports() {
        for n in MIN_DISKS..=MAX_DISKS {
            for (state, &d) in &distances(n) {
                assert_eq!(at(n, state).remaining(), d, "{n} disks, {state:?}");
            }
        }
    }

    #[test]
    fn every_suggested_move_is_legal_and_gets_one_move_closer() {
        for n in MIN_DISKS..=MAX_DISKS {
            let dist = distances(n);
            for (state, &d) in &dist {
                let mut g = at(n, state);
                match g.next_optimal() {
                    None => assert_eq!(d, 0, "{n} disks: no move offered at {state:?}"),
                    Some((from, to)) => {
                        assert!(d > 0, "{n} disks: a move offered on a finished puzzle");
                        assert!(g.apply(Action::Touch(from)), "cannot lift from {from}");
                        assert!(g.apply(Action::Touch(to)), "cannot drop on {to}");
                        assert_eq!(
                            dist[&g.spots()],
                            d - 1,
                            "{n} disks, {state:?}: {from}->{to} did not get closer"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn following_the_suggestions_finishes_in_the_fewest_moves() {
        for n in MIN_DISKS..=5 {
            for (state, &d) in &distances(n) {
                let mut g = at(n, state);
                if d == 0 {
                    // Handed the finished position: nothing to suggest.  (The
                    // real game can never start here; `set_spots` can.)
                    assert!(!g.step_once(), "{n} disks: a move offered at the goal");
                    continue;
                }
                while g.playing() {
                    if !g.step_once() {
                        break;
                    }
                }
                assert_eq!(g.state(), GameState::Solved, "{n} disks, {state:?}");
                assert_eq!(g.moves(), d, "{n} disks, {state:?}: took the long way");
            }
        }
    }

    #[test]
    fn a_full_solve_of_the_largest_puzzle_takes_two_five_five_moves() {
        let mut g = game();
        g.force_disks(8);
        while g.playing() && g.step_once() {}
        assert_eq!(g.state(), GameState::Solved);
        assert_eq!(g.moves(), 255);
        assert_eq!(g.peg(GOAL_PEG), [8, 7, 6, 5, 4, 3, 2, 1]);
    }

    #[test]
    fn a_disk_in_your_hand_counts_as_being_where_it_came_from() {
        // Otherwise "moves left" would be a number about no legal position at
        // all, and the hint would be a suggestion for a game nobody is playing.
        let mut g = game();
        let before = g.remaining();
        g.apply(Action::Touch(0));
        assert_eq!(g.remaining(), before, "lifting a disk is not a move");
        assert_eq!(g.spots()[0], 0);
    }

    #[test]
    fn a_hint_puts_a_held_disk_back_before_it_moves_anything() {
        let mut g = game();
        g.apply(Action::Touch(1)); // aims at the empty middle peg, lifts nothing
        g.apply(Action::Touch(0)); // lifts disk 1
        assert_eq!(g.held(), Some((0, 1)));
        assert!(g.apply(Action::Step));
        assert_eq!(g.held(), None);
        assert_eq!(g.moves(), 1);
    }

    // ── The clock the solver runs on ──────────────────────────────────────

    #[test]
    fn no_ticks_are_asked_for_while_nothing_is_moving() {
        let mut g = game();
        assert_eq!(g.tick_interval(), None);
        assert!(g.apply(Action::ToggleSolve));
        assert_eq!(g.tick_interval(), Some(Duration::from_millis(TICK_MS)));
        assert!(g.apply(Action::ToggleSolve));
        assert_eq!(g.tick_interval(), None, "stopping stops the clock too");
    }

    #[test]
    fn a_tick_with_nothing_to_do_is_ignored() {
        let mut g = game();
        assert_eq!(
            handle_event(&mut g, &Event::Tick { elapsed_ms: 16 }),
            EventResult::Ignored
        );
        assert_eq!(g.moves(), 0);
    }

    #[test]
    fn the_solver_moves_on_the_clock_not_all_at_once() {
        let mut g = game();
        g.apply(Action::ToggleSolve);
        handle_event(&mut g, &Event::Tick { elapsed_ms: 16 });
        assert_eq!(g.moves(), 1, "the first move lands straight away");
        handle_event(&mut g, &Event::Tick { elapsed_ms: 16 });
        assert_eq!(g.moves(), 1, "and the next one waits");
        for _ in 0..(SOLVE_STEP_MS / 16) {
            handle_event(&mut g, &Event::Tick { elapsed_ms: 16 });
        }
        assert_eq!(g.moves(), 2);
    }

    #[test]
    fn the_solver_is_paced_by_the_time_reported_not_by_tick_count() {
        let mut fast = game();
        let mut slow = game();
        fast.apply(Action::ToggleSolve);
        slow.apply(Action::ToggleSolve);
        for _ in 0..40 {
            handle_event(&mut fast, &Event::Tick { elapsed_ms: 8 });
        }
        for _ in 0..10 {
            handle_event(&mut slow, &Event::Tick { elapsed_ms: 32 });
        }
        assert_eq!(
            fast.moves(),
            slow.moves(),
            "the same elapsed time is the same number of moves"
        );
    }

    #[test]
    fn the_solver_runs_to_the_end_and_stops() {
        let mut g = game();
        g.apply(Action::ToggleSolve);
        settle(&mut g);
        assert_eq!(g.state(), GameState::Solved);
        assert_eq!(g.moves(), 15);
        assert_eq!(g.tick_interval(), None);
    }

    #[test]
    fn touching_a_peg_stops_the_solver() {
        let mut g = game();
        g.apply(Action::ToggleSolve);
        handle_event(&mut g, &Event::Tick { elapsed_ms: 16 });
        assert!(g.solving());
        g.apply(Action::Touch(2));
        assert!(!g.solving(), "two players must not share one board");
    }

    #[test]
    fn undo_stops_the_solver_and_takes_its_move_back() {
        let mut g = game();
        g.apply(Action::ToggleSolve);
        handle_event(&mut g, &Event::Tick { elapsed_ms: 16 });
        assert_eq!(g.moves(), 1);
        g.apply(Action::Undo);
        assert!(!g.solving());
        assert_eq!(g.moves(), 0);
        assert_eq!(g.peg(0), [4, 3, 2, 1]);
    }

    #[test]
    fn a_finished_puzzle_offers_nothing_to_solve() {
        let mut g = game();
        solve_by_hand(&mut g);
        assert!(!g.apply(Action::ToggleSolve));
        assert!(!g.solving());
        assert_eq!(g.next_optimal(), None);
    }

    // ── Taking moves back ─────────────────────────────────────────────────

    #[test]
    fn undo_walks_all_the_way_back_to_the_start() {
        let mut g = game();
        for _ in 0..6 {
            g.step_once();
        }
        assert_eq!(g.moves(), 6);
        for _ in 0..6 {
            assert!(g.apply(Action::Undo));
        }
        assert_eq!(g.moves(), 0);
        assert_eq!(g.peg(0), [4, 3, 2, 1]);
        assert!(g.peg(1).is_empty());
        assert!(g.peg(2).is_empty());
        assert!(!g.apply(Action::Undo), "there is nothing left to take back");
    }

    #[test]
    fn undo_is_refused_with_a_disk_in_the_air() {
        let mut g = game();
        g.apply(Action::Touch(0));
        g.apply(Action::Touch(1));
        g.apply(Action::Touch(0));
        assert_eq!(g.held(), Some((0, 2)));
        assert!(!g.undo());
        assert_eq!(g.moves(), 1);
    }

    #[test]
    fn a_change_of_mind_is_not_something_to_undo() {
        let mut g = game();
        g.apply(Action::Touch(0));
        g.apply(Action::Touch(0));
        assert_eq!(g.undo_depth(), 0);
    }

    // ── The record ────────────────────────────────────────────────────────

    #[test]
    fn a_solve_of_your_own_is_recorded() {
        let mut g = game();
        solve_by_hand(&mut g);
        assert_eq!(g.state(), GameState::Solved);
        assert_eq!(g.best()[g.best_row()], Some(15));
        assert!(says(&g, "the shortest there is"));
    }

    #[test]
    fn a_solve_you_were_shown_is_not_recorded() {
        // Otherwise every row would read `2^n - 1` by the end of the first
        // afternoon and the table would have nothing left to say.
        let mut g = game();
        g.apply(Action::ToggleSolve);
        settle(&mut g);
        assert_eq!(g.state(), GameState::Solved);
        assert!(g.assisted());
        assert_eq!(g.best()[g.best_row()], None);
        assert!(says(&g, "with help"));
    }

    #[test]
    fn one_hint_is_enough_to_make_a_solve_assisted() {
        let mut g = game();
        assert!(g.apply(Action::Step));
        solve_by_hand(&mut g);
        assert!(g.assisted());
        assert_eq!(g.best()[g.best_row()], None);
    }

    #[test]
    fn a_worse_solve_does_not_replace_a_better_one() {
        let mut g = game();
        solve_by_hand(&mut g);
        assert_eq!(g.best()[g.best_row()], Some(15));
        g.apply(Action::NewGame);
        // The same solve with two wasted moves in front of it.
        g.apply(Action::Touch(0));
        g.apply(Action::Touch(1));
        g.apply(Action::Touch(1));
        g.apply(Action::Touch(0));
        solve_by_hand(&mut g);
        assert_eq!(g.moves(), 17);
        assert_eq!(g.best()[g.best_row()], Some(15), "the record stands");
        assert!(says(&g, "15 is the shortest"));
    }

    #[test]
    fn a_better_solve_replaces_a_worse_one() {
        let mut g = game();
        g.apply(Action::Touch(0));
        g.apply(Action::Touch(1));
        g.apply(Action::Touch(1));
        g.apply(Action::Touch(0));
        solve_by_hand(&mut g);
        assert_eq!(g.best()[g.best_row()], Some(17));
        g.apply(Action::NewGame);
        solve_by_hand(&mut g);
        assert_eq!(g.best()[g.best_row()], Some(15));
    }

    #[test]
    fn the_record_is_kept_per_disk_count() {
        let mut g = game();
        solve_by_hand(&mut g);
        assert!(g.apply(Action::SetDisks(3)));
        solve_by_hand(&mut g);
        let best = g.best();
        assert_eq!(best[0], Some(7), "three disks");
        assert_eq!(best[1], Some(15), "four disks");
        assert_eq!(best[2], None, "five disks, never played");
    }

    #[test]
    fn the_record_survives_a_new_game_but_the_assist_flag_does_not() {
        let mut g = game();
        g.apply(Action::Step);
        assert!(g.assisted());
        g.apply(Action::NewGame);
        assert!(!g.assisted(), "a fresh attempt starts unassisted");
        solve_by_hand(&mut g);
        assert_eq!(g.best()[g.best_row()], Some(15));
    }

    #[test]
    fn a_position_reached_with_help_can_still_be_finished_honestly() {
        // `clear_assist` is the test's way of saying "pretend those moves were
        // mine" — the program has no such button, which is the point.
        let mut g = game();
        g.apply(Action::Step);
        g.clear_assist();
        solve_by_hand(&mut g);
        assert_eq!(g.best()[g.best_row()], Some(15));
    }
}
