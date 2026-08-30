//! Slate OS Asteroids -- classic space shooter arcade game.
//!
//! The player controls a triangular ship in the center of a wraparound
//! playfield. Asteroids of three sizes drift across the screen; shooting
//! a large asteroid splits it into two medium ones, medium into two small.
//! The player earns points for destroying asteroids and loses lives on
//! collision. Clearing all asteroids advances the wave.
//!
//! Controls: Left/Right to rotate, Up to thrust, Space to shoot,
//! P to pause, N for new game. While the game is paused or over, a click
//! anywhere on the playfield does what the overlay says the keyboard does.
//!
//! ## What this program was
//!
//! `main` built an `AsteroidsApp` and dropped it. There was no window: the
//! drawing pass returned a `Vec<RenderCommand>` measured against a window
//! size the program worked out for itself from a fixed 800x600 playfield,
//! and `handle_event` -- the only way in for a keystroke -- had no caller.
//! Twelve blanket `#![allow(...)]` at the top of the file, `dead_code` and
//! `unused_imports` among them, are what kept a compiler from saying so.
//!
//! It now opens a real window, lays every band out from the size that window
//! reports each frame, records a hit box for everything it draws, and answers
//! keys and clicks through one body that the tests drive too.

use guitk::color::Color;
use guitk::event::{Event, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::frame::{Frame, Rect};
use guitk::probe::Probe;
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::style::CornerRadii;
use guitk::text;
use oswindow::app::{self, App, Response};
use std::process::ExitCode;
use std::time::Duration;

// ── Catppuccin Mocha palette ────────────────────────────────────────
const BASE: Color = Color::from_hex(0x1E1E2E);
const MANTLE: Color = Color::from_hex(0x181825);
const TEXT_COLOR: Color = Color::from_hex(0xCDD6F4);
const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const BLUE: Color = Color::from_hex(0x89B4FA);
const GREEN: Color = Color::from_hex(0xA6E3A1);
const RED: Color = Color::from_hex(0xF38BA8);
const YELLOW: Color = Color::from_hex(0xF9E2AF);
const PEACH: Color = Color::from_hex(0xFAB387);
const LAVENDER: Color = Color::from_hex(0xB4BEFE);
const OVERLAY0: Color = Color::from_hex(0x6C7086);
const TEAL: Color = Color::from_hex(0x94E2D5);

// ── World size ──────────────────────────────────────────────────────

/// How wide the world is, in the units the game is played in.
///
/// This is a *game rule*, not a drawing size: it is how far a shot travels
/// before it comes back round the other side. The window scales the world to
/// fit and letterboxes what is left over, so a wider window shows the same
/// game bigger rather than a bigger game.
const FIELD_WIDTH: f32 = 800.0;
/// How tall the world is. See [`FIELD_WIDTH`].
const FIELD_HEIGHT: f32 = 600.0;

/// The window size the game opens at, and the size a test reads a click
/// against unless it says otherwise: enough for the world at 1:1 plus the
/// header band and its margins.
const WINDOW_WIDTH: f32 = FIELD_WIDTH + 24.0;
/// The height that goes with [`WINDOW_WIDTH`].
const WINDOW_HEIGHT: f32 = FIELD_HEIGHT + 74.0;
/// [`WINDOW_WIDTH`] as the window server wants it.
const INITIAL_WINDOW_W: u32 = 824;
/// [`WINDOW_HEIGHT`] as the window server wants it.
const INITIAL_WINDOW_H: u32 = 674;

/// How often the window is asked for a frame -- about sixty a second.
///
/// The game's motion is worked out from the elapsed time each tick reports,
/// not from the tick count, so a slower or a jittery clock slows the frame
/// rate without slowing the game.
const TICK: Duration = Duration::from_millis(16);

// ── Game constants ──────────────────────────────────────────────────
const SHIP_RADIUS: f32 = 15.0;
const SHIP_THRUST: f32 = 200.0;
const SHIP_DRAG: f32 = 0.98;
const SHIP_ROTATION_SPEED: f32 = 5.0;
const MAX_SPEED: f32 = 400.0;

const BULLET_SPEED: f32 = 500.0;
const BULLET_LIFETIME: f32 = 1.5;
const BULLET_RADIUS: f32 = 2.5;
const MAX_BULLETS: usize = 8;
const SHOOT_COOLDOWN: f32 = 0.15;

const ASTEROID_LARGE_RADIUS: f32 = 40.0;
const ASTEROID_MEDIUM_RADIUS: f32 = 22.0;
const ASTEROID_SMALL_RADIUS: f32 = 12.0;
const ASTEROID_LARGE_SPEED: f32 = 60.0;
const ASTEROID_MEDIUM_SPEED: f32 = 100.0;
const ASTEROID_SMALL_SPEED: f32 = 150.0;

const SCORE_LARGE: u32 = 20;
const SCORE_MEDIUM: u32 = 50;
const SCORE_SMALL: u32 = 100;

const INITIAL_LIVES: u32 = 3;
const INITIAL_ASTEROIDS: usize = 4;
/// The most asteroids a wave will ever open with. See [`AsteroidsApp::advance_wave`].
const MAX_WAVE_ASTEROIDS: usize = 12;
const RESPAWN_DELAY: f32 = 2.0;
const INVULNERABLE_TIME: f32 = 3.0;

/// Minimum safe distance from ship center for spawning asteroids.
const SAFE_SPAWN_DISTANCE: f32 = 150.0;

// ── Math helpers ────────────────────────────────────────────────────

/// Compute sine using a Taylor series approximation (no std dependency needed
/// in `no_std` environments, but we use `std` here for accuracy via `f32::sin`).
fn sin_f32(x: f32) -> f32 {
    x.sin()
}

fn cos_f32(x: f32) -> f32 {
    x.cos()
}

const PI: f32 = std::f32::consts::PI;
const TAU: f32 = std::f32::consts::TAU;

/// Normalize angle to [0, TAU).
fn normalize_angle(a: f32) -> f32 {
    let mut r = a % TAU;
    if r < 0.0 {
        r += TAU;
    }
    r
}

// ── Randomness ──────────────────────────────────────────────────────
//
// From `randrange`, not a local LCG. The local one's `next_bounded` reduced
// with `state % bound`, which on a modulus-2^64 generator returns the low bits
// — and those are a counter, not noise: the low two are a pure function of how
// many draws have been taken. `random_edge_position` picks the spawn edge with
// a bound of 4, and a Large asteroid consumes exactly **16** draws (edge, one
// coordinate, heading, speed, ten vertex radii, spin angle, spin rate). 16 is a
// multiple of 4, so every edge draw in a wave landed on the same two bits, and
// **every asteroid in the game entered from the same side of the screen** —
// which side being the only thing the seed chose. Verified before the fix over
// seeds 1, 2, 3, 42, 777 and 123456: eight spawns each, one edge each.
//
// The float path was already sound: it took the top 24 bits via `>> 40`, which
// is why the asteroids' shapes and headings looked fine while their entry point
// did not. See `known-issues.md` and `design-decisions.md` §447.
use randrange::{RandomSource, SeededRng};

/// Returns a random heading in `[0, TAU)`.
///
/// A free function rather than a method: the generator lives in another crate
/// now, and "a full turn" is this game's unit, not the generator's.
fn random_angle(rng: &mut SeededRng) -> f32 {
    rng.unit_f32() * TAU
}

// ── Vec2 ────────────────────────────────────────────────────────────

/// A point, or a direction and a distance, in field coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vec2 {
    x: f32,
    y: f32,
}

impl Vec2 {
    const ZERO: Self = Self { x: 0.0, y: 0.0 };

    const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    fn length(self) -> f32 {
        (self.x * self.x + self.y * self.y).sqrt()
    }

    fn add(self, other: Self) -> Self {
        Self {
            x: self.x + other.x,
            y: self.y + other.y,
        }
    }

    /// One point taken from another.
    ///
    /// The game itself never wants a straight difference -- the field wraps,
    /// so "which way is that asteroid" is always the wrapped answer, and a
    /// straight one would send a ship the long way round. It is kept for the
    /// tests, which work in a corner of the field where wrapping does not
    /// come into it, and is compiled only for them so that it cannot quietly
    /// become the wrong answer in the game.
    #[cfg(test)]
    fn sub(self, other: Self) -> Self {
        Self {
            x: self.x - other.x,
            y: self.y - other.y,
        }
    }

    fn scale(self, s: f32) -> Self {
        Self {
            x: self.x * s,
            y: self.y * s,
        }
    }

    /// How far apart two points are, ignoring the wrap. See [`Vec2::sub`].
    #[cfg(test)]
    fn distance_to(self, other: Self) -> f32 {
        self.sub(other).length()
    }

    /// Clamp the length of the vector.
    fn clamp_length(self, max: f32) -> Self {
        let len = self.length();
        if len > max {
            self.scale(max / len)
        } else {
            self
        }
    }

    /// Wrap position within the playfield boundaries.
    fn wrap(self, w: f32, h: f32) -> Self {
        let mut x = self.x % w;
        let mut y = self.y % h;
        if x < 0.0 {
            x += w;
        }
        if y < 0.0 {
            y += h;
        }
        Self { x, y }
    }

    /// Wrapped distance (shortest path on torus).
    fn wrapped_distance(self, other: Self, w: f32, h: f32) -> f32 {
        let mut dx = (self.x - other.x).abs();
        let mut dy = (self.y - other.y).abs();
        if dx > w / 2.0 {
            dx = w - dx;
        }
        if dy > h / 2.0 {
            dy = h - dy;
        }
        (dx * dx + dy * dy).sqrt()
    }
}

// ── Asteroid size ───────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AsteroidSize {
    Large,
    Medium,
    Small,
}

impl AsteroidSize {
    fn radius(self) -> f32 {
        match self {
            AsteroidSize::Large => ASTEROID_LARGE_RADIUS,
            AsteroidSize::Medium => ASTEROID_MEDIUM_RADIUS,
            AsteroidSize::Small => ASTEROID_SMALL_RADIUS,
        }
    }

    fn speed(self) -> f32 {
        match self {
            AsteroidSize::Large => ASTEROID_LARGE_SPEED,
            AsteroidSize::Medium => ASTEROID_MEDIUM_SPEED,
            AsteroidSize::Small => ASTEROID_SMALL_SPEED,
        }
    }

    fn score(self) -> u32 {
        match self {
            AsteroidSize::Large => SCORE_LARGE,
            AsteroidSize::Medium => SCORE_MEDIUM,
            AsteroidSize::Small => SCORE_SMALL,
        }
    }

    fn color(self) -> Color {
        match self {
            AsteroidSize::Large => SUBTEXT0,
            AsteroidSize::Medium => OVERLAY0,
            AsteroidSize::Small => Color::from_hex(0x585B70),
        }
    }

    /// Number of vertices for the polygon shape.
    fn vertex_count(self) -> usize {
        match self {
            AsteroidSize::Large => 10,
            AsteroidSize::Medium => 8,
            AsteroidSize::Small => 6,
        }
    }

    fn child_size(self) -> Option<AsteroidSize> {
        match self {
            AsteroidSize::Large => Some(AsteroidSize::Medium),
            AsteroidSize::Medium => Some(AsteroidSize::Small),
            AsteroidSize::Small => None,
        }
    }
}

// ── Asteroid ────────────────────────────────────────────────────────
#[derive(Clone, Debug)]
struct Asteroid {
    pos: Vec2,
    vel: Vec2,
    size: AsteroidSize,
    angle: f32,
    rotation_speed: f32,
    /// Vertex offsets from center for the jagged polygon shape.
    vertex_radii: Vec<f32>,
}

impl Asteroid {
    fn new(pos: Vec2, vel: Vec2, size: AsteroidSize, rng: &mut SeededRng) -> Self {
        let n = size.vertex_count();
        let base_r = size.radius();
        let mut vertex_radii = Vec::with_capacity(n);
        for _ in 0..n {
            // Vary each vertex radius by +/-30% for jagged look.
            vertex_radii.push(base_r * rng.between_f32(0.7, 1.3));
        }
        Self {
            pos,
            vel,
            size,
            angle: random_angle(rng),
            rotation_speed: rng.between_f32(-2.0, 2.0),
            vertex_radii,
        }
    }

    fn radius(&self) -> f32 {
        self.size.radius()
    }

    fn update(&mut self, dt: f32) {
        self.pos = self.pos.add(self.vel.scale(dt));
        self.pos = self.pos.wrap(FIELD_WIDTH, FIELD_HEIGHT);
        self.angle = normalize_angle(self.angle + self.rotation_speed * dt);
    }

    /// Get the polygon vertices for rendering (in world space).
    fn vertices(&self) -> Vec<Vec2> {
        let n = self.vertex_radii.len();
        let turn = TAU / f32_from_usize(n).max(1.0);
        self.vertex_radii
            .iter()
            .enumerate()
            .map(|(i, &r)| {
                let a = self.angle + f32_from_usize(i) * turn;
                Vec2::new(self.pos.x + cos_f32(a) * r, self.pos.y + sin_f32(a) * r)
            })
            .collect()
    }
}

// ── Bullet ──────────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug)]
struct Bullet {
    pos: Vec2,
    vel: Vec2,
    lifetime: f32,
}

impl Bullet {
    fn new(pos: Vec2, vel: Vec2) -> Self {
        Self {
            pos,
            vel,
            lifetime: BULLET_LIFETIME,
        }
    }

    fn alive(&self) -> bool {
        self.lifetime > 0.0
    }

    fn update(&mut self, dt: f32) {
        self.pos = self.pos.add(self.vel.scale(dt));
        self.pos = self.pos.wrap(FIELD_WIDTH, FIELD_HEIGHT);
        self.lifetime -= dt;
    }
}

// ── Ship ────────────────────────────────────────────────────────────
#[derive(Clone, Debug)]
struct Ship {
    pos: Vec2,
    vel: Vec2,
    angle: f32,
    thrusting: bool,
}

impl Ship {
    fn new() -> Self {
        Self {
            pos: Vec2::new(FIELD_WIDTH / 2.0, FIELD_HEIGHT / 2.0),
            vel: Vec2::ZERO,
            angle: -PI / 2.0, // Point upward.
            thrusting: false,
        }
    }

    fn reset(&mut self) {
        self.pos = Vec2::new(FIELD_WIDTH / 2.0, FIELD_HEIGHT / 2.0);
        self.vel = Vec2::ZERO;
        self.angle = -PI / 2.0;
        self.thrusting = false;
    }

    fn update(&mut self, dt: f32) {
        if self.thrusting {
            let ax = cos_f32(self.angle) * SHIP_THRUST;
            let ay = sin_f32(self.angle) * SHIP_THRUST;
            self.vel = self.vel.add(Vec2::new(ax * dt, ay * dt));
        }
        self.vel = self.vel.scale(SHIP_DRAG);
        self.vel = self.vel.clamp_length(MAX_SPEED);
        self.pos = self.pos.add(self.vel.scale(dt));
        self.pos = self.pos.wrap(FIELD_WIDTH, FIELD_HEIGHT);
    }

    fn rotate_left(&mut self, dt: f32) {
        self.angle -= SHIP_ROTATION_SPEED * dt;
        self.angle = normalize_angle(self.angle);
    }

    fn rotate_right(&mut self, dt: f32) {
        self.angle += SHIP_ROTATION_SPEED * dt;
        self.angle = normalize_angle(self.angle);
    }

    /// Nose tip position (front of the triangle).
    fn nose(&self) -> Vec2 {
        Vec2::new(
            self.pos.x + cos_f32(self.angle) * SHIP_RADIUS,
            self.pos.y + sin_f32(self.angle) * SHIP_RADIUS,
        )
    }

    /// Left rear vertex.
    fn left_wing(&self) -> Vec2 {
        let a = self.angle + 2.4;
        Vec2::new(
            self.pos.x + cos_f32(a) * SHIP_RADIUS,
            self.pos.y + sin_f32(a) * SHIP_RADIUS,
        )
    }

    /// Right rear vertex.
    fn right_wing(&self) -> Vec2 {
        let a = self.angle - 2.4;
        Vec2::new(
            self.pos.x + cos_f32(a) * SHIP_RADIUS,
            self.pos.y + sin_f32(a) * SHIP_RADIUS,
        )
    }

    /// Exhaust point (behind the ship center).
    fn exhaust_point(&self) -> Vec2 {
        Vec2::new(
            self.pos.x - cos_f32(self.angle) * SHIP_RADIUS * 0.6,
            self.pos.y - sin_f32(self.angle) * SHIP_RADIUS * 0.6,
        )
    }
}

// ── Particle (visual debris) ────────────────────────────────────────
#[derive(Clone, Copy, Debug)]
struct Particle {
    pos: Vec2,
    vel: Vec2,
    lifetime: f32,
    max_lifetime: f32,
    color: Color,
}

impl Particle {
    fn alive(&self) -> bool {
        self.lifetime > 0.0
    }

    fn update(&mut self, dt: f32) {
        self.pos = self.pos.add(self.vel.scale(dt));
        self.lifetime -= dt;
    }

    /// How solid the particle is, from full at birth to nothing at death.
    fn alpha(&self) -> u8 {
        if self.max_lifetime <= 0.0 {
            return 0;
        }
        let ratio = (self.lifetime / self.max_lifetime).clamp(0.0, 1.0);
        u8_from_f32(ratio * 255.0)
    }
}

// ── Game state ──────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GameState {
    Playing,
    Paused,
    GameOver,
}

// ── Input state ─────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug)]
struct InputState {
    left: bool,
    right: bool,
    thrust: bool,
    shoot: bool,
}

