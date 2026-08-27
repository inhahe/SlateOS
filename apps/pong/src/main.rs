//! Pong for SlateOS — two paddles, a ball, and an AI opponent.
//!
//! # What this file used to be
//!
//! `main` built a `PongApp` and dropped it. Everything below existed and none
//! of it ran. Giving it a window is what made the rest of the faults visible,
//! because each of them needs the game to actually be played to notice:
//!
//! * **A key press stuck the paddle to the wall.** The old input handler never
//!   looked at [`KeyEvent::pressed`], and a comment claimed the framework had
//!   no key releases — it has had them all along. So `up_held` was set by an
//!   Up press and never cleared: the paddle slid to the top of the field and
//!   stayed there. Worse, the *release* re-entered the whole state machine, so
//!   letting go of P paused again and letting go of Enter started a new game.
//! * **The ball's speed was the frame rate.** `update()` moved the ball a
//!   fixed distance per tick regardless of how long the tick was, so the game
//!   ran at whatever pace the compositor happened to deliver events. Speeds
//!   are now in field units per *second* and the step is scaled by the
//!   `elapsed_ms` each tick actually carries. See [`PongApp::advance`], which
//!   also sub-steps: a ball moved a whole tick's distance in one jump passes
//!   straight through a paddle twelve units wide.
//! * **The layout decided the window size.** `FIELD_X`/`FIELD_Y` were absolute
//!   screen coordinates and the field was a constant 700x500, so `render` took
//!   a width and a height and used them for one line of help text. Everything
//!   now comes from [`Layout`], which fits the field into the live window with
//!   its aspect kept — a field stretched on one axis would make a bounce leave
//!   at an angle it did not arrive at.
//! * **Nothing was clickable.** Pong is the game a pointer was made for, and
//!   the pointer could do nothing at all. Moving it over the field now steers
//!   the player's paddle, and the message box and the footer buttons do what
//!   they say.

#![allow(clippy::too_many_lines)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::cast_possible_wrap)]
#![allow(clippy::similar_names)]

use guitk::color::Color;
#[cfg(test)]
use guitk::event::Modifiers;
use guitk::event::{Event, EventResult, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::frame::Rect;
use guitk::probe::Probe;
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::style::CornerRadii;
use oswindow::app::{self, App, Response};
use std::process::ExitCode;
use std::time::Duration;

const COL_BASE: Color = Color::from_hex(0x1E1E2E);
const COL_MANTLE: Color = Color::from_hex(0x181825);
const COL_SURFACE0: Color = Color::from_hex(0x313244);
const COL_SURFACE1: Color = Color::from_hex(0x45475A);
const COL_TEXT: Color = Color::from_hex(0xCDD6F4);
const COL_SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const COL_BLUE: Color = Color::from_hex(0x89B4FA);
const COL_GREEN: Color = Color::from_hex(0xA6E3A1);
const COL_RED: Color = Color::from_hex(0xF38BA8);
const COL_YELLOW: Color = Color::from_hex(0xF9E2AF);
const COL_OVERLAY0: Color = Color::from_hex(0x6C7086);

// ── The playfield, in field units ───────────────────────────────────
//
// The game is played on a fixed 700x500 board and drawn onto whatever the
// window happens to be. Keeping the physics in its own units is what lets a
// resize be a drawing change and nothing more: a ball that bounced at 40
// degrees in a small window bounces at 40 degrees in a large one, and the same
// seed of play produces the same rally.

const FIELD_W: f32 = 700.0;
const FIELD_H: f32 = 500.0;
const PADDLE_W: f32 = 12.0;
const PADDLE_H: f32 = 80.0;
const PADDLE_INSET: f32 = 20.0;
const BALL_SIZE: f32 = 10.0;
const WIN_SCORE: u32 = 11;

/// Field units per second, not per tick.
///
/// The old values were per *tick* — 5.0 and 3.5 — which made every speed in
/// the game a function of how often the compositor felt like waking it. These
/// are the same speeds at the 60 Hz the old numbers were evidently written
/// for, expressed in a unit that does not change when the frame rate does.
const PADDLE_SPEED: f32 = 300.0;
const INITIAL_BALL_SPEED: f32 = 210.0;
const AI_SPEED: f32 = 210.0;
/// How far off centre a paddle hit throws the ball, in field units per second.
const DEFLECTION: f32 = 360.0;

/// The furthest the ball may travel in one collision test.
///
/// A paddle is twelve units wide. A ball moved more than that in a single step
/// can be on the far side of one without ever having been inside it, which is
/// how a "solid" paddle lets a ball through. [`PongApp::advance`] splits a
/// tick into steps no longer than this rather than trusting the tick to be
/// short — and a tick is exactly the thing an app may not assume about.
const MAX_STEP: f32 = 4.0;

/// The most simulated time one tick may be worth.
///
/// A window that was frozen for ten seconds owes the player some catching up,
/// but not ten seconds of a ball they could not see and could not have
/// returned. Past this the time is dropped rather than played out.
const MAX_CATCHUP_MS: u64 = 250;

/// One dash of the centre line, in field units. The gap is the same again.
///
/// In field units and not pixels so that the dashes belong to the board: a
/// twelve-pixel dash is a dotted line in a small window and a row of bricks in
/// a large one, and the line stops meaning "here is the middle".
const DASH_H: f32 = 12.0;

/// The footer's sentence — the whole of the game's instructions, since the
/// buttons beside it already name the two things that are not movement.
const HELP: &str = "\u{2191}/\u{2193} or the mouse to move  \u{2022}  first to 11 wins";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GameState {
    Menu,
    Playing,
    Paused,
    GameOver,
}

// ── What a pointer can do ───────────────────────────────────────────

/// Something the player can ask the game to do.
///
/// One enum for both input paths, so the footer buttons and the keys cannot
/// drift apart: a key that gains an action and no button is an action the
/// pointer silently cannot reach.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Start, or start over. What Enter does from the menu and from the end.
    NewGame,
    /// Pause a running game, resume a paused one.
    PauseToggle,
}

/// The footer buttons: every action, with the key that also performs it.
const BUTTONS: [(Action, &str); 2] = [
    (Action::NewGame, "N  New game"),
    (Action::PauseToggle, "P  Pause"),
];

/// Everything on screen a click can land on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// The play area. The pointer steers the player's paddle across it, which
    /// is how every Pong with a mouse has ever worked.
    Field,
    /// A footer button, which performs the action it names.
    Button(Action),
    /// The message box, which does what its text says: start, resume, or play
    /// again.
    Overlay,
}

/// A frame of this app's drawing, with the boxes a click can land in.
pub type Frame = guitk::frame::Frame<Target>;

// ── Layout ──────────────────────────────────────────────────────────

/// Where everything goes, at the size the window is *now*.
///
/// Built from the live width and height on every frame and never stored on the
/// app. The previous version had the field at a constant `(50, 50)` and a
/// constant 700x500, so the program was correct at one window size and drew
/// off the edge at every other.
#[derive(Clone, Copy, Debug)]
pub struct Layout {
    pub window: Rect,
    pub pad: f32,
    /// The scoreboard strip along the top.
    pub header: Rect,
    /// The field's drawn extent. Always 700:500, whatever the window is.
    pub field: Rect,
    /// Drawn pixels per field unit.
    pub scale: f32,
    /// The button strip along the bottom. `Rect::EMPTY` when there is no room.
    pub footer: Rect,
    pub overlay: Rect,
    pub font: f32,
}

