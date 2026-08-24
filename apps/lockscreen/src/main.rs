#![allow(dead_code)]
//! Slate OS Lock Screen
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
use guitk::event::{
    Event, EventResult, Key, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseEventKind,
};
#[allow(unused_imports)]
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
#[allow(unused_imports)]
use guitk::style::CornerRadii;
use guitk::text;
use pwkdf::{KdfError, KdfParams, PasswordVerifier};

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
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum LockScreenState {
    /// Showing the clock/date (idle). User clicks or presses a key to enter
    /// password mode.
    #[default]
    Clock,
    /// User is entering their password.
    PasswordEntry,
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
    if let Some(first) = parts.next()
        && let Some(ch) = first.chars().next()
    {
        result.push(ch.to_ascii_uppercase());
    }
    if let Some(second) = parts.next()
        && let Some(ch) = second.chars().next()
    {
        result.push(ch.to_ascii_uppercase());
    }
    if result.is_empty() {
        "?".to_string()
    } else {
        result
    }
}

// ============================================================================
// Who decides whether a password is right
// ============================================================================

/// The verdict on a password attempt.
///
/// # Why this is not a `bool`
///
/// It used to be, and the screen's two call sites both wrote
/// `let _ = self.submit_password();` — the answer to the only question the
/// screen asks was computed and thrown away. A richer type does not by itself
/// stop that, but it does make the discard visible, and each variant here
/// wants different handling that a `bool` cannot express.
///
/// The variants mirror `userspace/authlib`'s `Outcome` one for one, because
/// this is the shape the real answer arrives in — see
/// `requests/b-c-desktop-password-checks-go-through-a-privileged-verifier.md`
/// and `design-decisions.md` §341. The lock screen will not compute a verdict
/// itself for much longer: it will hand the typed bytes to `logind` over
/// `libservicebus` and receive one of these back. Naming the six cases now,
/// while the answer is still local, is the difference between growing toward
/// that shape and away from it.
///
/// # `Locked` and `Unusable` are not `Rejected`
///
/// All three show the user the same thing, because telling an attacker *which*
/// of the three they hit is free information. They are separate anyway because
/// `Unusable` means the *system* is broken — a stored entry that nothing on
/// this machine can recompute — and that has to reach an administrator rather
/// than be tallied as a typo and eventually lock the account out for a fault
/// it did not cause.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuthOutcome {
    /// The password is right.
    Accepted,
    /// The password is wrong, or there is no such user. Indistinguishable on
    /// purpose: `authlib` runs the same key derivation against a dummy entry
    /// for a missing account, so the call cannot be timed to enumerate users.
    Rejected,
    /// The account is disabled (`!`, `!!`, `*`, `!$6$…` in the store). No
    /// password will ever open it.
    Locked,
    /// The stored entry is empty. Deliberately *not* a verdict: `authlib`
    /// reports what it found and each caller states its own policy, because a
    /// console login may reasonably let an empty entry through and a lock
    /// screen may not. This screen's policy is [`LockScreen::unlocks_for`].
    NoPassword,
    /// The stored entry is in a format this system cannot recompute. A fault,
    /// not a wrong guess.
    Unusable,
    /// Too many recent failures. The verifier, not the caller, counts them —
    /// a password check is an oracle by construction, so the rate limit is
    /// part of the interface rather than something each caller is trusted to
    /// remember to add.
    RateLimited {
        /// Seconds until another attempt will be considered.
        retry_after_secs: u64,
    },
}

impl AuthOutcome {
    /// Whether this verdict, on its own, means the password was right.
    ///
    /// Note what this deliberately does *not* decide: [`Self::NoPassword`] is
    /// false here, and a caller that wants to accept an empty entry must say
    /// so itself. See the variant's own note.
    #[must_use]
    pub const fn is_accepted(self) -> bool {
        matches!(self, Self::Accepted)
    }

    /// Whether this verdict means the machine needs an administrator rather
    /// than another guess.
    #[must_use]
    pub const fn needs_administrator(self) -> bool {
        matches!(self, Self::Locked | Self::Unusable)
    }

    /// A message safe to show on a screen an attacker may be standing at.
    ///
    /// [`Self::Rejected`], [`Self::Locked`] and [`Self::Unusable`] share one
    /// string on purpose — see the type's own note.
    #[must_use]
    pub const fn user_message(self) -> &'static str {
        match self {
            Self::Accepted => "",
            Self::Rejected | Self::Locked | Self::Unusable => "Incorrect password",
            Self::NoPassword => "This account has no password",
            Self::RateLimited { .. } => "Too many attempts",
        }
    }
}

/// Something that can say whether a password is right.
///
/// The screen holds one of these rather than a hash, which is the whole point:
/// `apps/lockscreen` runs as the logged-in user and is the process most likely
/// to be attacked, so the thing worth stealing must not be in it. Today the
/// only implementor is [`PasswordValidator`], which does hold a verifier and
/// is interim scaffolding for exactly that reason. Tomorrow's implementor is a
/// `libservicebus` connection to `system.logind`, and nothing above this trait
/// changes when it arrives.
///
/// # Why `&mut self`
///
/// The failure tally lives on the verifier, not on the question — one
/// authority per process, not one per attempt. A rate limit rebuilt for every
/// guess is not a rate limit. [`PasswordValidator`] does not use the
/// mutability, and the signature carries it anyway so that the implementor
/// that needs it does not have to change the trait.
///
/// # Why `&[u8]` and not `&str`
///
/// A password is not text. `logind` takes `libservicebus` byte-string fields
/// precisely so that a password which is not valid UTF-8 survives the trip,
/// and a `&str` here would put a lossy conversion in front of the one call
/// where losing a byte silently means a correct password is rejected forever.
pub trait PasswordAuthority: core::fmt::Debug {
    /// Judge `password` for `username`.
    fn authenticate(&mut self, username: &str, password: &[u8]) -> AuthOutcome;
}

// ============================================================================
// Password validator
// ============================================================================

/// Separates this screen's stored verifier from every other caller's.
///
/// Without it, a user who reuses one password would produce the same stored
/// value here and in the credential vault, and either could be replayed
/// against the other.
const VERIFIER_DOMAIN: &[u8] = b"slateos-lockscreen-verifier";

