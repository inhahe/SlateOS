#![allow(clippy::too_many_lines)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::cast_possible_wrap)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::similar_names)]

//! Slate OS Yahtzee -- the dice game, in a window.
//!
//! Roll five dice up to three times a turn, holding the ones you want to keep,
//! and spend the result on one of thirteen scoring boxes. Full Yahtzee scoring:
//! the upper-section bonus at 63, the Joker rule, and 100 points for every
//! Yahtzee after the first. Playable with the keyboard or with the pointer.
//!
//! The whole picture is solved from the size the window reports each frame:
//! there is no built-in size the drawing falls back on, and every box a click
//! is tested against is one the drawing pass recorded.
//!
//! Themed with the Catppuccin Mocha palette.

use std::process::ExitCode;

use guitk::color::Color;
use guitk::event::{Event, EventResult, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::frame::{Frame, Rect};
use guitk::probe::Probe;
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::style::CornerRadii;
use guitk::text;
use oswindow::app::{self, App, Response};

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
const MAUVE: Color = Color::from_hex(0xCBA6F7);
const TEAL: Color = Color::from_hex(0x94E2D5);
const OVERLAY0: Color = Color::from_hex(0x6C7086);

// ── The size the window opens at ────────────────────────────────────
//
// The only two pixel counts in the file, and they are a *starting* size rather
// than a layout: everything below is solved from whatever size the window
// reports. What stood here was sixteen of them -- a 64-pixel die, a 320-pixel
// scorecard, a 28-pixel row, a high score pinned to `PADDING + 400.0` -- and
// the window was whatever they happened to add up to. `render` did take a
// width and a height, and spent them on the background rectangle and nothing
// else, so a wider window got a wider backdrop behind an unchanged picture and
// a shorter one lost its scorecard off the bottom.
const WINDOW_WIDTH: f32 = 820.0;
const WINDOW_HEIGHT: f32 = 700.0;

/// Total number of scoring categories.
const NUM_CATEGORIES: usize = 13;
/// Number of dice.
const NUM_DICE: usize = 5;
/// Maximum rolls per turn.
const MAX_ROLLS: u8 = 3;
/// Number of turns in a game.
const NUM_TURNS: usize = 13;
/// Upper section bonus threshold.
const UPPER_BONUS_THRESHOLD: u16 = 63;
/// Upper section bonus value.
const UPPER_BONUS_VALUE: u16 = 35;
/// Yahtzee bonus value.
const YAHTZEE_BONUS_VALUE: u16 = 100;
/// Full House score.
const FULL_HOUSE_SCORE: u16 = 25;
/// Small Straight score.
const SMALL_STRAIGHT_SCORE: u16 = 30;
/// Large Straight score.
const LARGE_STRAIGHT_SCORE: u16 = 40;
/// Yahtzee score.
const YAHTZEE_SCORE: u16 = 50;

/// The lines of key help along the bottom of the left column.
const HINTS: [&str; 4] = [
    "R: Roll  |  1-5: Hold/Release",
    "Tab: Switch focus  |  Arrows: Navigate",
    "Space/Enter: Hold die or Score category",
    "N: New Game",
];

// ── Scoring categories ─────────────────────────────────────────────

/// All 13 scoring categories in display order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Category {
    Ones,
    Twos,
    Threes,
    Fours,
    Fives,
    Sixes,
    ThreeOfAKind,
    FourOfAKind,
    FullHouse,
    SmallStraight,
    LargeStraight,
    Yahtzee,
    Chance,
}

impl Category {
    const ALL: [Category; NUM_CATEGORIES] = [
        Category::Ones,
        Category::Twos,
        Category::Threes,
        Category::Fours,
        Category::Fives,
        Category::Sixes,
        Category::ThreeOfAKind,
        Category::FourOfAKind,
        Category::FullHouse,
        Category::SmallStraight,
        Category::LargeStraight,
        Category::Yahtzee,
        Category::Chance,
    ];

    /// The category at `index`, or `None` past the end of the card.
    ///
    /// Folding the bounds check into the lookup is what lets every caller stop
    /// writing `if index >= NUM_CATEGORIES` a line away from the panic it was
    /// meant to prevent.
    fn at(index: usize) -> Option<Category> {
        Category::ALL.get(index).copied()
    }

    fn name(self) -> &'static str {
        match self {
            Category::Ones => "Ones",
            Category::Twos => "Twos",
            Category::Threes => "Threes",
            Category::Fours => "Fours",
            Category::Fives => "Fives",
            Category::Sixes => "Sixes",
            Category::ThreeOfAKind => "3 of a Kind",
            Category::FourOfAKind => "4 of a Kind",
            Category::FullHouse => "Full House",
            Category::SmallStraight => "Sm. Straight",
            Category::LargeStraight => "Lg. Straight",
            Category::Yahtzee => "Yahtzee",
            Category::Chance => "Chance",
        }
    }

    fn is_upper(self) -> bool {
        matches!(
            self,
            Category::Ones
                | Category::Twos
                | Category::Threes
                | Category::Fours
                | Category::Fives
                | Category::Sixes
        )
    }

    fn index(self) -> usize {
        Category::ALL
            .iter()
            .position(|&c| c == self)
            .unwrap_or_default()
    }
}

// ── Focus region ────────────────────────────────────────────────────

/// Which region of the UI currently has keyboard focus.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FocusRegion {
    Dice,
    Scorecard,
}

// ── Game phase ──────────────────────────────────────────────────────

/// Where the turn stands: derived, never stored.
///
/// This used to be a field, set by `roll` and by `advance_turn` alongside the
/// two counters it is a function of. Two representations of one fact are two
/// things that can disagree, and these did in practice: nothing in the drawing
/// or the input ever asked for `MustScore` -- the button read
/// `roll_number >= MAX_ROLLS` instead -- so the stored value was a third of it
/// dead and the rest a slower spelling of `turn_number >= NUM_TURNS`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GamePhase {
    /// Player needs to roll (start of turn, or between rolls).
    Rolling,
    /// All 3 rolls used; must pick a category.
    MustScore,
    /// Game is over (all 13 categories filled).
    GameOver,
}

// ── Randomness ──────────────────────────────────────────────────────
//
// From `randrange`, not a local LCG. The local one rolled a die with
// `state % 6 + 1`, and on a modulus-2^64 generator the low bit of `state`
// alternates 0,1,0,1 for ever. Six is even, and `x % 6` preserves the parity of
// `x`, so **adjacent dice always had opposite parity**. Five dice come from
// five consecutive draws, so every roll went odd, even, odd, even, odd — and
// therefore:
//
//   * a Yahtzee (five alike) was impossible;
//   * four alike was impossible too, since any four of five dice contain an
//     adjacent pair;
//   * three alike, at dice 1, 3 and 5, was the ceiling.
//
// Measured before the fix: **zero Yahtzees in 15 000 rolls** across three
// seeds, where about twelve are expected. The game's own name was unreachable
// and its Yahtzee-bonus branch was dead code. See `known-issues.md` and
// `design-decisions.md` §447.
use randrange::{RandomSource, SeededRng};

/// Roll one die: a value in `1..=6`.
///
/// A free function rather than a method, because a six-sided die is this
/// game's unit and not the generator's.
fn roll_die(rng: &mut SeededRng) -> u8 {
    // `below` reduces with the high bits, so nothing carries from one call to
    // the next. `try_from` cannot fail on 0..=5; a fallback that can never run
    // is still better than a cast that silently would.
    u8::try_from(rng.below(6)).unwrap_or(0).saturating_add(1)
}

// ── Scoring logic (pure functions) ──────────────────────────────────

/// Count how many times each die face appears. Returns array indexed 0..=6
/// (index 0 unused for clarity: counts[1] = count of ones, etc.).
fn face_counts(dice: &[u8; NUM_DICE]) -> [u8; 7] {
    let mut counts = [0u8; 7];
    for &d in dice {
        // A die outside 1..=6 has no column to be counted in, and asking the
        // array for one rather than checking the range first is the same test
        // written where it cannot drift from the access it guards.
        if let Some(c) = counts.get_mut(usize::from(d)).filter(|_| d >= 1) {
            *c = c.saturating_add(1);
        }
    }
    counts
}

/// The counts of the six real faces, without the unused zero column.
fn face_columns(counts: &[u8; 7]) -> impl Iterator<Item = u8> + '_ {
    counts.iter().skip(1).copied()
}

/// Sum of all dice.
fn dice_sum(dice: &[u8; NUM_DICE]) -> u16 {
    dice.iter()
        .map(|&d| u16::from(d))
        .fold(0, u16::saturating_add)
}

/// Score for an upper-section category (Ones-Sixes): sum of dice matching
/// the target face value.
fn score_upper(dice: &[u8; NUM_DICE], target: u8) -> u16 {
    dice.iter()
        .filter(|&&d| d == target)
        .map(|&d| u16::from(d))
        .fold(0, u16::saturating_add)
}

/// Returns true if the dice contain at least `n` of any single face.
fn has_n_of_a_kind(dice: &[u8; NUM_DICE], n: u8) -> bool {
    face_columns(&face_counts(dice)).any(|c| c >= n)
}

/// Score for Three of a Kind: sum of all dice if at least 3 match.
fn score_three_of_a_kind(dice: &[u8; NUM_DICE]) -> u16 {
    if has_n_of_a_kind(dice, 3) {
        dice_sum(dice)
    } else {
        0
    }
}

/// Score for Four of a Kind: sum of all dice if at least 4 match.
fn score_four_of_a_kind(dice: &[u8; NUM_DICE]) -> u16 {
    if has_n_of_a_kind(dice, 4) {
        dice_sum(dice)
    } else {
        0
    }
}

/// Score for Full House: 25 if exactly one face appears 3 times and another
/// appears 2 times. A Yahtzee (5 of a kind) does NOT count as a natural
/// full house unless the Joker rule applies.
fn score_full_house(dice: &[u8; NUM_DICE]) -> u16 {
    let counts = face_counts(dice);
    let has_three = face_columns(&counts).any(|c| c == 3);
    let has_two = face_columns(&counts).any(|c| c == 2);
    if has_three && has_two {
        FULL_HOUSE_SCORE
    } else {
        0
    }
}

/// Returns true if dice contain a consecutive run of length `len`.
fn has_consecutive_run(dice: &[u8; NUM_DICE], len: u8) -> bool {
    if len == 0 {
        return true;
    }
    let counts = face_counts(dice);
    // A run is a window of `len` adjacent faces with no gap in it. Sliding a
    // window over the six real columns says that directly; the old form
    // computed `7 - len` as a loop bound and indexed `start + offset`, which is
    // the same walk written in a way that can leave the array.
    let present: Vec<bool> = face_columns(&counts).map(|c| c > 0).collect();
    present
        .windows(usize::from(len))
        .any(|w| w.iter().all(|&p| p))
}

/// Score for Small Straight: 30 if dice contain 4 consecutive values.
fn score_small_straight(dice: &[u8; NUM_DICE]) -> u16 {
    if has_consecutive_run(dice, 4) {
        SMALL_STRAIGHT_SCORE
    } else {
        0
    }
}

/// Score for Large Straight: 40 if dice contain 5 consecutive values.
fn score_large_straight(dice: &[u8; NUM_DICE]) -> u16 {
    if has_consecutive_run(dice, 5) {
        LARGE_STRAIGHT_SCORE
    } else {
        0
    }
}

/// Score for Yahtzee: 50 if all five dice show the same face.
fn score_yahtzee(dice: &[u8; NUM_DICE]) -> u16 {
    if has_n_of_a_kind(dice, 5) {
        YAHTZEE_SCORE
    } else {
        0
    }
}

/// Score for Chance: sum of all dice.
fn score_chance(dice: &[u8; NUM_DICE]) -> u16 {
    dice_sum(dice)
}

/// Compute the potential score for a category given the current dice.
fn potential_score(dice: &[u8; NUM_DICE], category: Category) -> u16 {
    match category {
        Category::Ones => score_upper(dice, 1),
        Category::Twos => score_upper(dice, 2),
        Category::Threes => score_upper(dice, 3),
        Category::Fours => score_upper(dice, 4),
        Category::Fives => score_upper(dice, 5),
        Category::Sixes => score_upper(dice, 6),
        Category::ThreeOfAKind => score_three_of_a_kind(dice),
        Category::FourOfAKind => score_four_of_a_kind(dice),
        Category::FullHouse => score_full_house(dice),
        Category::SmallStraight => score_small_straight(dice),
        Category::LargeStraight => score_large_straight(dice),
        Category::Yahtzee => score_yahtzee(dice),
        Category::Chance => score_chance(dice),
    }
}

