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

//! Tower of Hanoi — classic disk-moving puzzle.
//!
//! Move all disks from the left peg to the right peg.
//! Only one disk at a time, never place a larger disk on a smaller one.
//! Supports 3-8 disks. Minimum moves = 2^n - 1.

use guitk::color::Color;
use guitk::event::{Event, Key, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseEventKind};
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

// ── Catppuccin Mocha palette ──
const BASE: Color = Color::from_hex(0x1E1E2E);
const MANTLE: Color = Color::from_hex(0x181825);
const CRUST: Color = Color::from_hex(0x11111B);
const SURFACE0: Color = Color::from_hex(0x313244);
const SURFACE1: Color = Color::from_hex(0x45475A);
const SURFACE2: Color = Color::from_hex(0x585B70);
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

// Disk colors by size
const DISK_COLORS: [Color; 8] = [RED, PEACH, YELLOW, GREEN, TEAL, BLUE, MAUVE, LAVENDER];

const MAX_DISKS: usize = 8;
const NUM_PEGS: usize = 3;

#[derive(Clone, Copy, PartialEq, Eq)]
enum GameState {
    Playing,
    Won,
}

struct TowersOfHanoi {
    pegs: [Vec<u8>; NUM_PEGS],  // disk sizes, bottom to top, 1 = smallest
    num_disks: usize,
    moves: u32,
    selected_peg: usize,        // cursor position (0-2)
    held_disk: Option<(usize, u8)>,  // (from_peg, disk_size)
    state: GameState,
    best_moves: [Option<u32>; 6],  // best for 3-8 disks (index = disks - 3)
    show_help: bool,
    undo_stack: Vec<(usize, usize)>,  // (from_peg, to_peg)
}

impl TowersOfHanoi {
    fn new() -> Self {
        let mut app = Self {
            pegs: [Vec::new(), Vec::new(), Vec::new()],
            num_disks: 4,
            moves: 0,
            selected_peg: 0,
            held_disk: None,
            state: GameState::Playing,
            best_moves: [None; 6],
            show_help: false,
            undo_stack: Vec::new(),
        };
        app.reset_pegs();
        app
    }

    fn reset_pegs(&mut self) {
        self.pegs = [Vec::new(), Vec::new(), Vec::new()];
        for i in (1..=self.num_disks as u8).rev() {
            self.pegs[0].push(i);
        }
        self.moves = 0;
        self.held_disk = None;
        self.state = GameState::Playing;
        self.undo_stack.clear();
    }

    fn set_disks(&mut self, n: usize) {
        if n >= 3 && n <= MAX_DISKS {
            self.num_disks = n;
            self.reset_pegs();
        }
    }

    fn min_moves(&self) -> u32 {
        (1u32 << self.num_disks).saturating_sub(1)
    }

    fn disk_index(&self) -> usize {
        self.num_disks.saturating_sub(3)
    }

    fn can_place(&self, peg: usize, disk: u8) -> bool {
        if peg >= NUM_PEGS {
            return false;
        }
        match self.pegs[peg].last() {
            Some(&top) => disk < top,
            None => true,
        }
    }

    fn try_pickup(&mut self) {
        if self.state != GameState::Playing || self.held_disk.is_some() {
            return;
        }
        let peg = self.selected_peg;
        if let Some(&disk) = self.pegs[peg].last() {
            self.held_disk = Some((peg, disk));
        }
    }

    fn try_place(&mut self) {
        if self.state != GameState::Playing {
            return;
        }
        let Some((from_peg, disk)) = self.held_disk else {
            return;
        };
        let to_peg = self.selected_peg;

        if to_peg == from_peg {
            // Put it back
            self.held_disk = None;
            return;
        }

        if self.can_place(to_peg, disk) {
            // Remove from source
            self.pegs[from_peg].pop();
            // Place on target
            self.pegs[to_peg].push(disk);
            self.held_disk = None;
            self.moves = self.moves.saturating_add(1);
            self.undo_stack.push((from_peg, to_peg));
            self.check_win();
        }
        // If can't place, stay held
    }

    fn cancel_held(&mut self) {
        self.held_disk = None;
    }