/// Checks a typed password against a stored, salted, stretched verifier.
///
/// # What this used to be, and why it mattered
///
/// It stored a bare `SHA-256(password)`: no salt, one pass. Both halves were
/// wrong, and each was wrong on its own.
///
/// *No salt* means the stored value depends only on the password, so one
/// precomputed table of hashed common passwords opens this screen on every
/// SlateOS machine ever installed — the attacker pays once, for everyone.
/// `gui/credentials` had the weaker version of this bug (a salt, but a
/// compile-time constant shared by every install) and it was fixed in
/// design-decisions §464; this screen never got a salt at all.
///
/// *One pass* means testing a guess costs one SHA-256, so commodity GPU
/// hardware runs through billions of candidates per second. A password's own
/// entropy — 30–40 bits at best — cannot survive that; the only defence is to
/// make each guess expensive, which is what [`pwkdf::DEFAULT_ROUNDS`] does.
///
/// # Why it is `pwkdf` rather than a local fix
///
/// The stored value here has to be one the credential store can produce: the
/// comment this type used to carry said "in a real system this would call into
/// the OS credential store via IPC", and that is still the plan. Two
/// independently-correct derivations would still be *incompatible*, and on the
/// day someone wires the two together the cheap way to reconcile them is to
/// weaken the store to match the screen. Sharing the derivation now settles
/// the format while nothing depends on it.
#[derive(Clone, Debug)]
pub struct PasswordValidator {
    /// The stored verifier, with the salt and cost it was derived under.
    ///
    /// Not a bare hash: `PasswordVerifier` keeps the three values that must
    /// agree together, because a mismatch in any of them rejects the correct
    /// password with no indication why.
    verifier: PasswordVerifier,
}

impl PasswordValidator {
    /// Rebuild a validator from what a credential store holds.
    ///
    /// `params` must be the salt and cost the verifier was created under. A
    /// persistence layer that stores the verifier and loses the salt has
    /// destroyed the account: every subsequent login fails, and the symptom
    /// ("correct password rejected") does not point at the cause.
    #[must_use]
    pub const fn from_stored(params: KdfParams, verifier: [u8; 32]) -> Self {
        Self {
            verifier: PasswordVerifier::from_parts(params, VERIFIER_DOMAIN, verifier),
        }
    }

    /// Enrol a new password, drawing a fresh salt from the kernel.
    ///
    /// # Errors
    ///
    /// [`KdfError::EntropyUnavailable`] if the kernel CSPRNG cannot be
    /// reached. Propagated rather than papered over with a fallback salt: this
    /// is the *secret* tier of design-decisions §465, and a predictable salt
    /// chosen once outlives every later chance to notice it. Refusing to
    /// enrol is recoverable; enrolling against a guessable salt is not.
    pub fn enrol(password: &str) -> Result<Self, KdfError> {
        let params = KdfParams::fresh(pwkdf::DEFAULT_ROUNDS)?;
        Ok(Self {
            verifier: PasswordVerifier::create(password.as_bytes(), params, VERIFIER_DOMAIN),
        })
    }

    /// Whether `candidate` is the password this validator was built from.
    ///
    /// Costs a full derivation — deliberately ~130 ms at
    /// [`pwkdf::DEFAULT_ROUNDS`]. That is the point, and it is why this is
    /// called on submit rather than per keystroke.
    #[must_use]
    pub fn validate(&self, candidate: &str) -> bool {
        self.verifier.check(candidate.as_bytes())
    }

    /// The salt and cost, for a persistence layer to write down beside
    /// [`Self::verifier`]. Both are required to check a password later.
    #[must_use]
    pub const fn params(&self) -> KdfParams {
        self.verifier.params()
    }

    /// The stored verifier, for a persistence layer to write down.
    #[must_use]
    pub const fn verifier(&self) -> [u8; 32] {
        self.verifier.verifier()
    }

    /// A validator for a known password, with a named salt and a cheap cost.
    ///
    /// `#[cfg(test)]` so that neither shortcut can reach production. Both are
    /// deliberate and neither is safe outside a test: the fixed salt makes
    /// assertions reproducible, and [`TEST_ROUNDS`] keeps a suite that builds
    /// validators in helper functions from spending ~130 ms on each one.
    #[cfg(test)]
    fn for_test(password: &str) -> Self {
        let params = KdfParams::new([0x5Au8; pwkdf::SALT_LEN], TEST_ROUNDS);
        Self {
            verifier: PasswordVerifier::create(password.as_bytes(), params, VERIFIER_DOMAIN),
        }
    }
}

/// The interim local path, until `logind` can tell who is calling it.
///
/// This is the implementation that ought not to exist: it means the process
/// guarding the session is also the process holding the thing worth stealing.
/// It stays because the replacement is blocked, not because it is right —
/// `logind`'s `AuthenticateSession` is written and tested, and answers
/// `system.logind.Error.UnknownCaller` to everyone, because the kernel gives a
/// service no way to learn its caller's uid (`SYS_SERVICE_ACCEPT` returns a
/// bare handle and records nothing about the peer). Lane B has asked lane A
/// for that in `requests/b-a-a-service-cannot-find-out-who-is-calling-it.md`.
/// When it lands, this impl is deleted and a bus connection takes its place;
/// nothing else in this file moves.
///
/// Only three of the six outcomes are reachable from here, which is itself the
/// measure of how much this stands in for. It cannot distinguish a disabled
/// account from a wrong password, because it never sees an account — it holds
/// one verifier and knows nothing about `username`. The rate limit is the
/// screen's own [`LockoutTimer`], which is the wrong place for it (see the
/// trait's note) and is where it has to stay while the tally has nowhere else
/// to live.
impl PasswordAuthority for PasswordValidator {
    fn authenticate(&mut self, _username: &str, password: &[u8]) -> AuthOutcome {
        if self.verifier.check(password) {
            AuthOutcome::Accepted
        } else {
            AuthOutcome::Rejected
        }
    }
}