// ── Geometry ────────────────────────────────────────────────────────

/// Shrink a rect by `pad` on every side, never past empty.
fn inset(rect: Rect, pad: f32) -> Rect {
    Rect::new(
        rect.x + pad,
        rect.y + pad,
        (rect.w - pad * 2.0).max(0.0),
        (rect.h - pad * 2.0).max(0.0),
    )
}

/// Widen a `u32` the window hands us into the `f32` the layout works in.
fn f32_from_u32(v: u32) -> f32 {
    // `u32::MAX` is not representable exactly, but a window that wide does not
    // exist; every real size round-trips.
    v as f32
}

/// The bands the window is divided into, solved from its live size.
#[derive(Clone, Copy, Debug)]
struct Layout {
    window: Rect,
    /// The strip along the top holding the title, the turn and the high score.
    header: Rect,
    /// The left column: the dice, the roll button and the key help.
    left: Rect,
    /// The right column: the scorecard.
    card: Rect,
    /// The gap left around and inside every band.
    pad: f32,
    /// The title's font size.
    title: f32,
    /// The body font size.
    font: f32,
    /// The font size for labels and the key help.
    small: f32,
}

impl Layout {
    fn solve(w: f32, h: f32) -> Self {
        let w = w.max(0.0);
        let h = h.max(0.0);
        // Everything scales with the *smaller* side, so a wide-and-short window
        // gets small padding rather than padding that eats its whole height.
        let pad = (w.min(h) * 0.02).clamp(2.0, 16.0).min(w.min(h) / 2.0);
        let title = (h * 0.034).clamp(10.0, 28.0);
        let font = (h * 0.022).clamp(8.0, 17.0);
        let small = (h * 0.018).clamp(7.0, font);

        let header_h = h * 0.09;
        let header = Rect::new(0.0, 0.0, w, header_h);
        let body = Rect::new(0.0, header.bottom(), w, (h - header_h).max(0.0));

        // The scorecard is floored so its longest row -- a name, a gap and a
        // score -- still fits, and capped at half the window so it can never
        // crowd the dice off a narrow one.
        let card_w = (w * 0.44).clamp(170.0, 380.0).min(w / 2.0);
        let left = Rect::new(body.x, body.y, (body.w - card_w).max(0.0), body.h);
        let card = Rect::new(left.right(), body.y, card_w.min(body.w), body.h);

        Self {
            window: Rect::new(0.0, 0.0, w, h),
            header,
            left,
            card,
            pad,
            title,
            font,
            small,
        }
    }

    /// The three bands of the left column, bottom-anchored.
    ///
    /// The help is pinned to the floor, the button sits above it and the dice
    /// take whatever is left, so the help cannot slide under the window's edge
    /// and the button cannot land on top of the dice. All three used to be
    /// stacked downwards from a fixed top with no regard for the bottom.
    fn left_bands(self) -> (Rect, Rect, Rect) {
        let inner = inset(self.left, self.pad);
        let line = self.small * 1.5;
        let hints_h = (line * HINTS.len() as f32).min(inner.h);
        let hints = Rect::new(
            inner.x,
            (inner.bottom() - hints_h).max(inner.y),
            inner.w,
            hints_h,
        );
        let button_h = (self.font * 2.2).min((hints.y - inner.y).max(0.0));
        let button = Rect::new(
            inner.x,
            (hints.y - self.pad - button_h).max(inner.y),
            inner.w,
            button_h,
        );
        let dice = Rect::new(
            inner.x,
            inner.y,
            inner.w,
            (button.y - self.pad - inner.y).max(0.0),
        );
        (dice, button, hints)
    }
}

/// The five dice, fitted to the room the layout gave them.
///
/// A die is square whatever shape its area is: the side is the smaller of what
/// the width and the height allow, and the leftovers become margins. The old
/// die was 64 pixels with a 12-pixel gap and its pips 6 pixels across at a
/// fixed 16-pixel offset, so nothing about a die scaled with anything.
#[derive(Clone, Copy, Debug)]
struct Dice {
    /// Top-left of the first die.
    origin: (f32, f32),
    /// The side of one die.
    side: f32,
    /// The gap between two dice.
    gap: f32,
}

impl Dice {
    fn fit(area: Rect, label: f32) -> Self {
        let n = NUM_DICE as f32;
        // The gap is a share of the die rather than a constant, so five dice
        // in a narrow window shrink together instead of the gaps swallowing
        // them.
        let gap_share = 0.2;
        let by_w = (area.w / (n + gap_share * (n - 1.0))).max(0.0);
        // A die sits between the number above it and the HELD label below, so
        // the height it may use is what is left after both.
        let by_h = (area.h - label * 3.4).max(0.0);
        let side = by_w.min(by_h).max(0.0);
        let gap = side * gap_share;
        let row_w = side * n + gap * (n - 1.0);
        Self {
            origin: (
                area.x + (area.w - row_w).max(0.0) / 2.0,
                area.y + label * 1.7,
            ),
            side,
            gap,
        }
    }

    /// The box die `i` occupies on screen.
    fn die(self, i: usize) -> Rect {
        Rect::new(
            self.origin.0 + (self.side + self.gap) * i as f32,
            self.origin.1,
            self.side,
            self.side,
        )
    }

    /// The whole row, from the first die's left edge to the last one's right.
    fn row(self) -> Rect {
        let first = self.die(0);
        let last = self.die(NUM_DICE.saturating_sub(1));
        Rect::new(first.x, first.y, (last.right() - first.x).max(0.0), first.h)
    }
}

/// One row of the scorecard, in the order they are drawn.
///
/// The list is built once and both the painting and the hit boxes walk it. It
/// used to be described twice: `render_scorecard` advanced a running `row_y`
/// past the header, the two totals and the rule, while `category_display_row`
/// said the same thing over again as `cat_index + 3`. The click was tested
/// against the second description, so it agreed with the picture only for as
/// long as nobody inserted a row in one place and not the other -- and the
/// Yahtzee-bonus row, which appears only after a second Yahtzee, is a row that
/// does exactly that.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Row {
    /// The "Category / Score" heading.
    Head,
    /// A scoring box.
    Cat(usize),
    /// The upper section's running total against the bonus threshold.
    UpperTotal,
    /// The upper section's bonus.
    Bonus,
    /// The rule between the sections.
    Rule,
    /// The Yahtzee bonus tally, present only once one has been earned.
    YahtzeeBonus,
    /// The grand total.
    GrandTotal,
}

// ── Main game struct ────────────────────────────────────────────────

struct Yahtzee {
    /// Current dice values (1-6).
    dice: [u8; NUM_DICE],
    /// Which dice are held (true = held, not re-rolled).
    held: [bool; NUM_DICE],
    /// Current roll number within the turn (0 = not rolled yet, 1-3).
    roll_number: u8,
    /// Current turn number (0-based, 0..13).
    turn_number: usize,
    /// Scores for each category. `None` means not yet filled.
    scores: [Option<u16>; NUM_CATEGORIES],
    /// Number of Yahtzee bonuses earned.
    yahtzee_bonus_count: u16,
    /// Which UI region has keyboard focus.
    focus: FocusRegion,
    /// Currently selected die index (0..5) when focus is on dice.
    selected_die: usize,
    /// Currently selected category index (0..13) when focus is on scorecard.
    selected_category: usize,
    /// Highest score achieved across games.
    high_score: u16,
    /// RNG for dice rolls.
    rng: SeededRng,
    /// The size the last frame was drawn at, which is the size the next click
    /// is read against.
    width: f32,
    height: f32,
}

impl Yahtzee {
    fn new() -> Self {
        Self::with_seed(0xDEAD_BEEF_CAFE_1234)
    }

    fn with_seed(seed: u64) -> Self {
        Self {
            dice: [1; NUM_DICE],
            held: [false; NUM_DICE],
            roll_number: 0,
            turn_number: 0,
            scores: [None; NUM_CATEGORIES],
            yahtzee_bonus_count: 0,
            focus: FocusRegion::Dice,
            selected_die: 0,
            selected_category: 0,
            high_score: 0,
            rng: SeededRng::new(seed),
            width: WINDOW_WIDTH,
            height: WINDOW_HEIGHT,
        }
    }

    /// Remember the size the window last reported.
    fn resize(&mut self, width: f32, height: f32) {
        self.width = width.max(0.0);
        self.height = height.max(0.0);
    }

    // ── Game logic ─────────────────────────────────────────────────

    /// Where the turn stands. Derived from the two counters rather than stored
    /// beside them; see [`GamePhase`].
    fn phase(&self) -> GamePhase {
        if self.turn_number >= NUM_TURNS {
            GamePhase::GameOver
        } else if self.roll_number >= MAX_ROLLS {
            GamePhase::MustScore
        } else {
            GamePhase::Rolling
        }
    }

    /// Roll all un-held dice. Returns false if no rolls remain.
    fn roll(&mut self) -> bool {
        if self.phase() != GamePhase::Rolling {
            return false;
        }

        for i in 0..NUM_DICE {
            let held = self.held.get(i).copied().unwrap_or(false);
            if !held {
                let value = roll_die(&mut self.rng);
                if let Some(slot) = self.dice.get_mut(i) {
                    *slot = value;
                }
            }
        }

        self.roll_number = self.roll_number.saturating_add(1);
        true
    }

    /// Toggle the hold state of a die. Returns whether anything changed.
    fn toggle_hold(&mut self, index: usize) -> bool {
        // Held dice only mean anything between rolls: before the first there is
        // nothing to keep, and after the last there is nothing to re-roll.
        if self.roll_number == 0 || self.roll_number >= MAX_ROLLS {
            return false;
        }
        match self.held.get_mut(index) {
            Some(h) => {
                *h = !*h;
                true
            }
            None => false,
        }
    }

    /// Returns true if a Yahtzee is currently rolled.
    fn is_yahtzee(&self) -> bool {
        has_n_of_a_kind(&self.dice, 5)
    }

    /// The score already written in `index`'s box, if any.
    fn score_at(&self, index: usize) -> Option<u16> {
        self.scores.get(index).copied().flatten()
    }

    /// Returns whether the Yahtzee category has already been scored with
    /// a non-zero value.
    fn yahtzee_already_scored_nonzero(&self) -> bool {
        self.score_at(Category::Yahtzee.index())
            .is_some_and(|s| s > 0)
    }

    /// Check and award Yahtzee bonus: if the player already scored a Yahtzee
    /// (non-zero) and rolls another Yahtzee, they get 100 bonus points.
    fn check_yahtzee_bonus(&mut self) {
        if self.is_yahtzee() && self.yahtzee_already_scored_nonzero() {
            self.yahtzee_bonus_count = self.yahtzee_bonus_count.saturating_add(1);
        }
    }

    /// Attempt to score the selected category. Returns false if invalid.
    fn score_category(&mut self, cat_index: usize) -> bool {
        let Some(cat) = Category::at(cat_index) else {
            return false;
        };
        if self.roll_number == 0 {
            return false;
        }
        if self.score_at(cat_index).is_some() {
            return false;
        }

        // Check for Yahtzee bonus before scoring.
        self.check_yahtzee_bonus();

        // Joker rule: if this is a Yahtzee (all five dice same) and the
        // Yahtzee category is already scored, the player can use the Joker
        // rule: the corresponding upper category (matching the die face)
        // must be used if open. If that's also filled, any lower-section
        // category can be used with the normal rules, except Full House,
        // Small Straight, and Large Straight score their face values
        // (25, 30, 40 respectively) even though the dice wouldn't normally
        // qualify. This function applies the Joker scoring adjustment.
        let joker = self.is_yahtzee() && self.score_at(Category::Yahtzee.index()).is_some();
        let score = if joker {
            match cat {
                Category::FullHouse => FULL_HOUSE_SCORE,
                Category::SmallStraight => SMALL_STRAIGHT_SCORE,
                Category::LargeStraight => LARGE_STRAIGHT_SCORE,
                _ => potential_score(&self.dice, cat),
            }
        } else {
            potential_score(&self.dice, cat)
        };

        // The box is filled before the turn advances: writing the score
        // through a checked lookup means a box that turned out not to exist
        // costs the player nothing rather than costing them a turn.
        let Some(slot) = self.scores.get_mut(cat_index) else {
            return false;
        };
        *slot = Some(score);
        self.advance_turn();
        true
    }