impl Layout {
    #[must_use]
    pub fn new(width: f32, height: f32) -> Self {
        let window = Rect::new(0.0, 0.0, width.max(1.0), height.max(1.0));
        // A fraction with no lower bound: a padding floor of eight pixels is
        // wider than a four-pixel window, and everything measured from it then
        // starts outside the window it was measured for.
        let pad = (window.w.min(window.h) * 0.025).min(12.0);
        let font = (window.h / 50.0).clamp(7.0, 13.0);

        let body_w = (window.w - pad * 2.0).max(0.0);
        // Capped by what is left after the padding, not by the window: the
        // twenty-pixel floor is a legibility wish, and a window shorter than
        // that does not grant it by letting the scoreboard hang off the bottom.
        let head_h = (window.h * 0.09)
            .clamp(20.0, 46.0)
            .min((window.h - pad * 2.0).max(0.0));
        let header = Rect::new(pad, pad, body_w, head_h);

        // The footer is dropped rather than shrunk: two buttons at four pixels
        // each are not buttons, and the field is the game.
        let foot_h = (window.h * 0.07).clamp(18.0, 30.0);
        let room_h = (window.h - header.bottom() - pad * 2.0).max(0.0);
        let has_footer = room_h - foot_h - pad >= FIELD_H * 0.15 && body_w >= 120.0;
        let footer = if has_footer {
            Rect::new(pad, window.h - pad - foot_h, body_w, foot_h)
        } else {
            Rect::EMPTY
        };

        let mid_y = header.bottom() + pad;
        let mid_h = if has_footer {
            (footer.y - pad - mid_y).max(0.0)
        } else {
            (window.h - pad - mid_y).max(0.0)
        };

        // One scale for both axes. Fitting the field to the window instead
        // would stretch it, and a stretched field is one where a ball leaves a
        // paddle at an angle it did not arrive at — the bounce would look
        // wrong and, worse, would differ between window shapes.
        let scale = (body_w / FIELD_W).min(mid_h / FIELD_H).max(0.0);
        let fw = FIELD_W * scale;
        let fh = FIELD_H * scale;
        let field = Rect::new(
            pad + ((body_w - fw) / 2.0).max(0.0),
            mid_y + ((mid_h - fh) / 2.0).max(0.0),
            fw,
            fh,
        );

        let ow = (window.w * 0.72).clamp(80.0, 320.0).min(window.w);
        let oh = (window.h * 0.3).clamp(44.0, 120.0).min(window.h);
        let overlay = Rect::new((window.w - ow) / 2.0, (window.h - oh) / 2.0, ow, oh);

        Self {
            window,
            pad,
            header,
            field,
            scale,
            footer,
            overlay,
            font,
        }
    }

    /// The drawn box of a field-space rectangle.
    #[must_use]
    pub fn to_screen(&self, x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect::new(
            self.field.x + x * self.scale,
            self.field.y + y * self.scale,
            w * self.scale,
            h * self.scale,
        )
    }

    /// Where a pointer at screen `y` is pointing, in field units.
    ///
    /// The inverse of [`Self::to_screen`] on one axis, and the reason the
    /// paddle follows the pointer rather than jumping: a click at the top of
    /// the field means the top of the field at any window size.
    #[must_use]
    pub fn field_y(&self, y: f32) -> f32 {
        if self.scale <= 0.0 {
            return 0.0;
        }
        ((y - self.field.y) / self.scale).clamp(0.0, FIELD_H)
    }

    /// The box of footer button `index`, or `Rect::EMPTY` if there is none.
    #[must_use]
    pub fn button(&self, index: usize) -> Rect {
        if self.footer.is_empty() || index >= BUTTONS.len() {
            return Rect::EMPTY;
        }
        // The buttons take the left of the strip and the help text the rest,
        // so a narrow window loses the sentence before it loses the buttons.
        let w = (self.footer.w * 0.24).min(110.0);
        Rect::new(
            self.footer.x + index as f32 * (w + self.pad),
            self.footer.y,
            w,
            self.footer.h,
        )
    }
}

struct PongApp {
    state: GameState,
    // Paddles (y position = top of paddle, in field units)
    left_y: f32,
    right_y: f32,
    // Ball
    ball_x: f32,
    ball_y: f32,
    /// Velocity in field units per second.
    ball_dx: f32,
    ball_dy: f32,
    // Scores
    left_score: u32,
    right_score: u32,
    // Input state
    up_held: bool,
    down_held: bool,
    /// AI paddle speed, field units per second.
    ai_speed: f32,
    // Speed multiplier (increases over rallies)
    speed_mult: f32,
    rally_count: u32,

