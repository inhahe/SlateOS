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

//! OurOS Asteroids -- classic space shooter arcade game.
//!
//! The player controls a triangular ship in the center of a wraparound
//! playfield. Asteroids of three sizes drift across the screen; shooting
//! a large asteroid splits it into two medium ones, medium into two small.
//! The player earns points for destroying asteroids and loses lives on
//! collision. Clearing all asteroids advances the wave.
//!
//! Controls: Left/Right to rotate, Up to thrust, Space to shoot,
//! P to pause, N for new game.

use guitk::color::Color;
use guitk::event::{Event, Key};
#[cfg(test)]
use guitk::event::{KeyEvent, Modifiers};
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

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
const MAUVE: Color = Color::from_hex(0xCBA6F7);

// ── Layout constants ────────────────────────────────────────────────
const FIELD_WIDTH: f32 = 800.0;
const FIELD_HEIGHT: f32 = 600.0;
const PADDING: f32 = 12.0;
const HEADER_HEIGHT: f32 = 50.0;
const WINDOW_WIDTH: f32 = FIELD_WIDTH + PADDING * 2.0;
const WINDOW_HEIGHT: f32 = FIELD_HEIGHT + HEADER_HEIGHT + PADDING * 2.0;
const HEADER_FONT_SIZE: f32 = 18.0;
const TITLE_FONT_SIZE: f32 = 24.0;
const OVERLAY_FONT_SIZE: f32 = 16.0;
const SMALL_FONT_SIZE: f32 = 13.0;

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

// ── LCG random number generator ────────────────────────────────────
/// Simple linear congruential generator. Parameters from Numerical Recipes.
struct Lcg {
    state: u64,
}

impl Lcg {
    const fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.state
    }

    /// Returns a value in `0..bound` (exclusive upper bound).
    fn next_bounded(&mut self, bound: usize) -> usize {
        let val = self.next_u64();
        (val % bound as u64) as usize
    }

    /// Returns a float in [0.0, 1.0).
    fn next_f32(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }

    /// Returns a float in [min, max).
    fn next_range(&mut self, min: f32, max: f32) -> f32 {
        min + self.next_f32() * (max - min)
    }

    /// Returns a random angle in [0, TAU).
    fn next_angle(&mut self) -> f32 {
        self.next_f32() * TAU
    }
}

// ── Vec2 ────────────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq)]
struct Vec2 {
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

    fn length_sq(self) -> f32 {
        self.x * self.x + self.y * self.y
    }

    fn add(self, other: Self) -> Self {
        Self {
            x: self.x + other.x,
            y: self.y + other.y,
        }
    }

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