    /// Advance to the next turn after scoring.
    fn advance_turn(&mut self) {
        self.turn_number = self.turn_number.saturating_add(1);
        self.roll_number = 0;
        self.held = [false; NUM_DICE];

        if self.phase() == GamePhase::GameOver {
            self.high_score = self.high_score.max(self.grand_total());
        }
    }

    /// Start a new game, preserving the high score.
    fn new_game(&mut self) {
        // Only the fields that describe a *game* are reset. This used to be
        // `*self = Self::with_seed(seed)` with the high score put back by hand
        // afterwards, which makes forgetting the default: every field is wiped
        // unless someone remembers to name it, and the window size this rewrite
        // added is exactly such a field -- a new game would have snapped the
        // layout back to the size the window opened at.
        self.rng = SeededRng::new(self.rng.next_u64());
        self.dice = [1; NUM_DICE];
        self.held = [false; NUM_DICE];
        self.roll_number = 0;
        self.turn_number = 0;
        self.scores = [None; NUM_CATEGORIES];
        self.yahtzee_bonus_count = 0;
        self.focus = FocusRegion::Dice;
        self.selected_die = 0;
        self.selected_category = 0;
    }

    // ── Score calculation ──────────────────────────────────────────

    /// Sum of the scored categories that `in_section` accepts.
    ///
    /// The section split is asked of [`Category::is_upper`] rather than
    /// written here as an index range.  Which section a category belongs to
    /// is a property of the category; spelling it a second time as `0..6`
    /// meant a reordering of `Category::ALL` would move a category between
    /// sections in one place and not the other, and the disagreement would
    /// show up as a wrong bonus rather than as an error.
    fn section_total(&self, in_section: impl Fn(Category) -> bool) -> u16 {
        Category::ALL
            .iter()
            .enumerate()
            .filter(|&(_, &c)| in_section(c))
            .filter_map(|(i, _)| self.score_at(i))
            .fold(0u16, u16::saturating_add)
    }

    /// Sum of the upper section (Ones-Sixes) scores.
    fn upper_total(&self) -> u16 {
        self.section_total(Category::is_upper)
    }

    /// Upper section bonus (35 if upper total >= 63).
    fn upper_bonus(&self) -> u16 {
        if self.upper_total() >= UPPER_BONUS_THRESHOLD {
            UPPER_BONUS_VALUE
        } else {
            0
        }
    }

    /// Sum of the lower section scores.
    fn lower_total(&self) -> u16 {
        self.section_total(|c| !c.is_upper())
    }

    /// Total Yahtzee bonus points.
    fn yahtzee_bonus_total(&self) -> u16 {
        // 656 bonuses would wrap a `u16`. That is a great many Yahtzees, but a
        // score that runs backwards is a worse answer than one that stops.
        self.yahtzee_bonus_count.saturating_mul(YAHTZEE_BONUS_VALUE)
    }

    /// Grand total score.
    fn grand_total(&self) -> u16 {
        [
            self.upper_total(),
            self.upper_bonus(),
            self.lower_total(),
            self.yahtzee_bonus_total(),
        ]
        .into_iter()
        .fold(0u16, u16::saturating_add)
    }

    // There was a `categories_filled` here -- how many of the thirteen boxes
    // hold a score -- with two tests and no caller.  It is redundant rather
    // than merely unused: `score_category` refuses a box that already holds
    // a score and then calls `advance_turn` exactly once, so the count is
    // `turn_number` by construction, and `turn_number >= NUM_TURNS` is what
    // actually ends the game.  Two ways to ask the same question is one more
    // than the game needs.  See known-issues.md lesson 45.

    // ── Input handling ─────────────────────────────────────────────

    /// Answer a key, and say whether the game used it.
    ///
    /// It used to return nothing at all, so a key the game ignored looked to
    /// the caller exactly like a move and every key release repainted the
    /// whole window.
    fn handle_key(&mut self, key: &KeyEvent) -> EventResult {
        if !key.pressed {
            return EventResult::Ignored;
        }

        match key.key {
            Key::R => {
                if !self.roll() {
                    return EventResult::Ignored;
                }
            }
            Key::N => self.new_game(),
            Key::Tab => {
                self.focus = match self.focus {
                    FocusRegion::Dice => FocusRegion::Scorecard,
                    FocusRegion::Scorecard => FocusRegion::Dice,
                };
            }
            Key::Left if self.focus == FocusRegion::Dice => {
                if self.selected_die == 0 {
                    return EventResult::Ignored;
                }
                self.selected_die = self.selected_die.saturating_sub(1);
            }
            Key::Right if self.focus == FocusRegion::Dice => {
                let last = NUM_DICE.saturating_sub(1);
                if self.selected_die >= last {
                    return EventResult::Ignored;
                }
                self.selected_die = self.selected_die.saturating_add(1).min(last);
            }
            Key::Up if self.focus == FocusRegion::Scorecard => {
                if self.selected_category == 0 {
                    return EventResult::Ignored;
                }
                self.selected_category = self.selected_category.saturating_sub(1);
            }
            Key::Down if self.focus == FocusRegion::Scorecard => {
                let last = NUM_CATEGORIES.saturating_sub(1);
                if self.selected_category >= last {
                    return EventResult::Ignored;
                }
                self.selected_category = self.selected_category.saturating_add(1).min(last);
            }
            Key::Space | Key::Enter => {
                let acted = match self.focus {
                    FocusRegion::Dice => self.toggle_hold(self.selected_die),
                    FocusRegion::Scorecard => self.score_category(self.selected_category),
                };
                if !acted {
                    return EventResult::Ignored;
                }
            }
            Key::Num1 | Key::Num2 | Key::Num3 | Key::Num4 | Key::Num5 => {
                let index = match key.key {
                    Key::Num1 => 0,
                    Key::Num2 => 1,
                    Key::Num3 => 2,
                    Key::Num4 => 3,
                    _ => 4,
                };
                if !self.toggle_hold(index) {
                    return EventResult::Ignored;
                }
            }
            _ => return EventResult::Ignored,
        }
        EventResult::Consumed
    }