    /// The window's current size. The only thing remembered about the window;
    /// everything else about where things go is derived from it each frame.
    width: f32,
    height: f32,
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
            ai_speed: AI_SPEED,
            speed_mult: 1.0,
            rally_count: 0,
            width: 800.0,
            height: 620.0,
        };
        app.reset_ball(true);
        app
    }

    fn reset_ball(&mut self, go_right: bool) {
        self.ball_x = FIELD_W / 2.0;
        self.ball_y = FIELD_H / 2.0;
        let dir = if go_right { 1.0 } else { -1.0 };
        self.ball_dx = INITIAL_BALL_SPEED * dir;
        self.ball_dy = if self.rally_count.is_multiple_of(2) {
            INITIAL_BALL_SPEED * 0.43
        } else {
            -INITIAL_BALL_SPEED * 0.43
        };
        self.speed_mult = 1.0;
        self.rally_count = 0;
    }

    fn new_game(&mut self) {
        self.left_score = 0;
        self.right_score = 0;
        self.left_y = FIELD_H / 2.0 - PADDLE_H / 2.0;
        self.right_y = FIELD_H / 2.0 - PADDLE_H / 2.0;
        // Held keys belong to the game that is ending. Carrying them over
        // would start the new one with the paddle already sliding.
        self.up_held = false;
        self.down_held = false;
        self.reset_ball(true);
        self.state = GameState::Playing;
    }

    // ── Physics ─────────────────────────────────────────────────────

    /// Advance the game by the time a tick actually carried.
    ///
    /// Two things this does that the old per-tick `update` could not.
    ///
    /// It scales by `dt_ms`, so the ball's speed is a property of the game and
    /// not of how often the compositor woke the window. And it *sub-steps*: a
    /// paddle is twelve field units wide, so a ball moved further than that in
    /// one go can be past it without ever having been inside it, and a solid
    /// paddle lets the ball through. Splitting the step is the fix rather than
    /// asking for shorter ticks, because the tick length is precisely the
    /// thing an application is told not to assume.
    ///
    /// Returns whether anything moved, so a tick delivered to a paused game
    /// does not cost a repaint.
    fn advance(&mut self, dt_ms: u64) -> bool {
        if self.state != GameState::Playing {
            return false;
        }
        let dt = dt_ms.min(MAX_CATCHUP_MS) as f32 / 1000.0;
        if dt <= 0.0 {
            return false;
        }

        // The fastest anything moves this tick decides how finely to slice it.
        let speed = (self.ball_dx.abs() * self.speed_mult)
            .max(self.ball_dy.abs() * self.speed_mult)
            .max(PADDLE_SPEED)
            .max(1.0);
        let slice = (MAX_STEP / speed).max(0.001);

        let mut left = dt;
        while left > 0.0 {
            let step = left.min(slice);
            let scored = self.step(step);
            left -= step;
            // A point resets the ball to the middle. Carrying the remainder of
            // the tick into the new serve would move it before the player has
            // seen where it starts — and a game that has just ended has nothing
            // left to simulate at all.
            if scored || self.state != GameState::Playing {
                break;
            }
        }
        true
    }

    /// One slice of simulation, `dt` seconds long.
    ///
    /// Returns whether the slice ended a point, which is [`Self::advance`]'s
    /// cue to stop: what follows a serve is the next tick's business.
    fn step(&mut self, dt: f32) -> bool {
        // Player paddle movement
        if self.up_held {
            self.left_y = (self.left_y - PADDLE_SPEED * dt).max(0.0);
        }
        if self.down_held {
            self.left_y = (self.left_y + PADDLE_SPEED * dt).min(FIELD_H - PADDLE_H);
        }

        // AI paddle movement (tracks ball with slight lag)
        let ai_target = self.ball_y - PADDLE_H / 2.0;
        let reach = self.ai_speed * dt;
        let ai_move = (ai_target - self.right_y).clamp(-reach, reach);
        self.right_y = (self.right_y + ai_move).clamp(0.0, FIELD_H - PADDLE_H);

        // Ball movement
        self.ball_x += self.ball_dx * self.speed_mult * dt;
        self.ball_y += self.ball_dy * self.speed_mult * dt;

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
        let left_paddle_x = PADDLE_INSET;
        if self.ball_x <= left_paddle_x + PADDLE_W
            && self.ball_x + BALL_SIZE >= left_paddle_x
            && self.ball_y + BALL_SIZE >= self.left_y
            && self.ball_y <= self.left_y + PADDLE_H
            && self.ball_dx < 0.0
        {
            self.ball_dx = self.ball_dx.abs();
            // Angle based on where the ball hit the paddle.
            let hit_pos = (self.ball_y + BALL_SIZE / 2.0 - self.left_y) / PADDLE_H;
            self.ball_dy = (hit_pos - 0.5) * DEFLECTION;
            self.rally_count = self.rally_count.saturating_add(1);
            if self.rally_count.is_multiple_of(5) {
                self.speed_mult += 0.15;
            }
        }

        // Right paddle collision
        let right_paddle_x = FIELD_W - PADDLE_INSET - PADDLE_W;
        if self.ball_x + BALL_SIZE >= right_paddle_x
            && self.ball_x <= right_paddle_x + PADDLE_W
            && self.ball_y + BALL_SIZE >= self.right_y
            && self.ball_y <= self.right_y + PADDLE_H
            && self.ball_dx > 0.0
        {
            self.ball_dx = -self.ball_dx.abs();
            let hit_pos = (self.ball_y + BALL_SIZE / 2.0 - self.right_y) / PADDLE_H;
            self.ball_dy = (hit_pos - 0.5) * DEFLECTION;
            self.rally_count = self.rally_count.saturating_add(1);
            if self.rally_count.is_multiple_of(5) {
                self.speed_mult += 0.15;
            }
        }

        // Score
        if self.ball_x + BALL_SIZE < 0.0 {
            self.right_score = self.right_score.saturating_add(1);
            if self.right_score >= WIN_SCORE {
                self.state = GameState::GameOver;
            } else {
                self.reset_ball(true);
            }
            return true;
        }
        if self.ball_x > FIELD_W {
            self.left_score = self.left_score.saturating_add(1);
            if self.left_score >= WIN_SCORE {
                self.state = GameState::GameOver;
            } else {
                self.reset_ball(false);
            }
            return true;
        }
        false
    }

    // ── Input ───────────────────────────────────────────────────────

    /// Perform an action, whichever input asked for it.
    ///
    /// Returns whether anything the player can see changed.
    fn apply(&mut self, action: Action) -> bool {
        match action {
            Action::NewGame => {
                self.new_game();
                true
            }
            Action::PauseToggle => match self.state {
                GameState::Playing => {
                    self.state = GameState::Paused;
                    // A paddle held down when the game stops is a paddle still
                    // held when it starts again, several seconds later, with
                    // the player's finger long since off the key.
                    self.up_held = false;
                    self.down_held = false;
                    true
                }
                GameState::Paused => {
                    self.state = GameState::Playing;
                    true
                }
                // Neither the menu nor a finished game has anything to pause.
                GameState::Menu | GameState::GameOver => false,
            },
        }
    }

    /// The action a key press names, in the state the game is in.
    ///
    /// Enter and Space mean "get me playing" everywhere they do anything, and
    /// that is one action, not three state-specific ones.
    fn action_for_key(&self, key: Key) -> Option<Action> {
        match key {
            Key::N => Some(Action::NewGame),
            Key::P | Key::Escape => Some(Action::PauseToggle),
            Key::Enter | Key::Space => match self.state {
                GameState::Menu | GameState::GameOver => Some(Action::NewGame),
                GameState::Paused => Some(Action::PauseToggle),
                GameState::Playing => None,
            },
            _ => None,
        }
    }

    /// Handle a key event.
    ///
    /// The whole of the old handler ran on *any* key event, press or release,
    /// because it never read `pressed`. Releasing P therefore paused again and
    /// releasing Enter started a new game — and `up_held`, being set on the
    /// press and not cleared on the release, welded the paddle to the wall.
    fn handle_key(&mut self, event: &KeyEvent) -> EventResult {
        // Ctrl, Alt and Super combinations belong to the window and the
        // desktop. Ctrl-N is "new window" everywhere else on the screen.
        if event.modifiers.ctrl || event.modifiers.alt || event.modifiers.super_key {
            return EventResult::Ignored;
        }

        // Movement is held, so it is the only thing that cares about a
        // release, and it is the only thing a release may reach.
        match event.key {
            Key::Up => {
                self.up_held = event.pressed;
                if event.pressed {
                    self.down_held = false;
                }
                return EventResult::Consumed;
            }
            Key::Down => {
                self.down_held = event.pressed;
                if event.pressed {
                    self.up_held = false;
                }
                return EventResult::Consumed;
            }
            _ => {}
        }

        if !event.pressed {
            return EventResult::Ignored;
        }
        match self.action_for_key(event.key) {
            Some(action) => {
                self.apply(action);
                EventResult::Consumed
            }
            None => EventResult::Ignored,
        }
    }

    /// What a click at (x, y) would hit, at the size the window is now.
    #[must_use]
    pub fn target_at(&self, x: f32, y: f32) -> Option<Target> {
        self.frame(self.width, self.height).hit_test(x, y)
    }

    /// The action the message box performs, given what it currently says.
    fn overlay_action(&self) -> Option<Action> {
        match self.state {
            GameState::Menu | GameState::GameOver => Some(Action::NewGame),
            GameState::Paused => Some(Action::PauseToggle),
            GameState::Playing => None,
        }
    }

    fn handle_mouse(&mut self, mouse: &MouseEvent) -> EventResult {
        let target = self.target_at(mouse.x, mouse.y);
        match mouse.kind {
            // Steering. The paddle centres on the pointer rather than chasing
            // it, because a pointer is an absolute device and a paddle that
            // lagged behind it would feel like a fault rather than a handicap.
            MouseEventKind::Move => {
                if self.state != GameState::Playing || target != Some(Target::Field) {
                    return EventResult::Ignored;
                }
                let layout = Layout::new(self.width, self.height);
                self.left_y =
                    (layout.field_y(mouse.y) - PADDLE_H / 2.0).clamp(0.0, FIELD_H - PADDLE_H);
                // Steering by hand overrides a key still held, or the paddle
                // would slide out from under the pointer.
                self.up_held = false;
                self.down_held = false;
                EventResult::Consumed
            }
            MouseEventKind::Press(MouseButton::Left) => {
                let action = match target {
                    Some(Target::Button(action)) => Some(action),
                    Some(Target::Overlay) => self.overlay_action(),
                    // A click on the field is not a command. Steering already
                    // happened on the move that brought the pointer here. And a
                    // click on nothing at all belongs to whoever wants it.
                    Some(Target::Field) | None => None,
                };
                match action {
                    Some(action) => {
                        self.apply(action);
                        EventResult::Consumed
                    }
                    None => EventResult::Ignored,
                }
            }
            _ => EventResult::Ignored,
        }
    }

    /// Adopt a new window size. Nothing is recomputed here — the size is all
    /// the layout needs, and it is read fresh on the next frame.
    fn resize(&mut self, width: f32, height: f32) {
        self.width = width.max(1.0);
        self.height = height.max(1.0);
    }

    /// Whether an action would do anything in the state the game is in.
    ///
    /// A button for something that cannot happen is drawn dim rather than
    /// hidden: a control that vanishes is a control the player has to find
    /// again, and the footer would change width as the game changed state.
    fn enabled(&self, action: Action) -> bool {
        match action {
            Action::NewGame => true,
            Action::PauseToggle => {
                matches!(self.state, GameState::Playing | GameState::Paused)
            }
        }
    }

    // ── Drawing ─────────────────────────────────────────────────────

    /// Everything the window shows, at the size it is now.
    ///
    /// Built from scratch on each call, and it records the box of everything a
    /// click can land on *as it draws them*. That is what stops the hit test
    /// disagreeing with the picture: there is one description of where things
    /// are, and both the drawing and the pointer read it.
    #[must_use]
    pub fn frame(&self, width: f32, height: f32) -> Frame {
        let l = Layout::new(width, height);
        let mut f = Frame::new(l.window.w, l.window.h);
        fill(&mut f, l.window, COL_BASE, 0.0);
        self.draw_header(&mut f, &l);
        self.draw_field(&mut f, &l);
        self.draw_footer(&mut f, &l);
        // Last of all: `hit_test` walks the recorded boxes in reverse, so the
        // message box covering the field must be recorded after it to win.
        self.draw_overlay(&mut f, &l);
        f
    }

    /// The scoreboard strip. Replaces a pair of scores placed at the field's
    /// centre plus and minus sixty pixels, which was a centre only while the
    /// field started at x=50 and was exactly 700 wide.
    fn draw_header(&self, f: &mut Frame, l: &Layout) {
        if l.header.is_empty() {
            return;
        }
        fill(f, l.header, COL_MANTLE, 4.0);
        let (cx, cy) = l.header.centre();
        let digits = (l.header.h * 0.6).clamp(8.0, 28.0);
        centred(
            f,
            cx - l.header.w * 0.1,
            cy,
            &self.left_score.to_string(),
            digits,
            COL_BLUE,
            FontWeightHint::Bold,
        );
        centred(
            f,
            cx + l.header.w * 0.1,
            cy,
            &self.right_score.to_string(),
            digits,
            COL_RED,
            FontWeightHint::Bold,
        );
        centred(
            f,
            cx - l.header.w * 0.25,
            cy,
            "You",
            l.font,
            COL_SUBTEXT0,
            FontWeightHint::Regular,
        );
        centred(
            f,
            cx + l.header.w * 0.25,
            cy,
            "AI",
            l.font,
            COL_SUBTEXT0,
            FontWeightHint::Regular,
        );
    }

    /// The play area, in field units mapped through [`Layout::to_screen`].
    fn draw_field(&self, f: &mut Frame, l: &Layout) {
        if l.field.is_empty() {
            return;
        }
        fill(f, l.field, COL_MANTLE, 4.0);
        stroke(f, l.field, COL_SURFACE0, 1.0, 4.0);
        // The whole field is one target. Making the paddle the thing to grab
        // would mean catching it before you could move it, and the ball beats
        // you to the corner while you do.
        f.hit(Target::Field, l.field);

        // Centre line. Dashed in *field* units, so the dashes are part of the
        // board and scale with it rather than becoming a dotted line in a small
        // window and a row of bricks in a large one.
        let mut y = 0.0;
        while y < FIELD_H {
            let h = DASH_H.min(FIELD_H - y);
            fill(
                f,
                l.to_screen(FIELD_W / 2.0 - 1.0, y, 2.0, h),
                COL_SURFACE0,
                0.0,
            );
            y += DASH_H * 2.0;
        }

        if !matches!(self.state, GameState::Playing | GameState::Paused) {
            return;
        }
        fill(
            f,
            l.to_screen(PADDLE_INSET, self.left_y, PADDLE_W, PADDLE_H),
            COL_BLUE,
            3.0 * l.scale,
        );
        fill(
            f,
            l.to_screen(
                FIELD_W - PADDLE_INSET - PADDLE_W,
                self.right_y,
                PADDLE_W,
                PADDLE_H,
            ),
            COL_RED,
            3.0 * l.scale,
        );
        fill(
            f,
            l.to_screen(self.ball_x, self.ball_y, BALL_SIZE, BALL_SIZE),
            COL_TEXT,
            BALL_SIZE / 2.0 * l.scale,
        );
    }

    /// The button strip. Every entry of [`BUTTONS`] is drawn and recorded, so
    /// an action the keyboard gains is an action the pointer gains with it.
    fn draw_footer(&self, f: &mut Frame, l: &Layout) {
        if l.footer.is_empty() {
            return;
        }
        for (index, (action, text)) in BUTTONS.iter().enumerate() {
            let r = l.button(index);
            if r.is_empty() {
                continue;
            }
            let (bg, fg) = if self.enabled(*action) {
                (COL_SURFACE0, COL_TEXT)
            } else {
                (COL_SURFACE1, COL_OVERLAY0)
            };
            fill(f, r, bg, 4.0);
            let (cx, cy) = r.centre();
            centred(f, cx, cy, text, l.font, fg, FontWeightHint::Regular);
            // Recorded even while dim. The click is still this button's click;
            // `apply` is what decides it changes nothing. Declining to record
            // it would drop the press through to whatever lies behind.
            f.hit(Target::Button(*action), r);
        }

        // The sentence takes what the buttons left, and is dropped rather than
        // squeezed: half a sentence tells nobody anything.
        let last = l.button(BUTTONS.len().saturating_sub(1));
        let x = last.right() + l.pad;
        let w = l.footer.right() - x;
        if w >= 110.0 {
            label(
                f,
                x,
                l.footer.y + (l.footer.h - l.font) / 2.0,
                HELP,
                l.font,
                COL_OVERLAY0,
                FontWeightHint::Regular,
                Some(w),
            );
        }
    }

    /// The message box, drawn only when it has something to say — and it says
    /// what clicking it does, because clicking it does that.
    fn draw_overlay(&self, f: &mut Frame, l: &Layout) {
        let (title, note, color) = match self.state {
            GameState::Menu => ("PONG", "Press Enter to start", COL_TEXT),
            GameState::Paused => ("PAUSED", "Press P to resume", COL_YELLOW),
            GameState::GameOver => {
                if self.left_score >= WIN_SCORE {
                    ("You win", "Press Enter to play again", COL_GREEN)
                } else {
                    ("AI wins", "Press Enter to play again", COL_RED)
                }
            }
            // A game in progress has nothing to announce.
            GameState::Playing => return,
        };

        // Dim what is behind first, so the box reads as being in front of a
        // game rather than beside one.
        if !l.field.is_empty() {
            fill(f, l.field, Color::rgba(0, 0, 0, 150), 4.0);
        }
        let r = l.overlay;
        if r.is_empty() {
            return;
        }
        fill(f, r, COL_SURFACE0, 6.0);
        stroke(f, r, COL_SURFACE1, 1.0, 6.0);
        let (cx, cy) = r.centre();
        let big = (r.h * 0.3).clamp(10.0, 30.0);
        centred(
            f,
            cx,
            cy - r.h * 0.16,
            title,
            big,
            color,
            FontWeightHint::Bold,
        );
        centred(
            f,
            cx,
            cy + r.h * 0.22,
            note,
            l.font,
            COL_SUBTEXT0,
            FontWeightHint::Regular,
        );
        f.hit(Target::Overlay, r);
    }
}

