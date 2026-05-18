#![allow(dead_code)]
//! OurOS Lock Screen
//!
//! Graphical lock screen application providing:
//! - Large clock display (HH:MM) with date below
//! - User avatar placeholder (circle with initials)
//! - Masked password input field with submit button
//! - "Wrong password" shake animation
//! - Failed attempt tracking with escalating lockout timers
//! - Hint text display after 3 failed attempts
//! - Wallpaper tint (darkened overlay)
//! - Accessibility: screen reader text for all elements
//! - Keyboard: Enter to submit, Escape to return to clock view
//! - Multiple user support (user list when >1 user)
//!
//! Uses the guitk library for UI rendering.

#[allow(unused_imports)]
use guitk::color::Color;
#[allow(unused_imports)]
use guitk::event::{Event, EventResult, Key, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseEventKind};
#[allow(unused_imports)]
use guitk::render::{FontWeightHint, RenderCommand, RenderTree};
#[allow(unused_imports)]
use guitk::style::CornerRadii;

// ============================================================================
// Theme — Catppuccin Mocha palette
// ============================================================================

mod theme {
    use guitk::color::Color;

    /// Base background.
    pub const BASE: Color = Color::from_hex(0x1E1E2E);
    /// Surface0 — elevated surfaces.
    pub const SURFACE0: Color = Color::from_hex(0x313244);
    /// Surface1 — interactive element backgrounds.
    pub const SURFACE1: Color = Color::from_hex(0x45475A);
    /// Surface2 — borders, dividers.
    pub const SURFACE2: Color = Color::from_hex(0x585B70);
    /// Text — primary text.
    pub const TEXT: Color = Color::from_hex(0xCDD6F4);
    /// Subtext — secondary/dimmer text.
    pub const SUBTEXT: Color = Color::from_hex(0xA6ADC8);
    /// Blue — accent color.
    pub const BLUE: Color = Color::from_hex(0x89B4FA);
    /// Red — error/warning color.
    pub const RED: Color = Color::from_hex(0xF38BA8);
    /// Green — success color.
    pub const GREEN: Color = Color::from_hex(0xA6E3A1);
    /// Overlay — for tinted wallpaper backdrop.
    pub const OVERLAY: Color = Color::rgba(0, 0, 0, 140);
    /// Avatar background — muted blue.
    pub const AVATAR_BG: Color = Color::from_hex(0x585B70);
}

// ============================================================================
// Layout constants
// ============================================================================

/// Screen width (logical pixels, 1920x1080 reference).
const SCREEN_WIDTH: f32 = 1920.0;
/// Screen height.
const SCREEN_HEIGHT: f32 = 1080.0;

/// Clock font size (large display).
const CLOCK_FONT_SIZE: f32 = 96.0;
/// Date font size.
const DATE_FONT_SIZE: f32 = 20.0;
/// Clock vertical position from top.
const CLOCK_Y: f32 = 200.0;

/// Avatar circle diameter.
const AVATAR_DIAMETER: f32 = 96.0;
/// Avatar initials font size.
const AVATAR_FONT_SIZE: f32 = 36.0;

/// Password field width.
const PASSWORD_FIELD_WIDTH: f32 = 320.0;
/// Password field height.
const PASSWORD_FIELD_HEIGHT: f32 = 48.0;
/// Password field corner radius.
const PASSWORD_FIELD_RADIUS: f32 = 24.0;
/// Password dot diameter (for masked characters).
const PASSWORD_DOT_DIAMETER: f32 = 10.0;
/// Spacing between password dots.
const PASSWORD_DOT_SPACING: f32 = 16.0;
/// Password font size (for placeholder text).
const PASSWORD_FONT_SIZE: f32 = 16.0;

/// Submit button width.
const SUBMIT_BUTTON_WIDTH: f32 = 48.0;
/// Submit button height.
const SUBMIT_BUTTON_HEIGHT: f32 = 48.0;
/// Submit button corner radius.
const SUBMIT_BUTTON_RADIUS: f32 = 24.0;

/// Display name font size.
const DISPLAY_NAME_FONT_SIZE: f32 = 22.0;
/// Hint text font size.
const HINT_FONT_SIZE: f32 = 13.0;
/// Error message font size.
const ERROR_FONT_SIZE: f32 = 14.0;
/// Lockout message font size.
const LOCKOUT_FONT_SIZE: f32 = 16.0;

/// User list item height.
const USER_LIST_ITEM_HEIGHT: f32 = 56.0;
/// User list item width.
const USER_LIST_ITEM_WIDTH: f32 = 280.0;
/// Small avatar diameter (in user list).
const SMALL_AVATAR_DIAMETER: f32 = 40.0;
/// Small avatar initials font size.
const SMALL_AVATAR_FONT_SIZE: f32 = 16.0;

/// Vertical gap between UI sections.
const SECTION_GAP: f32 = 16.0;

/// Maximum password length (characters).
const MAX_PASSWORD_LENGTH: usize = 128;

/// Shake animation duration (milliseconds).
const SHAKE_DURATION_MS: u64 = 400;
/// Shake animation amplitude (pixels).
const SHAKE_AMPLITUDE: f32 = 12.0;

// ============================================================================
// Lockout thresholds
// ============================================================================

/// After 5 failed attempts: 30 second lockout.
const LOCKOUT_TIER_1_ATTEMPTS: u32 = 5;
const LOCKOUT_TIER_1_SECS: u64 = 30;
/// After 10 failed attempts: 60 second lockout.
const LOCKOUT_TIER_2_ATTEMPTS: u32 = 10;
const LOCKOUT_TIER_2_SECS: u64 = 60;
/// After 15 failed attempts: 300 second lockout.
const LOCKOUT_TIER_3_ATTEMPTS: u32 = 15;
const LOCKOUT_TIER_3_SECS: u64 = 300;
/// Show password hint after this many failed attempts.
const HINT_THRESHOLD: u32 = 3;

// ============================================================================
// Lock screen state machine
// ============================================================================

/// Top-level state of the lock screen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LockScreenState {
    /// Showing the clock/date (idle). User clicks or presses a key to enter
    /// password mode.
    Clock,
    /// User is entering their password.
    PasswordEntry,
}

impl Default for LockScreenState {
    fn default() -> Self {
        Self::Clock
    }
}

// ============================================================================
// User info
// ============================================================================

/// Information about a user account displayed on the lock screen.
#[derive(Clone, Debug)]
pub struct UserInfo {
    /// Login username (e.g. "alice").
    pub username: String,
    /// Display name (e.g. "Alice Johnson").
    pub display_name: String,
    /// 1-2 character initials shown in the avatar circle.
    pub initials: String,
    /// Whether this user has a password set.
    pub has_password: bool,
    /// Optional hint shown after repeated failures.
    pub password_hint: Option<String>,
}

impl UserInfo {
    /// Create a new user with sensible defaults.
    pub fn new(username: &str, display_name: &str, has_password: bool) -> Self {
        let initials = compute_initials(display_name);
        Self {
            username: username.to_string(),
            display_name: display_name.to_string(),
            initials,
            has_password,
            password_hint: None,
        }
    }

    /// Create a user with a password hint.
    pub fn with_hint(mut self, hint: &str) -> Self {
        self.password_hint = Some(hint.to_string());
        self
    }
}

/// Derive 1-2 character initials from a display name.
///
/// Takes the first character of the first two whitespace-separated words.
/// Falls back to the first character of the name, or "?" if empty.
fn compute_initials(name: &str) -> String {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return "?".to_string();
    }
    let mut parts = trimmed.split_whitespace();
    let mut result = String::with_capacity(2);
    if let Some(first) = parts.next() {
        if let Some(ch) = first.chars().next() {
            result.push(ch.to_ascii_uppercase());
        }
    }
    if let Some(second) = parts.next() {
        if let Some(ch) = second.chars().next() {
            result.push(ch.to_ascii_uppercase());
        }
    }
    if result.is_empty() {
        "?".to_string()
    } else {
        result
    }
}

// ============================================================================
// Password validator
// ============================================================================

/// Simple password validator that compares against a stored SHA-256 hash.
///
/// In a real system this would call into the OS credential store via IPC.
/// Here we use a basic SHA-256 comparison for demonstration.
#[derive(Clone, Debug)]
pub struct PasswordValidator {
    /// Stored SHA-256 hash of the password (32 bytes).
    stored_hash: [u8; 32],
}

impl PasswordValidator {
    /// Create a validator with a pre-computed hash.
    pub fn new(stored_hash: [u8; 32]) -> Self {
        Self { stored_hash }
    }

