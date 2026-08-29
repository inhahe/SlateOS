//! Simon — watch a growing sequence of four colours light up, then repeat it.
//!
//! Four pads in a two-by-two grid. Each round the machine adds one colour to
//! the end of a sequence and plays the whole thing back; the player then has to
//! press the same pads in the same order. One wrong pad ends the game. Three
//! speeds change how fast the playback runs.
//!
//! ## What wiring this up found
//!
//! The state machine and the sequence were sound. Everything between them and a
//! person was not:
//!
//! 1. **`main` was `let _app = SimonApp::new();`.** It built the game, drew the
//!    first colour and exited. No window was opened, nothing was painted, and
//!    no key ever arrived.
//! 2. **`render` was not given the window at all.** It took `&self` and no
//!    size, and every rectangle in it was a compile-time constant: a
//!    160-pixel pad at a fixed `(GRID_X, 160.0)`, an info panel at
//!    `GRID_Y + BUTTON_SIZE * 2.0 + BUTTON_GAP + 30.0`, and a background
//!    rectangle `WINDOW_WIDTH` by `WINDOW_HEIGHT` — so a window the user
//!    resized showed the same picture in the wrong place, with unpainted
//!    canvas around it. `Layout` is derived from the live window size every
//!    frame now.
//! 3. **Nothing was clickable.** There was no mouse code in the file: `Event`
//!    was matched for `Key` and `Tick` and everything else fell into `_ => {}`.
//!    For a game whose entire interface is four big buttons, a pointer could do
//!    nothing at all. Every pad is a hit box now, recorded by the pass that
//!    paints it, and there is a footer with the three verbs that are not a pad.
//! 4. **The module documentation promised persistence that did not exist.**
//!    "High score tracking persists across restarts" — but `high_score` was
//!    `0` at construction and there was no load and no save anywhere in the
//!    crate, nor anything in the toolkit to save *to*. The claim is now what is
//!    true: the best is the best of this session. The real gap is logged in
//!    `known-issues.md` rather than papered over here.
//! 5. **The best score was kept twice, under two names.** `high_score` and
//!    `longest_streak` were raised by the same two `if self.score > …` pairs, in
//!    the same two places, and then shown to the player as "Best" and "Best
//!    streak" — two readouts that could not possibly disagree, taking up room
//!    that a readout carrying information would have used. There is one `best`
//!    now, raised in one place: the function that increments the score, which
//!    is the only event that can raise it. `trigger_game_over`'s copy was not
//!    moved, it was deleted — a high-water mark maintained at every rise needs
//!    no catch-up at the end.
//! 6. **A window that had just opened said "Games: 1".** `games_played` was
//!    incremented by `start_new_game`, and `with_seed` calls `start_new_game`
//!    to deal the first colour — so the counter began at one before anybody had
//!    played anything. It counts games *lost* now, which is the only way a
//!    Simon game ends, and it is incremented in exactly the one place a game
//!    ends.
//! 7. **Both animations ran at frame rate rather than on the clock.**
//!    `pulse_counter` was `wrapping_add(1)` once per *tick*, and the pulse and
//!    the blink were `% 8` and `% 6` of it. `Event::Tick` carries
//!    `elapsed_ms` — the machine's frame interval, not a fixed unit — so the
//!    animations sped up and slowed down with the compositor's load, and on a
//!    fast display the "blink" was a flicker. Both are derived from accumulated
//!    milliseconds now, so they take the same wall-clock time everywhere.
//! 8. **`MAX_SEQUENCE_LEN = 999` was declared and never read.** A limit nothing
//!    enforces is not a limit; it is a comment with a type. Deleted rather than
//!    enforced, because enforcing it needs a *behaviour* at the boundary — is
//!    999 a win? a stall? — and the constant never had one to enforce.
//! 9. **`round` and the sequence could disagree.** `start_next_round` raised
//!    `round` and then pushed a colour *conditionally*, on an `ALL.get` that
//!    could in principle miss; the round number and the thing it counts were
//!    two facts. The round is now read off the sequence's own length, so there
//!    is nothing to keep in step.
//! 10. **The two-by-two grid was written out four times.** `ALL`'s order,
//!     `to_index`'s match, `grid_pos`'s match, and the arrow keys' literal
//!     `2`s. A fifth colour would have had to be added in four places and would
//!     have compiled after three. The grid is `PAD_COLS` by `PAD_ROWS` now,
//!     positions are arithmetic on the index, and a `const` assertion ties the
//!     two to the length of `ALL`.
//! 11. **Pad labels were centred by guessing.** `x + BUTTON_SIZE / 2.0 - 20.0`
//!     and `- 10.0` — constants that happen to centre the word "Green" at one
//!     font size and centre nothing else. Labels are measured and centred now.
//! 12. **The glow was painted over the thing it was meant to be behind.** A lit
//!     pad pushed its face and *then* pushed a larger translucent rectangle of
//!     the same colour at `x - 4.0, y - 4.0`. Commands paint in order, so the
//!     "glow" covered the pad's face and its label with a 60-alpha wash instead
//!     of haloing it. It is drawn first now, and inset proportionally so it
//!     cannot reach outside a small window.
//! 13. **The window said the same word twice, one of them blinking.** The info
//!     panel read "Watch! (2/3)" while a separate indicator blinked "WATCH"
//!     above the grid. The blinking copy is gone; the one that carries the step
//!     number stayed.
//! 14. **The pause before playback threw away its overshoot.** A tick that
//!     carried the pre-sequence delay past its end started the first flash from
//!     zero, so the first colour of every round was shown for one frame longer
//!     than the rest — more on a slow machine, since the overshoot is the frame
//!     interval. The remainder is carried into the first flash now.
//! 15. **Changing speed mid-playback could skip a colour.** The elapsed time in
//!     the current phase was measured against the *new* duration, so switching
//!     from Slow to Fast with 700ms on a flash ended the flash immediately, ran
//!     700 − 300 = 400ms straight through the 150ms gap after it, and advanced
//!     two steps — in the middle of the sequence the player is being asked to
//!     memorise. A speed change restarts the current phase now.
//! 16. **The pad the player lost on stayed lit for ever.** The flash timer
//!     cleared `lit_button` only `if self.state == GameState::PlayerInput`, and
//!     a wrong press leaves `PlayerInput` immediately — so the losing pad was
//!     still lit under the game-over overlay, and stayed lit through the next
//!     game's countdown.
//! 17. **Four code paths wrote `lit_button`,** and the fault above is what that
//!     costs. Which pad is lit is not state: it is a question about the state
//!     machine and the timers, and it has one answer. [`Simon::lit`] computes
//!     it, and nothing stores it.
//! 18. **The restart hint listed three ways to restart out of five.** "Enter /
//!     R / 1-4 to restart", while `Escape` and `Space` restarted too. The keys
//!     and the sheet that documents them are one table now, and a test walks it
//!     both ways: every key the sheet lists does something, and every key that
//!     does something is on the sheet.
//! 19. **Ten crate-wide `allow`s,** including `dead_code` and
//!     `unused_imports` — the two that would have said the quiet part out loud,
//!     since `Modifiers` was imported and never used and the whole render half
//!     of the file was unreachable. They are gone; what survives is scoped to
//!     the test module, where a panic on bad data is the diagnosis rather than
//!     the fault.
//! 20. **Modifiers were ignored, so `Ctrl+S` cycled the speed.** `handle_key`
//!     took a bare `Key` and never saw `modifiers`, so every desktop
//!     accelerator that happens to end in one of this game's letters was also
//!     one of this game's controls.

use guitk::color::Color;
use guitk::event::{Event, EventResult, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::frame::Rect;
use guitk::probe::Probe;
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::rng::{RandomSource, SeededRng, seed_from_system};
use guitk::style::CornerRadii;
use guitk::text;
use oswindow::app::{self, App, Response};
use std::process::ExitCode;
use std::time::Duration;

// ── Catppuccin Mocha palette ────────────────────────────────────────────────

const COL_BASE: Color = Color::from_hex(0x1E1E2E);
const COL_MANTLE: Color = Color::from_hex(0x181825);
const COL_CRUST: Color = Color::from_hex(0x11111B);
const COL_SURFACE0: Color = Color::from_hex(0x313244);
const COL_SURFACE1: Color = Color::from_hex(0x45475A);
const COL_TEXT: Color = Color::from_hex(0xCDD6F4);
const COL_SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const COL_OVERLAY0: Color = Color::from_hex(0x6C7086);
const COL_BLUE: Color = Color::from_hex(0x89B4FA);
const COL_GREEN: Color = Color::from_hex(0xA6E3A1);
const COL_RED: Color = Color::from_hex(0xF38BA8);
const COL_YELLOW: Color = Color::from_hex(0xF9E2AF);
const COL_PEACH: Color = Color::from_hex(0xFAB387);
const COL_MAUVE: Color = Color::from_hex(0xCBA6F7);
const COL_TEAL: Color = Color::from_hex(0x94E2D5);
const COL_LAVENDER: Color = Color::from_hex(0xB4BEFE);

// ── The grid ────────────────────────────────────────────────────────────────

/// Columns and rows of pads. Written once each and used everywhere, so the
/// shape of the grid is one fact rather than the four it was: `ALL`'s order,
/// `to_index`'s match, `grid_pos`'s match, and the arrow keys' literal `2`s.
const PAD_COLS: usize = 2;
const PAD_ROWS: usize = 2;

/// The grid and the colour list are the same size, checked by the compiler
/// rather than by a comment. A fifth colour with no pad to sit on would
/// otherwise be a colour the sequence can play and the player cannot press.
const _: () = assert!(
    PAD_COLS * PAD_ROWS == SimonColor::ALL.len(),
    "every colour needs a pad and every pad needs a colour"
);

/// The `(row, column)` of the pad at `index`.
///
/// This and `grid_index` are the only two places in the program that know how
/// a pad's number relates to its position. Before the rewrite the same
/// `/ 2` and `% 2` were written out wherever they were needed — in the
/// layout, in the arrow keys, and in a `match` per colour — which is why a
/// fifth colour would have compiled with three of the four updated.
fn grid_pos(index: usize) -> (usize, usize) {
    (index / PAD_COLS, index % PAD_COLS)
}

/// The pad index at `(row, column)`, or `None` if that is off the grid.
fn grid_index(row: usize, col: usize) -> Option<usize> {
    if row >= PAD_ROWS || col >= PAD_COLS {
        return None;
    }
    row.checked_mul(PAD_COLS)?.checked_add(col)
}

// ── Window ──────────────────────────────────────────────────────────────────

/// The size the window is asked to open at. A request, not a promise — every
/// rectangle is computed from the size the frame is actually given.
const WINDOW_WIDTH: f32 = 520.0;
const WINDOW_HEIGHT: f32 = 620.0;

/// The smallest font the renderer will draw as asked.
///
/// `gui/font`'s `round_px` clamps a size to at least one whole pixel and then
/// rounds it, so a request below a pixel is not drawn small — it is drawn a
/// whole pixel high, *larger* than the layout asked for (`known-issues.md`
/// lesson 60). Every caller here shrinks its type to fit its band, so below
/// this point the renderer would silently overrule all of them.
const MIN_DRAWN_FONT: f32 = 1.0;

/// The share of the window height the pad grid is guaranteed before any band is
/// allowed a pixel. Bands are dropped whole until the rest fit.
const PAD_SHARE: f32 = 0.45;

/// Which band is given up first when they do not all fit: the status line, then
/// the header, and the footer last of all.
///
/// The footer goes last because it is the pointer's only route to a new game, to
/// the speed and to the help — the pads themselves are the route to a *move*,
/// and they are never dropped. The status line goes first because it says what
/// is about to happen, which the pads then say again by lighting up.
const BAND_DROP_ORDER: [usize; 3] = [1, 0, 2];

/// How often the game asks for the clock while something is moving.
///
/// About sixty a second. The flashes are timed in milliseconds from the elapsed
/// time each tick carries, not counted in ticks, so this interval sets how
/// smooth the pulse looks and nothing else — a slower machine shows a coarser
/// pulse over the same wall-clock flash.
const TICK: Duration = Duration::from_millis(16);

// ── Timing ──────────────────────────────────────────────────────────────────

/// How long a pad stays lit during playback, and the gap after it.
///
/// One function returning both, rather than two returning one each: they are
/// read together at every call site and a speed with a flash but no gap is not
/// a speed anyone wants.
fn playback_ms(speed: Speed) -> (u64, u64) {
    match speed {
        Speed::Slow => (800, 400),
        Speed::Medium => (500, 250),
        Speed::Fast => (300, 150),
    }
}

/// The pause between a round starting and the sequence beginning to play.
const PRE_SEQUENCE_MS: u64 = 600;

/// How long a pad stays lit when the player presses it.
const PLAYER_FLASH_MS: u64 = 250;

/// How long the pad the player got wrong stays lit before the overlay appears.
const ERROR_FLASH_MS: u64 = 800;

/// How long the window celebrates a completed round before dealing the next.
const SUCCESS_FLASH_MS: u64 = 600;

/// One full cycle of the pulse that stands in for the sound a lit pad would
/// make. In milliseconds, so it is the same speed on every machine.
const PULSE_PERIOD_MS: u64 = 480;

// ── Randomness ──────────────────────────────────────────────────────────────
// This crate used to carry its own LCG, and its `next_bounded` used to be
// `val % bound`. That was the worst instance of the shared reduction bug in
// the tree: this generator's modulus is 2^64, and in any power-of-two-modulus
// LCG bit *k* of the state has period 2^(k+1) — the low bits are not merely
// weak, they are a counter. `val % 4` read the low *two* bits, whose period is
// exactly 4, and this game draws from four colours, so the sequence it
// produced was Green, Red, Yellow, Blue repeating for ever, identical in every
// game and at every seed. The memory game had nothing to memorise.
//
// It was fixed here first, in place, with a widening multiply. It is now fixed
// for everyone: `randrange`'s `below` is that same widening multiply plus
// Lemire's rejection step, so the draw is *exactly* uniform rather than very
// nearly so. Deleting the local copy is the point — the bug got into sixteen
// crates by being copied, and it can only leave them the same way.

/// Seed used when the kernel's entropy source cannot be reached.
///
/// A per-crate constant rather than a shared one, so that two programs which
/// lose entropy on the same boot do not then produce correlated streams. The
/// bytes spell `SIMON!!!` — a value with no meaning is a value nobody can
/// mistake for a meaningful one.
const FALLBACK_SEED: u64 = 0x5349_4D4F_4E21_2121;

// ── The four colours ────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SimonColor {
    Red,
    Green,
    Blue,
    Yellow,
}

impl SimonColor {
    /// Every colour, in grid order: left to right, then top to bottom.
    ///
    /// This array *is* the grid's ordering. `index` reads a colour's position
    /// out of it and `from_index` reads a colour back, so the two are inverses
    /// by construction rather than by a pair of matches that agree today.
    pub const ALL: [SimonColor; 4] = [
        SimonColor::Red,
        SimonColor::Green,
        SimonColor::Blue,
        SimonColor::Yellow,
    ];

    /// The pad's colour when it is not lit.
    fn dim(self) -> Color {
        match self {
            SimonColor::Red => Color::from_hex(0x8B2240),
            SimonColor::Green => Color::from_hex(0x2D6B3F),
            SimonColor::Blue => Color::from_hex(0x2B4C8C),
            SimonColor::Yellow => Color::from_hex(0x8B7B2A),
        }
    }

    /// The pad's colour when it is lit.
    fn lit(self) -> Color {
        match self {
            SimonColor::Red => COL_RED,
            SimonColor::Green => COL_GREEN,
            SimonColor::Blue => COL_BLUE,
            SimonColor::Yellow => COL_YELLOW,
        }
    }

    /// Where in the grid this colour's pad sits, counting from zero.
    ///
    /// Derived from the position in [`ALL`](Self::ALL) rather than matched out
    /// by hand: the grid was written out four times in this file and adding a
    /// fifth colour would have compiled after updating three of them.
    pub fn index(self) -> usize {
        // `position` over four elements, not a match: the answer has to agree
        // with `ALL`'s order, and the only way to guarantee that is to read it
        // from `ALL`. The `unwrap_or(0)` cannot be reached — `self` is one of
        // the four — and is still not an `unwrap`, because "cannot be reached"
        // is exactly the claim that stops being true when somebody adds a
        // variant and forgets the array.
        Self::ALL.iter().position(|&c| c == self).unwrap_or(0)
    }

    /// The colour at grid position `index`, or `None` past the last pad.
    pub fn from_index(index: usize) -> Option<SimonColor> {
        Self::ALL.get(index).copied()
    }

    fn label(self) -> &'static str {
        match self {
            SimonColor::Red => "Red",
            SimonColor::Green => "Green",
            SimonColor::Blue => "Blue",
            SimonColor::Yellow => "Yellow",
        }
    }

    /// The note this pad would sound, for a machine with no speaker.
    fn tone(self) -> &'static str {
        match self {
            SimonColor::Red => "LOW",
            SimonColor::Green => "MID",
            SimonColor::Blue => "HIGH",
            SimonColor::Yellow => "TOP",
        }
    }
}

// ── Speed ───────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Speed {
    Slow,
    Medium,
    Fast,
}

impl Speed {
    fn label(self) -> &'static str {
        match self {
            Speed::Slow => "Slow",
            Speed::Medium => "Medium",
            Speed::Fast => "Fast",
        }
    }

    /// The next speed round the cycle. Wraps, so the one control can reach all
    /// three and there is no "you are at the end" state to explain.
    #[must_use]
    pub fn next(self) -> Speed {
        match self {
            Speed::Slow => Speed::Medium,
            Speed::Medium => Speed::Fast,
            Speed::Fast => Speed::Slow,
        }
    }
}

// ── Where the game is ───────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameState {
    /// A round has been dealt; the pause before playback is running.
    PreSequence,
    /// The machine is playing the sequence back.
    ShowSequence,
    /// The player is repeating it.
    PlayerInput,
    /// A pad was wrong. The game is over.
    GameOver,
    /// The round was completed; the pause before the next is running.
    RoundSuccess,
}

/// How far through the playback of the sequence the machine is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Playback {
    /// Which step of the sequence is being shown.
    pub step: usize,
    /// Whether the pad is currently lit (`true`) or in the gap after it.
    pub in_flash: bool,
    /// Milliseconds spent in the current flash or gap.
    pub phase_ms: u64,
}

impl Playback {
    fn new() -> Self {
        Self {
            step: 0,
            in_flash: true,
            phase_ms: 0,
        }
    }
}

// ── What a key or a click ultimately asks for ───────────────────────────────

/// The one thing a key or a click ultimately asks for.
///
/// Both routes go through here, so "what does clicking the speed button do" and
/// "what does pressing S do" cannot drift apart: they are the same line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Intent {
    /// Press the pad at this grid index.
    Pad(usize),
    /// Move the keyboard selection one pad.
    Move(Dir),
    /// Press the selected pad — or, when the game is over, start another.
    Confirm,
    CycleSpeed,
    NewGame,
    ToggleHelp,
    CloseHelp,
}

/// Which way an arrow key moves the selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    Up,
    Down,
    Left,
    Right,
}

impl Dir {
    /// The step in `(row, column)`, which is the only place the grid's axes are
    /// named. The bounds are the grid's, checked by the caller.
    fn step(self) -> (isize, isize) {
        match self {
            Dir::Up => (-1, 0),
            Dir::Down => (1, 0),
            Dir::Left => (0, -1),
            Dir::Right => (0, 1),
        }
    }
}

/// Every control the window offers a pointer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Pad(SimonColor),
    NewGame,
    Speed,
    Help,
    /// The help sheet itself: clicking it anywhere shuts it, which is what a
    /// modal panel over a game has to do or the pads behind it take the click.
    HelpSheet,
    /// The panel shown when the game is over. Clicking it starts another.
    GameOver,
}

// ── The layout ──────────────────────────────────────────────────────────────

/// Every rectangle in the window, derived from the window's own size.
///
/// Built fresh on every frame and never remembered. A layout stored on the
/// model is a layout that can disagree with the window it is drawn in, which is
/// the class of fault this file was built out of.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub window: Rect,
    pub header: Rect,
    /// The one line that says what the game is waiting for.
    pub status: Rect,
    /// The two-by-two block of pads, gaps included.
    pub grid: Rect,
    pub footer: Rect,
    pub help: Rect,
    /// The side of one pad's cell of the grid, gaps included.
    pub step: f32,
    pub font: f32,
    pub small: f32,
    pub pad: f32,
}

impl Layout {
    #[must_use]
    pub fn new(width: f32, height: f32) -> Self {
        let w = width.max(1.0);
        let h = height.max(1.0);
        let font = (h / 34.0).clamp(8.0, 20.0);
        let small = (font - 4.0).max(7.0);
        // Padding is bounded above by a quarter of the smaller side so that in a
        // tiny window the padding cannot eat the thing it is padding.
        let pad = (w.min(h) * 0.02).clamp(2.0, 12.0).min(w.min(h) / 4.0);

        // What each band would like, in [header, status, footer] order.
        let mut wants = [
            (h * 0.10).clamp(34.0, 76.0),
            (h * 0.07).clamp(18.0, 40.0),
            (h * 0.08).clamp(24.0, 48.0),
        ];
        let budget = (h - h * PAD_SHARE - pad * 2.0).max(0.0);
        for &i in &BAND_DROP_ORDER {
            if wants.iter().sum::<f32>() <= budget {
                break;
            }
            if let Some(band) = wants.get_mut(i) {
                *band = 0.0;
            }
        }
        let [hdr_h, st_h, foot_h] = wants;

        let header = if hdr_h > 0.0 {
            Rect::new(0.0, 0.0, w, hdr_h)
        } else {
            Rect::EMPTY
        };
        let status = if st_h > 0.0 {
            Rect::new(0.0, hdr_h, w, st_h)
        } else {
            Rect::EMPTY
        };
        let footer = if foot_h > 0.0 {
            Rect::new(0.0, h - foot_h, w, foot_h)
        } else {
            Rect::EMPTY
        };

        // The pads stay square, so the side of one is whichever of the two
        // dimensions runs out first.
        let top = hdr_h + st_h;
        let bottom = h - foot_h;
        let avail_w = (w - pad * 2.0).max(0.0);
        let avail_h = (bottom - top - pad * 2.0).max(0.0);
        let step = (avail_w / PAD_COLS as f32)
            .min(avail_h / PAD_ROWS as f32)
            .max(0.0);
        let gw = step * PAD_COLS as f32;
        let gh = step * PAD_ROWS as f32;
        let grid = Rect::new((w - gw) / 2.0, top + pad + (avail_h - gh) / 2.0, gw, gh);

        let help_w = (w * 0.92).min(420.0);
        let help_h = (h * 0.92).min(300.0);
        let help = Rect::new((w - help_w) / 2.0, (h - help_h) / 2.0, help_w, help_h);

        Self {
            window: Rect::new(0.0, 0.0, w, h),
            header,
            status,
            grid,
            footer,
            help,
            step,
            font,
            small,
            pad,
        }
    }