    fn label(self) -> &'static str {
        match self {
            AsteroidSize::Large => "Large",
            AsteroidSize::Medium => "Medium",
            AsteroidSize::Small => "Small",
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
    fn new(pos: Vec2, vel: Vec2, size: AsteroidSize, rng: &mut Lcg) -> Self {
        let n = size.vertex_count();
        let base_r = size.radius();
        let mut vertex_radii = Vec::with_capacity(n);
        for _ in 0..n {
            // Vary each vertex radius by +/-30% for jagged look.
            vertex_radii.push(base_r * rng.next_range(0.7, 1.3));
        }
        Self {
            pos,
            vel,
            size,
            angle: rng.next_angle(),
            rotation_speed: rng.next_range(-2.0, 2.0),
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
        let mut verts = Vec::with_capacity(n);
        for i in 0..n {
            let a = self.angle + (i as f32 / n as f32) * TAU;
            let r = self.vertex_radii[i];
            verts.push(Vec2::new(
                self.pos.x + cos_f32(a) * r,
                self.pos.y + sin_f32(a) * r,
            ));
        }
        verts
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

    fn alpha(&self) -> u8 {
        if self.max_lifetime <= 0.0 {
            return 0;
        }
        let ratio = self.lifetime / self.max_lifetime;
        (ratio * 255.0) as u8
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

// ── Main app struct ─────────────────────────────────────────────────
struct AsteroidsApp {
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
    rng: Lcg,
    frame_counter: u64,
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
            rng: Lcg::new(seed),
            frame_counter: 0,
        };
        app.spawn_wave(INITIAL_ASTEROIDS);
        app
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
        let angle = self.rng.next_angle();
        let speed = size.speed() * self.rng.next_range(0.5, 1.5);
        let vel = Vec2::new(cos_f32(angle) * speed, sin_f32(angle) * speed);
        self.asteroids.push(Asteroid::new(pos, vel, size, &mut self.rng));
    }

    /// Pick a random position along the field edges, ensuring it is far
    /// enough from the ship.
    fn random_edge_position(&mut self, radius: f32) -> Vec2 {
        loop {
            let side = self.rng.next_bounded(4);
            let pos = match side {
                0 => Vec2::new(self.rng.next_range(0.0, FIELD_WIDTH), radius),
                1 => Vec2::new(self.rng.next_range(0.0, FIELD_WIDTH), FIELD_HEIGHT - radius),
                2 => Vec2::new(radius, self.rng.next_range(0.0, FIELD_HEIGHT)),
                _ => Vec2::new(FIELD_WIDTH - radius, self.rng.next_range(0.0, FIELD_HEIGHT)),
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
            let speed = child_size.speed() * self.rng.next_range(0.6, 1.4);
            let vel = Vec2::new(cos_f32(base_angle) * speed, sin_f32(base_angle) * speed);
            let nudge = Vec2::new(
                cos_f32(base_angle) * child_size.radius(),
                sin_f32(base_angle) * child_size.radius(),
            );
            let pos = parent_pos.add(nudge).wrap(FIELD_WIDTH, FIELD_HEIGHT);
            self.asteroids.push(Asteroid::new(pos, vel, child_size, &mut self.rng));
        }
    }

    // ── Particle effects ────────────────────────────────────────────

    fn spawn_explosion(&mut self, pos: Vec2, count: usize, color: Color) {
        for _ in 0..count {
            let angle = self.rng.next_angle();
            let speed = self.rng.next_range(30.0, 150.0);
            let lifetime = self.rng.next_range(0.3, 0.8);
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
        let angle = self.ship.angle + PI + self.rng.next_range(-0.3, 0.3);
        let speed = self.rng.next_range(50.0, 120.0);
        let lifetime = self.rng.next_range(0.1, 0.3);
        self.particles.push(Particle {
            pos: exhaust,
            vel: Vec2::new(cos_f32(angle) * speed, sin_f32(angle) * speed),
            lifetime,
            max_lifetime: lifetime,
            color: PEACH,
        });
    }

    // ── New game / restart ──────────────────────────────────────────

    fn new_game(&mut self) {
        let high = self.high_score;
        let seed = self.rng.next_u64();
        *self = Self::with_seed(seed);
        self.high_score = high;
    }

    // ── Input handling ──────────────────────────────────────────────

    fn handle_key(&mut self, key: Key, pressed: bool) {
        match self.state {
            GameState::Playing => self.handle_key_playing(key, pressed),
            GameState::Paused => {
                if pressed {
                    self.handle_key_paused(key);
                }
            }
            GameState::GameOver => {
                if pressed {
                    self.handle_key_game_over(key);
                }
            }
        }
    }

    fn handle_key_playing(&mut self, key: Key, pressed: bool) {
        match key {
            Key::Left | Key::A => self.input.left = pressed,
            Key::Right | Key::D => self.input.right = pressed,
            Key::Up | Key::W => self.input.thrust = pressed,
            Key::Space => self.input.shoot = pressed,
            Key::P | Key::Escape
                if pressed => {
                    self.state = GameState::Paused;
                    // Release all input on pause.
                    self.input = InputState::new();
                }
            Key::N
                if pressed => {
                    self.new_game();
                }
            _ => {}
        }
    }

    fn handle_key_paused(&mut self, key: Key) {
        match key {
            Key::P | Key::Escape => self.state = GameState::Playing,
            Key::N => self.new_game(),
            _ => {}
        }
    }

    fn handle_key_game_over(&mut self, key: Key) {
        match key {
            Key::N | Key::Enter | Key::Space => self.new_game(),
            _ => {}
        }
    }

    // ── Game tick ───────────────────────────────────────────────────

    fn handle_tick(&mut self, elapsed_ms: u64) {
        if self.state != GameState::Playing {
            return;
        }

        let dt = elapsed_ms as f32 / 1000.0;
        self.frame_counter += 1;

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
        if self.input.shoot && self.ship_alive && self.shoot_cooldown <= 0.0
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
                let dist = bullet.pos.wrapped_distance(
                    asteroid.pos,
                    FIELD_WIDTH,
                    FIELD_HEIGHT,
                );
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
            let asteroid = &self.asteroids[ai];
            score_gain += asteroid.size.score();
            explosions.push((asteroid.pos, asteroid.size.color()));
            if let Some(child_size) = asteroid.size.child_size() {
                children_to_spawn.push((asteroid.pos, asteroid.vel, child_size));
            }
            destroyed_indices.push(ai);
            spent_bullets.push(bi);
        }

        // Apply score.
        self.score += score_gain;
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
            let dist = self.ship.pos.wrapped_distance(
                asteroid.pos,
                FIELD_WIDTH,
                FIELD_HEIGHT,
            );
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

    fn advance_wave(&mut self) {
        self.wave += 1;
        let count = INITIAL_ASTEROIDS + (self.wave as usize - 1);
        self.spawn_wave(count);
    }

    // ── Queries ─────────────────────────────────────────────────────

    fn asteroid_count(&self) -> usize {
        self.asteroids.len()
    }

    fn bullet_count(&self) -> usize {
        self.bullets.len()
    }

    fn is_invulnerable(&self) -> bool {
        self.invulnerable_timer > 0.0
    }

    // ── Rendering ───────────────────────────────────────────────────

    fn render(&self) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Background.
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: WINDOW_WIDTH,
            height: WINDOW_HEIGHT,
            color: BASE,
            corner_radii: CornerRadii::all(6.0),
        });

        // Header bar.
        self.render_header(&mut cmds);

        // Playfield background.
        let fx = PADDING;
        let fy = PADDING + HEADER_HEIGHT;
        cmds.push(RenderCommand::FillRect {
            x: fx,
            y: fy,
            width: FIELD_WIDTH,
            height: FIELD_HEIGHT,
            color: MANTLE,
            corner_radii: CornerRadii::all(4.0),
        });

        // Stars (static background dots based on frame counter seed).
        self.render_stars(&mut cmds, fx, fy);

        // Particles.
        self.render_particles(&mut cmds, fx, fy);

        // Asteroids.
        self.render_asteroids(&mut cmds, fx, fy);

        // Bullets.
        self.render_bullets(&mut cmds, fx, fy);

        // Ship.
        if self.ship_alive {
            self.render_ship(&mut cmds, fx, fy);
        }

        // Overlay.
        self.render_overlay(&mut cmds, fx, fy);

        cmds
    }

    fn render_header(&self, cmds: &mut Vec<RenderCommand>) {
        let header_w = WINDOW_WIDTH - PADDING * 2.0;

        cmds.push(RenderCommand::FillRect {
            x: PADDING,
            y: PADDING,
            width: header_w,
            height: HEADER_HEIGHT - 4.0,
            color: MANTLE,
            corner_radii: CornerRadii::all(4.0),
        });

        // Title.
        cmds.push(RenderCommand::Text {
            x: PADDING + 10.0,
            y: PADDING + 8.0,
            text: String::from("Asteroids"),
            color: TEAL,
            font_size: HEADER_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Score.
        cmds.push(RenderCommand::Text {
            x: PADDING + 120.0,
            y: PADDING + 8.0,
            text: format!("Score: {}", self.score),
            color: TEXT_COLOR,
            font_size: HEADER_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // High score.
        cmds.push(RenderCommand::Text {
            x: PADDING + 280.0,
            y: PADDING + 8.0,
            text: format!("Hi: {}", self.high_score),
            color: YELLOW,
            font_size: HEADER_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Lives.
        cmds.push(RenderCommand::Text {
            x: PADDING + 420.0,
            y: PADDING + 8.0,
            text: format!("Lives: {}", self.lives),
            color: RED,
            font_size: HEADER_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Wave.
        cmds.push(RenderCommand::Text {
            x: PADDING + 560.0,
            y: PADDING + 8.0,
            text: format!("Wave: {}", self.wave),
            color: LAVENDER,
            font_size: HEADER_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Controls hint.
        cmds.push(RenderCommand::Text {
            x: PADDING + 10.0,
            y: PADDING + 28.0,
            text: String::from("Arrows: Move  Space: Shoot  P: Pause  N: New"),
            color: OVERLAY0,
            font_size: SMALL_FONT_SIZE,
            font_weight: FontWeightHint::Light,
            max_width: None,
        });
    }

    fn render_stars(&self, cmds: &mut Vec<RenderCommand>, fx: f32, fy: f32) {
        // Draw a static starfield using deterministic positions from a fixed seed.
        let mut star_rng = Lcg::new(999);
        let star_color = Color::rgba(100, 100, 140, 60);
        let star_bright = Color::rgba(150, 150, 200, 100);
        for i in 0..40 {
            let sx = star_rng.next_range(4.0, FIELD_WIDTH - 4.0);
            let sy = star_rng.next_range(4.0, FIELD_HEIGHT - 4.0);
            let size = if i % 5 == 0 { 2.0 } else { 1.5 };
            let color = if i % 5 == 0 { star_bright } else { star_color };
            cmds.push(RenderCommand::FillRect {
                x: fx + sx,
                y: fy + sy,
                width: size,
                height: size,
                color,
                corner_radii: CornerRadii::all(1.0),
            });
        }
    }

    fn render_particles(&self, cmds: &mut Vec<RenderCommand>, fx: f32, fy: f32) {
        for particle in &self.particles {
            let alpha = particle.alpha();
            if alpha == 0 {
                continue;
            }
            let c = particle.color;
            let color = Color::rgba(c.r, c.g, c.b, alpha);
            let size = 2.0 + (alpha as f32 / 255.0) * 2.0;
            cmds.push(RenderCommand::FillRect {
                x: fx + particle.pos.x - size / 2.0,
                y: fy + particle.pos.y - size / 2.0,
                width: size,
                height: size,
                color,
                corner_radii: CornerRadii::all(size / 2.0),
            });
        }
    }

    fn render_asteroids(&self, cmds: &mut Vec<RenderCommand>, fx: f32, fy: f32) {
        for asteroid in &self.asteroids {
            let verts = asteroid.vertices();
            let color = asteroid.size.color();

            // Draw the asteroid as line segments connecting vertices.
            let n = verts.len();
            for i in 0..n {
                let v1 = &verts[i];
                let v2 = &verts[(i + 1) % n];
                cmds.push(RenderCommand::Line {
                    x1: fx + v1.x,
                    y1: fy + v1.y,
                    x2: fx + v2.x,
                    y2: fy + v2.y,
                    color,
                    width: 1.5,
                });
            }

            // Fill center slightly for visibility.
            let fill_r = asteroid.radius() * 0.3;
            let fill_color = Color::rgba(color.r, color.g, color.b, 40);
            cmds.push(RenderCommand::FillRect {
                x: fx + asteroid.pos.x - fill_r,
                y: fy + asteroid.pos.y - fill_r,
                width: fill_r * 2.0,
                height: fill_r * 2.0,
                color: fill_color,
                corner_radii: CornerRadii::all(fill_r),
            });
        }
    }

    fn render_bullets(&self, cmds: &mut Vec<RenderCommand>, fx: f32, fy: f32) {
        for bullet in &self.bullets {
            cmds.push(RenderCommand::FillRect {
                x: fx + bullet.pos.x - BULLET_RADIUS,
                y: fy + bullet.pos.y - BULLET_RADIUS,
                width: BULLET_RADIUS * 2.0,
                height: BULLET_RADIUS * 2.0,
                color: GREEN,
                corner_radii: CornerRadii::all(BULLET_RADIUS),
            });
        }
    }

    fn render_ship(&self, cmds: &mut Vec<RenderCommand>, fx: f32, fy: f32) {
        // Blink during invulnerability.
        if self.is_invulnerable() && self.frame_counter % 6 < 3 {
            return;
        }

        let nose = self.ship.nose();
        let lw = self.ship.left_wing();
        let rw = self.ship.right_wing();

        let ship_color = BLUE;

        // Draw the triangle as 3 lines.
        cmds.push(RenderCommand::Line {
            x1: fx + nose.x,
            y1: fy + nose.y,
            x2: fx + lw.x,
            y2: fy + lw.y,
            color: ship_color,
            width: 2.0,
        });
        cmds.push(RenderCommand::Line {
            x1: fx + lw.x,
            y1: fy + lw.y,
            x2: fx + rw.x,
            y2: fy + rw.y,
            color: ship_color,
            width: 2.0,
        });
        cmds.push(RenderCommand::Line {
            x1: fx + rw.x,
            y1: fy + rw.y,
            x2: fx + nose.x,
            y2: fy + nose.y,
            color: ship_color,
            width: 2.0,
        });

        // Thrust flame.
        if self.ship.thrusting {
            let exhaust = self.ship.exhaust_point();
            let flame_tip = Vec2::new(
                self.ship.pos.x - cos_f32(self.ship.angle) * SHIP_RADIUS * 1.2,
                self.ship.pos.y - sin_f32(self.ship.angle) * SHIP_RADIUS * 1.2,
            );
            let flame_color = if self.frame_counter % 4 < 2 { PEACH } else { YELLOW };
            cmds.push(RenderCommand::Line {
                x1: fx + lw.x,
                y1: fy + lw.y,
                x2: fx + flame_tip.x,
                y2: fy + flame_tip.y,
                color: flame_color,
                width: 1.5,
            });
            cmds.push(RenderCommand::Line {
                x1: fx + rw.x,
                y1: fy + rw.y,
                x2: fx + flame_tip.x,
                y2: fy + flame_tip.y,
                color: flame_color,
                width: 1.5,
            });

            // Inner flame (brighter, shorter).
            let inner_tip = Vec2::new(
                exhaust.x - cos_f32(self.ship.angle) * SHIP_RADIUS * 0.3,
                exhaust.y - sin_f32(self.ship.angle) * SHIP_RADIUS * 0.3,
            );
            cmds.push(RenderCommand::Line {
                x1: fx + exhaust.x + cos_f32(self.ship.angle + 1.0) * 3.0,
                y1: fy + exhaust.y + sin_f32(self.ship.angle + 1.0) * 3.0,
                x2: fx + inner_tip.x,
                y2: fy + inner_tip.y,
                color: YELLOW,
                width: 1.0,
            });
            cmds.push(RenderCommand::Line {
                x1: fx + exhaust.x + cos_f32(self.ship.angle - 1.0) * 3.0,
                y1: fy + exhaust.y + sin_f32(self.ship.angle - 1.0) * 3.0,
                x2: fx + inner_tip.x,
                y2: fy + inner_tip.y,
                color: YELLOW,
                width: 1.0,
            });
        }
    }

    fn render_overlay(&self, cmds: &mut Vec<RenderCommand>, fx: f32, fy: f32) {
        match self.state {
            GameState::Paused => self.render_pause_overlay(cmds, fx, fy),
            GameState::GameOver => self.render_game_over_overlay(cmds, fx, fy),
            GameState::Playing => {}
        }
    }

    fn render_pause_overlay(&self, cmds: &mut Vec<RenderCommand>, fx: f32, fy: f32) {
        // Semi-transparent overlay.
        cmds.push(RenderCommand::FillRect {
            x: fx,
            y: fy,
            width: FIELD_WIDTH,
            height: FIELD_HEIGHT,
            color: Color::rgba(17, 17, 27, 180),
            corner_radii: CornerRadii::ZERO,
        });

        cmds.push(RenderCommand::Text {
            x: fx + FIELD_WIDTH / 2.0 - 50.0,
            y: fy + FIELD_HEIGHT / 2.0 - 20.0,
            text: String::from("PAUSED"),
            color: LAVENDER,
            font_size: TITLE_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        cmds.push(RenderCommand::Text {
            x: fx + FIELD_WIDTH / 2.0 - 90.0,
            y: fy + FIELD_HEIGHT / 2.0 + 10.0,
            text: String::from("Press P or Esc to resume"),
            color: SUBTEXT0,
            font_size: OVERLAY_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        cmds.push(RenderCommand::Text {
            x: fx + FIELD_WIDTH / 2.0 - 80.0,
            y: fy + FIELD_HEIGHT / 2.0 + 35.0,
            text: String::from("Press N for new game"),
            color: TEAL,
            font_size: OVERLAY_FONT_SIZE - 2.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }

    fn render_game_over_overlay(&self, cmds: &mut Vec<RenderCommand>, fx: f32, fy: f32) {
        // Dark overlay.
        cmds.push(RenderCommand::FillRect {
            x: fx,
            y: fy,
            width: FIELD_WIDTH,
            height: FIELD_HEIGHT,
            color: Color::rgba(17, 17, 27, 200),
            corner_radii: CornerRadii::ZERO,
        });

        // Game over box.
        let box_w = 300.0;
        let box_h = 180.0;
        let box_x = fx + (FIELD_WIDTH - box_w) / 2.0;
        let box_y = fy + (FIELD_HEIGHT - box_h) / 2.0;

        cmds.push(RenderCommand::FillRect {
            x: box_x,
            y: box_y,
            width: box_w,
            height: box_h,
            color: Color::from_hex(0x313244),
            corner_radii: CornerRadii::all(8.0),
        });

        cmds.push(RenderCommand::StrokeRect {
            x: box_x,
            y: box_y,
            width: box_w,
            height: box_h,
            color: RED,
            line_width: 2.0,
            corner_radii: CornerRadii::all(8.0),
        });

        cmds.push(RenderCommand::Text {
            x: box_x + 70.0,
            y: box_y + 20.0,
            text: String::from("GAME OVER"),
            color: RED,
            font_size: TITLE_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        cmds.push(RenderCommand::Text {
            x: box_x + 20.0,
            y: box_y + 60.0,
            text: format!("Score: {}", self.score),
            color: TEXT_COLOR,
            font_size: OVERLAY_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        cmds.push(RenderCommand::Text {
            x: box_x + 20.0,
            y: box_y + 85.0,
            text: format!("High Score: {}", self.high_score),
            color: YELLOW,
            font_size: OVERLAY_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        cmds.push(RenderCommand::Text {
            x: box_x + 20.0,
            y: box_y + 110.0,
            text: format!("Wave reached: {}", self.wave),
            color: LAVENDER,
            font_size: OVERLAY_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        cmds.push(RenderCommand::Text {
            x: box_x + 20.0,
            y: box_y + 145.0,
            text: String::from("Press N or Enter for new game"),
            color: SUBTEXT0,
            font_size: SMALL_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }

    // ── Event dispatch ──────────────────────────────────────────────

    fn handle_event(&mut self, event: Event) {
        match event {
            Event::Key(ke) => self.handle_key(ke.key, ke.pressed),
            Event::Tick { elapsed_ms } => self.handle_tick(elapsed_ms),
            _ => {}
        }
    }
}

fn main() {
    let _app = AsteroidsApp::new();
}

// ═══════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════
#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: create a game with a fixed seed for deterministic tests.
    fn test_app() -> AsteroidsApp {
        AsteroidsApp::with_seed(12345)
    }

    /// Helper: advance the game by a given number of milliseconds.
    fn tick(app: &mut AsteroidsApp, ms: u64) {
        app.handle_tick(ms);
    }

    /// Helper: advance game by several small ticks totalling `total_ms`.
    fn tick_many(app: &mut AsteroidsApp, total_ms: u64, step_ms: u64) {
        let mut remaining = total_ms;
        while remaining > 0 {
            let step = if remaining >= step_ms { step_ms } else { remaining };
            tick(app, step);
            remaining -= step;
        }
    }

    /// Helper: press and release a key.
    fn press_key(app: &mut AsteroidsApp, key: Key) {
        app.handle_key(key, true);
        app.handle_key(key, false);
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
    fn test_vec2_length_sq() {
        let v = Vec2::new(3.0, 4.0);
        assert!((v.length_sq() - 25.0).abs() < 0.001);
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

    // ── LCG ─────────────────────────────────────────────────────────

    #[test]
    fn test_lcg_deterministic() {
        let mut a = Lcg::new(42);
        let mut b = Lcg::new(42);
        for _ in 0..10 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    #[test]
    fn test_lcg_bounded() {
        let mut rng = Lcg::new(42);
        for _ in 0..100 {
            let val = rng.next_bounded(10);
            assert!(val < 10);
        }
    }

    #[test]
    fn test_lcg_f32_range() {
        let mut rng = Lcg::new(42);
        for _ in 0..100 {
            let val = rng.next_f32();
            assert!(val >= 0.0);
            assert!(val < 1.0);
        }
    }

    #[test]
    fn test_lcg_next_range() {
        let mut rng = Lcg::new(42);
        for _ in 0..100 {
            let val = rng.next_range(10.0, 20.0);
            assert!(val >= 10.0);
            assert!(val < 20.0);
        }
    }

    #[test]
    fn test_lcg_next_angle() {
        let mut rng = Lcg::new(42);
        for _ in 0..100 {
            let a = rng.next_angle();
            assert!(a >= 0.0);
            assert!(a < TAU);
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
        let mut b = Bullet::new(
            Vec2::new(FIELD_WIDTH - 1.0, 200.0),
            Vec2::new(500.0, 0.0),
        );
        b.update(0.1);
        // Should wrap around.
        assert!(b.pos.x < FIELD_WIDTH);
    }

    // ── Asteroid ────────────────────────────────────────────────────

    #[test]
    fn test_asteroid_creation() {
        let mut rng = Lcg::new(42);
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
        let mut rng = Lcg::new(42);
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
        let mut rng = Lcg::new(42);
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
        let mut rng = Lcg::new(42);
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
        let mut rng = Lcg::new(42);
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
        app.handle_key(Key::Left, true);
        assert!(app.input.left);
        app.handle_key(Key::Left, false);
        assert!(!app.input.left);
    }

    #[test]
    fn test_right_key_sets_input() {
        let mut app = test_app();
        app.handle_key(Key::Right, true);
        assert!(app.input.right);
    }

    #[test]
    fn test_up_key_sets_thrust() {
        let mut app = test_app();
        app.handle_key(Key::Up, true);
        assert!(app.input.thrust);
    }

    #[test]
    fn test_space_key_sets_shoot() {
        let mut app = test_app();
        app.handle_key(Key::Space, true);
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
        app.handle_key(Key::Left, true);
        app.handle_key(Key::Up, true);
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
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_contains_background() {
        let app = test_app();
        let cmds = app.render();
        // First command should be the background fill.
        matches!(&cmds[0], RenderCommand::FillRect { .. });
    }

    #[test]
    fn test_render_paused_overlay() {
        let mut app = test_app();
        app.state = GameState::Paused;
        let cmds = app.render();
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
        let cmds = app.render();
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
        let cmds = app.render();
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
        let cmds = app.render();
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
        let cmds = app.render();
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
        let cmds = app.render();
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
        assert!(a >= 0.0 && a < TAU);
        assert!((a - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_normalize_angle_negative() {
        let a = normalize_angle(-1.0);
        assert!(a >= 0.0 && a < TAU);
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
        let moved = (new_pos.x - old_pos.x).abs() > 0.01
            || (new_pos.y - old_pos.y).abs() > 0.01;
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
        app.handle_event(Event::Key(KeyEvent {
            key: Key::Left,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        }));
        assert!(app.input.left);
    }

    #[test]
    fn test_handle_event_key_up() {
        let mut app = test_app();
        app.handle_event(Event::Key(KeyEvent {
            key: Key::Left,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        }));
        assert!(app.input.left);
        app.handle_event(Event::Key(KeyEvent {
            key: Key::Left,
            pressed: false,
            modifiers: Modifiers::NONE,
            text: None,
        }));
        assert!(!app.input.left);
    }

    #[test]
    fn test_handle_event_tick() {
        let mut app = test_app();
        let old_pos = app.asteroids[0].pos;
        app.handle_event(Event::Tick { elapsed_ms: 100 });
        let new_pos = app.asteroids[0].pos;
        let moved = (new_pos.x - old_pos.x).abs() > 0.01
            || (new_pos.y - old_pos.y).abs() > 0.01;
        assert!(moved);
    }

    // ── WASD alternative controls ───────────────────────────────────

    #[test]
    fn test_wasd_a_key() {
        let mut app = test_app();
        app.handle_key(Key::A, true);
        assert!(app.input.left);
    }

    #[test]
    fn test_wasd_d_key() {
        let mut app = test_app();
        app.handle_key(Key::D, true);
        assert!(app.input.right);
    }

    #[test]
    fn test_wasd_w_key() {
        let mut app = test_app();
        app.handle_key(Key::W, true);
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
}