// ── Drawing helpers ─────────────────────────────────────────────────

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
        corner_radii: CornerRadii::all(radius.max(0.0)),
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
        corner_radii: CornerRadii::all(radius.max(0.0)),
    });
}

fn label(
    f: &mut Frame,
    x: f32,
    y: f32,
    text: &str,
    size: f32,
    color: Color,
    weight: FontWeightHint,
    max_width: Option<f32>,
) {
    if size <= 0.0 || text.is_empty() {
        return;
    }
    f.push(RenderCommand::Text {
        x,
        y,
        text: text.to_string(),
        color,
        font_size: size,
        font_weight: weight,
        max_width,
        // A cut label is only readable if the cut is marked, and every label
        // here that has a width limit is one whose text can outgrow it.
        overflow: if max_width.is_some() {
            TextOverflow::Ellipsis
        } else {
            TextOverflow::Clip
        },
    });
}

/// Draw `text` centred on (`cx`, `cy`).
///
/// The width is estimated from the character count rather than measured,
/// because this function produces render commands and the compositor is what
/// shapes them — there is no font here to ask. 0.55 em is close for the digits
/// and short words this program centres, and a score a few pixels off centre is
/// not worth a round trip to a font.
fn centred(
    f: &mut Frame,
    cx: f32,
    cy: f32,
    text: &str,
    size: f32,
    color: Color,
    weight: FontWeightHint,
) {
    let w = text.chars().count() as f32 * size * 0.55;
    label(
        f,
        cx - w / 2.0,
        cy - size / 2.0,
        text,
        size,
        color,
        weight,
        None,
    );
}

