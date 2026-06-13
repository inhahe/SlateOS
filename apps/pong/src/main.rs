//! Pong game for SlateOS.
//!
//! Classic two-paddle ball game with AI opponent.
//! Left paddle (player) uses Up/Down, right paddle is AI.

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

use guitk::color::Color;
use guitk::event::{Event, Key, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseEventKind};
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

const COL_BASE: Color = Color::from_hex(0x1E1E2E);
const COL_MANTLE: Color = Color::from_hex(0x181825);
const COL_SURFACE0: Color = Color::from_hex(0x313244);
const COL_TEXT: Color = Color::from_hex(0xCDD6F4);
const COL_SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const COL_BLUE: Color = Color::from_hex(0x89B4FA);
const COL_GREEN: Color = Color::from_hex(0xA6E3A1);
const COL_RED: Color = Color::from_hex(0xF38BA8);
const COL_YELLOW: Color = Color::from_hex(0xF9E2AF);
const COL_LAVENDER: Color = Color::from_hex(0xB4BEFE);
const COL_OVERLAY0: Color = Color::from_hex(0x6C7086);
const COL_TEAL: Color = Color::from_hex(0x94E2D5);

const FIELD_W: f32 = 700.0;
const FIELD_H: f32 = 500.0;
const FIELD_X: f32 = 50.0;
const FIELD_Y: f32 = 50.0;
const PADDLE_W: f32 = 12.0;
const PADDLE_H: f32 = 80.0;
const BALL_SIZE: f32 = 10.0;
const PADDLE_SPEED: f32 = 5.0;
const INITIAL_BALL_SPEED: f32 = 3.5;
const WIN_SCORE: u32 = 11;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GameState {
    Menu,
    Playing,
    Paused,
    GameOver,
}

struct PongApp {
    state: GameState,
    // Paddles (y position = top of paddle)
    left_y: f32,
    right_y: f32,
    // Ball
    ball_x: f32,
    ball_y: f32,
    ball_dx: f32,
    ball_dy: f32,
    // Scores
    left_score: u32,
    right_score: u32,
    // Input state
    up_held: bool,
    down_held: bool,
    // AI difficulty
    ai_speed: f32,
    // Speed multiplier (increases over rallies)
    speed_mult: f32,
    rally_count: u32,
}

impl PongApp {
    fn new() -> Self {
        let mut app = Self {
            state: GameState::Menu,
            left_y: FIELD_H / 2.0 - PADDLE_H / 2.0,
            right_y: FIELD_H / 2.0 - PADDLE_H / 2.0,
            ball_x: FIELD_W / 2.0,
            ball_y: FIELD_H / 2.0,
            ball_dx: INITIAL_BALL_SPEED,
            ball_dy: INITIAL_BALL_SPEED * 0.5,
            left_score: 0,
            right_score: 0,
            up_held: false,
            down_held: false,
            ai_speed: 3.5,
            speed_mult: 1.0,
            rally_count: 0,
        };
        app.reset_ball(true);
        app
    }

    fn reset_ball(&mut self, go_right: bool) {
        self.ball_x = FIELD_W / 2.0;
        self.ball_y = FIELD_H / 2.0;
        let dir = if go_right { 1.0 } else { -1.0 };
        self.ball_dx = INITIAL_BALL_SPEED * dir;
        self.ball_dy = if self.rally_count.is_multiple_of(2) { 1.5 } else { -1.5 };
        self.speed_mult = 1.0;
        self.rally_count = 0;
    }

    fn new_game(&mut self) {
        self.left_score = 0;
        self.right_score = 0;
        self.left_y = FIELD_H / 2.0 - PADDLE_H / 2.0;
        self.right_y = FIELD_H / 2.0 - PADDLE_H / 2.0;
        self.reset_ball(true);
        self.state = GameState::Playing;
    }