impl InputState {
    const fn new() -> Self {
        Self {
            left: false,
            right: false,
            thrust: false,
            shoot: false,
        }
    }
}

// ── Targets ─────────────────────────────────────────────────────────

/// Everything the drawing pass records a hit box for.
///
/// The game itself is played with the keyboard -- a pointer cannot fly a ship
/// -- so most of these exist to be *found* rather than clicked: a test that
/// wants to know the ship is on screen, or that the third asteroid is inside
/// the playfield, asks for its rectangle by name. The two that are clickable
/// are the two the overlays already tell the player to press a key for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// The band along the top holding the score and the rest.
    Header,
    /// The game's name.
    Title,
    /// Points so far this game.
    Score,
    /// The best score this program has seen since it started.
    HighScore,
    /// Ships left after this one.
    Lives,
    /// Which wave is being flown.
    Wave,
    /// The line reminding the player which keys do what.
    Controls,
    /// The playfield: everything the game is played inside.
    Field,
    /// The player's ship, when it is on screen.
    Ship,
    /// One asteroid, by its index in the field.
    Asteroid(usize),
    /// One bullet, by its index in the field.
    Bullet(usize),
    /// The dimming sheet a pause or a game over lays over the field.
    Overlay,
    /// The word "PAUSED", or "GAME OVER".
    OverlayTitle,
    /// Carry on with the game that is already going.
    Resume,
    /// Throw the current game away and start another.
    NewGame,
    /// A line of the game-over box that only reports a number.
    FinalStat(usize),
}

/// Whether an event changed anything the window would need to redraw.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventResult {
    Consumed,
    Ignored,
}

// ── Layout ──────────────────────────────────────────────────────────

/// Where the two bands go in a window of a given size.
///
/// The header gives up its height before the playfield does, so a window
/// squashed from above loses the score before it loses the game.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub window: Rect,
    /// Score, wave, lives, and the high score.
    pub header: Rect,
    /// What is left for the playfield to be fitted into.
    pub body: Rect,
    /// The header's type size.
    pub head: f32,
    /// Body text — the overlays.
    pub font: f32,
    /// The smallest type on show: the controls line.
    pub small: f32,
    /// The margin between a band and what is inside it.
    pub pad: f32,
}

impl Layout {
    /// Solve the bands for a window of this size.
    #[must_use]
    pub fn new(w: f32, h: f32) -> Self {
        let w = w.max(0.0);
        let h = h.max(0.0);
        let pad = (w.min(h) * 0.014).clamp(2.0, 12.0);
        let head = (h / 36.0).clamp(9.0, 20.0);
        let font = (h / 30.0).clamp(10.0, 26.0);
        let small = (h / 52.0).clamp(7.0, 14.0);

        // A share of `h` clamped into a band, then held to what there is: a
        // header taller than the window would leave the body a negative
        // height, and a rectangle of negative height draws inside out.
        let header_h = (h * 0.08).clamp(20.0, 56.0).min(h);
        let header = Rect::new(
            pad,
            pad,
            (w - pad * 2.0).max(0.0),
            (header_h - pad).max(0.0),
        );
        // Held to the window as well as measured from the header. In a window
        // shorter than the header's floor the header fills it exactly, so
        // `header.bottom() + pad` lands *past* the bottom edge -- at 824x6 it
        // is 8, two pixels outside. The band was empty there, so nothing was
        // drawn wrong and nothing looked wrong; but an empty rectangle at an
        // impossible origin is a number waiting to be used by whatever is
        // added next (a scroll base, a centring calculation, a hit box), and
        // it would then be wrong in a way that traces back to here rather than
        // to the code that trusted it.
        let body_y = (header.bottom() + pad).min(h);
        Self {
            window: Rect::new(0.0, 0.0, w, h),
            header,
            body: Rect::new(
                pad,
                body_y,
                (w - pad * 2.0).max(0.0),
                (h - body_y - pad).max(0.0),
            ),
            head,
            font,
            small,
            pad,
        }
    }
}

/// Where the playfield goes, and how a position in it reaches the screen.
///
/// The playfield is not the window. Asteroids, bullets and the ship all wrap at
/// [`FIELD_WIDTH`] and [`FIELD_HEIGHT`], so those two numbers are the *rules of
/// the game*, not a drawing size: a window that changed them would change how
/// far a shot travels before it comes back, and how much room there is to dodge
/// in. So the field keeps its size in game terms and the window decides only
/// how large it is drawn — the largest rectangle of the field's proportions
/// that fits the space, centred, with the leftover left as margin.
///
/// One number is solved — `scale` — and every position, radius and line width
/// follows from it, so the drawing pass and the hit test cannot disagree about
/// where anything is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Field {
    /// The space the field was given, which is usually bigger than the field.
    pub area: Rect,
    /// The field itself, centred in `area`.
    pub rect: Rect,
    /// Screen pixels per unit of field.
    pub scale: f32,
}

impl Field {
    /// Fit the playfield into `area`, centred.
    #[must_use]
    pub fn new(area: Rect) -> Self {
        let scale = (area.w / FIELD_WIDTH).min(area.h / FIELD_HEIGHT).max(0.0);
        let w = FIELD_WIDTH * scale;
        let h = FIELD_HEIGHT * scale;
        Self {
            area,
            rect: Rect::new(
                area.x + (area.w - w) / 2.0,
                area.y + (area.h - h) / 2.0,
                w,
                h,
            ),
            scale,
        }
    }

    /// Where a position in the field lands on the screen.
    #[must_use]
    pub fn to_screen(&self, p: Vec2) -> (f32, f32) {
        (
            self.rect.x + p.x * self.scale,
            self.rect.y + p.y * self.scale,
        )
    }

    /// A length in the field, in screen pixels.
    #[must_use]
    pub fn scaled(&self, v: f32) -> f32 {
        v * self.scale
    }

    /// A line width in the field, never thinner than the renderer can draw.
    ///
    /// A hairline that rounds away to nothing is a ship that vanishes in a
    /// small window, so the scaled width has a floor rather than being trusted.
    #[must_use]
    pub fn stroke(&self, v: f32) -> f32 {
        (v * self.scale).max(1.0)
    }
}

// ── Main app struct ─────────────────────────────────────────────────
pub struct AsteroidsApp {
    ship: Ship,
    bullets: Vec<Bullet>,
    asteroids: Vec<Asteroid>,
    particles: Vec<Particle>,
    state: GameState,
    score: u32,
    high_score: u32,
    lives: u32,
    wave: u32,
    input: InputState,
    shoot_cooldown: f32,
    respawn_timer: f32,
    invulnerable_timer: f32,
    ship_alive: bool,
    rng: SeededRng,
    frame_counter: u64,
    /// The size the last frame was drawn at, and so the size the next click
    /// is read against.
    size: (f32, f32),
}

impl AsteroidsApp {
    fn new() -> Self {
        Self::with_seed(42)
    }

    fn with_seed(seed: u64) -> Self {
        let mut app = Self {
            ship: Ship::new(),
            bullets: Vec::new(),
            asteroids: Vec::new(),
            particles: Vec::new(),
            state: GameState::Playing,
            score: 0,
            high_score: 0,
            lives: INITIAL_LIVES,
            wave: 1,
            input: InputState::new(),
            shoot_cooldown: 0.0,
            respawn_timer: 0.0,
            invulnerable_timer: INVULNERABLE_TIME,
            ship_alive: true,
            rng: SeededRng::new(seed),
            frame_counter: 0,
            size: (WINDOW_WIDTH, WINDOW_HEIGHT),
        };
        app.spawn_wave(INITIAL_ASTEROIDS);
        app
    }

    /// The size the next frame will be drawn at, and the next click read
    /// against.
    pub fn resize(&mut self, width: f32, height: f32) {
        self.size = (width.max(0.0), height.max(0.0));
    }

    /// The size the last frame was drawn at.
    #[must_use]
    pub const fn size(&self) -> (f32, f32) {
        self.size
    }

    // ── Wave / asteroid spawning ────────────────────────────────────

    /// Spawn `count` large asteroids at random edge positions, away from ship.
    fn spawn_wave(&mut self, count: usize) {
        for _ in 0..count {
            self.spawn_asteroid_at_edge(AsteroidSize::Large);
        }
    }

    /// Spawn a single asteroid of the given size at a random edge position.
    fn spawn_asteroid_at_edge(&mut self, size: AsteroidSize) {
        let pos = self.random_edge_position(size.radius());
        let angle = random_angle(&mut self.rng);
        let speed = size.speed() * self.rng.between_f32(0.5, 1.5);
        let vel = Vec2::new(cos_f32(angle) * speed, sin_f32(angle) * speed);
        self.asteroids
            .push(Asteroid::new(pos, vel, size, &mut self.rng));
    }

    /// Pick a random position along the field edges, ensuring it is far
    /// enough from the ship.
    fn random_edge_position(&mut self, radius: f32) -> Vec2 {
        // The loop is unbounded but cannot spin: each edge spans a full field
        // dimension, and the wrapped distance along it reaches half of that —
        // 400 across, 300 down — both comfortably past SAFE_SPAWN_DISTANCE
        // (150). So an acceptable point exists on *every* edge whatever the
        // ship is doing, and the retry only rejects the near ones.
        loop {
            let side = self.rng.below(4);
            let pos = match side {
                0 => Vec2::new(self.rng.between_f32(0.0, FIELD_WIDTH), radius),
                1 => Vec2::new(
                    self.rng.between_f32(0.0, FIELD_WIDTH),
                    FIELD_HEIGHT - radius,
                ),
                2 => Vec2::new(radius, self.rng.between_f32(0.0, FIELD_HEIGHT)),
                _ => Vec2::new(
                    FIELD_WIDTH - radius,
                    self.rng.between_f32(0.0, FIELD_HEIGHT),
                ),
            };
            if pos.wrapped_distance(self.ship.pos, FIELD_WIDTH, FIELD_HEIGHT) > SAFE_SPAWN_DISTANCE
            {
                return pos;
            }
        }
    }

    /// Spawn child asteroids when a parent is destroyed.
    fn spawn_children(&mut self, parent_pos: Vec2, parent_vel: Vec2, child_size: AsteroidSize) {
        for i in 0..2 {
            let offset_angle = if i == 0 { PI / 4.0 } else { -PI / 4.0 };
            let base_angle = parent_vel.y.atan2(parent_vel.x) + offset_angle;
            let speed = child_size.speed() * self.rng.between_f32(0.6, 1.4);
            let vel = Vec2::new(cos_f32(base_angle) * speed, sin_f32(base_angle) * speed);
            let nudge = Vec2::new(
                cos_f32(base_angle) * child_size.radius(),
                sin_f32(base_angle) * child_size.radius(),
            );
            let pos = parent_pos.add(nudge).wrap(FIELD_WIDTH, FIELD_HEIGHT);
            self.asteroids
                .push(Asteroid::new(pos, vel, child_size, &mut self.rng));
        }
    }

    // ── Particle effects ────────────────────────────────────────────

    fn spawn_explosion(&mut self, pos: Vec2, count: usize, color: Color) {
        for _ in 0..count {
            let angle = random_angle(&mut self.rng);
            let speed = self.rng.between_f32(30.0, 150.0);
            let lifetime = self.rng.between_f32(0.3, 0.8);
            self.particles.push(Particle {
                pos,
                vel: Vec2::new(cos_f32(angle) * speed, sin_f32(angle) * speed),
                lifetime,
                max_lifetime: lifetime,
                color,
            });
        }
    }

    fn spawn_thrust_particle(&mut self) {
        let exhaust = self.ship.exhaust_point();
        let angle = self.ship.angle + PI + self.rng.between_f32(-0.3, 0.3);
        let speed = self.rng.between_f32(50.0, 120.0);
        let lifetime = self.rng.between_f32(0.1, 0.3);
        self.particles.push(Particle {
            pos: exhaust,
            vel: Vec2::new(cos_f32(angle) * speed, sin_f32(angle) * speed),
            lifetime,
            max_lifetime: lifetime,
            color: PEACH,
        });
    }

    // ── New game / restart ──────────────────────────────────────────

    /// Throw the game away and start another, keeping what is not part of it.
    ///
    /// The window is not part of the game: starting a new one must not put the
    /// drawing back to the size the program guessed at startup, or the first
    /// click after a restart would be read against a window that is not there.
    fn new_game(&mut self) {
        let high = self.high_score;
        let size = self.size;
        let seed = self.rng.next_u64();
        *self = Self::with_seed(seed);
        self.high_score = high;
        self.size = size;
    }

    // ── Input handling ──────────────────────────────────────────────

    /// A key going down or coming back up.
    ///
    /// A key that is *released* still matters here -- that is how the ship
    /// stops turning -- so unlike most programs this one cannot throw away
    /// the up-stroke. What it must not do is act on an up-stroke as if it
    /// were a press: a released `P` used to be handled only in `Playing`, so
    /// it was already correct there, but the paused and game-over arms had to
    /// be guarded by hand. They still are, in one place rather than three.
    pub fn handle_key(&mut self, ev: &KeyEvent) -> EventResult {
        match self.state {
            GameState::Playing => self.handle_key_playing(ev.key, ev.pressed),
            GameState::Paused if ev.pressed => self.handle_key_paused(ev.key),
            GameState::GameOver if ev.pressed => self.handle_key_game_over(ev.key),
            _ => EventResult::Ignored,
        }
    }

    fn handle_key_playing(&mut self, key: Key, pressed: bool) -> EventResult {
        match key {
            Key::Left | Key::A => self.input.left = pressed,
            Key::Right | Key::D => self.input.right = pressed,
            Key::Up | Key::W => self.input.thrust = pressed,
            Key::Space => self.input.shoot = pressed,
            Key::P | Key::Escape if pressed => self.pause(),
            Key::N if pressed => self.new_game(),
            _ => return EventResult::Ignored,
        }
        EventResult::Consumed
    }

    fn handle_key_paused(&mut self, key: Key) -> EventResult {
        match key {
            Key::P | Key::Escape => self.state = GameState::Playing,
            Key::N => self.new_game(),
            _ => return EventResult::Ignored,
        }
        EventResult::Consumed
    }

    fn handle_key_game_over(&mut self, key: Key) -> EventResult {
        match key {
            Key::N | Key::Enter | Key::Space => self.new_game(),
            _ => return EventResult::Ignored,
        }
        EventResult::Consumed
    }

    /// Stop the game where it is.
    ///
    /// The held keys are let go along with it. A player who paused mid-turn
    /// and came back to a ship still turning would be right to call that a
    /// bug: the key is not down any more, and the up-stroke that would have
    /// cleared it went to whatever had the keyboard while the game was away.
    fn pause(&mut self) {
        self.state = GameState::Paused;
        self.input = InputState::new();
    }

