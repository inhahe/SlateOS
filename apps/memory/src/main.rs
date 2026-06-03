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

//! Memory — card matching game.
//!
//! Flip two cards at a time to find matching pairs.
//! Supports 4x4 (8 pairs), 4x6 (12 pairs), and 6x6 (18 pairs) grids.

use guitk::color::Color;
use guitk::event::{Event, Key, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseEventKind};
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

const BASE: Color = Color::from_hex(0x1E1E2E);
const MANTLE: Color = Color::from_hex(0x181825);
const CRUST: Color = Color::from_hex(0x11111B);
const SURFACE0: Color = Color::from_hex(0x313244);
const SURFACE1: Color = Color::from_hex(0x45475A);
const TEXT: Color = Color::from_hex(0xCDD6F4);
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

// Symbols for card faces
const SYMBOLS: [&str; 18] = [
    "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R",
];
const SYMBOL_COLORS: [Color; 18] = [
    RED, BLUE, GREEN, YELLOW, PEACH, MAUVE, TEAL, LAVENDER, RED, BLUE, GREEN, YELLOW, PEACH, MAUVE,
    TEAL, LAVENDER, RED, BLUE,
];

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
    fn next_range(&mut self, max: usize) -> usize {
        if max == 0 {
            return 0;
        }
        (self.next() % max as u64) as usize
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum CardState {
    FaceDown,
    FaceUp,
    Matched,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum GameState {
    Playing,
    Won,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Phase {
    FirstPick,
    SecondPick,
    Showing, // brief display before hiding mismatched cards
}

struct MemoryGame {
    cards: Vec<u8>, // symbol index for each card
    states: Vec<CardState>,
    rows: usize,
    cols: usize,
    cursor: usize, // flat index
    first_pick: Option<usize>,
    second_pick: Option<usize>,
    phase: Phase,
    moves: u32,
    pairs_found: u32,
    total_pairs: u32,
    state: GameState,
    best_moves: [Option<u32>; 3], // 4x4, 4x6, 6x6
    rng: Rng,
    show_help: bool,
}

impl MemoryGame {
    fn new() -> Self {
        let rows = 4;
        let cols = 4;
        let mut rng = Rng::new(42);
        let cards = Self::generate_cards(rows, cols, &mut rng);
        let total = rows * cols;
        Self {
            cards,
            states: vec![CardState::FaceDown; total],
            rows,
            cols,
            cursor: 0,
            first_pick: None,
            second_pick: None,
            phase: Phase::FirstPick,
            moves: 0,
            pairs_found: 0,
            total_pairs: (total / 2) as u32,
            state: GameState::Playing,
            best_moves: [None; 3],
            rng,
            show_help: false,
        }
    }

    fn generate_cards(rows: usize, cols: usize, rng: &mut Rng) -> Vec<u8> {
        let total = rows * cols;
        let pairs = total / 2;
        let mut cards: Vec<u8> = Vec::with_capacity(total);
        for i in 0..pairs {
            let sym = (i % SYMBOLS.len()) as u8;
            cards.push(sym);
            cards.push(sym);
        }
        // Fisher-Yates shuffle
        for i in (1..cards.len()).rev() {
            let j = rng.next_range(i + 1);
            cards.swap(i, j);
        }
        cards
    }

    fn size_index(&self) -> usize {
        match (self.rows, self.cols) {
            (4, 4) => 0,
            (4, 6) | (6, 4) => 1,
            (6, 6) => 2,
            _ => 0,
        }
    }

    fn set_size(&mut self, rows: usize, cols: usize) {
        if (rows == 4 && (cols == 4 || cols == 6)) || (rows == 6 && cols == 6) {
            self.rows = rows;
            self.cols = cols;
            self.new_game();
        }
    }

    fn new_game(&mut self) {
        let total = self.rows * self.cols;
        self.cards = Self::generate_cards(self.rows, self.cols, &mut self.rng);
        self.states = vec![CardState::FaceDown; total];
        self.cursor = 0;
        self.first_pick = None;
        self.second_pick = None;
        self.phase = Phase::FirstPick;
        self.moves = 0;
        self.pairs_found = 0;
        self.total_pairs = (total / 2) as u32;
        self.state = GameState::Playing;
    }

    fn flip_card(&mut self, idx: usize) {
        if self.state != GameState::Playing || idx >= self.cards.len() {
            return;
        }
        if self.states[idx] != CardState::FaceDown {
            return;
        }

        match self.phase {
            Phase::FirstPick => {
                self.states[idx] = CardState::FaceUp;
                self.first_pick = Some(idx);
                self.phase = Phase::SecondPick;
            }
            Phase::SecondPick => {
                if Some(idx) == self.first_pick {
                    return;
                }
                self.states[idx] = CardState::FaceUp;
                self.second_pick = Some(idx);
                self.moves = self.moves.saturating_add(1);

                let first = self.first_pick.unwrap_or(0);
                if self.cards[first] == self.cards[idx] {
                    // Match found
                    self.states[first] = CardState::Matched;
                    self.states[idx] = CardState::Matched;
                    self.pairs_found = self.pairs_found.saturating_add(1);
                    self.first_pick = None;
                    self.second_pick = None;
                    self.phase = Phase::FirstPick;
                    self.check_win();
                } else {
                    self.phase = Phase::Showing;
                }
            }
            Phase::Showing => {
                // Dismiss the shown cards
                self.dismiss_shown();
            }
        }
    }

    fn dismiss_shown(&mut self) {
        if let Some(f) = self.first_pick
            && self.states[f] == CardState::FaceUp {
                self.states[f] = CardState::FaceDown;
            }
        if let Some(s) = self.second_pick
            && self.states[s] == CardState::FaceUp {
                self.states[s] = CardState::FaceDown;
            }
        self.first_pick = None;
        self.second_pick = None;
        self.phase = Phase::FirstPick;
    }

    fn check_win(&mut self) {
        if self.pairs_found == self.total_pairs {
            self.state = GameState::Won;
            let idx = self.size_index();
            if idx < 3 {
                if let Some(best) = self.best_moves[idx] {
                    if self.moves < best {
                        self.best_moves[idx] = Some(self.moves);
                    }
                } else {
                    self.best_moves[idx] = Some(self.moves);
                }
            }
        }
    }

    fn event(&mut self, event: &Event) {
        match event {
            Event::Key(KeyEvent { key, modifiers, .. })
                if *modifiers == Modifiers::NONE => {
                    match key {
                        Key::Up
                            if self.cursor >= self.cols => {
                                self.cursor -= self.cols;
                            }
                        Key::Down
                            if self.cursor + self.cols < self.rows * self.cols => {
                                self.cursor += self.cols;
                            }
                        Key::Left
                            if !self.cursor.is_multiple_of(self.cols) => {
                                self.cursor -= 1;
                            }
                        Key::Right
                            if self.cursor % self.cols < self.cols - 1 => {
                                self.cursor += 1;
                            }
                        Key::Enter | Key::Space => {
                            if self.phase == Phase::Showing {
                                self.dismiss_shown();
                            } else {
                                self.flip_card(self.cursor);
                            }
                        }
                        Key::N => self.new_game(),
                        Key::H => self.show_help = !self.show_help,
                        Key::Num1 => self.set_size(4, 4),
                        Key::Num2 => self.set_size(4, 6),
                        Key::Num3 => self.set_size(6, 6),
                        _ => {}
                    }
                }
            Event::Mouse(MouseEvent { x, y, kind }) => {
                if matches!(kind, MouseEventKind::Press(MouseButton::Left)) {
                    self.handle_mouse(*x, *y);
                }
            }
            _ => {}
        }
    }

    fn handle_mouse(&mut self, mx: f32, my: f32) {
        let grid_x = 50.0_f32;
        let grid_y = 80.0_f32;
        let card_w = 70.0_f32;
        let card_h = 80.0_f32;
        let gap = 8.0_f32;

        let total_w = self.cols as f32 * (card_w + gap);
        let total_h = self.rows as f32 * (card_h + gap);

        if mx < grid_x || my < grid_y || mx > grid_x + total_w || my > grid_y + total_h {
            return;
        }

        let col = ((mx - grid_x) / (card_w + gap)) as usize;
        let row = ((my - grid_y) / (card_h + gap)) as usize;

        if row < self.rows && col < self.cols {
            let idx = row * self.cols + col;
            self.cursor = idx;
            if self.phase == Phase::Showing {
                self.dismiss_shown();
            } else {
                self.flip_card(idx);
            }
        }
    }

    fn render(&self, width: f32, height: f32) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width,
            height,
            color: BASE,
            corner_radii: CornerRadii::ZERO,
        });

        cmds.push(RenderCommand::Text {
            x: 50.0,
            y: 28.0,
            text: "Memory".into(),
            color: LAVENDER,
            font_size: 24.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        let size_label = format!(
            "{}x{}   Moves: {}   Pairs: {}/{}",
            self.rows, self.cols, self.moves, self.pairs_found, self.total_pairs
        );
        cmds.push(RenderCommand::Text {
            x: 50.0,
            y: 56.0,
            text: size_label,
            color: SUBTEXT0,
            font_size: 14.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Grid
        let grid_x = 50.0_f32;
        let grid_y = 80.0_f32;
        let card_w = 70.0_f32;
        let card_h = 80.0_f32;
        let gap = 8.0_f32;

        for row in 0..self.rows {
            for col in 0..self.cols {
                let idx = row * self.cols + col;
                let cx = grid_x + col as f32 * (card_w + gap);
                let cy = grid_y + row as f32 * (card_h + gap);
                let is_cursor = idx == self.cursor;

                match self.states[idx] {
                    CardState::FaceDown => {
                        cmds.push(RenderCommand::FillRect {
                            x: cx,
                            y: cy,
                            width: card_w,
                            height: card_h,
                            color: SURFACE1,
                            corner_radii: CornerRadii::all(6.0),
                        });
                        cmds.push(RenderCommand::Text {
                            x: cx + card_w / 2.0 - 5.0,
                            y: cy + card_h / 2.0 - 8.0,
                            text: "?".into(),
                            color: OVERLAY0,
                            font_size: 20.0,
                            font_weight: FontWeightHint::Bold,
                            max_width: None,
                        });
                    }
                    CardState::FaceUp => {
                        let sym_idx = self.cards[idx] as usize;
                        let sym = SYMBOLS[sym_idx % SYMBOLS.len()];
                        let sym_color = SYMBOL_COLORS[sym_idx % SYMBOL_COLORS.len()];
                        cmds.push(RenderCommand::FillRect {
                            x: cx,
                            y: cy,
                            width: card_w,
                            height: card_h,
                            color: MANTLE,
                            corner_radii: CornerRadii::all(6.0),
                        });
                        cmds.push(RenderCommand::Text {
                            x: cx + card_w / 2.0 - 8.0,
                            y: cy + card_h / 2.0 - 12.0,
                            text: sym.into(),
                            color: sym_color,
                            font_size: 28.0,
                            font_weight: FontWeightHint::Bold,
                            max_width: None,
                        });
                    }
                    CardState::Matched => {
                        let sym_idx = self.cards[idx] as usize;
                        let sym = SYMBOLS[sym_idx % SYMBOLS.len()];
                        cmds.push(RenderCommand::FillRect {
                            x: cx,
                            y: cy,
                            width: card_w,
                            height: card_h,
                            color: CRUST,
                            corner_radii: CornerRadii::all(6.0),
                        });
                        cmds.push(RenderCommand::Text {
                            x: cx + card_w / 2.0 - 8.0,
                            y: cy + card_h / 2.0 - 12.0,
                            text: sym.into(),
                            color: OVERLAY0,
                            font_size: 28.0,
                            font_weight: FontWeightHint::Regular,
                            max_width: None,
                        });
                    }
                }

                // Cursor border
                if is_cursor {
                    for (bx, by, bw, bh) in [
                        (cx, cy, card_w, 3.0),
                        (cx, cy + card_h - 3.0, card_w, 3.0),
                        (cx, cy, 3.0, card_h),
                        (cx + card_w - 3.0, cy, 3.0, card_h),
                    ] {
                        cmds.push(RenderCommand::FillRect {
                            x: bx,
                            y: by,
                            width: bw,
                            height: bh,
                            color: YELLOW,
                            corner_radii: CornerRadii::ZERO,
                        });
                    }
                }
            }
        }

        // Win
        if self.state == GameState::Won {
            let total_h = self.rows as f32 * (card_h + gap);
            cmds.push(RenderCommand::Text {
                x: grid_x,
                y: grid_y + total_h + 16.0,
                text: format!("All pairs found in {} moves!", self.moves),
                color: GREEN,
                font_size: 18.0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
        }

        // Best scores
        let total_w = self.cols as f32 * (card_w + gap);
        let panel_x = grid_x + total_w + 20.0;
        let panel_y = grid_y;
        cmds.push(RenderCommand::FillRect {
            x: panel_x,
            y: panel_y,
            width: 150.0,
            height: 120.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(8.0),
        });
        cmds.push(RenderCommand::Text {
            x: panel_x + 10.0,
            y: panel_y + 14.0,
            text: "Best Scores".into(),
            color: YELLOW,
            font_size: 14.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        let labels = ["4x4", "4x6", "6x6"];
        for (i, label) in labels.iter().enumerate() {
            let sy = panel_y + 38.0 + i as f32 * 24.0;
            let s = match self.best_moves[i] {
                Some(m) => format!("{}: {} moves", label, m),
                None => format!("{}: ---", label),
            };
            cmds.push(RenderCommand::Text {
                x: panel_x + 10.0,
                y: sy,
                text: s,
                color: TEXT,
                font_size: 12.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }

        // Help hint
        if !self.show_help {
            cmds.push(RenderCommand::Text {
                x: panel_x + 10.0,
                y: panel_y + 130.0,
                text: "H for help".into(),
                color: OVERLAY0,
                font_size: 12.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }

        cmds
    }
}

fn main() {
    let _app = MemoryGame::new();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initial_state() {
        let app = MemoryGame::new();
        assert_eq!(app.rows, 4);
        assert_eq!(app.cols, 4);
        assert_eq!(app.moves, 0);
        assert_eq!(app.pairs_found, 0);
        assert_eq!(app.total_pairs, 8);
        assert_eq!(app.state, GameState::Playing);
    }

    #[test]
    fn test_card_count() {
        let app = MemoryGame::new();
        assert_eq!(app.cards.len(), 16);
        assert_eq!(app.states.len(), 16);
    }

    #[test]
    fn test_cards_have_pairs() {
        let app = MemoryGame::new();
        let mut counts = [0u32; 18];
        for &c in &app.cards {
            counts[c as usize] += 1;
        }
        // Every symbol used should appear exactly twice
        for &count in &counts {
            assert!(count == 0 || count == 2);
        }
    }

    #[test]
    fn test_flip_first_card() {
        let mut app = MemoryGame::new();
        app.flip_card(0);
        assert_eq!(app.states[0], CardState::FaceUp);
        assert_eq!(app.phase, Phase::SecondPick);
        assert_eq!(app.first_pick, Some(0));
    }

    #[test]
    fn test_flip_same_card_twice() {
        let mut app = MemoryGame::new();
        app.flip_card(0);
        app.flip_card(0); // same card
        assert_eq!(app.phase, Phase::SecondPick); // no change
    }

    #[test]
    fn test_flip_matching_pair() {
        let mut app = MemoryGame::new();
        // Find a matching pair
        let first_sym = app.cards[0];
        let mut second_idx = None;
        for i in 1..app.cards.len() {
            if app.cards[i] == first_sym {
                second_idx = Some(i);
                break;
            }
        }
        if let Some(si) = second_idx {
            app.flip_card(0);
            app.flip_card(si);
            assert_eq!(app.states[0], CardState::Matched);
            assert_eq!(app.states[si], CardState::Matched);
            assert_eq!(app.pairs_found, 1);
            assert_eq!(app.moves, 1);
            assert_eq!(app.phase, Phase::FirstPick);
        }
    }

    #[test]
    fn test_flip_non_matching() {
        let mut app = MemoryGame::new();
        // Find a non-matching pair
        let first_sym = app.cards[0];
        let mut second_idx = None;
        for i in 1..app.cards.len() {
            if app.cards[i] != first_sym {
                second_idx = Some(i);
                break;
            }
        }
        if let Some(si) = second_idx {
            app.flip_card(0);
            app.flip_card(si);
            assert_eq!(app.phase, Phase::Showing);
            assert_eq!(app.moves, 1);
        }
    }

    #[test]
    fn test_dismiss_shown() {
        let mut app = MemoryGame::new();
        let first_sym = app.cards[0];
        let mut second_idx = None;
        for i in 1..app.cards.len() {
            if app.cards[i] != first_sym {
                second_idx = Some(i);
                break;
            }
        }
        if let Some(si) = second_idx {
            app.flip_card(0);
            app.flip_card(si);
            assert_eq!(app.phase, Phase::Showing);
            app.dismiss_shown();
            assert_eq!(app.states[0], CardState::FaceDown);
            assert_eq!(app.states[si], CardState::FaceDown);
            assert_eq!(app.phase, Phase::FirstPick);
        }
    }

    #[test]
    fn test_flip_matched_card() {
        let mut app = MemoryGame::new();
        app.states[0] = CardState::Matched;
        app.flip_card(0);
        assert_eq!(app.states[0], CardState::Matched); // no change
    }

    #[test]
    fn test_flip_when_won() {
        let mut app = MemoryGame::new();
        app.state = GameState::Won;
        app.flip_card(0);
        assert_eq!(app.states[0], CardState::FaceDown); // no change
    }

    #[test]
    fn test_new_game() {
        let mut app = MemoryGame::new();
        app.moves = 10;
        app.pairs_found = 5;
        app.new_game();
        assert_eq!(app.moves, 0);
        assert_eq!(app.pairs_found, 0);
        assert_eq!(app.state, GameState::Playing);
    }

    #[test]
    fn test_set_size_4x6() {
        let mut app = MemoryGame::new();
        app.set_size(4, 6);
        assert_eq!(app.rows, 4);
        assert_eq!(app.cols, 6);
        assert_eq!(app.cards.len(), 24);
        assert_eq!(app.total_pairs, 12);
    }

    #[test]
    fn test_set_size_6x6() {
        let mut app = MemoryGame::new();
        app.set_size(6, 6);
        assert_eq!(app.rows, 6);
        assert_eq!(app.cols, 6);
        assert_eq!(app.cards.len(), 36);
    }

    #[test]
    fn test_set_size_invalid() {
        let mut app = MemoryGame::new();
        app.set_size(5, 5);
        assert_eq!(app.rows, 4); // unchanged
    }

    #[test]
    fn test_size_index() {
        let mut app = MemoryGame::new();
        assert_eq!(app.size_index(), 0);
        app.rows = 4;
        app.cols = 6;
        assert_eq!(app.size_index(), 1);
        app.rows = 6;
        app.cols = 6;
        assert_eq!(app.size_index(), 2);
    }

    #[test]
    fn test_win_detection() {
        let mut app = MemoryGame::new();
        // Mark all as matched
        for s in &mut app.states {
            *s = CardState::Matched;
        }
        app.pairs_found = app.total_pairs;
        app.check_win();
        assert_eq!(app.state, GameState::Won);
    }

    #[test]
    fn test_best_score_recorded() {
        let mut app = MemoryGame::new();
        app.pairs_found = app.total_pairs;
        app.moves = 10;
        app.check_win();
        assert_eq!(app.best_moves[0], Some(10));
    }

    #[test]
    fn test_best_score_improves() {
        let mut app = MemoryGame::new();
        app.best_moves[0] = Some(15);
        app.pairs_found = app.total_pairs;
        app.moves = 10;
        app.check_win();
        assert_eq!(app.best_moves[0], Some(10));
    }

    #[test]
    fn test_key_navigation() {
        let mut app = MemoryGame::new();
        app.cursor = 5;
        let right = Event::Key(KeyEvent {
            key: Key::Right,
            modifiers: Modifiers::NONE,
            pressed: true,
            text: None,
        });
        app.event(&right);
        assert_eq!(app.cursor, 6);
    }

    #[test]
    fn test_key_enter() {
        let mut app = MemoryGame::new();
        let evt = Event::Key(KeyEvent {
            key: Key::Enter,
            modifiers: Modifiers::NONE,
            pressed: true,
            text: None,
        });
        app.event(&evt);
        assert_eq!(app.states[0], CardState::FaceUp);
    }

    #[test]
    fn test_key_n() {
        let mut app = MemoryGame::new();
        app.moves = 20;
        let evt = Event::Key(KeyEvent {
            key: Key::N,
            modifiers: Modifiers::NONE,
            pressed: true,
            text: None,
        });
        app.event(&evt);
        assert_eq!(app.moves, 0);
    }

    #[test]
    fn test_render() {
        let app = MemoryGame::new();
        let cmds = app.render(800.0, 600.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_won() {
        let mut app = MemoryGame::new();
        app.state = GameState::Won;
        let cmds = app.render(800.0, 600.0);
        assert!(cmds.len() > 5);
    }

    #[test]
    fn test_generate_deterministic() {
        let mut r1 = Rng::new(42);
        let c1 = MemoryGame::generate_cards(4, 4, &mut r1);
        let mut r2 = Rng::new(42);
        let c2 = MemoryGame::generate_cards(4, 4, &mut r2);
        assert_eq!(c1, c2);
    }

    #[test]
    fn test_game_state_eq() {
        assert_eq!(GameState::Playing, GameState::Playing);
        assert_ne!(GameState::Playing, GameState::Won);
    }

    #[test]
    fn test_card_state_eq() {
        assert_eq!(CardState::FaceDown, CardState::FaceDown);
        assert_ne!(CardState::FaceDown, CardState::FaceUp);
    }

    #[test]
    fn test_phase_eq() {
        assert_eq!(Phase::FirstPick, Phase::FirstPick);
        assert_ne!(Phase::FirstPick, Phase::SecondPick);
    }
}