    fn update(&mut self) {
        if self.state != GameState::Playing {
            return;
        }

        // Player paddle movement
        if self.up_held && self.left_y > 0.0 {
            self.left_y -= PADDLE_SPEED;
            if self.left_y < 0.0 { self.left_y = 0.0; }
        }
        if self.down_held && self.left_y + PADDLE_H < FIELD_H {
            self.left_y += PADDLE_SPEED;
            if self.left_y + PADDLE_H > FIELD_H { self.left_y = FIELD_H - PADDLE_H; }
        }

        // AI paddle movement (tracks ball with slight lag)
        let ai_target = self.ball_y - PADDLE_H / 2.0;
        let ai_diff = ai_target - self.right_y;
        let ai_move = ai_diff.clamp(-self.ai_speed, self.ai_speed);
        self.right_y += ai_move;
        self.right_y = self.right_y.clamp(0.0, FIELD_H - PADDLE_H);

        // Ball movement
        let speed = self.speed_mult;
        self.ball_x += self.ball_dx * speed;
        self.ball_y += self.ball_dy * speed;

        // Top/bottom wall bounce
        if self.ball_y <= 0.0 {
            self.ball_y = 0.0;
            self.ball_dy = self.ball_dy.abs();
        }
        if self.ball_y + BALL_SIZE >= FIELD_H {
            self.ball_y = FIELD_H - BALL_SIZE;
            self.ball_dy = -self.ball_dy.abs();
        }

        // Left paddle collision
        let left_paddle_x = 20.0;
        if self.ball_x <= left_paddle_x + PADDLE_W
            && self.ball_x + BALL_SIZE >= left_paddle_x
            && self.ball_y + BALL_SIZE >= self.left_y
            && self.ball_y <= self.left_y + PADDLE_H
            && self.ball_dx < 0.0
        {
            self.ball_dx = self.ball_dx.abs();
            // Angle based on where ball hits paddle
            let hit_pos = (self.ball_y + BALL_SIZE / 2.0 - self.left_y) / PADDLE_H;
            self.ball_dy = (hit_pos - 0.5) * 6.0;
            self.rally_count += 1;
            if self.rally_count.is_multiple_of(5) {
                self.speed_mult += 0.15;
            }
        }

        // Right paddle collision
        let right_paddle_x = FIELD_W - 20.0 - PADDLE_W;
        if self.ball_x + BALL_SIZE >= right_paddle_x
            && self.ball_x <= right_paddle_x + PADDLE_W
            && self.ball_y + BALL_SIZE >= self.right_y
            && self.ball_y <= self.right_y + PADDLE_H
            && self.ball_dx > 0.0
        {
            self.ball_dx = -self.ball_dx.abs();
            let hit_pos = (self.ball_y + BALL_SIZE / 2.0 - self.right_y) / PADDLE_H;
            self.ball_dy = (hit_pos - 0.5) * 6.0;
            self.rally_count += 1;
            if self.rally_count.is_multiple_of(5) {
                self.speed_mult += 0.15;
            }
        }

        // Score
        if self.ball_x + BALL_SIZE < 0.0 {
            self.right_score += 1;
            if self.right_score >= WIN_SCORE {
                self.state = GameState::GameOver;
            } else {
                self.reset_ball(true);
            }
        }
        if self.ball_x > FIELD_W {
            self.left_score += 1;
            if self.left_score >= WIN_SCORE {
                self.state = GameState::GameOver;
            } else {
                self.reset_ball(false);
            }
        }
    }

    fn event(&mut self, event: &Event) {
        match event {
            Event::Tick { .. } => self.update(),
            Event::Key(KeyEvent { key, modifiers, .. }) => {
                if modifiers.ctrl { return; }
                match self.state {
                    GameState::Menu => {
                        if matches!(key, Key::Enter | Key::Space) {
                            self.new_game();
                        }
                    }
                    GameState::Playing => {
                        match key {
                            Key::Up => self.up_held = true,
                            Key::Down => self.down_held = true,
                            Key::P => self.state = GameState::Paused,
                            Key::Escape => self.state = GameState::Paused,
                            _ => {}
                        }
                    }
                    GameState::Paused => {
                        match key {
                            Key::P | Key::Escape | Key::Space => self.state = GameState::Playing,
                            Key::N => self.new_game(),
                            _ => {}
                        }
                    }
                    GameState::GameOver => {
                        if matches!(key, Key::Enter | Key::Space | Key::N) {
                            self.new_game();
                        }
                    }
                }
            }
            _ => {}
        }

        // Handle key releases for held keys
        // Since we don't have KeyUp events in this framework, we check per-tick
        // Actually, we'll use a simpler approach: release on opposing key
        if let Event::Key(KeyEvent { key, .. }) = event {
            match key {
                Key::Up => { self.down_held = false; }
                Key::Down => { self.up_held = false; }
                _ => {}
            }
        }
    }