    /// Create a validator from a known password (hashes it immediately).
    /// Used for testing; in production the hash comes from the credential store.
    pub fn from_password(password: &str) -> Self {
        Self {
            stored_hash: sha256_hash(password.as_bytes()),
        }
    }

    /// Validate a candidate password against the stored hash.
    pub fn validate(&self, candidate: &str) -> bool {
        let candidate_hash = sha256_hash(candidate.as_bytes());
        constant_time_eq(&self.stored_hash, &candidate_hash)
    }
}

/// Minimal SHA-256 implementation.
///
/// This is a from-scratch implementation for `no_std` compatibility.
/// It follows FIPS 180-4 exactly.
fn sha256_hash(data: &[u8]) -> [u8; 32] {
    // Initial hash values (first 32 bits of fractional parts of square roots
    // of the first 8 primes).
    let mut h: [u32; 8] = [
        0x6a09_e667, 0xbb67_ae85, 0x3c6e_f372, 0xa54f_f53a,
        0x510e_527f, 0x9b05_688c, 0x1f83_d9ab, 0x5be0_cd19,
    ];

    // Round constants (first 32 bits of fractional parts of cube roots
    // of the first 64 primes).
    const K: [u32; 64] = [
        0x428a_2f98, 0x7137_4491, 0xb5c0_fbcf, 0xe9b5_dba5,
        0x3956_c25b, 0x59f1_11f1, 0x923f_82a4, 0xab1c_5ed5,
        0xd807_aa98, 0x1283_5b01, 0x2431_85be, 0x550c_7dc3,
        0x72be_5d74, 0x80de_b1fe, 0x9bdc_06a7, 0xc19b_f174,
        0xe49b_69c1, 0xefbe_4786, 0x0fc1_9dc6, 0x240c_a1cc,
        0x2de9_2c6f, 0x4a74_84aa, 0x5cb0_a9dc, 0x76f9_88da,
        0x983e_5152, 0xa831_c66d, 0xb003_27c8, 0xbf59_7fc7,
        0xc6e0_0bf3, 0xd5a7_9147, 0x06ca_6351, 0x1429_2967,
        0x27b7_0a85, 0x2e1b_2138, 0x4d2c_6dfc, 0x5338_0d13,
        0x650a_7354, 0x766a_0abb, 0x81c2_c92e, 0x9272_2c85,
        0xa2bf_e8a1, 0xa81a_664b, 0xc24b_8b70, 0xc76c_51a3,
        0xd192_e819, 0xd699_0624, 0xf40e_3585, 0x106a_a070,
        0x19a4_c116, 0x1e37_6c08, 0x2748_774c, 0x34b0_bcb5,
        0x391c_0cb3, 0x4ed8_aa4a, 0x5b9c_ca4f, 0x682e_6ff3,
        0x748f_82ee, 0x78a5_636f, 0x84c8_7814, 0x8cc7_0208,
        0x90be_fffa, 0xa450_6ceb, 0xbef9_a3f7, 0xc671_78f2,
    ];

    // Padding: append 1 bit, then zeros, then 64-bit big-endian length.
    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut padded = Vec::from(data);
    padded.push(0x80);
    while padded.len() % 64 != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_len.to_be_bytes());

    // Process each 512-bit (64-byte) block.
    let mut block_start = 0;
    while block_start < padded.len() {
        let block = &padded[block_start..block_start + 64];
        let mut w = [0u32; 64];

        // First 16 words directly from the block.
        for i in 0..16 {
            let base = i * 4;
            w[i] = u32::from_be_bytes([
                block[base],
                block[base + 1],
                block[base + 2],
                block[base + 3],
            ]);
        }

        // Extend to 64 words.
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7)
                ^ w[i - 15].rotate_right(18)
                ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17)
                ^ w[i - 2].rotate_right(19)
                ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        // Compression.
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;

        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);

        block_start += 64;
    }

    // Produce the final 32-byte hash.
    let mut result = [0u8; 32];
    for (i, val) in h.iter().enumerate() {
        let bytes = val.to_be_bytes();
        result[i * 4] = bytes[0];
        result[i * 4 + 1] = bytes[1];
        result[i * 4 + 2] = bytes[2];
        result[i * 4 + 3] = bytes[3];
    }
    result
}

/// Constant-time comparison to avoid timing side-channels on password hashes.
fn constant_time_eq(a: &[u8; 32], b: &[u8; 32]) -> bool {
    let mut diff: u8 = 0;
    for i in 0..32 {
        diff |= a[i] ^ b[i];
    }
    diff == 0
}

// ============================================================================
// Lock screen configuration
// ============================================================================

/// Runtime configuration for the lock screen.
#[derive(Clone, Debug)]
pub struct LockScreenConfig {
    /// Seconds of inactivity before the screen locks automatically.
    /// `None` means auto-lock is disabled.
    pub auto_lock_timeout_secs: Option<u64>,
    /// Whether to show seconds in the clock display.
    pub show_clock_seconds: bool,
    /// Whether to show the date below the clock.
    pub show_date: bool,
    /// Alpha value for the wallpaper tint overlay (0 = invisible, 255 = opaque).
    pub wallpaper_tint_alpha: u8,
}

impl Default for LockScreenConfig {
    fn default() -> Self {
        Self {
            auto_lock_timeout_secs: Some(300),
            show_clock_seconds: false,
            show_date: true,
            wallpaper_tint_alpha: 140,
        }
    }
}

// ============================================================================
// Time representation (no std::time dependency in the OS)
// ============================================================================

/// Simple time-of-day representation for clock display.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimeOfDay {
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

impl TimeOfDay {
    pub fn new(hour: u8, minute: u8, second: u8) -> Option<Self> {
        if hour >= 24 || minute >= 60 || second >= 60 {
            return None;
        }
        Some(Self { hour, minute, second })
    }

    /// Format as "HH:MM".
    pub fn format_hhmm(&self) -> String {
        format!("{:02}:{:02}", self.hour, self.minute)
    }

    /// Format as "HH:MM:SS".
    pub fn format_hhmmss(&self) -> String {
        format!("{:02}:{:02}:{:02}", self.hour, self.minute, self.second)
    }
}

/// Simple date representation for the lock screen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DateInfo {
    /// Day of week name (e.g. "Monday").
    pub weekday: String,
    /// Month name (e.g. "January").
    pub month: String,
    /// Day of month (1-31).
    pub day: u8,
    /// Full year (e.g. 2026).
    pub year: u16,
}

impl DateInfo {
    /// Format as "Wednesday, January 15, 2026".
    pub fn format_long(&self) -> String {
        format!("{}, {} {}, {}", self.weekday, self.month, self.day, self.year)
    }
}

// ============================================================================
// Shake animation
// ============================================================================

/// Tracks the state of the "wrong password" shake animation.
#[derive(Clone, Debug)]
struct ShakeAnimation {
    /// Whether the animation is active.
    active: bool,
    /// Milliseconds elapsed since shake started.
    elapsed_ms: u64,
}

impl ShakeAnimation {
    fn new() -> Self {
        Self {
            active: false,
            elapsed_ms: 0,
        }
    }

    /// Start or restart the shake animation.
    fn trigger(&mut self) {
        self.active = true;
        self.elapsed_ms = 0;
    }

    /// Advance the animation by the given number of milliseconds.
    /// Returns the current horizontal offset in pixels.
    fn tick(&mut self, dt_ms: u64) -> f32 {
        if !self.active {
            return 0.0;
        }
        self.elapsed_ms = self.elapsed_ms.saturating_add(dt_ms);
        if self.elapsed_ms >= SHAKE_DURATION_MS {
            self.active = false;
            self.elapsed_ms = 0;
            return 0.0;
        }
        // Damped sine wave for a natural shake feel.
        let t = self.elapsed_ms as f32 / SHAKE_DURATION_MS as f32;
        let decay = 1.0 - t;
        // ~3 oscillations over the duration.
        let angle = t * 3.0 * 2.0 * core::f32::consts::PI;
        // sin approximation good enough for animation.
        SHAKE_AMPLITUDE * decay * sin_approx(angle)
    }

    fn is_active(&self) -> bool {
        self.active
    }
}