    /// A click, wherever it landed.
    ///
    /// The pointer does nothing during play -- there is nothing on the
    /// playfield to click *at*. Over an overlay it does what the overlay says
    /// the keyboard does, which is the only reason the overlay's lines are
    /// hit boxes at all.
    ///
    /// Every part of a sheet counts as the sheet. Written the obvious way --
    /// one arm for `Target::Overlay`, one for `Target::NewGame` -- the
    /// game-over sheet had a dead zone in the shape of its own box: the box
    /// is drawn over the middle of the overlay, and its title and final-score
    /// lines are hit boxes recorded *after* the overlay's, so they, not the
    /// overlay, won the hit test. A click on "GAME OVER" or on "Score: 4200"
    /// did nothing while a click on the dim margin around them started a new
    /// game -- the dead zone was exactly the part of the sheet a person
    /// looks at. The pause sheet only escaped because its middle line
    /// happens to be `Resume`, which had an arm of its own.
    pub fn handle_mouse(&mut self, ev: &MouseEvent) -> EventResult {
        if !matches!(ev.kind, MouseEventKind::Press(MouseButton::Left)) {
            return EventResult::Ignored;
        }
        let (w, h) = self.size();
        let Some(target) = self.frame(w, h).hit_test(ev.x, ev.y) else {
            return EventResult::Ignored;
        };
        match (self.state, target) {
            // "New game" says so on both sheets, so it means that on both.
            // This arm comes first because the game-over arm below would
            // otherwise have to name every target except this one.
            (GameState::Paused | GameState::GameOver, Target::NewGame) => {
                self.new_game();
                EventResult::Consumed
            }
            (
                GameState::Paused,
                Target::Overlay | Target::OverlayTitle | Target::Resume | Target::FinalStat(_),
            ) => {
                self.state = GameState::Playing;
                EventResult::Consumed
            }
            (
                GameState::GameOver,
                Target::Overlay | Target::OverlayTitle | Target::Resume | Target::FinalStat(_),
            ) => {
                self.new_game();
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    // ── Game tick ───────────────────────────────────────────────────

    /// One frame of the game.
    ///
    /// A tick that arrives while the game is paused or over changes nothing,
    /// and says so: the window is told `Idle` and does not redraw a frame
    /// identical to the one on screen. That is the whole reason this returns
    /// anything -- at sixty ticks a second, a paused game that answered
    /// `Consumed` would repaint sixty times a second to no effect.
    pub fn handle_tick(&mut self, elapsed_ms: u64) -> EventResult {
        if self.state != GameState::Playing {
            return EventResult::Ignored;
        }

        let dt = ms_to_seconds(elapsed_ms);
        self.frame_counter = self.frame_counter.saturating_add(1);

        // Handle ship rotation.
        if self.ship_alive {
            if self.input.left {
                self.ship.rotate_left(dt);
            }
            if self.input.right {
                self.ship.rotate_right(dt);
            }
            self.ship.thrusting = self.input.thrust;

            // Thrust particles.
            if self.ship.thrusting && self.frame_counter.is_multiple_of(2) {
                self.spawn_thrust_particle();
            }
        }

        // Update ship.
        if self.ship_alive {
            self.ship.update(dt);
        }

        // Shooting.
        self.shoot_cooldown -= dt;
        if self.input.shoot
            && self.ship_alive
            && self.shoot_cooldown <= 0.0
            && self.bullets.len() < MAX_BULLETS
        {
            self.fire_bullet();
            self.shoot_cooldown = SHOOT_COOLDOWN;
        }

        // Update bullets.
        for bullet in &mut self.bullets {
            bullet.update(dt);
        }
        self.bullets.retain(|b| b.alive());

        // Update asteroids.
        for asteroid in &mut self.asteroids {
            asteroid.update(dt);
        }

        // Update particles.
        for particle in &mut self.particles {
            particle.update(dt);
        }
        self.particles.retain(|p| p.alive());

        // Collision: bullet vs asteroid.
        self.check_bullet_asteroid_collisions();

        // Collision: ship vs asteroid.
        if self.ship_alive {
            self.invulnerable_timer -= dt;
            self.check_ship_asteroid_collision();
        } else {
            // Respawn timer.
            self.respawn_timer -= dt;
            if self.respawn_timer <= 0.0 && self.lives > 0 {
                self.respawn_ship();
            }
        }

        // Wave cleared?
        if self.asteroids.is_empty() {
            self.advance_wave();
        }

        EventResult::Consumed
    }

    fn fire_bullet(&mut self) {
        let nose = self.ship.nose();
        let vel = Vec2::new(
            cos_f32(self.ship.angle) * BULLET_SPEED + self.ship.vel.x * 0.5,
            sin_f32(self.ship.angle) * BULLET_SPEED + self.ship.vel.y * 0.5,
        );
        self.bullets.push(Bullet::new(nose, vel));
    }

    fn check_bullet_asteroid_collisions(&mut self) {
        // Collect hits first (indices only), then apply mutations.
        // This avoids borrowing self.bullets/self.asteroids while mutating self.
        let mut hits: Vec<(usize, usize)> = Vec::new(); // (bullet_idx, asteroid_idx)

        for (bi, bullet) in self.bullets.iter().enumerate() {
            if hits.iter().any(|(b, _)| *b == bi) {
                continue;
            }
            for (ai, asteroid) in self.asteroids.iter().enumerate() {
                if hits.iter().any(|(_, a)| *a == ai) {
                    continue;
                }
                let dist = bullet
                    .pos
                    .wrapped_distance(asteroid.pos, FIELD_WIDTH, FIELD_HEIGHT);
                if dist < asteroid.radius() + BULLET_RADIUS {
                    hits.push((bi, ai));
                    break; // One bullet hits one asteroid.
                }
            }
        }

        // Collect data from hits before mutating.
        let mut score_gain: u32 = 0;
        let mut explosions: Vec<(Vec2, Color)> = Vec::new();
        let mut children_to_spawn: Vec<(Vec2, Vec2, AsteroidSize)> = Vec::new();
        let mut destroyed_indices: Vec<usize> = Vec::new();
        let mut spent_bullets: Vec<usize> = Vec::new();

        for &(bi, ai) in &hits {
            let Some(asteroid) = self.asteroids.get(ai) else {
                continue;
            };
            score_gain = score_gain.saturating_add(asteroid.size.score());
            explosions.push((asteroid.pos, asteroid.size.color()));
            if let Some(child_size) = asteroid.size.child_size() {
                children_to_spawn.push((asteroid.pos, asteroid.vel, child_size));
            }
            destroyed_indices.push(ai);
            spent_bullets.push(bi);
        }

        // Apply score. A score that saturates is a score no player will reach
        // -- four billion points is a hundred thousand hours of small
        // asteroids -- but a score that *wraps* would put a champion back at
        // nothing, which is the kind of thing that only ever shows up in a
        // bug report.
        self.score = self.score.saturating_add(score_gain);
        if self.score > self.high_score {
            self.high_score = self.score;
        }

        // Spawn explosions.
        for (pos, color) in explosions {
            self.spawn_explosion(pos, 8, color);
        }

        // Spawn children.
        for (parent_pos, parent_vel, child_size) in children_to_spawn {
            self.spawn_children(parent_pos, parent_vel, child_size);
        }

        // Remove destroyed asteroids (reverse order to preserve indices).
        destroyed_indices.sort_unstable();
        destroyed_indices.dedup();
        for &idx in destroyed_indices.iter().rev() {
            self.asteroids.remove(idx);
        }

        // Remove spent bullets.
        spent_bullets.sort_unstable();
        spent_bullets.dedup();
        for &idx in spent_bullets.iter().rev() {
            self.bullets.remove(idx);
        }
    }

    fn check_ship_asteroid_collision(&mut self) {
        if self.invulnerable_timer > 0.0 {
            return;
        }
        for asteroid in &self.asteroids {
            let dist = self
                .ship
                .pos
                .wrapped_distance(asteroid.pos, FIELD_WIDTH, FIELD_HEIGHT);
            if dist < SHIP_RADIUS + asteroid.radius() {
                self.destroy_ship();
                return;
            }
        }
    }

    fn destroy_ship(&mut self) {
        self.ship_alive = false;
        self.lives = self.lives.saturating_sub(1);
        self.spawn_explosion(self.ship.pos, 15, BLUE);
        self.input = InputState::new();

        if self.lives == 0 {
            self.state = GameState::GameOver;
            if self.score > self.high_score {
                self.high_score = self.score;
            }
        } else {
            self.respawn_timer = RESPAWN_DELAY;
        }
    }

    fn respawn_ship(&mut self) {
        self.ship.reset();
        self.ship_alive = true;
        self.invulnerable_timer = INVULNERABLE_TIME;
    }

    /// Clear the field and send in the next wave: one more asteroid than the
    /// last, up to a cap.
    ///
    /// The count used to be `INITIAL_ASTEROIDS + wave - 1` with nothing on the
    /// end of it. A player good enough to reach wave two hundred would have
    /// been sent two hundred and three asteroids, in a field eight hundred
    /// units across -- which is not a harder game, it is a field with no gaps
    /// left in it, and every one of them wants a spawn point at least
    /// `SAFE_SPAWN_DISTANCE` from the ship. The arcade machine this copies
    /// capped the count for the same reason.
    fn advance_wave(&mut self) {
        self.wave = self.wave.saturating_add(1);
        let extra = usize::try_from(self.wave.saturating_sub(1)).unwrap_or(usize::MAX);
        let count = INITIAL_ASTEROIDS
            .saturating_add(extra)
            .min(MAX_WAVE_ASTEROIDS);
        self.spawn_wave(count);
    }

    // ── Queries ─────────────────────────────────────────────────────

    /// How many asteroids are left. Nothing on screen reports this -- clearing
    /// the field is what the player sees -- so it is compiled for the tests,
    /// which is the only thing that reads it.
    #[cfg(test)]
    fn asteroid_count(&self) -> usize {
        self.asteroids.len()
    }

    /// How many shots are in the air. See [`AsteroidsApp::asteroid_count`].
    #[cfg(test)]
    fn bullet_count(&self) -> usize {
        self.bullets.len()
    }

    fn is_invulnerable(&self) -> bool {
        self.invulnerable_timer > 0.0
    }

    // ── Rendering ───────────────────────────────────────────────────

    /// The whole window, drawn for a window this size.
    ///
    /// Everything below comes from `width` and `height`. The program this
    /// replaces drew against `WINDOW_WIDTH`/`WINDOW_HEIGHT` -- two constants
    /// it worked out for itself from the 800x600 playfield -- so a window of
    /// any other size showed a game drawn for a window that was not there.
    #[must_use]
    pub fn frame(&self, width: f32, height: f32) -> Frame<Target> {
        let mut f = Frame::new(width, height);
        let l = Layout::new(width, height);
        fill(&mut f, l.window, BASE, CornerRadii::ZERO);
        self.draw_header(&mut f, &l);

        let field = Field::new(l.body);
        fill(&mut f, field.rect, MANTLE, CornerRadii::all(4.0));
        // Recorded before anything inside it, so the ship and the asteroids --
        // which are drawn after -- win the hit test where they overlap it.
        f.hit(Target::Field, field.rect);

        draw_stars(&mut f, &field);
        self.draw_particles(&mut f, &field);
        self.draw_asteroids(&mut f, &field);
        self.draw_bullets(&mut f, &field);
        if self.ship_alive {
            self.draw_ship(&mut f, &field);
        }
        self.draw_overlay(&mut f, &l, &field);
        f
    }

    /// The five readings along the top, in the order they are written.
    fn readings(&self) -> [(Target, String, Color); 5] {
        [
            (Target::Title, String::from("Asteroids"), TEAL),
            (Target::Score, format!("Score: {}", self.score), TEXT_COLOR),
            (
                Target::HighScore,
                format!("Hi: {}", self.high_score),
                YELLOW,
            ),
            (Target::Lives, format!("Lives: {}", self.lives), RED),
            (Target::Wave, format!("Wave: {}", self.wave), LAVENDER),
        ]
    }

    /// The band along the top: the readings, and the controls line under them.
    ///
    /// The readings used to be placed at 10, 120, 280, 420 and 560 pixels from
    /// the left edge -- numbers that only line up under a 824-pixel window and
    /// a score below five digits. They are now laid out by their own measured
    /// widths, and one that will not fit is dropped rather than drawn over its
    /// neighbour.
    fn draw_header(&self, f: &mut Frame<Target>, l: &Layout) {
        if l.header.is_empty() {
            return;
        }
        fill(f, l.header, MANTLE, CornerRadii::all(4.0));
        f.hit(Target::Header, l.header);

        let inner = Rect::new(
            l.header.x + l.pad,
            l.header.y + l.pad / 2.0,
            (l.header.w - l.pad * 2.0).max(0.0),
            (l.header.h - l.pad).max(0.0),
        );
        if inner.is_empty() {
            return;
        }

        // The controls line is the first thing a squeezed header gives up: a
        // reminder of which key turns left is worth less than the score, and
        // stacking the two rows on top of each other would cost both.
        let hint_h = text::line_height(l.small, FontWeightHint::Light);
        let (readings, controls) = if inner.h >= hint_h * 2.0 {
            (
                Rect::new(inner.x, inner.y, inner.w, inner.h - hint_h),
                Rect::new(inner.x, inner.bottom() - hint_h, inner.w, hint_h),
            )
        } else {
            (inner, Rect::EMPTY)
        };

        let mut row = readings;
        for (target, value, color) in self.readings() {
            let weight = if target == Target::Title {
                FontWeightHint::Bold
            } else {
                FontWeightHint::Regular
            };
            let width = text::measure(&value, l.head, weight);
            let r = take_left(&mut row, width, l.pad);
            if r.is_empty() {
                continue;
            }
            label_left(
                f,
                &Label {
                    text: &value,
                    size: l.head,
                    weight,
                    color,
                },
                r,
            );
            f.hit(target, r);
        }

        if !controls.is_empty() {
            label_left(
                f,
                &Label {
                    text: "Arrows: Move  Space: Shoot  P: Pause  N: New",
                    size: l.small,
                    weight: FontWeightHint::Light,
                    color: OVERLAY0,
                },
                controls,
            );
            f.hit(Target::Controls, controls);
        }
    }

    fn draw_particles(&self, f: &mut Frame<Target>, field: &Field) {
        for particle in &self.particles {
            let alpha = particle.alpha();
            if alpha == 0 {
                continue;
            }
            let c = particle.color;
            let size = field.scaled(2.0 + f32_from_u8(alpha) / 255.0 * 2.0);
            let (x, y) = field.to_screen(particle.pos);
            fill(
                f,
                Rect::new(x - size / 2.0, y - size / 2.0, size, size),
                Color::rgba(c.r, c.g, c.b, alpha),
                CornerRadii::all(size / 2.0),
            );
        }
    }

    fn draw_asteroids(&self, f: &mut Frame<Target>, field: &Field) {
        for (index, asteroid) in self.asteroids.iter().enumerate() {
            let verts = asteroid.vertices();
            let color = asteroid.size.color();
            let width = field.stroke(1.5);

            // Each vertex paired with the next, the last joined back to the
            // first. `zip` against the finite list is what ends it, so the
            // cycle cannot run away even for an asteroid with no vertices.
            for (v1, v2) in verts.iter().zip(verts.iter().cycle().skip(1)) {
                let (x1, y1) = field.to_screen(*v1);
                let (x2, y2) = field.to_screen(*v2);
                f.push(RenderCommand::Line {
                    x1,
                    y1,
                    x2,
                    y2,
                    color,
                    width,
                });
            }

            // A dot in the middle, so an asteroid whose outline crosses a
            // dark part of the field is still visible as a thing.
            let fill_r = field.scaled(asteroid.radius() * 0.3);
            let (cx, cy) = field.to_screen(asteroid.pos);
            fill(
                f,
                Rect::new(cx - fill_r, cy - fill_r, fill_r * 2.0, fill_r * 2.0),
                Color::rgba(color.r, color.g, color.b, 40),
                CornerRadii::all(fill_r),
            );

            let r = field.scaled(asteroid.radius());
            f.hit(
                Target::Asteroid(index),
                Rect::new(cx - r, cy - r, r * 2.0, r * 2.0),
            );
        }
    }

    fn draw_bullets(&self, f: &mut Frame<Target>, field: &Field) {
        let r = field.scaled(BULLET_RADIUS);
        for (index, bullet) in self.bullets.iter().enumerate() {
            let (x, y) = field.to_screen(bullet.pos);
            let box_ = Rect::new(x - r, y - r, r * 2.0, r * 2.0);
            fill(f, box_, GREEN, CornerRadii::all(r));
            f.hit(Target::Bullet(index), box_);
        }
    }

    /// The ship, if this is a frame it is showing on.
    ///
    /// A ship that has just respawned blinks, and on the frames it is not
    /// drawn it records no hit box either -- so asking whether the ship is on
    /// screen gets the answer the screen would give, not the answer the game
    /// state would.
    fn draw_ship(&self, f: &mut Frame<Target>, field: &Field) {
        if self.is_invulnerable() && self.frame_counter % 6 < 3 {
            return;
        }

        let nose = self.ship.nose();
        let lw = self.ship.left_wing();
        let rw = self.ship.right_wing();
        let width = field.stroke(2.0);

        for (a, b) in [(nose, lw), (lw, rw), (rw, nose)] {
            let (x1, y1) = field.to_screen(a);
            let (x2, y2) = field.to_screen(b);
            f.push(RenderCommand::Line {
                x1,
                y1,
                x2,
                y2,
                color: BLUE,
                width,
            });
        }

        if self.ship.thrusting {
            self.draw_thrust(f, field, lw, rw);
        }

        let r = field.scaled(SHIP_RADIUS);
        let (cx, cy) = field.to_screen(self.ship.pos);
        f.hit(Target::Ship, Rect::new(cx - r, cy - r, r * 2.0, r * 2.0));
    }

    fn draw_thrust(&self, f: &mut Frame<Target>, field: &Field, lw: Vec2, rw: Vec2) {
        let angle = self.ship.angle;
        let exhaust = self.ship.exhaust_point();
        let flame_tip = Vec2::new(
            self.ship.pos.x - cos_f32(angle) * SHIP_RADIUS * 1.2,
            self.ship.pos.y - sin_f32(angle) * SHIP_RADIUS * 1.2,
        );
        let flame_color = if self.frame_counter % 4 < 2 {
            PEACH
        } else {
            YELLOW
        };
        let outer = field.stroke(1.5);
        for wing in [lw, rw] {
            let (x1, y1) = field.to_screen(wing);
            let (x2, y2) = field.to_screen(flame_tip);
            f.push(RenderCommand::Line {
                x1,
                y1,
                x2,
                y2,
                color: flame_color,
                width: outer,
            });
        }

        let inner_tip = Vec2::new(
            exhaust.x - cos_f32(angle) * SHIP_RADIUS * 0.3,
            exhaust.y - sin_f32(angle) * SHIP_RADIUS * 0.3,
        );
        let inner = field.stroke(1.0);
        for spread in [1.0_f32, -1.0] {
            let from = Vec2::new(
                exhaust.x + cos_f32(angle + spread) * 3.0,
                exhaust.y + sin_f32(angle + spread) * 3.0,
            );
            let (x1, y1) = field.to_screen(from);
            let (x2, y2) = field.to_screen(inner_tip);
            f.push(RenderCommand::Line {
                x1,
                y1,
                x2,
                y2,
                color: YELLOW,
                width: inner,
            });
        }
    }

    fn draw_overlay(&self, f: &mut Frame<Target>, l: &Layout, field: &Field) {
        match self.state {
            GameState::Paused => self.draw_pause_overlay(f, l, field),
            GameState::GameOver => self.draw_game_over_overlay(f, l, field),
            GameState::Playing => {}
        }
    }

    fn draw_pause_overlay(&self, f: &mut Frame<Target>, l: &Layout, field: &Field) {
        if field.rect.is_empty() {
            return;
        }
        fill(
            f,
            field.rect,
            Color::rgba(17, 17, 27, 180),
            CornerRadii::ZERO,
        );
        f.hit(Target::Overlay, field.rect);

        let lines: [(Target, &str, f32, FontWeightHint, Color); 3] = [
            (
                Target::OverlayTitle,
                "PAUSED",
                l.font * 1.5,
                FontWeightHint::Bold,
                LAVENDER,
            ),
            (
                Target::Resume,
                "Press P or Esc to resume",
                l.font,
                FontWeightHint::Regular,
                SUBTEXT0,
            ),
            (
                Target::NewGame,
                "Press N for new game",
                l.font * 0.85,
                FontWeightHint::Regular,
                TEAL,
            ),
        ];
        stack_centred(f, field.rect, l.pad, &lines);
    }

    fn draw_game_over_overlay(&self, f: &mut Frame<Target>, l: &Layout, field: &Field) {
        if field.rect.is_empty() {
            return;
        }
        fill(
            f,
            field.rect,
            Color::rgba(17, 17, 27, 200),
            CornerRadii::ZERO,
        );
        f.hit(Target::Overlay, field.rect);

        // The box keeps its share of the world rather than a fixed 300x180,
        // which in a half-size window would have covered twice the field it
        // was drawn to cover.
        let box_w = field.scaled(300.0).min(field.rect.w);
        let box_h = field.scaled(200.0).min(field.rect.h);
        let box_ = Rect::new(
            field.rect.x + (field.rect.w - box_w) / 2.0,
            field.rect.y + (field.rect.h - box_h) / 2.0,
            box_w,
            box_h,
        );
        fill(f, box_, Color::from_hex(0x0031_3244), CornerRadii::all(8.0));
        stroke(f, box_, RED, field.stroke(2.0), CornerRadii::all(8.0));

        let lines: [(Target, String, f32, FontWeightHint, Color); 5] = [
            (
                Target::OverlayTitle,
                String::from("GAME OVER"),
                l.font * 1.5,
                FontWeightHint::Bold,
                RED,
            ),
            (
                Target::FinalStat(0),
                format!("Score: {}", self.score),
                l.font,
                FontWeightHint::Regular,
                TEXT_COLOR,
            ),
            (
                Target::FinalStat(1),
                format!("High Score: {}", self.high_score),
                l.font,
                FontWeightHint::Regular,
                YELLOW,
            ),
            (
                Target::FinalStat(2),
                format!("Wave reached: {}", self.wave),
                l.font,
                FontWeightHint::Regular,
                LAVENDER,
            ),
            (
                Target::NewGame,
                String::from("Press N or Enter for new game"),
                l.small,
                FontWeightHint::Regular,
                SUBTEXT0,
            ),
        ];
        let borrowed: Vec<(Target, &str, f32, FontWeightHint, Color)> = lines
            .iter()
            .map(|(t, s, size, w, c)| (*t, s.as_str(), *size, *w, *c))
            .collect();
        stack_centred(f, box_, l.pad / 2.0, &borrowed);
    }
}

// ── Drawing helpers ─────────────────────────────────────────────────

fn fill(f: &mut Frame<Target>, r: Rect, color: Color, corner_radii: CornerRadii) {
    if r.is_empty() {
        return;
    }
    f.push(RenderCommand::FillRect {
        x: r.x,
        y: r.y,
        width: r.w,
        height: r.h,
        color,
        corner_radii,
    });
}

fn stroke(
    f: &mut Frame<Target>,
    r: Rect,
    color: Color,
    line_width: f32,
    corner_radii: CornerRadii,
) {
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
        corner_radii,
    });
}

/// The starfield: forty dots from a fixed seed, so they do not crawl.
///
/// They are placed in field coordinates and put on the screen through the
/// same transform as everything else, so they scale with the field instead of
/// bunching into one corner of a window that grew.
fn draw_stars(f: &mut Frame<Target>, field: &Field) {
    let mut rng = SeededRng::new(999);
    let dim = Color::rgba(100, 100, 140, 60);
    let bright = Color::rgba(150, 150, 200, 100);
    for i in 0..40 {
        let at = Vec2::new(
            rng.between_f32(4.0, FIELD_WIDTH - 4.0),
            rng.between_f32(4.0, FIELD_HEIGHT - 4.0),
        );
        let big = i % 5 == 0;
        let size = field.scaled(if big { 2.0 } else { 1.5 });
        let (x, y) = field.to_screen(at);
        fill(
            f,
            Rect::new(x, y, size, size),
            if big { bright } else { dim },
            CornerRadii::all(size / 2.0),
        );
    }
}

/// One string and everything about how it looks, minus where it goes.
struct Label<'a> {
    text: &'a str,
    size: f32,
    weight: FontWeightHint,
    color: Color,
}