    /// Answer a click by asking the frame what is under it.
    ///
    /// There was no mouse handling in the toolkit sense at all: the old
    /// `handle_mouse_click` recomputed the dice row, the button and the
    /// scorecard rows from the same constants the drawing used, a second copy
    /// of the geometry kept in step with the picture by nothing but care.
    fn click(&mut self, x: f32, y: f32, button: MouseButton) -> EventResult {
        // A card game answers the left button. Answering all three meant a
        // right-click scored a category, which is a turn the player cannot
        // take back.
        if button != MouseButton::Left {
            return EventResult::Ignored;
        }
        let Some(target) = self.frame(self.width, self.height).hit_test(x, y) else {
            return EventResult::Ignored;
        };
        match target {
            Target::Die(i) => {
                let moved = self.focus != FocusRegion::Dice || self.selected_die != i;
                self.focus = FocusRegion::Dice;
                self.selected_die = i;
                let held = self.toggle_hold(i);
                if moved || held {
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            Target::RollButton => {
                if self.phase() == GamePhase::GameOver {
                    self.new_game();
                    EventResult::Consumed
                } else if self.roll() {
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            Target::Category(i) => {
                let moved = self.focus != FocusRegion::Scorecard || self.selected_category != i;
                self.focus = FocusRegion::Scorecard;
                self.selected_category = i;
                let scored = self.score_category(i);
                if moved || scored {
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            Target::Title
            | Target::Turn
            | Target::High
            | Target::Hint(_)
            | Target::Tally(_)
            | Target::Scorecard => EventResult::Ignored,
        }
    }

    // ── Rendering ──────────────────────────────────────────────────

    /// The rows of the scorecard, in the order they are drawn.
    fn rows(&self) -> Vec<Row> {
        let mut rows = vec![Row::Head];
        let mut in_upper = true;
        for (i, cat) in Category::ALL.iter().enumerate() {
            if in_upper && !cat.is_upper() {
                // The upper section's tally and its bonus close it, and the
                // rule separates it from the lower one. Which section a
                // category is in is asked of the category, not of its index.
                rows.push(Row::UpperTotal);
                rows.push(Row::Bonus);
                rows.push(Row::Rule);
                in_upper = false;
            }
            rows.push(Row::Cat(i));
        }
        if self.yahtzee_bonus_count > 0 {
            rows.push(Row::YahtzeeBonus);
        }
        rows.push(Row::GrandTotal);
        rows
    }

    /// Draw the whole game at the size the window reports.
    ///
    /// Every box a click is tested against is recorded here as it is painted,
    /// so a hit test cannot disagree with the picture.
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
        self.draw_scorecard(&mut f, &l);
        self.draw_left(&mut f, &l);

        f.unclip();
        f
    }

    /// The title, the turn and roll counter, and the high score, laid left to
    /// right and measured rather than nudged.
    ///
    /// The counter used to be drawn at `PADDING + 130.0` and the high score at
    /// `PADDING + 400.0`: in a narrow window the high score was off the right
    /// edge entirely, and in a wide one all three huddled in the left quarter
    /// with the rest of the strip empty.
    fn draw_header(&self, f: &mut Frame<Target>, l: &Layout) {
        let band = inset(l.header, l.pad);
        if band.is_empty() {
            return;
        }

        let title_w = text::measure("Yahtzee", l.title, FontWeightHint::Bold);
        let title = Rect::new(band.x, band.y, title_w.min(band.w), band.h);
        f.push(RenderCommand::Text {
            x: title.x,
            y: title.y,
            text: String::from("Yahtzee"),
            color: LAVENDER,
            font_size: l.title,
            font_weight: FontWeightHint::Bold,
            max_width: Some(title.w),
            overflow: TextOverflow::Ellipsis,
        });
        f.hit(Target::Title, title);

        let turn_text = if self.phase() == GamePhase::GameOver {
            String::from("Game Over!")
        } else {
            format!(
                "Turn {}/{}  |  Roll {}/{}",
                self.turn_number.saturating_add(1),
                NUM_TURNS,
                self.roll_number,
                MAX_ROLLS
            )
        };
        let high_text = format!("High: {}", self.high_score);

        // The high score is anchored to the right edge and the turn counter
        // fills what is left between the title and it, so the three never
        // overlap and never drift apart.
        let high_w = text::measure(&high_text, l.font, FontWeightHint::Bold);
        let high = Rect::new(
            (band.right() - high_w).max(title.right() + l.pad),
            band.y,
            high_w.min(band.w),
            band.h,
        );
        let turn_x = title.right() + l.pad;
        let turn = Rect::new(turn_x, band.y, (high.x - l.pad - turn_x).max(0.0), band.h);

        f.push(RenderCommand::Text {
            x: turn.x,
            y: turn.y + (l.title - l.font).max(0.0) / 2.0,
            text: turn_text,
            color: SUBTEXT0,
            font_size: l.font,
            font_weight: FontWeightHint::Regular,
            max_width: Some(turn.w),
            overflow: TextOverflow::Ellipsis,
        });
        f.hit(Target::Turn, turn);

        f.push(RenderCommand::Text {
            x: high.x,
            y: high.y + (l.title - l.font).max(0.0) / 2.0,
            text: high_text,
            color: YELLOW,
            font_size: l.font,
            font_weight: FontWeightHint::Bold,
            max_width: Some(high.w),
            overflow: TextOverflow::Ellipsis,
        });
        f.hit(Target::High, high);
    }

    /// The dice, the roll button and the key help.
    fn draw_left(&self, f: &mut Frame<Target>, l: &Layout) {
        let (dice_area, button, hints) = l.left_bands();
        let d = Dice::fit(dice_area, l.small);

        for i in 0..NUM_DICE {
            self.draw_die(f, l, d, i);
        }

        self.draw_button(f, l, button, d);

        for (i, hint) in HINTS.iter().enumerate() {
            let line = l.small * 1.5;
            let row = Rect::new(
                hints.x,
                hints.y + line * i as f32,
                hints.w,
                line.min(hints.h),
            );
            if row.bottom() > hints.bottom() + 0.01 {
                // A line that does not fit is not drawn rather than drawn over
                // the window's edge.
                continue;
            }
            f.push(RenderCommand::Text {
                x: row.x,
                y: row.y,
                text: String::from(*hint),
                color: OVERLAY0,
                font_size: l.small,
                font_weight: FontWeightHint::Light,
                max_width: Some(row.w),
                overflow: TextOverflow::Ellipsis,
            });
            f.hit(Target::Hint(i), row);
        }
    }

    fn draw_die(&self, f: &mut Frame<Target>, l: &Layout, d: Dice, i: usize) {
        let die = d.die(i);
        if die.is_empty() {
            return;
        }
        let held = self.held.get(i).copied().unwrap_or(false);
        let selected = self.focus == FocusRegion::Dice && self.selected_die == i;

        let border = if selected {
            BLUE
        } else if held {
            PEACH
        } else {
            OVERLAY0
        };
        let ring = (d.side * 0.05).max(1.0);
        f.push(RenderCommand::FillRect {
            x: die.x - ring,
            y: die.y - ring,
            width: die.w + ring * 2.0,
            height: die.h + ring * 2.0,
            color: border,
            corner_radii: CornerRadii::all(d.side * 0.14 + ring),
        });
        f.push(RenderCommand::FillRect {
            x: die.x,
            y: die.y,
            width: die.w,
            height: die.h,
            color: if held { SURFACE1 } else { SURFACE0 },
            corner_radii: CornerRadii::all(d.side * 0.14),
        });

        if self.roll_number > 0 {
            let value = self.dice.get(i).copied().unwrap_or(1);
            self.draw_pips(f, die, value);
        }

        // The die's number above it, and HELD below it when it is held.
        f.push(RenderCommand::Text {
            x: die.centre().0 - text::measure("5", l.small, FontWeightHint::Regular) / 2.0,
            y: (die.y - l.small * 1.5).max(0.0),
            text: format!("{}", i.saturating_add(1)),
            color: OVERLAY0,
            font_size: l.small,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        if held {
            let label_w = text::measure("HELD", l.small, FontWeightHint::Bold);
            f.push(RenderCommand::Text {
                x: die.centre().0 - label_w / 2.0,
                y: die.bottom() + l.small * 0.4,
                text: String::from("HELD"),
                color: PEACH,
                font_size: l.small,
                font_weight: FontWeightHint::Bold,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
        }

        f.hit(Target::Die(i), die);
    }

    /// The pips of one die face, sized and spaced as a share of the die.
    fn draw_pips(&self, f: &mut Frame<Target>, die: Rect, value: u8) {
        let (cx, cy) = die.centre();
        let r = die.w * 0.09;
        let off = die.w * 0.25;
        // The layouts are written as which of the three columns and three rows
        // each face lights, so a face is a picture rather than a list of
        // hand-added offsets.
        let spots: &[(f32, f32)] = match value {
            1 => &[(0.0, 0.0)],
            2 => &[(-1.0, -1.0), (1.0, 1.0)],
            3 => &[(-1.0, -1.0), (0.0, 0.0), (1.0, 1.0)],
            4 => &[(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)],
            5 => &[
                (-1.0, -1.0),
                (1.0, -1.0),
                (0.0, 0.0),
                (-1.0, 1.0),
                (1.0, 1.0),
            ],
            6 => &[
                (-1.0, -1.0),
                (1.0, -1.0),
                (-1.0, 0.0),
                (1.0, 0.0),
                (-1.0, 1.0),
                (1.0, 1.0),
            ],
            _ => &[],
        };
        for &(sx, sy) in spots {
            f.push(RenderCommand::FillRect {
                x: cx + sx * off - r,
                y: cy + sy * off - r,
                width: r * 2.0,
                height: r * 2.0,
                color: TEXT_COLOR,
                corner_radii: CornerRadii::all(r),
            });
        }
    }

    fn draw_button(&self, f: &mut Frame<Target>, l: &Layout, band: Rect, d: Dice) {
        if band.is_empty() {
            return;
        }
        let (fill, label) = match self.phase() {
            GamePhase::GameOver => (GREEN, "New Game (N)"),
            GamePhase::MustScore => (OVERLAY0, "No Rolls Left"),
            GamePhase::Rolling => (BLUE, "Roll (R)"),
        };
        // The button is as wide as its widest legend rather than a constant, so
        // "No Rolls Left" cannot spill out of a box sized for "Roll (R)".
        let widest = ["New Game (N)", "No Rolls Left", "Roll (R)"]
            .into_iter()
            .map(|s| text::measure(s, l.font, FontWeightHint::Bold))
            .fold(0.0f32, f32::max);
        let width = (widest + l.pad * 3.0).min(band.w);
        // The button is centred on the dice rather than on the band, so it sits
        // under the row it rolls however much slack the band has beside it.
        let row = d.row();
        let x = (row.centre().0 - width / 2.0).clamp(band.x, (band.right() - width).max(band.x));
        let button = Rect::new(x, band.y, width, band.h);
        f.push(RenderCommand::FillRect {
            x: button.x,
            y: button.y,
            width: button.w,
            height: button.h,
            color: fill,
            corner_radii: CornerRadii::all(button.h * 0.2),
        });
        let label_w = text::measure(label, l.font, FontWeightHint::Bold);
        f.push(RenderCommand::Text {
            x: button.centre().0 - label_w / 2.0,
            y: button.centre().1 - l.font * 0.6,
            text: String::from(label),
            color: if self.phase() == GamePhase::MustScore {
                SUBTEXT0
            } else {
                CRUST
            },
            font_size: l.font,
            font_weight: FontWeightHint::Bold,
            max_width: Some(button.w),
            overflow: TextOverflow::Ellipsis,
        });
        f.hit(Target::RollButton, button);
    }

    /// The scorecard: one row per entry in [`Yahtzee::rows`].
    fn draw_scorecard(&self, f: &mut Frame<Target>, l: &Layout) {
        let area = inset(l.card, l.pad);
        if area.is_empty() {
            return;
        }
        let rows = self.rows();
        let count = rows.len().max(1) as f32;
        // The rows share the column's height, capped so that a tall window
        // gets a readable card rather than rows the height of a hand.
        let row_h = (area.h / count).min(l.font * 2.2);
        let card_h = row_h * count;

        f.push(RenderCommand::FillRect {
            x: area.x,
            y: area.y,
            width: area.w,
            height: card_h.min(area.h),
            color: MANTLE,
            corner_radii: CornerRadii::all(row_h * 0.3),
        });
        f.hit(Target::Scorecard, Rect::new(area.x, area.y, area.w, card_h));

        for (n, row) in rows.iter().enumerate() {
            let band = Rect::new(area.x, area.y + row_h * n as f32, area.w, row_h);
            if band.bottom() > area.bottom() + 0.01 {
                // The card ran out of window. A row drawn past the bottom edge
                // is a row painted over whatever the compositor puts there.
                break;
            }
            self.draw_row(f, l, band, *row);
        }
    }

    fn draw_row(&self, f: &mut Frame<Target>, l: &Layout, band: Rect, row: Row) {
        // The score column is a share of the row rather than a fixed 80 pixels
        // from its right edge, which at a narrow card left the name and the
        // number on top of one another.
        let score_w = (band.w * 0.32).min(band.w);
        let name_x = band.x + l.pad * 0.6;
        let score_x = band.right() - score_w;
        let text_y = band.y + (band.h - l.font).max(0.0) / 2.0;
        let pad = l.pad * 0.6;

        let fill = |f: &mut Frame<Target>, color: Color| {
            f.push(RenderCommand::FillRect {
                x: band.x,
                y: band.y,
                width: band.w,
                height: band.h,
                color,
                corner_radii: CornerRadii::ZERO,
            });
        };
        let write = |f: &mut Frame<Target>,
                     x: f32,
                     w: f32,
                     s: String,
                     color: Color,
                     weight: FontWeightHint,
                     size: f32| {
            f.push(RenderCommand::Text {
                x,
                y: text_y,
                text: s,
                color,
                font_size: size,
                font_weight: weight,
                max_width: Some((w - pad).max(0.0)),
                overflow: TextOverflow::Ellipsis,
            });
        };

        match row {
            Row::Head => {
                fill(f, SURFACE1);
                write(
                    f,
                    name_x,
                    score_x - name_x,
                    String::from("Category"),
                    TEXT_COLOR,
                    FontWeightHint::Bold,
                    l.font,
                );
                write(
                    f,
                    score_x,
                    score_w,
                    String::from("Score"),
                    TEXT_COLOR,
                    FontWeightHint::Bold,
                    l.font,
                );
            }
            Row::Cat(i) => {
                let Some(cat) = Category::at(i) else { return };
                let selected = self.focus == FocusRegion::Scorecard && self.selected_category == i;
                let filled = self.score_at(i);
                fill(
                    f,
                    if selected {
                        SURFACE1
                    } else if i.is_multiple_of(2) {
                        CRUST
                    } else {
                        MANTLE
                    },
                );
                if selected {
                    f.push(RenderCommand::FillRect {
                        x: band.x,
                        y: band.y,
                        width: (band.w * 0.012).max(2.0),
                        height: band.h,
                        color: BLUE,
                        corner_radii: CornerRadii::ZERO,
                    });
                }
                write(
                    f,
                    name_x,
                    score_x - name_x,
                    String::from(cat.name()),
                    if filled.is_some() {
                        SUBTEXT0
                    } else {
                        TEXT_COLOR
                    },
                    if selected {
                        FontWeightHint::Bold
                    } else {
                        FontWeightHint::Regular
                    },
                    l.font,
                );
                let (s, color) = match filled {
                    Some(v) => (format!("{v}"), if v > 0 { GREEN } else { RED }),
                    None if self.roll_number > 0 => {
                        let pot = potential_score(&self.dice, cat);
                        (format!("({pot})"), if pot > 0 { TEAL } else { OVERLAY0 })
                    }
                    None => (String::from("-"), OVERLAY0),
                };
                write(
                    f,
                    score_x,
                    score_w,
                    s,
                    color,
                    FontWeightHint::Regular,
                    l.font,
                );
                f.hit(Target::Category(i), band);
            }
            Row::UpperTotal => {
                fill(f, SURFACE0);
                write(
                    f,
                    name_x,
                    score_x - name_x,
                    String::from("Upper Total"),
                    SUBTEXT0,
                    FontWeightHint::Bold,
                    l.font,
                );
                let total = self.upper_total();
                write(
                    f,
                    score_x,
                    score_w,
                    format!("{total} / {UPPER_BONUS_THRESHOLD}"),
                    if total >= UPPER_BONUS_THRESHOLD {
                        GREEN
                    } else {
                        SUBTEXT0
                    },
                    FontWeightHint::Regular,
                    l.font,
                );
                f.hit(Target::Tally(Row::UpperTotal), band);
            }
            Row::Bonus => {
                fill(f, SURFACE0);
                write(
                    f,
                    name_x,
                    score_x - name_x,
                    String::from("Bonus"),
                    SUBTEXT0,
                    FontWeightHint::Bold,
                    l.font,
                );
                let bonus = self.upper_bonus();
                write(
                    f,
                    score_x,
                    score_w,
                    if bonus > 0 {
                        format!("+{bonus}")
                    } else {
                        String::from("-")
                    },
                    if bonus > 0 { GREEN } else { OVERLAY0 },
                    FontWeightHint::Regular,
                    l.font,
                );
                f.hit(Target::Tally(Row::Bonus), band);
            }
            Row::Rule => {
                f.push(RenderCommand::Line {
                    x1: band.x,
                    y1: band.centre().1,
                    x2: band.right(),
                    y2: band.centre().1,
                    color: SURFACE1,
                    width: 1.0,
                });
                f.hit(Target::Tally(Row::Rule), band);
            }
            Row::YahtzeeBonus => {
                fill(f, SURFACE0);
                write(
                    f,
                    name_x,
                    score_x - name_x,
                    format!("Yahtzee Bonus (x{})", self.yahtzee_bonus_count),
                    MAUVE,
                    FontWeightHint::Bold,
                    l.font,
                );
                write(
                    f,
                    score_x,
                    score_w,
                    format!("+{}", self.yahtzee_bonus_total()),
                    MAUVE,
                    FontWeightHint::Regular,
                    l.font,
                );
                f.hit(Target::Tally(Row::YahtzeeBonus), band);
            }
            Row::GrandTotal => {
                fill(f, SURFACE1);
                write(
                    f,
                    name_x,
                    score_x - name_x,
                    String::from("GRAND TOTAL"),
                    TEXT_COLOR,
                    FontWeightHint::Bold,
                    l.font,
                );
                write(
                    f,
                    score_x,
                    score_w,
                    format!("{}", self.grand_total()),
                    YELLOW,
                    FontWeightHint::Bold,
                    l.font,
                );
                f.hit(Target::Tally(Row::GrandTotal), band);
            }
        }
    }
}

// ── What a click can land on ────────────────────────────────────────

/// Everything the drawing pass records a box for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Target {
    Title,
    Turn,
    High,
    Die(usize),
    RollButton,
    Hint(usize),
    Category(usize),
    /// A scorecard row that is a tally rather than a box the player can spend.
    Tally(Row),
    /// The card behind the rows, so a click in its margin is not read as the
    /// row nearest to it.
    Scorecard,
}

// ── Event dispatch ──────────────────────────────────────────────────

fn handle_event(game: &mut Yahtzee, event: &Event) -> EventResult {
    match event {
        Event::Key(key) => game.handle_key(key),
        Event::Mouse(MouseEvent {
            x,
            y,
            kind: MouseEventKind::Press(button),
        }) => game.click(*x, *y, *button),
        Event::Resize { width, height } => {
            game.resize(f32_from_u32(*width), f32_from_u32(*height));
            EventResult::Consumed
        }
        _ => EventResult::Ignored,
    }
}

impl App for Yahtzee {
    fn title(&self) -> String {
        "Yahtzee".to_string()
    }

    fn app_id(&self) -> String {
        "yahtzee".to_string()
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

impl Probe for Yahtzee {
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
    let mut game = Yahtzee::new();
    app::launch("yahtzee", &mut game)
}

// ═══════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════
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

    /// Helper: create a game with a fixed seed for deterministic tests.
    fn test_game() -> Yahtzee {
        Yahtzee::with_seed(42)
    }

    /// Helper: create a game with dice pre-set to specific values.
    fn game_with_dice(dice: [u8; 5]) -> Yahtzee {
        let mut g = test_game();
        g.dice = dice;
        g.roll_number = 1; // Simulate having rolled once.
        g
    }

    /// Helper: simulate a key press.
    fn press_key(game: &mut Yahtzee, key: Key) -> EventResult {
        let event = Event::Key(KeyEvent {
            key,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        });
        handle_event(game, &event)
    }

    // ════════════════════════════════════════════════════════════════
    // Scoring: Upper Section
    // ════════════════════════════════════════════════════════════════

    #[test]
    fn test_score_ones_basic() {
        assert_eq!(score_upper(&[1, 1, 3, 4, 5], 1), 2);
    }

    #[test]
    fn test_score_ones_none() {
        assert_eq!(score_upper(&[2, 3, 4, 5, 6], 1), 0);
    }

    #[test]
    fn test_score_ones_all() {
        assert_eq!(score_upper(&[1, 1, 1, 1, 1], 1), 5);
    }

    #[test]
    fn test_score_twos() {
        assert_eq!(score_upper(&[2, 2, 2, 4, 5], 2), 6);
    }

    #[test]
    fn test_score_twos_none() {
        assert_eq!(score_upper(&[1, 3, 4, 5, 6], 2), 0);
    }

    #[test]
    fn test_score_threes() {
        assert_eq!(score_upper(&[3, 3, 3, 3, 5], 3), 12);
    }

    #[test]
    fn test_score_fours() {
        assert_eq!(score_upper(&[4, 4, 1, 2, 3], 4), 8);
    }

    #[test]
    fn test_score_fives() {
        assert_eq!(score_upper(&[5, 5, 5, 5, 5], 5), 25);
    }

    #[test]
    fn test_score_sixes() {
        assert_eq!(score_upper(&[6, 6, 1, 2, 3], 6), 12);
    }

    #[test]
    fn test_score_sixes_none() {
        assert_eq!(score_upper(&[1, 2, 3, 4, 5], 6), 0);
    }

    #[test]
    fn test_score_sixes_all() {
        assert_eq!(score_upper(&[6, 6, 6, 6, 6], 6), 30);
    }

    // ════════════════════════════════════════════════════════════════
    // Scoring: Three of a Kind
    // ════════════════════════════════════════════════════════════════

    #[test]
    fn test_three_of_a_kind_valid() {
        assert_eq!(score_three_of_a_kind(&[3, 3, 3, 4, 5]), 18);
    }

    #[test]
    fn test_three_of_a_kind_four_of_a_kind_counts() {
        assert_eq!(score_three_of_a_kind(&[2, 2, 2, 2, 5]), 13);
    }

    #[test]
    fn test_three_of_a_kind_yahtzee_counts() {
        assert_eq!(score_three_of_a_kind(&[4, 4, 4, 4, 4]), 20);
    }

    #[test]
    fn test_three_of_a_kind_invalid() {
        assert_eq!(score_three_of_a_kind(&[1, 2, 3, 4, 5]), 0);
    }

    #[test]
    fn test_three_of_a_kind_two_pair() {
        assert_eq!(score_three_of_a_kind(&[1, 1, 2, 2, 3]), 0);
    }

    // ════════════════════════════════════════════════════════════════
    // Scoring: Four of a Kind
    // ════════════════════════════════════════════════════════════════

    #[test]
    fn test_four_of_a_kind_valid() {
        assert_eq!(score_four_of_a_kind(&[5, 5, 5, 5, 3]), 23);
    }

    #[test]
    fn test_four_of_a_kind_yahtzee_counts() {
        assert_eq!(score_four_of_a_kind(&[6, 6, 6, 6, 6]), 30);
    }

    #[test]
    fn test_four_of_a_kind_invalid_three() {
        assert_eq!(score_four_of_a_kind(&[3, 3, 3, 4, 5]), 0);
    }

    #[test]
    fn test_four_of_a_kind_invalid_no_match() {
        assert_eq!(score_four_of_a_kind(&[1, 2, 3, 4, 5]), 0);
    }

    // ════════════════════════════════════════════════════════════════
    // Scoring: Full House
    // ════════════════════════════════════════════════════════════════

    #[test]
    fn test_full_house_valid() {
        assert_eq!(score_full_house(&[2, 2, 3, 3, 3]), FULL_HOUSE_SCORE);
    }

    #[test]
    fn test_full_house_reversed() {
        assert_eq!(score_full_house(&[6, 6, 6, 1, 1]), FULL_HOUSE_SCORE);
    }

    #[test]
    fn test_full_house_invalid_three_of_a_kind() {
        assert_eq!(score_full_house(&[3, 3, 3, 4, 5]), 0);
    }

    #[test]
    fn test_full_house_invalid_two_pair() {
        assert_eq!(score_full_house(&[1, 1, 2, 2, 3]), 0);
    }

    #[test]
    fn test_full_house_yahtzee_not_natural_full_house() {
        // A Yahtzee is NOT a natural full house (5 of same != 3+2 of different).
        assert_eq!(score_full_house(&[4, 4, 4, 4, 4]), 0);
    }

    #[test]
    fn test_full_house_invalid_all_different() {
        assert_eq!(score_full_house(&[1, 2, 3, 4, 5]), 0);
    }

    // ════════════════════════════════════════════════════════════════
    // Scoring: Small Straight
    // ════════════════════════════════════════════════════════════════

    #[test]
    fn test_small_straight_1234() {
        assert_eq!(score_small_straight(&[1, 2, 3, 4, 6]), SMALL_STRAIGHT_SCORE);
    }

    #[test]
    fn test_small_straight_2345() {
        assert_eq!(score_small_straight(&[2, 3, 4, 5, 1]), SMALL_STRAIGHT_SCORE);
    }

    #[test]
    fn test_small_straight_3456() {
        assert_eq!(score_small_straight(&[6, 5, 4, 3, 1]), SMALL_STRAIGHT_SCORE);
    }

    #[test]
    fn test_small_straight_with_duplicate() {
        assert_eq!(score_small_straight(&[1, 2, 3, 4, 4]), SMALL_STRAIGHT_SCORE);
    }

    #[test]
    fn test_small_straight_from_large() {
        assert_eq!(score_small_straight(&[1, 2, 3, 4, 5]), SMALL_STRAIGHT_SCORE);
    }

    #[test]
    fn test_small_straight_invalid() {
        assert_eq!(score_small_straight(&[1, 2, 3, 5, 6]), 0);
    }

    #[test]
    fn test_small_straight_invalid_pairs() {
        assert_eq!(score_small_straight(&[1, 1, 2, 2, 3]), 0);
    }

    #[test]
    fn test_small_straight_invalid_all_same() {
        assert_eq!(score_small_straight(&[3, 3, 3, 3, 3]), 0);
    }

    // ════════════════════════════════════════════════════════════════
    // Scoring: Large Straight
    // ════════════════════════════════════════════════════════════════

    #[test]
    fn test_large_straight_12345() {
        assert_eq!(score_large_straight(&[1, 2, 3, 4, 5]), LARGE_STRAIGHT_SCORE);
    }

    #[test]
    fn test_large_straight_23456() {
        assert_eq!(score_large_straight(&[2, 3, 4, 5, 6]), LARGE_STRAIGHT_SCORE);
    }

    #[test]
    fn test_large_straight_unordered() {
        assert_eq!(score_large_straight(&[5, 3, 1, 4, 2]), LARGE_STRAIGHT_SCORE);
    }

    #[test]
    fn test_large_straight_invalid_small() {
        assert_eq!(score_large_straight(&[1, 2, 3, 4, 4]), 0);
    }

    #[test]
    fn test_large_straight_invalid_gap() {
        assert_eq!(score_large_straight(&[1, 2, 3, 4, 6]), 0);
    }

    #[test]
    fn test_large_straight_invalid_all_same() {
        assert_eq!(score_large_straight(&[5, 5, 5, 5, 5]), 0);
    }

    // ════════════════════════════════════════════════════════════════
    // Scoring: Yahtzee
    // ════════════════════════════════════════════════════════════════

    #[test]
    fn test_yahtzee_ones() {
        assert_eq!(score_yahtzee(&[1, 1, 1, 1, 1]), YAHTZEE_SCORE);
    }

    #[test]
    fn test_yahtzee_sixes() {
        assert_eq!(score_yahtzee(&[6, 6, 6, 6, 6]), YAHTZEE_SCORE);
    }

    #[test]
    fn test_yahtzee_invalid_four() {
        assert_eq!(score_yahtzee(&[3, 3, 3, 3, 4]), 0);
    }

    #[test]
    fn test_yahtzee_invalid_all_different() {
        assert_eq!(score_yahtzee(&[1, 2, 3, 4, 5]), 0);
    }

    // ════════════════════════════════════════════════════════════════
    // Scoring: Chance
    // ════════════════════════════════════════════════════════════════

    #[test]
    fn test_chance_sum() {
        assert_eq!(score_chance(&[1, 2, 3, 4, 5]), 15);
    }

    #[test]
    fn test_chance_all_sixes() {
        assert_eq!(score_chance(&[6, 6, 6, 6, 6]), 30);
    }

    #[test]
    fn test_chance_all_ones() {
        assert_eq!(score_chance(&[1, 1, 1, 1, 1]), 5);
    }

    #[test]
    fn test_chance_mixed() {
        assert_eq!(score_chance(&[2, 3, 5, 5, 6]), 21);
    }

    // ════════════════════════════════════════════════════════════════
    // Scoring: potential_score dispatch
    // ════════════════════════════════════════════════════════════════

    #[test]
    fn test_potential_score_ones() {
        assert_eq!(potential_score(&[1, 1, 2, 3, 4], Category::Ones), 2);
    }

    #[test]
    fn test_potential_score_twos() {
        assert_eq!(potential_score(&[2, 2, 2, 1, 1], Category::Twos), 6);
    }

    #[test]
    fn test_potential_score_threes() {
        assert_eq!(potential_score(&[3, 3, 1, 1, 1], Category::Threes), 6);
    }

    #[test]
    fn test_potential_score_fours() {
        assert_eq!(potential_score(&[4, 4, 4, 4, 1], Category::Fours), 16);
    }

    #[test]
    fn test_potential_score_fives() {
        assert_eq!(potential_score(&[5, 5, 1, 2, 3], Category::Fives), 10);
    }

    #[test]
    fn test_potential_score_sixes() {
        assert_eq!(potential_score(&[6, 6, 6, 1, 2], Category::Sixes), 18);
    }

    #[test]
    fn test_potential_score_three_of_a_kind() {
        assert_eq!(
            potential_score(&[4, 4, 4, 2, 1], Category::ThreeOfAKind),
            15
        );
    }

    #[test]
    fn test_potential_score_four_of_a_kind() {
        assert_eq!(potential_score(&[5, 5, 5, 5, 2], Category::FourOfAKind), 22);
    }

    #[test]
    fn test_potential_score_full_house() {
        assert_eq!(
            potential_score(&[3, 3, 3, 2, 2], Category::FullHouse),
            FULL_HOUSE_SCORE
        );
    }

    #[test]
    fn test_potential_score_small_straight() {
        assert_eq!(
            potential_score(&[1, 2, 3, 4, 6], Category::SmallStraight),
            SMALL_STRAIGHT_SCORE
        );
    }

    #[test]
    fn test_potential_score_large_straight() {
        assert_eq!(
            potential_score(&[2, 3, 4, 5, 6], Category::LargeStraight),
            LARGE_STRAIGHT_SCORE
        );
    }

    #[test]
    fn test_potential_score_yahtzee() {
        assert_eq!(
            potential_score(&[2, 2, 2, 2, 2], Category::Yahtzee),
            YAHTZEE_SCORE
        );
    }

    #[test]
    fn test_potential_score_chance() {
        assert_eq!(potential_score(&[1, 2, 3, 4, 5], Category::Chance), 15);
    }

    // ════════════════════════════════════════════════════════════════
    // Helper: face_counts
    // ════════════════════════════════════════════════════════════════

    #[test]
    fn test_face_counts_all_ones() {
        let c = face_counts(&[1, 1, 1, 1, 1]);
        assert_eq!(c[1], 5);
        assert_eq!(c[2], 0);
    }

    #[test]
    fn test_face_counts_all_different() {
        let c = face_counts(&[1, 2, 3, 4, 5]);
        for i in 1..=5 {
            assert_eq!(c[i], 1);
        }
        assert_eq!(c[6], 0);
    }

    #[test]
    fn test_face_counts_mixed() {
        let c = face_counts(&[3, 3, 6, 6, 6]);
        assert_eq!(c[3], 2);
        assert_eq!(c[6], 3);
        assert_eq!(c[1], 0);
    }

    // ════════════════════════════════════════════════════════════════
    // Helper: dice_sum
    // ════════════════════════════════════════════════════════════════

    #[test]
    fn test_dice_sum_min() {
        assert_eq!(dice_sum(&[1, 1, 1, 1, 1]), 5);
    }

    #[test]
    fn test_dice_sum_max() {
        assert_eq!(dice_sum(&[6, 6, 6, 6, 6]), 30);
    }

    #[test]
    fn test_dice_sum_sequential() {
        assert_eq!(dice_sum(&[1, 2, 3, 4, 5]), 15);
    }

    // ════════════════════════════════════════════════════════════════
    // Helper: has_n_of_a_kind
    // ════════════════════════════════════════════════════════════════

    #[test]
    fn test_has_2_of_a_kind() {
        assert!(has_n_of_a_kind(&[1, 1, 2, 3, 4], 2));
    }

    #[test]
    fn test_has_3_of_a_kind() {
        assert!(has_n_of_a_kind(&[5, 5, 5, 2, 3], 3));
    }

    #[test]
    fn test_has_4_of_a_kind() {
        assert!(has_n_of_a_kind(&[4, 4, 4, 4, 1], 4));
    }

    #[test]
    fn test_has_5_of_a_kind() {
        assert!(has_n_of_a_kind(&[6, 6, 6, 6, 6], 5));
    }

    #[test]
    fn test_not_has_3_of_a_kind() {
        assert!(!has_n_of_a_kind(&[1, 2, 3, 4, 5], 3));
    }

    #[test]
    fn test_not_has_5_of_a_kind() {
        assert!(!has_n_of_a_kind(&[3, 3, 3, 3, 4], 5));
    }

    // ════════════════════════════════════════════════════════════════
    // Helper: has_consecutive_run
    // ════════════════════════════════════════════════════════════════

    #[test]
    fn test_has_run_4_valid() {
        assert!(has_consecutive_run(&[1, 2, 3, 4, 6], 4));
    }

    #[test]
    fn test_has_run_4_in_middle() {
        assert!(has_consecutive_run(&[2, 3, 4, 5, 1], 4));
    }

    #[test]
    fn test_has_run_5_valid() {
        assert!(has_consecutive_run(&[1, 2, 3, 4, 5], 5));
    }

    #[test]
    fn test_has_run_4_invalid() {
        assert!(!has_consecutive_run(&[1, 2, 3, 5, 6], 4));
    }

    #[test]
    fn test_has_run_5_invalid() {
        assert!(!has_consecutive_run(&[1, 2, 3, 4, 4], 5));
    }

    // ════════════════════════════════════════════════════════════════
    // Game state: initialization
    // ════════════════════════════════════════════════════════════════

    #[test]
    fn test_initial_dice_values() {
        let g = test_game();
        for &d in &g.dice {
            assert_eq!(d, 1); // Initial placeholder before rolling.
        }
    }

    #[test]
    fn test_initial_no_dice_held() {
        let g = test_game();
        assert!(!g.held.iter().any(|&h| h));
    }

    #[test]
    fn test_initial_roll_number_zero() {
        let g = test_game();
        assert_eq!(g.roll_number, 0);
    }

    #[test]
    fn test_initial_turn_number_zero() {
        let g = test_game();
        assert_eq!(g.turn_number, 0);
    }

    #[test]
    fn test_initial_no_scores() {
        let g = test_game();
        assert!(g.scores.iter().all(|s| s.is_none()));
    }

    #[test]
    fn test_initial_phase_rolling() {
        let g = test_game();
        assert_eq!(g.phase(), GamePhase::Rolling);
    }

    #[test]
    fn test_initial_focus_dice() {
        let g = test_game();
        assert_eq!(g.focus, FocusRegion::Dice);
    }

    #[test]
    fn test_initial_selected_die_zero() {
        let g = test_game();
        assert_eq!(g.selected_die, 0);
    }

    #[test]
    fn test_initial_high_score_zero() {
        let g = test_game();
        assert_eq!(g.high_score, 0);
    }

    #[test]
    fn test_initial_yahtzee_bonus_zero() {
        let g = test_game();
        assert_eq!(g.yahtzee_bonus_count, 0);
    }

    // ════════════════════════════════════════════════════════════════
    // Game logic: rolling
    // ════════════════════════════════════════════════════════════════

    #[test]
    fn test_roll_changes_dice() {
        let mut g = test_game();
        let before = g.dice;
        g.roll();
        // With the LCG, at least some dice should differ from all-1 initial.
        assert_ne!(g.dice, before);
    }

    #[test]
    fn test_roll_increments_roll_number() {
        let mut g = test_game();
        g.roll();
        assert_eq!(g.roll_number, 1);
        g.roll();
        assert_eq!(g.roll_number, 2);
        g.roll();
        assert_eq!(g.roll_number, 3);
    }

    #[test]
    fn test_fourth_roll_fails() {
        let mut g = test_game();
        g.roll();
        g.roll();
        g.roll();
        assert!(!g.roll());
        assert_eq!(g.roll_number, 3);
    }

    #[test]
    fn test_roll_respects_held_dice() {
        let mut g = test_game();
        g.roll(); // First roll sets dice.
        let val = g.dice[2];
        g.held[2] = true;
        g.roll(); // Second roll.
        assert_eq!(g.dice[2], val); // Held die unchanged.
    }

    #[test]
    fn test_after_three_rolls_phase_is_must_score() {
        let mut g = test_game();
        g.roll();
        g.roll();
        g.roll();
        assert_eq!(g.phase(), GamePhase::MustScore);
    }

    #[test]
    fn test_dice_values_in_range() {
        let mut g = test_game();
        for _ in 0..100 {
            g.roll_number = 0; // Reset to allow rolling.
            g.held = [false; 5];
            g.roll();
            for &d in &g.dice {
                assert!((1..=6).contains(&d), "Die value out of range: {d}");
            }
        }
    }

    #[test]
    fn test_roll_returns_true_on_success() {
        let mut g = test_game();
        assert!(g.roll());
    }

    #[test]
    fn test_cannot_roll_when_game_over() {
        let mut g = test_game();
        g.turn_number = NUM_TURNS;
        assert!(!g.roll());
    }

    // ════════════════════════════════════════════════════════════════
    // Game logic: holding
    // ════════════════════════════════════════════════════════════════

    #[test]
    fn test_toggle_hold() {
        let mut g = game_with_dice([1, 2, 3, 4, 5]);
        assert!(!g.held[0]);
        g.toggle_hold(0);
        assert!(g.held[0]);
        g.toggle_hold(0);
        assert!(!g.held[0]);
    }

    #[test]
    fn test_cannot_hold_before_first_roll() {
        let mut g = test_game();
        // roll_number is 0 before rolling.
        g.toggle_hold(0);
        assert!(!g.held[0]);
    }

    #[test]
    fn test_cannot_hold_after_third_roll() {
        let mut g = test_game();
        g.roll();
        g.roll();
        g.roll();
        // roll_number is 3, can no longer toggle.
        g.toggle_hold(0);
        assert!(!g.held[0]);
    }

    #[test]
    fn test_hold_out_of_range_ignored() {
        let mut g = game_with_dice([1, 2, 3, 4, 5]);
        g.toggle_hold(5); // Out of range.
        // Should not panic.
    }

    #[test]
    fn test_hold_multiple_dice() {
        let mut g = game_with_dice([1, 2, 3, 4, 5]);
        g.toggle_hold(0);
        g.toggle_hold(2);
        g.toggle_hold(4);
        assert!(g.held[0]);
        assert!(!g.held[1]);
        assert!(g.held[2]);
        assert!(!g.held[3]);
        assert!(g.held[4]);
    }

    // ════════════════════════════════════════════════════════════════
    // Game logic: scoring a category
    // ════════════════════════════════════════════════════════════════

    #[test]
    fn test_score_category_basic() {
        let mut g = game_with_dice([1, 1, 1, 2, 3]);
        assert!(g.score_category(Category::Ones.index()));
        assert_eq!(g.scores[Category::Ones.index()], Some(3));
    }

    #[test]
    fn test_score_category_advances_turn() {
        let mut g = game_with_dice([1, 2, 3, 4, 5]);
        g.score_category(Category::Chance.index());
        assert_eq!(g.turn_number, 1);
        assert_eq!(g.roll_number, 0);
    }

    #[test]
    fn test_score_category_resets_held() {
        let mut g = game_with_dice([1, 2, 3, 4, 5]);
        g.held = [true, true, false, false, false];
        g.score_category(Category::Chance.index());
        assert!(!g.held.iter().any(|&h| h));
    }

    #[test]
    fn test_cannot_score_same_category_twice() {
        let mut g = game_with_dice([1, 1, 1, 2, 3]);
        g.score_category(Category::Ones.index());
        // Try again after rolling in a new turn.
        g.roll();
        assert!(!g.score_category(Category::Ones.index()));
    }

    #[test]
    fn test_cannot_score_without_rolling() {
        let mut g = test_game();
        assert!(!g.score_category(Category::Chance.index()));
    }

    #[test]
    fn test_cannot_score_out_of_range() {
        let mut g = game_with_dice([1, 2, 3, 4, 5]);
        assert!(!g.score_category(NUM_CATEGORIES));
    }

    #[test]
    fn test_score_zero_for_unmatched() {
        let mut g = game_with_dice([2, 3, 4, 5, 6]);
        g.score_category(Category::Ones.index());
        assert_eq!(g.scores[Category::Ones.index()], Some(0));
    }

    // ════════════════════════════════════════════════════════════════
    // Game logic: turn advancement and game over
    // ════════════════════════════════════════════════════════════════

    #[test]
    fn test_game_over_after_13_turns() {
        let mut g = game_with_dice([1, 2, 3, 4, 5]);
        for i in 0..NUM_CATEGORIES {
            g.dice = [1, 2, 3, 4, 5];
            g.roll_number = 1;
            g.score_category(i);
        }
        assert_eq!(g.phase(), GamePhase::GameOver);
        assert_eq!(g.turn_number, NUM_TURNS);
    }

    /// The turn counter and the filled boxes never disagree.
    ///
    /// This replaces `test_categories_filled_count`, which exercised a
    /// `categories_filled()` helper nothing called.  The reason that helper
    /// was safe to delete is precisely this invariant -- one box filled per
    /// turn, no box filled twice -- so the invariant is what is asserted
    /// now, against the field the game really ends on.
    #[test]
    fn the_turn_counter_equals_the_number_of_filled_boxes() {
        let mut g = game_with_dice([1, 2, 3, 4, 5]);
        assert_eq!(g.turn_number, 0);
        assert_eq!(g.scores.iter().filter(|s| s.is_some()).count(), 0);

        g.score_category(0);
        g.dice = [1, 2, 3, 4, 5];
        g.roll_number = 1;
        g.score_category(1);
        // A repeat of a box already scored must change neither.
        g.dice = [1, 2, 3, 4, 5];
        g.roll_number = 1;
        assert!(!g.score_category(1), "a filled box was scored twice");

        assert_eq!(g.turn_number, 2);
        assert_eq!(g.scores.iter().filter(|s| s.is_some()).count(), 2);
    }

    // ════════════════════════════════════════════════════════════════
    // Game logic: upper section bonus
    // ════════════════════════════════════════════════════════════════

    #[test]
    fn test_upper_bonus_not_met() {
        let g = test_game();
        assert_eq!(g.upper_bonus(), 0);
    }

    #[test]
    fn test_upper_bonus_exactly_63() {
        let mut g = test_game();
        // 3*1 + 3*2 + 3*3 + 3*4 + 3*5 + 3*6 = 3+6+9+12+15+18 = 63
        g.scores[0] = Some(3);
        g.scores[1] = Some(6);
        g.scores[2] = Some(9);
        g.scores[3] = Some(12);
        g.scores[4] = Some(15);
        g.scores[5] = Some(18);
        assert_eq!(g.upper_total(), 63);
        assert_eq!(g.upper_bonus(), UPPER_BONUS_VALUE);
    }

    #[test]
    fn test_upper_bonus_above_63() {
        let mut g = test_game();
        g.scores[0] = Some(5); // Five ones.
        g.scores[1] = Some(10);
        g.scores[2] = Some(15);
        g.scores[3] = Some(20);
        g.scores[4] = Some(25);
        g.scores[5] = Some(30);
        assert!(g.upper_total() > UPPER_BONUS_THRESHOLD);
        assert_eq!(g.upper_bonus(), UPPER_BONUS_VALUE);
    }

    #[test]
    fn test_upper_bonus_below_63() {
        let mut g = test_game();
        g.scores[0] = Some(1);
        g.scores[1] = Some(2);
        g.scores[2] = Some(3);
        g.scores[3] = Some(4);
        g.scores[4] = Some(5);
        g.scores[5] = Some(6);
        assert_eq!(g.upper_total(), 21);
        assert_eq!(g.upper_bonus(), 0);
    }

    // ════════════════════════════════════════════════════════════════
    // Game logic: Yahtzee bonus
    // ════════════════════════════════════════════════════════════════

    #[test]
    fn test_yahtzee_bonus_awarded() {
        let mut g = game_with_dice([3, 3, 3, 3, 3]);
        // First, score the Yahtzee category.
        g.score_category(Category::Yahtzee.index());
        assert_eq!(g.yahtzee_bonus_count, 0);

        // Roll another Yahtzee and score something else.
        g.dice = [5, 5, 5, 5, 5];
        g.roll_number = 1;
        g.score_category(Category::Fives.index());
        // Bonus should have been awarded.
        assert_eq!(g.yahtzee_bonus_count, 1);
    }

    #[test]
    fn test_no_yahtzee_bonus_if_first_yahtzee_scored_zero() {
        let mut g = game_with_dice([1, 2, 3, 4, 5]);
        // Score Yahtzee as zero (not a Yahtzee).
        g.score_category(Category::Yahtzee.index());
        assert_eq!(g.scores[Category::Yahtzee.index()], Some(0));

        // Now roll a Yahtzee.
        g.dice = [4, 4, 4, 4, 4];
        g.roll_number = 1;
        g.score_category(Category::Chance.index());
        // No bonus because first Yahtzee was scored as 0.
        assert_eq!(g.yahtzee_bonus_count, 0);
    }

    #[test]
    fn test_yahtzee_bonus_total() {
        let mut g = test_game();
        g.yahtzee_bonus_count = 3;
        assert_eq!(g.yahtzee_bonus_total(), 300);
    }

    #[test]
    fn test_multiple_yahtzee_bonuses() {
        let mut g = game_with_dice([2, 2, 2, 2, 2]);
        // Score Yahtzee category first.
        g.score_category(Category::Yahtzee.index());

        // Second Yahtzee.
        g.dice = [3, 3, 3, 3, 3];
        g.roll_number = 1;
        g.score_category(Category::ThreeOfAKind.index());
        assert_eq!(g.yahtzee_bonus_count, 1);

        // Third Yahtzee.
        g.dice = [4, 4, 4, 4, 4];
        g.roll_number = 1;
        g.score_category(Category::FourOfAKind.index());
        assert_eq!(g.yahtzee_bonus_count, 2);
    }

    // ════════════════════════════════════════════════════════════════
    // Game logic: joker rule
    // ════════════════════════════════════════════════════════════════

    #[test]
    fn test_joker_full_house_gets_25() {
        let mut g = game_with_dice([4, 4, 4, 4, 4]);
        // Score Yahtzee first.
        g.score_category(Category::Yahtzee.index());

        // Roll another Yahtzee, score Full House via joker rule.
        g.dice = [4, 4, 4, 4, 4];
        g.roll_number = 1;
        g.score_category(Category::FullHouse.index());
        assert_eq!(
            g.scores[Category::FullHouse.index()],
            Some(FULL_HOUSE_SCORE)
        );
    }

    #[test]
    fn test_joker_small_straight_gets_30() {
        let mut g = game_with_dice([5, 5, 5, 5, 5]);
        g.score_category(Category::Yahtzee.index());

        g.dice = [5, 5, 5, 5, 5];
        g.roll_number = 1;
        g.score_category(Category::SmallStraight.index());
        assert_eq!(
            g.scores[Category::SmallStraight.index()],
            Some(SMALL_STRAIGHT_SCORE)
        );
    }

    #[test]
    fn test_joker_large_straight_gets_40() {
        let mut g = game_with_dice([6, 6, 6, 6, 6]);
        g.score_category(Category::Yahtzee.index());

        g.dice = [6, 6, 6, 6, 6];
        g.roll_number = 1;
        g.score_category(Category::LargeStraight.index());
        assert_eq!(
            g.scores[Category::LargeStraight.index()],
            Some(LARGE_STRAIGHT_SCORE)
        );
    }

    // ════════════════════════════════════════════════════════════════
    // Game logic: grand total
    // ════════════════════════════════════════════════════════════════

    #[test]
    fn test_grand_total_empty() {
        let g = test_game();
        assert_eq!(g.grand_total(), 0);
    }

    #[test]
    fn test_grand_total_with_upper_and_lower() {
        let mut g = test_game();
        g.scores[Category::Ones.index()] = Some(3);
        g.scores[Category::Chance.index()] = Some(20);
        assert_eq!(g.grand_total(), 23);
    }

    #[test]
    fn test_grand_total_includes_bonus() {
        let mut g = test_game();
        g.scores[0] = Some(3);
        g.scores[1] = Some(6);
        g.scores[2] = Some(9);
        g.scores[3] = Some(12);
        g.scores[4] = Some(15);
        g.scores[5] = Some(18);
        // 63 + 35 bonus = 98
        assert_eq!(g.grand_total(), 98);
    }

    #[test]
    fn test_grand_total_includes_yahtzee_bonus() {
        let mut g = test_game();
        g.scores[Category::Yahtzee.index()] = Some(50);
        g.yahtzee_bonus_count = 2;
        assert_eq!(g.grand_total(), 250); // 50 + 200
    }

    // ════════════════════════════════════════════════════════════════
    // Game logic: new game
    // ════════════════════════════════════════════════════════════════

    #[test]
    fn test_new_game_resets_scores() {
        let mut g = game_with_dice([1, 2, 3, 4, 5]);
        g.score_category(0);
        g.new_game();
        assert!(g.scores.iter().all(|s| s.is_none()));
    }

    #[test]
    fn test_new_game_preserves_high_score() {
        let mut g = test_game();
        g.high_score = 300;
        g.new_game();
        assert_eq!(g.high_score, 300);
    }

    #[test]
    fn test_new_game_resets_turn() {
        let mut g = game_with_dice([1, 2, 3, 4, 5]);
        g.score_category(0);
        g.new_game();
        assert_eq!(g.turn_number, 0);
        assert_eq!(g.roll_number, 0);
    }

    #[test]
    fn test_new_game_resets_phase() {
        let mut g = test_game();
        g.turn_number = NUM_TURNS;
        g.new_game();
        assert_eq!(g.phase(), GamePhase::Rolling);
    }

    #[test]
    fn test_new_game_resets_held() {
        let mut g = game_with_dice([1, 2, 3, 4, 5]);
        g.held = [true; 5];
        g.new_game();
        assert!(!g.held.iter().any(|&h| h));
    }

    #[test]
    fn test_new_game_resets_yahtzee_bonus() {
        let mut g = test_game();
        g.yahtzee_bonus_count = 3;
        g.new_game();
        assert_eq!(g.yahtzee_bonus_count, 0);
    }

    // ════════════════════════════════════════════════════════════════
    // Game logic: high score tracking
    // ════════════════════════════════════════════════════════════════

    #[test]
    fn test_high_score_set_on_game_over() {
        let mut g = test_game();
        // Play a full game with all Chance scores.
        for i in 0..NUM_CATEGORIES {
            g.dice = [6, 6, 6, 6, 6];
            g.roll_number = 1;
            g.score_category(i);
        }
        assert!(g.high_score > 0);
    }

    #[test]
    fn test_high_score_only_increases() {
        let mut g = test_game();
        // First game: high scores.
        for i in 0..NUM_CATEGORIES {
            g.dice = [6, 6, 6, 6, 6];
            g.roll_number = 1;
            g.score_category(i);
        }
        let first_high = g.high_score;

        g.new_game();

        // Second game: low scores.
        for i in 0..NUM_CATEGORIES {
            g.dice = [1, 1, 1, 1, 1];
            g.roll_number = 1;
            g.score_category(i);
        }
        // High score should not decrease.
        assert!(g.high_score >= first_high);
    }

    // ════════════════════════════════════════════════════════════════
    // Keyboard input
    // ════════════════════════════════════════════════════════════════

    #[test]
    fn test_key_r_rolls() {
        let mut g = test_game();
        press_key(&mut g, Key::R);
        assert_eq!(g.roll_number, 1);
    }

    #[test]
    fn test_key_n_new_game() {
        let mut g = game_with_dice([1, 2, 3, 4, 5]);
        g.score_category(0);
        press_key(&mut g, Key::N);
        assert_eq!(g.turn_number, 0);
        assert!(g.scores.iter().all(|s| s.is_none()));
    }

    #[test]
    fn test_key_tab_toggles_focus() {
        let mut g = test_game();
        assert_eq!(g.focus, FocusRegion::Dice);
        press_key(&mut g, Key::Tab);
        assert_eq!(g.focus, FocusRegion::Scorecard);
        press_key(&mut g, Key::Tab);
        assert_eq!(g.focus, FocusRegion::Dice);
    }

    #[test]
    fn test_key_left_right_navigate_dice() {
        let mut g = test_game();
        assert_eq!(g.selected_die, 0);
        press_key(&mut g, Key::Right);
        assert_eq!(g.selected_die, 1);
        press_key(&mut g, Key::Right);
        assert_eq!(g.selected_die, 2);
        press_key(&mut g, Key::Left);
        assert_eq!(g.selected_die, 1);
    }

    #[test]
    fn test_key_left_at_zero_stays() {
        let mut g = test_game();
        press_key(&mut g, Key::Left);
        assert_eq!(g.selected_die, 0);
    }

    #[test]
    fn test_key_right_at_max_stays() {
        let mut g = test_game();
        g.selected_die = NUM_DICE - 1;
        press_key(&mut g, Key::Right);
        assert_eq!(g.selected_die, NUM_DICE - 1);
    }

    #[test]
    fn test_key_up_down_navigate_categories() {
        let mut g = test_game();
        g.focus = FocusRegion::Scorecard;
        assert_eq!(g.selected_category, 0);
        press_key(&mut g, Key::Down);
        assert_eq!(g.selected_category, 1);
        press_key(&mut g, Key::Down);
        assert_eq!(g.selected_category, 2);
        press_key(&mut g, Key::Up);
        assert_eq!(g.selected_category, 1);
    }

    #[test]
    fn test_key_up_at_zero_stays() {
        let mut g = test_game();
        g.focus = FocusRegion::Scorecard;
        press_key(&mut g, Key::Up);
        assert_eq!(g.selected_category, 0);
    }

    #[test]
    fn test_key_down_at_max_stays() {
        let mut g = test_game();
        g.focus = FocusRegion::Scorecard;
        g.selected_category = NUM_CATEGORIES - 1;
        press_key(&mut g, Key::Down);
        assert_eq!(g.selected_category, NUM_CATEGORIES - 1);
    }

    #[test]
    fn test_key_space_holds_die() {
        let mut g = game_with_dice([1, 2, 3, 4, 5]);
        g.focus = FocusRegion::Dice;
        g.selected_die = 2;
        press_key(&mut g, Key::Space);
        assert!(g.held[2]);
    }

    #[test]
    fn test_key_enter_scores_category() {
        let mut g = game_with_dice([1, 2, 3, 4, 5]);
        g.focus = FocusRegion::Scorecard;
        g.selected_category = Category::Chance.index();
        press_key(&mut g, Key::Enter);
        assert_eq!(g.scores[Category::Chance.index()], Some(15));
    }

    #[test]
    fn test_number_keys_hold_dice() {
        let mut g = game_with_dice([1, 2, 3, 4, 5]);
        press_key(&mut g, Key::Num1);
        assert!(g.held[0]);
        press_key(&mut g, Key::Num2);
        assert!(g.held[1]);
        press_key(&mut g, Key::Num3);
        assert!(g.held[2]);
        press_key(&mut g, Key::Num4);
        assert!(g.held[3]);
        press_key(&mut g, Key::Num5);
        assert!(g.held[4]);
    }

    #[test]
    fn test_key_release_ignored() {
        let mut g = test_game();
        let event = Event::Key(KeyEvent {
            key: Key::R,
            pressed: false, // Release, not press.
            modifiers: Modifiers::NONE,
            text: String::new(),
        });
        handle_event(&mut g, &event);
        assert_eq!(g.roll_number, 0);
    }

    #[test]
    fn test_arrows_in_wrong_focus_do_nothing() {
        let mut g = test_game();
        g.focus = FocusRegion::Scorecard;
        g.selected_die = 0;
        press_key(&mut g, Key::Right); // Should not move die selection.
        assert_eq!(g.selected_die, 0);

        g.focus = FocusRegion::Dice;
        g.selected_category = 0;
        press_key(&mut g, Key::Down); // Should not move category selection.
        assert_eq!(g.selected_category, 0);
    }

    // ════════════════════════════════════════════════════════════════
    // Mouse input
    // ════════════════════════════════════════════════════════════════

    // ════════════════════════════════════════════════════════════════
    // Rendering
    // ════════════════════════════════════════════════════════════════

    // ════════════════════════════════════════════════════════════════
    // Category metadata
    // ════════════════════════════════════════════════════════════════

    #[test]
    fn test_category_names() {
        assert_eq!(Category::Ones.name(), "Ones");
        assert_eq!(Category::FullHouse.name(), "Full House");
        assert_eq!(Category::Yahtzee.name(), "Yahtzee");
        assert_eq!(Category::Chance.name(), "Chance");
    }

    #[test]
    fn test_category_is_upper() {
        assert!(Category::Ones.is_upper());
        assert!(Category::Sixes.is_upper());
        assert!(!Category::ThreeOfAKind.is_upper());
        assert!(!Category::Yahtzee.is_upper());
    }

    #[test]
    fn test_category_all_has_13() {
        assert_eq!(Category::ALL.len(), NUM_CATEGORIES);
    }

    #[test]
    fn test_category_indices_unique() {
        let mut seen = [false; NUM_CATEGORIES];
        for cat in &Category::ALL {
            let idx = cat.index();
            assert!(!seen[idx], "Duplicate category index: {idx}");
            seen[idx] = true;
        }
    }

    // ════════════════════════════════════════════════════════════════
    // RNG
    // ════════════════════════════════════════════════════════════════

    #[test]
    fn test_rng_die_range() {
        let mut rng = SeededRng::new(9999);
        for _ in 0..1000 {
            let val = roll_die(&mut rng);
            assert!((1..=6).contains(&val), "Die out of range: {val}");
        }
    }

    #[test]
    fn test_rng_produces_all_values() {
        let mut rng = SeededRng::new(7777);
        let mut seen = [false; 7];
        for _ in 0..10000 {
            let val = roll_die(&mut rng);
            seen[val as usize] = true;
        }
        for v in 1..=6 {
            assert!(seen[v], "RNG never produced {v}");
        }
    }

    /// A Yahtzee has to be rollable in Yahtzee.
    ///
    /// It was not. The old die was `state % 6 + 1`; six is even, `x % 6` keeps
    /// the parity of `x`, and the low bit of a modulus-2^64 LCG alternates on
    /// every draw. So consecutive dice always had opposite parity, and five
    /// alike could not occur — nor four alike, since any four of five dice
    /// contain an adjacent pair. Three was the ceiling. Measured over 15 000
    /// rolls at three seeds before the fix: zero Yahtzees.
    ///
    /// Note what the two tests above could not see. `test_rng_die_range` and
    /// `test_rng_produces_all_values` both passed against that die, and were
    /// right to: every face appeared, and appeared about equally often. The
    /// defect lived entirely in the *relationship between successive rolls*,
    /// which no test of one roll at a time can reach. This one rolls five and
    /// asks about the hand.
    #[test]
    fn five_of_a_kind_is_reachable() {
        // A Yahtzee is 1 in 1296, so 40 000 rolls expects about 31 and misses
        // entirely with probability below 1e-13. Four alike is counted too: it
        // was equally impossible and is 25 times more common, so it fails
        // loudly rather than marginally.
        let mut rng = SeededRng::new(2024);
        let mut yahtzees = 0_u32;
        let mut four_alike = 0_u32;
        for _ in 0..40_000 {
            let dice: [u8; NUM_DICE] = core::array::from_fn(|_| roll_die(&mut rng));
            let counts = face_counts(&dice);
            let best = counts.iter().skip(1).copied().max().unwrap_or(0);
            if best >= 5 {
                yahtzees += 1;
            }
            if best >= 4 {
                four_alike += 1;
            }
        }
        assert!(yahtzees > 0, "40000 rolls produced no Yahtzee at all");
        assert!(
            four_alike > 100,
            "40000 rolls produced only {four_alike} hands of four alike"
        );
    }

    /// Adjacent dice must not be locked to opposite parity.
    ///
    /// The defect stated directly rather than through its consequences: with
    /// the old die, neighbouring dice never showed `(odd, odd)` or
    /// `(even, even)`, at any seed. All four combinations must be reachable.
    #[test]
    fn adjacent_dice_are_not_locked_to_opposite_parity() {
        let mut rng = SeededRng::new(31);
        let mut seen = std::collections::BTreeSet::new();
        for _ in 0..2000 {
            let dice: [u8; NUM_DICE] = core::array::from_fn(|_| roll_die(&mut rng));
            for pair in dice.windows(2) {
                if let [a, b] = pair {
                    seen.insert((a % 2, b % 2));
                }
            }
        }
        assert_eq!(
            seen.len(),
            4,
            "adjacent dice only ever showed parity pairs {seen:?}"
        );
    }

    // ════════════════════════════════════════════════════════════════
    // Edge cases
    // ════════════════════════════════════════════════════════════════

    #[test]
    fn test_all_zeros_upper_section() {
        let mut g = test_game();
        for i in 0..6 {
            g.scores[i] = Some(0);
        }
        assert_eq!(g.upper_total(), 0);
        assert_eq!(g.upper_bonus(), 0);
    }

    #[test]
    fn test_lower_total_all_zeros() {
        let mut g = test_game();
        for i in 6..NUM_CATEGORIES {
            g.scores[i] = Some(0);
        }
        assert_eq!(g.lower_total(), 0);
    }

    #[test]
    fn test_lower_total_all_max() {
        let mut g = test_game();
        // Max possible lower section scores.
        g.scores[Category::ThreeOfAKind.index()] = Some(30); // All sixes.
        g.scores[Category::FourOfAKind.index()] = Some(30);
        g.scores[Category::FullHouse.index()] = Some(25);
        g.scores[Category::SmallStraight.index()] = Some(30);
        g.scores[Category::LargeStraight.index()] = Some(40);
        g.scores[Category::Yahtzee.index()] = Some(50);
        g.scores[Category::Chance.index()] = Some(30);
        assert_eq!(g.lower_total(), 235);
    }

    #[test]
    fn test_perfect_game_score() {
        // Theoretical perfect game: all sixes in upper, all max in lower, bonus.
        let mut g = test_game();
        g.scores[0] = Some(5); // 5 ones = impossible with 5 dice of value 6, but for scoring test
        g.scores[1] = Some(10);
        g.scores[2] = Some(15);
        g.scores[3] = Some(20);
        g.scores[4] = Some(25);
        g.scores[5] = Some(30); // Upper total = 105 >= 63 → bonus 35
        g.scores[Category::ThreeOfAKind.index()] = Some(30);
        g.scores[Category::FourOfAKind.index()] = Some(30);
        g.scores[Category::FullHouse.index()] = Some(25);
        g.scores[Category::SmallStraight.index()] = Some(30);
        g.scores[Category::LargeStraight.index()] = Some(40);
        g.scores[Category::Yahtzee.index()] = Some(50);
        g.scores[Category::Chance.index()] = Some(30);
        g.yahtzee_bonus_count = 0;
        // 105 + 35 + 235 = 375
        assert_eq!(g.grand_total(), 375);
    }

    #[test]
    fn test_full_game_flow() {
        // Simulate a full game: roll, hold, score each turn.
        let mut g = Yahtzee::with_seed(54321);
        for turn in 0..NUM_TURNS {
            assert_eq!(g.turn_number, turn);
            assert!(g.roll());
            // Score in the current turn's category.
            assert!(g.score_category(turn));
        }
        assert_eq!(g.phase(), GamePhase::GameOver);
        assert!(g.grand_total() > 0);
    }

    #[test]
    fn test_event_handling_does_not_panic() {
        let mut g = test_game();
        // Fire a bunch of random events to ensure no panics.
        let keys = [
            Key::R,
            Key::N,
            Key::Tab,
            Key::Left,
            Key::Right,
            Key::Up,
            Key::Down,
            Key::Space,
            Key::Enter,
            Key::Escape,
            Key::Num1,
            Key::Num2,
            Key::Num3,
            Key::Num4,
            Key::Num5,
        ];
        for &k in &keys {
            press_key(&mut g, k);
        }
    }

    #[test]
    fn test_score_category_after_game_over() {
        let mut g = test_game();
        g.turn_number = NUM_TURNS;
        g.roll_number = 1;
        // Cannot score after game over because score_category checks roll_number > 0
        // but the category check should still prevent invalid scoring.
        // Actually, we should be able to score if phase is game_over — but the function
        // checks if the category is already filled. Let's ensure no crash.
        let result = g.score_category(0);
        // It succeeds because we haven't filled it and roll_number > 0.
        // The turn advancement code will see turn_number == 1 which is < NUM_TURNS so
        // it sets phase to Rolling. This is an edge case, but it should not crash.
        assert!(result);
    }
}