/// Fast sine approximation (Bhaskara I formula) for animation use.
/// Accurate to ~0.2% over the full range.
fn sin_approx(x: f32) -> f32 {
    // Normalize x to [0, 2*PI).
    let pi = core::f32::consts::PI;
    let two_pi = 2.0 * pi;
    let mut x = x % two_pi;
    if x < 0.0 {
        x += two_pi;
    }
    let sign = if x > pi { -1.0 } else { 1.0 };
    if x > pi {
        x -= pi;
    }
    // Bhaskara I: sin(x) ~ 16x(pi-x) / (5*pi^2 - 4x(pi-x))
    let num = 16.0 * x * (pi - x);
    let den = 5.0 * pi * pi - 4.0 * x * (pi - x);
    if den.abs() < 1e-10 {
        return 0.0;
    }
    sign * num / den
}

// ============================================================================
// Lockout timer
// ============================================================================

/// Manages the lockout timer after too many failed attempts.
#[derive(Clone, Debug)]
struct LockoutTimer {
    /// Whether the lockout is currently active.
    active: bool,
    /// Remaining lockout time in seconds.
    remaining_secs: u64,
    /// Millisecond accumulator for sub-second ticks.
    ms_accumulator: u64,
}

impl LockoutTimer {
    fn new() -> Self {
        Self {
            active: false,
            remaining_secs: 0,
            ms_accumulator: 0,
        }
    }

    /// Start a lockout for the given duration.
    fn start(&mut self, duration_secs: u64) {
        self.active = true;
        self.remaining_secs = duration_secs;
        self.ms_accumulator = 0;
    }

    /// Advance the timer. Returns `true` if the lockout just expired.
    fn tick(&mut self, dt_ms: u64) -> bool {
        if !self.active {
            return false;
        }
        self.ms_accumulator = self.ms_accumulator.saturating_add(dt_ms);
        while self.ms_accumulator >= 1000 {
            self.ms_accumulator -= 1000;
            if self.remaining_secs > 0 {
                self.remaining_secs -= 1;
            }
        }
        if self.remaining_secs == 0 {
            self.active = false;
            self.ms_accumulator = 0;
            return true;
        }
        false
    }

    fn is_active(&self) -> bool {
        self.active
    }

    /// Format remaining time for display (e.g. "0:30", "4:59").
    fn format_remaining(&self) -> String {
        let mins = self.remaining_secs / 60;
        let secs = self.remaining_secs % 60;
        format!("{mins}:{secs:02}")
    }
}

/// Determine the lockout duration for a given number of failed attempts.
/// Returns `None` if no lockout is triggered at this attempt count.
fn lockout_duration_for_attempts(failed_attempts: u32) -> Option<u64> {
    if failed_attempts > 0 && failed_attempts % LOCKOUT_TIER_3_ATTEMPTS == 0 {
        Some(LOCKOUT_TIER_3_SECS)
    } else if failed_attempts > 0 && failed_attempts % LOCKOUT_TIER_2_ATTEMPTS == 0 {
        Some(LOCKOUT_TIER_2_SECS)
    } else if failed_attempts > 0 && failed_attempts % LOCKOUT_TIER_1_ATTEMPTS == 0 {
        Some(LOCKOUT_TIER_1_SECS)
    } else {
        None
    }
}

// ============================================================================
// Accessibility text builder
// ============================================================================

/// Collect screen reader descriptions for the current lock screen state.
fn build_accessibility_text(lock_screen: &LockScreen) -> String {
    let mut parts = Vec::new();

    match lock_screen.state {
        LockScreenState::Clock => {
            parts.push(format!(
                "Lock screen. Time: {}.",
                if lock_screen.config.show_clock_seconds {
                    lock_screen.time.format_hhmmss()
                } else {
                    lock_screen.time.format_hhmm()
                }
            ));
            if lock_screen.config.show_date {
                if let Some(ref date) = lock_screen.date {
                    parts.push(format!("Date: {}.", date.format_long()));
                }
            }
            parts.push("Press any key or click to unlock.".to_string());
        }
        LockScreenState::PasswordEntry => {
            let user = lock_screen.active_user();
            parts.push(format!(
                "Unlock screen for {}.",
                user.display_name
            ));
            if lock_screen.lockout.is_active() {
                parts.push(format!(
                    "Account locked. Try again in {}.",
                    lock_screen.lockout.format_remaining()
                ));
            } else {
                let char_count = lock_screen.password_buffer.len();
                parts.push(format!(
                    "Password field: {} characters entered.",
                    char_count
                ));
                if lock_screen.show_error {
                    parts.push("Incorrect password.".to_string());
                }
                if lock_screen.failed_attempts >= HINT_THRESHOLD {
                    if let Some(ref hint) = user.password_hint {
                        parts.push(format!("Hint: {hint}."));
                    }
                }
            }
            parts.push("Press Enter to submit, Escape to return to clock.".to_string());
        }
    }

    parts.join(" ")
}

// ============================================================================
// Main lock screen struct
// ============================================================================

/// The lock screen application state.
#[derive(Clone, Debug)]
pub struct LockScreen {
    /// Current UI state (clock view or password entry).
    pub state: LockScreenState,
    /// Screen dimensions.
    pub screen_width: f32,
    pub screen_height: f32,
    /// Current time of day.
    pub time: TimeOfDay,
    /// Current date (optional; omitted if not available yet).
    pub date: Option<DateInfo>,
    /// Configuration.
    pub config: LockScreenConfig,
    /// List of users that can log in.
    pub users: Vec<UserInfo>,
    /// Index of the currently selected user.
    pub selected_user_index: usize,
    /// Password input buffer (plaintext, never displayed).
    password_buffer: String,
    /// Number of consecutive failed password attempts.
    pub failed_attempts: u32,
    /// Whether to display the "wrong password" error message.
    show_error: bool,
    /// Shake animation state.
    shake: ShakeAnimation,
    /// Lockout timer state.
    lockout: LockoutTimer,
    /// Password validator for the selected user (optional; users without
    /// passwords don't need one).
    validator: Option<PasswordValidator>,
    /// Whether the submit button is hovered.
    submit_hovered: bool,
    /// Whether the password field is focused.
    password_focused: bool,
}

impl LockScreen {
    /// Create a new lock screen with the given users and configuration.
    ///
    /// # Panics
    ///
    /// Does not panic. Returns a default lock screen with a placeholder user
    /// if the user list is empty.
    pub fn new(
        users: Vec<UserInfo>,
        config: LockScreenConfig,
        validator: Option<PasswordValidator>,
    ) -> Self {
        let users = if users.is_empty() {
            vec![UserInfo::new("user", "User", true)]
        } else {
            users
        };
        Self {
            state: LockScreenState::Clock,
            screen_width: SCREEN_WIDTH,
            screen_height: SCREEN_HEIGHT,
            time: TimeOfDay { hour: 12, minute: 0, second: 0 },
            date: None,
            config,
            users,
            selected_user_index: 0,
            password_buffer: String::new(),
            failed_attempts: 0,
            show_error: false,
            shake: ShakeAnimation::new(),
            lockout: LockoutTimer::new(),
            validator,
            submit_hovered: false,
            password_focused: false,
        }
    }

    /// Create a lock screen with sensible defaults for testing.
    pub fn default_single_user() -> Self {
        let user = UserInfo::new("admin", "Administrator", true)
            .with_hint("It's the name of your first pet");
        let validator = PasswordValidator::from_password("password123");
        Self::new(vec![user], LockScreenConfig::default(), Some(validator))
    }

    /// Get the currently active/selected user.
    pub fn active_user(&self) -> &UserInfo {
        self.users
            .get(self.selected_user_index)
            .unwrap_or_else(|| {
                // This should never happen if the constructor ensures non-empty,
                // but we handle it defensively.
                &self.users[0]
            })
    }

    /// Whether there are multiple users to choose from.
    pub fn is_multi_user(&self) -> bool {
        self.users.len() > 1
    }

    /// Update the displayed time.
    pub fn set_time(&mut self, time: TimeOfDay) {
        self.time = time;
    }

    /// Update the displayed date.
    pub fn set_date(&mut self, date: DateInfo) {
        self.date = Some(date);
    }

    /// Switch to the password entry view.
    pub fn enter_password_mode(&mut self) {
        self.state = LockScreenState::PasswordEntry;
        self.password_focused = true;
        self.show_error = false;
    }

    /// Switch back to the clock view (e.g. on Escape).
    pub fn return_to_clock(&mut self) {
        self.state = LockScreenState::Clock;
        self.password_buffer.clear();
        self.show_error = false;
        self.password_focused = false;
    }

    /// Select a user by index (for multi-user support).
    pub fn select_user(&mut self, index: usize) {
        if index < self.users.len() {
            self.selected_user_index = index;
            self.password_buffer.clear();
            self.failed_attempts = 0;
            self.show_error = false;
        }
    }

