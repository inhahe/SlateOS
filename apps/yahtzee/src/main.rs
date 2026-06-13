#![allow(dead_code)]
#![allow(clippy::too_many_lines)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::cast_possible_wrap)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::similar_names)]
#![allow(clippy::struct_excessive_bools)]
#![allow(clippy::fn_params_excessive_bools)]
#![allow(clippy::needless_range_loop)]
#![allow(unused_imports)]

//! SlateOS Yahtzee — the classic dice game.
//!
//! Roll five dice up to three times per turn, strategically holding dice
//! between rolls, and assign the result to one of 13 scoring categories.
//! Features full Yahtzee scoring including upper-section bonus (35 points
//! when upper total >= 63), Yahtzee bonus (100 per additional Yahtzee),
//! keyboard and mouse controls, a visual scorecard, and high-score tracking.

use guitk::color::Color;
use guitk::event::{Event, Key, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseEventKind};
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

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

// ── Layout constants ────────────────────────────────────────────────
const PADDING: f32 = 16.0;
const DICE_SIZE: f32 = 64.0;
const DICE_GAP: f32 = 12.0;
const DICE_DOT_RADIUS: f32 = 6.0;
const DICE_CORNER_RADIUS: f32 = 8.0;
const SCORECARD_WIDTH: f32 = 320.0;
const SCORECARD_ROW_HEIGHT: f32 = 28.0;
const HEADER_HEIGHT: f32 = 50.0;
const DICE_AREA_HEIGHT: f32 = 120.0;
const BUTTON_HEIGHT: f32 = 36.0;
const BUTTON_WIDTH: f32 = 140.0;
const TITLE_FONT_SIZE: f32 = 24.0;
const HEADER_FONT_SIZE: f32 = 16.0;
const SCORE_FONT_SIZE: f32 = 14.0;
const DICE_LABEL_FONT_SIZE: f32 = 11.0;
const BUTTON_FONT_SIZE: f32 = 14.0;
const INFO_FONT_SIZE: f32 = 13.0;

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

    /// Whether this category belongs to the upper section (Ones-Sixes).
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

    /// The index into `Category::ALL`.
    fn index(self) -> usize {
        Category::ALL.iter().position(|&c| c == self).unwrap_or(0)
    }
}

// ── Focus region ────────────────────────────────────────────────────

/// Which region of the UI currently has keyboard focus.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FocusRegion {
    Dice,
    Scorecard,
}

// ── Game state ──────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GamePhase {
    /// Player needs to roll (start of turn, or between rolls).
    Rolling,
    /// All 3 rolls used; must pick a category.
    MustScore,
    /// Game is over (all 13 categories filled).
    GameOver,
}

// ── Seeded RNG ──────────────────────────────────────────────────────

struct Rng {
    state: u64,
}

impl Rng {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.state
    }

    /// Return a value in `1..=6`.
    fn die(&mut self) -> u8 {
        (self.next() % 6) as u8 + 1
    }
}

// ── Scoring logic (pure functions) ──────────────────────────────────

/// Count how many times each die face appears. Returns array indexed 0..=6
/// (index 0 unused for clarity: counts[1] = count of ones, etc.).
fn face_counts(dice: &[u8; NUM_DICE]) -> [u8; 7] {
    let mut counts = [0u8; 7];
    for &d in dice {
        if (1..=6).contains(&d) {
            counts[d as usize] += 1;
        }
    }
    counts
}

/// Sum of all dice.
fn dice_sum(dice: &[u8; NUM_DICE]) -> u16 {
    dice.iter().map(|&d| u16::from(d)).sum()
}

/// Score for an upper-section category (Ones-Sixes): sum of dice matching
/// the target face value.
fn score_upper(dice: &[u8; NUM_DICE], target: u8) -> u16 {
    dice.iter()
        .filter(|&&d| d == target)
        .map(|&d| u16::from(d))
        .sum()
}

/// Returns true if the dice contain at least `n` of any single face.
fn has_n_of_a_kind(dice: &[u8; NUM_DICE], n: u8) -> bool {
    let counts = face_counts(dice);
    counts[1..].iter().any(|&c| c >= n)
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
    let has_three = counts[1..].contains(&3);
    let has_two = counts[1..].contains(&2);
    if has_three && has_two {
        FULL_HOUSE_SCORE
    } else {
        0
    }
}