    fn undo(&mut self) {
        if self.state != GameState::Playing || self.held_disk.is_some() {
            return;
        }
        if let Some((from_peg, to_peg)) = self.undo_stack.pop() {
            if let Some(disk) = self.pegs[to_peg].pop() {
                self.pegs[from_peg].push(disk);
                self.moves = self.moves.saturating_sub(1);
            }
        }
    }

    fn check_win(&mut self) {
        // Win if all disks are on peg 2 (rightmost)
        if self.pegs[2].len() == self.num_disks {
            self.state = GameState::Won;
            let idx = self.disk_index();
            if idx < 6 {
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
            Event::Key(KeyEvent { key, modifiers, .. }) => {
                if *modifiers == Modifiers::NONE {
                    match key {
                        Key::Left => {
                            if self.selected_peg > 0 {
                                self.selected_peg -= 1;
                            }
                        }
                        Key::Right => {
                            if self.selected_peg < NUM_PEGS - 1 {
                                self.selected_peg += 1;
                            }
                        }
                        Key::Num1 => self.selected_peg = 0,
                        Key::Num2 => self.selected_peg = 1,
                        Key::Num3 => self.selected_peg = 2,
                        Key::Enter | Key::Space => {
                            if self.held_disk.is_some() {
                                self.try_place();
                            } else {
                                self.try_pickup();
                            }
                        }
                        Key::Escape => self.cancel_held(),
                        Key::Z => self.undo(),
                        Key::N => self.reset_pegs(),
                        Key::H => self.show_help = !self.show_help,
                        Key::Up => {
                            if self.num_disks < MAX_DISKS && self.state != GameState::Playing
                                || (self.moves == 0 && self.held_disk.is_none())
                            {
                                self.set_disks(self.num_disks + 1);
                            }
                        }
                        Key::Down => {
                            if self.num_disks > 3 && self.state != GameState::Playing
                                || (self.moves == 0 && self.held_disk.is_none())
                            {
                                self.set_disks(self.num_disks - 1);
                            }
                        }
                        _ => {}
                    }
                }
            }
            Event::Mouse(MouseEvent { x, kind, .. }) => {
                if matches!(kind, MouseEventKind::Press(MouseButton::Left)) {
                    // Determine which peg was clicked
                    let peg_width = 200.0_f32;
                    let start_x = 60.0_f32;
                    let total = peg_width * 3.0;
                    let mx = *x;
                    if mx >= start_x && mx <= start_x + total {
                        let peg = ((mx - start_x) / peg_width) as usize;
                        if peg < NUM_PEGS {
                            self.selected_peg = peg;
                            if self.held_disk.is_some() {
                                self.try_place();
                            } else {
                                self.try_pickup();
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn render(&self, width: f32, height: f32) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width,
            height,
            color: BASE,
            corner_radii: CornerRadii::ZERO,
        });

        // Title
        cmds.push(RenderCommand::Text {
            x: 50.0,
            y: 28.0,
            text: "Tower of Hanoi".into(),
            color: LAVENDER,
            font_size: 24.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Disk count and moves
        let info = format!(
            "Disks: {}   Moves: {} / {} (min)",
            self.num_disks, self.moves, self.min_moves()
        );
        cmds.push(RenderCommand::Text {
            x: 50.0,
            y: 56.0,
            text: info,
            color: SUBTEXT0,
            font_size: 14.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Pegs area
        let peg_area_y = 90.0_f32;
        let peg_area_h = 300.0_f32;
        let peg_width = 200.0_f32;
        let peg_start_x = 60.0_f32;
        let peg_height = 200.0_f32;
        let base_y = peg_area_y + peg_area_h - 20.0;

        for p in 0..NUM_PEGS {
            let cx = peg_start_x + p as f32 * peg_width + peg_width / 2.0;

            // Peg label
            let label = format!("{}", p + 1);
            let is_selected = p == self.selected_peg;
            let label_color = if is_selected { YELLOW } else { SUBTEXT0 };
            cmds.push(RenderCommand::Text {
                x: cx - 5.0,
                y: peg_area_y,
                text: label,
                color: label_color,
                font_size: 16.0,
                font_weight: if is_selected {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: None,
            });

            // Peg rod
            let rod_w = 6.0_f32;
            let rod_h = peg_height;
            cmds.push(RenderCommand::FillRect {
                x: cx - rod_w / 2.0,
                y: base_y - rod_h,
                width: rod_w,
                height: rod_h,
                color: SURFACE1,
                corner_radii: CornerRadii::all(2.0),
            });

            // Base platform
            cmds.push(RenderCommand::FillRect {
                x: cx - peg_width / 2.0 + 10.0,
                y: base_y,
                width: peg_width - 20.0,
                height: 8.0,
                color: SURFACE1,
                corner_radii: CornerRadii::all(3.0),
            });

            // Selection indicator
            if is_selected {
                cmds.push(RenderCommand::FillRect {
                    x: cx - peg_width / 2.0 + 10.0,
                    y: base_y + 12.0,
                    width: peg_width - 20.0,
                    height: 3.0,
                    color: YELLOW,
                    corner_radii: CornerRadii::all(1.0),
                });
            }

            // Disks on this peg
            let disk_h = 20.0_f32;
            let max_disk_w = peg_width - 30.0;
            let min_disk_w = 30.0_f32;

            for (i, &disk) in self.pegs[p].iter().enumerate() {
                let frac = if self.num_disks <= 1 {
                    1.0
                } else {
                    (disk as f32 - 1.0) / (self.num_disks as f32 - 1.0)
                };
                let dw = min_disk_w + frac * (max_disk_w - min_disk_w);
                let dy = base_y - (i as f32 + 1.0) * disk_h;

                let color_idx = (disk as usize).saturating_sub(1) % DISK_COLORS.len();
                let color = DISK_COLORS[color_idx];

                cmds.push(RenderCommand::FillRect {
                    x: cx - dw / 2.0,
                    y: dy,
                    width: dw,
                    height: disk_h - 2.0,
                    color,
                    corner_radii: CornerRadii::all(4.0),
                });

                // Disk number
                let num_str = format!("{}", disk);
                cmds.push(RenderCommand::Text {
                    x: cx - 5.0,
                    y: dy + 3.0,
                    text: num_str,
                    color: CRUST,
                    font_size: 12.0,
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                });
            }
        }

        // Held disk indicator
        if let Some((from_peg, disk)) = self.held_disk {
            let target_cx = peg_start_x + self.selected_peg as f32 * peg_width + peg_width / 2.0;
            let frac = if self.num_disks <= 1 {
                1.0
            } else {
                (disk as f32 - 1.0) / (self.num_disks as f32 - 1.0)
            };
            let max_disk_w = peg_width - 30.0;
            let min_disk_w = 30.0_f32;
            let dw = min_disk_w + frac * (max_disk_w - min_disk_w);
            let held_y = peg_area_y + 10.0;

            let color_idx = (disk as usize).saturating_sub(1) % DISK_COLORS.len();
            let color = DISK_COLORS[color_idx];

            cmds.push(RenderCommand::FillRect {
                x: target_cx - dw / 2.0,
                y: held_y,
                width: dw,
                height: 18.0,
                color,
                corner_radii: CornerRadii::all(4.0),
            });

            let _ = from_peg; // Used in held_disk tuple
            cmds.push(RenderCommand::Text {
                x: target_cx - 5.0,
                y: held_y + 2.0,
                text: format!("{}", disk),
                color: CRUST,
                font_size: 12.0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
        }

        // Win message
        if self.state == GameState::Won {
            let win_y = base_y + 40.0;
            let msg = if self.moves == self.min_moves() {
                format!("Perfect! Solved in {} moves (minimum)!", self.moves)
            } else {
                format!(
                    "Solved in {} moves! (minimum was {})",
                    self.moves,
                    self.min_moves()
                )
            };
            cmds.push(RenderCommand::Text {
                x: 50.0,
                y: win_y,
                text: msg,
                color: GREEN,
                font_size: 18.0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
            cmds.push(RenderCommand::Text {
                x: 50.0,
                y: win_y + 26.0,
                text: "Press N for new game, Up/Down to change disk count".into(),
                color: SUBTEXT0,
                font_size: 13.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }

        // Best scores panel
        let panel_x = peg_start_x + 3.0 * peg_width + 30.0;
        let panel_y = peg_area_y;

        cmds.push(RenderCommand::FillRect {
            x: panel_x,
            y: panel_y,
            width: 160.0,
            height: 220.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(8.0),
        });

        cmds.push(RenderCommand::Text {
            x: panel_x + 12.0,
            y: panel_y + 16.0,
            text: "Best Scores".into(),
            color: YELLOW,
            font_size: 15.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        for i in 0..6 {
            let disks = i + 3;
            let sy = panel_y + 44.0 + i as f32 * 26.0;
            let min = (1u32 << disks) - 1;
            let score_str = match self.best_moves[i] {
                Some(m) => {
                    if m == min {
                        format!("{} disks: {} (perfect)", disks, m)
                    } else {
                        format!("{} disks: {}", disks, m)
                    }
                }
                None => format!("{} disks: ---", disks),
            };
            let highlight = disks == self.num_disks;
            cmds.push(RenderCommand::Text {
                x: panel_x + 12.0,
                y: sy,
                text: score_str,
                color: if highlight { TEXT } else { SUBTEXT0 },
                font_size: 12.0,
                font_weight: if highlight {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: None,
            });
        }

        // Help panel
        if self.show_help {
            let help_x = panel_x;
            let help_y = panel_y + 240.0;

            cmds.push(RenderCommand::FillRect {
                x: help_x,
                y: help_y,
                width: 160.0,
                height: 200.0,
                color: SURFACE0,
                corner_radii: CornerRadii::all(8.0),
            });

            cmds.push(RenderCommand::Text {
                x: help_x + 12.0,
                y: help_y + 16.0,
                text: "Controls".into(),
                color: YELLOW,
                font_size: 15.0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });

            let controls = [
                ("Left/Right", "Select peg"),
                ("1/2/3", "Jump to peg"),
                ("Enter", "Pick/Place"),
                ("Esc", "Cancel hold"),
                ("Z", "Undo"),
                ("Up/Down", "Change disks"),
                ("N", "New game"),
            ];
            for (i, (k, v)) in controls.iter().enumerate() {
                let ly = help_y + 42.0 + i as f32 * 22.0;
                cmds.push(RenderCommand::Text {
                    x: help_x + 12.0,
                    y: ly,
                    text: (*k).into(),
                    color: BLUE,
                    font_size: 11.0,
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                });
                cmds.push(RenderCommand::Text {
                    x: help_x + 85.0,
                    y: ly,
                    text: (*v).into(),
                    color: SUBTEXT0,
                    font_size: 11.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                });
            }
        } else {
            cmds.push(RenderCommand::Text {
                x: panel_x + 12.0,
                y: panel_y + 240.0,
                text: "Press H for help".into(),
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
    let _app = TowersOfHanoi::new();
}

// ── Tests ──
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initial_state() {
        let app = TowersOfHanoi::new();
        assert_eq!(app.num_disks, 4);
        assert_eq!(app.moves, 0);
        assert_eq!(app.selected_peg, 0);
        assert!(app.held_disk.is_none());
        assert_eq!(app.state, GameState::Playing);
    }

    #[test]
    fn test_initial_pegs() {
        let app = TowersOfHanoi::new();
        assert_eq!(app.pegs[0], vec![4, 3, 2, 1]);
        assert!(app.pegs[1].is_empty());
        assert!(app.pegs[2].is_empty());
    }

    #[test]
    fn test_min_moves_3() {
        let mut app = TowersOfHanoi::new();
        app.num_disks = 3;
        assert_eq!(app.min_moves(), 7);
    }

    #[test]
    fn test_min_moves_4() {
        let app = TowersOfHanoi::new();
        assert_eq!(app.min_moves(), 15);
    }

    #[test]
    fn test_min_moves_5() {
        let mut app = TowersOfHanoi::new();
        app.num_disks = 5;
        assert_eq!(app.min_moves(), 31);
    }

    #[test]
    fn test_min_moves_8() {
        let mut app = TowersOfHanoi::new();
        app.num_disks = 8;
        assert_eq!(app.min_moves(), 255);
    }

    #[test]
    fn test_set_disks() {
        let mut app = TowersOfHanoi::new();
        app.set_disks(5);
        assert_eq!(app.num_disks, 5);
        assert_eq!(app.pegs[0], vec![5, 4, 3, 2, 1]);
    }

    #[test]
    fn test_set_disks_min() {
        let mut app = TowersOfHanoi::new();
        app.set_disks(3);
        assert_eq!(app.num_disks, 3);
        assert_eq!(app.pegs[0].len(), 3);
    }

    #[test]
    fn test_set_disks_max() {
        let mut app = TowersOfHanoi::new();
        app.set_disks(8);
        assert_eq!(app.num_disks, 8);
        assert_eq!(app.pegs[0].len(), 8);
    }

    #[test]
    fn test_set_disks_invalid_low() {
        let mut app = TowersOfHanoi::new();
        app.set_disks(2);
        assert_eq!(app.num_disks, 4); // unchanged
    }

    #[test]
    fn test_set_disks_invalid_high() {
        let mut app = TowersOfHanoi::new();
        app.set_disks(9);
        assert_eq!(app.num_disks, 4); // unchanged
    }

    #[test]
    fn test_can_place_empty_peg() {
        let app = TowersOfHanoi::new();
        assert!(app.can_place(1, 4));
        assert!(app.can_place(2, 1));
    }

    #[test]
    fn test_can_place_smaller_on_larger() {
        let app = TowersOfHanoi::new();
        // Peg 0 has [4,3,2,1], top is 1. Can't place 2 on 1.
        assert!(!app.can_place(0, 2));
    }

    #[test]
    fn test_can_place_same_size() {
        let app = TowersOfHanoi::new();
        assert!(!app.can_place(0, 1));
    }

    #[test]
    fn test_can_place_out_of_bounds() {
        let app = TowersOfHanoi::new();
        assert!(!app.can_place(5, 1));
    }

    #[test]
    fn test_pickup() {
        let mut app = TowersOfHanoi::new();
        app.selected_peg = 0;
        app.try_pickup();
        assert_eq!(app.held_disk, Some((0, 1)));
    }

    #[test]
    fn test_pickup_empty_peg() {
        let mut app = TowersOfHanoi::new();
        app.selected_peg = 1;
        app.try_pickup();
        assert!(app.held_disk.is_none());
    }

    #[test]
    fn test_pickup_when_already_holding() {
        let mut app = TowersOfHanoi::new();
        app.held_disk = Some((0, 1));
        app.selected_peg = 1;
        app.try_pickup();
        assert_eq!(app.held_disk, Some((0, 1))); // unchanged
    }

    #[test]
    fn test_pickup_when_won() {
        let mut app = TowersOfHanoi::new();
        app.state = GameState::Won;
        app.try_pickup();
        assert!(app.held_disk.is_none());
    }

    #[test]
    fn test_place_on_empty_peg() {
        let mut app = TowersOfHanoi::new();
        // Pick up disk 1 from peg 0
        app.selected_peg = 0;
        app.try_pickup();
        // Place on peg 1
        app.selected_peg = 1;
        app.try_place();
        assert!(app.held_disk.is_none());
        assert_eq!(app.pegs[1], vec![1]);
        assert_eq!(app.pegs[0], vec![4, 3, 2]);
        assert_eq!(app.moves, 1);
    }

    #[test]
    fn test_place_on_larger_disk() {
        let mut app = TowersOfHanoi::new();
        // Move disk 1 to peg 1
        app.selected_peg = 0;
        app.try_pickup();
        app.selected_peg = 1;
        app.try_place();
        // Move disk 2 to peg 2
        app.selected_peg = 0;
        app.try_pickup();
        app.selected_peg = 2;
        app.try_place();
        // Move disk 1 on top of disk 2
        app.selected_peg = 1;
        app.try_pickup();
        app.selected_peg = 2;
        app.try_place();
        assert_eq!(app.pegs[2], vec![2, 1]);
        assert_eq!(app.moves, 3);
    }

    #[test]
    fn test_place_larger_on_smaller_rejected() {
        let mut app = TowersOfHanoi::new();
        // Move disk 1 to peg 1
        app.selected_peg = 0;
        app.try_pickup();
        app.selected_peg = 1;
        app.try_place();
        // Try to move disk 2 onto disk 1
        app.selected_peg = 0;
        app.try_pickup();
        assert_eq!(app.held_disk, Some((0, 2)));
        app.selected_peg = 1;
        app.try_place();
        // Should still be held
        assert_eq!(app.held_disk, Some((0, 2)));
        assert_eq!(app.moves, 1); // only 1 successful move
    }

    #[test]
    fn test_place_on_same_peg() {
        let mut app = TowersOfHanoi::new();
        app.selected_peg = 0;
        app.try_pickup();
        app.selected_peg = 0; // same peg
        app.try_place();
        assert!(app.held_disk.is_none());
        assert_eq!(app.moves, 0); // not counted as a move
    }

    #[test]
    fn test_cancel_held() {
        let mut app = TowersOfHanoi::new();
        app.selected_peg = 0;
        app.try_pickup();
        assert!(app.held_disk.is_some());
        app.cancel_held();
        assert!(app.held_disk.is_none());
    }

    #[test]
    fn test_undo_single_move() {
        let mut app = TowersOfHanoi::new();
        app.selected_peg = 0;
        app.try_pickup();
        app.selected_peg = 1;
        app.try_place();
        assert_eq!(app.moves, 1);
        app.undo();
        assert_eq!(app.moves, 0);
        assert_eq!(app.pegs[0], vec![4, 3, 2, 1]);
        assert!(app.pegs[1].is_empty());
    }

    #[test]
    fn test_undo_multiple() {
        let mut app = TowersOfHanoi::new();
        // Move disk 1 to peg 1
        app.selected_peg = 0;
        app.try_pickup();
        app.selected_peg = 1;
        app.try_place();
        // Move disk 2 to peg 2
        app.selected_peg = 0;
        app.try_pickup();
        app.selected_peg = 2;
        app.try_place();
        assert_eq!(app.moves, 2);
        app.undo();
        assert_eq!(app.moves, 1);
        assert_eq!(app.pegs[0], vec![4, 3, 2]);
        app.undo();
        assert_eq!(app.moves, 0);
        assert_eq!(app.pegs[0], vec![4, 3, 2, 1]);
    }

    #[test]
    fn test_undo_empty_stack() {
        let mut app = TowersOfHanoi::new();
        app.undo(); // should do nothing
        assert_eq!(app.moves, 0);
    }

    #[test]
    fn test_undo_while_holding() {
        let mut app = TowersOfHanoi::new();
        app.selected_peg = 0;
        app.try_pickup();
        app.selected_peg = 1;
        app.try_place();
        // Pick up again
        app.selected_peg = 1;
        app.try_pickup();
        // Try undo while holding — should not work
        app.undo();
        assert_eq!(app.moves, 1); // unchanged
    }

    #[test]
    fn test_win_detection() {
        let mut app = TowersOfHanoi::new();
        app.num_disks = 3;
        app.reset_pegs();
        // Manually solve 3-disk puzzle (7 moves)
        let moves = [(0, 2), (0, 1), (2, 1), (0, 2), (1, 0), (1, 2), (0, 2)];
        for (from, to) in moves {
            app.selected_peg = from;
            app.try_pickup();
            app.selected_peg = to;
            app.try_place();
        }
        assert_eq!(app.state, GameState::Won);
        assert_eq!(app.moves, 7);
    }

    #[test]
    fn test_win_records_best() {
        let mut app = TowersOfHanoi::new();
        app.num_disks = 3;
        app.reset_pegs();
        // Solve with 7 moves (minimum)
        let moves = [(0, 2), (0, 1), (2, 1), (0, 2), (1, 0), (1, 2), (0, 2)];
        for (from, to) in moves {
            app.selected_peg = from;
            app.try_pickup();
            app.selected_peg = to;
            app.try_place();
        }
        assert_eq!(app.best_moves[0], Some(7)); // index 0 = 3 disks
    }

    #[test]
    fn test_best_score_improves() {
        let mut app = TowersOfHanoi::new();
        app.best_moves[0] = Some(10); // 3 disks, previous best
        app.num_disks = 3;
        app.reset_pegs();
        // Solve optimally
        let moves = [(0, 2), (0, 1), (2, 1), (0, 2), (1, 0), (1, 2), (0, 2)];
        for (from, to) in moves {
            app.selected_peg = from;
            app.try_pickup();
            app.selected_peg = to;
            app.try_place();
        }
        assert_eq!(app.best_moves[0], Some(7)); // improved
    }

    #[test]
    fn test_best_score_no_worsen() {
        let mut app = TowersOfHanoi::new();
        app.best_moves[0] = Some(7);
        // Simulate a win with more moves
        app.num_disks = 3;
        app.pegs = [Vec::new(), Vec::new(), vec![3, 2, 1]];
        app.moves = 15;
        app.check_win();
        assert_eq!(app.best_moves[0], Some(7)); // unchanged
    }

    #[test]
    fn test_disk_index() {
        let mut app = TowersOfHanoi::new();
        app.num_disks = 3;
        assert_eq!(app.disk_index(), 0);
        app.num_disks = 8;
        assert_eq!(app.disk_index(), 5);
    }

    #[test]
    fn test_reset_pegs() {
        let mut app = TowersOfHanoi::new();
        // Make some moves
        app.selected_peg = 0;
        app.try_pickup();
        app.selected_peg = 1;
        app.try_place();
        app.reset_pegs();
        assert_eq!(app.moves, 0);
        assert!(app.held_disk.is_none());
        assert_eq!(app.pegs[0], vec![4, 3, 2, 1]);
        assert!(app.pegs[1].is_empty());
        assert!(app.pegs[2].is_empty());
        assert_eq!(app.state, GameState::Playing);
    }

    // ── Key events ──

    #[test]
    fn test_key_left() {
        let mut app = TowersOfHanoi::new();
        app.selected_peg = 1;
        let evt = Event::Key(KeyEvent {
            key: Key::Left,
            modifiers: Modifiers::NONE,
            pressed: true,
            text: None,
        });
        app.event(&evt);
        assert_eq!(app.selected_peg, 0);
    }

    #[test]
    fn test_key_left_at_min() {
        let mut app = TowersOfHanoi::new();
        app.selected_peg = 0;
        let evt = Event::Key(KeyEvent {
            key: Key::Left,
            modifiers: Modifiers::NONE,
            pressed: true,
            text: None,
        });
        app.event(&evt);
        assert_eq!(app.selected_peg, 0);
    }

    #[test]
    fn test_key_right() {
        let mut app = TowersOfHanoi::new();
        app.selected_peg = 0;
        let evt = Event::Key(KeyEvent {
            key: Key::Right,
            modifiers: Modifiers::NONE,
            pressed: true,
            text: None,
        });
        app.event(&evt);
        assert_eq!(app.selected_peg, 1);
    }

    #[test]
    fn test_key_right_at_max() {
        let mut app = TowersOfHanoi::new();
        app.selected_peg = 2;
        let evt = Event::Key(KeyEvent {
            key: Key::Right,
            modifiers: Modifiers::NONE,
            pressed: true,
            text: None,
        });
        app.event(&evt);
        assert_eq!(app.selected_peg, 2);
    }

    #[test]
    fn test_key_number_jump() {
        let mut app = TowersOfHanoi::new();
        let evt = Event::Key(KeyEvent {
            key: Key::Num2,
            modifiers: Modifiers::NONE,
            pressed: true,
            text: None,
        });
        app.event(&evt);
        assert_eq!(app.selected_peg, 1);
    }

    #[test]
    fn test_key_enter_pickup_place() {
        let mut app = TowersOfHanoi::new();
        // Enter on peg 0 = pickup
        app.selected_peg = 0;
        let evt = Event::Key(KeyEvent {
            key: Key::Enter,
            modifiers: Modifiers::NONE,
            pressed: true,
            text: None,
        });
        app.event(&evt);
        assert!(app.held_disk.is_some());
        // Move to peg 1 and enter = place
        app.selected_peg = 1;
        app.event(&evt);
        assert!(app.held_disk.is_none());
        assert_eq!(app.moves, 1);
    }

    #[test]
    fn test_key_n_new_game() {
        let mut app = TowersOfHanoi::new();
        app.moves = 10;
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
    fn test_key_z_undo() {
        let mut app = TowersOfHanoi::new();
        app.selected_peg = 0;
        app.try_pickup();
        app.selected_peg = 1;
        app.try_place();
        let evt = Event::Key(KeyEvent {
            key: Key::Z,
            modifiers: Modifiers::NONE,
            pressed: true,
            text: None,
        });
        app.event(&evt);
        assert_eq!(app.moves, 0);
    }

    #[test]
    fn test_key_escape_cancel() {
        let mut app = TowersOfHanoi::new();
        app.held_disk = Some((0, 1));
        let evt = Event::Key(KeyEvent {
            key: Key::Escape,
            modifiers: Modifiers::NONE,
            pressed: true,
            text: None,
        });
        app.event(&evt);
        assert!(app.held_disk.is_none());
    }

    #[test]
    fn test_key_h_toggle_help() {
        let mut app = TowersOfHanoi::new();
        assert!(!app.show_help);
        let evt = Event::Key(KeyEvent {
            key: Key::H,
            modifiers: Modifiers::NONE,
            pressed: true,
            text: None,
        });
        app.event(&evt);
        assert!(app.show_help);
        app.event(&evt);
        assert!(!app.show_help);
    }

    // ── Render tests ──

    #[test]
    fn test_render_returns_commands() {
        let app = TowersOfHanoi::new();
        let cmds = app.render(800.0, 600.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_with_held_disk() {
        let mut app = TowersOfHanoi::new();
        app.held_disk = Some((0, 2));
        let cmds = app.render(800.0, 600.0);
        assert!(cmds.len() > 10);
    }

    #[test]
    fn test_render_won_state() {
        let mut app = TowersOfHanoi::new();
        app.state = GameState::Won;
        app.moves = 15;
        let cmds = app.render(800.0, 600.0);
        assert!(cmds.len() > 10);
    }

    #[test]
    fn test_render_with_help() {
        let mut app = TowersOfHanoi::new();
        app.show_help = true;
        let cmds = app.render(800.0, 600.0);
        assert!(cmds.len() > 20);
    }

    #[test]
    fn test_render_3_disks() {
        let mut app = TowersOfHanoi::new();
        app.set_disks(3);
        let cmds = app.render(800.0, 600.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_8_disks() {
        let mut app = TowersOfHanoi::new();
        app.set_disks(8);
        let cmds = app.render(800.0, 600.0);
        assert!(!cmds.is_empty());
    }

    // ── Game state ──

    #[test]
    fn test_game_state_eq() {
        assert_eq!(GameState::Playing, GameState::Playing);
        assert_eq!(GameState::Won, GameState::Won);
        assert_ne!(GameState::Playing, GameState::Won);
    }

    // ── 3-disk full solve ──

    #[test]
    fn test_3disk_optimal_solve() {
        let mut app = TowersOfHanoi::new();
        app.set_disks(3);
        // Standard 3-disk solution: A→C, A→B, C→B, A→C, B→A, B→C, A→C
        let moves = [(0, 2), (0, 1), (2, 1), (0, 2), (1, 0), (1, 2), (0, 2)];
        for (from, to) in moves {
            app.selected_peg = from;
            app.try_pickup();
            app.selected_peg = to;
            app.try_place();
        }
        assert_eq!(app.state, GameState::Won);
        assert_eq!(app.moves, 7);
        assert_eq!(app.pegs[2], vec![3, 2, 1]);
    }

    // ── Edge cases ──

    #[test]
    fn test_move_when_won() {
        let mut app = TowersOfHanoi::new();
        app.state = GameState::Won;
        app.selected_peg = 0;
        app.try_pickup();
        assert!(app.held_disk.is_none());
    }

    #[test]
    fn test_place_when_not_holding() {
        let mut app = TowersOfHanoi::new();
        app.selected_peg = 1;
        app.try_place(); // nothing held
        assert_eq!(app.moves, 0);
    }

    #[test]
    fn test_undo_clears_stack() {
        let mut app = TowersOfHanoi::new();
        app.selected_peg = 0;
        app.try_pickup();
        app.selected_peg = 1;
        app.try_place();
        app.undo();
        assert!(app.undo_stack.is_empty());
    }

    // ── Multiple complete games ──

    #[test]
    fn test_new_game_after_win() {
        let mut app = TowersOfHanoi::new();
        app.state = GameState::Won;
        app.moves = 50;
        app.reset_pegs();
        assert_eq!(app.state, GameState::Playing);
        assert_eq!(app.moves, 0);
    }

    #[test]
    fn test_change_disks_resets() {
        let mut app = TowersOfHanoi::new();
        app.selected_peg = 0;
        app.try_pickup();
        app.selected_peg = 1;
        app.try_place();
        app.set_disks(5);
        assert_eq!(app.moves, 0);
        assert_eq!(app.pegs[0].len(), 5);
    }
}