/// Iteration count for validators built by tests.
///
/// The properties under test — that the right password is accepted, that a
/// wrong one is not, that the salt is honoured — do not depend on the number
/// of rounds, and [`pwkdf::DEFAULT_ROUNDS`] is chosen to take ~130 ms, which
/// several helper functions per test would turn into a slow suite.
#[cfg(test)]
const TEST_ROUNDS: u32 = 16;

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
        Some(Self {
            hour,
            minute,
            second,
        })
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
        format!(
            "{}, {} {}, {}",
            self.weekday, self.month, self.day, self.year
        )
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
        // Divide rather than loop. The `while` this replaces subtracted 1000
        // per iteration, so a single large `dt_ms` -- the frame after a
        // suspend, or a debugger pause -- spun it once per elapsed
        // millisecond/1000 while doing nothing a division does not. It also
        // needed two guards to keep its two subtractions from underflowing,
        // both of which the compiler had to take on trust.
        let total_ms = self.ms_accumulator.saturating_add(dt_ms);
        self.ms_accumulator = total_ms % 1000;
        self.remaining_secs = self.remaining_secs.saturating_sub(total_ms / 1000);
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

    /// Whole seconds left before another attempt will be considered.
    const fn remaining_secs(&self) -> u64 {
        self.remaining_secs
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
    if failed_attempts > 0 && failed_attempts.is_multiple_of(LOCKOUT_TIER_3_ATTEMPTS) {
        Some(LOCKOUT_TIER_3_SECS)
    } else if failed_attempts > 0 && failed_attempts.is_multiple_of(LOCKOUT_TIER_2_ATTEMPTS) {
        Some(LOCKOUT_TIER_2_SECS)
    } else if failed_attempts > 0 && failed_attempts.is_multiple_of(LOCKOUT_TIER_1_ATTEMPTS) {
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
            if lock_screen.config.show_date
                && let Some(ref date) = lock_screen.date
            {
                parts.push(format!("Date: {}.", date.format_long()));
            }
            parts.push("Press any key or click to unlock.".to_string());
        }
        LockScreenState::PasswordEntry => {
            let user = lock_screen.active_user();
            parts.push(format!("Unlock screen for {}.", user.display_name));
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
                if lock_screen.failed_attempts >= HINT_THRESHOLD
                    && let Some(ref hint) = user.password_hint
                {
                    parts.push(format!("Hint: {hint}."));
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

/// A list of users with at least one entry, guaranteed by construction.
///
/// `LockScreen::new` already substituted a placeholder when handed an empty
/// vector, so "there is always a user to log in as" was true — but true by
/// *convention*, established in one constructor and depended on by
/// [`LockScreen::active_user`] and its nine callers. The compiler knew none of
/// it, so `active_user` carried a fallback that read `&self.users[0]` under a
/// comment saying it should never happen. That fallback panics in exactly the
/// situation it was written to survive: if the list were ever empty, `get`
/// returns `None` and the "defensive" branch then indexes the empty vector.
///
/// Splitting the first user out of the vector makes the invariant structural.
/// There is no empty state to defend against, so there is no fallback to get
/// wrong.
#[derive(Clone, Debug)]
pub struct UserList {
    /// The user that always exists.
    first: UserInfo,
    /// Any others, in order after `first`.
    rest: Vec<UserInfo>,
}

impl UserList {
    /// Build a list, substituting a placeholder user if `users` is empty.
    ///
    /// The substitution is the behaviour `LockScreen::new` already had; it
    /// moves here so that it happens at the point the invariant is
    /// established rather than one layer above it.
    #[must_use]
    pub fn new(users: Vec<UserInfo>) -> Self {
        let mut it = users.into_iter();
        let first = it
            .next()
            .unwrap_or_else(|| UserInfo::new("user", "User", true));
        Self {
            first,
            rest: it.collect(),
        }
    }

    /// How many users there are. Never zero.
    #[must_use]
    #[allow(
        clippy::len_without_is_empty,
        reason = "an `is_empty` here would be a function that returns `false`, \
                  and its only effect would be to invite callers to write a \
                  branch that can never be taken -- which is the habit this \
                  type exists to remove"
    )]
    pub fn len(&self) -> usize {
        self.rest.len().saturating_add(1)
    }

    /// The user at `index`, or `None` if there is no such user.
    #[must_use]
    pub fn get(&self, index: usize) -> Option<&UserInfo> {
        match index {
            0 => Some(&self.first),
            // `checked_sub` rather than `index - 1`: the match arm above means
            // `index >= 1` here, but that is a fact about the match, not about
            // the type, and expressing it as a subtraction that cannot
            // underflow costs nothing.
            n => n.checked_sub(1).and_then(|i| self.rest.get(i)),
        }
    }

    /// The first user. Total, because the list cannot be empty — this is what
    /// replaces `&self.users[0]`.
    #[must_use]
    pub const fn first(&self) -> &UserInfo {
        &self.first
    }

    /// Iterate every user in order.
    pub fn iter(&self) -> impl Iterator<Item = &UserInfo> {
        core::iter::once(&self.first).chain(self.rest.iter())
    }
}

/// The lock screen application state.
///
/// No longer `Clone`: it owns a [`PasswordAuthority`], which will shortly be a
/// bus connection, and a second screen sharing one connection's failure tally
/// is not a thing that should be constructible by accident. Nothing cloned it.
#[derive(Debug)]
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
    /// Users that can log in. Never empty — see [`UserList`].
    pub users: UserList,
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
    /// Who judges a typed password. `None` means nothing can judge one, which
    /// is [`AuthOutcome::Unusable`] and not "let them in".
    authority: Option<Box<dyn PasswordAuthority>>,
    /// Set when a password attempt succeeded and the session should be
    /// unlocked; cleared by [`LockScreen::take_unlock_request`].
    ///
    /// [`LockScreen::handle_event`]'s doc comment promised this field for a
    /// long time before it existed. Until it did, the two places that submit a
    /// password both wrote `let _ = self.submit_password();`, so pressing
    /// Enter with the right password consumed the event, cleared the buffer,
    /// reset the failure count and unlocked nothing — the screen was
    /// undismissable through its own event loop, and every test that appeared
    /// to prove otherwise called `submit_password` directly.
    unlock_requested: bool,
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
        authority: Option<Box<dyn PasswordAuthority>>,
    ) -> Self {
        let users = UserList::new(users);
        Self {
            state: LockScreenState::Clock,
            screen_width: SCREEN_WIDTH,
            screen_height: SCREEN_HEIGHT,
            time: TimeOfDay {
                hour: 12,
                minute: 0,
                second: 0,
            },
            date: None,
            config,
            users,
            selected_user_index: 0,
            password_buffer: String::new(),
            failed_attempts: 0,
            show_error: false,
            shake: ShakeAnimation::new(),
            lockout: LockoutTimer::new(),
            authority,
            unlock_requested: false,
            submit_hovered: false,
            password_focused: false,
        }
    }

    /// A lock screen with a known password, for tests.
    ///
    /// `#[cfg(test)]` rather than `pub`, which is what it used to be. It has
    /// never had a caller outside this file, and a public constructor that
    /// silently installs the password `password123` on the screen guarding a
    /// user's session is the kind of thing that acquires one by accident.
    ///
    /// It names its salt and uses [`TEST_ROUNDS`] instead of drawing a fresh
    /// salt at full cost: a test must be reproducible and must not spend
    /// ~130 ms per construction. Production enrolment is
    /// [`PasswordValidator::enrol`], which does neither of those things.
    #[cfg(test)]
    fn default_single_user() -> Self {
        let user = UserInfo::new("admin", "Administrator", true)
            .with_hint("It's the name of your first pet");
        let validator = PasswordValidator::for_test("password123");
        Self::new(
            vec![user],
            LockScreenConfig::default(),
            Some(Box::new(validator)),
        )
    }

    /// Get the currently active/selected user.
    ///
    /// Falls back to the first user when the selection is out of range. That
    /// fallback is total now: [`UserList::first`] cannot fail, where the
    /// `&self.users[0]` it replaces would have panicked precisely when the
    /// list it was defending against was empty.
    pub fn active_user(&self) -> &UserInfo {
        self.users
            .get(self.selected_user_index)
            .unwrap_or_else(|| self.users.first())
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
            // An authorisation is for the account that earned it. Carrying one
            // across a switch would let a correct password for a guest account
            // unlock the administrator's session.
            self.unlock_requested = false;
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

    /// This screen's policy on an outcome that is not, by itself, a verdict.
    ///
    /// Only [`AuthOutcome::NoPassword`] is in question: `authlib` reports an
    /// empty stored entry without judging it, because a console login may
    /// reasonably let one through and a lock screen may not. Everything else
    /// is already decided by [`AuthOutcome::is_accepted`].
    ///
    /// **This currently accepts it, which is the behaviour this screen has
    /// always had, and it is an open question rather than a settled one** —
    /// see `open-questions.md`. Refusing is the secure answer and is what lane
    /// B recommends; it also means a user whose account has no password can
    /// reach a screen that will never let them back in. Both are bad and the
    /// choice is the operator's. It is one function so that changing it is one
    /// line, and it is *not* changed as part of the refactor that introduced
    /// it: a rework that quietly alters who can unlock a machine is two
    /// changes wearing one commit message.
    #[must_use]
    pub const fn unlocks_for(outcome: AuthOutcome) -> bool {
        matches!(outcome, AuthOutcome::Accepted | AuthOutcome::NoPassword)
    }

    /// Whether an unlock has been requested since this was last called, and
    /// clear it.
    ///
    /// Taking rather than reading: an unlock authorises exactly *one* unlock,
    /// not a mode. That mirrors the one-shot ticket `logind` leaves on a
    /// session — a caller that authenticated, was interrupted and walked away
    /// must not leave a screen the next person clears for free.
    pub fn take_unlock_request(&mut self) -> bool {
        core::mem::take(&mut self.unlock_requested)
    }

    /// Attempt to submit the current password, and report what came back.
    ///
    /// On a verdict this screen unlocks for ([`Self::unlocks_for`]) the unlock
    /// flag is raised, to be collected by [`Self::take_unlock_request`]. The
    /// return value is for the caller that wants to *say* something about the
    /// outcome; the flag is what actually dismisses the screen, so a caller
    /// that ignores the return value no longer silently ignores the unlock.
    pub fn submit_password(&mut self) -> AuthOutcome {
        if self.lockout.is_active() {
            return AuthOutcome::RateLimited {
                retry_after_secs: self.lockout.remaining_secs(),
            };
        }

        if !self.active_user().has_password {
            return self.settle(AuthOutcome::NoPassword);
        }

        // An empty box is not a guess, and it does not cost the user an
        // attempt. Reported as a rejection because that is what it is; the
        // difference from a wrong password is that nothing is asked and
        // nothing is tallied.
        if self.password_buffer.is_empty() {
            return AuthOutcome::Rejected;
        }

        // The username goes with the password because the authority that will
        // shortly answer this is per-machine, not per-user, and needs to know
        // whose entry to check and whose failures to count.
        let username = self.active_user().username.clone();
        let outcome = self.authority.as_mut().map_or(
            // No authority at all is a broken system, not an open door. The
            // shape this replaces asked `is_some_and(...)`, which reads as
            // "reject" and is right by accident: it collapses "wrong password"
            // and "nothing here can check a password" into one answer, and the
            // second needs an administrator rather than another guess.
            AuthOutcome::Rejected,
            |authority| authority.authenticate(&username, self.password_buffer.as_bytes()),
        );
        self.settle(outcome)
    }

    /// Apply `outcome` to the screen: raise the unlock flag or count the
    /// failure, and return it unchanged for the caller to inspect.
    fn settle(&mut self, outcome: AuthOutcome) -> AuthOutcome {
        self.password_buffer.clear();
        if Self::unlocks_for(outcome) {
            self.failed_attempts = 0;
            self.unlock_requested = true;
            return outcome;
        }

        // A failed guess revokes an authorisation already earned, so that
        // someone who walks up to a screen the real user authenticated and
        // abandoned cannot spend it. In practice the caller collects the flag
        // after every event and it never lives this long -- which is exactly
        // why it is worth writing down: the rule must hold for the caller that
        // does not, not only for the one that does.
        self.unlock_requested = false;
        self.failed_attempts = self.failed_attempts.saturating_add(1);
        self.show_error = true;
        self.shake.trigger();
        if let Some(duration) = lockout_duration_for_attempts(self.failed_attempts) {
            self.lockout.start(duration);
        }
        outcome
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
    /// An event may authorise an unlock, which is *not* in the return value —
    /// collect it with [`Self::take_unlock_request`] after each call. This
    /// doc has said so since before the flag existed; it does now.
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
                    Key::LeftShift
                    | Key::RightShift
                    | Key::LeftCtrl
                    | Key::RightCtrl
                    | Key::LeftAlt
                    | Key::RightAlt
                    | Key::LeftSuper
                    | Key::RightSuper
                    | Key::CapsLock
                    | Key::NumLock
                    | Key::ScrollLock => EventResult::Ignored,
                    _ => {
                        self.enter_password_mode();
                        // If the key was a printable character, also type it.
                        if let Some(ch) = key.text
                            && !ch.is_control()
                        {
                            self.type_char(ch);
                        }
                        EventResult::Consumed
                    }
                }
            }
            LockScreenState::PasswordEntry => match key.key {
                Key::Escape => {
                    self.return_to_clock();
                    EventResult::Consumed
                }
                Key::Enter => {
                    // The verdict is deliberately not inspected here: acting
                    // on it is `submit_password`'s own job, so that pressing
                    // Enter and clicking Submit cannot drift apart. What used
                    // to be wrong was that `submit_password` had no way to
                    // act, and this discard was the whole of the handling.
                    self.submit_password();
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
                    if let Some(ch) = key.text
                        && !ch.is_control()
                    {
                        self.type_char(ch);
                    }
                    EventResult::Consumed
                }
            },
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
                            self.submit_password();
                            return EventResult::Consumed;
                        }
                        // Check if click is on a user in the user list.
                        if self.is_multi_user()
                            && let Some(idx) = self.user_list_hit_test(mouse.x, mouse.y)
                        {
                            self.select_user(idx);
                            return EventResult::Consumed;
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
        tree.fill_rect(
            0.0,
            0.0,
            self.screen_width,
            self.screen_height,
            overlay_color,
        );
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
        let time_x = text::center_x(&time_str, cx, CLOCK_FONT_SIZE, FontWeightHint::Light);

        tree.push(RenderCommand::Text {
            x: time_x,
            y: CLOCK_Y,
            text: time_str,
            color: theme::TEXT,
            font_size: CLOCK_FONT_SIZE,
            font_weight: FontWeightHint::Light,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Date display.
        if self.config.show_date
            && let Some(ref date) = self.date
        {
            let date_str = date.format_long();
            let date_x = text::center_x(&date_str, cx, DATE_FONT_SIZE, FontWeightHint::Regular);
            let date_y = CLOCK_Y + CLOCK_FONT_SIZE + 12.0;

            tree.push(RenderCommand::Text {
                x: date_x,
                y: date_y,
                text: date_str,
                color: theme::SUBTEXT,
                font_size: DATE_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
        }

        // "Click or press any key" hint at the bottom.
        let hint_text = "Press any key or click to unlock";
        let hint_font_size: f32 = 14.0;
        let hint_x = text::center_x(hint_text, cx, hint_font_size, FontWeightHint::Regular);
        let hint_y = self.screen_height - 80.0;

        tree.push(RenderCommand::Text {
            x: hint_x,
            y: hint_y,
            text: hint_text.to_string(),
            color: theme::SUBTEXT,
            font_size: hint_font_size,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
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
        let name_x = text::center_x(name, cx, DISPLAY_NAME_FONT_SIZE, FontWeightHint::Bold);
        let name_y = avatar_y + AVATAR_DIAMETER + SECTION_GAP;

        tree.push(RenderCommand::Text {
            x: name_x,
            y: name_y,
            text: name.clone(),
            color: theme::TEXT,
            font_size: DISPLAY_NAME_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
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
            let err_x = text::center_x(error_text, cx, ERROR_FONT_SIZE, FontWeightHint::Regular)
                + shake_offset;
            let err_y = field_y + PASSWORD_FIELD_HEIGHT + 12.0;

            tree.push(RenderCommand::Text {
                x: err_x,
                y: err_y,
                text: error_text.to_string(),
                color: theme::RED,
                font_size: ERROR_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
        }

        // Lockout message.
        if self.lockout.is_active() {
            let lockout_msg = format!(
                "Too many attempts. Try again in {}",
                self.lockout.format_remaining()
            );
            let lock_x = text::center_x(&lockout_msg, cx, LOCKOUT_FONT_SIZE, FontWeightHint::Bold);
            let lock_y = field_y + PASSWORD_FIELD_HEIGHT + 32.0;

            tree.push(RenderCommand::Text {
                x: lock_x,
                y: lock_y,
                text: lockout_msg,
                color: theme::RED,
                font_size: LOCKOUT_FONT_SIZE,
                font_weight: FontWeightHint::Bold,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
        }

        // Password hint (shown after HINT_THRESHOLD failed attempts).
        if self.failed_attempts >= HINT_THRESHOLD
            && !self.lockout.is_active()
            && let Some(ref hint) = self.active_user().password_hint
        {
            let hint_str = format!("Hint: {hint}");
            let hint_x = text::center_x(&hint_str, cx, HINT_FONT_SIZE, FontWeightHint::Regular);
            let hint_y = field_y + PASSWORD_FIELD_HEIGHT + 52.0;

            tree.push(RenderCommand::Text {
                x: hint_x,
                y: hint_y,
                text: hint_str,
                color: theme::SUBTEXT,
                font_size: HINT_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
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
        let text_x = text::center_x(initials, center_x, font_size, FontWeightHint::Bold);
        let text_y = top_y + (diameter - font_size) / 2.0;

        tree.push(RenderCommand::Text {
            x: text_x,
            y: text_y,
            text: initials.clone(),
            color: theme::TEXT,
            font_size,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
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
                overflow: TextOverflow::Ellipsis,
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
                let dot_x = dots_start_x + (i as f32 * PASSWORD_DOT_SPACING) - scroll_offset
                    + PASSWORD_DOT_SPACING / 2.0
                    - dot_radius;
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
            overflow: TextOverflow::Clip,
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
        let label_x = text::center_x(label, cx, label_font_size, FontWeightHint::Regular);
        let label_y = list_start_y - 20.0;

        tree.push(RenderCommand::Text {
            x: label_x,
            y: label_y,
            text: label.to_string(),
            color: theme::SUBTEXT,
            font_size: label_font_size,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
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
            let initials_x = text::center_x(
                &user.initials,
                avatar_x + SMALL_AVATAR_DIAMETER / 2.0,
                SMALL_AVATAR_FONT_SIZE,
                FontWeightHint::Bold,
            );
            let initials_y = avatar_y + (SMALL_AVATAR_DIAMETER - SMALL_AVATAR_FONT_SIZE) / 2.0;

            tree.push(RenderCommand::Text {
                x: initials_x,
                y: initials_y,
                text: user.initials.clone(),
                color: theme::TEXT,
                font_size: SMALL_AVATAR_FONT_SIZE,
                font_weight: FontWeightHint::Bold,
                max_width: None,
                overflow: TextOverflow::Clip,
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
                overflow: TextOverflow::Ellipsis,
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
    px >= rect.x && px <= rect.x + rect.width && py >= rect.y && py <= rect.y + rect.height
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
    use std::cell::RefCell;
    use std::rc::Rc;

    // -- Helper factories --

    fn single_user_lockscreen() -> LockScreen {
        let user = UserInfo::new("alice", "Alice Johnson", true).with_hint("Name of your cat");
        let validator = PasswordValidator::for_test("correcthorse");
        LockScreen::new(
            vec![user],
            LockScreenConfig::default(),
            Some(Box::new(validator)),
        )
    }

    fn multi_user_lockscreen() -> LockScreen {
        let users = vec![
            UserInfo::new("alice", "Alice Johnson", true),
            UserInfo::new("bob", "Bob Smith", true),
            UserInfo::new("charlie", "Charlie Brown", false),
        ];
        let validator = PasswordValidator::for_test("correcthorse");
        LockScreen::new(
            users,
            LockScreenConfig::default(),
            Some(Box::new(validator)),
        )
    }

    fn no_password_lockscreen() -> LockScreen {
        let user = UserInfo::new("guest", "Guest User", false);
        LockScreen::new(vec![user], LockScreenConfig::default(), None)
    }

    /// An authority that answers with whatever it was told to, and remembers
    /// what it was asked.
    ///
    /// [`PasswordValidator`] can only ever say `Accepted` or `Rejected` — it
    /// holds one verifier and has never heard of an account — so three of the
    /// six outcomes have no way to reach the screen through it. Those three
    /// are the ones whose handling differs, which is precisely why the screen
    /// needs to be driven by something that can produce them.
    /// What a [`FakeAuthority`] was asked: the username and the raw password
    /// bytes, in order. `Vec<u8>` and not `String` on purpose — the point of
    /// the byte-string signature is that a password which is not valid UTF-8
    /// survives, and a spy that stringified it could not show that.
    type AskLog = Rc<RefCell<Vec<(String, Vec<u8>)>>>;

    #[derive(Debug)]
    struct FakeAuthority {
        answer: AuthOutcome,
        asked: Option<AskLog>,
    }

    impl FakeAuthority {
        const fn always(answer: AuthOutcome) -> Self {
            Self {
                answer,
                asked: None,
            }
        }

        const fn recording(answer: AuthOutcome, asked: AskLog) -> Self {
            Self {
                answer,
                asked: Some(asked),
            }
        }
    }

    impl PasswordAuthority for FakeAuthority {
        fn authenticate(&mut self, username: &str, password: &[u8]) -> AuthOutcome {
            if let Some(log) = self.asked.as_ref() {
                log.borrow_mut()
                    .push((username.to_string(), password.to_vec()));
            }
            self.answer
        }
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
        let v = PasswordValidator::for_test("hello");
        assert!(v.validate("hello"));
    }

    #[test]
    fn test_validator_wrong_password() {
        let v = PasswordValidator::for_test("hello");
        assert!(!v.validate("world"));
    }

    #[test]
    fn test_validator_empty_password() {
        let v = PasswordValidator::for_test("");
        assert!(v.validate(""));
        assert!(!v.validate("x"));
    }

    #[test]
    fn test_validator_unicode_password() {
        let v = PasswordValidator::for_test("\u{1F600}password\u{1F600}");
        assert!(v.validate("\u{1F600}password\u{1F600}"));
        assert!(!v.validate("password"));
    }

    // -- What the stored verifier is --
    //
    // The five tests that used to sit here checked SHA-256 against its
    // published vectors and checked `eq_constant_time` -- from a consumer, for
    // a `sha2` that already owns both (`sha2/src/lib.rs` has the same two FIPS
    // vectors and the same comparison test). They were deleted rather than
    // ported: a duplicated known-answer test does not make the answer more
    // known, and it made this file look like it had cryptographic test
    // coverage when what it actually lacked -- any check that the stored value
    // was salted or stretched at all -- had none. These replace them.

    #[test]
    fn the_stored_value_is_not_a_bare_hash_of_the_password() {
        // The defect directly. A bare `sha256(password)` is what this file
        // stored, and it is why one precomputed table opened every SlateOS
        // machine: the stored value depended on nothing but the password.
        let v = PasswordValidator::for_test("password123");
        assert_ne!(v.verifier(), sha2::sha256(b"password123"));
    }

    #[test]
    fn two_installs_store_different_values_for_one_password() {
        // What a salt is *for*. Without one these are equal, and an attacker
        // who cracks a verifier from any machine has cracked it on all of them.
        let a = PasswordValidator::for_test("password123");
        let params_b = KdfParams::new([0x11u8; pwkdf::SALT_LEN], TEST_ROUNDS);
        let b = PasswordValidator {
            verifier: PasswordVerifier::create(b"password123", params_b, VERIFIER_DOMAIN),
        };
        assert_ne!(a.verifier(), b.verifier());
        // Both still accept the password they were built from -- differing
        // stored values must not mean one of them is broken.
        assert!(a.validate("password123"));
        assert!(b.validate("password123"));
    }

    #[test]
    fn a_stored_validator_round_trips_through_its_salt_and_verifier() {
        // The path a credential store will take: write down `params` and
        // `verifier`, read them back, check a password. Losing the salt
        // rejects the correct password, which is why `params()` exists.
        let original = PasswordValidator::for_test("correcthorse");
        let reloaded = PasswordValidator::from_stored(original.params(), original.verifier());
        assert!(reloaded.validate("correcthorse"));
        assert!(!reloaded.validate("correcthorsf"));

        let wrong_salt = PasswordValidator::from_stored(
            KdfParams::new([0xFFu8; pwkdf::SALT_LEN], TEST_ROUNDS),
            original.verifier(),
        );
        assert!(!wrong_salt.validate("correcthorse"));
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
        assert_eq!(ls.submit_password(), AuthOutcome::Accepted);
        assert_eq!(ls.failed_attempts, 0);
        assert!(ls.take_unlock_request());
    }

    #[test]
    fn test_submit_wrong_password() {
        let mut ls = single_user_lockscreen();
        ls.enter_password_mode();
        for ch in "wrongpassword".chars() {
            ls.type_char(ch);
        }
        assert_eq!(ls.submit_password(), AuthOutcome::Rejected);
        assert_eq!(ls.failed_attempts, 1);
        assert!(ls.show_error);
        assert!(!ls.take_unlock_request());
    }

    #[test]
    fn test_submit_empty_password() {
        let mut ls = single_user_lockscreen();
        ls.enter_password_mode();
        assert_eq!(ls.submit_password(), AuthOutcome::Rejected);
        assert_eq!(ls.failed_attempts, 0); // Empty submit doesn't count as failure.
        assert!(!ls.take_unlock_request());
    }

    #[test]
    fn test_no_password_user_unlocks_immediately() {
        let mut ls = no_password_lockscreen();
        ls.enter_password_mode();
        // Reported as `NoPassword`, not `Accepted`: nothing was checked. The
        // screen unlocks for it only because `unlocks_for` says so, which is
        // the open question that function documents.
        assert_eq!(ls.submit_password(), AuthOutcome::NoPassword);
        assert!(ls.take_unlock_request());
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
        assert_eq!(
            ls.submit_password(),
            AuthOutcome::RateLimited {
                retry_after_secs: LOCKOUT_TIER_1_SECS
            },
            "a submit during lockout must say why it was refused"
        );
        assert!(!ls.take_unlock_request());
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
        assert!(
            (val - 1.0).abs() < 0.01,
            "sin(pi/2) should be ~1, got {val}"
        );
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

    // -- UserList: the non-empty invariant --

    #[test]
    fn a_lock_screen_built_from_no_users_still_has_someone_to_log_in_as() {
        let ls = LockScreen::new(Vec::new(), LockScreenConfig::default(), None);
        assert_eq!(ls.users.len(), 1);
        assert_eq!(ls.active_user().username, "user");
    }

    #[test]
    fn an_out_of_range_selection_falls_back_to_the_first_user() {
        // The fallback path `active_user` used to take by indexing `[0]`. The
        // selection is set directly rather than through `select_user`, which
        // rejects out-of-range indices -- the point is that `active_user` is
        // total even if some future caller does not go through that check.
        let mut ls = multi_user_lockscreen();
        ls.selected_user_index = 999;
        assert_eq!(
            ls.active_user().username,
            ls.users.first().username,
            "an out-of-range selection did not resolve to the first user"
        );
    }

    #[test]
    fn a_user_list_iterates_and_indexes_in_the_order_it_was_given() {
        let list = UserList::new(vec![
            UserInfo::new("a", "A", true),
            UserInfo::new("b", "B", true),
            UserInfo::new("c", "C", true),
        ]);
        let names: Vec<&str> = list.iter().map(|u| u.username.as_str()).collect();
        assert_eq!(names, ["a", "b", "c"]);
        assert_eq!(list.len(), 3);
        assert_eq!(list.get(2).map(|u| u.username.as_str()), Some("c"));
        assert!(list.get(3).is_none());
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
        // Consuming the event is the weaker half. This test used to stop at
        // the line above, which `handle_key` would have satisfied by doing
        // nothing at all -- and for a while that is close to what it did.
        assert!(
            ls.take_unlock_request(),
            "pressing Enter with the right password must authorise an unlock"
        );
    }

    #[test]
    fn pressing_enter_with_the_wrong_password_authorises_nothing() {
        let mut ls = single_user_lockscreen();
        ls.enter_password_mode();
        for ch in "wronghorse".chars() {
            ls.type_char(ch);
        }
        ls.handle_event(&Event::Key(KeyEvent {
            key: Key::Enter,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        }));

        assert!(!ls.take_unlock_request());
        assert_eq!(ls.failed_attempts, 1);
    }

    #[test]
    fn clicking_submit_with_the_right_password_authorises_an_unlock() {
        let mut ls = single_user_lockscreen();
        ls.enter_password_mode();
        for ch in "correcthorse".chars() {
            ls.type_char(ch);
        }
        // Through the same door a mouse uses, because the two submit paths
        // were separately written and could separately stop working.
        let button = ls.submit_button_rect();
        ls.handle_event(&Event::Mouse(MouseEvent {
            x: button.x + button.width / 2.0,
            y: button.y + button.height / 2.0,
            kind: MouseEventKind::Press(MouseButton::Left),
        }));

        assert!(ls.take_unlock_request());
    }

    #[test]
    fn an_unlock_is_authorised_once_and_not_left_standing() {
        let mut ls = single_user_lockscreen();
        ls.enter_password_mode();
        for ch in "correcthorse".chars() {
            ls.type_char(ch);
        }
        ls.submit_password();

        assert!(ls.take_unlock_request());
        assert!(
            !ls.take_unlock_request(),
            "one accepted password authorises one unlock, not a mode"
        );
    }

    #[test]
    fn a_failed_guess_revokes_an_authorisation_nobody_collected() {
        let mut ls = single_user_lockscreen();
        ls.enter_password_mode();
        for ch in "correcthorse".chars() {
            ls.type_char(ch);
        }
        ls.submit_password();

        // The real user authenticated, was interrupted, and walked away
        // without the screen being dismissed. Someone else guesses.
        ls.type_char('x');
        ls.submit_password();

        assert!(
            !ls.take_unlock_request(),
            "a wrong guess must spend the authorisation it did not earn"
        );
    }

    #[test]
    fn switching_user_revokes_an_authorisation_earned_by_the_other_account() {
        let mut ls = multi_user_lockscreen();
        ls.enter_password_mode();
        for ch in "correcthorse".chars() {
            ls.type_char(ch);
        }
        ls.submit_password();

        ls.select_user(1);

        assert!(
            !ls.take_unlock_request(),
            "an authorisation belongs to the account that earned it"
        );
    }

    #[test]
    fn a_screen_with_nothing_to_check_a_password_against_reports_a_fault() {
        // Not "rejected": a user whose account has a password, on a screen
        // holding no authority, is a broken machine rather than a bad typist,
        // and the difference decides whether an administrator is called.
        let user = UserInfo::new("alice", "Alice Johnson", true);
        let mut ls = LockScreen::new(vec![user], LockScreenConfig::default(), None);
        ls.enter_password_mode();
        ls.type_char('x');

        let outcome = ls.submit_password();
        assert_eq!(outcome, AuthOutcome::Unusable);
        assert!(outcome.needs_administrator());
        assert!(!ls.take_unlock_request());
    }

    #[test]
    fn a_locked_account_is_told_apart_from_a_typo_without_being_shown_apart() {
        for outcome in [
            AuthOutcome::Rejected,
            AuthOutcome::Locked,
            AuthOutcome::Unusable,
        ] {
            let mut ls = LockScreen::new(
                vec![UserInfo::new("alice", "Alice Johnson", true)],
                LockScreenConfig::default(),
                Some(Box::new(FakeAuthority::always(outcome))),
            );
            ls.enter_password_mode();
            ls.type_char('x');

            assert_eq!(ls.submit_password(), outcome);
            assert!(!ls.take_unlock_request());
            // Same words for all three: which one an attacker hit is free
            // information. The screen still knows the difference.
            assert_eq!(outcome.user_message(), "Incorrect password");
        }
        assert!(!AuthOutcome::Rejected.needs_administrator());
        assert!(AuthOutcome::Locked.needs_administrator());
        assert!(AuthOutcome::Unusable.needs_administrator());
    }

    #[test]
    fn an_empty_stored_entry_is_not_by_itself_an_acceptance() {
        // The distinction the whole `NoPassword` variant exists to carry: the
        // verdict is "there was nothing to check", and only this screen's own
        // policy turns that into an unlock. Collapsing it into `is_accepted`
        // would decide the question in `open-questions.md` by accident, in a
        // library function, for every caller at once.
        assert!(!AuthOutcome::NoPassword.is_accepted());
        assert!(AuthOutcome::Accepted.is_accepted());
        assert!(
            LockScreen::unlocks_for(AuthOutcome::NoPassword),
            "the screen's policy, which is the thing that is allowed to say yes"
        );
    }

    #[test]
    fn a_submitted_password_does_not_stay_in_the_buffer() {
        // Right or wrong, the typed bytes are gone afterwards. Wrong matters
        // because the next guess would otherwise be appended to this one and
        // every later attempt would fail for a reason the user cannot see;
        // right matters because a plaintext password should not outlive the
        // moment it was needed.
        let mut ls = single_user_lockscreen();
        ls.enter_password_mode();
        for ch in "wronghorse".chars() {
            ls.type_char(ch);
        }
        ls.submit_password();
        assert_eq!(ls.password_len(), 0);

        for ch in "correcthorse".chars() {
            ls.type_char(ch);
        }
        ls.submit_password();
        assert_eq!(ls.password_len(), 0);
    }

    #[test]
    fn a_correct_password_clears_the_failures_that_came_before_it() {
        // `test_submit_correct_password` asserts this too, but from a screen
        // that never failed -- so the count it checks is zero either way and
        // deleting the reset leaves it green. Here the count is genuinely
        // non-zero first, which is the only arrangement that can see it.
        let mut ls = single_user_lockscreen();
        ls.enter_password_mode();
        for _ in 0..2 {
            ls.type_char('x');
            ls.submit_password();
        }
        assert_eq!(ls.failed_attempts, 2);

        for ch in "correcthorse".chars() {
            ls.type_char(ch);
        }
        assert_eq!(ls.submit_password(), AuthOutcome::Accepted);
        assert_eq!(
            ls.failed_attempts, 0,
            "a right password must not leave the user two guesses from a lockout"
        );
    }

    #[test]
    fn the_username_reaches_the_authority_that_has_to_look_it_up() {
        // The old signature had nowhere to put it, because the screen held the
        // one verifier it would ever consult. A verifier that serves the whole
        // machine cannot work that way, and a screen that never sends the name
        // would ask it about the wrong account.
        let seen = Rc::new(RefCell::new(Vec::new()));
        let mut ls = LockScreen::new(
            vec![UserInfo::new("alice", "Alice Johnson", true)],
            LockScreenConfig::default(),
            Some(Box::new(FakeAuthority::recording(
                AuthOutcome::Rejected,
                Rc::clone(&seen),
            ))),
        );
        ls.enter_password_mode();
        for ch in "hunter2".chars() {
            ls.type_char(ch);
        }
        ls.submit_password();

        assert_eq!(
            *seen.borrow(),
            vec![("alice".to_string(), b"hunter2".to_vec())]
        );
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
        let event = Event::Resize {
            width: 2560,
            height: 1440,
        };
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
        assert!(
            tree.len() >= 4,
            "Clock view should produce at least 4 commands, got {}",
            tree.len()
        );
    }

    #[test]
    fn test_render_password_screen_produces_commands() {
        let mut ls = single_user_lockscreen();
        ls.enter_password_mode();
        ls.type_char('x');
        let tree = ls.render();
        // Should have: overlay + avatar bg + initials + name + field bg + field border
        //  + dots (clipped) + submit button + arrow
        assert!(
            tree.len() >= 8,
            "Password view should produce at least 8 commands, got {}",
            tree.len()
        );
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
        assert!(
            has_error_text,
            "Render output should include the error message"
        );
    }

    #[test]
    fn test_render_multi_user_includes_user_list() {
        let mut ls = multi_user_lockscreen();
        ls.enter_password_mode();
        let tree = ls.render();
        // Should include "Switch user" label and user entries.
        let has_switch_label = tree
            .commands
            .iter()
            .any(|cmd| matches!(cmd, RenderCommand::Text { text, .. } if text == "Switch user"));
        assert!(
            has_switch_label,
            "Multi-user render should include 'Switch user' label"
        );
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
        let rect = Rect {
            x: 10.0,
            y: 10.0,
            width: 100.0,
            height: 50.0,
        };
        assert!(hit_test(50.0, 30.0, &rect));
    }

    #[test]
    fn test_hit_test_outside() {
        let rect = Rect {
            x: 10.0,
            y: 10.0,
            width: 100.0,
            height: 50.0,
        };
        assert!(!hit_test(5.0, 30.0, &rect));
        assert!(!hit_test(50.0, 70.0, &rect));
    }

    #[test]
    fn test_hit_test_edge() {
        let rect = Rect {
            x: 10.0,
            y: 10.0,
            width: 100.0,
            height: 50.0,
        };
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