    /// Whether a band survived the drop ladder and is worth drawing into.
    ///
    /// A band that did not fit is `Rect::EMPTY`, not a flat one.
    #[must_use]
    pub fn shows(&self, band: Rect) -> bool {
        band.w > 0.0 && band.h > 0.0
    }

    /// The `index`th of `count` evenly-spaced buttons filling `row`.
    ///
    /// There is no guard against an empty `row`: a band that did not fit is
    /// `Rect::EMPTY`, and the arithmetic turns that back into `Rect::EMPTY`
    /// unaided — a zero width leaves a zero gap and a zero button width, a zero
    /// height leaves a zero button height, and every offset is a multiple of
    /// those. A guard there would stand in front of a rule that already holds,
    /// which is a line no test can own (`known-issues.md` lesson 51). The range
    /// check is a different matter and does have to be made.
    fn nth_of(row: Rect, count: usize, index: usize) -> Rect {
        if index >= count {
            return Rect::EMPTY;
        }
        let n = count.max(1) as f32;
        let gap = (row.w * 0.012).min(8.0);
        let bw = ((row.w - gap * (n + 1.0)) / n).max(0.0);
        let bh = (row.h * 0.74).max(0.0);
        Rect::new(
            row.x + gap + index as f32 * (bw + gap),
            row.y + (row.h - bh) / 2.0,
            bw,
            bh,
        )
    }

    /// The footer buttons: new game, speed, help.
    #[must_use]
    pub fn footer_button(&self, index: usize) -> Rect {
        Self::nth_of(self.footer, 3, index)
    }

    /// One of the readouts at the right of the header: 0 is the best, 1 the
    /// score, 2 the round. Empty when the header did not survive, or when it is
    /// too narrow to hold the title and all three.
    ///
    /// There is no separate "did the header survive?" guard, and none is
    /// needed. A dropped header is `Rect::EMPTY`, so `right` is at or left of
    /// zero while `bw` is floored at 38 by its own clamp — every box then
    /// starts left of `header.x` and is refused by the check further down,
    /// which is the same refusal a header merely too narrow gets. A guard here
    /// would stand in front of a rule that already holds
    /// (`known-issues.md` lesson 51); this file had one, and deleting it
    /// changed no test.
    #[must_use]
    pub fn score_box(&self, index: usize) -> Rect {
        if index >= 3 {
            return Rect::EMPTY;
        }
        let bw = (self.header.w * 0.17).clamp(38.0, 100.0);
        let bh = (self.header.h * 0.7).max(1.0);
        let gap = self.pad;
        let right = self.header.right() - self.pad;
        // Laid out from the right edge inwards, so the box nearest the edge is
        // index 0 and adding a fourth would not move the other three.
        let x = right - (bw + gap) * (index as f32 + 1.0) + gap;
        if x < self.header.x {
            return Rect::EMPTY;
        }
        Rect::new(x, self.header.y + (self.header.h - bh) / 2.0, bw, bh)
    }

    /// One pad, by grid index. The gap is taken out of the cell rather than
    /// added to the step, so the last pad ends exactly at the grid's edge.
    #[must_use]
    pub fn pad_rect(&self, index: usize) -> Rect {
        if self.grid.is_empty() || index >= PAD_COLS * PAD_ROWS {
            return Rect::EMPTY;
        }
        let (row, col) = grid_pos(index);
        let gap = (self.step * 0.08).min(16.0);
        Rect::new(
            self.grid.x + col as f32 * self.step + gap / 2.0,
            self.grid.y + row as f32 * self.step + gap / 2.0,
            (self.step - gap).max(0.0),
            (self.step - gap).max(0.0),
        )
    }

    /// The panel shown when the game is over: centred on the grid, never wider
    /// than the window.
    #[must_use]
    pub fn game_over(&self) -> Rect {
        if self.grid.is_empty() {
            return Rect::EMPTY;
        }
        let bw = (self.grid.w * 0.94).min(self.window.w);
        let bh = (self.grid.h * 0.7).min(self.window.h);
        let (cx, cy) = self.grid.centre();
        Rect::new(cx - bw / 2.0, cy - bh / 2.0, bw, bh)
    }
}

// ── Drawing helpers ─────────────────────────────────────────────────────────

pub type Frame = guitk::frame::Frame<Target>;

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
        corner_radii: if radius > 0.0 {
            CornerRadii::all(radius)
        } else {
            CornerRadii::ZERO
        },
    });
}

fn stroke(f: &mut Frame, r: Rect, color: Color, width: f32, radius: f32) {
    if r.is_empty() || width <= 0.0 {
        return;
    }
    f.push(RenderCommand::StrokeRect {
        x: r.x,
        y: r.y,
        width: r.w,
        height: r.h,
        color,
        line_width: width,
        corner_radii: if radius > 0.0 {
            CornerRadii::all(radius)
        } else {
            CornerRadii::ZERO
        },
    });
}