// ── Window wiring ───────────────────────────────────────────────────

/// One body for every event, whoever delivers it.
///
/// [`App::on_event`] and the [`Probe`] impl both call this, so a test that
/// clicks the Pause button is a test of the shipped program and not of a second
/// implementation written to make the test pass.
fn handle_event(app: &mut PongApp, event: &Event) -> EventResult {
    match event {
        Event::Key(key) => app.handle_key(key),
        Event::Mouse(mouse) => app.handle_mouse(mouse),
        Event::Resize { width, height } => {
            app.resize(*width as f32, *height as f32);
            EventResult::Consumed
        }
        // A window without the keyboard cannot be played, and a ball that kept
        // going while it was away would be answered by nobody: the player comes
        // back to a score they did not lose. Pausing is the only honest thing
        // to do with time the player could not use.
        Event::FocusOut => {
            if app.state == GameState::Playing {
                app.apply(Action::PauseToggle);
                EventResult::Consumed
            } else {
                EventResult::Ignored
            }
        }
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

impl App for PongApp {
    fn on_event(&mut self, event: &Event) -> Response {
        match handle_event(self, event) {
            EventResult::Consumed => Response::Redraw,
            EventResult::Ignored => Response::Idle,
        }
    }

    fn render(&mut self, width: f32, height: f32) -> RenderTree {
        self.resize(width, height);
        self.frame(width, height).into_tree()
    }

    fn title(&self) -> String {
        String::from("Pong")
    }

    fn app_id(&self) -> String {
        String::from("pong")
    }

    fn initial_size(&self) -> (u32, u32) {
        (800, 620)
    }

    /// How often the game asks to be woken — a floor, not a promise.
    ///
    /// The distinction is the whole of the second fault this file used to
    /// have. [`PongApp::advance`] takes the `elapsed_ms` the tick actually
    /// carried and moves the ball by that much, so a tick that comes late moves
    /// it *further*, not slower, and the rally plays out at the same speed on a
    /// compositor running at 30 Hz as on one running at 144. Nothing here may
    /// assume this number was honoured.
    ///
    /// Dropped entirely outside play: a menu is a still picture, and a still
    /// picture that asks to be redrawn sixty times a second is a still picture
    /// that keeps a laptop awake.
    fn tick_interval(&self) -> Option<Duration> {
        match self.state {
            GameState::Playing => Some(Duration::from_millis(16)),
            GameState::Menu | GameState::Paused | GameState::GameOver => None,
        }
    }
}

impl Probe for PongApp {
    type Target = Target;
    type Outcome = EventResult;

    const SIZE: (f32, f32) = (800.0, 620.0);

    fn draw(&self, size: (f32, f32)) -> Frame {
        self.frame(size.0, size.1)
    }

    fn click_at(&mut self, x: f32, y: f32, button: MouseButton, size: (f32, f32)) -> EventResult {
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

    fn key_at(&mut self, key: &KeyEvent, size: (f32, f32)) -> EventResult {
        self.resize(size.0, size.1);
        handle_event(self, &Event::Key(key.clone()))
    }
}

fn main() -> ExitCode {
    let mut app = PongApp::new();
    app::launch("pong", &mut app)
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
    use guitk::probe;

    const SIZE: (f32, f32) = PongApp::SIZE;

    /// One compositor frame at 60 Hz, which is what the old per-tick constants
    /// were evidently written for and so what the converted tests below mean by
    /// "a tick".
    const FRAME: u64 = 16;

    /// A game under way, which is the state most of this is about.
    fn playing() -> PongApp {
        let mut app = PongApp::new();
        app.new_game();
        app.resize(SIZE.0, SIZE.1);
        app
    }

    fn release(key: Key) -> KeyEvent {
        KeyEvent {
            key,
            pressed: false,
            modifiers: Modifiers::NONE,
            text: String::new(),
        }
    }

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
        handle_event(
            &mut app,
            &Event::Key(KeyEvent {
                key: Key::Enter,
                modifiers: Modifiers::default(),
                pressed: true,
                text: String::new(),
            }),
        );
        assert_eq!(app.state, GameState::Playing);
    }

    #[test]
    fn test_pause() {
        let mut app = PongApp::new();
        app.new_game();
        handle_event(
            &mut app,
            &Event::Key(KeyEvent {
                key: Key::P,
                modifiers: Modifiers::default(),
                pressed: true,
                text: String::new(),
            }),
        );
        assert_eq!(app.state, GameState::Paused);
    }

    #[test]
    fn test_unpause() {
        let mut app = PongApp::new();
        app.new_game();
        app.state = GameState::Paused;
        handle_event(
            &mut app,
            &Event::Key(KeyEvent {
                key: Key::P,
                modifiers: Modifiers::default(),
                pressed: true,
                text: String::new(),
            }),
        );
        assert_eq!(app.state, GameState::Playing);
    }

    #[test]
    fn test_ball_moves() {
        let mut app = PongApp::new();
        app.new_game();
        let old_x = app.ball_x;
        app.advance(FRAME);
        assert_ne!(app.ball_x, old_x);
    }

    #[test]
    fn test_ball_top_bounce() {
        let mut app = PongApp::new();
        app.new_game();
        app.ball_y = -1.0;
        app.ball_dy = -2.0;
        app.advance(FRAME);
        assert!(app.ball_dy > 0.0);
    }

    #[test]
    fn test_ball_bottom_bounce() {
        let mut app = PongApp::new();
        app.new_game();
        app.ball_y = FIELD_H;
        app.ball_dy = 2.0;
        app.advance(FRAME);
        assert!(app.ball_dy < 0.0);
    }

    #[test]
    fn test_player_paddle_up() {
        let mut app = PongApp::new();
        app.new_game();
        app.up_held = true;
        let old_y = app.left_y;
        app.advance(FRAME);
        assert!(app.left_y < old_y);
    }

    #[test]
    fn test_player_paddle_down() {
        let mut app = PongApp::new();
        app.new_game();
        app.down_held = true;
        let old_y = app.left_y;
        app.advance(FRAME);
        assert!(app.left_y > old_y);
    }

    #[test]
    fn test_paddle_clamp_top() {
        let mut app = PongApp::new();
        app.new_game();
        app.left_y = 0.0;
        app.up_held = true;
        app.advance(FRAME);
        assert_eq!(app.left_y, 0.0);
    }

    #[test]
    fn test_paddle_clamp_bottom() {
        let mut app = PongApp::new();
        app.new_game();
        app.left_y = FIELD_H - PADDLE_H;
        app.down_held = true;
        app.advance(FRAME);
        assert_eq!(app.left_y, FIELD_H - PADDLE_H);
    }

    #[test]
    fn test_right_score_on_left_miss() {
        let mut app = PongApp::new();
        app.new_game();
        app.ball_x = -BALL_SIZE - 1.0;
        app.ball_dx = -1.0;
        app.advance(FRAME);
        assert_eq!(app.right_score, 1);
    }

    #[test]
    fn test_left_score_on_right_miss() {
        let mut app = PongApp::new();
        app.new_game();
        app.ball_x = FIELD_W + 1.0;
        app.ball_dx = 1.0;
        app.advance(FRAME);
        assert_eq!(app.left_score, 1);
    }

    #[test]
    fn test_game_over_right_wins() {
        let mut app = PongApp::new();
        app.new_game();
        app.right_score = WIN_SCORE - 1;
        app.ball_x = -BALL_SIZE - 1.0;
        app.ball_dx = -1.0;
        app.advance(FRAME);
        assert_eq!(app.state, GameState::GameOver);
    }

    #[test]
    fn test_game_over_left_wins() {
        let mut app = PongApp::new();
        app.new_game();
        app.left_score = WIN_SCORE - 1;
        app.ball_x = FIELD_W + 1.0;
        app.ball_dx = 1.0;
        app.advance(FRAME);
        assert_eq!(app.state, GameState::GameOver);
    }

    #[test]
    fn test_game_over_enter_restarts() {
        let mut app = PongApp::new();
        app.state = GameState::GameOver;
        app.left_score = 11;
        handle_event(
            &mut app,
            &Event::Key(KeyEvent {
                key: Key::Enter,
                modifiers: Modifiers::default(),
                pressed: true,
                text: String::new(),
            }),
        );
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
            app.advance(FRAME);
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
        app.advance(FRAME);
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
        app.advance(FRAME);
        assert_eq!(app.ball_x, ball_x);
    }

    #[test]
    fn test_no_update_when_menu() {
        let mut app = PongApp::new();
        let ball_x = app.ball_x;
        app.advance(FRAME);
        assert_eq!(app.ball_x, ball_x);
    }

    #[test]
    fn test_escape_pauses() {
        let mut app = PongApp::new();
        app.new_game();
        handle_event(
            &mut app,
            &Event::Key(KeyEvent {
                key: Key::Escape,
                modifiers: Modifiers::default(),
                pressed: true,
                text: String::new(),
            }),
        );
        assert_eq!(app.state, GameState::Paused);
    }

    #[test]
    fn test_n_restarts_from_pause() {
        let mut app = PongApp::new();
        app.state = GameState::Paused;
        app.left_score = 5;
        handle_event(
            &mut app,
            &Event::Key(KeyEvent {
                key: Key::N,
                modifiers: Modifiers::default(),
                pressed: true,
                text: String::new(),
            }),
        );
        assert_eq!(app.state, GameState::Playing);
        assert_eq!(app.left_score, 0);
    }

    #[test]
    fn test_render_menu() {
        let app = PongApp::new();
        let f = app.frame(800.0, 600.0);
        assert!(!f.commands().is_empty());
        assert!(f.is_balanced());
    }

    #[test]
    fn test_render_playing() {
        let mut app = PongApp::new();
        app.new_game();
        let f = app.frame(800.0, 600.0);
        assert!(!f.commands().is_empty());
        assert!(f.is_balanced());
    }

    #[test]
    fn test_render_paused() {
        let mut app = PongApp::new();
        app.state = GameState::Paused;
        let f = app.frame(800.0, 600.0);
        assert!(!f.commands().is_empty());
        assert!(f.is_balanced());
    }

    #[test]
    fn test_render_game_over() {
        let mut app = PongApp::new();
        app.state = GameState::GameOver;
        app.left_score = WIN_SCORE;
        let f = app.frame(800.0, 600.0);
        assert!(!f.commands().is_empty());
        assert!(f.is_balanced());
    }

    #[test]
    fn test_ctrl_ignored() {
        let mut app = PongApp::new();
        handle_event(
            &mut app,
            &Event::Key(KeyEvent {
                key: Key::Enter,
                modifiers: Modifiers {
                    ctrl: true,
                    ..Modifiers::default()
                },
                pressed: true,
                text: String::new(),
            }),
        );
        assert_eq!(app.state, GameState::Menu);
    }

    #[test]
    fn test_left_paddle_collision() {
        let mut app = PongApp::new();
        app.new_game();
        app.ball_x = 21.0;
        app.ball_dx = -INITIAL_BALL_SPEED;
        app.ball_y = app.left_y + PADDLE_H / 2.0;
        app.advance(FRAME);
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
        app.advance(FRAME);
        assert!(app.ball_dx < 0.0);
    }

    #[test]
    fn test_tick_updates() {
        let mut app = PongApp::new();
        app.new_game();
        let old_x = app.ball_x;
        handle_event(&mut app, &Event::Tick { elapsed_ms: 16 });
        assert_ne!(app.ball_x, old_x);
    }

    #[test]
    fn test_space_starts_game() {
        let mut app = PongApp::new();
        handle_event(
            &mut app,
            &Event::Key(KeyEvent {
                key: Key::Space,
                modifiers: Modifiers::default(),
                pressed: true,
                text: String::new(),
            }),
        );
        assert_eq!(app.state, GameState::Playing);
    }

    // ── The pointer ─────────────────────────────────────────────────

    #[test]
    fn the_buttons_offer_every_action_the_keys_do() {
        let app = PongApp::new();
        for key in [Key::N, Key::P, Key::Escape, Key::Enter, Key::Space] {
            let Some(action) = app.action_for_key(key) else {
                continue;
            };
            assert!(
                BUTTONS.iter().any(|(a, _)| *a == action),
                "{key:?} performs {action:?}, which no button offers"
            );
        }
    }

    #[test]
    fn every_button_is_the_action_it_names() {
        for (action, text) in BUTTONS {
            let mut clicked = playing();
            assert_eq!(
                probe::click(&mut clicked, Target::Button(action)),
                EventResult::Consumed,
                "{text} did not take its own click"
            );

            let mut applied = playing();
            applied.apply(action);
            assert_eq!(clicked.state, applied.state, "{text} did something else");
            assert_eq!(clicked.left_score, applied.left_score);
            assert_eq!(clicked.right_score, applied.right_score);
        }
    }

    #[test]
    fn a_button_that_can_do_nothing_still_takes_its_own_click() {
        // Pause is drawn dim on the menu, but it must still swallow the press:
        // letting it fall through would hand the click to whatever lies behind,
        // and a button you can see is a button that has been clicked.
        let mut app = PongApp::new();
        assert!(!app.enabled(Action::PauseToggle));
        assert_eq!(
            probe::click(&mut app, Target::Button(Action::PauseToggle)),
            EventResult::Consumed
        );
        assert_eq!(app.state, GameState::Menu);
    }

    #[test]
    fn the_message_box_does_what_its_text_says() {
        for state in [GameState::Menu, GameState::Paused, GameState::GameOver] {
            let mut app = playing();
            app.state = state;
            assert_eq!(
                probe::click(&mut app, Target::Overlay),
                EventResult::Consumed
            );
            assert_eq!(
                app.state,
                GameState::Playing,
                "the box shown in {state:?} did not do what it offered"
            );
        }
    }

    #[test]
    fn a_game_in_progress_has_no_message_box() {
        let app = playing();
        assert!(probe::rect_of(&app, Target::Overlay).is_none());
    }

    #[test]
    fn the_message_box_is_in_front_of_the_field_it_covers() {
        let app = PongApp::new();
        let l = Layout::new(SIZE.0, SIZE.1);
        let (cx, cy) = l.overlay.centre();
        assert!(
            l.field.contains(cx, cy),
            "this proves nothing unless the box is over the field"
        );
        assert_eq!(app.target_at(cx, cy), Some(Target::Overlay));
    }

    #[test]
    fn a_click_on_nothing_is_left_for_whoever_wants_it() {
        let mut app = playing();
        assert_eq!(probe::click_background(&mut app), EventResult::Ignored);
    }

    #[test]
    fn the_pointer_steers_the_paddle_to_where_it_points() {
        let mut app = playing();
        let field = probe::rect_of(&app, Target::Field).expect("a field to point at");
        let y = field.y + field.h * 0.25;
        let outcome = handle_event(
            &mut app,
            &Event::Mouse(MouseEvent {
                x: field.centre().0,
                y,
                kind: MouseEventKind::Move,
            }),
        );
        assert_eq!(outcome, EventResult::Consumed);
        let l = Layout::new(SIZE.0, SIZE.1);
        let want = (l.field_y(y) - PADDLE_H / 2.0).clamp(0.0, FIELD_H - PADDLE_H);
        assert!(
            (app.left_y - want).abs() < 0.01,
            "paddle at {} for a pointer asking for {want}",
            app.left_y
        );
    }

    #[test]
    fn the_pointer_only_steers_while_it_is_over_the_field() {
        let mut app = playing();
        let before = app.left_y;
        let (hx, hy) = Layout::new(SIZE.0, SIZE.1).header.centre();
        let outcome = handle_event(
            &mut app,
            &Event::Mouse(MouseEvent {
                x: hx,
                y: hy,
                kind: MouseEventKind::Move,
            }),
        );
        assert_eq!(outcome, EventResult::Ignored);
        assert_eq!(app.left_y, before);
    }

    #[test]
    fn the_pointer_takes_the_paddle_from_a_key_that_is_still_held() {
        // Otherwise the paddle slides out from under the pointer that just
        // placed it, which reads as the game fighting the player.
        let mut app = playing();
        probe::key(&mut app, &probe::press(Key::Up));
        assert!(app.up_held);
        let (cx, cy) = Layout::new(SIZE.0, SIZE.1).field.centre();
        handle_event(
            &mut app,
            &Event::Mouse(MouseEvent {
                x: cx,
                y: cy,
                kind: MouseEventKind::Move,
            }),
        );
        assert!(!app.up_held);
    }

    #[test]
    fn a_ctrl_or_alt_combination_belongs_to_the_desktop() {
        for modifiers in [
            Modifiers {
                ctrl: true,
                ..Modifiers::NONE
            },
            Modifiers {
                alt: true,
                ..Modifiers::NONE
            },
            Modifiers {
                super_key: true,
                ..Modifiers::NONE
            },
        ] {
            let mut app = PongApp::new();
            let outcome = probe::key(&mut app, &probe::press_with(Key::N, modifiers));
            assert_eq!(outcome, EventResult::Ignored, "{modifiers:?}");
            assert_eq!(app.state, GameState::Menu);
        }
    }

    // ── Key releases, which this framework has always had ───────────

    #[test]
    fn releasing_the_key_is_what_stops_the_paddle() {
        // The headline fault: the old handler never read `pressed`, so a single
        // Up press set `up_held` for good and welded the paddle to the top of
        // the field for the rest of the game.
        let mut app = playing();
        probe::key(&mut app, &probe::press(Key::Up));
        assert!(app.up_held);
        probe::key(&mut app, &release(Key::Up));
        assert!(!app.up_held);

        let resting = app.left_y;
        for _ in 0..40 {
            app.advance(FRAME);
        }
        assert_eq!(
            app.left_y, resting,
            "the paddle kept climbing after the key was let go"
        );
    }

    #[test]
    fn a_release_is_not_a_second_press() {
        // Letting go of P used to pause again; letting go of Enter used to
        // start a whole new game. Both because the release re-entered the
        // state machine the press had just been through.
        let mut app = playing();
        probe::key(&mut app, &probe::press(Key::P));
        assert_eq!(app.state, GameState::Paused);
        assert_eq!(
            probe::key(&mut app, &release(Key::P)),
            EventResult::Ignored,
            "the release of P was taken for a second press"
        );
        assert_eq!(app.state, GameState::Paused);

        let mut app = playing();
        app.left_score = 4;
        probe::key(&mut app, &release(Key::Enter));
        assert_eq!(app.left_score, 4, "the release of Enter started a new game");
    }

    #[test]
    fn the_two_directions_do_not_hold_each_other_down() {
        let mut app = playing();
        probe::key(&mut app, &probe::press(Key::Up));
        probe::key(&mut app, &probe::press(Key::Down));
        assert!(app.down_held);
        assert!(
            !app.up_held,
            "both directions held is a paddle standing still"
        );
    }

    #[test]
    fn pausing_lets_go_of_the_keys() {
        // A paddle held down when the game stops is a paddle still held when it
        // starts again, seconds later, with the player's finger long gone.
        let mut app = playing();
        probe::key(&mut app, &probe::press(Key::Down));
        assert!(app.down_held);
        probe::key(&mut app, &probe::press(Key::P));
        assert!(!app.down_held);
    }

    // ── Time, which is what a tick carries and not what a tick is ───

    #[test]
    fn the_ball_goes_the_same_distance_however_the_ticks_are_cut() {
        // The second fault: `update` moved the ball a fixed distance per tick,
        // so the game's speed was the compositor's frame rate.
        let mut coarse = playing();
        let mut fine = playing();
        coarse.advance(100);
        for _ in 0..10 {
            fine.advance(10);
        }
        assert!(
            (coarse.ball_x - fine.ball_x).abs() < 1.0,
            "one 100 ms tick put the ball at {} and ten 10 ms ticks at {}",
            coarse.ball_x,
            fine.ball_x
        );
        assert!((coarse.ball_y - fine.ball_y).abs() < 1.0);
    }

    #[test]
    fn a_window_that_was_frozen_does_not_owe_the_player_ten_seconds() {
        let mut frozen = playing();
        let mut capped = playing();
        frozen.advance(10_000);
        capped.advance(MAX_CATCHUP_MS);
        assert!((frozen.ball_x - capped.ball_x).abs() < 1.0);
    }

    #[test]
    fn a_paddle_stops_a_ball_that_would_jump_straight_past_it() {
        // A paddle is twelve field units wide and this ball covers three
        // hundred in a tick. An unsliced step would put it on the far side
        // without it ever having been inside, and a solid paddle would miss.
        let mut app = playing();
        app.ball_x = 200.0;
        app.ball_y = 250.0;
        app.left_y = 250.0 - PADDLE_H / 2.0;
        app.ball_dx = -3000.0;
        app.ball_dy = 0.0;
        app.advance(100);
        assert!(app.ball_dx > 0.0, "the ball went through the paddle");
        assert_eq!(app.right_score, 0);
    }

    #[test]
    fn a_serve_waits_for_the_next_tick_rather_than_the_rest_of_this_one() {
        let mut app = playing();
        app.ball_x = -BALL_SIZE - 1.0;
        app.ball_dx = -INITIAL_BALL_SPEED;
        app.advance(200);
        assert_eq!(app.right_score, 1);
        assert_eq!(
            app.ball_x,
            FIELD_W / 2.0,
            "the rest of the tick moved the new ball before it was seen"
        );
    }

    #[test]
    fn the_game_asks_for_ticks_only_while_it_is_playing() {
        let mut app = PongApp::new();
        assert!(
            app.tick_interval().is_none(),
            "a menu is a still picture and should not keep a laptop awake"
        );
        app.new_game();
        assert!(app.tick_interval().is_some());
        app.apply(Action::PauseToggle);
        assert!(app.tick_interval().is_none());
        app.state = GameState::GameOver;
        assert!(app.tick_interval().is_none());
    }

    #[test]
    fn a_tick_that_moves_nothing_does_not_cost_a_repaint() {
        let mut app = PongApp::new();
        assert_eq!(
            handle_event(&mut app, &Event::Tick { elapsed_ms: FRAME }),
            EventResult::Ignored
        );
        app.new_game();
        assert_eq!(
            handle_event(&mut app, &Event::Tick { elapsed_ms: FRAME }),
            EventResult::Consumed
        );
    }

    #[test]
    fn losing_focus_pauses_rather_than_playing_on_unwatched() {
        let mut app = playing();
        app.up_held = true;
        assert_eq!(
            handle_event(&mut app, &Event::FocusOut),
            EventResult::Consumed
        );
        assert_eq!(app.state, GameState::Paused);
        assert!(!app.up_held);
        assert_eq!(
            handle_event(&mut app, &Event::FocusOut),
            EventResult::Ignored,
            "a game already stopped has nothing left to stop"
        );
    }

    // ── The layout, which follows the window ────────────────────────

    #[test]
    fn the_layout_stays_inside_the_window_at_every_size() {
        for w in [1.0, 4.0, 40.0, 120.0, 400.0, 800.0, 2400.0] {
            for h in [1.0, 4.0, 40.0, 120.0, 400.0, 620.0, 1800.0] {
                let l = Layout::new(w, h);
                for (name, r) in [
                    ("header", l.header),
                    ("field", l.field),
                    ("footer", l.footer),
                    ("overlay", l.overlay),
                    ("button 0", l.button(0)),
                    ("button 1", l.button(1)),
                ] {
                    if r.is_empty() {
                        continue;
                    }
                    assert!(
                        r.x >= -0.01
                            && r.y >= -0.01
                            && r.right() <= w + 0.01
                            && r.bottom() <= h + 0.01,
                        "{name} {r:?} runs outside a {w}x{h} window"
                    );
                }
                assert!(l.scale.is_finite() && l.scale >= 0.0);
                assert!(l.font > 0.0 && l.pad >= 0.0);
            }
        }
    }

    #[test]
    fn the_field_keeps_its_shape_whatever_shape_the_window_is() {
        // A stretched field is one where the ball leaves a paddle at an angle
        // it did not arrive at, and where the same rally plays differently in
        // a tall window than in a wide one.
        for (w, h) in [
            (800.0, 620.0),
            (400.0, 1200.0),
            (2000.0, 400.0),
            (640.0, 480.0),
        ] {
            let l = Layout::new(w, h);
            if l.field.is_empty() {
                continue;
            }
            let ratio = l.field.w / l.field.h;
            assert!(
                (ratio - FIELD_W / FIELD_H).abs() < 0.001,
                "a {w}x{h} window stretched the field to {ratio}"
            );
        }
    }

    #[test]
    fn a_cramped_window_drops_the_buttons_rather_than_the_game() {
        let roomy = Layout::new(SIZE.0, SIZE.1);
        assert!(!roomy.footer.is_empty());
        assert!(!roomy.button(0).is_empty());
        assert!(roomy.button(BUTTONS.len()).is_empty());

        // Too short: a strip of four-pixel buttons is not a strip of buttons.
        assert!(Layout::new(200.0, 100.0).footer.is_empty());
        // Too narrow: the labels would be a column of single letters.
        assert!(Layout::new(100.0, 620.0).footer.is_empty());
    }

    #[test]
    fn the_buttons_follow_the_window_when_it_is_resized() {
        let app = playing();
        let target = Target::Button(Action::NewGame);
        let small = probe::rect_of_sized(&app, target, (700.0, 560.0)).unwrap();
        let large = probe::rect_of_sized(&app, target, (1400.0, 1000.0)).unwrap();
        assert!(
            large.y > small.y,
            "the footer stayed where the smaller window put it"
        );
    }

    #[test]
    fn a_resize_event_is_what_the_next_frame_is_drawn_at() {
        let mut app = playing();
        handle_event(
            &mut app,
            &Event::Resize {
                width: 1200,
                height: 900,
            },
        );
        let grown = Layout::new(1200.0, 900.0);
        let (x, y) = (grown.field.right() - 1.0, grown.field.bottom() - 1.0);
        assert!(
            !Layout::new(SIZE.0, SIZE.1).field.contains(x, y),
            "this proves nothing unless the point is new"
        );
        assert_eq!(app.target_at(x, y), Some(Target::Field));
    }

    #[test]
    fn a_point_on_the_screen_names_the_same_point_on_the_field_at_any_size() {
        for (w, h) in [(800.0, 620.0), (1600.0, 1200.0), (500.0, 900.0)] {
            let l = Layout::new(w, h);
            for fy in [0.0, 125.0, 250.0, 499.0] {
                let screen = l.to_screen(0.0, fy, 1.0, 1.0);
                let back = l.field_y(screen.y);
                assert!(
                    (back - fy).abs() < 0.01,
                    "{w}x{h}: field {fy} drawn at {} came back as {back}",
                    screen.y
                );
            }
        }
    }

    #[test]
    fn every_state_draws_a_balanced_frame_at_every_size() {
        for state in [
            GameState::Menu,
            GameState::Playing,
            GameState::Paused,
            GameState::GameOver,
        ] {
            let mut app = playing();
            app.state = state;
            for (w, h) in [(1.0, 1.0), (200.0, 100.0), SIZE, (1920.0, 1080.0)] {
                let f = app.frame(w, h);
                assert!(f.is_balanced(), "{state:?} at {w}x{h}");
                assert!(
                    !f.commands().is_empty(),
                    "{state:?} at {w}x{h} drew nothing"
                );
            }
        }
    }
}