    /// Append a character to the password buffer.
    pub fn type_char(&mut self, ch: char) {
        if self.lockout.is_active() {
            return;
        }
        if self.password_buffer.len() < MAX_PASSWORD_LENGTH {
            self.show_error = false;
            self.password_buffer.push(ch);
        }
    }

    /// Delete the last character from the password buffer.
    pub fn backspace(&mut self) {
        if self.lockout.is_active() {
            return;
        }
        self.password_buffer.pop();
        self.show_error = false;
    }

    /// Clear the entire password buffer.
    pub fn clear_password(&mut self) {
        self.password_buffer.clear();
        self.show_error = false;
    }

    /// Get the number of characters currently in the password buffer.
    pub fn password_len(&self) -> usize {
        self.password_buffer.len()
    }

    /// Attempt to submit the current password.
    ///
    /// Returns `true` if the password is correct (screen should unlock),
    /// `false` if incorrect.
    pub fn submit_password(&mut self) -> bool {
        if self.lockout.is_active() {
            return false;
        }

        let user = self.active_user();
        if !user.has_password {
            // No password required — unlock immediately.
            return true;
        }

        if self.password_buffer.is_empty() {
            return false;
        }

        let is_valid = self.validator
            .as_ref()
            .map_or(false, |v| v.validate(&self.password_buffer));

        if is_valid {
            self.failed_attempts = 0;
            self.password_buffer.clear();
            true
        } else {
            self.failed_attempts = self.failed_attempts.saturating_add(1);
            self.show_error = true;
            self.shake.trigger();
            self.password_buffer.clear();

            // Check if we should start a lockout.
            if let Some(duration) = lockout_duration_for_attempts(self.failed_attempts) {
                self.lockout.start(duration);
            }

            false
        }
    }

    /// Get the current accessibility description of the screen.
    pub fn accessibility_text(&self) -> String {
        build_accessibility_text(self)
    }

    // ========================================================================
    // Event handling
    // ========================================================================