/// A disc, as far as a renderer with rectangles and corner radii has one.
fn disc(f: &mut Frame, r: Rect, color: Color) {
    fill(f, r, color, r.w.min(r.h) / 2.0);
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
    // A width of nothing is not a narrow label, it is no label: elided to fit in
    // no space at all it would be an empty string sitting in the frame — a text
    // command that paints nothing and still counts as text drawn. The check
    // lives here, once, rather than at each call site.
    //
    // The floor under the size is the renderer's, not a taste in typography: a
    // request below a pixel is drawn a whole pixel high, *larger* than the band
    // it was sized to fit, so every caller that shrinks its type to fit would be
    // silently overruled (`known-issues.md` lesson 60). Refusing is the honest
    // answer — a window with no room for a legible line shows its pads and its
    // colours and no words.
    if body.is_empty() || size < MIN_DRAWN_FONT || max_width.is_some_and(|w| w <= 0.0) {
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

/// A line centred in a box, both ways, and never started outside it.
///
/// The offsets are clamped at zero in *both* directions. A line wider or taller
/// than its box would otherwise centre to a negative offset and begin above or
/// to the left of the box it is supposed to be inside — which for a box at the
/// top of the window means beginning off the window.
///
/// This is what replaced the pad labels' `x + BUTTON_SIZE / 2.0 - 20.0`: half
/// the width of the word "Green" at one font size, applied to every word at
/// every size.
fn centred(f: &mut Frame, r: Rect, body: &str, size: f32, color: Color, weight: FontWeightHint) {
    if r.is_empty() {
        return;
    }
    let tw = text::measure(body, size, weight);
    let th = text::line_height(size, weight);
    label(
        f,
        r.x + (r.w - tw).max(0.0) / 2.0,
        r.y + (r.h - th).max(0.0) / 2.0,
        body,
        size,
        color,
        weight,
        Some(r.w),
    );
}

/// A button: a filled box with a hit box on it and a centred label.
fn button(f: &mut Frame, r: Rect, target: Target, body: &str, size: f32, face: Color, ink: Color) {
    // No `r.is_empty()` guard, and none is needed: `fill` and `centred` return
    // on an empty box, and `Frame::hit` refuses to record one — so a button with
    // no box paints nothing and takes no clicks whichever way round it is
    // written. A guard here would be lesson 51 again.
    fill(f, r, face, (r.h * 0.22).min(8.0));
    // Recorded by the pass that paints it, so a button that moved took its hit
    // box with it and there is no second copy of the geometry to disagree.
    f.hit(target, r);
    centred(f, r, body, size, ink, FontWeightHint::Bold);
}

// ── The controls, written once ──────────────────────────────────────────────

const HELP_TITLE: &str = "Simon";

/// Every key the game answers to, with what it does — the text the help sheet
/// shows and the list the tests walk.
///
/// The hint under the game-over panel used to be a hand-written "Enter / R / 1-4
/// to restart" beside a handler that also took Space and Escape: a sentence and
/// a `match` that were free to disagree, and did. Both now come from here.
const HELP_ROWS: [(&str, &str); 6] = [
    ("1 - 4", "press that pad"),
    ("Arrows", "move the outline"),
    ("Enter / Space", "press the outlined pad"),
    ("S", "change the speed"),
    ("N", "start a new game"),
    ("H / Escape", "show or hide this"),
];

// ── The game ────────────────────────────────────────────────────────────────

pub struct Simon {
    /// The colours to repeat, in order. Its length is the round number.
    pub sequence: Vec<SimonColor>,
    pub state: GameState,
    pub speed: Speed,
    /// How much of the sequence the player has repeated correctly this round.
    pub player_index: usize,
    /// Rounds completed this game.
    pub score: u32,
    /// The best score of this session.
    ///
    /// Not of all time: nothing in this tree can save a preference yet, and the
    /// module documentation used to claim otherwise. See `known-issues.md`
    /// `TD-NO-APP-SETTINGS-STORE`.
    pub best: u32,
    /// Games that ended by a wrong pad — the only way a Simon game ends.
    ///
    /// A game abandoned with New Game is not counted. The number answers "how
    /// many times have I been beaten", and inflating it with restarts would make
    /// the stats line disagree with the player's own memory of the session.
    pub games_lost: u32,
    /// The pad lit by a press, and the milliseconds left on it.
    ///
    /// The one thing a press writes. Which pad is *lit* is not stored — see
    /// [`Simon::lit`], which answers it from the state machine, because four
    /// separate writers to a stored `lit_button` is what left the losing pad
    /// glowing under the game-over panel for the rest of the session.
    flash: Option<(SimonColor, u64)>,
    pub playback: Playback,
    /// Milliseconds into the pause before playback.
    pre_ms: u64,
    /// Milliseconds into the pause after a completed round.
    success_ms: u64,
    rng: SeededRng,
    /// Milliseconds since the window opened, for the pulse.
    clock_ms: u64,
    /// The pad the arrow keys have moved to.
    pub selected: usize,
    /// Whether the selection outline is shown.
    ///
    /// False until the player touches a pad by any route. A window that has
    /// just opened shows no outline, because an outline before the player has
    /// chosen anything points at a pad the game picked; after any press —
    /// keyboard *or* pointer — it is shown, because by then it is the honest
    /// answer to "where would Enter go", and a selection that has moved
    /// invisibly is worse than one that was never hidden.
    pub show_selection: bool,
    show_help: bool,
    width: f32,
    height: f32,
}

impl Simon {
    #[must_use]
    pub fn new() -> Self {
        // Was `with_seed(0xDEAD_BEEF_CAFE)`: every player, on every machine, got
        // the same sequence of colours in the same order. Predicting a Simon
        // sequence costs the user nothing but the game, so this asks the kernel
        // and falls back rather than refusing — see `randrange::seed_from_system`.
        Self::with_seed(seed_from_system(FALLBACK_SEED))
    }

    #[must_use]
    pub fn with_seed(seed: u64) -> Self {
        let mut game = Self {
            sequence: Vec::new(),
            state: GameState::PreSequence,
            speed: Speed::Medium,
            player_index: 0,
            score: 0,
            best: 0,
            games_lost: 0,
            flash: None,
            playback: Playback::new(),
            pre_ms: 0,
            success_ms: 0,
            rng: SeededRng::new(seed),
            clock_ms: 0,
            selected: 0,
            show_selection: false,
            show_help: false,
            width: WINDOW_WIDTH,
            height: WINDOW_HEIGHT,
        };
        game.deal_round();
        game
    }

    // ── What the game is, asked rather than remembered ──────────────────────

    /// The round number, which is the length of the sequence.
    ///
    /// Not a field. `round` used to be one, raised before a push that was itself
    /// conditional, so "which round is this" and "what is there to repeat" were
    /// two facts with nothing keeping them equal.
    #[must_use]
    pub fn round(&self) -> usize {
        self.sequence.len()
    }

    /// Which pad is lit, if any.
    ///
    /// The single answer to a question four different code paths used to write
    /// down. During playback it is whichever step is being shown, and only while
    /// the flash rather than the gap is running; at every other moment it is the
    /// pad the player pressed, for as long as its flash has left.
    #[must_use]
    pub fn lit(&self) -> Option<SimonColor> {
        match self.state {
            GameState::ShowSequence if self.playback.in_flash => {
                self.sequence.get(self.playback.step).copied()
            }
            // Not `_ if !in_flash` — the gap between two flashes is dark, and so
            // is the pause before the first one, whatever a leftover press
            // flash from the previous round might still say.
            GameState::ShowSequence | GameState::PreSequence => None,
            GameState::PlayerInput | GameState::RoundSuccess | GameState::GameOver => {
                self.flash.map(|(color, _)| color)
            }
        }
    }

    /// Whether the game-over panel is showing.
    ///
    /// After the losing pad has finished flashing, so the player sees *which*
    /// pad they got wrong before the panel covers the grid. One clock drives
    /// both: the panel appears exactly when the flash expires, rather than being
    /// gated on a second timer that had to be kept in step with it.
    #[must_use]
    pub fn game_over_shown(&self) -> bool {
        self.state == GameState::GameOver && self.flash.is_none()
    }

    #[must_use]
    pub fn help_is_open(&self) -> bool {
        self.show_help
    }

    /// Whether the clock runs at all.
    ///
    /// The help sheet pauses the game. Without this the sequence would play on
    /// behind a modal panel — and the moment a player is most likely to open
    /// the instructions is the moment they are least sure what to do, which is
    /// mid-round. Reading how to play would cost them the round they were
    /// reading about.
    ///
    /// One function rather than a condition in each of the two places that
    /// need it, so "is the game paused" cannot be answered one way by the
    /// timer request and the other way by the timer itself: a `tick_interval`
    /// of `None` with a `tick` that still advanced would freeze on a machine
    /// that honours the interval and run on one that also delivers a resize.
    fn clock_runs(&self) -> bool {
        !self.show_help
    }

    /// Whether anything on screen is moving, and so whether the clock is worth
    /// asking for. A game waiting on a person with nothing lit holds no timer,
    /// and the desktop is not kept awake by a window nobody is playing.
    #[must_use]
    pub fn wants_clock(&self) -> bool {
        self.clock_runs()
            && match self.state {
                GameState::PreSequence | GameState::ShowSequence | GameState::RoundSuccess => true,
                GameState::PlayerInput | GameState::GameOver => self.flash.is_some(),
            }
    }

    // ── Running the game ────────────────────────────────────────────────────

    /// Throw away the current game and start another, keeping the session's
    /// best and its count of losses.
    /// The flash is deliberately *not* cleared here. `deal_round` leaves the
    /// game in `PreSequence`, and `lit` returns nothing in that state whatever
    /// `self.flash` holds, so clearing it changed nothing any test or any
    /// player could see. The pad going out at a new game is derived, not
    /// assigned.
    pub fn new_game(&mut self) {
        self.sequence.clear();
        self.score = 0;
        self.deal_round();
    }

    /// Add a colour to the sequence and begin the pause before showing it.
    fn deal_round(&mut self) {
        // The bound and the lookup come from the same array. Written as
        // `below(4)` plus a match, the count of colours was stated twice, and a
        // fifth colour would have left the generator drawing from four while
        // every other part of the game knew about five.
        let index = self.rng.below(SimonColor::ALL.len());
        if let Some(&color) = SimonColor::ALL.get(index) {
            self.sequence.push(color);
        }
        self.player_index = 0;
        self.state = GameState::PreSequence;
        self.pre_ms = 0;
        self.success_ms = 0;
        self.playback = Playback::new();
    }

    /// Light `color` for `ms`, and note which pad it was.
    ///
    /// The one writer. `ms` is passed through unfloored. A `max(1)` used to
    /// stand here, with a comment claiming a `Some((c, 0))` would be "lit by
    /// [`lit`](Self::lit) and never cleared by [`age_flash`](Self::age_flash)".
    /// That was simply false -- `age_flash` clears a remainder of zero on the
    /// next tick exactly as it clears one that has just run out -- so the floor
    /// was a line no test could own, defended by an assertion nobody checked.
    fn flash_pad(&mut self, color: SimonColor, ms: u64) {
        self.flash = Some((color, ms));
    }

    /// Run the press flash down by `elapsed`, clearing it when it runs out.
    ///
    /// `Some(0)` clears rather than lingering: a remainder of zero is a flash
    /// with no time left on it, and keeping it would leave the pad lit until
    /// something else overwrote it. This is what makes the floor in
    /// [`flash_pad`](Self::flash_pad) unnecessary.
    fn age_flash(&mut self, elapsed: u64) {
        let Some((color, left)) = self.flash else {
            return;
        };
        self.flash = match left.checked_sub(elapsed) {
            Some(0) | None => None,
            Some(rest) => Some((color, rest)),
        };
    }

    /// Raise the score by one, and the session best with it.
    ///
    /// The only place either changes. `trigger_game_over` used to raise the best
    /// as well, which was dead work: a high-water mark that is raised at every
    /// rise is already at its mark when the game ends.
    fn score_round(&mut self) {
        self.score = self.score.saturating_add(1);
        self.best = self.best.max(self.score);
    }

    /// The player pressed `color`.
    fn press_colour(&mut self, color: SimonColor) {
        // `get` rather than an index. That `player_index` is inside the sequence
        // whenever the state is `PlayerInput` is an invariant kept by three
        // other methods between them, not a fact visible here; a press that
        // arrives when it does not hold should do nothing, not end the process.
        let Some(&expected) = self.sequence.get(self.player_index) else {
            return;
        };
        if color != expected {
            // The wrong pad stays lit longer than a right one, because it is the
            // only thing the player is being told: this is the one you meant.
            self.flash_pad(color, ERROR_FLASH_MS);
            self.state = GameState::GameOver;
            self.games_lost = self.games_lost.saturating_add(1);
            return;
        }
        self.flash_pad(color, PLAYER_FLASH_MS);
        self.player_index = self.player_index.saturating_add(1);
        if self.player_index >= self.sequence.len() {
            self.score_round();
            self.state = GameState::RoundSuccess;
            self.success_ms = 0;
        }
    }

    /// Press the pad at grid index `index`, moving the outline to it.
    fn press_pad(&mut self, index: usize) -> EventResult {
        let Some(color) = SimonColor::from_index(index) else {
            return EventResult::Ignored;
        };
        // The outline follows the pad that was pressed however it was pressed,
        // so a player who reaches for `3` and then wants the arrows finds them
        // starting where they left off rather than where they last looked.
        self.selected = index;
        self.show_selection = true;
        if self.state == GameState::PlayerInput {
            self.press_colour(color);
        }
        // Consumed either way: even out of turn, the outline moved.
        EventResult::Consumed
    }

    /// Move the selection outline, and say whether anything changed.
    fn move_selection(&mut self, dir: Dir) -> EventResult {
        let (row, col) = grid_pos(self.selected);
        let (dr, dc) = dir.step();
        let next = row
            .checked_add_signed(dr)
            .zip(col.checked_add_signed(dc))
            .and_then(|(r, c)| grid_index(r, c));
        // The first arrow key is not a no-op even when the pad it would move to
        // does not exist: it turns the outline on, which is a change the player
        // can see. Without this an arrow into the wall at the start of a game
        // would leave the window looking exactly as it did.
        let revealed = !self.show_selection;
        self.show_selection = true;
        match next {
            Some(index) => {
                self.selected = index;
                EventResult::Consumed
            }
            None if revealed => EventResult::Consumed,
            None => EventResult::Ignored,
        }
    }

    /// Change the speed.
    fn set_speed(&mut self, speed: Speed) -> EventResult {
        if speed == self.speed {
            return EventResult::Ignored;
        }
        self.speed = speed;
        // Restart the current phase. The time already spent was measured against
        // the *old* duration, and carrying it into a shorter one ends a flash
        // the player has not finished seeing and then runs the remainder
        // straight through the gap behind it — skipping a colour of the very
        // sequence they are being asked to memorise.
        self.playback.phase_ms = 0;
        EventResult::Consumed
    }

    /// Act on an intent, whichever route it arrived by.
    pub fn apply(&mut self, intent: Intent) -> EventResult {
        // The help sheet is modal: while it is up, the only thing anything can
        // ask for is to put it away. Written as one guard rather than as a
        // condition inside each arm, so a control added later is covered by
        // having been added.
        if self.show_help {
            return match intent {
                Intent::ToggleHelp | Intent::CloseHelp => {
                    self.show_help = false;
                    EventResult::Consumed
                }
                _ => EventResult::Ignored,
            };
        }
        match intent {
            Intent::Pad(index) => self.press_pad(index),
            Intent::Move(dir) => self.move_selection(dir),
            Intent::Confirm => {
                if self.state == GameState::GameOver {
                    self.new_game();
                    EventResult::Consumed
                } else {
                    self.press_pad(self.selected)
                }
            }
            Intent::CycleSpeed => self.set_speed(self.speed.next()),
            Intent::NewGame => {
                self.new_game();
                EventResult::Consumed
            }
            Intent::ToggleHelp => {
                self.show_help = true;
                EventResult::Consumed
            }
            // Nothing to close. Reported as ignored rather than consumed so the
            // window is not redrawn for a key that did nothing.
            Intent::CloseHelp => EventResult::Ignored,
        }
    }

    // ── The clock ───────────────────────────────────────────────────────────

    /// Advance everything by `elapsed` milliseconds.
    pub fn tick(&mut self, elapsed: u64) -> EventResult {
        if elapsed == 0 || !self.clock_runs() {
            return EventResult::Ignored;
        }
        self.clock_ms = self.clock_ms.wrapping_add(elapsed);
        self.age_flash(elapsed);
        match self.state {
            GameState::PreSequence => {
                self.pre_ms = self.pre_ms.saturating_add(elapsed);
                if let Some(over) = self.pre_ms.checked_sub(PRE_SEQUENCE_MS) {
                    // The overshoot goes into the first flash rather than on the
                    // floor. A tick is however long the compositor took, so
                    // dropping it made the first colour of every round outstay
                    // the others by a frame — more on a slower machine.
                    self.begin_playback(over);
                }
            }
            GameState::ShowSequence => self.advance_playback(elapsed),
            GameState::RoundSuccess => {
                self.success_ms = self.success_ms.saturating_add(elapsed);
                if self.success_ms >= SUCCESS_FLASH_MS {
                    self.deal_round();
                }
            }
            // Nothing automatic: one is waiting for a person and the other is
            // over. Both still needed the tick above, to run the flash down.
            GameState::PlayerInput | GameState::GameOver => {}
        }
        EventResult::Consumed
    }

    /// Start showing the sequence, `carried` milliseconds into the first flash.
    ///
    /// The playback is not reset here. `deal_round` is the only route into
    /// `PreSequence` and it already resets it, so a second reset one step later
    /// restated a fact rather than establishing one -- and a restatement is a
    /// line no test can own, because removing it changes nothing.
    fn begin_playback(&mut self, carried: u64) {
        self.state = GameState::ShowSequence;
        self.advance_playback(carried);
    }

    /// Move the playback on by `elapsed` milliseconds.
    ///
    /// Loops through as many flash/gap transitions as the elapsed time covers,
    /// so one large tick — a window that was not drawn for a second — completes
    /// as much of the sequence as it should rather than one step of it.
    fn advance_playback(&mut self, elapsed: u64) {
        let (flash_ms, gap_ms) = playback_ms(self.speed);
        // Floored at one so the loop below always makes progress. The constants
        // are all far above zero; the floor is against a future speed table, not
        // against this one.
        let (flash_ms, gap_ms) = (flash_ms.max(1), gap_ms.max(1));
        self.playback.phase_ms = self.playback.phase_ms.saturating_add(elapsed);

        loop {
            if self.playback.in_flash {
                let Some(rest) = self.playback.phase_ms.checked_sub(flash_ms) else {
                    return;
                };
                self.playback.in_flash = false;
                self.playback.phase_ms = rest;
            }
            let Some(rest) = self.playback.phase_ms.checked_sub(gap_ms) else {
                return;
            };
            self.playback.phase_ms = rest;
            self.playback.in_flash = true;
            self.playback.step = self.playback.step.saturating_add(1);
            // One lookup rather than a length test followed by an index: "is
            // there a next step" and "what is it" were the same question asked
            // twice, in two places free to drift apart.
            if self.sequence.get(self.playback.step).is_none() {
                // Only the state changes. `player_index` was zeroed by
                // `deal_round` and nothing between there and here advances it
                // -- the player has not been allowed to press yet -- and the
                // flash is what playback was *using* to light the pads, which
                // `lit` stops consulting the moment the state leaves
                // `ShowSequence`. Both assignments restated what already held.
                self.state = GameState::PlayerInput;
                return;
            }
        }
    }

    // ── Input ───────────────────────────────────────────────────────────────

    /// Remembers the size the window is now, which is the size the next click
    /// will be read against.
    pub fn resize(&mut self, width: f32, height: f32) {
        self.width = width;
        self.height = height;
    }

    pub fn handle_key(&mut self, ev: &KeyEvent) -> EventResult {
        match key_intent(ev) {
            Some(intent) => self.apply(intent),
            None => EventResult::Ignored,
        }
    }

    pub fn handle_mouse(&mut self, ev: &MouseEvent) -> EventResult {
        if !matches!(ev.kind, MouseEventKind::Press(MouseButton::Left)) {
            return EventResult::Ignored;
        }
        // Hit-tested against a frame drawn at the size the last one was drawn
        // at, so a click is read against the picture the player is looking at.
        let frame = self.frame(self.width, self.height);
        match frame.hit_test(ev.x, ev.y) {
            Some(target) => self.apply(target_intent(target)),
            None => EventResult::Ignored,
        }
    }

    // ── Drawing ─────────────────────────────────────────────────────────────

    /// The line the status band shows.
    fn status_line(&self) -> String {
        match self.state {
            GameState::PreSequence => "Get ready...".to_string(),
            GameState::ShowSequence => format!(
                "Watch  {}/{}",
                self.playback.step.saturating_add(1).min(self.round()),
                self.round()
            ),
            GameState::PlayerInput => format!(
                "Your turn  {}/{}",
                self.player_index.saturating_add(1).min(self.round()),
                self.round()
            ),
            GameState::RoundSuccess => format!("Round {} complete", self.round()),
            GameState::GameOver => "Game over".to_string(),
        }
    }

    fn status_colour(&self) -> Color {
        match self.state {
            GameState::PreSequence => COL_SUBTEXT0,
            GameState::ShowSequence => COL_MAUVE,
            GameState::PlayerInput => COL_TEAL,
            GameState::RoundSuccess => COL_GREEN,
            GameState::GameOver => COL_RED,
        }
    }

    /// The whole window, as commands and the hit boxes the same pass recorded.
    #[must_use]
    pub fn frame(&self, width: f32, height: f32) -> Frame {
        let l = Layout::new(width, height);
        let mut f = Frame::new(l.window.w, l.window.h);
        fill(&mut f, l.window, COL_BASE, 0.0);
        self.draw_header(&mut f, &l);
        self.draw_status(&mut f, &l);
        self.draw_pads(&mut f, &l);
        self.draw_footer(&mut f, &l);
        if self.game_over_shown() {
            self.draw_game_over(&mut f, &l);
        }
        if self.show_help {
            self.draw_help(&mut f, &l);
        }
        f
    }

    fn draw_header(&self, f: &mut Frame, l: &Layout) {
        // No "did the header fit?" guard. A band that did not fit is
        // `Rect::EMPTY`, and every call below already refuses one: `fill` and
        // `centred` return on an empty box, and `score_box` returns empty boxes
        // of its own when the header is empty.
        fill(f, l.header, COL_MANTLE, 0.0);
        let title = (l.font * 1.3).min(l.header.h * 0.55);
        // Never over the readouts: the title is given the room to their left and
        // elides into it rather than running underneath them.
        let limit = (self.readouts_left(l) - l.pad * 2.0).max(0.0);
        label(
            f,
            l.header.x + l.pad,
            l.header.y + (l.header.h - text::line_height(title, FontWeightHint::Bold)) / 2.0,
            HELP_TITLE.to_uppercase().as_str(),
            title,
            COL_LAVENDER,
            FontWeightHint::Bold,
            Some(limit),
        );
        self.draw_readout(f, l, 0, "BEST", self.best, COL_YELLOW);
        self.draw_readout(f, l, 1, "SCORE", self.score, COL_GREEN);
        self.draw_readout(f, l, 2, "ROUND", self.round() as u32, COL_TEAL);
    }

    /// The left edge of the leftmost readout that is actually drawn, or the
    /// header's right edge when none is.
    ///
    /// Read off the boxes rather than recomputed, because the boxes are dropped
    /// one at a time as the header narrows and a title sized against a box that
    /// was not drawn is a title with a margin against nothing.
    fn readouts_left(&self, l: &Layout) -> f32 {
        (0..3)
            .map(|i| l.score_box(i))
            .filter(|r| !r.is_empty())
            .map(|r| r.x)
            .fold(l.header.right(), f32::min)
    }

    fn draw_readout(
        &self,
        f: &mut Frame,
        l: &Layout,
        index: usize,
        name: &str,
        value: u32,
        ink: Color,
    ) {
        let box_rect = l.score_box(index);
        fill(f, box_rect, COL_SURFACE0, (box_rect.h * 0.2).min(6.0));
        let cap = (l.small).min(box_rect.h * 0.34);
        let num = (l.font).min(box_rect.h * 0.46);
        let cap_h = text::line_height(cap, FontWeightHint::Regular);
        let num_h = text::line_height(num, FontWeightHint::Bold);
        let top = box_rect.y + (box_rect.h - cap_h - num_h).max(0.0) / 2.0;
        centred(
            f,
            Rect::new(box_rect.x, top, box_rect.w, cap_h),
            name,
            cap,
            COL_SUBTEXT0,
            FontWeightHint::Regular,
        );
        centred(
            f,
            Rect::new(box_rect.x, top + cap_h, box_rect.w, num_h),
            &value.to_string(),
            num,
            ink,
            FontWeightHint::Bold,
        );
    }

    fn draw_status(&self, f: &mut Frame, l: &Layout) {
        if !l.shows(l.status) {
            // Not a redundant guard: the dot and the tone below are positioned
            // from the band's own height, and a band of no height would put them
            // at the top-left corner of the window rather than nowhere.
            return;
        }
        let size = (l.font).min(l.status.h * 0.62);
        let dot_side = (l.status.h * 0.34).min(l.small);
        let lit = self.lit();

        // The pulse: a disc that grows and shrinks once per `PULSE_PERIOD_MS`.
        // Derived from accumulated milliseconds, not from a count of ticks, so
        // it takes the same time on every machine — the old `pulse_counter % 8`
        // ran at whatever rate the compositor happened to be drawing at.
        let phase = (self.clock_ms % PULSE_PERIOD_MS) as f32 / PULSE_PERIOD_MS as f32;
        let grow = 1.0 + 0.35 * (1.0 - (phase * 2.0 - 1.0).abs());
        let side = if lit.is_some() {
            dot_side * grow
        } else {
            dot_side
        };
        let dot = Rect::new(
            l.status.x + l.pad,
            l.status.y + (l.status.h - side) / 2.0,
            side,
            side,
        );
        disc(f, dot, lit.map_or(COL_SURFACE1, SimonColor::lit));

        let text_x = dot.right() + l.pad;
        // The tone label is the sound this machine cannot make. It is only there
        // while a pad is lit, so the room it takes is given back to the status
        // line the rest of the time.
        let tone = lit.map(SimonColor::tone);
        let tone_w = tone.map_or(0.0, |t| {
            text::measure(t, l.small, FontWeightHint::Bold) + l.pad
        });
        let line_w = (l.status.right() - l.pad - tone_w - text_x).max(0.0);
        label(
            f,
            text_x,
            l.status.y + (l.status.h - text::line_height(size, FontWeightHint::Bold)) / 2.0,
            &self.status_line(),
            size,
            self.status_colour(),
            FontWeightHint::Bold,
            Some(line_w),
        );
        if let (Some(tone), Some(colour)) = (tone, lit) {
            let w = text::measure(tone, l.small, FontWeightHint::Bold);
            label(
                f,
                l.status.right() - l.pad - w,
                l.status.y + (l.status.h - text::line_height(l.small, FontWeightHint::Bold)) / 2.0,
                tone,
                l.small,
                colour.lit(),
                FontWeightHint::Bold,
                Some(w),
            );
        }
    }

    fn draw_pads(&self, f: &mut Frame, l: &Layout) {
        let lit = self.lit();
        for &colour in &SimonColor::ALL {
            let index = colour.index();
            let r = l.pad_rect(index);
            let is_lit = lit == Some(colour);
            let radius = (r.w.min(r.h) * 0.1).min(16.0);

            // The halo goes *behind* the pad, which means it is pushed first.
            // It used to be pushed after, so a 60-alpha wash of the pad's own
            // colour covered the pad's face and its label rather than ringing
            // it. Grown proportionally, so it cannot reach outside a small
            // window the way a flat `- 4.0` did.
            //
            // Not clipped to the window, and it does not need to be: the halo
            // cannot leave the pad's own grid cell, because `grow` is never
            // more than half the gap `pad_rect` insets the pad by. Below a
            // 200-px step the gap is `step * 0.08`, so `grow` is at most
            // `step * 0.92 * 0.04 = step * 0.0368` against a half-gap of
            // `step * 0.04`; above it the gap is a flat 16 and `grow` is capped
            // at 6 against a half-gap of 8. The cells tile the grid and the grid
            // is inside the window, so the clip could never remove a pixel. The
            // sweep in `the_glow_never_reaches_outside_the_window` is what
            // holds this, rather than this comment.
            if is_lit {
                let grow = (r.w.min(r.h) * 0.04).min(6.0);
                let halo = Rect::new(r.x - grow, r.y - grow, r.w + grow * 2.0, r.h + grow * 2.0);
                let c = colour.lit();
                fill(f, halo, Color::rgba(c.r, c.g, c.b, 60), radius + grow);
            }

            fill(
                f,
                r,
                if is_lit { colour.lit() } else { colour.dim() },
                radius,
            );
            f.hit(Target::Pad(colour), r);

            if self.show_selection && index == self.selected {
                stroke(f, r, COL_TEXT, (r.w * 0.012).clamp(1.0, 3.0), radius);
            }

            let ink = if is_lit {
                COL_CRUST
            } else {
                let c = colour.lit();
                Color::rgba(c.r, c.g, c.b, 140)
            };
            centred(
                f,
                r,
                colour.label(),
                (l.font).min(r.h * 0.22),
                ink,
                FontWeightHint::Bold,
            );
            // The number that presses this pad, in its corner. Written from the
            // index rather than from a second list, so a pad that moves in the
            // grid takes its number with it.
            let inset = (r.w * 0.06).min(10.0);
            label(
                f,
                r.x + inset,
                r.y + inset,
                &index.saturating_add(1).to_string(),
                (l.small).min(r.h * 0.16),
                ink,
                FontWeightHint::Regular,
                Some((r.w - inset * 2.0).max(0.0)),
            );
        }
    }

    fn draw_footer(&self, f: &mut Frame, l: &Layout) {
        fill(f, l.footer, COL_MANTLE, 0.0);
        let size = (l.small).min(l.footer_button(0).h * 0.42);
        button(
            f,
            l.footer_button(0),
            Target::NewGame,
            "New game",
            size,
            COL_SURFACE0,
            COL_TEXT,
        );
        button(
            f,
            l.footer_button(1),
            Target::Speed,
            // The speed is on the control that changes it, so there is one place
            // in the window that knows what it is.
            &format!("Speed: {}", self.speed.label()),
            size,
            COL_SURFACE0,
            COL_PEACH,
        );
        button(
            f,
            l.footer_button(2),
            Target::Help,
            "Help",
            size,
            COL_SURFACE0,
            COL_TEXT,
        );
    }

    fn draw_game_over(&self, f: &mut Frame, l: &Layout) {
        let panel = l.game_over();
        if panel.is_empty() {
            return;
        }
        // A wash over the grid first, so the pads behind the panel are plainly
        // out of play rather than merely partly covered.
        fill(f, l.grid, Color::rgba(17, 17, 27, 200), 0.0);
        fill(f, panel, COL_SURFACE0, (panel.h * 0.06).min(12.0));
        stroke(f, panel, COL_RED, 2.0, (panel.h * 0.06).min(12.0));
        // The whole panel takes the click, and it is recorded after the pads, so
        // `hit_test` — which reads the last box first — gives it the click even
        // though the pads are underneath.
        f.hit(Target::GameOver, panel);

        let rows: [(String, Color, FontWeightHint); 5] = [
            ("GAME OVER".to_string(), COL_RED, FontWeightHint::Bold),
            (
                format!("Score: {} rounds", self.score),
                COL_TEXT,
                FontWeightHint::Regular,
            ),
            (
                format!("Best: {} rounds", self.best),
                COL_YELLOW,
                FontWeightHint::Regular,
            ),
            (
                format!("Games lost: {}", self.games_lost),
                COL_SUBTEXT0,
                FontWeightHint::Regular,
            ),
            (
                "Click here, or Enter, to play again".to_string(),
                COL_OVERLAY0,
                FontWeightHint::Regular,
            ),
        ];
        let n = rows.len() as f32;
        let row_h = panel.h / n;
        for (i, (body, ink, weight)) in rows.iter().enumerate() {
            let r = Rect::new(panel.x, panel.y + i as f32 * row_h, panel.w, row_h);
            let size = if i == 0 {
                (l.font * 1.2).min(row_h * 0.7)
            } else {
                (l.small).min(row_h * 0.6)
            };
            centred(f, r, body, size, *ink, *weight);
        }
    }

    fn draw_help(&self, f: &mut Frame, l: &Layout) {
        // The sheet takes every click that lands on it, and it is drawn last, so
        // nothing behind it can be reached while it is up.
        fill(f, l.window, Color::rgba(17, 17, 27, 190), 0.0);
        f.hit(Target::HelpSheet, l.window);
        fill(f, l.help, COL_SURFACE0, (l.help.h * 0.05).min(12.0));
        stroke(f, l.help, COL_SURFACE1, 1.0, (l.help.h * 0.05).min(12.0));

        let rows = HELP_ROWS.len() as f32 + 2.0;
        let row_h = l.help.h / rows;
        let size = (l.small).min(row_h * 0.6);
        centred(
            f,
            Rect::new(l.help.x, l.help.y, l.help.w, row_h),
            HELP_TITLE,
            (l.font * 1.1).min(row_h * 0.7),
            COL_LAVENDER,
            FontWeightHint::Bold,
        );
        let key_w = l.help.w * 0.42;
        for (i, (key, what)) in HELP_ROWS.iter().enumerate() {
            let y = l.help.y + (i as f32 + 1.0) * row_h;
            label(
                f,
                l.help.x + l.pad * 2.0,
                y + (row_h - text::line_height(size, FontWeightHint::Bold)) / 2.0,
                key,
                size,
                COL_PEACH,
                FontWeightHint::Bold,
                Some((key_w - l.pad * 2.0).max(0.0)),
            );
            label(
                f,
                l.help.x + key_w,
                y + (row_h - text::line_height(size, FontWeightHint::Regular)) / 2.0,
                what,
                size,
                COL_TEXT,
                FontWeightHint::Regular,
                Some((l.help.right() - l.pad * 2.0 - (l.help.x + key_w)).max(0.0)),
            );
        }
        centred(
            f,
            Rect::new(l.help.x, l.help.bottom() - row_h, l.help.w, row_h),
            "Click anywhere to close",
            size,
            COL_OVERLAY0,
            FontWeightHint::Regular,
        );
    }
}

impl Default for Simon {
    fn default() -> Self {
        Self::new()
    }
}

/// The digit keys name pads one to four, so there have to be four of them. A
/// compile-time check rather than a comment: a grid with another pad would
/// otherwise leave a pad no digit reaches, silently.
const _: () = assert!(
    SimonColor::ALL.len() == 4,
    "the digit keys 1-4 name every pad"
);

/// What a key asks for, if anything.
///
/// A free function rather than a method, so the mapping can be read and tested
/// without a game to read it against.
#[must_use]
pub fn key_intent(ev: &KeyEvent) -> Option<Intent> {
    // Presses only. The compositor sends a `KeyEvent` with `pressed: false` for
    // every key coming back up, so a handler that reads only `key` runs every
    // action twice — here, two pads pressed for one keystroke, the second of
    // them wrong. See `known-issues.md` lesson 63 and
    // `scripts/check-key-release-wiring.py`, which is what now keeps this line
    // in place across the tree.
    if !ev.pressed {
        return None;
    }
    // Ctrl and Alt combinations belong to the window, not to the game: a Ctrl+S
    // that changes the speed is a Ctrl+S the desktop cannot have.
    if ev.modifiers.ctrl || ev.modifiers.alt {
        return None;
    }
    match ev.key {
        Key::Up => Some(Intent::Move(Dir::Up)),
        Key::Down => Some(Intent::Move(Dir::Down)),
        Key::Left => Some(Intent::Move(Dir::Left)),
        Key::Right => Some(Intent::Move(Dir::Right)),
        Key::Enter | Key::Space => Some(Intent::Confirm),
        Key::Num1 => Some(Intent::Pad(0)),
        Key::Num2 => Some(Intent::Pad(1)),
        Key::Num3 => Some(Intent::Pad(2)),
        Key::Num4 => Some(Intent::Pad(3)),
        Key::S => Some(Intent::CycleSpeed),
        Key::N => Some(Intent::NewGame),
        Key::H => Some(Intent::ToggleHelp),
        Key::Escape => Some(Intent::CloseHelp),
        _ => None,
    }
}

/// What clicking a control asks for.
#[must_use]
pub fn target_intent(target: Target) -> Intent {
    match target {
        Target::Pad(colour) => Intent::Pad(colour.index()),
        Target::NewGame => Intent::NewGame,
        Target::Speed => Intent::CycleSpeed,
        Target::Help => Intent::ToggleHelp,
        Target::HelpSheet => Intent::CloseHelp,
        // The panel says "click here to play again", and this is the line that
        // makes that true.
        Target::GameOver => Intent::Confirm,
    }
}

pub fn handle_event(game: &mut Simon, event: &Event) -> EventResult {
    match event {
        Event::Key(ev) => game.handle_key(ev),
        Event::Mouse(ev) => game.handle_mouse(ev),
        Event::Resize { width, height } => {
            game.resize(*width as f32, *height as f32);
            EventResult::Consumed
        }
        Event::Tick { elapsed_ms } => game.tick(*elapsed_ms),
        _ => EventResult::Ignored,
    }
}

impl App for Simon {
    fn title(&self) -> String {
        "Simon".to_string()
    }

    fn app_id(&self) -> String {
        "simon".to_string()
    }

    fn initial_size(&self) -> (u32, u32) {
        (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)
    }

    /// A clock only while something is moving.
    ///
    /// Consulted after every event, so it starts when a round is dealt and stops
    /// when the last flash of a lost game runs out. A game waiting on a person
    /// with nothing lit holds no timer.
    fn tick_interval(&self) -> Option<Duration> {
        self.wants_clock().then_some(TICK)
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

impl Probe for Simon {
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
    let mut game = Simon::new();
    app::launch("simon", &mut game)
}

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
    use guitk::event::Modifiers;
    use guitk::probe;

    /// Windows to check the layout against, from a desktop down to something no
    /// sane person would resize to.
    ///
    /// The small end is not decoration. Each of the last five makes a rule bind
    /// that binds nowhere else in the list:
    ///
    /// - `(110, 800)` is tall enough for every band and too narrow for the third
    ///   readout, which is the only condition under which `score_box`'s "ran off
    ///   the left edge" refusal fires.
    /// - `(600, 150)` and `(300, 110)` are short enough that the drop ladder
    ///   runs, which is the only condition under which `PAD_SHARE` and
    ///   `BAND_DROP_ORDER` have any effect at all.
    /// - `(24, 24)` drops every band, so the grid is drawn with no chrome above
    ///   or below it and the pads have the window to themselves.
    /// - `(4, 4)` is here for the padding and the font floor: at every other
    ///   size the padding lands on its 2px floor, so the clamp's upper bound --
    ///   a quarter of the smaller side, which stops the padding eating the thing
    ///   it pads -- never binds; and it is the only size at which every label in
    ///   the window is driven below one pixel and refused.
    const WINDOWS: [(f32, f32); 12] = [
        (1920.0, 1080.0),
        (1280.0, 800.0),
        (WINDOW_WIDTH, WINDOW_HEIGHT),
        (480.0, 400.0),
        (400.0, 640.0),
        (110.0, 800.0),
        (600.0, 150.0),
        (640.0, 200.0),
        (300.0, 110.0),
        (90.0, 120.0),
        (24.0, 24.0),
        (4.0, 4.0),
    ];

    // ── Fixtures ────────────────────────────────────────────────────────────

    /// A game with a fixed seed, so a failure is the same failure twice.
    fn game() -> Simon {
        Simon::with_seed(0x1234_5678_9ABC_DEF0)
    }

    /// A game whose stored window is `(width, height)`, which is the size a
    /// click on it will be read against.
    fn windowed(width: f32, height: f32) -> Simon {
        let mut app = game();
        app.resize(width, height);
        app
    }

    /// Drive the clock until the game is waiting for the player.
    ///
    /// One long tick rather than a hand-written walk of flashes and gaps:
    /// `advance_playback` loops through as many phases as the elapsed time
    /// covers, and a helper that stepped the sequence itself would be a slower
    /// copy of the code every test using it is meant to be checking.
    fn to_player_input(app: &mut Simon) {
        app.tick(1_000_000);
        assert_eq!(
            app.state,
            GameState::PlayerInput,
            "the sequence did not hand over"
        );
    }

    /// Press the rest of the sequence correctly, from wherever the player is.
    fn play_sequence(app: &mut Simon) {
        let rest: Vec<SimonColor> = app.sequence[app.player_index..].to_vec();
        for colour in rest {
            app.apply(Intent::Pad(colour.index()));
        }
    }

    /// Complete one round: watch the sequence, repeat it, and wait out the
    /// celebration. Leaves the game in the pause before the next round, which
    /// is the only moment at which the sequence is complete and nothing has
    /// been shown of it yet.
    fn complete_round(app: &mut Simon) {
        to_player_input(app);
        play_sequence(app);
        assert_eq!(app.state, GameState::RoundSuccess, "the round did not end");
        app.tick(SUCCESS_FLASH_MS);
        assert_eq!(
            app.state,
            GameState::PreSequence,
            "the next round was not dealt"
        );
    }

    /// Complete `rounds` rounds and arrive at the player's turn in the one
    /// after.
    fn advance_rounds(app: &mut Simon, rounds: usize) {
        for _ in 0..rounds {
            complete_round(app);
        }
        to_player_input(app);
    }

    /// The colour that is not the one the game is waiting for.
    fn a_wrong_colour(app: &Simon) -> SimonColor {
        let expected = app.sequence[app.player_index];
        SimonColor::ALL
            .iter()
            .find(|&&c| c != expected)
            .copied()
            .expect("four colours, so one of them is not the expected one")
    }

    /// Lose the current game by pressing a pad that is not the one expected.
    fn lose(app: &mut Simon) {
        to_player_input(app);
        let wrong = a_wrong_colour(app);
        app.apply(Intent::Pad(wrong.index()));
        assert_eq!(app.state, GameState::GameOver, "the wrong pad was accepted");
    }

    fn press(app: &mut Simon, key: Key) -> EventResult {
        handle_event(app, &Event::Key(probe::press(key)))
    }

    fn click(app: &mut Simon, x: f32, y: f32) -> EventResult {
        let size = (app.width, app.height);
        app.click_at(x, y, MouseButton::Left, size)
    }

    /// Click the middle of the named control, at the app's current size.
    fn click_on(app: &mut Simon, target: Target) -> EventResult {
        let size = (app.width, app.height);
        probe::click_sized(app, target, MouseButton::Left, size)
    }

    fn texts(f: &Frame) -> Vec<String> {
        f.commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    /// Every line in the frame with the box it will actually cover.
    ///
    /// The width is the one that reaches the screen, not the one the whole
    /// string measures to: a label carries a maximum width and is elided to fit
    /// it, so measuring the body would report an overflow that is never painted
    /// -- and would make every one of these tests a test of the string rather
    /// than of the layout.
    fn text_boxes(f: &Frame) -> Vec<(String, Rect, f32)> {
        f.commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text {
                    text,
                    x,
                    y,
                    font_size,
                    font_weight,
                    max_width,
                    ..
                } => {
                    let full = text::measure(text, *font_size, *font_weight);
                    let w = max_width.map_or(full, |m| full.min(m));
                    Some((
                        text.clone(),
                        Rect::new(*x, *y, w, text::line_height(*font_size, *font_weight)),
                        *font_size,
                    ))
                }
                _ => None,
            })
            .collect()
    }

    /// Every filled box in the frame, in the order they were painted.
    fn fills(f: &Frame) -> Vec<(Rect, Color)> {
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

    /// The colour the frame last painted the given box, read out of the
    /// commands rather than out of the function that chose it.
    fn fill_at(f: &Frame, r: Rect) -> Option<Color> {
        fills(f)
            .into_iter()
            .rev()
            .find(|(b, _)| same(*b, r))
            .map(|(_, c)| c)
    }

    /// Where in the paint order the given box was filled, if it was.
    fn fill_order(f: &Frame, r: Rect) -> Option<usize> {
        fills(f).into_iter().position(|(b, _)| same(b, r))
    }

    /// Every outlined box in the frame, in the order they were painted.
    fn strokes(f: &Frame) -> Vec<(Rect, Color)> {
        f.commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::StrokeRect {
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

    /// Whether the frame draws an outline round exactly the given box.
    fn outlined(f: &Frame, r: Rect) -> bool {
        strokes(f).iter().any(|&(b, _)| same(b, r))
    }

    fn hits_for(f: &Frame, target: Target) -> Vec<Rect> {
        f.hits()
            .iter()
            .filter(|(t, _)| *t == target)
            .map(|&(_, r)| r)
            .collect()
    }

    fn same(a: Rect, b: Rect) -> bool {
        (a.x - b.x).abs() < 0.01
            && (a.y - b.y).abs() < 0.01
            && (a.w - b.w).abs() < 0.01
            && (a.h - b.h).abs() < 0.01
    }

    /// Whether two boxes share any area. Touching edges do not count.
    fn overlaps(a: Rect, b: Rect) -> bool {
        a.intersect(b).is_some_and(|r| r.w > 0.01 && r.h > 0.01)
    }

    fn inside(inner: Rect, outer: Rect) -> bool {
        inner.x >= outer.x - 0.01
            && inner.y >= outer.y - 0.01
            && inner.right() <= outer.right() + 0.01
            && inner.bottom() <= outer.bottom() + 0.01
    }

    // ── The grid and the four colours ───────────────────────────────────────

    #[test]
    fn every_colour_has_a_pad_and_every_pad_a_colour() {
        assert_eq!(SimonColor::ALL.len(), PAD_COLS * PAD_ROWS);
    }

    #[test]
    fn a_colours_index_and_the_colour_at_that_index_are_inverses() {
        for &colour in &SimonColor::ALL {
            assert_eq!(
                SimonColor::from_index(colour.index()),
                Some(colour),
                "{colour:?} does not come back from its own index"
            );
        }
        for i in 0..SimonColor::ALL.len() {
            assert_eq!(
                SimonColor::from_index(i).map(SimonColor::index),
                Some(i),
                "index {i} does not survive the round trip"
            );
        }
    }

    #[test]
    fn from_index_stops_at_the_last_pad() {
        assert_eq!(SimonColor::from_index(SimonColor::ALL.len()), None);
        assert_eq!(SimonColor::from_index(usize::MAX), None);
    }

    #[test]
    fn grid_positions_and_indices_are_inverses() {
        for i in 0..PAD_COLS * PAD_ROWS {
            let (row, col) = grid_pos(i);
            assert_eq!(grid_index(row, col), Some(i), "index {i} at ({row}, {col})");
        }
    }

    #[test]
    fn grid_index_refuses_a_position_off_the_grid() {
        assert_eq!(grid_index(PAD_ROWS, 0), None, "a row past the bottom");
        assert_eq!(grid_index(0, PAD_COLS), None, "a column past the right");
        assert_eq!(grid_index(usize::MAX, usize::MAX), None);
    }

    #[test]
    fn the_pads_are_numbered_left_to_right_then_top_to_bottom() {
        // The grid's reading order, stated once here so that a change to
        // `grid_pos` has to be a deliberate change to this line too. Every
        // other test that talks about position leans on this being the order.
        assert_eq!(grid_pos(0), (0, 0));
        assert_eq!(grid_pos(1), (0, 1));
        assert_eq!(grid_pos(2), (1, 0));
        assert_eq!(grid_pos(3), (1, 1));
    }

    #[test]
    fn no_two_colours_share_a_label_a_tone_or_a_shade() {
        for (i, &a) in SimonColor::ALL.iter().enumerate() {
            for &b in &SimonColor::ALL[i + 1..] {
                assert_ne!(a.label(), b.label(), "{a:?} and {b:?} read the same");
                assert_ne!(a.tone(), b.tone(), "{a:?} and {b:?} sound the same");
                assert_ne!(a.lit(), b.lit(), "{a:?} and {b:?} light the same");
                assert_ne!(a.dim(), b.dim(), "{a:?} and {b:?} rest the same");
            }
        }
    }

    #[test]
    fn a_lit_pad_is_brighter_than_a_dim_one() {
        // Otherwise "lit" is a word in the source and nothing on the screen.
        for &colour in &SimonColor::ALL {
            let (lit, dim) = (colour.lit(), colour.dim());
            let brightness = |c: Color| u32::from(c.r) + u32::from(c.g) + u32::from(c.b);
            assert!(
                brightness(lit) > brightness(dim),
                "{colour:?} is no brighter lit than dark"
            );
        }
    }

    // ── Speed ───────────────────────────────────────────────────────────────

    #[test]
    fn the_speed_control_reaches_every_speed_and_comes_back() {
        let mut seen = vec![Speed::Slow];
        let mut speed = Speed::Slow;
        for _ in 0..3 {
            speed = speed.next();
            seen.push(speed);
        }
        assert_eq!(speed, Speed::Slow, "the cycle does not close");
        assert!(seen.contains(&Speed::Medium) && seen.contains(&Speed::Fast));
    }

    #[test]
    fn a_faster_speed_is_faster_in_both_halves() {
        // The flash and the gap move together. A "faster" speed that shortened
        // only the flash would leave the sequence taking almost as long while
        // showing each colour for half the time -- harder to read and no
        // quicker to sit through.
        let (slow_f, slow_g) = playback_ms(Speed::Slow);
        let (med_f, med_g) = playback_ms(Speed::Medium);
        let (fast_f, fast_g) = playback_ms(Speed::Fast);
        assert!(
            slow_f > med_f && med_f > fast_f,
            "the flashes do not shorten"
        );
        assert!(slow_g > med_g && med_g > fast_g, "the gaps do not shorten");
        assert!(fast_f > 0 && fast_g > 0, "a speed with no time in it");
    }

    #[test]
    fn no_two_speeds_share_a_label() {
        let labels = [
            Speed::Slow.label(),
            Speed::Medium.label(),
            Speed::Fast.label(),
        ];
        for (i, a) in labels.iter().enumerate() {
            for b in &labels[i + 1..] {
                assert_ne!(a, b, "two speeds read the same");
            }
        }
    }

    // ── The layout, at every size ───────────────────────────────────────────

    #[test]
    fn the_bands_never_overlap_one_another() {
        for (w, h) in WINDOWS {
            let l = Layout::new(w, h);
            let bands = [
                ("header", l.header),
                ("status", l.status),
                ("footer", l.footer),
            ];
            for (i, (an, a)) in bands.iter().enumerate() {
                for (bn, b) in &bands[i + 1..] {
                    assert!(
                        !overlaps(*a, *b),
                        "{an} {a:?} lies over {bn} {b:?} at {w}x{h}"
                    );
                }
            }
        }
    }

    #[test]
    fn every_band_stays_inside_the_window() {
        for (w, h) in WINDOWS {
            let l = Layout::new(w, h);
            for (name, band) in [
                ("header", l.header),
                ("status", l.status),
                ("footer", l.footer),
                ("grid", l.grid),
                ("help", l.help),
                ("game over", l.game_over()),
            ] {
                if band.is_empty() {
                    continue;
                }
                assert!(
                    inside(band, l.window),
                    "{name} {band:?} leaves the window at {w}x{h}"
                );
            }
        }
    }

    #[test]
    fn the_grid_sits_between_the_bands_that_survived() {
        for (w, h) in WINDOWS {
            let l = Layout::new(w, h);
            let top = l.header.bottom().max(l.status.bottom());
            let bottom = if l.footer.is_empty() { h } else { l.footer.y };
            assert!(
                l.grid.y >= top - 0.01,
                "the grid at {:?} runs under the bands ending at {top} at {w}x{h}",
                l.grid
            );
            assert!(
                l.grid.bottom() <= bottom + 0.01,
                "the grid at {:?} runs under the footer at {bottom} at {w}x{h}",
                l.grid
            );
        }
    }

    #[test]
    fn the_pads_are_never_given_up() {
        // The bands are droppable; the pads are the game. A window with room
        // for nothing else still shows four pads, because a Simon with no pads
        // is not a smaller Simon, it is a blank rectangle.
        for (w, h) in WINDOWS {
            let l = Layout::new(w, h);
            assert!(!l.grid.is_empty(), "no grid at {w}x{h}");
            for i in 0..SimonColor::ALL.len() {
                assert!(!l.pad_rect(i).is_empty(), "no pad {i} at {w}x{h}");
            }
        }
    }

    #[test]
    fn the_pads_are_square_and_inside_the_grid() {
        for (w, h) in WINDOWS {
            let l = Layout::new(w, h);
            for i in 0..SimonColor::ALL.len() {
                let r = l.pad_rect(i);
                assert!(
                    (r.w - r.h).abs() < 0.01,
                    "pad {i} is {r:?}, not square, at {w}x{h}"
                );
                assert!(
                    inside(r, l.grid),
                    "pad {i} at {r:?} leaves the grid at {w}x{h}"
                );
            }
        }
    }

    #[test]
    fn no_two_pads_overlap() {
        for (w, h) in WINDOWS {
            let l = Layout::new(w, h);
            for i in 0..SimonColor::ALL.len() {
                for j in i + 1..SimonColor::ALL.len() {
                    assert!(
                        !overlaps(l.pad_rect(i), l.pad_rect(j)),
                        "pads {i} and {j} overlap at {w}x{h}"
                    );
                }
            }
        }
    }

    #[test]
    fn the_pads_are_laid_out_in_the_order_their_numbers_say() {
        // Pad 2 is to the right of pad 1 and pad 3 below it. Without this the
        // digits in the corners would be a numbering nobody could follow.
        for (w, h) in WINDOWS {
            let l = Layout::new(w, h);
            let r = |i| l.pad_rect(i);
            assert!(r(1).x > r(0).x, "pad 2 is not right of pad 1 at {w}x{h}");
            assert!(r(2).y > r(0).y, "pad 3 is not below pad 1 at {w}x{h}");
            assert!((r(1).y - r(0).y).abs() < 0.01, "the top row is not level");
            assert!(
                (r(3).x - r(1).x).abs() < 0.01,
                "the right column is not plumb"
            );
        }
    }

    #[test]
    fn pad_rect_refuses_an_index_off_the_grid() {
        let l = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        assert_eq!(l.pad_rect(PAD_COLS * PAD_ROWS), Rect::EMPTY);
        assert_eq!(l.pad_rect(usize::MAX), Rect::EMPTY);
    }

    #[test]
    fn the_footer_buttons_fill_the_footer_without_overlapping() {
        for (w, h) in WINDOWS {
            let l = Layout::new(w, h);
            if l.footer.is_empty() {
                continue;
            }
            for i in 0..3 {
                let b = l.footer_button(i);
                assert!(
                    inside(b, l.footer),
                    "footer button {i} at {b:?} leaves the footer at {w}x{h}"
                );
                for j in i + 1..3 {
                    assert!(
                        !overlaps(b, l.footer_button(j)),
                        "footer buttons {i} and {j} overlap at {w}x{h}"
                    );
                }
            }
        }
    }

    #[test]
    fn footer_button_refuses_an_index_past_the_last_one() {
        let l = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        assert_eq!(l.footer_button(3), Rect::EMPTY);
        assert_eq!(l.footer_button(usize::MAX), Rect::EMPTY);
    }

    #[test]
    fn the_readouts_line_up_inside_the_header_without_overlapping() {
        for (w, h) in WINDOWS {
            let l = Layout::new(w, h);
            for i in 0..3 {
                let b = l.score_box(i);
                if b.is_empty() {
                    continue;
                }
                assert!(
                    inside(b, l.header),
                    "readout {i} at {b:?} leaves the header {:?} at {w}x{h}",
                    l.header
                );
                for j in i + 1..3 {
                    let other = l.score_box(j);
                    if other.is_empty() {
                        continue;
                    }
                    assert!(
                        !overlaps(b, other),
                        "readouts {i} and {j} overlap at {w}x{h}"
                    );
                }
            }
        }
    }

    #[test]
    fn the_readouts_are_laid_out_from_the_right_edge_inwards() {
        // So that adding a fourth would not move the other three, and so that
        // the one dropped when the header narrows is the leftmost.
        let l = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        assert!(
            l.score_box(0).x > l.score_box(1).x,
            "the best is not rightmost"
        );
        assert!(l.score_box(1).x > l.score_box(2).x, "the score is not next");
    }

    #[test]
    fn a_header_too_narrow_for_a_readout_drops_it_rather_than_squeezing_it() {
        // A box that has run off the left edge is `Rect::EMPTY`, not a box at a
        // negative x -- which would be a readout painted outside the window.
        let l = Layout::new(110.0, 800.0);
        assert!(
            !l.header.is_empty(),
            "this size is meant to keep the header"
        );
        assert!(l.score_box(2).is_empty(), "the third readout still fits");
        for i in 0..3 {
            assert!(l.score_box(i).x >= 0.0, "readout {i} is off the left edge");
        }

        // The same refusal covers a header that was dropped altogether, which
        // is why `score_box` needs no second guard for it: an empty header
        // leaves every box starting left of `header.x`. `score_box` used to
        // check for a dropped header separately, and deleting that check
        // changed nothing (`known-issues.md` lesson 51) -- this is the line
        // that keeps the surviving refusal honest about covering both.
        let dropped = Layout::new(300.0, 110.0);
        assert!(
            dropped.header.is_empty(),
            "this size is meant to drop the header"
        );
        for i in 0..3 {
            assert!(
                dropped.score_box(i).is_empty(),
                "readout {i} is laid out into a header that was dropped"
            );
        }
    }

    #[test]
    fn score_box_refuses_an_index_past_the_last_one() {
        let l = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        assert_eq!(l.score_box(3), Rect::EMPTY);
        assert_eq!(l.score_box(usize::MAX), Rect::EMPTY);
    }

    #[test]
    fn the_bands_are_given_up_in_the_documented_order() {
        // Status first, then the header, and the footer last of all -- the
        // footer being the pointer's only route to a new game, to the speed and
        // to the help. Written as implications over a sweep rather than as
        // three hand-picked sizes, because the sizes at which each drop happens
        // are exactly what a change to `PAD_SHARE` would move.
        for h in (20..600).step_by(3) {
            let l = Layout::new(600.0, h as f32);
            let (header, status, footer) = (
                !l.header.is_empty(),
                !l.status.is_empty(),
                !l.footer.is_empty(),
            );
            if !header {
                assert!(!status, "the header went before the status line at h={h}");
            }
            if !footer {
                assert!(
                    !header && !status,
                    "the footer went first at h={h}: header {header}, status {status}"
                );
            }
        }
    }

    #[test]
    fn the_pads_keep_their_share_of_the_window() {
        // What the drop ladder is for. If every band took what it asked for,
        // the pads in a short window would be the strip left over.
        //
        // The 0.45 is written out rather than read from `PAD_SHARE`. Comparing
        // the layout against the very constant that produced it is a test that
        // cannot fail: drop `PAD_SHARE` to 0.05 and a self-referential form
        // still passes, having checked only that the code agrees with itself.
        // The literal is the claim -- the pads get at least 45% of the height --
        // and moving `PAD_SHARE` is supposed to break this test.
        for h in (60..600).step_by(3) {
            let l = Layout::new(600.0, h as f32);
            assert!(
                l.grid.h >= h as f32 * 0.45 - 0.01,
                "the grid is only {} of {h} at 600 wide",
                l.grid.h
            );
        }
    }

    #[test]
    fn the_padding_never_eats_what_it_pads() {
        for (w, h) in WINDOWS {
            let l = Layout::new(w, h);
            assert!(
                l.pad * 4.0 <= w.min(h) + 0.01,
                "padding {} in a {w}x{h} window",
                l.pad
            );
            assert!(l.pad > 0.0, "no padding at all at {w}x{h}");
        }
    }

    #[test]
    fn a_window_of_no_size_is_still_a_window() {
        // The compositor can hand over a zero during a resize. Every rectangle
        // has to be finite and non-negative afterwards, or the first frame of
        // the new size paints somewhere that is not the screen.
        for (w, h) in [(0.0, 0.0), (0.0, 600.0), (600.0, 0.0), (-5.0, -5.0)] {
            let l = Layout::new(w, h);
            for (name, r) in [
                ("window", l.window),
                ("header", l.header),
                ("status", l.status),
                ("footer", l.footer),
                ("grid", l.grid),
                ("help", l.help),
                ("game over", l.game_over()),
                ("pad 0", l.pad_rect(0)),
            ] {
                assert!(
                    r.x.is_finite() && r.y.is_finite() && r.w.is_finite() && r.h.is_finite(),
                    "{name} is {r:?} at {w}x{h}"
                );
                assert!(r.w >= 0.0 && r.h >= 0.0, "{name} is {r:?} at {w}x{h}");
            }
        }
    }

    // ── Hit boxes ───────────────────────────────────────────────────────────

    #[test]
    fn every_pad_records_one_hit_box_and_it_is_the_pad_that_was_drawn() {
        // Recorded by the pass that paints it. A second copy of the geometry is
        // a second copy free to disagree, and the disagreement is invisible:
        // the window looks right and the clicks land somewhere else.
        for (w, h) in WINDOWS {
            let l = Layout::new(w, h);
            let f = game().frame(w, h);
            for &colour in &SimonColor::ALL {
                let boxes = hits_for(&f, Target::Pad(colour));
                assert_eq!(boxes.len(), 1, "{colour:?} has {boxes:?} at {w}x{h}");
                assert!(
                    same(boxes[0], l.pad_rect(colour.index())),
                    "{colour:?} takes clicks at {:?} but is drawn at {:?} at {w}x{h}",
                    boxes[0],
                    l.pad_rect(colour.index())
                );
            }
        }
    }

    #[test]
    fn no_two_hit_boxes_overlap() {
        for (w, h) in WINDOWS {
            let f = game().frame(w, h);
            let hits = f.hits();
            for (i, (at, a)) in hits.iter().enumerate() {
                for (bt, b) in &hits[i + 1..] {
                    assert!(
                        !overlaps(*a, *b),
                        "{at:?} at {a:?} and {bt:?} at {b:?} overlap at {w}x{h}"
                    );
                }
            }
        }
    }

    #[test]
    fn the_footer_offers_the_three_verbs_that_are_not_a_pad() {
        let l = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        let f = game().frame(WINDOW_WIDTH, WINDOW_HEIGHT);
        for (i, target) in [Target::NewGame, Target::Speed, Target::Help]
            .into_iter()
            .enumerate()
        {
            let boxes = hits_for(&f, target);
            assert_eq!(boxes.len(), 1, "{target:?} has {boxes:?}");
            assert!(
                same(boxes[0], l.footer_button(i)),
                "{target:?} is not footer button {i}"
            );
        }
    }

    #[test]
    fn each_footer_button_does_the_thing_its_label_names() {
        // `the_footer_offers_the_three_verbs_that_are_not_a_pad` checks that the
        // three boxes are in the three places; this checks that each is wired to
        // its own verb. Without it the whole footer could be pointed at one
        // target -- New Game at Speed, say -- and every box would still be
        // exactly where the layout says it should be.
        let mut app = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
        advance_rounds(&mut app, 2);
        let speed_before = app.speed;
        assert_eq!(app.score, 2);
        assert_eq!(click_on(&mut app, Target::NewGame), EventResult::Consumed);
        assert_eq!(app.score, 0, "New Game did not start another game");
        assert_eq!(app.state, GameState::PreSequence);
        assert_eq!(app.speed, speed_before, "New Game changed the speed");
        assert!(!app.help_is_open(), "New Game opened the sheet");

        let mut app = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
        advance_rounds(&mut app, 2);
        let score_before = app.score;
        assert_eq!(click_on(&mut app, Target::Speed), EventResult::Consumed);
        assert_ne!(app.speed, speed_before, "Speed did not change the speed");
        assert_eq!(app.score, score_before, "Speed restarted the game");

        let mut app = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
        advance_rounds(&mut app, 2);
        assert_eq!(click_on(&mut app, Target::Help), EventResult::Consumed);
        assert!(app.help_is_open(), "Help did not open the sheet");
        assert_eq!(app.score, 2, "Help restarted the game");
    }

    #[test]
    fn clicking_a_pad_presses_that_colour() {
        for &colour in &SimonColor::ALL {
            let mut app = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
            to_player_input(&mut app);
            assert_eq!(
                click_on(&mut app, Target::Pad(colour)),
                EventResult::Consumed
            );
            assert_eq!(
                app.lit(),
                Some(colour),
                "clicking {colour:?} lit something else"
            );
        }
    }

    #[test]
    fn a_click_lands_on_the_pad_it_looks_like_it_lands_on() {
        // Swept over the whole grid rather than at the four centres: a hit box
        // one cell out would still pass a centre test if every box were out by
        // the same amount, and would fail this one at the first edge.
        let l = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        let f = game().frame(WINDOW_WIDTH, WINDOW_HEIGHT);
        for i in 0..SimonColor::ALL.len() {
            let r = l.pad_rect(i);
            let colour = SimonColor::from_index(i).unwrap();
            for (fx, fy) in [
                (0.05, 0.05),
                (0.95, 0.05),
                (0.05, 0.95),
                (0.95, 0.95),
                (0.5, 0.5),
            ] {
                let (x, y) = (r.x + r.w * fx, r.y + r.h * fy);
                assert_eq!(
                    f.hit_test(x, y),
                    Some(Target::Pad(colour)),
                    "({x}, {y}) in pad {i} at {r:?} does not hit it"
                );
            }
        }
    }

    #[test]
    fn a_click_on_nothing_is_ignored() {
        let mut app = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
        to_player_input(&mut app);
        let l = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        // Between the header and the grid, which is padding and nothing else.
        let (x, y) = (WINDOW_WIDTH / 2.0, l.grid.y - l.pad / 2.0);
        assert_eq!(app.frame(WINDOW_WIDTH, WINDOW_HEIGHT).hit_test(x, y), None);
        assert_eq!(click(&mut app, x, y), EventResult::Ignored);
        assert_eq!(app.lit(), None, "a click on nothing lit a pad");
    }

    #[test]
    fn only_the_left_button_presses_a_pad() {
        // A right-click is the desktop's, for a context menu it may one day
        // show. A game that took it would swallow the menu and press a pad.
        let l = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        let (cx, cy) = l.pad_rect(0).centre();
        for button in [MouseButton::Right, MouseButton::Middle] {
            let mut app = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
            to_player_input(&mut app);
            let size = (WINDOW_WIDTH, WINDOW_HEIGHT);
            assert_eq!(
                app.click_at(cx, cy, button, size),
                EventResult::Ignored,
                "{button:?} pressed a pad"
            );
            assert_eq!(app.lit(), None, "{button:?} lit a pad");
        }
    }

    #[test]
    fn a_release_of_the_mouse_is_not_a_second_click() {
        let mut app = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
        to_player_input(&mut app);
        let l = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        let (cx, cy) = l.pad_rect(0).centre();
        for kind in [
            MouseEventKind::Release(MouseButton::Left),
            MouseEventKind::Move,
        ] {
            assert_eq!(
                handle_event(
                    &mut app,
                    &Event::Mouse(MouseEvent {
                        x: cx,
                        y: cy,
                        kind: kind.clone()
                    })
                ),
                EventResult::Ignored,
                "{kind:?} was taken for a press"
            );
            assert_eq!(app.lit(), None, "{kind:?} lit a pad");
        }
    }

    #[test]
    fn a_click_is_read_against_the_size_the_window_was_last_drawn_at() {
        // The whole reason the size is stored on the model. A click carries
        // window coordinates and nothing else; without the size the frame was
        // drawn at, there is no picture to test it against.
        let mut app = game();
        app.render(1200.0, 900.0);
        let wide = Layout::new(1200.0, 900.0);
        let (cx, cy) = wide.pad_rect(3).centre();
        assert!(
            cx > WINDOW_WIDTH,
            "the point has to be outside the opening window for this to prove anything"
        );
        to_player_input(&mut app);
        let size = (app.width, app.height);
        assert_eq!(
            app.click_at(cx, cy, MouseButton::Left, size),
            EventResult::Consumed
        );
        assert_eq!(app.selected, 3, "the click was read against the old size");
    }

    #[test]
    fn the_frame_is_balanced_at_every_size_and_in_every_state() {
        // A clip left open runs to the end of the frame and takes every
        // command after it with it.
        for (w, h) in WINDOWS {
            for (name, app) in states() {
                assert!(
                    app.frame(w, h).is_balanced(),
                    "{name} leaves a clip open at {w}x{h}"
                );
            }
        }
    }

    /// One game in each of the five states, plus the two overlays, so that a
    /// sweep over "every way the window can look" is a list and not a habit.
    fn states() -> Vec<(&'static str, Simon)> {
        let mut out = Vec::new();

        out.push(("pre-sequence", game()));

        let mut showing = game();
        showing.tick(PRE_SEQUENCE_MS);
        assert_eq!(showing.state, GameState::ShowSequence);
        out.push(("showing the sequence", showing));

        let mut waiting = game();
        to_player_input(&mut waiting);
        out.push(("the player's turn", waiting));

        let mut success = game();
        to_player_input(&mut success);
        play_sequence(&mut success);
        assert_eq!(success.state, GameState::RoundSuccess);
        out.push(("a round completed", success));

        let mut losing = game();
        lose(&mut losing);
        assert!(!losing.game_over_shown(), "the panel is up too early");
        out.push(("the losing pad still lit", losing));

        let mut over = game();
        lose(&mut over);
        over.tick(ERROR_FLASH_MS);
        assert!(over.game_over_shown(), "the panel never came up");
        out.push(("game over", over));

        let mut helped = game();
        helped.apply(Intent::ToggleHelp);
        out.push(("the help sheet", helped));

        out
    }

    // ── Dealing and showing a round ─────────────────────────────────────────

    #[test]
    fn a_new_game_has_one_colour_to_repeat() {
        let app = game();
        assert_eq!(app.sequence.len(), 1);
        assert_eq!(app.state, GameState::PreSequence);
        assert_eq!(app.player_index, 0);
        assert_eq!(app.score, 0);
    }

    #[test]
    fn a_fresh_window_has_not_lost_a_game() {
        // `games_played` used to be raised by `start_new_game`, and the
        // constructor calls it to deal the first colour -- so a window that had
        // just opened told the player they had already played one.
        let app = game();
        assert_eq!(
            app.games_lost, 0,
            "a game was counted before one was played"
        );
        assert_eq!(app.best, 0, "a best was recorded before a round was won");
    }

    #[test]
    fn the_round_number_is_the_length_of_the_sequence() {
        // Not a field beside it. `round` used to be raised before a push that
        // was itself conditional, so the number and the thing it counted were
        // two facts with nothing holding them together.
        let mut app = game();
        for expected in 1..6 {
            assert_eq!(app.round(), app.sequence.len());
            assert_eq!(app.round(), expected);
            advance_rounds(&mut app, 1);
        }
    }

    #[test]
    fn each_round_adds_exactly_one_colour_and_keeps_the_rest() {
        let mut app = game();
        let mut previous = app.sequence.clone();
        for _ in 0..5 {
            complete_round(&mut app);
            assert_eq!(
                app.sequence.len(),
                previous.len() + 1,
                "the round did not add exactly one colour"
            );
            assert_eq!(
                &app.sequence[..previous.len()],
                &previous[..],
                "the sequence changed behind the player"
            );
            previous = app.sequence.clone();
        }
    }

    #[test]
    fn the_sequence_is_not_the_same_four_colours_for_ever() {
        // The fault this crate's own generator shipped with: a power-of-two
        // modulus LCG reduced with `% 4` reads the low two bits, whose period
        // is exactly four, so every game at every seed played the same four
        // colours round and round and there was nothing to memorise.
        let mut app = game();
        advance_rounds(&mut app, 40);
        let seq = app.sequence.clone();
        let repeats_every_four = seq
            .iter()
            .enumerate()
            .skip(4)
            .all(|(i, c)| *c == seq[i % 4]);
        assert!(
            !repeats_every_four,
            "the sequence is a four-colour loop: {seq:?}"
        );
        for &colour in &SimonColor::ALL {
            assert!(
                seq.contains(&colour),
                "{colour:?} never came up in {} draws: {seq:?}",
                seq.len()
            );
        }
    }

    #[test]
    fn two_seeds_deal_two_different_games() {
        let mut a = Simon::with_seed(1);
        let mut b = Simon::with_seed(2);
        advance_rounds(&mut a, 20);
        advance_rounds(&mut b, 20);
        assert_ne!(a.sequence, b.sequence, "the seed does not reach the deal");
    }

    #[test]
    fn the_pause_runs_before_anything_lights_up() {
        let mut app = game();
        assert_eq!(app.lit(), None, "a pad is lit before the pause is over");
        app.tick(PRE_SEQUENCE_MS - 1);
        assert_eq!(app.state, GameState::PreSequence, "the pause ended early");
        assert_eq!(app.lit(), None, "a pad lit up during the pause");
        app.tick(1);
        assert_eq!(app.state, GameState::ShowSequence);
        assert_eq!(app.lit(), Some(app.sequence[0]));
    }

    #[test]
    fn the_pause_carries_its_overshoot_into_the_first_flash() {
        // A tick is however long the compositor took, not a fixed unit. Dropping
        // the remainder made the first colour of every round outstay the rest by
        // a frame -- more on a slower machine, which is the machine whose player
        // is least able to spare the difference.
        let mut app = game();
        app.tick(PRE_SEQUENCE_MS + 100);
        assert_eq!(app.state, GameState::ShowSequence);
        assert_eq!(
            app.playback.phase_ms, 100,
            "the overshoot was thrown on the floor"
        );
    }

    #[test]
    fn every_colour_of_the_sequence_is_shown_for_the_same_time() {
        // The property the carried overshoot exists for, measured rather than
        // asserted about the field: step the clock in small units and count how
        // many of them each colour is lit for.
        //
        // 7ms is deliberately not a divisor of any flash or gap, so a step that
        // happened to land on the phase boundaries cannot hide a drift.
        let mut app = game();
        for _ in 0..3 {
            complete_round(&mut app);
        }
        assert_eq!(app.sequence.len(), 4, "four colours to compare");

        let mut runs: Vec<(SimonColor, u32)> = Vec::new();
        let mut previous: Option<SimonColor> = None;
        for _ in 0..2000 {
            app.tick(7);
            let now = app.lit();
            // A run is a stretch of samples between two dark ones, so the same
            // colour twice in a row is two runs and not one long one.
            match now {
                Some(colour) if previous != Some(colour) => runs.push((colour, 1)),
                Some(_) => {
                    if let Some(last) = runs.last_mut() {
                        last.1 += 1;
                    }
                }
                None => {}
            }
            previous = now;
            if app.state == GameState::PlayerInput {
                break;
            }
        }

        assert_eq!(
            runs.len(),
            app.sequence.len(),
            "the sequence was not shown once through: {runs:?}"
        );
        let shown: Vec<SimonColor> = runs.iter().map(|&(c, _)| c).collect();
        assert_eq!(shown, app.sequence, "the wrong colours, or the wrong order");
        let counts: Vec<u32> = runs.iter().map(|&(_, n)| n).collect();
        let lo = counts.iter().copied().min().unwrap();
        let hi = counts.iter().copied().max().unwrap();
        assert!(
            hi - lo <= 1,
            "the colours were shown for {counts:?} steps of 7ms each"
        );
    }

    #[test]
    fn the_gap_between_two_flashes_is_dark() {
        let mut app = game();
        complete_round(&mut app);
        complete_round(&mut app);
        assert_eq!(app.sequence.len(), 3, "three colours, so two gaps");
        let (flash, gap) = playback_ms(app.speed);
        app.tick(PRE_SEQUENCE_MS);
        assert_eq!(app.lit(), Some(app.sequence[0]), "the first colour is dark");
        app.tick(flash);
        assert!(!app.playback.in_flash, "the flash did not end");
        assert_eq!(app.lit(), None, "the gap after the first colour is lit");
        app.tick(gap);
        assert_eq!(
            app.lit(),
            Some(app.sequence[1]),
            "the second colour is dark"
        );
    }

    #[test]
    fn the_playback_hands_over_to_the_player_at_the_end() {
        let mut app = game();
        advance_rounds(&mut app, 2);
        assert_eq!(app.state, GameState::PlayerInput);
        assert_eq!(app.player_index, 0, "the player starts part-way through");
        assert_eq!(app.lit(), None, "a pad is left lit for the player's turn");
    }

    #[test]
    fn one_long_tick_completes_as_much_of_the_sequence_as_it_covers() {
        // A window that was not drawn for a second gets one tick carrying the
        // whole second. Advancing one step per tick would leave the playback
        // running in slow motion for as long as the machine was busy.
        let mut big = game();
        let mut small = game();
        for _ in 0..4 {
            complete_round(&mut big);
            complete_round(&mut small);
        }
        assert_eq!(
            big.sequence, small.sequence,
            "the two games are not the same"
        );

        let (flash, gap) = playback_ms(big.speed);
        let whole = PRE_SEQUENCE_MS + (flash + gap) * big.sequence.len() as u64;
        big.tick(whole);
        for _ in 0..whole {
            small.tick(1);
        }
        assert_eq!(big.state, GameState::PlayerInput, "one big tick fell short");
        assert_eq!(
            (big.state, big.playback),
            (small.state, small.playback),
            "one big tick and many small ones part company"
        );
    }

    #[test]
    fn a_tick_of_no_time_changes_nothing() {
        let mut app = game();
        let before = (app.state, app.pre_ms, app.clock_ms, app.playback);
        assert_eq!(app.tick(0), EventResult::Ignored);
        assert_eq!((app.state, app.pre_ms, app.clock_ms, app.playback), before);
    }

    #[test]
    fn the_clock_is_asked_for_only_while_something_is_moving() {
        let mut app = game();
        assert!(
            app.wants_clock(),
            "the pause before the sequence holds no timer"
        );
        app.tick(PRE_SEQUENCE_MS);
        assert_eq!(app.state, GameState::ShowSequence);
        assert!(app.wants_clock(), "the playback holds no timer");

        // Two colours from here on, so pressing one leaves the round unfinished
        // and the game still in the player's turn -- which is the state whose
        // timer is the press flash and nothing else.
        complete_round(&mut app);
        to_player_input(&mut app);
        assert!(
            !app.wants_clock(),
            "a game waiting on a person keeps the machine awake"
        );
        let first = app.sequence[0].index();
        app.apply(Intent::Pad(first));
        assert_eq!(app.state, GameState::PlayerInput, "the round ended early");
        assert!(app.wants_clock(), "the press flash is never run down");
        app.tick(PLAYER_FLASH_MS);
        assert!(!app.wants_clock(), "the flash never runs out");
    }

    #[test]
    fn a_lost_game_stops_asking_for_the_clock_once_the_pad_goes_out() {
        let mut app = game();
        lose(&mut app);
        assert!(app.wants_clock(), "the losing pad is never run down");
        app.tick(ERROR_FLASH_MS);
        assert!(
            !app.wants_clock(),
            "a finished game keeps the machine awake for ever"
        );
    }

    // ── Repeating the sequence ──────────────────────────────────────────────

    #[test]
    fn the_right_pad_advances_the_player_through_the_sequence() {
        let mut app = game();
        advance_rounds(&mut app, 3);
        assert_eq!(app.sequence.len(), 4);
        for step in 0..3 {
            let colour = app.sequence[step];
            assert_eq!(
                app.apply(Intent::Pad(colour.index())),
                EventResult::Consumed
            );
            assert_eq!(app.player_index, step + 1, "the press did not count");
            assert_eq!(app.state, GameState::PlayerInput, "the round ended early");
            assert_eq!(app.lit(), Some(colour), "the pressed pad did not light");
        }
    }

    #[test]
    fn finishing_the_sequence_scores_a_round_and_celebrates() {
        let mut app = game();
        to_player_input(&mut app);
        assert_eq!(app.score, 0);
        play_sequence(&mut app);
        assert_eq!(app.state, GameState::RoundSuccess);
        assert_eq!(app.score, 1, "the round was not scored");
        assert_eq!(app.success_ms, 0, "the celebration starts part-way through");
    }

    #[test]
    fn the_score_is_the_number_of_rounds_completed() {
        let mut app = game();
        for done in 0..6 {
            assert_eq!(app.score, done, "the score is not the rounds completed");
            complete_round(&mut app);
        }
    }

    #[test]
    fn a_completed_round_deals_another_after_its_pause() {
        let mut app = game();
        to_player_input(&mut app);
        play_sequence(&mut app);
        app.tick(SUCCESS_FLASH_MS - 1);
        assert_eq!(app.state, GameState::RoundSuccess, "the pause ended early");
        assert_eq!(
            app.sequence.len(),
            1,
            "the next colour came before its time"
        );
        app.tick(1);
        assert_eq!(app.state, GameState::PreSequence);
        assert_eq!(app.sequence.len(), 2, "no colour was added");
        assert_eq!(app.player_index, 0, "the player is not back at the start");
    }

    #[test]
    fn the_wrong_pad_ends_the_game_and_counts_a_loss() {
        let mut app = game();
        to_player_input(&mut app);
        let wrong = a_wrong_colour(&app);
        assert_eq!(app.apply(Intent::Pad(wrong.index())), EventResult::Consumed);
        assert_eq!(app.state, GameState::GameOver);
        assert_eq!(app.games_lost, 1, "the loss was not counted");
        assert_eq!(app.player_index, 0, "a wrong press advanced the player");
        assert_eq!(
            app.lit(),
            Some(wrong),
            "the pad they got wrong is not shown"
        );
    }

    #[test]
    fn the_wrong_pad_stays_lit_longer_than_a_right_one() {
        // It is the only thing the player is being told: this is the one you
        // meant. A right press is confirmation and can be brief.
        const {
            assert!(ERROR_FLASH_MS > PLAYER_FLASH_MS);
        }
        let mut app = game();
        to_player_input(&mut app);
        let wrong = a_wrong_colour(&app);
        app.apply(Intent::Pad(wrong.index()));
        app.tick(PLAYER_FLASH_MS);
        assert_eq!(app.lit(), Some(wrong), "the losing pad went out too soon");
    }

    #[test]
    fn the_pad_the_player_lost_on_goes_out() {
        // The flash timer used to clear the lit pad only while the state was
        // still the player's turn -- and a wrong press leaves that state at
        // once. The losing pad stayed lit under the game-over panel, and
        // through the whole of the next game.
        let mut app = game();
        lose(&mut app);
        app.tick(ERROR_FLASH_MS);
        assert_eq!(app.lit(), None, "the losing pad is still lit");

        // The second half is the one that matters, and it has to start a new
        // game while the pad is *still lit* to mean anything. Ticking the flash
        // out first and only then restarting asked whether `None` stays `None`,
        // which it does however the restart is written.
        let mut app = game();
        lose(&mut app);
        assert!(app.lit().is_some(), "the losing pad never lit at all");
        app.apply(Intent::NewGame);
        assert_eq!(app.lit(), None, "it followed the player into the next game");
    }

    #[test]
    fn the_players_turn_always_begins_at_the_first_step_with_nothing_lit() {
        // `advance_playback` used to zero `player_index` and clear `flash` at
        // the handover, and both restated what already held -- `deal_round` had
        // zeroed the one, and `lit` stops reading the other the moment the state
        // leaves `ShowSequence`. Deleting a line because nothing can see it is
        // only safe while nothing can see it, so this holds the property the
        // deleted lines were aiming at, from every route into the state: a
        // future change that lets the index drift, or that makes `lit` consult
        // the flash in `PlayerInput`, breaks here rather than silently.
        let mut app = game();
        for round in 1..=4 {
            to_player_input(&mut app);
            assert_eq!(
                app.state,
                GameState::PlayerInput,
                "round {round} never reached the player"
            );
            assert_eq!(
                app.player_index, 0,
                "round {round} handed over part-way through the sequence"
            );
            assert_eq!(app.lit(), None, "round {round} handed over with a pad lit");
            play_sequence(&mut app);
            app.tick(SUCCESS_FLASH_MS);
        }

        // The same, reached the other way: a fresh game after a loss, where the
        // flash left over from the losing press is genuinely still set.
        let mut app = game();
        lose(&mut app);
        assert!(app.flash.is_some(), "the losing press left no flash");
        app.apply(Intent::NewGame);
        to_player_input(&mut app);
        assert_eq!(app.player_index, 0);
        assert_eq!(
            app.lit(),
            None,
            "the losing flash survived into the next turn"
        );
    }

    #[test]
    fn the_game_over_panel_waits_for_the_losing_pad_to_go_out() {
        // One clock drives both, so the panel appears exactly when the flash
        // expires rather than being gated on a second timer kept in step by
        // hand.
        let mut app = game();
        lose(&mut app);
        assert!(!app.game_over_shown(), "the panel covered the losing pad");
        app.tick(ERROR_FLASH_MS - 1);
        assert!(!app.game_over_shown(), "the panel came up early");
        app.tick(1);
        assert!(app.game_over_shown(), "the panel never came up");
    }

    #[test]
    fn a_pad_pressed_out_of_turn_moves_the_outline_and_nothing_else() {
        for setup in [
            "the pause before the sequence",
            "the machine's turn",
            "the celebration",
        ] {
            let mut app = game();
            match setup {
                "the machine's turn" => {
                    app.tick(PRE_SEQUENCE_MS);
                }
                "the celebration" => {
                    to_player_input(&mut app);
                    play_sequence(&mut app);
                }
                _ => {}
            }
            let before = (app.state, app.sequence.clone(), app.player_index, app.score);
            assert_eq!(app.apply(Intent::Pad(2)), EventResult::Consumed);
            assert_eq!(app.selected, 2, "the outline did not move during {setup}");
            assert!(app.show_selection, "the outline is still hidden");
            assert_eq!(
                (app.state, app.sequence.clone(), app.player_index, app.score),
                before,
                "a press during {setup} played a move"
            );
        }
    }

    #[test]
    fn a_press_that_arrives_out_of_step_does_nothing_rather_than_ending_the_process() {
        // That `player_index` is inside the sequence during the player's turn is
        // an invariant three other methods keep between them, not a fact visible
        // where the press lands. A press that arrives when it does not hold
        // should do nothing, not panic in front of the user.
        let mut app = game();
        to_player_input(&mut app);
        app.player_index = app.sequence.len();
        let before = app.state;
        app.apply(Intent::Pad(0));
        assert_eq!(app.state, before, "a press past the end changed the game");
        assert_eq!(app.lit(), None, "it lit a pad");
    }

    #[test]
    fn a_flash_with_no_time_left_on_it_goes_out_on_the_next_tick() {
        // `flash_pad` used to floor its argument at one, on the stated grounds
        // that a `Some((c, 0))` would be "lit by `lit` and never cleared by
        // `age_flash`". The second half of that was false, and this is the test
        // that says so: `age_flash`'s `Some(0) | None => None` arm clears a
        // remainder of zero exactly as it clears one that has just run out. It
        // is that arm, not a floor at the writer, that makes a zero-length
        // flash safe -- so it is that arm this test holds.
        for length in [0, 1] {
            let mut app = game();
            to_player_input(&mut app);
            app.flash_pad(SimonColor::Red, length);
            assert_eq!(
                app.lit(),
                Some(SimonColor::Red),
                "a flash of {length}ms did not light the pad"
            );
            app.tick(1);
            assert_eq!(app.lit(), None, "a flash of {length}ms outlived the clock");
        }

        // And the arm has to clear on reaching zero, not merely on going past
        // it: a flash ticked down by exactly its own length is finished.
        let mut app = game();
        to_player_input(&mut app);
        app.flash_pad(SimonColor::Red, 40);
        app.tick(39);
        assert_eq!(app.lit(), Some(SimonColor::Red), "it went out a tick early");
        app.tick(1);
        assert_eq!(app.lit(), None, "a flash ticked down to zero stayed lit");
    }

    #[test]
    fn a_press_flash_runs_out_on_its_own() {
        let mut app = game();
        advance_rounds(&mut app, 2);
        let first = app.sequence[0];
        app.apply(Intent::Pad(first.index()));
        app.tick(PLAYER_FLASH_MS - 1);
        assert_eq!(app.lit(), Some(first), "the press flash ended early");
        app.tick(1);
        assert_eq!(app.lit(), None, "the press flash never ended");
    }

    // ── The score, the best and the losses ──────────────────────────────────

    #[test]
    fn the_best_rises_with_the_score_and_stays_up() {
        // `high_score` and `longest_streak` were the same number under two
        // names, raised by the same two lines in the same two places, and shown
        // to the player as two readouts that could not possibly disagree.
        let mut app = game();
        for expected in 1..5 {
            complete_round(&mut app);
            assert_eq!(app.score, expected);
            assert_eq!(app.best, expected, "the best did not follow the score up");
        }
        lose(&mut app);
        assert_eq!(app.best, 4, "the best fell when the game ended");
    }

    #[test]
    fn a_new_game_keeps_the_best_and_the_losses_and_drops_the_score() {
        let mut app = game();
        advance_rounds(&mut app, 3);
        lose(&mut app);
        assert_eq!((app.score, app.best, app.games_lost), (3, 3, 1));
        app.apply(Intent::NewGame);
        assert_eq!(app.score, 0, "the score survived a new game");
        assert_eq!(app.best, 3, "the session best was thrown away");
        assert_eq!(app.games_lost, 1, "the losses were thrown away");
        assert_eq!(app.sequence.len(), 1, "the new game kept the old sequence");
        assert_eq!(app.state, GameState::PreSequence);
    }

    #[test]
    fn the_best_and_the_score_are_two_numbers_that_can_disagree() {
        // Which is the whole reason for showing both. Two fields raised by the
        // same lines are one number drawn twice.
        let mut app = game();
        advance_rounds(&mut app, 2);
        lose(&mut app);
        app.apply(Intent::NewGame);
        assert_eq!(app.score, 0);
        assert_eq!(app.best, 2);
        assert_ne!(app.score, app.best);

        // The best is a high-water mark, not a copy of the last score. Written
        // `self.best = self.score` it would agree with everything above --
        // every round in a single game raises the score to a new high -- and
        // only come apart here, where a shorter game follows a longer one.
        advance_rounds(&mut app, 1);
        assert_eq!(app.score, 1, "the second game did not score");
        assert_eq!(
            app.best, 2,
            "a shorter game pulled the session best down to {}",
            app.best
        );
        advance_rounds(&mut app, 2);
        assert_eq!(app.score, 3);
        assert_eq!(app.best, 3, "passing the old best did not raise it");
    }

    #[test]
    fn only_a_wrong_pad_counts_as_a_loss() {
        // A game abandoned with New Game is not one the player was beaten at,
        // and counting it would make the stats line disagree with their own
        // memory of the session.
        let mut app = game();
        for _ in 0..3 {
            app.apply(Intent::NewGame);
        }
        assert_eq!(app.games_lost, 0, "restarting counted as losing");
        lose(&mut app);
        assert_eq!(app.games_lost, 1);
        app.apply(Intent::NewGame);
        lose(&mut app);
        assert_eq!(app.games_lost, 2, "the second loss did not count");
    }

    #[test]
    fn a_lost_game_counts_once_however_long_it_is_left_sitting() {
        let mut app = game();
        lose(&mut app);
        for _ in 0..20 {
            app.tick(100);
        }
        assert_eq!(app.games_lost, 1, "the clock counted the loss again");
        assert_eq!(
            app.state,
            GameState::GameOver,
            "a finished game restarted itself"
        );
    }

    // ── The selection outline ───────────────────────────────────────────────

    /// Move the outline onto `index` with the arrow keys, from wherever it is.
    ///
    /// Deliberately by arrows rather than by `press_pad`: a pad press during the
    /// player's turn is a move in the game, so a helper that used one to line the
    /// outline up would be playing the round it is being used to set up.
    fn select(app: &mut Simon, index: usize) {
        for _ in 0..PAD_ROWS {
            app.apply(Intent::Move(Dir::Up));
        }
        for _ in 0..PAD_COLS {
            app.apply(Intent::Move(Dir::Left));
        }
        let (row, col) = grid_pos(index);
        for _ in 0..row {
            app.apply(Intent::Move(Dir::Down));
        }
        for _ in 0..col {
            app.apply(Intent::Move(Dir::Right));
        }
        assert_eq!(app.selected, index, "the outline did not reach pad {index}");
    }

    #[test]
    fn a_new_window_shows_no_outline() {
        // An outline before the player has touched anything points at a pad the
        // game picked, which reads as a hint and is not one.
        let app = game();
        assert!(!app.show_selection);
        let f = app.frame(WINDOW_WIDTH, WINDOW_HEIGHT);
        let l = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        for i in 0..SimonColor::ALL.len() {
            assert!(!outlined(&f, l.pad_rect(i)), "pad {i} is outlined already");
        }
    }

    #[test]
    fn the_arrows_walk_the_outline_round_the_grid() {
        let mut app = game();
        assert_eq!(app.selected, 0);
        for (dir, expected) in [
            (Dir::Right, 1),
            (Dir::Down, 3),
            (Dir::Left, 2),
            (Dir::Up, 0),
        ] {
            assert_eq!(app.apply(Intent::Move(dir)), EventResult::Consumed);
            assert_eq!(app.selected, expected, "{dir:?} went somewhere else");
            assert!(app.show_selection, "the outline is still hidden");
        }
    }

    #[test]
    fn an_arrow_into_the_wall_reveals_the_outline_once_and_then_does_nothing() {
        // The first arrow key is never a no-op, because turning the outline on is
        // a change the player can see. The second one, with the outline already
        // up and nowhere to go, has nothing to report -- and a window redrawn for
        // it would be a window redrawn for nothing.
        let mut app = game();
        assert_eq!(app.apply(Intent::Move(Dir::Up)), EventResult::Consumed);
        assert!(app.show_selection, "the arrow did not reveal the outline");
        assert_eq!(app.selected, 0, "the outline left the grid");
        assert_eq!(app.apply(Intent::Move(Dir::Up)), EventResult::Ignored);
        assert_eq!(app.selected, 0);
        assert_eq!(app.apply(Intent::Move(Dir::Left)), EventResult::Ignored);
        assert_eq!(app.selected, 0);
    }

    #[test]
    fn the_outline_follows_a_pad_pressed_by_any_route() {
        // Whichever way the player reached the last pad, the arrows start from
        // there -- not from wherever the outline was left the last time they
        // were used.
        let mut app = game();
        app.apply(Intent::Pad(2));
        assert_eq!(
            app.selected, 2,
            "the keyboard press did not move the outline"
        );
        assert!(app.show_selection);

        let mut clicked = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
        assert!(!clicked.show_selection);
        click_on(&mut clicked, Target::Pad(SimonColor::ALL[3]));
        assert_eq!(clicked.selected, 3, "the click did not move the outline");
        assert!(clicked.show_selection, "the click left the outline hidden");
    }

    #[test]
    fn the_outline_is_drawn_round_the_pad_it_names_and_no_other() {
        let l = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        for i in 0..SimonColor::ALL.len() {
            let mut app = game();
            select(&mut app, i);
            let f = app.frame(WINDOW_WIDTH, WINDOW_HEIGHT);
            for j in 0..SimonColor::ALL.len() {
                assert_eq!(
                    outlined(&f, l.pad_rect(j)),
                    i == j,
                    "with pad {i} selected, pad {j} is outlined wrongly"
                );
            }
        }
    }

    #[test]
    fn a_pads_name_is_centred_on_it_and_its_number_is_in_the_corner() {
        // The two pieces of type a pad carries are the only thing that tells a
        // player which pad is which and which digit presses it. The header
        // draws "0" and "1" too, so the boxes are picked by the pad they fall
        // inside rather than by what they say.
        for (w, h) in WINDOWS {
            let app = game();
            let f = app.frame(w, h);
            let l = Layout::new(w, h);
            for &colour in &SimonColor::ALL {
                let index = colour.index();
                let r = l.pad_rect(index);
                if r.is_empty() {
                    continue;
                }
                let on_pad: Vec<(String, Rect, f32)> = text_boxes(&f)
                    .into_iter()
                    .filter(|(_, b, _)| inside(*b, r))
                    .collect();

                let digit = index.saturating_add(1).to_string();
                let Some(name) = on_pad.iter().find(|(t, _, _)| t == colour.label()) else {
                    continue;
                };
                let Some(number) = on_pad.iter().find(|(t, _, _)| *t == digit) else {
                    continue;
                };

                let (cx, cy) = r.centre();
                let (nx, ny) = name.1.centre();
                assert!(
                    (nx - cx).abs() <= r.w * 0.06,
                    "the name of pad {index} at {w}x{h} sits off to one side: \
                     centre {nx} against the pad's {cx}"
                );
                assert!(
                    (ny - cy).abs() <= r.h * 0.06,
                    "the name of pad {index} at {w}x{h} sits high or low: \
                     centre {ny} against the pad's {cy}"
                );

                // The number belongs in the corner: inset from it, and well
                // clear of the middle where the name is.
                assert!(
                    number.1.x > r.x + 0.01 && number.1.y > r.y + 0.01,
                    "the number on pad {index} at {w}x{h} is flush against the corner"
                );
                assert!(
                    number.1.x < cx && number.1.y < cy,
                    "the number on pad {index} at {w}x{h} is not in the top-left corner"
                );
                assert!(
                    !overlaps(name.1, number.1),
                    "the name and the number of pad {index} at {w}x{h} are written over one another"
                );
            }
        }
    }

    #[test]
    fn every_pad_big_enough_for_words_carries_its_name_and_its_number() {
        // The test above lets a pad too small for type off; this one says the
        // pads in a window a player would actually use are never blank.
        let (w, h) = (WINDOW_WIDTH, WINDOW_HEIGHT);
        let app = game();
        let f = app.frame(w, h);
        let l = Layout::new(w, h);
        for &colour in &SimonColor::ALL {
            let index = colour.index();
            let r = l.pad_rect(index);
            let on_pad: Vec<String> = text_boxes(&f)
                .into_iter()
                .filter(|(_, b, _)| inside(*b, r))
                .map(|(t, _, _)| t)
                .collect();
            assert!(
                on_pad.iter().any(|t| t == colour.label()),
                "pad {index} is not named"
            );
            let digit = index.saturating_add(1).to_string();
            assert!(
                on_pad.contains(&digit),
                "pad {index} does not show the digit that presses it"
            );
        }
    }

    #[test]
    fn enter_presses_the_pad_the_outline_is_round() {
        // The arrows and Enter are the whole keyboard route into the game for a
        // player not using the digits. If Enter pressed anything but the
        // outlined pad, the outline would be decoration.
        let mut app = game();
        to_player_input(&mut app);
        let want = app.sequence[0];
        select(&mut app, want.index());
        assert_eq!(app.apply(Intent::Confirm), EventResult::Consumed);
        assert_eq!(app.player_index, 1, "Enter did not press the outlined pad");
        assert_eq!(app.lit(), Some(want));
    }

    #[test]
    fn enter_on_the_lost_game_starts_another_one() {
        // The panel says so in as many words, and this is the line that makes it
        // true from the keyboard.
        let mut app = game();
        advance_rounds(&mut app, 2);
        lose(&mut app);
        app.tick(ERROR_FLASH_MS);
        assert!(app.game_over_shown());
        assert_eq!(app.apply(Intent::Confirm), EventResult::Consumed);
        assert_eq!(app.state, GameState::PreSequence);
        assert_eq!(app.sequence.len(), 1);
        assert_eq!(app.score, 0);
    }

    #[test]
    fn clicking_the_lost_game_panel_starts_another_one() {
        let mut app = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
        lose(&mut app);
        app.tick(ERROR_FLASH_MS);
        assert_eq!(click_on(&mut app, Target::GameOver), EventResult::Consumed);
        assert_eq!(app.state, GameState::PreSequence, "the panel is a dead end");
    }

    #[test]
    fn the_panel_takes_the_click_from_the_pad_underneath_it() {
        // The pads are still drawn behind the panel and still have hit boxes.
        // The panel is recorded after them, and `hit_test` reads the last box
        // first, so a click on the panel is a restart and not a pad press.
        let mut app = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
        lose(&mut app);
        app.tick(ERROR_FLASH_MS);
        let l = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        let (cx, cy) = l.game_over().centre();
        let f = app.frame(WINDOW_WIDTH, WINDOW_HEIGHT);
        assert_eq!(
            f.hit_test(cx, cy),
            Some(Target::GameOver),
            "the pad behind the panel takes the click"
        );
    }

    // ── The speed control ───────────────────────────────────────────────────

    #[test]
    fn the_speed_key_cycles_and_the_footer_says_what_it_landed_on() {
        // One place in the window knows the speed, and it is the control that
        // changes it.
        let mut app = game();
        let opened_at = app.speed;
        let mut seen = vec![opened_at];
        for _ in 0..3 {
            let expected = app.speed.next();
            assert_eq!(press(&mut app, Key::S), EventResult::Consumed);
            assert_eq!(app.speed, expected, "S did not move the speed on");
            seen.push(expected);
            let lines = texts(&app.frame(WINDOW_WIDTH, WINDOW_HEIGHT));
            assert!(
                lines.contains(&format!("Speed: {}", expected.label())),
                "the footer does not say {expected:?}: {lines:?}"
            );
        }
        assert_eq!(app.speed, opened_at, "three presses did not come home");
        for speed in [Speed::Slow, Speed::Medium, Speed::Fast] {
            assert!(seen.contains(&speed), "S never reaches {speed:?}");
        }
    }

    #[test]
    fn clicking_the_speed_button_does_what_the_key_does() {
        let mut keyed = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
        let mut clicked = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
        press(&mut keyed, Key::S);
        click_on(&mut clicked, Target::Speed);
        assert_eq!(
            clicked.speed, keyed.speed,
            "the button and the key disagree"
        );
    }

    #[test]
    fn asking_for_the_speed_it_is_already_at_changes_nothing() {
        let mut app = game();
        let before = app.speed;
        assert_eq!(app.set_speed(before), EventResult::Ignored);
        assert_eq!(app.speed, before);
    }

    #[test]
    fn changing_the_speed_mid_playback_restarts_the_phase_rather_than_skipping_a_colour() {
        // The time already spent was measured against the *old* duration.
        // Carrying it into a shorter one ends a flash the player has not
        // finished seeing and runs the remainder straight through the gap behind
        // it -- skipping a colour of the very sequence they are being asked to
        // memorise.
        let mut app = game();
        complete_round(&mut app);
        complete_round(&mut app);
        assert_eq!(app.sequence.len(), 3);
        let (flash, gap) = playback_ms(app.speed);
        app.tick(PRE_SEQUENCE_MS + flash + gap + 50);
        assert_eq!(app.state, GameState::ShowSequence);
        assert_eq!(app.playback.step, 1, "the second colour is not up yet");
        assert!(app.playback.in_flash);
        assert_eq!(app.playback.phase_ms, 50);
        let showing = app.lit();

        assert_eq!(app.apply(Intent::CycleSpeed), EventResult::Consumed);
        assert_eq!(app.playback.phase_ms, 0, "the old phase was carried over");
        assert_eq!(app.playback.step, 1, "the speed change skipped a colour");
        assert!(app.playback.in_flash, "the flash was cut short");
        assert_eq!(app.lit(), showing, "a different colour is lit");
        assert_eq!(app.state, GameState::ShowSequence);
    }

    #[test]
    fn the_speed_survives_a_new_game() {
        // It is a preference, not a state of play. A player who set it to Fast
        // and then lost should not be put back to Slow for having lost.
        let mut app = game();
        let opened_at = app.speed;
        press(&mut app, Key::S);
        let chosen = app.speed;
        assert_ne!(chosen, opened_at, "this test needs the speed to have moved");
        lose(&mut app);
        app.apply(Intent::NewGame);
        assert_eq!(app.speed, chosen, "the new game reset the speed");
    }

    // ── The keys ────────────────────────────────────────────────────────────

    /// Every key this system can deliver.
    ///
    /// A list rather than a handful of interesting ones, so "nothing else does
    /// anything" is swept rather than assumed. `game_answers` below is the same
    /// set written as a wildcard-free match, which is what stops this file
    /// compiling when a variant is added to `Key` upstream: the two together mean
    /// a new key cannot arrive without someone deciding what Simon does with it.
    #[rustfmt::skip]
    const EVERY_KEY: [Key; 96] = [
        Key::A, Key::B, Key::C, Key::D, Key::E, Key::F, Key::G, Key::H, Key::I,
        Key::J, Key::K, Key::L, Key::M, Key::N, Key::O, Key::P, Key::Q, Key::R,
        Key::S, Key::T, Key::U, Key::V, Key::W, Key::X, Key::Y, Key::Z,
        Key::Num0, Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5,
        Key::Num6, Key::Num7, Key::Num8, Key::Num9,
        Key::F1, Key::F2, Key::F3, Key::F4, Key::F5, Key::F6, Key::F7, Key::F8,
        Key::F9, Key::F10, Key::F11, Key::F12,
        Key::Left, Key::Right, Key::Up, Key::Down, Key::Home, Key::End,
        Key::PageUp, Key::PageDown,
        Key::Backspace, Key::Delete, Key::Insert, Key::Enter, Key::Tab,
        Key::Escape, Key::Space,
        Key::LeftShift, Key::RightShift, Key::LeftCtrl, Key::RightCtrl,
        Key::LeftAlt, Key::RightAlt, Key::LeftSuper, Key::RightSuper,
        Key::Comma, Key::Period, Key::Semicolon, Key::Colon, Key::Slash,
        Key::Backslash, Key::LeftBracket, Key::RightBracket, Key::Minus,
        Key::Equals, Key::Apostrophe, Key::Grave,
        Key::PrintScreen, Key::ScrollLock, Key::Pause, Key::CapsLock,
        Key::NumLock,
        Key::VolumeUp, Key::VolumeDown, Key::VolumeMute, Key::MediaPlayPause,
        Key::MediaNextTrack, Key::MediaPrevTrack, Key::MediaStop,
        Key::Unknown(0),
    ];

    /// Whether Simon answers this key. No wildcard arm, on purpose.
    #[rustfmt::skip]
    fn game_answers(key: Key) -> bool {
        match key {
            Key::Num1 | Key::Num2 | Key::Num3 | Key::Num4
            | Key::Up | Key::Down | Key::Left | Key::Right
            | Key::Enter | Key::Space
            | Key::S | Key::N | Key::H | Key::Escape => true,

            Key::A | Key::B | Key::C | Key::D | Key::E | Key::F | Key::G
            | Key::I | Key::J | Key::K | Key::L | Key::M | Key::O | Key::P
            | Key::Q | Key::R | Key::T | Key::U | Key::V | Key::W | Key::X
            | Key::Y | Key::Z
            | Key::Num0 | Key::Num5 | Key::Num6 | Key::Num7 | Key::Num8
            | Key::Num9
            | Key::F1 | Key::F2 | Key::F3 | Key::F4 | Key::F5 | Key::F6
            | Key::F7 | Key::F8 | Key::F9 | Key::F10 | Key::F11 | Key::F12
            | Key::Home | Key::End | Key::PageUp | Key::PageDown
            | Key::Backspace | Key::Delete | Key::Insert | Key::Tab
            | Key::LeftShift | Key::RightShift | Key::LeftCtrl | Key::RightCtrl
            | Key::LeftAlt | Key::RightAlt | Key::LeftSuper | Key::RightSuper
            | Key::Comma | Key::Period | Key::Semicolon | Key::Colon
            | Key::Slash | Key::Backslash | Key::LeftBracket
            | Key::RightBracket | Key::Minus | Key::Equals | Key::Apostrophe
            | Key::Grave
            | Key::PrintScreen | Key::ScrollLock | Key::Pause | Key::CapsLock
            | Key::NumLock
            | Key::VolumeUp | Key::VolumeDown | Key::VolumeMute
            | Key::MediaPlayPause | Key::MediaNextTrack | Key::MediaPrevTrack
            | Key::MediaStop
            | Key::Unknown(_) => false,
        }
    }

    /// The keys a help row names.
    ///
    /// The sheet writes them for a person -- "1 - 4", "Arrows" -- and this is the
    /// translation back into the enum, so the sheet and the handler can be walked
    /// against one another. A row nobody has taught this function about is a
    /// panic rather than a silent pass.
    fn keys_named(row: &str) -> Vec<Key> {
        match row {
            "1 - 4" => vec![Key::Num1, Key::Num2, Key::Num3, Key::Num4],
            "Arrows" => vec![Key::Up, Key::Down, Key::Left, Key::Right],
            "Enter / Space" => vec![Key::Enter, Key::Space],
            "S" => vec![Key::S],
            "N" => vec![Key::N],
            "H / Escape" => vec![Key::H, Key::Escape],
            other => panic!("the sheet lists {other}, which no test knows the keys for"),
        }
    }

    #[test]
    fn every_key_the_sheet_lists_does_something() {
        for (row, what) in HELP_ROWS {
            for key in keys_named(row) {
                assert!(
                    key_intent(&probe::press(key)).is_some(),
                    "the sheet promises {key:?} will {what}, and it does nothing"
                );
            }
        }
    }

    #[test]
    fn no_key_that_does_something_is_missing_from_the_sheet() {
        // The other direction, which is the one that rots: a key added to the
        // handler and not to the sheet is a control only its author knows about.
        let listed: Vec<Key> = HELP_ROWS
            .iter()
            .flat_map(|&(row, _)| keys_named(row))
            .collect();
        for key in EVERY_KEY {
            let acts = key_intent(&probe::press(key)).is_some();
            assert_eq!(
                acts,
                listed.contains(&key),
                "{key:?}: the handler says {acts}, the sheet says {}",
                listed.contains(&key)
            );
        }
    }

    #[test]
    fn the_sweep_of_keys_agrees_with_the_handler() {
        // `game_answers` is the wildcard-free copy that makes a new `Key`
        // variant a compile error here rather than a key that quietly does
        // nothing. Checking it against the handler is what keeps the copy
        // honest.
        for key in EVERY_KEY {
            assert_eq!(
                key_intent(&probe::press(key)).is_some(),
                game_answers(key),
                "{key:?}"
            );
        }
        assert_eq!(
            EVERY_KEY.iter().filter(|&&k| game_answers(k)).count(),
            14,
            "the keys the game answers changed without this number changing"
        );
    }

    #[test]
    fn a_key_coming_back_up_is_not_a_second_press() {
        // The compositor sends a `KeyEvent` with `pressed: false` for every key
        // released. A handler that reads only `key` runs every action twice --
        // here, two pads pressed for one keystroke, the second of them wrong.
        // See `known-issues.md` lesson 63.
        for key in EVERY_KEY {
            let mut up = probe::press(key);
            up.pressed = false;
            assert_eq!(key_intent(&up), None, "{key:?} acts on the way up");
        }

        // And through the whole event path, not only the mapping.
        let mut app = game();
        assert_eq!(press(&mut app, Key::S), EventResult::Consumed);
        let after = app.speed;
        let mut up = probe::press(Key::S);
        up.pressed = false;
        assert_eq!(
            handle_event(&mut app, &Event::Key(up)),
            EventResult::Ignored,
            "the release was taken for a press"
        );
        assert_eq!(app.speed, after, "the release cycled the speed again");
    }

    #[test]
    fn the_window_keeps_its_ctrl_and_alt_combinations() {
        // A Ctrl+S that changes the speed is a Ctrl+S the desktop cannot have.
        // The game had exactly that before the rewrite.
        let alt = Modifiers {
            alt: true,
            ..Modifiers::default()
        };
        for key in EVERY_KEY {
            assert_eq!(key_intent(&probe::ctrl(key)), None, "Ctrl+{key:?} plays");
            assert_eq!(
                key_intent(&probe::press_with(key, alt)),
                None,
                "Alt+{key:?} plays"
            );
        }
        let mut app = game();
        let before = app.speed;
        assert_eq!(
            handle_event(&mut app, &Event::Key(probe::ctrl(Key::S))),
            EventResult::Ignored
        );
        assert_eq!(app.speed, before, "Ctrl+S changed the speed");
    }

    #[test]
    fn shift_does_not_stop_a_key_working() {
        // Shift is how a hurried player holds a key, not a combination the
        // desktop wants back. A game that ignored it would be silently
        // unresponsive to a keystroke that looks identical to the working one.
        let mut app = game();
        assert_eq!(
            handle_event(&mut app, &Event::Key(probe::shift(Key::N))),
            EventResult::Consumed
        );
        assert_eq!(app.state, GameState::PreSequence);
        assert_eq!(
            key_intent(&probe::shift(Key::S)),
            Some(Intent::CycleSpeed),
            "Shift+S is not the speed key"
        );
    }

    #[test]
    fn the_digits_press_the_pads_they_are_printed_on() {
        for (key, index) in [
            (Key::Num1, 0),
            (Key::Num2, 1),
            (Key::Num3, 2),
            (Key::Num4, 3),
        ] {
            let mut app = game();
            assert_eq!(press(&mut app, key), EventResult::Consumed);
            assert_eq!(app.selected, index, "{key:?} is not pad {index}");
        }
    }

    #[test]
    fn the_arrow_keys_move_the_outline_the_way_they_point() {
        // The four directions are checked as intents elsewhere. This is the
        // other half of the wire: that the key with the arrow printed on it
        // asks for the direction the arrow points. Two of them swapped in the
        // handler would leave every intent test green and the keyboard useless.
        for (key, dir) in [
            (Key::Up, Dir::Up),
            (Key::Down, Dir::Down),
            (Key::Left, Dir::Left),
            (Key::Right, Dir::Right),
        ] {
            assert_eq!(
                key_intent(&probe::press(key)),
                Some(Intent::Move(dir)),
                "{key:?} does not point {dir:?}"
            );
        }
        // And through the whole event path, walking the grid from the pad the
        // outline starts on and back to it.
        let mut app = game();
        for (key, expected) in [
            (Key::Right, 1),
            (Key::Down, 3),
            (Key::Left, 2),
            (Key::Up, 0),
        ] {
            assert_eq!(press(&mut app, key), EventResult::Consumed);
            assert_eq!(app.selected, expected, "{key:?} went somewhere else");
        }
    }

    // ── The help sheet ──────────────────────────────────────────────────────

    #[test]
    fn the_sheet_is_not_drawn_until_it_is_asked_for() {
        let lines = texts(&game().frame(WINDOW_WIDTH, WINDOW_HEIGHT));
        for (key, meaning) in HELP_ROWS {
            assert!(!lines.contains(&key.to_string()), "{key} without the sheet");
            assert!(
                !lines.contains(&meaning.to_string()),
                "{meaning} without the sheet"
            );
        }
        assert!(
            !lines.iter().any(|l| l.contains("Click anywhere")),
            "the closing line is drawn over a shut sheet"
        );
        // The header draws the title upper-cased, so the mixed-case one is the
        // sheet's own and its absence is a real test rather than a coincidence.
        assert!(
            !lines.contains(&HELP_TITLE.to_string()),
            "the sheet heads itself with the sheet shut: {lines:?}"
        );
    }

    #[test]
    fn the_sheet_lists_every_control_and_what_it_does() {
        let mut app = game();
        app.apply(Intent::ToggleHelp);
        let lines = texts(&app.frame(WINDOW_WIDTH, WINDOW_HEIGHT));
        assert!(
            lines.contains(&HELP_TITLE.to_string()),
            "the sheet has no head"
        );
        for (key, meaning) in HELP_ROWS {
            assert!(lines.contains(&key.to_string()), "{key} is not listed");
            assert!(lines.contains(&meaning.to_string()), "{meaning} is missing");
        }
        assert!(
            lines.iter().any(|l| l.contains("Click anywhere")),
            "the sheet does not say how to shut it"
        );
    }

    /// Every line the sheet itself writes, with the box it covers.
    ///
    /// The game is still drawn behind the sheet -- the sheet is painted over
    /// it -- so the frame's other text is not the sheet's and is allowed to sit
    /// anywhere. Picking the sheet's own lines out by their wording is what
    /// keeps the tests below tests of the sheet's layout rather than of the
    /// window's.
    fn sheet_lines(f: &Frame) -> Vec<(String, Rect, f32)> {
        let mut wanted: Vec<&str> = vec![HELP_TITLE, "Click anywhere to close"];
        for (key, what) in HELP_ROWS {
            wanted.push(key);
            wanted.push(what);
        }
        text_boxes(f)
            .into_iter()
            .filter(|(t, _, _)| wanted.contains(&t.as_str()))
            .collect()
    }

    #[test]
    fn the_sheets_rows_do_not_overwrite_one_another() {
        // The sheet divides its own height into one row per control plus one
        // for the heading and one for the closing line. Get that count wrong,
        // or start the rows at the wrong offset, and the sheet still writes
        // every string it promises -- on top of itself, or off the bottom of
        // the panel. A test that only counts strings would call that a pass.
        for (w, h) in WINDOWS {
            let mut app = game();
            app.apply(Intent::ToggleHelp);
            let f = app.frame(w, h);
            let l = Layout::new(w, h);
            let lines = sheet_lines(&f);
            for (text, r, size) in &lines {
                assert!(
                    *size >= MIN_DRAWN_FONT,
                    "{text:?} is written at {size}px in a {w}x{h} window"
                );
                assert!(
                    inside(*r, l.help),
                    "{text:?} at {r:?} leaves the sheet {:?} of a {w}x{h} window",
                    l.help
                );
            }
            for (i, (a_text, a, _)) in lines.iter().enumerate() {
                for (b_text, b, _) in &lines[i + 1..] {
                    assert!(
                        !overlaps(*a, *b),
                        "{a_text:?} at {a:?} and {b_text:?} at {b:?} \
                         are written over one another in a {w}x{h} window"
                    );
                }
            }
        }
    }

    #[test]
    fn a_rows_meaning_is_written_to_the_right_of_the_key_it_explains() {
        // Two columns, and the same two columns on every row: a sheet whose
        // meanings began wherever the key before them happened to end would be
        // a ragged wall of text to read down. The column has to leave room for
        // the meaning as well -- pushed far enough right, every meaning is
        // elided to nothing and the sheet explains none of its own controls.
        let mut app = game();
        app.apply(Intent::ToggleHelp);
        let f = app.frame(WINDOW_WIDTH, WINDOW_HEIGHT);
        let l = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        let boxes = sheet_lines(&f);
        let find = |wanted: &str| -> Rect {
            boxes
                .iter()
                .find(|(t, _, _)| t == wanted)
                .map(|&(_, r, _)| r)
                .unwrap_or_else(|| panic!("the sheet does not write {wanted:?}"))
        };
        let mut columns: Option<(f32, f32)> = None;
        for (key, what) in HELP_ROWS {
            let (k, m) = (find(key), find(what));
            assert!(k.w > 0.0, "{key:?} is written with no width");
            assert!(m.w > 0.0, "{what:?} is written with no width");
            assert!(
                m.x >= k.right() - 0.01,
                "{what:?} at {m:?} runs back over {key:?} at {k:?}"
            );
            assert!(
                k.x > l.help.x + 0.01,
                "{key:?} at {k:?} is written on the sheet's own edge {:?}",
                l.help
            );
            match columns {
                None => columns = Some((k.x, m.x)),
                Some((kx, mx)) => {
                    assert!(
                        (k.x - kx).abs() < 0.01 && (m.x - mx).abs() < 0.01,
                        "{key:?}/{what:?} start at ({}, {}) and the rows above \
                         at ({kx}, {mx})",
                        k.x,
                        m.x
                    );
                }
            }
        }
    }

    #[test]
    fn the_sheet_covers_the_window_and_takes_every_click() {
        for (w, h) in WINDOWS {
            let mut app = game();
            app.apply(Intent::ToggleHelp);
            let f = app.frame(w, h);
            let boxes = hits_for(&f, Target::HelpSheet);
            assert_eq!(boxes.len(), 1, "the sheet has {boxes:?} at {w}x{h}");
            assert!(
                same(boxes[0], Layout::new(w, h).window),
                "the sheet takes clicks on {:?} of a {w}x{h} window",
                boxes[0]
            );
        }
    }

    #[test]
    fn either_key_shuts_the_sheet() {
        for key in [Key::H, Key::Escape] {
            let mut app = game();
            assert_eq!(press(&mut app, Key::H), EventResult::Consumed);
            assert!(app.help_is_open(), "H did not open the sheet");
            assert_eq!(press(&mut app, key), EventResult::Consumed);
            assert!(!app.help_is_open(), "{key:?} did not shut the sheet");
        }
    }

    #[test]
    fn escape_with_no_sheet_up_does_nothing_at_all() {
        // Reported as ignored rather than consumed, so the window is not redrawn
        // for a key that changed nothing.
        let mut app = game();
        let before = (app.state, app.selected, app.show_selection);
        assert_eq!(press(&mut app, Key::Escape), EventResult::Ignored);
        assert_eq!((app.state, app.selected, app.show_selection), before);
        assert!(!app.help_is_open());
    }

    #[test]
    fn clicking_the_sheet_anywhere_shuts_it() {
        let mut app = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
        app.apply(Intent::ToggleHelp);
        let l = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        // The centre of a pad: the point that would press a colour if the sheet
        // were not in front of it.
        let (cx, cy) = l.pad_rect(0).centre();
        assert_eq!(click(&mut app, cx, cy), EventResult::Consumed);
        assert!(!app.help_is_open(), "the click went through the sheet");
        assert_eq!(
            app.selected, 0,
            "the click reached the pad behind the sheet"
        );
        assert!(
            !app.show_selection,
            "the click pressed a pad through the sheet"
        );
    }

    #[test]
    fn clicking_the_help_button_opens_the_sheet() {
        // The button is the only way into the instructions for a player who
        // never touches the keyboard, which is the player most likely to want
        // them.
        let mut app = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
        assert!(
            !app.help_is_open(),
            "the sheet is up before it was asked for"
        );
        assert_eq!(click_on(&mut app, Target::Help), EventResult::Consumed);
        assert!(app.help_is_open(), "the Help button did not open the sheet");
    }

    #[test]
    fn clicking_the_help_button_with_the_sheet_up_shuts_it_once() {
        // The button is underneath the sheet, and the sheet is recorded after
        // it, so the click is the sheet's. A click that reached the button would
        // ask to open a sheet that is already open -- which, with a toggle
        // behind it, is a sheet that never shuts.
        let mut app = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
        app.apply(Intent::ToggleHelp);
        assert_eq!(click_on(&mut app, Target::Help), EventResult::Consumed);
        assert!(!app.help_is_open(), "the Help button reopened the sheet");
    }

    #[test]
    fn the_sheet_swallows_every_other_control() {
        for intent in [
            Intent::Pad(0),
            Intent::Move(Dir::Right),
            Intent::Confirm,
            Intent::CycleSpeed,
            Intent::NewGame,
        ] {
            let mut app = game();
            advance_rounds(&mut app, 2);
            app.apply(Intent::ToggleHelp);
            let before = (
                app.state,
                app.sequence.clone(),
                app.player_index,
                app.score,
                app.speed,
                app.selected,
                app.show_selection,
            );
            assert_eq!(
                app.apply(intent),
                EventResult::Ignored,
                "{intent:?} reached the game through the sheet"
            );
            assert_eq!(
                (
                    app.state,
                    app.sequence.clone(),
                    app.player_index,
                    app.score,
                    app.speed,
                    app.selected,
                    app.show_selection,
                ),
                before,
                "{intent:?} changed the game behind the sheet"
            );
            assert!(app.help_is_open(), "{intent:?} shut the sheet");
        }
    }

    #[test]
    fn the_sheet_pauses_the_game() {
        // The moment a player is most likely to open the instructions is the
        // moment they are least sure what to do, which is mid-round. Reading how
        // to play would otherwise cost them the round they were reading about.
        let mut app = game();
        app.tick(100);
        assert!(app.wants_clock());
        app.apply(Intent::ToggleHelp);
        assert!(!app.wants_clock(), "the paused game still holds a timer");

        let before = (app.state, app.pre_ms, app.clock_ms, app.playback);
        assert_eq!(app.tick(10_000), EventResult::Ignored, "the clock ran on");
        assert_eq!(
            (app.state, app.pre_ms, app.clock_ms, app.playback),
            before,
            "the sequence played behind the sheet"
        );

        app.apply(Intent::CloseHelp);
        assert!(app.wants_clock(), "the game did not start again");
        app.tick(PRE_SEQUENCE_MS);
        assert_eq!(
            app.state,
            GameState::ShowSequence,
            "the pause did not pick up where it left off"
        );
    }

    #[test]
    fn a_lost_game_can_still_be_read_about() {
        // The sheet is reachable from every state, including the one where the
        // player has just been beaten and may want to know what the keys were.
        let mut app = game();
        lose(&mut app);
        app.tick(ERROR_FLASH_MS);
        assert_eq!(press(&mut app, Key::H), EventResult::Consumed);
        assert!(app.help_is_open());
        assert_eq!(app.state, GameState::GameOver, "reading restarted the game");

        // And it has to be closable from there, which is a fact about the order
        // the two overlays are drawn in. `hit_test` reads the last box first, so
        // the sheet has to be painted *after* the game-over panel; painted
        // before it, the panel's box wins in the middle of the screen, the
        // modal guard in `apply` throws that intent away, and the sheet becomes
        // a trap the player can only leave with the keyboard.
        let f = app.frame(app.width, app.height);
        let (px, py) = Layout::new(app.width, app.height).game_over().centre();
        assert_eq!(
            f.hit_test(px, py),
            Some(Target::HelpSheet),
            "the game-over panel took a click aimed at the sheet over it"
        );
        assert_eq!(click(&mut app, px, py), EventResult::Consumed);
        assert!(
            !app.help_is_open(),
            "the sheet could not be closed over a lost game"
        );
    }

    // ── What the window says ────────────────────────────────────────────────

    #[test]
    fn the_status_line_names_the_part_of_the_round_the_game_is_in() {
        let mut app = game();
        assert_eq!(app.status_line(), "Get ready...");
        app.tick(PRE_SEQUENCE_MS);
        assert_eq!(app.state, GameState::ShowSequence);
        assert_eq!(app.status_line(), "Watch  1/1");
        to_player_input(&mut app);
        assert_eq!(app.status_line(), "Your turn  1/1");
        play_sequence(&mut app);
        assert_eq!(app.status_line(), "Round 1 complete");

        let mut lost = game();
        lose(&mut lost);
        assert_eq!(lost.status_line(), "Game over");
    }

    #[test]
    fn no_two_states_share_a_status_line_or_a_colour() {
        // The band is the only running commentary the game gives. Two states
        // that read and look the same are two states the player cannot tell
        // apart.
        let lines: Vec<(String, String, Color)> = states()
            .into_iter()
            .filter(|(name, _)| *name != "the losing pad still lit" && *name != "the help sheet")
            .map(|(name, app)| (name.to_string(), app.status_line(), app.status_colour()))
            .collect();
        for (i, (an, a_line, a_col)) in lines.iter().enumerate() {
            for (bn, b_line, b_col) in &lines[i + 1..] {
                assert_ne!(a_line, b_line, "{an} and {bn} read the same");
                assert_ne!(a_col, b_col, "{an} and {bn} are the same colour");
            }
        }
    }

    #[test]
    fn the_status_line_counts_the_player_through_the_sequence() {
        let mut app = game();
        advance_rounds(&mut app, 3);
        assert_eq!(app.sequence.len(), 4);
        for step in 0..4 {
            assert_eq!(app.status_line(), format!("Your turn  {}/4", step + 1));
            app.apply(Intent::Pad(app.sequence[step].index()));
        }
        assert_eq!(app.status_line(), "Round 4 complete");
    }

    #[test]
    fn the_count_never_reads_past_the_end_of_the_sequence() {
        // Both counts are clamped, because both are "how far through" plus one
        // and the plus one runs off the end at the moment the round changes
        // hands. A status line reading 2/1 is a line the player has to ignore.
        let mut app = game();
        app.tick(PRE_SEQUENCE_MS);
        app.playback.step = 99;
        assert_eq!(app.status_line(), "Watch  1/1");
        to_player_input(&mut app);
        app.player_index = 99;
        assert_eq!(app.status_line(), "Your turn  1/1");
    }

    #[test]
    fn the_window_says_watch_once() {
        // It used to say it twice: in the status band, and again in a banner
        // over the grid that the player had to look past to see the colours
        // they were being told to watch.
        let mut app = game();
        app.tick(PRE_SEQUENCE_MS);
        let lines = texts(&app.frame(WINDOW_WIDTH, WINDOW_HEIGHT));
        assert_eq!(
            lines.iter().filter(|l| l.contains("Watch")).count(),
            1,
            "the window says Watch more than once: {lines:?}"
        );
    }

    #[test]
    fn the_header_shows_the_best_the_score_and_the_round() {
        // The three numbers are deliberately made to differ. Reached by playing
        // straight through, the best and the score are always equal -- every
        // round sets a new high -- so swapping which readout gets which was a
        // change no assertion here could see. A longer game followed by a
        // shorter one pulls them apart.
        let mut app = game();
        advance_rounds(&mut app, 3);
        app.apply(Intent::NewGame);
        advance_rounds(&mut app, 1);
        assert_eq!(
            (app.best, app.score, app.round()),
            (3, 1, 2),
            "the fixture no longer distinguishes the three readouts"
        );
        let l = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        let boxes = text_boxes(&app.frame(WINDOW_WIDTH, WINDOW_HEIGHT));
        for (i, name, value) in [(0, "BEST", "3"), (1, "SCORE", "1"), (2, "ROUND", "2")] {
            let readout = l.score_box(i);
            assert!(!readout.is_empty(), "readout {i} is not drawn at all");
            for want in [name, value] {
                assert!(
                    boxes
                        .iter()
                        .any(|(body, r, _)| body == want && inside(*r, readout)),
                    "{want} is not inside readout {i} at {readout:?}: {boxes:?}"
                );
            }
        }
    }

    #[test]
    fn the_title_never_runs_under_the_readouts() {
        // It is elided into the room to their left rather than drawn full width
        // and painted over.
        for (w, h) in WINDOWS {
            let l = Layout::new(w, h);
            let app = game();
            let f = app.frame(w, h);
            let title = HELP_TITLE.to_uppercase();
            for (body, r, _) in text_boxes(&f) {
                if body != title {
                    continue;
                }
                for i in 0..3 {
                    let readout = l.score_box(i);
                    if readout.is_empty() {
                        continue;
                    }
                    assert!(
                        !overlaps(r, readout),
                        "the title at {r:?} runs under readout {i} at {readout:?} at {w}x{h}"
                    );
                }
            }
        }
    }

    // ── The pads, drawn ─────────────────────────────────────────────────────

    #[test]
    fn only_the_lit_pad_is_drawn_lit() {
        let mut app = game();
        complete_round(&mut app);
        app.tick(PRE_SEQUENCE_MS);
        let lit = app.lit().expect("a colour is being shown");
        let l = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        let f = app.frame(WINDOW_WIDTH, WINDOW_HEIGHT);
        for &colour in &SimonColor::ALL {
            let want = if colour == lit {
                colour.lit()
            } else {
                colour.dim()
            };
            assert_eq!(
                fill_at(&f, l.pad_rect(colour.index())),
                Some(want),
                "{colour:?} is the wrong shade with {lit:?} lit"
            );
        }
    }

    #[test]
    fn the_glow_goes_behind_the_pad_rather_than_over_its_face() {
        // It used to be pushed after the pad, so a 60-alpha wash of the pad's
        // own colour covered the face and the label rather than ringing them.
        let mut app = game();
        app.tick(PRE_SEQUENCE_MS);
        let colour = app.lit().expect("a colour is being shown");
        let l = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        let r = l.pad_rect(colour.index());
        let grow = (r.w.min(r.h) * 0.04).min(6.0);
        let halo = Rect::new(r.x - grow, r.y - grow, r.w + grow * 2.0, r.h + grow * 2.0)
            .intersect(l.window)
            .expect("the halo is somewhere in the window");
        let f = app.frame(WINDOW_WIDTH, WINDOW_HEIGHT);
        let halo_at = fill_order(&f, halo).expect("no halo round the lit pad");
        let face_at = fill_order(&f, r).expect("the lit pad is not painted");
        assert!(
            halo_at < face_at,
            "the halo is painted over the pad it is meant to ring"
        );
        assert!(
            halo.w > r.w && halo.h > r.h,
            "the halo is no larger than the pad, so nothing of it shows"
        );
    }

    #[test]
    fn the_glow_never_reaches_outside_the_window() {
        // Grown proportionally, rather than offset by a flat number of pixels
        // that a small window has no room for.
        //
        // This is the whole of the argument that `draw_pads` needs no clip. The
        // halo is drawn unclipped because `grow` is never more than half the gap
        // `pad_rect` insets a pad by -- so it cannot leave the pad's own cell,
        // the cells tile the grid, and the grid is inside the window. That is a
        // claim about every size, not about eight of them, so the sweep below is
        // dense: a comment asserting the bound is an assertion nobody checks,
        // and the bound is tight enough (0.0368 against 0.04) that a small
        // change to either number would break it silently.
        // The geometry is checked densely and the frame coarsely, because the
        // two cost very different amounts and only one of them needs the sweep:
        // the halo's size comes from `pad_rect` alone, so a layout is enough to
        // decide it, and building four thousand frames to learn the same thing
        // would make this the slowest test in the crate for nothing.
        for w in (24..1400).step_by(11) {
            for h in (24..1000).step_by(13) {
                let (w, h) = (w as f32, h as f32);
                let l = Layout::new(w, h);
                for i in 0..PAD_COLS * PAD_ROWS {
                    let cell = l.pad_rect(i);
                    if cell.is_empty() {
                        continue;
                    }
                    let grow = (cell.w.min(cell.h) * 0.04).min(6.0);
                    let halo = Rect::new(
                        cell.x - grow,
                        cell.y - grow,
                        cell.w + grow * 2.0,
                        cell.h + grow * 2.0,
                    );
                    assert!(
                        inside(halo, l.window),
                        "the glow round pad {i}, {halo:?}, leaves the {w}x{h} window"
                    );
                    // Named separately, because "inside the window" would still
                    // hold if the halo swelled far enough to swallow a
                    // neighbouring pad.
                    for j in 0..PAD_COLS * PAD_ROWS {
                        let other = l.pad_rect(j);
                        if i == j || other.is_empty() {
                            continue;
                        }
                        assert!(
                            !overlaps(halo, other),
                            "the glow round pad {i} reaches pad {j} at {other:?} at {w}x{h}"
                        );
                    }
                }
            }
        }

        // And the drawn frame, on the handful of sizes, so the rule above is
        // known to be the rule the drawing actually follows.
        for (w, h) in WINDOWS {
            let mut app = game();
            app.tick(PRE_SEQUENCE_MS);
            assert!(app.lit().is_some(), "nothing is lit at {w}x{h}");
            let l = Layout::new(w, h);
            for (r, _) in fills(&app.frame(w, h)) {
                assert!(inside(r, l.window), "{r:?} leaves the {w}x{h} window");
            }
        }
    }

    #[test]
    fn every_pad_carries_its_colour_and_the_number_that_presses_it() {
        let l = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        let boxes = text_boxes(&game().frame(WINDOW_WIDTH, WINDOW_HEIGHT));
        for &colour in &SimonColor::ALL {
            let r = l.pad_rect(colour.index());
            let number = colour.index().saturating_add(1).to_string();
            for want in [colour.label().to_string(), number] {
                assert!(
                    boxes
                        .iter()
                        .any(|(body, b, _)| *body == want && inside(*b, r)),
                    "{want} is not on the {colour:?} pad at {r:?}"
                );
            }
        }
    }

    #[test]
    fn the_status_dot_takes_the_colour_of_whatever_is_lit() {
        let mut app = game();
        let l = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        let dot_side = (l.status.h * 0.34).min(l.small);
        let dark = Rect::new(
            l.status.x + l.pad,
            l.status.y + (l.status.h - dot_side) / 2.0,
            dot_side,
            dot_side,
        );
        assert_eq!(app.lit(), None);
        assert_eq!(
            fill_at(&app.frame(WINDOW_WIDTH, WINDOW_HEIGHT), dark),
            Some(COL_SURFACE1),
            "the dot is coloured with nothing lit"
        );

        app.tick(PRE_SEQUENCE_MS);
        let colour = app.lit().expect("a colour is being shown");
        let f = app.frame(WINDOW_WIDTH, WINDOW_HEIGHT);
        assert!(
            fills(&f)
                .iter()
                .any(|&(r, c)| c == colour.lit() && inside(r, l.status)),
            "nothing in the status band is the colour of the lit pad"
        );
    }

    #[test]
    fn the_tone_is_named_only_while_a_pad_is_lit() {
        // The sound this machine cannot make. The room it takes is given back to
        // the status line the rest of the time.
        let mut app = game();
        let quiet = texts(&app.frame(WINDOW_WIDTH, WINDOW_HEIGHT));
        for &colour in &SimonColor::ALL {
            assert!(
                !quiet.contains(&colour.tone().to_string()),
                "{:?} is named with nothing lit",
                colour.tone()
            );
        }
        app.tick(PRE_SEQUENCE_MS);
        let colour = app.lit().expect("a colour is being shown");
        let loud = texts(&app.frame(WINDOW_WIDTH, WINDOW_HEIGHT));
        assert!(
            loud.contains(&colour.tone().to_string()),
            "{colour:?} is lit and its tone is not named: {loud:?}"
        );
        for &other in &SimonColor::ALL {
            if other != colour {
                assert!(
                    !loud.contains(&other.tone().to_string()),
                    "{other:?} is named while {colour:?} is lit"
                );
            }
        }
    }

    // ── The panel over a lost game ──────────────────────────────────────────

    #[test]
    fn the_panel_reports_the_score_the_best_and_the_losses() {
        // Three losses deep, and the last game deliberately shorter than the
        // best, so all three numbers differ. With a score equal to the best --
        // which is what a single game always produces -- swapping the two rows
        // read identically and nothing here could tell.
        let mut app = game();
        advance_rounds(&mut app, 3);
        lose(&mut app);
        for _ in 0..2 {
            app.apply(Intent::NewGame);
            lose(&mut app);
        }
        app.apply(Intent::NewGame);
        advance_rounds(&mut app, 1);
        lose(&mut app);
        app.tick(ERROR_FLASH_MS);
        assert_eq!(
            (app.score, app.best, app.games_lost),
            (1, 3, 4),
            "the fixture no longer distinguishes the three rows"
        );
        let lines = texts(&app.frame(WINDOW_WIDTH, WINDOW_HEIGHT));
        for want in [
            "GAME OVER",
            "Score: 1 rounds",
            "Best: 3 rounds",
            "Games lost: 4",
        ] {
            assert!(
                lines.contains(&want.to_string()),
                "the panel does not say {want}: {lines:?}"
            );
        }
        assert!(
            lines.iter().any(|l| l.contains("play again")),
            "the panel does not say how to start another game"
        );
    }

    #[test]
    fn the_panel_is_not_drawn_over_the_pad_the_player_lost_on() {
        let mut app = game();
        lose(&mut app);
        let lines = texts(&app.frame(WINDOW_WIDTH, WINDOW_HEIGHT));
        assert!(
            !lines.contains(&"GAME OVER".to_string()),
            "the panel is up before the losing pad has been seen"
        );
        assert!(
            hits_for(&app.frame(WINDOW_WIDTH, WINDOW_HEIGHT), Target::GameOver).is_empty(),
            "the panel takes clicks before it is drawn"
        );
    }

    #[test]
    fn the_panels_rows_do_not_overwrite_one_another() {
        // Each row is sized against the band it is written in, not against the
        // panel, so a row taller than its band is a row written across the one
        // beneath it.
        for (w, h) in WINDOWS {
            let mut app = game();
            lose(&mut app);
            app.tick(ERROR_FLASH_MS);
            let panel = Layout::new(w, h).game_over();
            if panel.is_empty() {
                continue;
            }
            // By what the rows say rather than by where they are: the pads are
            // drawn behind the panel and their labels fall inside it, so a
            // filter on the box would compare the panel against the window it
            // is covering.
            let rows: Vec<(String, Rect, f32)> = text_boxes(&app.frame(w, h))
                .into_iter()
                .filter(|(body, _, _)| {
                    body == "GAME OVER"
                        || body.starts_with("Score: ")
                        || body.starts_with("Best: ")
                        || body.starts_with("Games lost: ")
                        || body.starts_with("Click here")
                })
                .collect();
            for (i, (a_body, a, _)) in rows.iter().enumerate() {
                for (b_body, b, _) in &rows[i + 1..] {
                    assert!(
                        !overlaps(*a, *b),
                        "{a_body:?} at {a:?} lies over {b_body:?} at {b:?} at {w}x{h}"
                    );
                }
            }
        }
    }

    // ── Type that fits, at every size and in every state ────────────────────

    #[test]
    fn every_line_of_type_stays_inside_the_window() {
        for (w, h) in WINDOWS {
            let window = Layout::new(w, h).window;
            for (name, app) in states() {
                for (body, r, size) in text_boxes(&app.frame(w, h)) {
                    assert!(
                        inside(r, window),
                        "{name}: {body:?} at {r:?} leaves the {w}x{h} window"
                    );
                    assert!(
                        size >= MIN_DRAWN_FONT,
                        "{name}: {body:?} is {size}px at {w}x{h}, below the floor the \
                         renderer will honour"
                    );
                    assert!(!body.is_empty(), "{name}: an empty line at {w}x{h}");
                }
            }
        }
    }

    #[test]
    fn a_window_with_no_room_for_words_shows_none() {
        // The renderer draws anything under a pixel a whole pixel high, so type
        // shrunk to fit a band it cannot fit comes out larger than the band. The
        // honest answer is to draw the pads and the colours and no words.
        for (name, app) in states() {
            let lines = texts(&app.frame(4.0, 4.0));
            assert!(
                lines.is_empty(),
                "{name} writes {lines:?} into a 4x4 window"
            );
        }
    }

    // ── The pulse ───────────────────────────────────────────────────────────

    #[test]
    fn a_redraw_without_a_tick_paints_exactly_the_same_window() {
        // The animation used to be driven by a counter raised once per draw, so
        // the game ran at whatever rate the compositor happened to be drawing
        // at -- and a window redrawn for a resize aged the game.
        let mut app = game();
        app.tick(PRE_SEQUENCE_MS + 100);
        let first = fills(&app.frame(WINDOW_WIDTH, WINDOW_HEIGHT));
        for _ in 0..30 {
            // Thrown away on purpose: the point of the loop is that drawing is
            // the thing that must not change the game.
            let _ = app.frame(WINDOW_WIDTH, WINDOW_HEIGHT);
        }
        assert_eq!(
            fills(&app.frame(WINDOW_WIDTH, WINDOW_HEIGHT)),
            first,
            "the window changes when it is merely drawn again"
        );
    }

    #[test]
    fn the_pulse_moves_with_the_clock_and_comes_round_once_a_period() {
        // A fresh game, so the clock is at a known place: the pause is 600ms and
        // the period 480, which puts the first sample a quarter of the way
        // through a beat and the second one halfway.
        let mut app = game();
        app.set_speed(Speed::Slow);
        app.tick(PRE_SEQUENCE_MS);
        assert!(
            app.lit().is_some(),
            "the pulse only beats while a pad is lit"
        );
        let (flash, _) = playback_ms(Speed::Slow);
        assert!(
            flash > PULSE_PERIOD_MS + PULSE_PERIOD_MS / 4,
            "this test needs one flash to outlast the beat it is measuring"
        );

        let quarter = fills(&app.frame(WINDOW_WIDTH, WINDOW_HEIGHT));
        app.tick(PULSE_PERIOD_MS / 4);
        let half = fills(&app.frame(WINDOW_WIDTH, WINDOW_HEIGHT));
        assert_ne!(quarter, half, "the pulse never moves");

        app.tick(PULSE_PERIOD_MS);
        assert_eq!(app.state, GameState::ShowSequence, "the flash ended early");
        assert_eq!(
            fills(&app.frame(WINDOW_WIDTH, WINDOW_HEIGHT)),
            half,
            "the pulse does not come round in a period"
        );

        // The other half of the rule, and the half nothing above holds: with no
        // pad lit the disc is a fixed size. `draw_status` picks between
        // `dot_side * grow` and a bare `dot_side` on exactly that condition, and
        // dropping the condition -- pulsing all the time -- would leave every
        // assertion above passing, because every one of them is taken with a
        // pad lit.
        let mut idle = game();
        assert_eq!(idle.lit(), None, "the pause should light nothing");
        let at_rest = fills(&idle.frame(WINDOW_WIDTH, WINDOW_HEIGHT));
        idle.tick(PULSE_PERIOD_MS / 4);
        assert_eq!(idle.lit(), None, "a quarter of a beat lit a pad");
        assert_eq!(
            fills(&idle.frame(WINDOW_WIDTH, WINDOW_HEIGHT)),
            at_rest,
            "the dot beats with nothing lit"
        );
    }

    // ── The window the desktop sees ─────────────────────────────────────────

    #[test]
    fn the_window_is_named_for_the_desktop_and_for_the_person() {
        let app = game();
        assert_eq!(app.title(), "Simon");
        assert_eq!(app.app_id(), "simon");
        // The id is what the taskbar groups by and the session store files
        // under, so it has to be a stable token rather than a caption.
        assert!(
            app.app_id()
                .chars()
                .all(|c| c.is_ascii_lowercase() || c == '-'),
            "the app id is not a token: {:?}",
            app.app_id()
        );
    }

    #[test]
    fn the_window_opens_at_the_size_its_layout_was_written_for() {
        let app = game();
        assert_eq!(
            app.initial_size(),
            (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)
        );
        assert_eq!(
            Simon::SIZE,
            (WINDOW_WIDTH, WINDOW_HEIGHT),
            "the size the tests click at is not the size the window opens at"
        );
    }

    #[test]
    fn the_timer_the_window_asks_for_is_the_one_the_game_wants() {
        // Two answers to "is anything moving" -- the interval the compositor is
        // given and the flag the game keeps -- would be a game that freezes on a
        // machine honouring the interval and runs on one that also delivers a
        // resize.
        for (name, app) in states() {
            assert_eq!(
                app.tick_interval(),
                app.wants_clock().then_some(TICK),
                "{name} asks for the wrong timer"
            );
        }
        let mut idle = game();
        to_player_input(&mut idle);
        assert_eq!(
            idle.tick_interval(),
            None,
            "a game waiting on a person keeps the machine awake"
        );
        assert_eq!(
            game().tick_interval(),
            Some(TICK),
            "the pause before the first sequence holds no timer"
        );
    }

    #[test]
    fn the_close_button_closes_the_window() {
        let mut app = game();
        assert_eq!(app.on_event(&Event::CloseRequested), Response::Exit);
    }

    #[test]
    fn an_event_that_changes_nothing_does_not_ask_for_a_frame() {
        // Every redraw is a buffer copied and a frame the compositor has to
        // composite -- a window that asks for one per stray keystroke costs
        // battery for nothing.
        let mut app = game();
        assert_eq!(
            app.on_event(&Event::Key(probe::press(Key::Q))),
            Response::Idle
        );
        assert_eq!(app.on_event(&Event::FocusIn), Response::Idle);
        assert_eq!(app.on_event(&Event::Moved { x: 3, y: 4 }), Response::Idle);
        assert_eq!(
            app.on_event(&Event::ScaleChanged { scale: 2.0 }),
            Response::Idle
        );
        assert_eq!(app.on_event(&Event::Tick { elapsed_ms: 0 }), Response::Idle);
    }

    #[test]
    fn an_event_that_changes_something_asks_for_a_frame() {
        let mut app = game();
        assert_eq!(
            app.on_event(&Event::Key(probe::press(Key::S))),
            Response::Redraw
        );
        assert_eq!(
            app.on_event(&Event::Tick { elapsed_ms: 16 }),
            Response::Redraw
        );
        assert_eq!(
            app.on_event(&Event::Resize {
                width: 900,
                height: 700
            }),
            Response::Redraw
        );
    }

    #[test]
    fn a_tick_event_carries_its_milliseconds_into_the_game() {
        // Asking only for `Response::Redraw` above is not enough: an `on_event`
        // that dropped `Event::Tick` on the floor would still redraw -- the arm
        // that catches everything else says `Redraw` too -- and the game would
        // simply stand still, with the pause before the sequence never ending
        // and no test able to say why.
        let mut app = game();
        let before = app.clock_ms;
        assert_eq!(app.state, GameState::PreSequence);
        assert_eq!(
            app.on_event(&Event::Tick {
                elapsed_ms: PRE_SEQUENCE_MS
            }),
            Response::Redraw
        );
        assert_eq!(
            app.clock_ms,
            before.saturating_add(PRE_SEQUENCE_MS),
            "the tick's milliseconds never reached the clock"
        );
        assert_eq!(
            app.state,
            GameState::ShowSequence,
            "a tick long enough to end the pause did not end it"
        );

        // And the amount matters, not merely that a tick arrived: half the pause
        // must leave the game in the pause.
        let mut app = game();
        assert_eq!(
            app.on_event(&Event::Tick {
                elapsed_ms: PRE_SEQUENCE_MS / 2
            }),
            Response::Redraw
        );
        assert_eq!(
            app.state,
            GameState::PreSequence,
            "half the pause ended the whole of it"
        );
    }

    #[test]
    fn a_resize_event_is_the_size_the_next_click_is_read_against() {
        let mut app = game();
        handle_event(
            &mut app,
            &Event::Resize {
                width: 1200,
                height: 900,
            },
        );
        assert_eq!((app.width, app.height), (1200.0, 900.0));
        let wide = Layout::new(1200.0, 900.0);
        let (cx, cy) = wide.pad_rect(3).centre();
        to_player_input(&mut app);
        assert_eq!(click(&mut app, cx, cy), EventResult::Consumed);
        assert_eq!(app.selected, 3, "the click was read against the old size");
    }

    #[test]
    fn drawing_the_window_remembers_the_size_it_was_drawn_at() {
        // `render` is the only place the compositor tells the game how big it
        // is, so a game that drew at the given size without storing it would
        // hit-test every click against the size it opened at.
        let mut app = game();
        app.render(1000.0, 800.0);
        assert_eq!((app.width, app.height), (1000.0, 800.0));
    }

    #[test]
    fn the_drawn_tree_carries_the_commands_the_frame_recorded() {
        // Not at the default size. `render` takes a width and a height, and a
        // `render` that ignored them and drew at `WINDOW_WIDTH`/`WINDOW_HEIGHT`
        // would agree with a frame taken at the default size exactly -- so
        // checking it there is checking nothing. 740x500 is neither the default
        // nor square, so a swapped or dropped argument shows up too.
        let mut app = game();
        for (w, h) in [(740.0, 500.0), (WINDOW_WIDTH, WINDOW_HEIGHT)] {
            let commands = app.frame(w, h).commands().len();
            assert!(commands > 0, "the window paints nothing at {w}x{h}");
            let tree = app.render(w, h);
            assert_eq!(
                tree.commands.len(),
                commands,
                "the tree handed to the compositor is not the frame drawn at {w}x{h}"
            );
            assert_eq!(
                format!("{:?}", tree.commands),
                format!("{:?}", app.frame(w, h).commands()),
                "the tree at {w}x{h} is a different window from the frame"
            );
        }
    }

    #[test]
    fn the_probe_draws_the_same_window_the_compositor_gets() {
        // Compared as text because a render command is not comparable: the point
        // is that the tests are not looking at a different window from the
        // player, and every field of every command is in the debug form.
        let app = game();
        assert_eq!(
            format!("{:?}", app.draw(Simon::SIZE).commands()),
            format!("{:?}", app.frame(WINDOW_WIDTH, WINDOW_HEIGHT).commands()),
            "the probe and the compositor are shown different windows"
        );
    }

    #[test]
    fn every_control_the_window_offers_can_be_reached_by_the_probe() {
        // The list the tests click through, so a control added without a hit box
        // is a name that appears here and cannot be clicked.
        let mut app = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
        to_player_input(&mut app);
        let names = probe::control_names(&app);
        // Truncated at the bracket, so all four pads read as `Pad` and the list
        // answers "can this window ever draw one", which is what it is for.
        for target in [
            Target::Pad(SimonColor::Red),
            Target::NewGame,
            Target::Speed,
            Target::Help,
        ] {
            assert!(
                names.contains(&probe::variant_name(target)),
                "{target:?} is not among {names:?}"
            );
        }
        assert_eq!(
            names
                .iter()
                .filter(|n| *n == &probe::variant_name(Target::Pad(SimonColor::Red)))
                .count(),
            SimonColor::ALL.len(),
            "not every pad is clickable: {names:?}"
        );
        for hidden in [Target::HelpSheet, Target::GameOver] {
            assert!(
                !names.contains(&probe::variant_name(hidden)),
                "{hidden:?} takes clicks while it is not drawn"
            );
        }

        // And the two that only appear over a game that has stopped.
        app.apply(Intent::ToggleHelp);
        assert!(
            probe::control_names(&app).contains(&probe::variant_name(Target::HelpSheet)),
            "the open sheet takes no clicks"
        );
        let mut lost = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
        lose(&mut lost);
        lost.tick(ERROR_FLASH_MS);
        assert!(
            probe::control_names(&lost).contains(&probe::variant_name(Target::GameOver)),
            "the panel takes no clicks"
        );
    }
}