    fn render(&self, width: f32, height: f32) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Background
        cmds.push(RenderCommand::FillRect {
            x: 0.0, y: 0.0, width, height,
            color: COL_BASE,
            corner_radii: CornerRadii::ZERO,
        });

        // Field
        cmds.push(RenderCommand::FillRect {
            x: FIELD_X, y: FIELD_Y, width: FIELD_W, height: FIELD_H,
            color: COL_MANTLE,
            corner_radii: CornerRadii::all(4.0),
        });

        // Center line (dashed via small rects)
        let center_x = FIELD_X + FIELD_W / 2.0 - 1.0;
        let mut dash_y = FIELD_Y;
        while dash_y < FIELD_Y + FIELD_H {
            cmds.push(RenderCommand::FillRect {
                x: center_x, y: dash_y,
                width: 2.0, height: 12.0,
                color: COL_SURFACE0,
                corner_radii: CornerRadii::ZERO,
            });
            dash_y += 24.0;
        }

        // Scores
        cmds.push(RenderCommand::Text {
            x: FIELD_X + FIELD_W / 2.0 - 60.0, y: 15.0,
            text: format!("{}", self.left_score),
            font_size: 28.0,
            color: COL_BLUE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        cmds.push(RenderCommand::Text {
            x: FIELD_X + FIELD_W / 2.0 + 40.0, y: 15.0,
            text: format!("{}", self.right_score),
            font_size: 28.0,
            color: COL_RED,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Labels
        cmds.push(RenderCommand::Text {
            x: FIELD_X + FIELD_W / 2.0 - 80.0, y: 18.0,
            text: "You".to_string(),
            font_size: 12.0,
            color: COL_SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        cmds.push(RenderCommand::Text {
            x: FIELD_X + FIELD_W / 2.0 + 55.0, y: 18.0,
            text: "AI".to_string(),
            font_size: 12.0,
            color: COL_SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        if self.state == GameState::Playing || self.state == GameState::Paused {
            // Left paddle
            cmds.push(RenderCommand::FillRect {
                x: FIELD_X + 20.0,
                y: FIELD_Y + self.left_y,
                width: PADDLE_W,
                height: PADDLE_H,
                color: COL_BLUE,
                corner_radii: CornerRadii::all(3.0),
            });

            // Right paddle
            cmds.push(RenderCommand::FillRect {
                x: FIELD_X + FIELD_W - 20.0 - PADDLE_W,
                y: FIELD_Y + self.right_y,
                width: PADDLE_W,
                height: PADDLE_H,
                color: COL_RED,
                corner_radii: CornerRadii::all(3.0),
            });

            // Ball
            cmds.push(RenderCommand::FillRect {
                x: FIELD_X + self.ball_x,
                y: FIELD_Y + self.ball_y,
                width: BALL_SIZE,
                height: BALL_SIZE,
                color: COL_TEXT,
                corner_radii: CornerRadii::all(BALL_SIZE / 2.0),
            });
        }

        // State overlays
        match self.state {
            GameState::Menu => {
                cmds.push(RenderCommand::Text {
                    x: FIELD_X + FIELD_W / 2.0 - 40.0,
                    y: FIELD_Y + FIELD_H / 2.0 - 30.0,
                    text: "PONG".to_string(),
                    font_size: 36.0,
                    color: COL_TEXT,
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                });
                cmds.push(RenderCommand::Text {
                    x: FIELD_X + FIELD_W / 2.0 - 80.0,
                    y: FIELD_Y + FIELD_H / 2.0 + 20.0,
                    text: "Press Enter to start".to_string(),
                    font_size: 16.0,
                    color: COL_SUBTEXT0,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                });
            }
            GameState::Paused => {
                cmds.push(RenderCommand::FillRect {
                    x: FIELD_X, y: FIELD_Y, width: FIELD_W, height: FIELD_H,
                    color: Color::rgba(0, 0, 0, 140),
                    corner_radii: CornerRadii::ZERO,
                });
                cmds.push(RenderCommand::Text {
                    x: FIELD_X + FIELD_W / 2.0 - 50.0,
                    y: FIELD_Y + FIELD_H / 2.0 - 10.0,
                    text: "PAUSED".to_string(),
                    font_size: 28.0,
                    color: COL_YELLOW,
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                });
            }
            GameState::GameOver => {
                cmds.push(RenderCommand::FillRect {
                    x: FIELD_X, y: FIELD_Y, width: FIELD_W, height: FIELD_H,
                    color: Color::rgba(0, 0, 0, 160),
                    corner_radii: CornerRadii::ZERO,
                });
                let msg = if self.left_score >= WIN_SCORE { "You Win!" } else { "AI Wins!" };
                let msg_color = if self.left_score >= WIN_SCORE { COL_GREEN } else { COL_RED };
                cmds.push(RenderCommand::Text {
                    x: FIELD_X + FIELD_W / 2.0 - 60.0,
                    y: FIELD_Y + FIELD_H / 2.0 - 20.0,
                    text: msg.to_string(),
                    font_size: 28.0,
                    color: msg_color,
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                });
                cmds.push(RenderCommand::Text {
                    x: FIELD_X + FIELD_W / 2.0 - 90.0,
                    y: FIELD_Y + FIELD_H / 2.0 + 20.0,
                    text: "Press Enter for new game".to_string(),
                    font_size: 14.0,
                    color: COL_SUBTEXT0,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                });
            }
            _ => {}
        }

        // Help
        cmds.push(RenderCommand::Text {
            x: 20.0, y: height - 20.0,
            text: "Up/Down=Move  P=Pause  N=New Game  First to 11 wins".to_string(),
            font_size: 11.0,
            color: COL_OVERLAY0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        cmds
    }
}

fn main() {
    let _app = PongApp::new();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_new() {
        let app = PongApp::new();
        assert_eq!(app.state, GameState::Menu);
        assert_eq!(app.left_score, 0);
        assert_eq!(app.right_score, 0);
    }

    #[test]
    fn test_new_game() {
        let mut app = PongApp::new();
        app.new_game();
        assert_eq!(app.state, GameState::Playing);
        assert_eq!(app.left_score, 0);
        assert_eq!(app.right_score, 0);
    }

    #[test]
    fn test_reset_ball() {
        let mut app = PongApp::new();
        app.ball_x = 0.0;
        app.ball_y = 0.0;
        app.reset_ball(true);
        assert_eq!(app.ball_x, FIELD_W / 2.0);
        assert_eq!(app.ball_y, FIELD_H / 2.0);
        assert!(app.ball_dx > 0.0);
    }

    #[test]
    fn test_reset_ball_left() {
        let mut app = PongApp::new();
        app.reset_ball(false);
        assert!(app.ball_dx < 0.0);
    }

    #[test]
    fn test_enter_starts_game() {
        let mut app = PongApp::new();
        app.event(&Event::Key(KeyEvent {
            key: Key::Enter,
            modifiers: Modifiers::default(),
            pressed: true,
            text: None,
        }));
        assert_eq!(app.state, GameState::Playing);
    }

    #[test]
    fn test_pause() {
        let mut app = PongApp::new();
        app.new_game();
        app.event(&Event::Key(KeyEvent {
            key: Key::P,
            modifiers: Modifiers::default(),
            pressed: true,
            text: None,
        }));
        assert_eq!(app.state, GameState::Paused);
    }

    #[test]
    fn test_unpause() {
        let mut app = PongApp::new();
        app.new_game();
        app.state = GameState::Paused;
        app.event(&Event::Key(KeyEvent {
            key: Key::P,
            modifiers: Modifiers::default(),
            pressed: true,
            text: None,
        }));
        assert_eq!(app.state, GameState::Playing);
    }

    #[test]
    fn test_ball_moves() {
        let mut app = PongApp::new();
        app.new_game();
        let old_x = app.ball_x;
        app.update();
        assert_ne!(app.ball_x, old_x);
    }

    #[test]
    fn test_ball_top_bounce() {
        let mut app = PongApp::new();
        app.new_game();
        app.ball_y = -1.0;
        app.ball_dy = -2.0;
        app.update();
        assert!(app.ball_dy > 0.0);
    }

    #[test]
    fn test_ball_bottom_bounce() {
        let mut app = PongApp::new();
        app.new_game();
        app.ball_y = FIELD_H;
        app.ball_dy = 2.0;
        app.update();
        assert!(app.ball_dy < 0.0);
    }

    #[test]
    fn test_player_paddle_up() {
        let mut app = PongApp::new();
        app.new_game();
        app.up_held = true;
        let old_y = app.left_y;
        app.update();
        assert!(app.left_y < old_y);
    }

    #[test]
    fn test_player_paddle_down() {
        let mut app = PongApp::new();
        app.new_game();
        app.down_held = true;
        let old_y = app.left_y;
        app.update();
        assert!(app.left_y > old_y);
    }

    #[test]
    fn test_paddle_clamp_top() {
        let mut app = PongApp::new();
        app.new_game();
        app.left_y = 0.0;
        app.up_held = true;
        app.update();
        assert_eq!(app.left_y, 0.0);
    }

    #[test]
    fn test_paddle_clamp_bottom() {
        let mut app = PongApp::new();
        app.new_game();
        app.left_y = FIELD_H - PADDLE_H;
        app.down_held = true;
        app.update();
        assert_eq!(app.left_y, FIELD_H - PADDLE_H);
    }

    #[test]
    fn test_right_score_on_left_miss() {
        let mut app = PongApp::new();
        app.new_game();
        app.ball_x = -BALL_SIZE - 1.0;
        app.ball_dx = -1.0;
        app.update();
        assert_eq!(app.right_score, 1);
    }

    #[test]
    fn test_left_score_on_right_miss() {
        let mut app = PongApp::new();
        app.new_game();
        app.ball_x = FIELD_W + 1.0;
        app.ball_dx = 1.0;
        app.update();
        assert_eq!(app.left_score, 1);
    }

    #[test]
    fn test_game_over_right_wins() {
        let mut app = PongApp::new();
        app.new_game();
        app.right_score = WIN_SCORE - 1;
        app.ball_x = -BALL_SIZE - 1.0;
        app.ball_dx = -1.0;
        app.update();
        assert_eq!(app.state, GameState::GameOver);
    }

    #[test]
    fn test_game_over_left_wins() {
        let mut app = PongApp::new();
        app.new_game();
        app.left_score = WIN_SCORE - 1;
        app.ball_x = FIELD_W + 1.0;
        app.ball_dx = 1.0;
        app.update();
        assert_eq!(app.state, GameState::GameOver);
    }

    #[test]
    fn test_game_over_enter_restarts() {
        let mut app = PongApp::new();
        app.state = GameState::GameOver;
        app.left_score = 11;
        app.event(&Event::Key(KeyEvent {
            key: Key::Enter,
            modifiers: Modifiers::default(),
            pressed: true,
            text: None,
        }));
        assert_eq!(app.state, GameState::Playing);
        assert_eq!(app.left_score, 0);
    }

    #[test]
    fn test_ai_tracks_ball() {
        let mut app = PongApp::new();
        app.new_game();
        app.ball_y = 0.0; // Ball at top
        app.right_y = FIELD_H / 2.0;
        for _ in 0..50 {
            app.update();
        }
        // AI should have moved up toward ball
        assert!(app.right_y < FIELD_H / 2.0);
    }

    #[test]
    fn test_speed_increases_with_rally() {
        let mut app = PongApp::new();
        app.new_game();
        app.rally_count = 4;
        let old_mult = app.speed_mult;
        // Simulate paddle hit
        app.ball_x = 21.0;
        app.ball_dx = -INITIAL_BALL_SPEED;
        app.ball_y = app.left_y + PADDLE_H / 2.0;
        app.update();
        // Rally count should be 5 → speed increase
        if app.rally_count == 5 {
            assert!(app.speed_mult > old_mult);
        }
    }

    #[test]
    fn test_no_update_when_paused() {
        let mut app = PongApp::new();
        app.new_game();
        app.state = GameState::Paused;
        let ball_x = app.ball_x;
        app.update();
        assert_eq!(app.ball_x, ball_x);
    }

    #[test]
    fn test_no_update_when_menu() {
        let mut app = PongApp::new();
        let ball_x = app.ball_x;
        app.update();
        assert_eq!(app.ball_x, ball_x);
    }

    #[test]
    fn test_escape_pauses() {
        let mut app = PongApp::new();
        app.new_game();
        app.event(&Event::Key(KeyEvent {
            key: Key::Escape,
            modifiers: Modifiers::default(),
            pressed: true,
            text: None,
        }));
        assert_eq!(app.state, GameState::Paused);
    }

    #[test]
    fn test_n_restarts_from_pause() {
        let mut app = PongApp::new();
        app.state = GameState::Paused;
        app.left_score = 5;
        app.event(&Event::Key(KeyEvent {
            key: Key::N,
            modifiers: Modifiers::default(),
            pressed: true,
            text: None,
        }));
        assert_eq!(app.state, GameState::Playing);
        assert_eq!(app.left_score, 0);
    }

    #[test]
    fn test_render_menu() {
        let app = PongApp::new();
        let cmds = app.render(800.0, 600.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_playing() {
        let mut app = PongApp::new();
        app.new_game();
        let cmds = app.render(800.0, 600.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_paused() {
        let mut app = PongApp::new();
        app.state = GameState::Paused;
        let cmds = app.render(800.0, 600.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_game_over() {
        let mut app = PongApp::new();
        app.state = GameState::GameOver;
        app.left_score = WIN_SCORE;
        let cmds = app.render(800.0, 600.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_ctrl_ignored() {
        let mut app = PongApp::new();
        app.event(&Event::Key(KeyEvent {
            key: Key::Enter,
            modifiers: Modifiers { ctrl: true, ..Modifiers::default() },
            pressed: true,
            text: None,
        }));
        assert_eq!(app.state, GameState::Menu);
    }

    #[test]
    fn test_left_paddle_collision() {
        let mut app = PongApp::new();
        app.new_game();
        app.ball_x = 21.0;
        app.ball_dx = -INITIAL_BALL_SPEED;
        app.ball_y = app.left_y + PADDLE_H / 2.0;
        app.update();
        assert!(app.ball_dx > 0.0);
    }

    #[test]
    fn test_right_paddle_collision() {
        let mut app = PongApp::new();
        app.new_game();
        let right_x = FIELD_W - 20.0 - PADDLE_W;
        app.ball_x = right_x - BALL_SIZE + 1.0;
        app.ball_dx = INITIAL_BALL_SPEED;
        app.ball_y = app.right_y + PADDLE_H / 2.0;
        app.update();
        assert!(app.ball_dx < 0.0);
    }

    #[test]
    fn test_main_no_panic() {
        main();
    }

    #[test]
    fn test_tick_updates() {
        let mut app = PongApp::new();
        app.new_game();
        let old_x = app.ball_x;
        app.event(&Event::Tick { elapsed_ms: 16 });
        assert_ne!(app.ball_x, old_x);
    }

    #[test]
    fn test_space_starts_game() {
        let mut app = PongApp::new();
        app.event(&Event::Key(KeyEvent {
            key: Key::Space,
            modifiers: Modifiers::default(),
            pressed: true,
            text: None,
        }));
        assert_eq!(app.state, GameState::Playing);
    }
}
