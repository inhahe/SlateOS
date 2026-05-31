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

//! Nim — a mathematical strategy game.
//!
//! Players take turns removing objects from heaps.
//! The player who takes the last object loses (misère) or wins (normal).
//! Includes perfect AI using Nim-sum (XOR) strategy.

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

const HEAP_COLORS: [Color; 5] = [RED, PEACH, YELLOW, GREEN, TEAL];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Player {
    Human,
    Computer,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum GameState {
    Playing,
    Won(Player),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum NimVariant {
    Misere, // Last to take loses
    Normal, // Last to take wins
}

struct Nim {
    heaps: Vec<u32>,
    selected_heap: usize,
    take_count: u32,
    current_player: Player,
    state: GameState,
    variant: NimVariant,
    preset: usize, // which preset configuration
    show_help: bool,
    scores: [u32; 2], // [human, computer]
}

// Preset heap configurations
const PRESETS: [[u32; 4]; 5] = [
    [1, 3, 5, 7], // Classic
    [3, 4, 5, 0], // Three heaps
    [2, 3, 4, 5], // Four heaps
    [1, 2, 3, 0], // Simple
    [5, 7, 9, 0], // Large
];

const PRESET_NAMES: [&str; 5] = [
    "Classic (1,3,5,7)",
    "Three (3,4,5)",
    "Four (2,3,4,5)",
    "Simple (1,2,3)",
    "Large (5,7,9)",
];

impl Nim {
    fn new() -> Self {
        let preset = 0;
        let heaps = Self::heaps_from_preset(preset);
        Self {
            heaps,
            selected_heap: 0,
            take_count: 1,
            current_player: Player::Human,
            state: GameState::Playing,
            variant: NimVariant::Misere,
            preset,
            show_help: false,
            scores: [0, 0],
        }
    }

    fn heaps_from_preset(preset: usize) -> Vec<u32> {
        let p = PRESETS[preset % PRESETS.len()];
        p.iter().copied().filter(|&x| x > 0).collect()
    }

    fn new_game(&mut self) {
        self.heaps = Self::heaps_from_preset(self.preset);
        self.selected_heap = 0;
        self.take_count = 1;
        self.current_player = Player::Human;
        self.state = GameState::Playing;
    }

    fn set_preset(&mut self, p: usize) {
        if p < PRESETS.len() {
            self.preset = p;
            self.new_game();
        }
    }

    fn toggle_variant(&mut self) {
        self.variant = match self.variant {
            NimVariant::Misere => NimVariant::Normal,
            NimVariant::Normal => NimVariant::Misere,
        };
        self.new_game();
    }

    fn total_remaining(&self) -> u32 {
        self.heaps.iter().sum()
    }

    fn nim_sum(&self) -> u32 {
        self.heaps.iter().fold(0u32, |acc, &h| acc ^ h)
    }

    fn take(&mut self, heap: usize, count: u32) -> bool {
        if self.state != GameState::Playing {
            return false;
        }
        if heap >= self.heaps.len() || count == 0 || count > self.heaps[heap] {
            return false;
        }
        self.heaps[heap] = self.heaps[heap].saturating_sub(count);

        // Check for game end
        if self.total_remaining() == 0 {
            let winner = match self.variant {
                NimVariant::Misere => {
                    // Last to take loses
                    match self.current_player {
                        Player::Human => Player::Computer,
                        Player::Computer => Player::Human,
                    }
                }
                NimVariant::Normal => self.current_player,
            };
            self.state = GameState::Won(winner);
            match winner {
                Player::Human => self.scores[0] = self.scores[0].saturating_add(1),
                Player::Computer => self.scores[1] = self.scores[1].saturating_add(1),
            }
        } else {
            self.current_player = match self.current_player {
                Player::Human => Player::Computer,
                Player::Computer => Player::Human,
            };
        }
        true
    }

    fn computer_move(&mut self) {
        if self.current_player != Player::Computer || self.state != GameState::Playing {
            return;
        }

        let ns = self.nim_sum();
        let total = self.total_remaining();

        if self.variant == NimVariant::Misere && total == 1 {
            // Last object — take it, but we lose
            for i in 0..self.heaps.len() {
                if self.heaps[i] > 0 {
                    self.take(i, 1);
                    return;
                }
            }
        }

        // Count non-empty heaps
        let non_empty: Vec<usize> = (0..self.heaps.len())
            .filter(|&i| self.heaps[i] > 0)
            .collect();

        if self.variant == NimVariant::Misere {
            // Misère endgame: if all heaps are 0 or 1, take to leave odd number
            let all_small = self.heaps.iter().all(|&h| h <= 1);
            if all_small {
                let ones = self.heaps.iter().filter(|&&h| h == 1).count();
                if ones % 2 == 0 {
                    // Even ones — take one to make it odd
                    for i in 0..self.heaps.len() {
                        if self.heaps[i] == 1 {
                            self.take(i, 1);
                            return;
                        }
                    }
                }
                // Odd ones — any move loses, just take one
                for i in 0..self.heaps.len() {
                    if self.heaps[i] > 0 {
                        self.take(i, 1);
                        return;
                    }
                }
            }
        }

        if ns != 0 {
            // Winning position: make a move to set nim-sum to 0
            for i in 0..self.heaps.len() {
                let target = self.heaps[i] ^ ns;
                if target < self.heaps[i] {
                    let remove = self.heaps[i] - target;

                    // Misère adjustment: if this would leave all heaps ≤ 1
                    if self.variant == NimVariant::Misere {
                        let mut test_heaps = self.heaps.clone();
                        test_heaps[i] = target;
                        let all_le1 = test_heaps.iter().all(|&h| h <= 1);
                        if all_le1 {
                            let ones = test_heaps.iter().filter(|&&h| h == 1).count();
                            if ones % 2 == 0 {
                                // Good for misère
                                self.take(i, remove);
                                return;
                            }
                            // Try leaving one more/less
                            if target > 0 {
                                self.take(i, remove + 1);
                                return;
                            }
                        }
                    }

                    self.take(i, remove);
                    return;
                }
            }
        }

        // Losing position or fallback: take 1 from largest heap
        let max_i = non_empty
            .iter()
            .copied()
            .max_by_key(|&i| self.heaps[i])
            .unwrap_or(0);
        self.take(max_i, 1);
    }

    fn event(&mut self, event: &Event) {
        match event {
            Event::Key(KeyEvent { key, modifiers, .. }) => {
                if *modifiers == Modifiers::NONE {
                    match key {
                        Key::Left => {
                            if self.selected_heap > 0 {
                                self.selected_heap -= 1;
                                self.take_count = 1;
                            }
                        }
                        Key::Right => {
                            if self.selected_heap + 1 < self.heaps.len() {
                                self.selected_heap += 1;
                                self.take_count = 1;
                            }
                        }
                        Key::Up => {
                            let max = self.heaps.get(self.selected_heap).copied().unwrap_or(0);
                            if self.take_count < max {
                                self.take_count += 1;
                            }
                        }
                        Key::Down => {
                            if self.take_count > 1 {
                                self.take_count -= 1;
                            }
                        }
                        Key::Enter | Key::Space => {
                            if self.current_player == Player::Human
                                && self.state == GameState::Playing
                            {
                                if self.take(self.selected_heap, self.take_count) {
                                    self.take_count = 1;
                                    self.computer_move();
                                }
                            }
                        }
                        Key::N => self.new_game(),
                        Key::H => self.show_help = !self.show_help,
                        Key::V => self.toggle_variant(),
                        Key::Num1 => self.set_preset(0),
                        Key::Num2 => self.set_preset(1),
                        Key::Num3 => self.set_preset(2),
                        Key::Num4 => self.set_preset(3),
                        Key::Num5 => self.set_preset(4),
                        _ => {}
                    }
                }
            }
            _ => {}
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

        // Title
        let variant_label = match self.variant {
            NimVariant::Misere => "Misère",
            NimVariant::Normal => "Normal",
        };
        cmds.push(RenderCommand::Text {
            x: 50.0,
            y: 28.0,
            text: format!("Nim ({})", variant_label),
            color: LAVENDER,
            font_size: 24.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Preset name
        cmds.push(RenderCommand::Text {
            x: 50.0,
            y: 56.0,
            text: PRESET_NAMES[self.preset].into(),
            color: SUBTEXT0,
            font_size: 13.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Turn / status
        let status = match self.state {
            GameState::Playing => match self.current_player {
                Player::Human => "Your turn".into(),
                Player::Computer => "Computer thinking...".into(),
            },
            GameState::Won(Player::Human) => "You win!".into(),
            GameState::Won(Player::Computer) => "Computer wins!".into(),
        };
        let status_color = match self.state {
            GameState::Won(Player::Human) => GREEN,
            GameState::Won(Player::Computer) => RED,
            _ => TEXT,
        };
        cmds.push(RenderCommand::Text {
            x: 300.0,
            y: 56.0,
            text: status,
            color: status_color,
            font_size: 14.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Scores
        cmds.push(RenderCommand::Text {
            x: 500.0,
            y: 56.0,
            text: format!("Score: You {} - {} CPU", self.scores[0], self.scores[1]),
            color: SUBTEXT0,
            font_size: 13.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Heaps
        let heap_y = 100.0_f32;
        let heap_spacing = 150.0_f32;
        let token_size = 24.0_f32;
        let token_gap = 4.0_f32;

        for (i, &count) in self.heaps.iter().enumerate() {
            let hx = 60.0 + i as f32 * heap_spacing;
            let is_selected = i == self.selected_heap;

            // Heap label
            let label_color = if is_selected { YELLOW } else { SUBTEXT0 };
            cmds.push(RenderCommand::Text {
                x: hx,
                y: heap_y,
                text: format!("Heap {} ({})", i + 1, count),
                color: label_color,
                font_size: 14.0,
                font_weight: if is_selected {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: None,
            });

            // Tokens
            let color = HEAP_COLORS[i % HEAP_COLORS.len()];
            for j in 0..count {
                let ty = heap_y + 24.0 + j as f32 * (token_size + token_gap);
                let is_marked = is_selected && j >= count.saturating_sub(self.take_count);
                let tc = if is_marked { SURFACE1 } else { color };
                cmds.push(RenderCommand::FillRect {
                    x: hx,
                    y: ty,
                    width: token_size * 3.0,
                    height: token_size,
                    color: tc,
                    corner_radii: CornerRadii::all(4.0),
                });
                if is_marked {
                    cmds.push(RenderCommand::Text {
                        x: hx + token_size,
                        y: ty + 4.0,
                        text: "X".into(),
                        color: RED,
                        font_size: 14.0,
                        font_weight: FontWeightHint::Bold,
                        max_width: None,
                    });
                }
            }

            // Selection indicator
            if is_selected {
                cmds.push(RenderCommand::FillRect {
                    x: hx,
                    y: heap_y + 24.0 + count as f32 * (token_size + token_gap) + 4.0,
                    width: token_size * 3.0,
                    height: 3.0,
                    color: YELLOW,
                    corner_radii: CornerRadii::all(1.0),
                });
            }
        }

        // Take info
        if self.state == GameState::Playing && self.current_player == Player::Human {
            let take_y = heap_y + 24.0 + 10.0 * (token_size + token_gap);
            cmds.push(RenderCommand::Text {
                x: 60.0,
                y: take_y,
                text: format!(
                    "Taking {} from Heap {}",
                    self.take_count,
                    self.selected_heap + 1
                ),
                color: TEXT,
                font_size: 14.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
            cmds.push(RenderCommand::Text {
                x: 60.0,
                y: take_y + 22.0,
                text: "Up/Down to change amount, Enter to take".into(),
                color: OVERLAY0,
                font_size: 12.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }

        // Help panel
        if self.show_help {
            let hx = 500.0_f32;
            let hy = 100.0_f32;
            cmds.push(RenderCommand::FillRect {
                x: hx,
                y: hy,
                width: 200.0,
                height: 220.0,
                color: SURFACE0,
                corner_radii: CornerRadii::all(8.0),
            });
            cmds.push(RenderCommand::Text {
                x: hx + 10.0,
                y: hy + 14.0,
                text: "Controls".into(),
                color: YELLOW,
                font_size: 14.0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
            let lines = [
                ("Left/Right", "Select heap"),
                ("Up/Down", "Change amount"),
                ("Enter", "Take tokens"),
                ("1-5", "Change preset"),
                ("V", "Toggle variant"),
                ("N", "New game"),
                ("H", "Toggle help"),
            ];
            for (i, (k, v)) in lines.iter().enumerate() {
                let ly = hy + 38.0 + i as f32 * 24.0;
                cmds.push(RenderCommand::Text {
                    x: hx + 10.0,
                    y: ly,
                    text: (*k).into(),
                    color: BLUE,
                    font_size: 11.0,
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                });
                cmds.push(RenderCommand::Text {
                    x: hx + 95.0,
                    y: ly,
                    text: (*v).into(),
                    color: SUBTEXT0,
                    font_size: 11.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                });
            }
        }

        cmds
    }
}

fn main() {
    let _app = Nim::new();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initial_state() {
        let app = Nim::new();
        assert_eq!(app.heaps, vec![1, 3, 5, 7]);
        assert_eq!(app.state, GameState::Playing);
        assert_eq!(app.current_player, Player::Human);
    }

    #[test]
    fn test_total_remaining() {
        let app = Nim::new();
        assert_eq!(app.total_remaining(), 16);
    }

    #[test]
    fn test_nim_sum() {
        let app = Nim::new();
        assert_eq!(app.nim_sum(), 1 ^ 3 ^ 5 ^ 7);
    }

    #[test]
    fn test_take_valid() {
        let mut app = Nim::new();
        assert!(app.take(0, 1));
        assert_eq!(app.heaps[0], 0);
        assert_eq!(app.current_player, Player::Computer);
    }

    #[test]
    fn test_take_too_many() {
        let mut app = Nim::new();
        assert!(!app.take(0, 5));
    }

    #[test]
    fn test_take_zero() {
        let mut app = Nim::new();
        assert!(!app.take(0, 0));
    }

    #[test]
    fn test_take_invalid_heap() {
        let mut app = Nim::new();
        assert!(!app.take(10, 1));
    }

    #[test]
    fn test_take_when_won() {
        let mut app = Nim::new();
        app.state = GameState::Won(Player::Human);
        assert!(!app.take(0, 1));
    }

    #[test]
    fn test_win_misere() {
        let mut app = Nim::new();
        app.heaps = vec![1];
        app.variant = NimVariant::Misere;
        app.take(0, 1);
        // In misère, taking last loses
        assert_eq!(app.state, GameState::Won(Player::Computer));
    }

    #[test]
    fn test_win_normal() {
        let mut app = Nim::new();
        app.heaps = vec![1];
        app.variant = NimVariant::Normal;
        app.take(0, 1);
        assert_eq!(app.state, GameState::Won(Player::Human));
    }

    #[test]
    fn test_score_tracking() {
        let mut app = Nim::new();
        app.heaps = vec![1];
        app.variant = NimVariant::Normal;
        app.take(0, 1);
        assert_eq!(app.scores[0], 1); // human won
    }

    #[test]
    fn test_new_game() {
        let mut app = Nim::new();
        app.heaps = vec![0, 0, 0, 0];
        app.state = GameState::Won(Player::Human);
        app.new_game();
        assert_eq!(app.heaps, vec![1, 3, 5, 7]);
        assert_eq!(app.state, GameState::Playing);
    }

    #[test]
    fn test_set_preset() {
        let mut app = Nim::new();
        app.set_preset(1);
        assert_eq!(app.heaps, vec![3, 4, 5]);
        assert_eq!(app.preset, 1);
    }

    #[test]
    fn test_set_preset_invalid() {
        let mut app = Nim::new();
        app.set_preset(100);
        assert_eq!(app.preset, 0); // unchanged
    }

    #[test]
    fn test_toggle_variant() {
        let mut app = Nim::new();
        assert_eq!(app.variant, NimVariant::Misere);
        app.toggle_variant();
        assert_eq!(app.variant, NimVariant::Normal);
        app.toggle_variant();
        assert_eq!(app.variant, NimVariant::Misere);
    }

    #[test]
    fn test_heaps_from_preset() {
        assert_eq!(Nim::heaps_from_preset(0), vec![1, 3, 5, 7]);
        assert_eq!(Nim::heaps_from_preset(1), vec![3, 4, 5]);
        assert_eq!(Nim::heaps_from_preset(3), vec![1, 2, 3]);
    }

    #[test]
    fn test_computer_move_takes() {
        let mut app = Nim::new();
        app.current_player = Player::Computer;
        let before = app.total_remaining();
        app.computer_move();
        assert!(app.total_remaining() < before);
    }

    #[test]
    fn test_computer_move_when_human() {
        let mut app = Nim::new();
        let before = app.total_remaining();
        app.computer_move();
        assert_eq!(app.total_remaining(), before);
    }

    #[test]
    fn test_key_left() {
        let mut app = Nim::new();
        app.selected_heap = 2;
        let evt = Event::Key(KeyEvent {
            key: Key::Left,
            modifiers: Modifiers::NONE,
            pressed: true,
            text: None,
        });
        app.event(&evt);
        assert_eq!(app.selected_heap, 1);
    }

    #[test]
    fn test_key_right() {
        let mut app = Nim::new();
        app.selected_heap = 0;
        let evt = Event::Key(KeyEvent {
            key: Key::Right,
            modifiers: Modifiers::NONE,
            pressed: true,
            text: None,
        });
        app.event(&evt);
        assert_eq!(app.selected_heap, 1);
    }

    #[test]
    fn test_key_up_down() {
        let mut app = Nim::new();
        app.selected_heap = 1; // heap with 3
        let up = Event::Key(KeyEvent {
            key: Key::Up,
            modifiers: Modifiers::NONE,
            pressed: true,
            text: None,
        });
        app.event(&up);
        assert_eq!(app.take_count, 2);
        let down = Event::Key(KeyEvent {
            key: Key::Down,
            modifiers: Modifiers::NONE,
            pressed: true,
            text: None,
        });
        app.event(&down);
        assert_eq!(app.take_count, 1);
    }

    #[test]
    fn test_key_enter_takes() {
        let mut app = Nim::new();
        app.selected_heap = 0;
        app.take_count = 1;
        let evt = Event::Key(KeyEvent {
            key: Key::Enter,
            modifiers: Modifiers::NONE,
            pressed: true,
            text: None,
        });
        app.event(&evt);
        // Human took, then computer moved
        assert_eq!(app.current_player, Player::Human);
    }

    #[test]
    fn test_key_n_new_game() {
        let mut app = Nim::new();
        app.heaps = vec![0, 0, 0, 0];
        let evt = Event::Key(KeyEvent {
            key: Key::N,
            modifiers: Modifiers::NONE,
            pressed: true,
            text: None,
        });
        app.event(&evt);
        assert_eq!(app.total_remaining(), 16);
    }

    #[test]
    fn test_key_v_toggle() {
        let mut app = Nim::new();
        let evt = Event::Key(KeyEvent {
            key: Key::V,
            modifiers: Modifiers::NONE,
            pressed: true,
            text: None,
        });
        app.event(&evt);
        assert_eq!(app.variant, NimVariant::Normal);
    }

    #[test]
    fn test_render() {
        let app = Nim::new();
        let cmds = app.render(800.0, 600.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_won() {
        let mut app = Nim::new();
        app.state = GameState::Won(Player::Human);
        let cmds = app.render(800.0, 600.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_help() {
        let mut app = Nim::new();
        app.show_help = true;
        let cmds = app.render(800.0, 600.0);
        assert!(cmds.len() > 20);
    }

    #[test]
    fn test_player_eq() {
        assert_eq!(Player::Human, Player::Human);
        assert_ne!(Player::Human, Player::Computer);
    }

    #[test]
    fn test_game_state_eq() {
        assert_eq!(GameState::Playing, GameState::Playing);
        assert_eq!(GameState::Won(Player::Human), GameState::Won(Player::Human));
        assert_ne!(
            GameState::Won(Player::Human),
            GameState::Won(Player::Computer)
        );
    }

    #[test]
    fn test_nim_sum_zero() {
        let mut app = Nim::new();
        app.heaps = vec![3, 3];
        assert_eq!(app.nim_sum(), 0);
    }

    #[test]
    fn test_computer_optimal_play() {
        let mut app = Nim::new();
        app.heaps = vec![3, 3];
        app.current_player = Player::Computer;
        app.variant = NimVariant::Normal;
        app.computer_move();
        // Optimal: nim_sum is 0, so computer is in losing position
        // It should still make a valid move
        assert!(app.total_remaining() < 6);
    }

    #[test]
    fn test_take_count_capped() {
        let mut app = Nim::new();
        app.selected_heap = 0; // heap with 1
        // Try to increase beyond heap size
        app.take_count = 1;
        let up = Event::Key(KeyEvent {
            key: Key::Up,
            modifiers: Modifiers::NONE,
            pressed: true,
            text: None,
        });
        app.event(&up);
        assert_eq!(app.take_count, 1); // can't go above 1
    }

    #[test]
    fn test_take_count_min() {
        let mut app = Nim::new();
        app.take_count = 1;
        let down = Event::Key(KeyEvent {
            key: Key::Down,
            modifiers: Modifiers::NONE,
            pressed: true,
            text: None,
        });
        app.event(&down);
        assert_eq!(app.take_count, 1); // can't go below 1
    }
}