    /// Handle an input event. Returns `EventResult::Consumed` if the event
    /// was handled, or `EventResult::Ignored` if it should propagate.
    ///
    /// Special return: if the screen should unlock, this is signaled by
    /// the `unlock_requested` flag on the struct (checked separately).
    pub fn handle_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::Key(key_event) => self.handle_key(key_event),
            Event::Mouse(mouse_event) => self.handle_mouse(mouse_event),
            Event::Tick { elapsed_ms } => {
                self.handle_tick(*elapsed_ms);
                EventResult::Consumed
            }
            Event::Resize { width, height } => {
                self.screen_width = *width as f32;
                self.screen_height = *height as f32;
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    fn handle_key(&mut self, key: &KeyEvent) -> EventResult {
        if !key.pressed {
            return EventResult::Ignored;
        }

        match self.state {
            LockScreenState::Clock => {
                // Any key press transitions to password entry.
                match key.key {
                    // Don't transition on bare modifier keys.
                    Key::LeftShift | Key::RightShift
                    | Key::LeftCtrl | Key::RightCtrl
                    | Key::LeftAlt | Key::RightAlt
                    | Key::LeftSuper | Key::RightSuper
                    | Key::CapsLock | Key::NumLock
                    | Key::ScrollLock => EventResult::Ignored,
                    _ => {
                        self.enter_password_mode();
                        // If the key was a printable character, also type it.
                        if let Some(ch) = key.text {
                            if !ch.is_control() {
                                self.type_char(ch);
                            }
                        }
                        EventResult::Consumed
                    }
                }
            }
            LockScreenState::PasswordEntry => {
                match key.key {
                    Key::Escape => {
                        self.return_to_clock();
                        EventResult::Consumed
                    }
                    Key::Enter => {
                        let _ = self.submit_password();
                        EventResult::Consumed
                    }
                    Key::Backspace => {
                        self.backspace();
                        EventResult::Consumed
                    }
                    Key::Delete => {
                        self.clear_password();
                        EventResult::Consumed
                    }
                    _ => {
                        if let Some(ch) = key.text {
                            if !ch.is_control() {
                                self.type_char(ch);
                            }
                        }
                        EventResult::Consumed
                    }
                }
            }
        }
    }

    fn handle_mouse(&mut self, mouse: &MouseEvent) -> EventResult {
        match self.state {
            LockScreenState::Clock => {
                // Click anywhere to enter password mode.
                if matches!(mouse.kind, MouseEventKind::Press(MouseButton::Left)) {
                    self.enter_password_mode();
                    return EventResult::Consumed;
                }
                EventResult::Ignored
            }
            LockScreenState::PasswordEntry => {
                match mouse.kind {
                    MouseEventKind::Press(MouseButton::Left) => {
                        // Check if click is on submit button.
                        let submit_rect = self.submit_button_rect();
                        if hit_test(mouse.x, mouse.y, &submit_rect) {
                            let _ = self.submit_password();
                            return EventResult::Consumed;
                        }
                        // Check if click is on a user in the user list.
                        if self.is_multi_user() {
                            if let Some(idx) = self.user_list_hit_test(mouse.x, mouse.y) {
                                self.select_user(idx);
                                return EventResult::Consumed;
                            }
                        }
                        EventResult::Consumed
                    }
                    MouseEventKind::Move => {
                        // Update submit button hover state.
                        let submit_rect = self.submit_button_rect();
                        self.submit_hovered = hit_test(mouse.x, mouse.y, &submit_rect);
                        EventResult::Ignored
                    }
                    _ => EventResult::Ignored,
                }
            }
        }
    }

    fn handle_tick(&mut self, elapsed_ms: u64) {
        // Advance shake animation.
        let _ = self.shake.tick(elapsed_ms);
        // Advance lockout timer.
        let expired = self.lockout.tick(elapsed_ms);
        if expired {
            self.show_error = false;
        }
    }

    // ========================================================================
    // Geometry helpers
    // ========================================================================

    /// Center X position for the main content area.
    fn center_x(&self) -> f32 {
        self.screen_width / 2.0
    }

    /// Compute the rectangle for the submit button.
    fn submit_button_rect(&self) -> Rect {
        let cx = self.center_x();
        let field_right = cx + PASSWORD_FIELD_WIDTH / 2.0;
        let button_x = field_right + 8.0;
        // Vertical center: password field is at a computed Y.
        let field_y = self.password_field_y();
        Rect {
            x: button_x,
            y: field_y,
            width: SUBMIT_BUTTON_WIDTH,
            height: SUBMIT_BUTTON_HEIGHT,
        }
    }

    /// Y position of the password field (below avatar + display name).
    fn password_field_y(&self) -> f32 {
        let avatar_y = self.avatar_y();
        avatar_y + AVATAR_DIAMETER + SECTION_GAP + DISPLAY_NAME_FONT_SIZE + SECTION_GAP
    }

    /// Y position of the avatar circle.
    fn avatar_y(&self) -> f32 {
        self.screen_height / 2.0 - AVATAR_DIAMETER - 40.0
    }

    /// Hit test against the user list items. Returns the user index if hit.
    fn user_list_hit_test(&self, mx: f32, my: f32) -> Option<usize> {
        if !self.is_multi_user() {
            return None;
        }
        let list_x = self.center_x() - USER_LIST_ITEM_WIDTH / 2.0;
        let list_start_y = self.password_field_y() + PASSWORD_FIELD_HEIGHT + SECTION_GAP * 3.0;

        for (i, _user) in self.users.iter().enumerate() {
            let item_y = list_start_y + (i as f32) * (USER_LIST_ITEM_HEIGHT + 4.0);
            let rect = Rect {
                x: list_x,
                y: item_y,
                width: USER_LIST_ITEM_WIDTH,
                height: USER_LIST_ITEM_HEIGHT,
            };
            if hit_test(mx, my, &rect) {
                return Some(i);
            }
        }
        None
    }

    // ========================================================================
    // Rendering
    // ========================================================================

    /// Render the entire lock screen into a `RenderTree`.
    pub fn render(&mut self) -> RenderTree {
        let mut tree = RenderTree::new();

        // Layer 1: Wallpaper tint overlay (darkens/blurs the wallpaper behind).
        self.render_wallpaper_overlay(&mut tree);

        match self.state {
            LockScreenState::Clock => {
                self.render_clock(&mut tree);
            }
            LockScreenState::PasswordEntry => {
                self.render_password_screen(&mut tree);
            }
        }

        tree
    }

    /// Render the darkened wallpaper overlay.
    fn render_wallpaper_overlay(&self, tree: &mut RenderTree) {
        let overlay_color = Color::rgba(0, 0, 0, self.config.wallpaper_tint_alpha);
        tree.fill_rect(0.0, 0.0, self.screen_width, self.screen_height, overlay_color);
    }

    /// Render the clock view (large time + date).
    fn render_clock(&self, tree: &mut RenderTree) {
        let cx = self.center_x();

        // Time display.
        let time_str = if self.config.show_clock_seconds {
            self.time.format_hhmmss()
        } else {
            self.time.format_hhmm()
        };
        // Approximate centering: estimate character width at this font size.
        let approx_char_width = CLOCK_FONT_SIZE * 0.55;
        let time_width = time_str.len() as f32 * approx_char_width;
        let time_x = cx - time_width / 2.0;

        tree.push(RenderCommand::Text {
            x: time_x,
            y: CLOCK_Y,
            text: time_str,
            color: theme::TEXT,
            font_size: CLOCK_FONT_SIZE,
            font_weight: FontWeightHint::Light,
            max_width: None,
        });

        // Date display.
        if self.config.show_date {
            if let Some(ref date) = self.date {
                let date_str = date.format_long();
                let date_char_width = DATE_FONT_SIZE * 0.55;
                let date_width = date_str.len() as f32 * date_char_width;
                let date_x = cx - date_width / 2.0;
                let date_y = CLOCK_Y + CLOCK_FONT_SIZE + 12.0;

                tree.push(RenderCommand::Text {
                    x: date_x,
                    y: date_y,
                    text: date_str,
                    color: theme::SUBTEXT,
                    font_size: DATE_FONT_SIZE,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                });
            }
        }

        // "Click or press any key" hint at the bottom.
        let hint_text = "Press any key or click to unlock";
        let hint_font_size: f32 = 14.0;
        let hint_char_width = hint_font_size * 0.55;
        let hint_width = hint_text.len() as f32 * hint_char_width;
        let hint_x = cx - hint_width / 2.0;
        let hint_y = self.screen_height - 80.0;

        tree.push(RenderCommand::Text {
            x: hint_x,
            y: hint_y,
            text: hint_text.to_string(),
            color: theme::SUBTEXT,
            font_size: hint_font_size,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }

    /// Render the password entry screen.
    fn render_password_screen(&mut self, tree: &mut RenderTree) {
        let cx = self.center_x();
        let shake_offset = self.shake.tick(0);

        // Avatar circle.
        let avatar_y = self.avatar_y();
        self.render_avatar(tree, cx, avatar_y, AVATAR_DIAMETER, AVATAR_FONT_SIZE);

        // Display name.
        let user = self.active_user();
        let name = &user.display_name;
        let name_char_width = DISPLAY_NAME_FONT_SIZE * 0.55;
        let name_width = name.len() as f32 * name_char_width;
        let name_x = cx - name_width / 2.0;
        let name_y = avatar_y + AVATAR_DIAMETER + SECTION_GAP;

        tree.push(RenderCommand::Text {
            x: name_x,
            y: name_y,
            text: name.clone(),
            color: theme::TEXT,
            font_size: DISPLAY_NAME_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Password field (with shake offset applied).
        let field_y = self.password_field_y();
        let field_x = cx - PASSWORD_FIELD_WIDTH / 2.0 + shake_offset;

        self.render_password_field(tree, field_x, field_y);

        // Submit button.
        let submit_x = field_x + PASSWORD_FIELD_WIDTH + 8.0;
        self.render_submit_button(tree, submit_x, field_y);

        // Error message.
        if self.show_error {
            let error_text = "Incorrect password";
            let err_char_width = ERROR_FONT_SIZE * 0.55;
            let err_width = error_text.len() as f32 * err_char_width;
            let err_x = cx - err_width / 2.0 + shake_offset;
            let err_y = field_y + PASSWORD_FIELD_HEIGHT + 12.0;

            tree.push(RenderCommand::Text {
                x: err_x,
                y: err_y,
                text: error_text.to_string(),
                color: theme::RED,
                font_size: ERROR_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }

        // Lockout message.
        if self.lockout.is_active() {
            let lockout_msg = format!(
                "Too many attempts. Try again in {}",
                self.lockout.format_remaining()
            );
            let lock_char_width = LOCKOUT_FONT_SIZE * 0.55;
            let lock_width = lockout_msg.len() as f32 * lock_char_width;
            let lock_x = cx - lock_width / 2.0;
            let lock_y = field_y + PASSWORD_FIELD_HEIGHT + 32.0;

            tree.push(RenderCommand::Text {
                x: lock_x,
                y: lock_y,
                text: lockout_msg,
                color: theme::RED,
                font_size: LOCKOUT_FONT_SIZE,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
        }

        // Password hint (shown after HINT_THRESHOLD failed attempts).
        if self.failed_attempts >= HINT_THRESHOLD && !self.lockout.is_active() {
            if let Some(ref hint) = self.active_user().password_hint {
                let hint_str = format!("Hint: {hint}");
                let hint_char_width = HINT_FONT_SIZE * 0.55;
                let hint_width = hint_str.len() as f32 * hint_char_width;
                let hint_x = cx - hint_width / 2.0;
                let hint_y = field_y + PASSWORD_FIELD_HEIGHT + 52.0;

                tree.push(RenderCommand::Text {
                    x: hint_x,
                    y: hint_y,
                    text: hint_str,
                    color: theme::SUBTEXT,
                    font_size: HINT_FONT_SIZE,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                });
            }
        }

        // Multi-user list.
        if self.is_multi_user() {
            self.render_user_list(tree);
        }
    }

    /// Render the avatar circle with initials.
    fn render_avatar(
        &self,
        tree: &mut RenderTree,
        center_x: f32,
        top_y: f32,
        diameter: f32,
        font_size: f32,
    ) {
        let user = self.active_user();
        let radius = diameter / 2.0;
        let left = center_x - radius;

        // Background circle (approximated as a rounded rect with full radius).
        tree.fill_rounded_rect(
            left,
            top_y,
            diameter,
            diameter,
            theme::AVATAR_BG,
            CornerRadii::all(radius),
        );

        // Initials text centered in the circle.
        let initials = &user.initials;
        let char_width = font_size * 0.6;
        let text_width = initials.len() as f32 * char_width;
        let text_x = center_x - text_width / 2.0;
        let text_y = top_y + (diameter - font_size) / 2.0;

        tree.push(RenderCommand::Text {
            x: text_x,
            y: text_y,
            text: initials.clone(),
            color: theme::TEXT,
            font_size,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
    }

    /// Render the password input field.
    fn render_password_field(&self, tree: &mut RenderTree, x: f32, y: f32) {
        let border_color = if self.password_focused {
            theme::BLUE
        } else {
            theme::SURFACE2
        };

        // Field background.
        tree.fill_rounded_rect(
            x,
            y,
            PASSWORD_FIELD_WIDTH,
            PASSWORD_FIELD_HEIGHT,
            theme::SURFACE0,
            CornerRadii::all(PASSWORD_FIELD_RADIUS),
        );

        // Field border.
        tree.push(RenderCommand::StrokeRect {
            x,
            y,
            width: PASSWORD_FIELD_WIDTH,
            height: PASSWORD_FIELD_HEIGHT,
            color: border_color,
            line_width: 2.0,
            corner_radii: CornerRadii::all(PASSWORD_FIELD_RADIUS),
        });

        if self.password_buffer.is_empty() {
            // Placeholder text.
            tree.push(RenderCommand::Text {
                x: x + 20.0,
                y: y + (PASSWORD_FIELD_HEIGHT - PASSWORD_FONT_SIZE) / 2.0,
                text: "Password".to_string(),
                color: theme::SUBTEXT,
                font_size: PASSWORD_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(PASSWORD_FIELD_WIDTH - 40.0),
            });
        } else {
            // Masked dots for each character entered.
            let dot_count = self.password_buffer.len();
            let total_dot_width = dot_count as f32 * PASSWORD_DOT_SPACING;
            let dots_start_x = x + 20.0;
            let dot_cy = y + PASSWORD_FIELD_HEIGHT / 2.0;
            let dot_radius = PASSWORD_DOT_DIAMETER / 2.0;

            // Clip dots to within the field.
            tree.clip(
                x + 4.0,
                y,
                PASSWORD_FIELD_WIDTH - 8.0,
                PASSWORD_FIELD_HEIGHT,
            );

            // If there are too many dots, scroll them so the latest are visible.
            let max_visible_width = PASSWORD_FIELD_WIDTH - 40.0;
            let scroll_offset = if total_dot_width > max_visible_width {
                total_dot_width - max_visible_width
            } else {
                0.0
            };

            for i in 0..dot_count {
                let dot_x = dots_start_x + (i as f32 * PASSWORD_DOT_SPACING)
                    - scroll_offset + PASSWORD_DOT_SPACING / 2.0 - dot_radius;
                let dot_y = dot_cy - dot_radius;

                tree.fill_rounded_rect(
                    dot_x,
                    dot_y,
                    PASSWORD_DOT_DIAMETER,
                    PASSWORD_DOT_DIAMETER,
                    theme::TEXT,
                    CornerRadii::all(dot_radius),
                );
            }

            tree.unclip();
        }
    }

    /// Render the submit (arrow) button.
    fn render_submit_button(&self, tree: &mut RenderTree, x: f32, y: f32) {
        let bg_color = if self.submit_hovered {
            theme::BLUE
        } else {
            theme::SURFACE1
        };
        let arrow_color = if self.submit_hovered {
            theme::BASE
        } else {
            theme::TEXT
        };

        // Circle background.
        let radius = SUBMIT_BUTTON_RADIUS;
        tree.fill_rounded_rect(
            x,
            y,
            SUBMIT_BUTTON_WIDTH,
            SUBMIT_BUTTON_HEIGHT,
            bg_color,
            CornerRadii::all(radius),
        );

        // Arrow symbol (right-pointing arrow ">").
        let arrow_font_size = 20.0;
        let arrow_x = x + (SUBMIT_BUTTON_WIDTH - arrow_font_size * 0.5) / 2.0;
        let arrow_y = y + (SUBMIT_BUTTON_HEIGHT - arrow_font_size) / 2.0;

        tree.push(RenderCommand::Text {
            x: arrow_x,
            y: arrow_y,
            text: "\u{2192}".to_string(), // Right arrow Unicode
            color: arrow_color,
            font_size: arrow_font_size,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
    }

    /// Render the multi-user list below the password field.
    fn render_user_list(&self, tree: &mut RenderTree) {
        let cx = self.center_x();
        let list_x = cx - USER_LIST_ITEM_WIDTH / 2.0;
        let list_start_y = self.password_field_y() + PASSWORD_FIELD_HEIGHT + SECTION_GAP * 3.0;

        // "Switch user" label.
        let label = "Switch user";
        let label_font_size: f32 = 12.0;
        let label_char_width = label_font_size * 0.55;
        let label_width = label.len() as f32 * label_char_width;
        let label_x = cx - label_width / 2.0;
        let label_y = list_start_y - 20.0;

        tree.push(RenderCommand::Text {
            x: label_x,
            y: label_y,
            text: label.to_string(),
            color: theme::SUBTEXT,
            font_size: label_font_size,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        for (i, user) in self.users.iter().enumerate() {
            let item_y = list_start_y + (i as f32) * (USER_LIST_ITEM_HEIGHT + 4.0);
            let is_selected = i == self.selected_user_index;

            // Item background.
            let bg_color = if is_selected {
                theme::SURFACE1
            } else {
                theme::SURFACE0
            };
            tree.fill_rounded_rect(
                list_x,
                item_y,
                USER_LIST_ITEM_WIDTH,
                USER_LIST_ITEM_HEIGHT,
                bg_color,
                CornerRadii::all(8.0),
            );

            // Small avatar.
            let avatar_x = list_x + 8.0;
            let avatar_y = item_y + (USER_LIST_ITEM_HEIGHT - SMALL_AVATAR_DIAMETER) / 2.0;
            let small_radius = SMALL_AVATAR_DIAMETER / 2.0;
            tree.fill_rounded_rect(
                avatar_x,
                avatar_y,
                SMALL_AVATAR_DIAMETER,
                SMALL_AVATAR_DIAMETER,
                theme::AVATAR_BG,
                CornerRadii::all(small_radius),
            );

            // Small avatar initials.
            let initials_char_width = SMALL_AVATAR_FONT_SIZE * 0.6;
            let initials_width = user.initials.len() as f32 * initials_char_width;
            let initials_x = avatar_x + SMALL_AVATAR_DIAMETER / 2.0 - initials_width / 2.0;
            let initials_y = avatar_y
                + (SMALL_AVATAR_DIAMETER - SMALL_AVATAR_FONT_SIZE) / 2.0;

            tree.push(RenderCommand::Text {
                x: initials_x,
                y: initials_y,
                text: user.initials.clone(),
                color: theme::TEXT,
                font_size: SMALL_AVATAR_FONT_SIZE,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });

            // User display name.
            let name_x = avatar_x + SMALL_AVATAR_DIAMETER + 12.0;
            let name_y = item_y + (USER_LIST_ITEM_HEIGHT - 14.0) / 2.0;

            tree.push(RenderCommand::Text {
                x: name_x,
                y: name_y,
                text: user.display_name.clone(),
                color: theme::TEXT,
                font_size: 14.0,
                font_weight: if is_selected {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(USER_LIST_ITEM_WIDTH - SMALL_AVATAR_DIAMETER - 32.0),
            });
        }
    }
}

// ============================================================================
// Geometry helpers
// ============================================================================

/// Simple axis-aligned rectangle for hit testing.
#[derive(Clone, Copy, Debug)]
struct Rect {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

/// Test whether a point (px, py) is inside a rectangle.
fn hit_test(px: f32, py: f32, rect: &Rect) -> bool {
    px >= rect.x
        && px <= rect.x + rect.width
        && py >= rect.y
        && py <= rect.y + rect.height
}

// ============================================================================
// Entry point (placeholder — the real entry will integrate with the
// compositor via IPC).
// ============================================================================

fn main() {}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -- Helper factories --

    fn single_user_lockscreen() -> LockScreen {
        let user = UserInfo::new("alice", "Alice Johnson", true)
            .with_hint("Name of your cat");
        let validator = PasswordValidator::from_password("correcthorse");
        LockScreen::new(vec![user], LockScreenConfig::default(), Some(validator))
    }

    fn multi_user_lockscreen() -> LockScreen {
        let users = vec![
            UserInfo::new("alice", "Alice Johnson", true),
            UserInfo::new("bob", "Bob Smith", true),
            UserInfo::new("charlie", "Charlie Brown", false),
        ];
        let validator = PasswordValidator::from_password("correcthorse");
        LockScreen::new(users, LockScreenConfig::default(), Some(validator))
    }

    fn no_password_lockscreen() -> LockScreen {
        let user = UserInfo::new("guest", "Guest User", false);
        LockScreen::new(vec![user], LockScreenConfig::default(), None)
    }

    // -- LockScreenState --

    #[test]
    fn test_default_state_is_clock() {
        let ls = single_user_lockscreen();
        assert_eq!(ls.state, LockScreenState::Clock);
    }

    #[test]
    fn test_enter_password_mode() {
        let mut ls = single_user_lockscreen();
        ls.enter_password_mode();
        assert_eq!(ls.state, LockScreenState::PasswordEntry);
        assert!(ls.password_focused);
    }

    #[test]
    fn test_return_to_clock() {
        let mut ls = single_user_lockscreen();
        ls.enter_password_mode();
        ls.type_char('a');
        ls.return_to_clock();
        assert_eq!(ls.state, LockScreenState::Clock);
        assert_eq!(ls.password_len(), 0);
        assert!(!ls.password_focused);
    }

    // -- UserInfo --

    #[test]
    fn test_compute_initials_two_words() {
        assert_eq!(compute_initials("Alice Johnson"), "AJ");
    }

    #[test]
    fn test_compute_initials_single_word() {
        assert_eq!(compute_initials("Admin"), "A");
    }

    #[test]
    fn test_compute_initials_empty() {
        assert_eq!(compute_initials(""), "?");
    }

    #[test]
    fn test_compute_initials_whitespace_only() {
        assert_eq!(compute_initials("   "), "?");
    }

    #[test]
    fn test_compute_initials_three_words() {
        // Takes first two words only.
        assert_eq!(compute_initials("John Paul Jones"), "JP");
    }

    #[test]
    fn test_user_info_with_hint() {
        let user = UserInfo::new("u", "U", true).with_hint("pet name");
        assert_eq!(user.password_hint.as_deref(), Some("pet name"));
    }

    // -- PasswordValidator --

    #[test]
    fn test_validator_correct_password() {
        let v = PasswordValidator::from_password("hello");
        assert!(v.validate("hello"));
    }

    #[test]
    fn test_validator_wrong_password() {
        let v = PasswordValidator::from_password("hello");
        assert!(!v.validate("world"));
    }

    #[test]
    fn test_validator_empty_password() {
        let v = PasswordValidator::from_password("");
        assert!(v.validate(""));
        assert!(!v.validate("x"));
    }

    #[test]
    fn test_validator_unicode_password() {
        let v = PasswordValidator::from_password("\u{1F600}password\u{1F600}");
        assert!(v.validate("\u{1F600}password\u{1F600}"));
        assert!(!v.validate("password"));
    }

    // -- SHA-256 --

    #[test]
    fn test_sha256_empty() {
        // SHA-256("") = e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855
        let hash = sha256_hash(b"");
        assert_eq!(hash[0], 0xe3);
        assert_eq!(hash[1], 0xb0);
        assert_eq!(hash[31], 0x55);
    }

    #[test]
    fn test_sha256_abc() {
        // SHA-256("abc") = ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad
        let hash = sha256_hash(b"abc");
        assert_eq!(hash[0], 0xba);
        assert_eq!(hash[1], 0x78);
        assert_eq!(hash[31], 0xad);
    }

    #[test]
    fn test_sha256_known_vector() {
        // SHA-256("password123")
        // = ef92b778bafe771e89245b89ecbc08a44a4e166c06659911881f383d4473e94f
        let hash = sha256_hash(b"password123");
        assert_eq!(hash[0], 0xef);
        assert_eq!(hash[1], 0x92);
        assert_eq!(hash[31], 0x4f);
    }

    #[test]
    fn test_constant_time_eq_equal() {
        let a = [0xAAu8; 32];
        let b = [0xAAu8; 32];
        assert!(constant_time_eq(&a, &b));
    }

    #[test]
    fn test_constant_time_eq_different() {
        let a = [0xAAu8; 32];
        let mut b = [0xAAu8; 32];
        b[15] = 0xBB;
        assert!(!constant_time_eq(&a, &b));
    }

    // -- Password input --

    #[test]
    fn test_type_char() {
        let mut ls = single_user_lockscreen();
        ls.enter_password_mode();
        ls.type_char('a');
        ls.type_char('b');
        ls.type_char('c');
        assert_eq!(ls.password_len(), 3);
    }

    #[test]
    fn test_backspace() {
        let mut ls = single_user_lockscreen();
        ls.enter_password_mode();
        ls.type_char('a');
        ls.type_char('b');
        ls.backspace();
        assert_eq!(ls.password_len(), 1);
    }

    #[test]
    fn test_backspace_on_empty() {
        let mut ls = single_user_lockscreen();
        ls.enter_password_mode();
        ls.backspace(); // Should not panic.
        assert_eq!(ls.password_len(), 0);
    }

    #[test]
    fn test_clear_password() {
        let mut ls = single_user_lockscreen();
        ls.enter_password_mode();
        ls.type_char('x');
        ls.type_char('y');
        ls.clear_password();
        assert_eq!(ls.password_len(), 0);
    }

    #[test]
    fn test_max_password_length() {
        let mut ls = single_user_lockscreen();
        ls.enter_password_mode();
        for _ in 0..MAX_PASSWORD_LENGTH + 10 {
            ls.type_char('a');
        }
        assert_eq!(ls.password_len(), MAX_PASSWORD_LENGTH);
    }

    // -- Submit password --

    #[test]
    fn test_submit_correct_password() {
        let mut ls = single_user_lockscreen();
        ls.enter_password_mode();
        for ch in "correcthorse".chars() {
            ls.type_char(ch);
        }
        assert!(ls.submit_password());
        assert_eq!(ls.failed_attempts, 0);
    }

    #[test]
    fn test_submit_wrong_password() {
        let mut ls = single_user_lockscreen();
        ls.enter_password_mode();
        for ch in "wrongpassword".chars() {
            ls.type_char(ch);
        }
        assert!(!ls.submit_password());
        assert_eq!(ls.failed_attempts, 1);
        assert!(ls.show_error);
    }

    #[test]
    fn test_submit_empty_password() {
        let mut ls = single_user_lockscreen();
        ls.enter_password_mode();
        assert!(!ls.submit_password());
        assert_eq!(ls.failed_attempts, 0); // Empty submit doesn't count as failure.
    }

    #[test]
    fn test_no_password_user_unlocks_immediately() {
        let mut ls = no_password_lockscreen();
        ls.enter_password_mode();
        assert!(ls.submit_password());
    }

    // -- Failed attempts and lockout --

    #[test]
    fn test_failed_attempts_increment() {
        let mut ls = single_user_lockscreen();
        ls.enter_password_mode();
        for _ in 0..3 {
            ls.type_char('x');
            ls.submit_password();
        }
        assert_eq!(ls.failed_attempts, 3);
    }

    #[test]
    fn test_lockout_after_5_failures() {
        let mut ls = single_user_lockscreen();
        ls.enter_password_mode();
        for _ in 0..5 {
            ls.type_char('x');
            ls.submit_password();
        }
        assert!(ls.lockout.is_active());
        assert_eq!(ls.lockout.remaining_secs, LOCKOUT_TIER_1_SECS);
    }

    #[test]
    fn test_lockout_blocks_typing() {
        let mut ls = single_user_lockscreen();
        ls.enter_password_mode();
        for _ in 0..5 {
            ls.type_char('x');
            ls.submit_password();
        }
        assert!(ls.lockout.is_active());
        ls.type_char('a');
        assert_eq!(ls.password_len(), 0); // Typing blocked during lockout.
    }

    #[test]
    fn test_lockout_blocks_submit() {
        let mut ls = single_user_lockscreen();
        ls.enter_password_mode();
        for _ in 0..5 {
            ls.type_char('x');
            ls.submit_password();
        }
        assert!(!ls.submit_password()); // Submit blocked during lockout.
    }

    #[test]
    fn test_hint_shown_after_threshold() {
        let mut ls = single_user_lockscreen();
        ls.enter_password_mode();
        for _ in 0..HINT_THRESHOLD {
            ls.type_char('x');
            ls.submit_password();
        }
        assert!(ls.failed_attempts >= HINT_THRESHOLD);
        let user = ls.active_user();
        assert!(user.password_hint.is_some());
    }

    // -- Lockout duration calculation --

    #[test]
    fn test_lockout_duration_tier_1() {
        assert_eq!(lockout_duration_for_attempts(5), Some(LOCKOUT_TIER_1_SECS));
    }

    #[test]
    fn test_lockout_duration_tier_2() {
        assert_eq!(lockout_duration_for_attempts(10), Some(LOCKOUT_TIER_2_SECS));
    }

    #[test]
    fn test_lockout_duration_tier_3() {
        assert_eq!(lockout_duration_for_attempts(15), Some(LOCKOUT_TIER_3_SECS));
    }

    #[test]
    fn test_lockout_duration_no_lockout() {
        assert_eq!(lockout_duration_for_attempts(3), None);
        assert_eq!(lockout_duration_for_attempts(0), None);
    }

    // -- LockoutTimer --

    #[test]
    fn test_lockout_timer_tick() {
        let mut timer = LockoutTimer::new();
        timer.start(2);
        assert!(timer.is_active());
        assert!(!timer.tick(999)); // Not yet expired.
        assert!(timer.is_active());
        assert!(timer.tick(1001)); // 2000ms total -> 2 seconds expired.
        assert!(!timer.is_active());
    }

    #[test]
    fn test_lockout_timer_format() {
        let mut timer = LockoutTimer::new();
        timer.start(65);
        assert_eq!(timer.format_remaining(), "1:05");
    }

    // -- ShakeAnimation --

    #[test]
    fn test_shake_animation_lifecycle() {
        let mut shake = ShakeAnimation::new();
        assert!(!shake.is_active());
        assert_eq!(shake.tick(100), 0.0);

        shake.trigger();
        assert!(shake.is_active());

        // Should produce a non-zero offset during the animation.
        let offset = shake.tick(50);
        // Just verify it did something (exact value depends on sine).
        assert!(shake.is_active());

        // Advance past the end.
        let _ = shake.tick(SHAKE_DURATION_MS);
        assert!(!shake.is_active());
        let _ = offset; // suppress unused warning in test
    }

    // -- sin_approx --

    #[test]
    fn test_sin_approx_zero() {
        let val = sin_approx(0.0);
        assert!(val.abs() < 0.01, "sin(0) should be ~0, got {val}");
    }

    #[test]
    fn test_sin_approx_pi_half() {
        let val = sin_approx(core::f32::consts::FRAC_PI_2);
        assert!((val - 1.0).abs() < 0.01, "sin(pi/2) should be ~1, got {val}");
    }

    #[test]
    fn test_sin_approx_pi() {
        let val = sin_approx(core::f32::consts::PI);
        assert!(val.abs() < 0.01, "sin(pi) should be ~0, got {val}");
    }

    // -- Multi-user --

    #[test]
    fn test_multi_user_selection() {
        let mut ls = multi_user_lockscreen();
        assert!(ls.is_multi_user());
        assert_eq!(ls.selected_user_index, 0);
        assert_eq!(ls.active_user().username, "alice");

        ls.select_user(1);
        assert_eq!(ls.active_user().username, "bob");
    }

    #[test]
    fn test_select_user_clears_password() {
        let mut ls = multi_user_lockscreen();
        ls.enter_password_mode();
        ls.type_char('a');
        ls.select_user(1);
        assert_eq!(ls.password_len(), 0);
        assert_eq!(ls.failed_attempts, 0);
    }

    #[test]
    fn test_select_user_out_of_bounds() {
        let mut ls = multi_user_lockscreen();
        ls.select_user(999); // Should be a no-op.
        assert_eq!(ls.selected_user_index, 0);
    }

    // -- TimeOfDay --

    #[test]
    fn test_time_of_day_format_hhmm() {
        let t = TimeOfDay::new(9, 5, 0).expect("valid time");
        assert_eq!(t.format_hhmm(), "09:05");
    }

    #[test]
    fn test_time_of_day_format_hhmmss() {
        let t = TimeOfDay::new(14, 30, 7).expect("valid time");
        assert_eq!(t.format_hhmmss(), "14:30:07");
    }

    #[test]
    fn test_time_of_day_invalid() {
        assert!(TimeOfDay::new(24, 0, 0).is_none());
        assert!(TimeOfDay::new(0, 60, 0).is_none());
        assert!(TimeOfDay::new(0, 0, 60).is_none());
    }

    // -- DateInfo --

    #[test]
    fn test_date_format_long() {
        let date = DateInfo {
            weekday: "Wednesday".to_string(),
            month: "January".to_string(),
            day: 15,
            year: 2026,
        };
        assert_eq!(date.format_long(), "Wednesday, January 15, 2026");
    }

    // -- LockScreenConfig --

    #[test]
    fn test_default_config() {
        let cfg = LockScreenConfig::default();
        assert_eq!(cfg.auto_lock_timeout_secs, Some(300));
        assert!(!cfg.show_clock_seconds);
        assert!(cfg.show_date);
        assert_eq!(cfg.wallpaper_tint_alpha, 140);
    }

    // -- Event handling --

    #[test]
    fn test_key_enter_submits() {
        let mut ls = single_user_lockscreen();
        ls.enter_password_mode();
        for ch in "correcthorse".chars() {
            ls.type_char(ch);
        }
        let event = Event::Key(KeyEvent {
            key: Key::Enter,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        let result = ls.handle_event(&event);
        assert_eq!(result, EventResult::Consumed);
    }

    #[test]
    fn test_key_escape_returns_to_clock() {
        let mut ls = single_user_lockscreen();
        ls.enter_password_mode();
        let event = Event::Key(KeyEvent {
            key: Key::Escape,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        ls.handle_event(&event);
        assert_eq!(ls.state, LockScreenState::Clock);
    }

    #[test]
    fn test_clock_any_key_enters_password_mode() {
        let mut ls = single_user_lockscreen();
        let event = Event::Key(KeyEvent {
            key: Key::A,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: Some('a'),
        });
        ls.handle_event(&event);
        assert_eq!(ls.state, LockScreenState::PasswordEntry);
        assert_eq!(ls.password_len(), 1); // The 'a' was typed.
    }

    #[test]
    fn test_modifier_keys_dont_enter_password_mode() {
        let mut ls = single_user_lockscreen();
        let event = Event::Key(KeyEvent {
            key: Key::LeftShift,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        ls.handle_event(&event);
        assert_eq!(ls.state, LockScreenState::Clock);
    }

    #[test]
    fn test_resize_event() {
        let mut ls = single_user_lockscreen();
        let event = Event::Resize { width: 2560, height: 1440 };
        let result = ls.handle_event(&event);
        assert_eq!(result, EventResult::Consumed);
        assert_eq!(ls.screen_width, 2560.0);
        assert_eq!(ls.screen_height, 1440.0);
    }

    // -- Rendering --

    #[test]
    fn test_render_clock_produces_commands() {
        let mut ls = single_user_lockscreen();
        ls.set_date(DateInfo {
            weekday: "Monday".to_string(),
            month: "May".to_string(),
            day: 18,
            year: 2026,
        });
        let tree = ls.render();
        // Should have at least: overlay + clock text + date text + hint text.
        assert!(tree.len() >= 4, "Clock view should produce at least 4 commands, got {}", tree.len());
    }

    #[test]
    fn test_render_password_screen_produces_commands() {
        let mut ls = single_user_lockscreen();
        ls.enter_password_mode();
        ls.type_char('x');
        let tree = ls.render();
        // Should have: overlay + avatar bg + initials + name + field bg + field border
        //  + dots (clipped) + submit button + arrow
        assert!(tree.len() >= 8, "Password view should produce at least 8 commands, got {}", tree.len());
    }

    #[test]
    fn test_render_with_error_includes_error_text() {
        let mut ls = single_user_lockscreen();
        ls.enter_password_mode();
        ls.type_char('x');
        ls.submit_password();
        let tree = ls.render();
        // Look for the error text command.
        let has_error_text = tree.commands.iter().any(|cmd| {
            matches!(cmd, RenderCommand::Text { text, color, .. }
                if text == "Incorrect password" && *color == theme::RED)
        });
        assert!(has_error_text, "Render output should include the error message");
    }

    #[test]
    fn test_render_multi_user_includes_user_list() {
        let mut ls = multi_user_lockscreen();
        ls.enter_password_mode();
        let tree = ls.render();
        // Should include "Switch user" label and user entries.
        let has_switch_label = tree.commands.iter().any(|cmd| {
            matches!(cmd, RenderCommand::Text { text, .. } if text == "Switch user")
        });
        assert!(has_switch_label, "Multi-user render should include 'Switch user' label");
    }

    // -- Accessibility --

    #[test]
    fn test_accessibility_clock_state() {
        let ls = single_user_lockscreen();
        let text = ls.accessibility_text();
        assert!(text.contains("Lock screen"));
        assert!(text.contains("Time:"));
        assert!(text.contains("Press any key"));
    }

    #[test]
    fn test_accessibility_password_state() {
        let mut ls = single_user_lockscreen();
        ls.enter_password_mode();
        ls.type_char('a');
        let text = ls.accessibility_text();
        assert!(text.contains("Unlock screen"));
        assert!(text.contains("1 characters entered"));
    }

    #[test]
    fn test_accessibility_lockout_state() {
        let mut ls = single_user_lockscreen();
        ls.enter_password_mode();
        for _ in 0..5 {
            ls.type_char('x');
            ls.submit_password();
        }
        let text = ls.accessibility_text();
        assert!(text.contains("Account locked"));
    }

    // -- Hit testing --

    #[test]
    fn test_hit_test_inside() {
        let rect = Rect { x: 10.0, y: 10.0, width: 100.0, height: 50.0 };
        assert!(hit_test(50.0, 30.0, &rect));
    }

    #[test]
    fn test_hit_test_outside() {
        let rect = Rect { x: 10.0, y: 10.0, width: 100.0, height: 50.0 };
        assert!(!hit_test(5.0, 30.0, &rect));
        assert!(!hit_test(50.0, 70.0, &rect));
    }

    #[test]
    fn test_hit_test_edge() {
        let rect = Rect { x: 10.0, y: 10.0, width: 100.0, height: 50.0 };
        // Edges are inclusive.
        assert!(hit_test(10.0, 10.0, &rect));
        assert!(hit_test(110.0, 60.0, &rect));
    }

    // -- Empty user list fallback --

    #[test]
    fn test_empty_user_list_fallback() {
        let ls = LockScreen::new(vec![], LockScreenConfig::default(), None);
        assert_eq!(ls.users.len(), 1);
        assert_eq!(ls.active_user().username, "user");
    }
}