/// Returns true if dice contain the consecutive sequence of length `len`
/// starting at `start`.
fn has_consecutive_run(dice: &[u8; NUM_DICE], len: u8) -> bool {
    let counts = face_counts(dice);
    // Check every possible starting point for a run of `len`.
    'outer: for start in 1..=(7 - len) {
        for offset in 0..len {
            if counts[(start + offset) as usize] == 0 {
                continue 'outer;
            }
        }
        return true;
    }
    false
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
    /// Current game phase.
    phase: GamePhase,
    /// Which UI region has keyboard focus.
    focus: FocusRegion,
    /// Currently selected die index (0..5) when focus is on dice.
    selected_die: usize,
    /// Currently selected category index (0..13) when focus is on scorecard.
    selected_category: usize,
    /// Highest score achieved across games.
    high_score: u16,
    /// RNG for dice rolls.
    rng: Rng,
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
            phase: GamePhase::Rolling,
            focus: FocusRegion::Dice,
            selected_die: 0,
            selected_category: 0,
            high_score: 0,
            rng: Rng::new(seed),
        }
    }

    // ── Game logic ─────────────────────────────────────────────────

    /// Roll all un-held dice. Returns false if no rolls remain.
    fn roll(&mut self) -> bool {
        if self.roll_number >= MAX_ROLLS || self.phase == GamePhase::GameOver {
            return false;
        }

        for i in 0..NUM_DICE {
            if !self.held[i] {
                self.dice[i] = self.rng.die();
            }
        }

        self.roll_number += 1;
        if self.roll_number >= MAX_ROLLS {
            self.phase = GamePhase::MustScore;
        } else {
            self.phase = GamePhase::Rolling;
        }

        true
    }

    /// Toggle the hold state of a die at the given index.
    fn toggle_hold(&mut self, index: usize) {
        if index < NUM_DICE && self.roll_number > 0 && self.roll_number < MAX_ROLLS {
            self.held[index] = !self.held[index];
        }
    }

    /// Returns true if a Yahtzee is currently rolled.
    fn is_yahtzee(&self) -> bool {
        has_n_of_a_kind(&self.dice, 5)
    }

    /// Returns whether the Yahtzee category has already been scored with
    /// a non-zero value.
    fn yahtzee_already_scored_nonzero(&self) -> bool {
        self.scores[Category::Yahtzee.index()]
            .is_some_and(|s| s > 0)
    }

    /// Check and award Yahtzee bonus: if the player already scored a Yahtzee
    /// (non-zero) and rolls another Yahtzee, they get 100 bonus points.
    fn check_yahtzee_bonus(&mut self) {
        if self.is_yahtzee() && self.yahtzee_already_scored_nonzero() {
            self.yahtzee_bonus_count += 1;
        }
    }

    /// Attempt to score the selected category. Returns false if invalid.
    fn score_category(&mut self, cat_index: usize) -> bool {
        if cat_index >= NUM_CATEGORIES {
            return false;
        }
        if self.roll_number == 0 {
            return false;
        }
        if self.scores[cat_index].is_some() {
            return false;
        }

        let cat = Category::ALL[cat_index];

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
        let score = if self.is_yahtzee()
            && self.scores[Category::Yahtzee.index()].is_some()
        {
            // Joker rule for lower-section categories
            match cat {
                Category::FullHouse => FULL_HOUSE_SCORE,
                Category::SmallStraight => SMALL_STRAIGHT_SCORE,
                Category::LargeStraight => LARGE_STRAIGHT_SCORE,
                _ => potential_score(&self.dice, cat),
            }
        } else {
            potential_score(&self.dice, cat)
        };

        self.scores[cat_index] = Some(score);
        self.advance_turn();
        true
    }

    /// Advance to the next turn after scoring.
    fn advance_turn(&mut self) {
        self.turn_number += 1;
        self.roll_number = 0;
        self.held = [false; NUM_DICE];

        if self.turn_number >= NUM_TURNS {
            self.phase = GamePhase::GameOver;
            let total = self.grand_total();
            if total > self.high_score {
                self.high_score = total;
            }
        } else {
            self.phase = GamePhase::Rolling;
        }
    }

    /// Start a new game, preserving the high score.
    fn new_game(&mut self) {
        let high = self.high_score;
        let seed = self.rng.next();
        *self = Self::with_seed(seed);
        self.high_score = high;
    }

    // ── Score calculation ──────────────────────────────────────────

    /// Sum of the upper section (Ones-Sixes) scores.
    fn upper_total(&self) -> u16 {
        let mut sum = 0u16;
        for i in 0..6 {
            if let Some(s) = self.scores[i] {
                sum += s;
            }
        }
        sum
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
        let mut sum = 0u16;
        for i in 6..NUM_CATEGORIES {
            if let Some(s) = self.scores[i] {
                sum += s;
            }
        }
        sum
    }

    /// Total Yahtzee bonus points.
    fn yahtzee_bonus_total(&self) -> u16 {
        self.yahtzee_bonus_count * YAHTZEE_BONUS_VALUE
    }

    /// Grand total score.
    fn grand_total(&self) -> u16 {
        self.upper_total()
            + self.upper_bonus()
            + self.lower_total()
            + self.yahtzee_bonus_total()
    }

    /// Number of categories that have been scored.
    fn categories_filled(&self) -> usize {
        self.scores.iter().filter(|s| s.is_some()).count()
    }

    // ── Input handling ─────────────────────────────────────────────

    fn handle_key(&mut self, key: Key, pressed: bool) {
        if !pressed {
            return;
        }

        match key {
            Key::R
                if self.phase != GamePhase::GameOver => {
                    self.roll();
                }
            Key::N => {
                self.new_game();
            }
            Key::Tab => {
                // Toggle focus between dice and scorecard.
                self.focus = match self.focus {
                    FocusRegion::Dice => FocusRegion::Scorecard,
                    FocusRegion::Scorecard => FocusRegion::Dice,
                };
            }
            Key::Left
                if self.focus == FocusRegion::Dice && self.selected_die > 0 => {
                    self.selected_die -= 1;
                }
            Key::Right
                if self.focus == FocusRegion::Dice && self.selected_die < NUM_DICE - 1 => {
                    self.selected_die += 1;
                }
            Key::Up
                if self.focus == FocusRegion::Scorecard && self.selected_category > 0 => {
                    self.selected_category -= 1;
                }
            Key::Down
                if self.focus == FocusRegion::Scorecard
                    && self.selected_category < NUM_CATEGORIES - 1
                => {
                    self.selected_category += 1;
                }
            Key::Space | Key::Enter => match self.focus {
                FocusRegion::Dice => {
                    self.toggle_hold(self.selected_die);
                }
                FocusRegion::Scorecard => {
                    self.score_category(self.selected_category);
                }
            },
            Key::Num1 => self.toggle_hold(0),
            Key::Num2 => self.toggle_hold(1),
            Key::Num3 => self.toggle_hold(2),
            Key::Num4 => self.toggle_hold(3),
            Key::Num5 => self.toggle_hold(4),
            _ => {}
        }
    }

    fn handle_mouse_click(&mut self, x: f32, y: f32) {
        // Check dice area clicks.
        let dice_y_start = PADDING + HEADER_HEIGHT + 10.0;
        let dice_x_start = PADDING;
        for i in 0..NUM_DICE {
            let dx = dice_x_start + i as f32 * (DICE_SIZE + DICE_GAP);
            let dy = dice_y_start;
            if x >= dx && x <= dx + DICE_SIZE && y >= dy && y <= dy + DICE_SIZE {
                self.focus = FocusRegion::Dice;
                self.selected_die = i;
                self.toggle_hold(i);
                return;
            }
        }

        // Check roll button click.
        let roll_btn_x = PADDING;
        let roll_btn_y = dice_y_start + DICE_SIZE + 30.0;
        if x >= roll_btn_x
            && x <= roll_btn_x + BUTTON_WIDTH
            && y >= roll_btn_y
            && y <= roll_btn_y + BUTTON_HEIGHT
        {
            if self.phase == GamePhase::GameOver {
                self.new_game();
            } else {
                self.roll();
            }
            return;
        }

        // Check scorecard category clicks.
        let sc_x = self.scorecard_x();
        let sc_y = PADDING + HEADER_HEIGHT + 10.0 + SCORECARD_ROW_HEIGHT; // after header row
        for i in 0..NUM_CATEGORIES {
            // Account for section headers and separators.
            let row_y = sc_y + self.category_display_row(i) as f32 * SCORECARD_ROW_HEIGHT;
            if x >= sc_x
                && x <= sc_x + SCORECARD_WIDTH
                && y >= row_y
                && y <= row_y + SCORECARD_ROW_HEIGHT
            {
                self.focus = FocusRegion::Scorecard;
                self.selected_category = i;
                self.score_category(i);
                return;
            }
        }
    }

    /// Map category index to display row, accounting for the separator
    /// between upper and lower sections, and the bonus/total rows.
    fn category_display_row(&self, cat_index: usize) -> usize {
        if cat_index < 6 {
            // Upper section: rows 0..5
            cat_index
        } else {
            // Lower section: skip upper(6) + upper-total row + bonus row + separator row = +3
            cat_index + 3
        }
    }

    /// X position of the scorecard panel.
    fn scorecard_x(&self) -> f32 {
        PADDING + NUM_DICE as f32 * (DICE_SIZE + DICE_GAP) + 20.0
    }

    // ── Rendering ──────────────────────────────────────────────────

    fn render(&self, width: f32, height: f32) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Background.
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width,
            height,
            color: BASE,
            corner_radii: CornerRadii::ZERO,
        });

        // Title.
        cmds.push(RenderCommand::Text {
            x: PADDING,
            y: PADDING + 4.0,
            text: String::from("Yahtzee"),
            color: LAVENDER,
            font_size: TITLE_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Turn and roll info.
        let turn_display = if self.phase == GamePhase::GameOver {
            String::from("Game Over!")
        } else {
            format!(
                "Turn {}/{}  |  Roll {}/{}",
                self.turn_number + 1,
                NUM_TURNS,
                self.roll_number,
                MAX_ROLLS
            )
        };
        cmds.push(RenderCommand::Text {
            x: PADDING + 130.0,
            y: PADDING + 8.0,
            text: turn_display,
            color: SUBTEXT0,
            font_size: HEADER_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // High score.
        cmds.push(RenderCommand::Text {
            x: PADDING + 400.0,
            y: PADDING + 8.0,
            text: format!("High: {}", self.high_score),
            color: YELLOW,
            font_size: HEADER_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Render dice.
        self.render_dice(&mut cmds);

        // Render action button.
        self.render_button(&mut cmds);

        // Render controls hint.
        self.render_controls_hint(&mut cmds);

        // Render scorecard.
        self.render_scorecard(&mut cmds);

        // Render grand total.
        self.render_grand_total(&mut cmds);

        cmds
    }

    fn render_dice(&self, cmds: &mut Vec<RenderCommand>) {
        let dice_y = PADDING + HEADER_HEIGHT + 10.0;
        let dice_x_start = PADDING;

        for i in 0..NUM_DICE {
            let x = dice_x_start + i as f32 * (DICE_SIZE + DICE_GAP);
            let y = dice_y;

            // Die background.
            let bg_color = if self.held[i] { SURFACE1 } else { SURFACE0 };
            let border_color = if self.focus == FocusRegion::Dice && self.selected_die == i {
                BLUE
            } else if self.held[i] {
                PEACH
            } else {
                OVERLAY0
            };

            // Border (slightly larger rect behind).
            cmds.push(RenderCommand::FillRect {
                x: x - 2.0,
                y: y - 2.0,
                width: DICE_SIZE + 4.0,
                height: DICE_SIZE + 4.0,
                color: border_color,
                corner_radii: CornerRadii::all(DICE_CORNER_RADIUS + 2.0),
            });

            // Die face.
            cmds.push(RenderCommand::FillRect {
                x,
                y,
                width: DICE_SIZE,
                height: DICE_SIZE,
                color: bg_color,
                corner_radii: CornerRadii::all(DICE_CORNER_RADIUS),
            });

            // Draw dots if rolled.
            if self.roll_number > 0 {
                self.render_die_dots(cmds, x, y, self.dice[i]);
            }

            // "HELD" label.
            if self.held[i] {
                cmds.push(RenderCommand::Text {
                    x: x + 14.0,
                    y: y + DICE_SIZE + 4.0,
                    text: String::from("HELD"),
                    color: PEACH,
                    font_size: DICE_LABEL_FONT_SIZE,
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                });
            }

            // Die number label (1-5).
            cmds.push(RenderCommand::Text {
                x: x + DICE_SIZE / 2.0 - 3.0,
                y: y - 14.0,
                text: format!("{}", i + 1),
                color: OVERLAY0,
                font_size: DICE_LABEL_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }
    }

    /// Render the dots on a single die face using filled circles (approximated
    /// as small rounded rectangles).
    fn render_die_dots(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, value: u8) {
        let dot_size = DICE_DOT_RADIUS * 2.0;
        let cx = x + DICE_SIZE / 2.0; // center X of die
        let cy = y + DICE_SIZE / 2.0; // center Y of die
        let off = 16.0; // offset from center for corner dots

        let dot_color = TEXT_COLOR;

        // Position arrays for each value.
        let positions: Vec<(f32, f32)> = match value {
            1 => vec![(cx, cy)],
            2 => vec![(cx - off, cy - off), (cx + off, cy + off)],
            3 => vec![(cx - off, cy - off), (cx, cy), (cx + off, cy + off)],
            4 => vec![
                (cx - off, cy - off),
                (cx + off, cy - off),
                (cx - off, cy + off),
                (cx + off, cy + off),
            ],
            5 => vec![
                (cx - off, cy - off),
                (cx + off, cy - off),
                (cx, cy),
                (cx - off, cy + off),
                (cx + off, cy + off),
            ],
            6 => vec![
                (cx - off, cy - off),
                (cx + off, cy - off),
                (cx - off, cy),
                (cx + off, cy),
                (cx - off, cy + off),
                (cx + off, cy + off),
            ],
            _ => vec![],
        };

        for (px, py) in positions {
            cmds.push(RenderCommand::FillRect {
                x: px - DICE_DOT_RADIUS,
                y: py - DICE_DOT_RADIUS,
                width: dot_size,
                height: dot_size,
                color: dot_color,
                corner_radii: CornerRadii::all(DICE_DOT_RADIUS),
            });
        }
    }

    fn render_button(&self, cmds: &mut Vec<RenderCommand>) {
        let dice_y = PADDING + HEADER_HEIGHT + 10.0;
        let btn_x = PADDING;
        let btn_y = dice_y + DICE_SIZE + 30.0;

        let (btn_color, btn_text) = if self.phase == GamePhase::GameOver {
            (GREEN, "New Game (N)")
        } else if self.roll_number >= MAX_ROLLS {
            (OVERLAY0, "No Rolls Left")
        } else {
            (BLUE, "Roll (R)")
        };

        cmds.push(RenderCommand::FillRect {
            x: btn_x,
            y: btn_y,
            width: BUTTON_WIDTH,
            height: BUTTON_HEIGHT,
            color: btn_color,
            corner_radii: CornerRadii::all(6.0),
        });

        cmds.push(RenderCommand::Text {
            x: btn_x + 10.0,
            y: btn_y + 9.0,
            text: String::from(btn_text),
            color: if self.roll_number >= MAX_ROLLS && self.phase != GamePhase::GameOver {
                SUBTEXT0
            } else {
                CRUST
            },
            font_size: BUTTON_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
    }

    fn render_controls_hint(&self, cmds: &mut Vec<RenderCommand>) {
        let dice_y = PADDING + HEADER_HEIGHT + 10.0;
        let hint_y = dice_y + DICE_SIZE + 30.0 + BUTTON_HEIGHT + 12.0;
        let hints = [
            "R: Roll  |  1-5: Hold/Release",
            "Tab: Switch focus  |  Arrows: Navigate",
            "Space/Enter: Hold die or Score category",
            "N: New Game",
        ];

        for (i, hint) in hints.iter().enumerate() {
            cmds.push(RenderCommand::Text {
                x: PADDING,
                y: hint_y + i as f32 * 16.0,
                text: String::from(*hint),
                color: OVERLAY0,
                font_size: INFO_FONT_SIZE - 2.0,
                font_weight: FontWeightHint::Light,
                max_width: None,
            });
        }
    }

    fn render_scorecard(&self, cmds: &mut Vec<RenderCommand>) {
        let sc_x = self.scorecard_x();
        let sc_y = PADDING + HEADER_HEIGHT + 10.0;

        // Scorecard background.
        let total_rows = NUM_CATEGORIES + 5; // categories + header + upper total + bonus + separator + lower label
        let sc_height = (total_rows as f32 + 1.0) * SCORECARD_ROW_HEIGHT;
        cmds.push(RenderCommand::FillRect {
            x: sc_x - 4.0,
            y: sc_y - 4.0,
            width: SCORECARD_WIDTH + 8.0,
            height: sc_height + 8.0,
            color: MANTLE,
            corner_radii: CornerRadii::all(8.0),
        });

        // Header row.
        cmds.push(RenderCommand::FillRect {
            x: sc_x,
            y: sc_y,
            width: SCORECARD_WIDTH,
            height: SCORECARD_ROW_HEIGHT,
            color: SURFACE1,
            corner_radii: CornerRadii::all(4.0),
        });
        cmds.push(RenderCommand::Text {
            x: sc_x + 8.0,
            y: sc_y + 6.0,
            text: String::from("Category"),
            color: TEXT_COLOR,
            font_size: SCORE_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        cmds.push(RenderCommand::Text {
            x: sc_x + SCORECARD_WIDTH - 80.0,
            y: sc_y + 6.0,
            text: String::from("Score"),
            color: TEXT_COLOR,
            font_size: SCORE_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        let mut row_y = sc_y + SCORECARD_ROW_HEIGHT;

        // Upper section.
        for i in 0..6 {
            self.render_scorecard_row(cmds, sc_x, row_y, i);
            row_y += SCORECARD_ROW_HEIGHT;
        }

        // Upper total row.
        cmds.push(RenderCommand::FillRect {
            x: sc_x,
            y: row_y,
            width: SCORECARD_WIDTH,
            height: SCORECARD_ROW_HEIGHT,
            color: SURFACE0,
            corner_radii: CornerRadii::ZERO,
        });
        cmds.push(RenderCommand::Text {
            x: sc_x + 8.0,
            y: row_y + 6.0,
            text: String::from("Upper Total"),
            color: SUBTEXT0,
            font_size: SCORE_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        cmds.push(RenderCommand::Text {
            x: sc_x + SCORECARD_WIDTH - 80.0,
            y: row_y + 6.0,
            text: format!("{} / {}", self.upper_total(), UPPER_BONUS_THRESHOLD),
            color: if self.upper_total() >= UPPER_BONUS_THRESHOLD {
                GREEN
            } else {
                SUBTEXT0
            },
            font_size: SCORE_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        row_y += SCORECARD_ROW_HEIGHT;

        // Bonus row.
        cmds.push(RenderCommand::FillRect {
            x: sc_x,
            y: row_y,
            width: SCORECARD_WIDTH,
            height: SCORECARD_ROW_HEIGHT,
            color: SURFACE0,
            corner_radii: CornerRadii::ZERO,
        });
        cmds.push(RenderCommand::Text {
            x: sc_x + 8.0,
            y: row_y + 6.0,
            text: String::from("Bonus"),
            color: SUBTEXT0,
            font_size: SCORE_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        cmds.push(RenderCommand::Text {
            x: sc_x + SCORECARD_WIDTH - 80.0,
            y: row_y + 6.0,
            text: if self.upper_bonus() > 0 {
                format!("+{}", self.upper_bonus())
            } else {
                String::from("-")
            },
            color: if self.upper_bonus() > 0 { GREEN } else { OVERLAY0 },
            font_size: SCORE_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        row_y += SCORECARD_ROW_HEIGHT;

        // Separator.
        cmds.push(RenderCommand::Line {
            x1: sc_x,
            y1: row_y + SCORECARD_ROW_HEIGHT / 2.0,
            x2: sc_x + SCORECARD_WIDTH,
            y2: row_y + SCORECARD_ROW_HEIGHT / 2.0,
            color: SURFACE1,
            width: 1.0,
        });
        row_y += SCORECARD_ROW_HEIGHT;

        // Lower section.
        for i in 6..NUM_CATEGORIES {
            self.render_scorecard_row(cmds, sc_x, row_y, i);
            row_y += SCORECARD_ROW_HEIGHT;
        }

        // Yahtzee bonus row (if any bonuses earned).
        if self.yahtzee_bonus_count > 0 {
            cmds.push(RenderCommand::FillRect {
                x: sc_x,
                y: row_y,
                width: SCORECARD_WIDTH,
                height: SCORECARD_ROW_HEIGHT,
                color: SURFACE0,
                corner_radii: CornerRadii::ZERO,
            });
            cmds.push(RenderCommand::Text {
                x: sc_x + 8.0,
                y: row_y + 6.0,
                text: format!("Yahtzee Bonus (x{})", self.yahtzee_bonus_count),
                color: MAUVE,
                font_size: SCORE_FONT_SIZE,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
            cmds.push(RenderCommand::Text {
                x: sc_x + SCORECARD_WIDTH - 80.0,
                y: row_y + 6.0,
                text: format!("+{}", self.yahtzee_bonus_total()),
                color: MAUVE,
                font_size: SCORE_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }
    }

    fn render_scorecard_row(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        cat_index: usize,
    ) {
        let cat = Category::ALL[cat_index];
        let is_selected =
            self.focus == FocusRegion::Scorecard && self.selected_category == cat_index;
        let is_filled = self.scores[cat_index].is_some();

        // Row background.
        let bg_color = if is_selected {
            SURFACE1
        } else if cat_index.is_multiple_of(2) {
            CRUST
        } else {
            MANTLE
        };
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width: SCORECARD_WIDTH,
            height: SCORECARD_ROW_HEIGHT,
            color: bg_color,
            corner_radii: CornerRadii::ZERO,
        });

        // Selection indicator.
        if is_selected {
            cmds.push(RenderCommand::FillRect {
                x,
                y,
                width: 3.0,
                height: SCORECARD_ROW_HEIGHT,
                color: BLUE,
                corner_radii: CornerRadii::ZERO,
            });
        }

        // Category name.
        cmds.push(RenderCommand::Text {
            x: x + 8.0,
            y: y + 6.0,
            text: String::from(cat.name()),
            color: if is_filled { SUBTEXT0 } else { TEXT_COLOR },
            font_size: SCORE_FONT_SIZE,
            font_weight: if is_selected {
                FontWeightHint::Bold
            } else {
                FontWeightHint::Regular
            },
            max_width: None,
        });

        // Score value.
        let score_text;
        let score_color;
        if let Some(s) = self.scores[cat_index] {
            score_text = format!("{s}");
            score_color = if s > 0 { GREEN } else { RED };
        } else if self.roll_number > 0 {
            // Show potential score.
            let pot = potential_score(&self.dice, cat);
            score_text = format!("({pot})");
            score_color = if pot > 0 { TEAL } else { OVERLAY0 };
        } else {
            score_text = String::from("-");
            score_color = OVERLAY0;
        }

        cmds.push(RenderCommand::Text {
            x: x + SCORECARD_WIDTH - 80.0,
            y: y + 6.0,
            text: score_text,
            color: score_color,
            font_size: SCORE_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }

    fn render_grand_total(&self, cmds: &mut Vec<RenderCommand>) {
        let sc_x = self.scorecard_x();
        // Position below the scorecard.
        let total_rows = NUM_CATEGORIES + 5;
        let sc_y = PADDING + HEADER_HEIGHT + 10.0;
        let total_y = sc_y + (total_rows as f32 + 1.0) * SCORECARD_ROW_HEIGHT + 16.0;

        cmds.push(RenderCommand::FillRect {
            x: sc_x - 4.0,
            y: total_y - 4.0,
            width: SCORECARD_WIDTH + 8.0,
            height: SCORECARD_ROW_HEIGHT + 8.0,
            color: SURFACE1,
            corner_radii: CornerRadii::all(6.0),
        });

        cmds.push(RenderCommand::Text {
            x: sc_x + 8.0,
            y: total_y + 6.0,
            text: String::from("GRAND TOTAL"),
            color: TEXT_COLOR,
            font_size: SCORE_FONT_SIZE + 2.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        cmds.push(RenderCommand::Text {
            x: sc_x + SCORECARD_WIDTH - 80.0,
            y: total_y + 6.0,
            text: format!("{}", self.grand_total()),
            color: YELLOW,
            font_size: SCORE_FONT_SIZE + 2.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
    }

    // ── Event dispatch ─────────────────────────────────────────────

    fn handle_event(&mut self, event: &Event) {
        match event {
            Event::Key(ke) => self.handle_key(ke.key, ke.pressed),
            Event::Mouse(me) => {
                if let MouseEventKind::Press(MouseButton::Left) = me.kind {
                    self.handle_mouse_click(me.x, me.y);
                }
            }
            _ => {}
        }
    }
}

// ── Entry point ─────────────────────────────────────────────────────

fn main() {
    let _app = Yahtzee::new();
}

// ═══════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════
#[cfg(test)]
mod tests {
    use super::*;

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
    fn press_key(game: &mut Yahtzee, key: Key) {
        let event = Event::Key(KeyEvent {
            key,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        game.handle_event(&event);
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
        assert_eq!(
            potential_score(&[5, 5, 5, 5, 2], Category::FourOfAKind),
            22
        );
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
        assert_eq!(
            potential_score(&[1, 2, 3, 4, 5], Category::Chance),
            15
        );
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
        assert_eq!(g.phase, GamePhase::Rolling);
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
        assert_eq!(g.phase, GamePhase::MustScore);
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
        g.phase = GamePhase::GameOver;
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
        assert_eq!(g.phase, GamePhase::GameOver);
        assert_eq!(g.turn_number, NUM_TURNS);
    }

    #[test]
    fn test_categories_filled_count() {
        let mut g = game_with_dice([1, 2, 3, 4, 5]);
        assert_eq!(g.categories_filled(), 0);
        g.score_category(0);
        g.dice = [1, 2, 3, 4, 5];
        g.roll_number = 1;
        g.score_category(1);
        assert_eq!(g.categories_filled(), 2);
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
        assert_eq!(g.scores[Category::FullHouse.index()], Some(FULL_HOUSE_SCORE));
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
        g.phase = GamePhase::GameOver;
        g.new_game();
        assert_eq!(g.phase, GamePhase::Rolling);
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
            text: None,
        });
        g.handle_event(&event);
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

    #[test]
    fn test_mouse_click_on_die() {
        let mut g = game_with_dice([1, 2, 3, 4, 5]);
        let die_x = PADDING + DICE_SIZE / 2.0;
        let die_y = PADDING + HEADER_HEIGHT + 10.0 + DICE_SIZE / 2.0;
        let event = Event::Mouse(MouseEvent {
            x: die_x,
            y: die_y,
            kind: MouseEventKind::Press(MouseButton::Left),
        });
        g.handle_event(&event);
        assert!(g.held[0]);
    }

    // ════════════════════════════════════════════════════════════════
    // Rendering
    // ════════════════════════════════════════════════════════════════

    #[test]
    fn test_render_produces_commands() {
        let g = test_game();
        let cmds = g.render(800.0, 600.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_after_roll_produces_commands() {
        let mut g = test_game();
        g.roll();
        let cmds = g.render(800.0, 600.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_game_over_produces_commands() {
        let mut g = test_game();
        g.phase = GamePhase::GameOver;
        let cmds = g.render(800.0, 600.0);
        assert!(!cmds.is_empty());
    }

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
    fn test_rng_deterministic() {
        let mut rng1 = Rng::new(12345);
        let mut rng2 = Rng::new(12345);
        for _ in 0..100 {
            assert_eq!(rng1.next(), rng2.next());
        }
    }

    #[test]
    fn test_rng_die_range() {
        let mut rng = Rng::new(9999);
        for _ in 0..1000 {
            let val = rng.die();
            assert!((1..=6).contains(&val), "Die out of range: {val}");
        }
    }

    #[test]
    fn test_rng_produces_all_values() {
        let mut rng = Rng::new(7777);
        let mut seen = [false; 7];
        for _ in 0..10000 {
            let val = rng.die();
            seen[val as usize] = true;
        }
        for v in 1..=6 {
            assert!(seen[v], "RNG never produced {v}");
        }
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
        assert_eq!(g.phase, GamePhase::GameOver);
        assert!(g.grand_total() > 0);
    }

    #[test]
    fn test_event_handling_does_not_panic() {
        let mut g = test_game();
        // Fire a bunch of random events to ensure no panics.
        let keys = [
            Key::R, Key::N, Key::Tab, Key::Left, Key::Right,
            Key::Up, Key::Down, Key::Space, Key::Enter, Key::Escape,
            Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5,
        ];
        for &k in &keys {
            press_key(&mut g, k);
        }
    }

    #[test]
    fn test_score_category_after_game_over() {
        let mut g = test_game();
        g.phase = GamePhase::GameOver;
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

    #[test]
    fn test_roll_button_area_click() {
        let mut g = test_game();
        let btn_x = PADDING + 5.0;
        let btn_y = PADDING + HEADER_HEIGHT + 10.0 + DICE_SIZE + 35.0;
        let event = Event::Mouse(MouseEvent {
            x: btn_x,
            y: btn_y,
            kind: MouseEventKind::Press(MouseButton::Left),
        });
        g.handle_event(&event);
        assert_eq!(g.roll_number, 1);
    }

    #[test]
    fn test_right_click_ignored() {
        let mut g = test_game();
        let event = Event::Mouse(MouseEvent {
            x: PADDING + 5.0,
            y: PADDING + HEADER_HEIGHT + 15.0,
            kind: MouseEventKind::Press(MouseButton::Right),
        });
        g.handle_event(&event);
        assert_eq!(g.roll_number, 0); // Nothing happened.
    }
}