/// The one place a `Text` command is built.
fn push_text(f: &mut Frame<Target>, l: &Label, x: f32, y: f32, limit: f32) {
    if l.text.is_empty() || limit <= 0.0 {
        return;
    }
    f.push(RenderCommand::Text {
        x,
        y,
        text: l.text.to_string(),
        color: l.color,
        font_size: l.size,
        font_weight: l.weight,
        max_width: Some(limit),
        overflow: TextOverflow::Ellipsis,
    });
}

/// Against the left edge of `r`, centred down it.
fn label_left(f: &mut Frame<Target>, l: &Label, r: Rect) {
    if r.is_empty() {
        return;
    }
    let lh = text::line_height(l.size, l.weight);
    push_text(f, l, r.x, r.y + (r.h - lh) / 2.0, r.w);
}

/// Centred in `r`, and limited to it.
///
/// The width that decides the centre is the width the renderer is told to
/// stop at, so the two cannot disagree; and because it is never more than
/// `r.w`, the offset is never negative, which is what keeps a string too wide
/// for its box starting at the box rather than to the left of it.
fn label_centred(f: &mut Frame<Target>, l: &Label, r: Rect) {
    if r.is_empty() {
        return;
    }
    let w = text::measure(l.text, l.size, l.weight).min(r.w);
    let lh = text::line_height(l.size, l.weight);
    push_text(f, l, r.x + (r.w - w) / 2.0, r.y + (r.h - lh) / 2.0, r.w);
}

/// Stack labels down the middle of `area`, each with its own hit box.
///
/// The block is centred vertically as a block, so an overlay in a short
/// window loses as much off the bottom as off the top instead of running out
/// through the floor. A line with no room left is dropped, not drawn on top
/// of the one above it.
fn stack_centred(
    f: &mut Frame<Target>,
    area: Rect,
    gap: f32,
    lines: &[(Target, &str, f32, FontWeightHint, Color)],
) {
    if area.is_empty() || lines.is_empty() {
        return;
    }
    let heights: Vec<f32> = lines
        .iter()
        .map(|(_, _, size, weight, _)| text::line_height(*size, *weight))
        .collect();
    let total: f32 =
        heights.iter().sum::<f32>() + gap * f32_from_usize(lines.len().saturating_sub(1));
    let mut y = area.y + (area.h - total) / 2.0;

    for ((target, value, size, weight, color), lh) in lines.iter().zip(heights) {
        let row = Rect::new(area.x, y, area.w, lh);
        if row.bottom() > area.bottom() {
            break;
        }
        label_centred(
            f,
            &Label {
                text: value,
                size: *size,
                weight: *weight,
                color: *color,
            },
            row,
        );
        f.hit(*target, row);
        y += lh + gap;
    }
}

/// Take `w` off the left-hand end of `area`, leaving `gap` behind it.
///
/// Returns [`Rect::EMPTY`] and takes nothing if there is not room, so a row
/// that runs out of space drops its right-hand items rather than drawing them
/// past the end of the band.
fn take_left(area: &mut Rect, w: f32, gap: f32) -> Rect {
    if w <= 0.0 || area.w < w {
        return Rect::EMPTY;
    }
    let taken = Rect::new(area.x, area.y, w, area.h);
    area.x += w + gap;
    area.w = (area.w - w - gap).max(0.0);
    taken
}

// ── Casts, written out once ─────────────────────────────────────────

/// A count as a length. Counts here are vertex and line counts, well under
/// the 2^24 an `f32` counts exactly.
#[expect(
    clippy::cast_precision_loss,
    reason = "vertex and line counts are single digits"
)]
fn f32_from_usize(v: usize) -> f32 {
    v as f32
}

/// A window dimension as a length. Window sizes are pixel counts in the
/// thousands.
#[expect(
    clippy::cast_precision_loss,
    reason = "a window dimension is orders of magnitude below 2^24"
)]
fn f32_from_u32(v: u32) -> f32 {
    v as f32
}

/// An alpha as a fraction. Every `u8` is exact in an `f32`.
fn f32_from_u8(v: u8) -> f32 {
    f32::from(v)
}

/// A fraction of full opacity as an alpha.
///
/// The caller clamps to 0..=1 before multiplying by 255, so the value is in
/// range; the saturating cast makes that true whatever the caller does.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "an `as` cast from f32 to u8 saturates at both ends, which is the wanted behaviour"
)]
fn u8_from_f32(v: f32) -> u8 {
    v as u8
}

/// Milliseconds as seconds.
///
/// A tick interval is tens of milliseconds; the cast is written out so the
/// lint does not have to be turned off across the file to allow it.
#[expect(
    clippy::cast_precision_loss,
    reason = "a tick interval is tens of milliseconds"
)]
fn ms_to_seconds(ms: u64) -> f32 {
    ms as f32 / 1000.0
}

// ── Window plumbing ─────────────────────────────────────────────────

/// The one body both the window and the test probe drive, so what a click
/// does in a test is what it does on a screen.
pub fn handle_event(app: &mut AsteroidsApp, event: &Event) -> EventResult {
    match event {
        Event::Key(ev) => app.handle_key(ev),
        Event::Mouse(ev) => app.handle_mouse(ev),
        Event::Tick { elapsed_ms } => app.handle_tick(*elapsed_ms),
        Event::Resize { width, height } => {
            app.resize(f32_from_u32(*width), f32_from_u32(*height));
            EventResult::Consumed
        }
        _ => EventResult::Ignored,
    }
}

impl App for AsteroidsApp {
    fn title(&self) -> String {
        "Asteroids".to_string()
    }

    fn app_id(&self) -> String {
        "asteroids".to_string()
    }

    fn initial_size(&self) -> (u32, u32) {
        (INITIAL_WINDOW_W, INITIAL_WINDOW_H)
    }

    fn tick_interval(&self) -> Option<Duration> {
        Some(TICK)
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

impl Probe for AsteroidsApp {
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

fn main() -> ExitCode {
    let mut game = AsteroidsApp::new();
    app::launch("asteroids", &mut game)
}

// ═══════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════
#[cfg(test)]
mod tests {
    // A test that indexes out of range should fail loudly and point at the
    // line that did it -- that is the diagnosis. The defensive lints exist to
    // keep panics out of code that runs on a user's data, which this is not.
    #![allow(
        clippy::indexing_slicing,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::float_cmp,
        clippy::arithmetic_side_effects,
        clippy::cast_precision_loss
    )]

    use super::*;
    use guitk::probe;

    /// The size a test reads a click against, spelled once.
    const SIZE: (f32, f32) = (WINDOW_WIDTH, WINDOW_HEIGHT);

    /// Helper: create a game with a fixed seed for deterministic tests.
    fn test_app() -> AsteroidsApp {
        AsteroidsApp::with_seed(12345)
    }

    /// The commands the app draws at its default size.
    fn commands(app: &AsteroidsApp) -> Vec<RenderCommand> {
        app.frame(SIZE.0, SIZE.1).commands().to_vec()
    }

    /// Helper: advance the game by a given number of milliseconds.
    fn tick(app: &mut AsteroidsApp, ms: u64) {
        app.handle_tick(ms);
    }

    /// A key going down, and staying down.
    fn key_down(app: &mut AsteroidsApp, key: Key) -> EventResult {
        app.handle_key(&probe::press(key))
    }

    /// The same key coming back up.
    fn key_up(app: &mut AsteroidsApp, key: Key) -> EventResult {
        app.handle_key(&probe::release(key))
    }

    /// Helper: advance game by several small ticks totalling `total_ms`.
    fn tick_many(app: &mut AsteroidsApp, total_ms: u64, step_ms: u64) {
        let mut remaining = total_ms;
        while remaining > 0 {
            let step = if remaining >= step_ms {
                step_ms
            } else {
                remaining
            };
            tick(app, step);
            remaining -= step;
        }
    }

    /// Helper: press and release a key.
    fn press_key(app: &mut AsteroidsApp, key: Key) {
        key_down(app, key);
        key_up(app, key);
    }

    /// Helper: set up a scenario where ship is pointing right and a large
    /// asteroid is directly ahead at a known position.
    fn setup_target_practice() -> AsteroidsApp {
        let mut app = AsteroidsApp::with_seed(99);
        app.asteroids.clear();
        app.ship.pos = Vec2::new(100.0, 300.0);
        app.ship.vel = Vec2::ZERO;
        app.ship.angle = 0.0; // Pointing right.
        // Place a large asteroid directly ahead.
        app.asteroids.push(Asteroid::new(
            Vec2::new(200.0, 300.0),
            Vec2::ZERO,
            AsteroidSize::Large,
            &mut app.rng,
        ));
        app
    }

    // ── Construction & initialization ───────────────────────────────

    #[test]
    fn test_initial_ship_position() {
        let app = test_app();
        assert!((app.ship.pos.x - FIELD_WIDTH / 2.0).abs() < 0.01);
        assert!((app.ship.pos.y - FIELD_HEIGHT / 2.0).abs() < 0.01);
    }

    #[test]
    fn test_initial_ship_angle_points_up() {
        let app = test_app();
        // Ship starts pointing upward (-PI/2).
        assert!((app.ship.angle - (-PI / 2.0)).abs() < 0.01);
    }

    #[test]
    fn test_initial_state_is_playing() {
        let app = test_app();
        assert_eq!(app.state, GameState::Playing);
    }

    #[test]
    fn test_initial_score_is_zero() {
        let app = test_app();
        assert_eq!(app.score, 0);
    }

    #[test]
    fn test_initial_lives() {
        let app = test_app();
        assert_eq!(app.lives, INITIAL_LIVES);
    }

    #[test]
    fn test_initial_wave_is_one() {
        let app = test_app();
        assert_eq!(app.wave, 1);
    }

    #[test]
    fn test_initial_asteroids_count() {
        let app = test_app();
        assert_eq!(app.asteroid_count(), INITIAL_ASTEROIDS);
    }

    #[test]
    fn test_initial_asteroids_are_large() {
        let app = test_app();
        for asteroid in &app.asteroids {
            assert_eq!(asteroid.size, AsteroidSize::Large);
        }
    }

    #[test]
    fn test_initial_no_bullets() {
        let app = test_app();
        assert_eq!(app.bullet_count(), 0);
    }

    #[test]
    fn test_initial_ship_alive() {
        let app = test_app();
        assert!(app.ship_alive);
    }

    #[test]
    fn test_initial_invulnerable() {
        let app = test_app();
        assert!(app.is_invulnerable());
    }

    #[test]
    fn test_initial_no_particles() {
        let app = test_app();
        assert!(app.particles.is_empty());
    }

    // ── Vec2 ────────────────────────────────────────────────────────

    #[test]
    fn test_vec2_add() {
        let a = Vec2::new(1.0, 2.0);
        let b = Vec2::new(3.0, 4.0);
        let c = a.add(b);
        assert!((c.x - 4.0).abs() < 0.001);
        assert!((c.y - 6.0).abs() < 0.001);
    }

    #[test]
    fn test_vec2_sub() {
        let a = Vec2::new(5.0, 7.0);
        let b = Vec2::new(2.0, 3.0);
        let c = a.sub(b);
        assert!((c.x - 3.0).abs() < 0.001);
        assert!((c.y - 4.0).abs() < 0.001);
    }

    #[test]
    fn test_vec2_scale() {
        let v = Vec2::new(3.0, 4.0);
        let s = v.scale(2.0);
        assert!((s.x - 6.0).abs() < 0.001);
        assert!((s.y - 8.0).abs() < 0.001);
    }

    #[test]
    fn test_vec2_length() {
        let v = Vec2::new(3.0, 4.0);
        assert!((v.length() - 5.0).abs() < 0.001);
    }

    #[test]
    fn test_vec2_zero() {
        let v = Vec2::ZERO;
        assert!((v.x).abs() < 0.001);
        assert!((v.y).abs() < 0.001);
    }

    #[test]
    fn test_vec2_distance() {
        let a = Vec2::new(0.0, 0.0);
        let b = Vec2::new(3.0, 4.0);
        assert!((a.distance_to(b) - 5.0).abs() < 0.001);
    }

    #[test]
    fn test_vec2_clamp_length() {
        let v = Vec2::new(30.0, 40.0); // length 50
        let c = v.clamp_length(10.0);
        assert!((c.length() - 10.0).abs() < 0.01);
    }

    #[test]
    fn test_vec2_clamp_length_no_change() {
        let v = Vec2::new(3.0, 4.0); // length 5
        let c = v.clamp_length(10.0);
        assert!((c.x - 3.0).abs() < 0.001);
        assert!((c.y - 4.0).abs() < 0.001);
    }

    #[test]
    fn test_vec2_wrap_positive() {
        let v = Vec2::new(810.0, 610.0);
        let w = v.wrap(FIELD_WIDTH, FIELD_HEIGHT);
        assert!((w.x - 10.0).abs() < 0.01);
        assert!((w.y - 10.0).abs() < 0.01);
    }

    #[test]
    fn test_vec2_wrap_negative() {
        let v = Vec2::new(-10.0, -10.0);
        let w = v.wrap(FIELD_WIDTH, FIELD_HEIGHT);
        assert!((w.x - 790.0).abs() < 0.01);
        assert!((w.y - 590.0).abs() < 0.01);
    }

    #[test]
    fn test_vec2_wrap_in_bounds_unchanged() {
        let v = Vec2::new(100.0, 200.0);
        let w = v.wrap(FIELD_WIDTH, FIELD_HEIGHT);
        assert!((w.x - 100.0).abs() < 0.01);
        assert!((w.y - 200.0).abs() < 0.01);
    }

    #[test]
    fn test_vec2_wrapped_distance_simple() {
        let a = Vec2::new(100.0, 100.0);
        let b = Vec2::new(103.0, 104.0);
        let d = a.wrapped_distance(b, FIELD_WIDTH, FIELD_HEIGHT);
        assert!((d - 5.0).abs() < 0.01);
    }

    #[test]
    fn test_vec2_wrapped_distance_across_boundary() {
        let a = Vec2::new(10.0, 300.0);
        let b = Vec2::new(790.0, 300.0);
        // Direct distance = 780, wrapped = 800 - 780 = 20.
        let d = a.wrapped_distance(b, FIELD_WIDTH, FIELD_HEIGHT);
        assert!((d - 20.0).abs() < 0.01);
    }

    // ── Randomness ──────────────────────────────────────────────────
    //
    // Bounded, deterministic, in-range: those are properties of the generator,
    // and the generator is `randrange`'s, which tests all of them. The two
    // tests left here are about this game.

    /// A full turn, and nothing outside one.
    ///
    /// `random_angle` is the one piece of randomness this crate still owns, so
    /// it keeps its own test.
    #[test]
    fn random_angle_stays_within_one_turn() {
        let mut rng = SeededRng::new(42);
        for _ in 0..1000 {
            let a = random_angle(&mut rng);
            assert!((0.0..TAU).contains(&a), "angle {a} is outside [0, TAU)");
        }
    }

    /// Asteroids must not all enter from the same side of the screen.
    ///
    /// They used to. `random_edge_position` chose the edge with a bound of 4,
    /// reduced by `state % 4`, which on a modulus-2^64 LCG is a pure function
    /// of how many draws have been taken — the low two bits are a counter.
    /// Spawning one Large asteroid consumes exactly **16** draws (edge, one
    /// coordinate, heading, speed, ten vertex radii, spin angle, spin rate),
    /// and 16 is a multiple of 4, so every edge draw in a wave landed on the
    /// same two bits. Simulated over seeds 1, 2, 3, 42, 777 and 123456 before
    /// the fix: eight spawns each, and every one of the eight from one edge.
    /// The seed chose *which* edge and nothing else.
    ///
    /// The test infers the edge from the position rather than reading the
    /// draw, because the edge is what a player sees.
    ///
    /// It counts edges **within one seed**, which matters: pooling the edges
    /// across seeds passes against the broken generator, since the seed did
    /// choose the edge and twenty seeds cover all four. The defect is that one
    /// *game* only ever saw one edge, so one game is the unit to measure. With
    /// a working generator, eight spawns landing on a single edge has
    /// probability 4 × (1/4)^8 ≈ 1/16000, so requiring at least two edges from
    /// every one of twenty seeds is safe.
    #[test]
    fn asteroids_do_not_all_enter_from_one_edge() {
        for seed in 1..=20_u64 {
            let mut app = AsteroidsApp::with_seed(seed);
            app.asteroids.clear();
            app.spawn_wave(8);
            let mut edges = std::collections::BTreeSet::new();
            for a in &app.asteroids {
                let r = a.radius();
                let edge = if (a.pos.y - r).abs() < 1.0 {
                    "top"
                } else if (a.pos.y - (FIELD_HEIGHT - r)).abs() < 1.0 {
                    "bottom"
                } else if (a.pos.x - r).abs() < 1.0 {
                    "left"
                } else {
                    "right"
                };
                edges.insert(edge);
            }
            assert!(
                edges.len() > 1,
                "seed {seed}: all eight asteroids entered from {edges:?}"
            );
        }
    }

    // ── Asteroid size ───────────────────────────────────────────────

    #[test]
    fn test_asteroid_large_radius() {
        assert!((AsteroidSize::Large.radius() - ASTEROID_LARGE_RADIUS).abs() < 0.01);
    }

    #[test]
    fn test_asteroid_medium_radius() {
        assert!((AsteroidSize::Medium.radius() - ASTEROID_MEDIUM_RADIUS).abs() < 0.01);
    }

    #[test]
    fn test_asteroid_small_radius() {
        assert!((AsteroidSize::Small.radius() - ASTEROID_SMALL_RADIUS).abs() < 0.01);
    }

    #[test]
    fn test_asteroid_large_score() {
        assert_eq!(AsteroidSize::Large.score(), SCORE_LARGE);
    }

    #[test]
    fn test_asteroid_medium_score() {
        assert_eq!(AsteroidSize::Medium.score(), SCORE_MEDIUM);
    }

    #[test]
    fn test_asteroid_small_score() {
        assert_eq!(AsteroidSize::Small.score(), SCORE_SMALL);
    }

    #[test]
    fn test_asteroid_large_splits_to_medium() {
        assert_eq!(AsteroidSize::Large.child_size(), Some(AsteroidSize::Medium));
    }

    #[test]
    fn test_asteroid_medium_splits_to_small() {
        assert_eq!(AsteroidSize::Medium.child_size(), Some(AsteroidSize::Small));
    }

    #[test]
    fn test_asteroid_small_does_not_split() {
        assert_eq!(AsteroidSize::Small.child_size(), None);
    }

    // ── Ship ────────────────────────────────────────────────────────

    #[test]
    fn test_ship_new_center() {
        let ship = Ship::new();
        assert!((ship.pos.x - FIELD_WIDTH / 2.0).abs() < 0.01);
        assert!((ship.pos.y - FIELD_HEIGHT / 2.0).abs() < 0.01);
    }

    #[test]
    fn test_ship_reset() {
        let mut ship = Ship::new();
        ship.pos = Vec2::new(100.0, 100.0);
        ship.vel = Vec2::new(50.0, 50.0);
        ship.angle = 1.0;
        ship.reset();
        assert!((ship.pos.x - FIELD_WIDTH / 2.0).abs() < 0.01);
        assert!(ship.vel.length() < 0.01);
    }

    #[test]
    fn test_ship_rotation_left() {
        let mut ship = Ship::new();
        let initial = ship.angle;
        ship.rotate_left(0.1);
        // Angle should decrease (rotate counterclockwise).
        // After normalize, just check it changed.
        assert!((ship.angle - initial).abs() > 0.01);
    }

    #[test]
    fn test_ship_rotation_right() {
        let mut ship = Ship::new();
        let initial = ship.angle;
        ship.rotate_right(0.1);
        assert!((ship.angle - initial).abs() > 0.01);
    }

    #[test]
    fn test_ship_thrust_increases_speed() {
        let mut ship = Ship::new();
        ship.angle = 0.0; // Pointing right.
        ship.thrusting = true;
        let initial_speed = ship.vel.length();
        ship.update(0.016);
        assert!(ship.vel.length() > initial_speed);
    }

    #[test]
    fn test_ship_no_thrust_drag() {
        let mut ship = Ship::new();
        ship.vel = Vec2::new(100.0, 0.0);
        ship.thrusting = false;
        ship.update(0.016);
        // Velocity should decrease due to drag.
        assert!(ship.vel.x < 100.0);
    }

    #[test]
    fn test_ship_wraps_position() {
        let mut ship = Ship::new();
        ship.pos = Vec2::new(FIELD_WIDTH + 10.0, FIELD_HEIGHT + 10.0);
        ship.update(0.0);
        assert!(ship.pos.x < FIELD_WIDTH);
        assert!(ship.pos.y < FIELD_HEIGHT);
    }

    #[test]
    fn test_ship_triangle_vertices() {
        let ship = Ship::new();
        let nose = ship.nose();
        let lw = ship.left_wing();
        let rw = ship.right_wing();
        // All vertices should be within SHIP_RADIUS of center.
        assert!((nose.distance_to(ship.pos) - SHIP_RADIUS).abs() < 0.1);
        assert!((lw.distance_to(ship.pos) - SHIP_RADIUS).abs() < 0.1);
        assert!((rw.distance_to(ship.pos) - SHIP_RADIUS).abs() < 0.1);
    }

    // ── Bullet ──────────────────────────────────────────────────────

    #[test]
    fn test_bullet_creation() {
        let b = Bullet::new(Vec2::new(100.0, 200.0), Vec2::new(500.0, 0.0));
        assert!(b.alive());
        assert!((b.pos.x - 100.0).abs() < 0.01);
    }

    #[test]
    fn test_bullet_moves() {
        let mut b = Bullet::new(Vec2::new(100.0, 200.0), Vec2::new(500.0, 0.0));
        b.update(0.1);
        assert!(b.pos.x > 100.0);
    }

    #[test]
    fn test_bullet_expires() {
        let mut b = Bullet::new(Vec2::new(100.0, 200.0), Vec2::new(500.0, 0.0));
        b.update(BULLET_LIFETIME + 0.1);
        assert!(!b.alive());
    }

    #[test]
    fn test_bullet_wraps() {
        let mut b = Bullet::new(Vec2::new(FIELD_WIDTH - 1.0, 200.0), Vec2::new(500.0, 0.0));
        b.update(0.1);
        // Should wrap around.
        assert!(b.pos.x < FIELD_WIDTH);
    }

    // ── Asteroid ────────────────────────────────────────────────────

    #[test]
    fn test_asteroid_creation() {
        let mut rng = SeededRng::new(42);
        let a = Asteroid::new(
            Vec2::new(100.0, 100.0),
            Vec2::new(50.0, 0.0),
            AsteroidSize::Large,
            &mut rng,
        );
        assert_eq!(a.size, AsteroidSize::Large);
        assert_eq!(a.vertex_radii.len(), AsteroidSize::Large.vertex_count());
    }

    #[test]
    fn test_asteroid_moves() {
        let mut rng = SeededRng::new(42);
        let mut a = Asteroid::new(
            Vec2::new(100.0, 100.0),
            Vec2::new(50.0, 0.0),
            AsteroidSize::Large,
            &mut rng,
        );
        let old_x = a.pos.x;
        a.update(0.1);
        assert!(a.pos.x > old_x);
    }

    #[test]
    fn test_asteroid_wraps() {
        let mut rng = SeededRng::new(42);
        let mut a = Asteroid::new(
            Vec2::new(FIELD_WIDTH - 1.0, 100.0),
            Vec2::new(500.0, 0.0),
            AsteroidSize::Large,
            &mut rng,
        );
        a.update(0.1);
        assert!(a.pos.x < FIELD_WIDTH);
    }

    #[test]
    fn test_asteroid_rotates() {
        let mut rng = SeededRng::new(42);
        let mut a = Asteroid::new(
            Vec2::new(100.0, 100.0),
            Vec2::ZERO,
            AsteroidSize::Large,
            &mut rng,
        );
        let old_angle = a.angle;
        a.update(0.5);
        // If rotation_speed is non-zero the angle should change.
        if a.rotation_speed.abs() > 0.01 {
            assert!((a.angle - old_angle).abs() > 0.001);
        }
    }

    #[test]
    fn test_asteroid_vertices_count() {
        let mut rng = SeededRng::new(42);
        let a = Asteroid::new(
            Vec2::new(100.0, 100.0),
            Vec2::ZERO,
            AsteroidSize::Medium,
            &mut rng,
        );
        assert_eq!(a.vertices().len(), AsteroidSize::Medium.vertex_count());
    }

    // ── Particle ────────────────────────────────────────────────────

    #[test]
    fn test_particle_alive() {
        let p = Particle {
            pos: Vec2::ZERO,
            vel: Vec2::ZERO,
            lifetime: 1.0,
            max_lifetime: 1.0,
            color: RED,
        };
        assert!(p.alive());
    }

    #[test]
    fn test_particle_dies() {
        let mut p = Particle {
            pos: Vec2::ZERO,
            vel: Vec2::ZERO,
            lifetime: 0.5,
            max_lifetime: 1.0,
            color: RED,
        };
        p.update(0.6);
        assert!(!p.alive());
    }

    #[test]
    fn test_particle_alpha_full() {
        let p = Particle {
            pos: Vec2::ZERO,
            vel: Vec2::ZERO,
            lifetime: 1.0,
            max_lifetime: 1.0,
            color: RED,
        };
        assert_eq!(p.alpha(), 255);
    }

    #[test]
    fn test_particle_alpha_half() {
        let p = Particle {
            pos: Vec2::ZERO,
            vel: Vec2::ZERO,
            lifetime: 0.5,
            max_lifetime: 1.0,
            color: RED,
        };
        assert!((p.alpha() as i32 - 127).abs() <= 1);
    }

    // ── Shooting ────────────────────────────────────────────────────

    #[test]
    fn test_shooting_creates_bullet() {
        let mut app = test_app();
        app.input.shoot = true;
        tick(&mut app, 16);
        assert!(app.bullet_count() > 0);
    }

    #[test]
    fn test_shoot_cooldown_prevents_spam() {
        let mut app = test_app();
        app.input.shoot = true;
        tick(&mut app, 16);
        let count1 = app.bullet_count();
        // Tick again immediately -- cooldown should prevent new bullet.
        tick(&mut app, 16);
        let count2 = app.bullet_count();
        assert_eq!(count1, count2);
    }

    #[test]
    fn test_shoot_after_cooldown() {
        let mut app = test_app();
        app.input.shoot = true;
        tick(&mut app, 16);
        let count1 = app.bullet_count();
        // Wait for cooldown.
        tick(&mut app, (SHOOT_COOLDOWN * 1000.0) as u64 + 20);
        assert!(app.bullet_count() > count1);
    }

    #[test]
    fn test_max_bullets_enforced() {
        let mut app = test_app();
        app.input.shoot = true;
        // Fire many bullets with enough cooldown time.
        for _ in 0..(MAX_BULLETS + 5) {
            app.shoot_cooldown = 0.0; // Reset cooldown for testing.
            tick(&mut app, 16);
        }
        assert!(app.bullet_count() <= MAX_BULLETS);
    }

    #[test]
    fn test_bullet_fired_from_nose() {
        let mut app = test_app();
        app.asteroids.clear(); // Remove asteroids so bullet isn't consumed.
        let nose = app.ship.nose();
        app.input.shoot = true;
        tick(&mut app, 16);
        assert!(app.bullet_count() >= 1);
        // First bullet should be near the nose.
        let b = &app.bullets[0];
        assert!(b.pos.distance_to(nose) < 30.0);
    }

    // ── Collision ───────────────────────────────────────────────────

    #[test]
    fn test_bullet_destroys_asteroid() {
        let mut app = setup_target_practice();
        let initial_count = app.asteroid_count();
        app.input.shoot = true;
        // Fire and advance until bullet reaches asteroid.
        tick_many(&mut app, 500, 16);
        // Large asteroid splits into 2 medium, so count changes.
        assert_ne!(app.asteroid_count(), initial_count);
    }

    #[test]
    fn test_large_asteroid_splits_into_medium() {
        let mut app = setup_target_practice();
        assert_eq!(app.asteroids[0].size, AsteroidSize::Large);
        app.input.shoot = true;
        tick_many(&mut app, 500, 16);
        // After destruction, should have 2 medium asteroids.
        let medium_count = app
            .asteroids
            .iter()
            .filter(|a| a.size == AsteroidSize::Medium)
            .count();
        assert_eq!(medium_count, 2);
    }

    #[test]
    fn test_scoring_large_asteroid() {
        let mut app = setup_target_practice();
        assert_eq!(app.score, 0);
        app.input.shoot = true;
        tick_many(&mut app, 500, 16);
        assert_eq!(app.score, SCORE_LARGE);
    }

    #[test]
    fn test_ship_collision_loses_life() {
        let mut app = test_app();
        app.invulnerable_timer = 0.0; // Remove initial invulnerability.
        app.asteroids.clear();
        // Place asteroid right on top of ship.
        app.asteroids.push(Asteroid::new(
            app.ship.pos,
            Vec2::ZERO,
            AsteroidSize::Large,
            &mut app.rng,
        ));
        let old_lives = app.lives;
        tick(&mut app, 16);
        assert!(app.lives < old_lives);
    }

    #[test]
    fn test_ship_invulnerable_no_collision() {
        let mut app = test_app();
        // Start with invulnerability.
        assert!(app.is_invulnerable());
        app.asteroids.clear();
        app.asteroids.push(Asteroid::new(
            app.ship.pos,
            Vec2::ZERO,
            AsteroidSize::Large,
            &mut app.rng,
        ));
        let old_lives = app.lives;
        tick(&mut app, 16);
        assert_eq!(app.lives, old_lives);
    }

    #[test]
    fn test_game_over_on_zero_lives() {
        let mut app = test_app();
        app.invulnerable_timer = 0.0;
        app.lives = 1;
        app.asteroids.clear();
        app.asteroids.push(Asteroid::new(
            app.ship.pos,
            Vec2::ZERO,
            AsteroidSize::Large,
            &mut app.rng,
        ));
        tick(&mut app, 16);
        assert_eq!(app.state, GameState::GameOver);
    }

    #[test]
    fn test_ship_respawns_after_death() {
        let mut app = test_app();
        app.invulnerable_timer = 0.0;
        app.lives = 3;
        app.asteroids.clear();
        app.asteroids.push(Asteroid::new(
            app.ship.pos,
            Vec2::ZERO,
            AsteroidSize::Large,
            &mut app.rng,
        ));
        tick(&mut app, 16);
        assert!(!app.ship_alive);
        // Advance past respawn delay.
        tick_many(&mut app, (RESPAWN_DELAY * 1000.0) as u64 + 100, 16);
        assert!(app.ship_alive);
    }

    #[test]
    fn test_respawn_grants_invulnerability() {
        let mut app = test_app();
        app.invulnerable_timer = 0.0;
        app.lives = 3;
        app.asteroids.clear();
        // Place asteroid away from center so it doesn't collide after respawn.
        app.asteroids.push(Asteroid::new(
            app.ship.pos,
            Vec2::new(0.0, -200.0),
            AsteroidSize::Large,
            &mut app.rng,
        ));
        tick(&mut app, 16);
        assert!(!app.ship_alive);
        tick_many(&mut app, (RESPAWN_DELAY * 1000.0) as u64 + 100, 16);
        assert!(app.ship_alive);
        assert!(app.is_invulnerable());
    }

    // ── Waves ───────────────────────────────────────────────────────

    #[test]
    fn test_wave_advance_on_clear() {
        let mut app = test_app();
        assert_eq!(app.wave, 1);
        app.asteroids.clear();
        tick(&mut app, 16);
        assert_eq!(app.wave, 2);
    }

    #[test]
    fn test_wave_two_has_more_asteroids() {
        let mut app = test_app();
        app.asteroids.clear();
        tick(&mut app, 16);
        // Wave 2 should have INITIAL_ASTEROIDS + 1 large asteroids.
        let large_count = app
            .asteroids
            .iter()
            .filter(|a| a.size == AsteroidSize::Large)
            .count();
        assert_eq!(large_count, INITIAL_ASTEROIDS + 1);
    }

    #[test]
    fn test_wave_counter_increments() {
        let mut app = test_app();
        app.asteroids.clear();
        tick(&mut app, 16);
        assert_eq!(app.wave, 2);
        app.asteroids.clear();
        tick(&mut app, 16);
        assert_eq!(app.wave, 3);
    }

    // ── Input handling ──────────────────────────────────────────────

    #[test]
    fn test_left_key_sets_input() {
        let mut app = test_app();
        key_down(&mut app, Key::Left);
        assert!(app.input.left);
        key_up(&mut app, Key::Left);
        assert!(!app.input.left);
    }

    #[test]
    fn test_right_key_sets_input() {
        let mut app = test_app();
        key_down(&mut app, Key::Right);
        assert!(app.input.right);
    }

    #[test]
    fn test_up_key_sets_thrust() {
        let mut app = test_app();
        key_down(&mut app, Key::Up);
        assert!(app.input.thrust);
    }

    #[test]
    fn test_space_key_sets_shoot() {
        let mut app = test_app();
        key_down(&mut app, Key::Space);
        assert!(app.input.shoot);
    }

    #[test]
    fn test_pause_key() {
        let mut app = test_app();
        press_key(&mut app, Key::P);
        assert_eq!(app.state, GameState::Paused);
    }

    #[test]
    fn test_unpause_key() {
        let mut app = test_app();
        press_key(&mut app, Key::P);
        assert_eq!(app.state, GameState::Paused);
        press_key(&mut app, Key::P);
        assert_eq!(app.state, GameState::Playing);
    }

    #[test]
    fn test_escape_pauses() {
        let mut app = test_app();
        press_key(&mut app, Key::Escape);
        assert_eq!(app.state, GameState::Paused);
    }

    #[test]
    fn test_new_game_key() {
        let mut app = test_app();
        app.score = 500;
        press_key(&mut app, Key::N);
        assert_eq!(app.score, 0);
        assert_eq!(app.lives, INITIAL_LIVES);
    }

    #[test]
    fn test_new_game_preserves_high_score() {
        let mut app = test_app();
        app.score = 500;
        app.high_score = 500;
        press_key(&mut app, Key::N);
        assert_eq!(app.high_score, 500);
    }

    #[test]
    fn test_pause_releases_input() {
        let mut app = test_app();
        key_down(&mut app, Key::Left);
        key_down(&mut app, Key::Up);
        assert!(app.input.left);
        assert!(app.input.thrust);
        press_key(&mut app, Key::P);
        assert!(!app.input.left);
        assert!(!app.input.thrust);
    }

    #[test]
    fn test_game_over_enter_restarts() {
        let mut app = test_app();
        app.state = GameState::GameOver;
        press_key(&mut app, Key::Enter);
        assert_eq!(app.state, GameState::Playing);
    }

    #[test]
    fn test_game_over_n_restarts() {
        let mut app = test_app();
        app.state = GameState::GameOver;
        press_key(&mut app, Key::N);
        assert_eq!(app.state, GameState::Playing);
    }

    #[test]
    fn test_paused_no_tick_update() {
        let mut app = test_app();
        app.state = GameState::Paused;
        let old_pos = app.ship.pos;
        app.ship.vel = Vec2::new(100.0, 0.0);
        tick(&mut app, 100);
        // Ship should not move while paused.
        assert!((app.ship.pos.x - old_pos.x).abs() < 0.01);
    }

    // ── Rendering ───────────────────────────────────────────────────

    #[test]
    fn test_render_produces_commands() {
        let app = test_app();
        let cmds = commands(&app);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_contains_background() {
        let app = test_app();
        let cmds = commands(&app);
        // First command should be the background fill.
        matches!(&cmds[0], RenderCommand::FillRect { .. });
    }

    #[test]
    fn test_render_paused_overlay() {
        let mut app = test_app();
        app.state = GameState::Paused;
        let cmds = commands(&app);
        let has_paused_text = cmds.iter().any(|c| {
            if let RenderCommand::Text { text, .. } = c {
                text == "PAUSED"
            } else {
                false
            }
        });
        assert!(has_paused_text);
    }

    #[test]
    fn test_render_game_over_overlay() {
        let mut app = test_app();
        app.state = GameState::GameOver;
        let cmds = commands(&app);
        let has_game_over = cmds.iter().any(|c| {
            if let RenderCommand::Text { text, .. } = c {
                text == "GAME OVER"
            } else {
                false
            }
        });
        assert!(has_game_over);
    }

    #[test]
    fn test_render_ship_lines() {
        let app = test_app();
        // Make sure ship is visible (not blinking off).
        let cmds = commands(&app);
        let line_count = cmds
            .iter()
            .filter(|c| matches!(c, RenderCommand::Line { .. }))
            .count();
        // Should have at least the 3 ship triangle lines.
        assert!(line_count >= 3);
    }

    #[test]
    fn test_render_asteroid_lines() {
        let app = test_app();
        let cmds = commands(&app);
        let line_count = cmds
            .iter()
            .filter(|c| matches!(c, RenderCommand::Line { .. }))
            .count();
        // Each asteroid has vertex_count lines, with INITIAL_ASTEROIDS asteroids
        // each having 10 vertices (large), that's at least 40 lines for asteroids.
        assert!(line_count >= INITIAL_ASTEROIDS * 10);
    }

    #[test]
    fn test_render_with_bullets() {
        let mut app = test_app();
        app.asteroids.clear();
        app.input.shoot = true;
        tick(&mut app, 16);
        let cmds = commands(&app);
        // Should have bullet fill rects.
        let fill_count = cmds
            .iter()
            .filter(|c| matches!(c, RenderCommand::FillRect { .. }))
            .count();
        assert!(fill_count > 2); // Background + field + bullet + stars.
    }

    #[test]
    fn test_render_header_shows_score() {
        let mut app = test_app();
        app.score = 42;
        let cmds = commands(&app);
        let has_score = cmds.iter().any(|c| {
            if let RenderCommand::Text { text, .. } = c {
                text.contains("42")
            } else {
                false
            }
        });
        assert!(has_score);
    }

    // ── Normalize angle ─────────────────────────────────────────────

    #[test]
    fn test_normalize_angle_positive() {
        let a = normalize_angle(TAU + 1.0);
        assert!((0.0..TAU).contains(&a));
        assert!((a - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_normalize_angle_negative() {
        let a = normalize_angle(-1.0);
        assert!((0.0..TAU).contains(&a));
        assert!((a - (TAU - 1.0)).abs() < 0.001);
    }

    #[test]
    fn test_normalize_angle_zero() {
        let a = normalize_angle(0.0);
        assert!((a).abs() < 0.001);
    }

    // ── High score tracking ─────────────────────────────────────────

    #[test]
    fn test_high_score_updates_on_scoring() {
        let mut app = setup_target_practice();
        assert_eq!(app.high_score, 0);
        app.input.shoot = true;
        tick_many(&mut app, 500, 16);
        assert!(app.high_score >= SCORE_LARGE);
    }

    #[test]
    fn test_high_score_persists_across_new_game() {
        let mut app = setup_target_practice();
        app.input.shoot = true;
        tick_many(&mut app, 500, 16);
        let high = app.high_score;
        app.new_game();
        assert_eq!(app.high_score, high);
    }

    // ── Explosion particles ─────────────────────────────────────────

    #[test]
    fn test_explosion_spawns_particles() {
        let mut app = test_app();
        assert!(app.particles.is_empty());
        app.spawn_explosion(Vec2::new(100.0, 100.0), 10, RED);
        assert_eq!(app.particles.len(), 10);
    }

    #[test]
    fn test_particles_decay() {
        let mut app = test_app();
        app.spawn_explosion(Vec2::new(100.0, 100.0), 5, RED);
        // Advance a long time so all particles expire.
        tick_many(&mut app, 2000, 16);
        assert!(app.particles.is_empty());
    }

    #[test]
    fn test_thrust_particle_spawned() {
        let mut app = test_app();
        app.ship.thrusting = true;
        app.spawn_thrust_particle();
        assert!(!app.particles.is_empty());
    }

    // ── Asteroid update and movement ────────────────────────────────

    #[test]
    fn test_asteroids_move_on_tick() {
        let mut app = test_app();
        let old_pos = app.asteroids[0].pos;
        tick(&mut app, 100);
        // At least one asteroid should have moved.
        let new_pos = app.asteroids[0].pos;
        let moved = (new_pos.x - old_pos.x).abs() > 0.01 || (new_pos.y - old_pos.y).abs() > 0.01;
        assert!(moved);
    }

    // ── Asteroid spawning at edges ──────────────────────────────────

    #[test]
    fn test_asteroids_spawn_away_from_ship() {
        let app = test_app();
        for asteroid in &app.asteroids {
            let dist = asteroid
                .pos
                .wrapped_distance(app.ship.pos, FIELD_WIDTH, FIELD_HEIGHT);
            assert!(dist > SAFE_SPAWN_DISTANCE - ASTEROID_LARGE_RADIUS);
        }
    }

    // ── Small asteroid destruction (no children) ────────────────────

    #[test]
    fn test_small_asteroid_destruction_no_children() {
        let mut app = AsteroidsApp::with_seed(99);
        app.asteroids.clear();
        app.ship.pos = Vec2::new(100.0, 300.0);
        app.ship.vel = Vec2::ZERO;
        app.ship.angle = 0.0;
        app.asteroids.push(Asteroid::new(
            Vec2::new(200.0, 300.0),
            Vec2::ZERO,
            AsteroidSize::Small,
            &mut app.rng,
        ));
        app.input.shoot = true;
        tick_many(&mut app, 500, 16);
        // Small asteroid should be destroyed with no children spawned
        // (only wave-spawned asteroids should remain).
        let small_count = app
            .asteroids
            .iter()
            .filter(|a| a.size == AsteroidSize::Small)
            .count();
        // If the small asteroid was hit, there should be no small children.
        // The wave may have advanced and spawned large ones.
        assert_eq!(small_count, 0);
    }

    // ── Event dispatch ──────────────────────────────────────────────

    #[test]
    fn test_handle_event_key_down() {
        let mut app = test_app();
        handle_event(&mut app, &Event::Key(probe::press(Key::Left)));
        assert!(app.input.left);
    }

    #[test]
    fn test_handle_event_key_up() {
        let mut app = test_app();
        handle_event(&mut app, &Event::Key(probe::press(Key::Left)));
        assert!(app.input.left);
        handle_event(&mut app, &Event::Key(probe::release(Key::Left)));
        assert!(!app.input.left);
    }

    #[test]
    fn test_handle_event_tick() {
        let mut app = test_app();
        let old_pos = app.asteroids[0].pos;
        handle_event(&mut app, &Event::Tick { elapsed_ms: 100 });
        let new_pos = app.asteroids[0].pos;
        let moved = (new_pos.x - old_pos.x).abs() > 0.01 || (new_pos.y - old_pos.y).abs() > 0.01;
        assert!(moved);
    }

    // ── WASD alternative controls ───────────────────────────────────

    #[test]
    fn test_wasd_a_key() {
        let mut app = test_app();
        key_down(&mut app, Key::A);
        assert!(app.input.left);
    }

    #[test]
    fn test_wasd_d_key() {
        let mut app = test_app();
        key_down(&mut app, Key::D);
        assert!(app.input.right);
    }

    #[test]
    fn test_wasd_w_key() {
        let mut app = test_app();
        key_down(&mut app, Key::W);
        assert!(app.input.thrust);
    }

    // ── Speed clamping ──────────────────────────────────────────────

    #[test]
    fn test_ship_max_speed_clamped() {
        let mut app = test_app();
        app.ship.vel = Vec2::new(MAX_SPEED * 2.0, 0.0);
        app.ship.thrusting = true;
        app.ship.angle = 0.0;
        tick(&mut app, 16);
        assert!(app.ship.vel.length() <= MAX_SPEED + 1.0);
    }

    // ── Destroy ship with no lives left ─────────────────────────────

    #[test]
    fn test_high_score_updated_on_game_over() {
        let mut app = test_app();
        app.score = 1000;
        app.high_score = 500;
        app.invulnerable_timer = 0.0;
        app.lives = 1;
        app.asteroids.clear();
        app.asteroids.push(Asteroid::new(
            app.ship.pos,
            Vec2::ZERO,
            AsteroidSize::Large,
            &mut app.rng,
        ));
        tick(&mut app, 16);
        assert_eq!(app.state, GameState::GameOver);
        assert_eq!(app.high_score, 1000);
    }

    // ═══════════════════════════════════════════════════════════════
    // Window wiring
    //
    // The program this replaces drew into a `Vec<RenderCommand>` nobody
    // displayed, at a size it worked out for itself. Everything below asks
    // the frame where things are rather than asking the game state, because
    // the frame is what a player sees and the state is not.
    // ═══════════════════════════════════════════════════════════════

    /// Every string the app draws at a given size.
    fn texts(app: &AsteroidsApp, size: (f32, f32)) -> Vec<String> {
        app.frame(size.0, size.1)
            .commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    /// Do two rectangles share any area?
    fn overlaps(a: Rect, b: Rect) -> bool {
        a.intersect(b).is_some_and(|r| r.w > 0.01 && r.h > 0.01)
    }

    /// Is `inner` wholly within `outer`, give or take a rounding error?
    fn inside(inner: Rect, outer: Rect) -> bool {
        inner.x >= outer.x - 0.01
            && inner.y >= outer.y - 0.01
            && inner.right() <= outer.right() + 0.01
            && inner.bottom() <= outer.bottom() + 0.01
    }

    /// A game whose ship is not blinking, so it is on screen to be found.
    fn app_with_a_settled_ship() -> AsteroidsApp {
        let mut app = test_app();
        app.invulnerable_timer = 0.0;
        app
    }

    // ── Layout ──────────────────────────────────────────────────────

    #[test]
    fn the_bands_do_not_overlap_and_stay_inside_the_window() {
        for (w, h) in [
            (824.0, 674.0),
            (400.0, 300.0),
            (1600.0, 1200.0),
            (300.0, 900.0),
            // Shorter than the header's own floor. The header's height is a
            // share of `h` clamped up to at least 20, so below 20 the clamp
            // is trying to make the band *taller* than the window it is in,
            // and only the `.min(h)` behind it stops the header hanging out
            // of the bottom. Every size above is 300 or more, so none of them
            // reaches that: dropping the `.min(h)` left this test green.
            (824.0, 6.0),
            (40.0, 12.0),
        ] {
            let l = Layout::new(w, h);
            assert!(
                inside(l.header, l.window),
                "the header left the window at {w}x{h}"
            );
            assert!(
                inside(l.body, l.window),
                "the body left the window at {w}x{h}"
            );
            assert!(
                !overlaps(l.header, l.body),
                "the bands overlapped at {w}x{h}"
            );
            assert!(
                l.body.y >= l.header.bottom(),
                "the body started above the header at {w}x{h}"
            );
        }
    }

    #[test]
    fn no_band_is_ever_drawn_inside_out() {
        // A rectangle of negative height draws from its bottom edge upwards,
        // which is a band in the wrong place rather than a band that is
        // missing -- much harder to notice, so it is asserted rather than
        // trusted.
        for (w, h) in [(0.0, 0.0), (10.0, 4.0), (824.0, 6.0), (2.0, 674.0)] {
            let l = Layout::new(w, h);
            for (name, r) in [("window", l.window), ("header", l.header), ("body", l.body)] {
                assert!(r.w >= 0.0, "{name} had a negative width at {w}x{h}");
                assert!(r.h >= 0.0, "{name} had a negative height at {w}x{h}");
            }
        }
    }

    #[test]
    fn a_negative_window_is_read_as_no_window() {
        let l = Layout::new(-100.0, -50.0);
        assert_eq!(l.window.w, 0.0);
        assert_eq!(l.window.h, 0.0);
    }

    #[test]
    fn a_taller_window_gives_the_playfield_the_extra_room() {
        let short = Layout::new(824.0, 500.0);
        let tall = Layout::new(824.0, 900.0);
        assert!(
            tall.body.h > short.body.h,
            "400 more pixels of window and the playfield got none of them"
        );
    }

    // ── The field ───────────────────────────────────────────────────

    #[test]
    fn the_field_keeps_the_worlds_proportions_whatever_the_window() {
        let want = FIELD_WIDTH / FIELD_HEIGHT;
        for (w, h) in [
            (824.0, 674.0),
            (2000.0, 700.0),
            (500.0, 1400.0),
            (400.0, 320.0),
        ] {
            let field = Field::new(Layout::new(w, h).body);
            assert!(field.rect.h > 0.0, "no field at all at {w}x{h}");
            let got = field.rect.w / field.rect.h;
            assert!(
                (got - want).abs() < 0.001,
                "the world was stretched at {w}x{h}: {got} against {want}"
            );
        }
    }

    #[test]
    fn a_wide_window_letterboxes_the_field_rather_than_stretching_it() {
        let area = Layout::new(2000.0, 700.0).body;
        let field = Field::new(area);
        assert!(
            inside(field.rect, area),
            "the field ran out of the space it was given"
        );
        assert!(
            field.rect.w < area.w - 1.0,
            "a window twice as wide as the world left no margin, so the field was stretched"
        );
        // Centred: the two margins are equal.
        let left = field.rect.x - area.x;
        let right = area.right() - field.rect.right();
        assert!(
            (left - right).abs() < 0.01,
            "the field was not centred: {left} against {right}"
        );
    }

    #[test]
    fn a_tall_window_letterboxes_the_field_above_and_below() {
        let area = Layout::new(500.0, 1400.0).body;
        let field = Field::new(area);
        assert!(inside(field.rect, area));
        assert!(
            field.rect.h < area.h - 1.0,
            "no margin in a window far taller than the world"
        );
        let top = field.rect.y - area.y;
        let bottom = area.bottom() - field.rect.bottom();
        assert!(
            (top - bottom).abs() < 0.01,
            "the field was not centred: {top} against {bottom}"
        );
    }

    #[test]
    fn the_corners_of_the_world_land_on_the_corners_of_the_field() {
        let field = Field::new(Layout::new(824.0, 674.0).body);
        let (x0, y0) = field.to_screen(Vec2::ZERO);
        let (x1, y1) = field.to_screen(Vec2::new(FIELD_WIDTH, FIELD_HEIGHT));
        assert!((x0 - field.rect.x).abs() < 0.01);
        assert!((y0 - field.rect.y).abs() < 0.01);
        assert!((x1 - field.rect.right()).abs() < 0.01);
        assert!((y1 - field.rect.bottom()).abs() < 0.01);
    }

    #[test]
    fn a_bigger_window_draws_the_same_game_bigger() {
        let small = Field::new(Layout::new(500.0, 420.0).body);
        let big = Field::new(Layout::new(1600.0, 1300.0).body);
        assert!(
            big.scale > small.scale,
            "the field did not grow with the window"
        );
        // The same point in the world is further from the field's own corner.
        let mid = Vec2::new(FIELD_WIDTH / 2.0, FIELD_HEIGHT / 2.0);
        let (sx, _) = small.to_screen(mid);
        let (bx, _) = big.to_screen(mid);
        assert!(bx - big.rect.x > sx - small.rect.x);
    }

    #[test]
    fn a_window_with_no_room_has_a_field_of_nothing_rather_than_a_backwards_one() {
        // Through the layout first, which is how the game reaches it.
        let field = Field::new(Layout::new(20.0, 8.0).body);
        assert!(field.scale >= 0.0);
        assert!(field.rect.w >= 0.0 && field.rect.h >= 0.0);

        // And then directly, with the shape the clamp in `Field::new` is
        // actually for. `Layout` already clamps its bands to a non-negative
        // size, so no window -- however small -- can hand `Field::new` a
        // backwards rectangle, and a test that only goes through `Layout`
        // cannot reach the `.max(0.0)` at all: deleting it left the case
        // above green. `Field::new` is public and documents no precondition,
        // so the shape is reachable by a caller even though it is not
        // reachable by the game, and the clamp is what keeps such a caller
        // from getting a field drawn inside out rather than an empty one.
        let backwards = Field::new(Rect::new(10.0, 10.0, -400.0, -300.0));
        assert!(
            backwards.scale >= 0.0,
            "a backwards area gave the field a negative scale"
        );
        assert!(
            backwards.rect.w >= 0.0 && backwards.rect.h >= 0.0,
            "a backwards area gave the field a backwards rectangle"
        );
    }

    #[test]
    fn a_line_never_thins_away_to_nothing_in_a_small_window() {
        // A hairline that rounds to zero is a ship that vanishes, which reads
        // as the game losing the ship rather than the window being small.
        let field = Field::new(Layout::new(120.0, 100.0).body);
        assert!(
            field.scale < 1.0,
            "the window was not small enough to be a test"
        );
        assert!(field.stroke(1.0) >= 1.0);
        assert!(field.stroke(0.0) >= 1.0);
    }

    // ── The header ──────────────────────────────────────────────────

    #[test]
    fn the_header_names_every_reading() {
        let app = test_app();
        for target in [
            Target::Title,
            Target::Score,
            Target::HighScore,
            Target::Lives,
            Target::Wave,
            Target::Controls,
        ] {
            assert!(
                probe::is_visible(&app, target),
                "{target:?} was not on screen"
            );
        }
    }

    #[test]
    fn the_readings_do_not_overlap_each_other() {
        let mut app = test_app();
        // Numbers wide enough that a fixed-offset layout would collide.
        app.score = 1_234_567;
        app.high_score = 9_876_543;
        app.lives = 1_000_000;
        app.wave = 999_999;
        let frame = app.frame(SIZE.0, SIZE.1);
        // The controls line is in the list too. It is not a reading, but it
        // shares the header with them and is laid out by a different rule --
        // the readings walk left to right across the top strip, the controls
        // line takes the bottom one -- so the two rules can disagree about
        // where the boundary is. Giving the readings the *whole* inner
        // rectangle instead of the strip above the controls line left this
        // test green while the two rows sat on top of each other.
        let boxes: Vec<Rect> = [
            Target::Title,
            Target::Score,
            Target::HighScore,
            Target::Lives,
            Target::Wave,
            Target::Controls,
        ]
        .into_iter()
        .filter_map(|t| frame.rect_of(|c| *c == t))
        .collect();
        assert!(boxes.len() >= 2, "not enough readings drawn to be a test");
        for (i, a) in boxes.iter().enumerate() {
            for b in boxes.iter().skip(i + 1) {
                assert!(
                    !overlaps(*a, *b),
                    "two readings shared space: {a:?} and {b:?}"
                );
            }
        }

        // Not overlapping is only half of it: five boxes of a fixed width laid
        // end to end do not overlap either, and that is exactly what the
        // hardcoded-offset layout this replaced amounted to. What makes the
        // layout right is that each box is as wide as the thing written in it,
        // so a six-digit score is drawn rather than cut short with an
        // ellipsis. Measured against the same call the drawing pass uses.
        let l = Layout::new(SIZE.0, SIZE.1);
        for (target, value, _) in app.readings() {
            let Some(r) = frame.rect_of(|c| *c == target) else {
                continue;
            };
            let weight = if target == Target::Title {
                FontWeightHint::Bold
            } else {
                FontWeightHint::Regular
            };
            let needed = text::measure(&value, l.head, weight);
            assert!(
                r.w >= needed,
                "{target:?} was given {}px for {value:?}, which needs {needed}px",
                r.w
            );
        }
    }

    #[test]
    fn the_score_on_screen_is_the_score() {
        let mut app = test_app();
        app.score = 4242;
        let shown = texts(&app, SIZE);
        assert!(
            shown.iter().any(|t| t == "Score: 4242"),
            "the header did not say the score: {shown:?}"
        );
    }

    #[test]
    fn a_narrow_window_drops_readings_rather_than_drawing_them_over_each_other() {
        let app = test_app();
        let narrow = (240.0, 674.0);
        let frame = app.frame(narrow.0, narrow.1);
        let boxes: Vec<Rect> = [
            Target::Title,
            Target::Score,
            Target::HighScore,
            Target::Lives,
            Target::Wave,
        ]
        .into_iter()
        .filter_map(|t| frame.rect_of(|c| *c == t))
        .collect();
        assert!(
            boxes.len() < 5,
            "all five readings claimed to fit in a 240-pixel window"
        );
        for r in &boxes {
            assert!(
                inside(*r, Layout::new(narrow.0, narrow.1).header),
                "a reading was drawn past the end of the header: {r:?}"
            );
        }
    }

    #[test]
    fn a_header_with_room_for_one_row_keeps_the_score_and_drops_the_controls_line() {
        // 120 pixels of window: the header band is 20 tall, which one row of
        // type fills. The score is worth more than a reminder of which key
        // turns left, so it is the hint that goes.
        let app = test_app();
        let size = (824.0, 120.0);
        assert!(
            probe::is_visible_sized(&app, Target::Score, size),
            "the score went before the hint did"
        );
        assert!(
            !probe::is_visible_sized(&app, Target::Controls, size),
            "both rows claimed to fit in a 20-pixel band"
        );
    }

    /// A fixed width cannot pass this, whatever the fixed width is.
    ///
    /// Asserting `box >= measured text` is the right property but it is a
    /// *threshold*, and a threshold can be cleared by a constant that happens
    /// to be large enough: 130px survived that assertion because the values it
    /// was tried against need about 130px. Two scores of visibly different
    /// length compared against each other have no such loophole -- a layout
    /// that ignores its content gives them the same box, and same is not
    /// bigger. (`known-issues.md` lesson 75's shape: a witness has to move
    /// further than the slack in the thing measuring it.)
    #[test]
    fn a_longer_reading_is_given_a_wider_box() {
        let mut small = test_app();
        small.score = 7;
        let mut large = test_app();
        large.score = 1_234_567_890;

        let narrow = probe::rect_of(&small, Target::Score).expect("the score is drawn");
        let wide = probe::rect_of(&large, Target::Score).expect("the score is drawn");
        assert!(
            wide.w > narrow.w,
            "a ten-digit score got the same {}px box as a one-digit one",
            narrow.w
        );
    }

    #[test]
    fn a_reading_that_will_not_fit_is_not_drawn_at_all() {
        // Not merely clipped: a hit box for a control nobody can see is a
        // click that lands on nothing.
        let mut app = test_app();
        app.wave = 4_000_000;
        let frame = app.frame(200.0, 674.0);
        if let Some(r) = frame.rect_of(|c| *c == Target::Wave) {
            assert!(
                inside(r, Layout::new(200.0, 674.0).header),
                "{r:?} left the header"
            );
        }
    }

    // ── What the playfield draws ────────────────────────────────────

    #[test]
    fn the_playfield_is_on_screen_and_inside_the_body() {
        let app = test_app();
        let l = Layout::new(SIZE.0, SIZE.1);
        let rect = probe::rect_of(&app, Target::Field).expect("the field is drawn");
        assert!(inside(rect, l.body));
    }

    #[test]
    fn every_asteroid_is_drawn_where_the_field_puts_it() {
        let app = test_app();
        let field = Field::new(Layout::new(SIZE.0, SIZE.1).body);
        assert!(!app.asteroids.is_empty(), "no asteroids to look for");
        for (i, asteroid) in app.asteroids.iter().enumerate() {
            let rect = probe::rect_of(&app, Target::Asteroid(i))
                .unwrap_or_else(|| panic!("asteroid {i} was not drawn"));
            let (cx, cy) = field.to_screen(asteroid.pos);
            let (gx, gy) = rect.centre();
            assert!(
                (cx - gx).abs() < 0.01 && (cy - gy).abs() < 0.01,
                "asteroid {i}'s hit box is not where the field puts it"
            );
        }
    }

    #[test]
    fn an_asteroid_wins_the_hit_test_over_the_playfield_behind_it() {
        // The field's box is recorded first on purpose. A click on an
        // asteroid that answered `Field` would be a click that could never
        // reach anything in the game.
        let app = test_app();
        let rect = probe::rect_of(&app, Target::Asteroid(0)).expect("the first asteroid is drawn");
        let (x, y) = rect.centre();
        assert_eq!(
            app.draw(SIZE).hit_test(x, y),
            Some(Target::Asteroid(0)),
            "the playfield swallowed the asteroid in front of it"
        );
    }

    #[test]
    fn the_ship_is_on_screen_once_it_has_stopped_blinking() {
        let app = app_with_a_settled_ship();
        let field = Field::new(Layout::new(SIZE.0, SIZE.1).body);
        let rect = probe::rect_of(&app, Target::Ship).expect("the ship is drawn");
        let (cx, cy) = field.to_screen(app.ship.pos);
        let (gx, gy) = rect.centre();
        assert!((cx - gx).abs() < 0.01 && (cy - gy).abs() < 0.01);
    }

    #[test]
    fn a_blinking_ship_is_off_the_screen_on_the_frames_it_is_not_drawn() {
        // Asking the game state would say the ship is alive on every one of
        // these frames. The screen says otherwise for half of them, and the
        // screen is what the player is looking at.
        let mut app = test_app();
        app.invulnerable_timer = INVULNERABLE_TIME;
        let mut drawn = 0;
        let mut hidden = 0;
        for counter in 0..6 {
            app.frame_counter = counter;
            if probe::is_visible(&app, Target::Ship) {
                drawn += 1;
            } else {
                hidden += 1;
            }
        }
        assert_eq!(drawn, 3, "the ship did not blink");
        assert_eq!(hidden, 3, "the ship did not come back");
    }

    #[test]
    fn a_dead_ship_is_not_drawn() {
        let mut app = app_with_a_settled_ship();
        app.ship_alive = false;
        assert!(!probe::is_visible(&app, Target::Ship));
    }

    #[test]
    fn a_shot_in_the_air_is_drawn_where_the_field_puts_it() {
        let mut app = test_app();
        app.asteroids.clear();
        app.invulnerable_timer = 0.0;
        app.input.shoot = true;
        tick(&mut app, 16);
        assert!(!app.bullets.is_empty(), "the shot was never fired");
        let field = Field::new(Layout::new(SIZE.0, SIZE.1).body);
        let rect = probe::rect_of(&app, Target::Bullet(0)).expect("the shot is drawn");
        let (cx, cy) = field.to_screen(app.bullets[0].pos);
        let (gx, gy) = rect.centre();
        assert!((cx - gx).abs() < 0.01 && (cy - gy).abs() < 0.01);
    }

    // ── The overlays ────────────────────────────────────────────────

    #[test]
    fn the_pause_sheet_names_both_ways_out() {
        let mut app = test_app();
        app.state = GameState::Paused;
        for target in [
            Target::Overlay,
            Target::OverlayTitle,
            Target::Resume,
            Target::NewGame,
        ] {
            assert!(
                probe::is_visible(&app, target),
                "{target:?} was not on the pause sheet"
            );
        }

        // The words, not just the boxes. A line whose text is blank still
        // occupies its place in the stack and still records its hit box, so
        // `is_visible(Target::NewGame)` stays true for a sheet that offers
        // nothing -- emptying the string left every assertion above green.
        // The name of this test is a claim about what the sheet *says*, and
        // only reading the text tests that claim.
        let shown = texts(&app, SIZE);
        for wanted in ["PAUSED", "Press P or Esc to resume", "Press N for new game"] {
            assert!(
                shown.iter().any(|t| t == wanted),
                "the pause sheet never said {wanted:?}; it said {shown:?}"
            );
        }
    }

    #[test]
    fn the_game_over_box_reports_every_final_number() {
        let mut app = test_app();
        app.state = GameState::GameOver;
        app.score = 1234;
        app.high_score = 5678;
        app.wave = 9;
        let shown = texts(&app, SIZE);
        for wanted in [
            "GAME OVER",
            "Score: 1234",
            "High Score: 5678",
            "Wave reached: 9",
        ] {
            assert!(
                shown.iter().any(|t| t == wanted),
                "{wanted:?} was missing from {shown:?}"
            );
        }
        for i in 0..3 {
            assert!(
                probe::is_visible(&app, Target::FinalStat(i)),
                "final stat {i} had no box"
            );
        }
    }

    #[test]
    fn the_overlay_lines_do_not_sit_on_top_of_each_other() {
        let mut app = test_app();
        app.state = GameState::GameOver;
        let frame = app.frame(SIZE.0, SIZE.1);
        let boxes: Vec<Rect> = [
            Target::OverlayTitle,
            Target::FinalStat(0),
            Target::FinalStat(1),
            Target::FinalStat(2),
            Target::NewGame,
        ]
        .into_iter()
        .filter_map(|t| frame.rect_of(|c| *c == t))
        .collect();
        assert_eq!(boxes.len(), 5, "not every line was drawn");
        for (i, a) in boxes.iter().enumerate() {
            for b in boxes.iter().skip(i + 1) {
                assert!(!overlaps(*a, *b), "two overlay lines shared space");
            }
        }
    }

    #[test]
    fn an_overlay_in_a_short_window_drops_lines_rather_than_running_out_of_the_box() {
        let mut app = test_app();
        app.state = GameState::GameOver;
        let size = (824.0, 200.0);
        let frame = app.frame(size.0, size.1);
        let field = Field::new(Layout::new(size.0, size.1).body);
        let boxes: Vec<Rect> = [
            Target::OverlayTitle,
            Target::FinalStat(0),
            Target::FinalStat(1),
            Target::FinalStat(2),
            Target::NewGame,
        ]
        .into_iter()
        .filter_map(|t| frame.rect_of(|c| *c == t))
        .collect();
        assert!(
            boxes.len() < 5,
            "all five lines claimed to fit a 200-pixel window"
        );
        for r in &boxes {
            assert!(
                inside(*r, field.rect),
                "an overlay line ran out of the field: {r:?}"
            );
        }
    }

    #[test]
    fn the_game_over_box_takes_its_share_of_the_field_rather_than_a_fixed_size() {
        let mut app = test_app();
        app.state = GameState::GameOver;
        let small = (500.0, 420.0);
        let big = (1600.0, 1300.0);
        let a = probe::rect_of_sized(&app, Target::OverlayTitle, small).expect("drawn small");
        let b = probe::rect_of_sized(&app, Target::OverlayTitle, big).expect("drawn big");
        assert!(
            b.w > a.w,
            "the box was the same size in a window three times as large"
        );
    }

    #[test]
    fn there_is_no_overlay_while_the_game_is_being_played() {
        let app = test_app();
        assert_eq!(app.state, GameState::Playing);
        assert!(!probe::is_visible(&app, Target::Overlay));
        assert!(!probe::is_visible(&app, Target::Resume));
    }

    // ── Clicks ──────────────────────────────────────────────────────

    #[test]
    fn a_click_on_the_resume_line_resumes() {
        let mut app = test_app();
        app.state = GameState::Paused;
        assert_eq!(
            probe::click(&mut app, Target::Resume),
            EventResult::Consumed
        );
        assert_eq!(app.state, GameState::Playing);
    }

    #[test]
    fn a_click_anywhere_on_the_pause_sheet_resumes() {
        let mut app = test_app();
        app.state = GameState::Paused;
        assert_eq!(
            probe::click(&mut app, Target::Overlay),
            EventResult::Consumed
        );
        assert_eq!(app.state, GameState::Playing);
    }

    #[test]
    fn a_click_on_the_new_game_line_while_paused_starts_a_new_game() {
        let mut app = test_app();
        // Both, because that is the pair real play produces: the high score
        // is raised as the score is earned, not at the end, so a game
        // abandoned at 500 has already banked its 500. Setting `score`
        // alone would be a state the game cannot reach, and the assertion
        // below would then be testing the fixture rather than `new_game`.
        app.score = 500;
        app.high_score = 500;
        app.state = GameState::Paused;
        assert_eq!(
            probe::click(&mut app, Target::NewGame),
            EventResult::Consumed
        );
        assert_eq!(app.score, 0, "the old score survived the new game");
        assert_eq!(app.state, GameState::Playing);
        assert_eq!(
            app.high_score, 500,
            "the high score did not survive the new game"
        );
    }

    #[test]
    fn a_click_on_the_game_over_sheet_starts_a_new_game() {
        let mut app = test_app();
        app.state = GameState::GameOver;
        app.lives = 0;
        assert_eq!(
            probe::click(&mut app, Target::Overlay),
            EventResult::Consumed
        );
        assert_eq!(app.state, GameState::Playing);
        assert_eq!(app.lives, INITIAL_LIVES);
    }

    /// The dead zone the sheet's own box used to cut out of itself.
    ///
    /// `probe::click(.., Target::Overlay)` aims at the middle of the overlay,
    /// which is where the box is, so the test above covers this too -- but
    /// only by accident of where the centre lands. These aim at the lines by
    /// name, so a future overlay that moves its box off-centre still has the
    /// question asked of it.
    #[test]
    fn a_click_on_a_line_of_the_game_over_box_starts_a_new_game() {
        for target in [
            Target::OverlayTitle,
            Target::FinalStat(0),
            Target::FinalStat(1),
            Target::FinalStat(2),
        ] {
            let mut app = test_app();
            app.state = GameState::GameOver;
            app.lives = 0;
            assert_eq!(
                probe::click(&mut app, target),
                EventResult::Consumed,
                "{target:?} is part of the sheet and did nothing"
            );
            assert_eq!(app.state, GameState::Playing, "{target:?}");
        }
    }

    #[test]
    fn a_click_on_the_title_of_the_pause_sheet_resumes() {
        let mut app = test_app();
        app.state = GameState::Paused;
        assert_eq!(
            probe::click(&mut app, Target::OverlayTitle),
            EventResult::Consumed
        );
        assert_eq!(app.state, GameState::Playing);
    }

    #[test]
    fn a_click_during_play_does_nothing() {
        // There is nothing on a playfield to click at, and a pointer cannot
        // fly a ship. Saying `Ignored` is what stops the window redrawing on
        // every stray click.
        let mut app = test_app();
        assert_eq!(probe::click(&mut app, Target::Field), EventResult::Ignored);
        assert_eq!(app.state, GameState::Playing);
    }

    #[test]
    fn a_click_on_the_header_does_nothing() {
        let mut app = test_app();
        app.state = GameState::Paused;
        assert_eq!(probe::click(&mut app, Target::Score), EventResult::Ignored);
        assert_eq!(
            app.state,
            GameState::Paused,
            "the score bar resumed the game"
        );

        // And then the strip itself, which the click above never reaches. The
        // header's own hit box is recorded *before* the readings drawn on it,
        // so a reading wins the hit test wherever the two overlap, and a click
        // aimed at `Target::Score` tests the reading only. An arm added for
        // `Target::Header` -- the overlay dead zone the other way round -- was
        // invisible here until the strip was clicked where nothing covers it.
        // (`known-issues.md` lesson 74: a test that names the target delivers
        // the event past the code that decides the target.)
        let header = probe::rect_of(&app, Target::Header).expect("the header is drawn");
        let (x, y) = (header.right() - 2.0, header.centre().1);
        assert_eq!(
            app.frame(SIZE.0, SIZE.1).hit_test(x, y),
            Some(Target::Header),
            "the bare end of the header strip is covered by something else, \
             so this click is not testing the header"
        );
        assert_eq!(
            app.click_at(x, y, MouseButton::Left, SIZE),
            EventResult::Ignored
        );
        assert_eq!(
            app.state,
            GameState::Paused,
            "the header strip resumed the game"
        );
    }

    #[test]
    fn a_click_on_nothing_at_all_does_nothing() {
        let mut app = test_app();
        app.state = GameState::Paused;
        assert_eq!(probe::click_background(&mut app), EventResult::Ignored);
        assert_eq!(app.state, GameState::Paused);
    }

    #[test]
    fn a_right_click_is_not_a_click() {
        let mut app = test_app();
        app.state = GameState::Paused;
        assert_eq!(
            probe::click_with(&mut app, Target::Resume, MouseButton::Right),
            EventResult::Ignored
        );
        assert_eq!(app.state, GameState::Paused);
    }

    #[test]
    fn a_click_lands_where_the_window_it_was_resized_to_put_the_control() {
        // The click is read against the size the frame was drawn at. If the
        // app kept the size it started with, this click would land on the
        // wrong thing in a window the user resized.
        //
        // Two things about the shape of this test were wrong at first and are
        // deliberate now.
        //
        // It used to resize *down*, to 500x420, and click the pause sheet's
        // resume line there. That is not a witness: the small window's
        // coordinates are all inside the big one, and every target on the
        // pause sheet resumes, so reading the click against the wrong size
        // still landed on the sheet and still resumed. Both answers were
        // `Consumed`/`Playing` and the test could not tell them apart --
        // pinning the size to the one the game opens at left it green
        // (`known-issues.md` lesson 75: a witness that moves less than the
        // tolerance has not moved). Resizing *up* past the opening size puts
        // the control at a point that is off the opening window altogether,
        // where the wrong reading can only answer "nothing here".
        //
        // And it used to resize through `Probe::click_at`, which calls
        // `resize` itself. That routes around `handle_event`'s `Resize` arm,
        // so a build that threw resize events away passed this test too. The
        // event is how a real window says it, so the event is what is sent.
        let mut app = test_app();
        app.state = GameState::Paused;
        let big = (2000.0, 1600.0);
        let rect = probe::rect_of_sized(&app, Target::Resume, big).expect("drawn big");
        let (x, y) = rect.centre();

        // The premise, checked rather than assumed: this point must be one
        // the opening size cannot explain.
        assert_eq!(
            app.draw(SIZE).hit_test(x, y),
            None,
            "({x}, {y}) is still on something in a {SIZE:?} window, so a click \
             read against the wrong size would land on it anyway"
        );

        assert_eq!(
            handle_event(
                &mut app,
                &Event::Resize {
                    width: 2000,
                    height: 1600
                }
            ),
            EventResult::Consumed
        );
        assert_eq!(
            handle_event(
                &mut app,
                &Event::Mouse(MouseEvent {
                    x,
                    y,
                    kind: MouseEventKind::Press(MouseButton::Left),
                })
            ),
            EventResult::Consumed
        );
        assert_eq!(app.state, GameState::Playing);
    }

    #[test]
    fn the_overlay_hides_the_asteroids_behind_it_from_a_click() {
        // The sheet is drawn over the field, so it takes the click. Anything
        // else would be a paused game that could still be poked.
        let mut app = test_app();
        let rect = probe::rect_of(&app, Target::Asteroid(0)).expect("an asteroid is drawn");
        let (x, y) = rect.centre();
        app.state = GameState::Paused;
        let hit = app.draw(SIZE).hit_test(x, y);
        assert!(
            matches!(
                hit,
                Some(Target::Overlay | Target::Resume | Target::NewGame | Target::OverlayTitle)
            ),
            "the asteroid was still reachable through the pause sheet: {hit:?}"
        );
    }

    // ── Keys ────────────────────────────────────────────────────────

    #[test]
    fn keys_reach_the_app_through_the_window() {
        let mut app = test_app();
        assert_eq!(
            probe::key(&mut app, &probe::press(Key::Left)),
            EventResult::Consumed
        );
        assert!(app.input.left);
    }

    #[test]
    fn a_key_coming_back_up_is_not_a_second_press_while_paused() {
        // A `P` held down pauses on the way down. Acting on the way up too
        // would unpause the moment the player let go.
        let mut app = test_app();
        key_down(&mut app, Key::P);
        assert_eq!(app.state, GameState::Paused);
        assert_eq!(key_up(&mut app, Key::P), EventResult::Ignored);
        assert_eq!(
            app.state,
            GameState::Paused,
            "letting go of P unpaused the game"
        );
    }

    #[test]
    fn a_key_coming_back_up_does_not_restart_a_finished_game() {
        let mut app = test_app();
        app.state = GameState::GameOver;
        assert_eq!(key_up(&mut app, Key::N), EventResult::Ignored);
        assert_eq!(app.state, GameState::GameOver);
    }

    #[test]
    fn a_key_the_game_does_not_use_is_ignored() {
        let mut app = test_app();
        assert_eq!(
            probe::key(&mut app, &probe::press(Key::Q)),
            EventResult::Ignored
        );
    }

    #[test]
    fn pausing_lets_go_of_every_key_that_was_held() {
        // The up-stroke for a key held at the moment of pausing goes to
        // whatever has the keyboard while the game is away, so it never
        // arrives. A ship still turning on unpause is what that looks like.
        let mut app = test_app();
        key_down(&mut app, Key::Left);
        key_down(&mut app, Key::Up);
        assert!(app.input.left && app.input.thrust);
        key_down(&mut app, Key::P);
        assert!(!app.input.left, "the ship was still turning");
        assert!(!app.input.thrust, "the engine was still running");
    }

    // ── The window ──────────────────────────────────────────────────

    #[test]
    fn the_window_has_a_name() {
        let app = test_app();
        assert_eq!(app.title(), "Asteroids");
        assert_eq!(app.app_id(), "asteroids");
    }

    #[test]
    fn the_window_opens_at_the_size_the_game_was_drawn_for() {
        let app = test_app();
        let (w, h) = app.initial_size();
        assert_eq!(f32_from_u32(w), AsteroidsApp::SIZE.0);
        assert_eq!(f32_from_u32(h), AsteroidsApp::SIZE.1);
    }

    #[test]
    fn the_window_asks_to_be_woken_for_the_animation() {
        // Without a tick interval nothing moves: every asteroid in the game
        // is where it was when the window opened.
        let app = test_app();
        assert_eq!(app.tick_interval(), Some(TICK));
    }

    #[test]
    fn the_close_button_closes() {
        let mut app = test_app();
        assert_eq!(app.on_event(&Event::CloseRequested), Response::Exit);
    }

    #[test]
    fn an_event_that_changed_something_asks_for_a_redraw() {
        let mut app = test_app();
        assert_eq!(
            app.on_event(&Event::Key(probe::press(Key::Left))),
            Response::Redraw
        );
    }

    #[test]
    fn an_event_that_changed_nothing_does_not_ask_for_a_redraw() {
        let mut app = test_app();
        assert_eq!(
            app.on_event(&Event::Key(probe::press(Key::Q))),
            Response::Idle
        );
    }

    #[test]
    fn a_tick_while_paused_is_not_a_change() {
        // Sixty ticks a second against a frame that cannot have changed. A
        // paused game that answered `Redraw` would repaint sixty times a
        // second to no effect.
        let mut app = test_app();
        app.state = GameState::Paused;
        assert_eq!(
            app.on_event(&Event::Tick { elapsed_ms: 16 }),
            Response::Idle
        );
    }

    #[test]
    fn a_tick_while_playing_is_a_change() {
        let mut app = test_app();
        assert_eq!(
            app.on_event(&Event::Tick { elapsed_ms: 16 }),
            Response::Redraw
        );
    }

    #[test]
    fn a_resize_is_remembered() {
        let mut app = test_app();
        assert_eq!(
            handle_event(
                &mut app,
                &Event::Resize {
                    width: 640,
                    height: 480
                }
            ),
            EventResult::Consumed
        );
        assert_eq!(app.size(), (640.0, 480.0));
    }

    #[test]
    fn the_size_a_frame_is_drawn_at_is_the_size_the_next_click_is_read_against() {
        let mut app = test_app();
        app.state = GameState::Paused;
        let _ = app.render(500.0, 420.0);
        assert_eq!(app.size(), (500.0, 420.0));
    }

    #[test]
    fn starting_a_new_game_does_not_forget_the_window() {
        // `new_game` is `*self = Self::with_seed(..)`, which would otherwise
        // put the size back to the one the program guessed at startup -- and
        // the next click would be read against a window that is not there.
        let mut app = test_app();
        app.resize(500.0, 420.0);
        app.new_game();
        assert_eq!(app.size(), (500.0, 420.0));
    }

    #[test]
    fn the_frame_is_balanced() {
        // Every clip and translate pushed is popped. An unbalanced frame
        // draws the next window's contents through this one's clip.
        let mut app = test_app();
        assert!(app.frame(SIZE.0, SIZE.1).is_balanced());
        app.state = GameState::Paused;
        assert!(app.frame(SIZE.0, SIZE.1).is_balanced());
        app.state = GameState::GameOver;
        assert!(app.frame(SIZE.0, SIZE.1).is_balanced());
    }

    #[test]
    fn a_window_of_no_size_still_draws_a_frame() {
        // The compositor can hand out a zero-size window while a resize is in
        // flight. Panicking there loses the game.
        let mut app = test_app();
        for size in [(0.0, 0.0), (1.0, 800.0), (800.0, 1.0)] {
            let f = app.frame(size.0, size.1);
            assert!(f.is_balanced());

            // And it draws nothing of no size. A window this small makes
            // every band empty, so each `fill` is handed a rectangle with no
            // area; without the guard in `fill` they all reach the
            // compositor as zero-size `FillRect`s. Balance says nothing
            // about that -- deleting the guard left the assertion above
            // green -- and the commands are the only place it shows.
            for c in f.commands() {
                if let RenderCommand::FillRect { width, height, .. } = c {
                    assert!(
                        *width > 0.0 && *height > 0.0,
                        "a {width}x{height} rectangle was filled in a {size:?} window"
                    );
                }
            }
        }
        app.state = GameState::GameOver;
        assert!(app.frame(0.0, 0.0).is_balanced());
    }

    // ── Faults the wiring exposed ───────────────────────────────────

    #[test]
    fn a_late_wave_does_not_fill_the_field_with_asteroids() {
        // `INITIAL_ASTEROIDS + wave - 1` with nothing on the end of it: wave
        // two hundred meant two hundred and three asteroids, each wanting a
        // spawn point 150 units clear of the ship in a field 800 across.
        let mut app = test_app();
        app.asteroids.clear();
        app.wave = 200;
        app.advance_wave();
        assert_eq!(app.asteroids.len(), MAX_WAVE_ASTEROIDS);
    }

    #[test]
    fn an_early_wave_is_still_one_asteroid_bigger_than_the_last() {
        // The cap must not flatten the difficulty curve where the curve is
        // the point.
        let mut app = test_app();
        app.asteroids.clear();
        app.wave = 1;
        app.advance_wave();
        assert_eq!(app.asteroids.len(), INITIAL_ASTEROIDS + 1);
    }

    #[test]
    fn a_score_at_the_ceiling_does_not_wrap_round_to_nothing() {
        let mut app = setup_target_practice();
        app.score = u32::MAX;
        app.high_score = u32::MAX;
        app.input.shoot = true;
        tick_many(&mut app, 500, 16);
        assert_eq!(app.score, u32::MAX, "the score wrapped");
    }

    #[test]
    fn the_wave_counter_does_not_wrap_round_to_nothing() {
        let mut app = test_app();
        app.wave = u32::MAX;
        app.asteroids.clear();
        app.advance_wave();
        assert_eq!(app.wave, u32::MAX);
    }
}
