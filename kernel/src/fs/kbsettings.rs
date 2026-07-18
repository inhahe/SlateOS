//! Keyboard settings — repeat rate, repeat delay, and input behavior.
//!
//! Configures keyboard repeat timing, Sticky Keys, Filter Keys, Toggle
//! Keys, and other keyboard accessibility/behavior settings referenced
//! in the Settings panel.
//!
//! ## Design Reference
//!
//! design.txt line 1272: "keyboard repeat speed"
//! Also implied by accessibility features (Sticky Keys, Filter Keys) and
//! general keyboard configuration needs.
//!
//! ## Architecture
//!
//! ```text
//! Keyboard driver / input subsystem
//!   → kbsettings::repeat_delay_ms()
//!   → kbsettings::repeat_rate_ms()
//!   → kbsettings::should_repeat(keycode, held_ms) → bool
//!
//! Accessibility layer
//!   → kbsettings::sticky_keys_enabled()
//!   → kbsettings::filter_keys_enabled()
//! ```

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum keyboard profiles.
const MAX_PROFILES: usize = 16;

/// Maximum custom key overrides.
const MAX_OVERRIDES: usize = 128;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Keyboard repeat speed preset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepeatPreset {
    Slow,
    Normal,
    Fast,
    VeryFast,
    Custom,
}

impl RepeatPreset {
    pub fn label(self) -> &'static str {
        match self {
            Self::Slow => "slow",
            Self::Normal => "normal",
            Self::Fast => "fast",
            Self::VeryFast => "very-fast",
            Self::Custom => "custom",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "slow" => Some(Self::Slow),
            "normal" | "default" => Some(Self::Normal),
            "fast" => Some(Self::Fast),
            "veryfast" | "very-fast" | "vf" => Some(Self::VeryFast),
            "custom" => Some(Self::Custom),
            _ => None,
        }
    }

    /// Delay and rate for preset (delay_ms, rate_ms).
    pub fn timing(self) -> (u32, u32) {
        match self {
            Self::Slow => (700, 100),
            Self::Normal => (500, 50),
            Self::Fast => (300, 30),
            Self::VeryFast => (200, 15),
            Self::Custom => (500, 50), // Placeholder; use custom values.
        }
    }
}

/// Num Lock behavior on boot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumLockBoot {
    /// Leave as-is from BIOS.
    Unchanged,
    /// Force on at boot.
    On,
    /// Force off at boot.
    Off,
}

impl NumLockBoot {
    pub fn label(self) -> &'static str {
        match self {
            Self::Unchanged => "unchanged",
            Self::On => "on",
            Self::Off => "off",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "unchanged" | "default" => Some(Self::Unchanged),
            "on" | "true" => Some(Self::On),
            "off" | "false" => Some(Self::Off),
            _ => None,
        }
    }
}

/// A named keyboard settings profile.
#[derive(Debug, Clone)]
pub struct KeyboardProfile {
    pub name: String,
    /// Whether this is the active profile.
    pub active: bool,
    /// Repeat delay (ms before repeat starts).
    pub repeat_delay_ms: u32,
    /// Repeat rate (ms between repeated keystrokes).
    pub repeat_rate_ms: u32,
    /// Repeat preset (if using a preset).
    pub preset: RepeatPreset,
}

/// Per-key repeat override.
#[derive(Debug, Clone)]
pub struct KeyOverride {
    /// Keycode.
    pub keycode: u16,
    /// Custom repeat delay (0 = use default).
    pub delay_ms: u32,
    /// Custom repeat rate (0 = use default).
    pub rate_ms: u32,
    /// Whether repeat is disabled for this key.
    pub no_repeat: bool,
}

/// Full keyboard configuration.
#[derive(Debug, Clone)]
pub struct KeyboardConfig {
    // --- Repeat settings ---
    /// Repeat delay before keys start repeating (ms).
    pub repeat_delay_ms: u32,
    /// Interval between repeated keystrokes (ms).
    pub repeat_rate_ms: u32,
    /// Active preset.
    pub preset: RepeatPreset,

    // --- Lock keys ---
    /// Num Lock behavior at boot.
    pub numlock_boot: NumLockBoot,
    /// Caps Lock behavior: true = toggle, false = momentary.
    pub caps_lock_toggle: bool,

    // --- Accessibility ---
    /// Sticky Keys: modifier keys stay active after single press.
    pub sticky_keys: bool,
    /// Sticky Keys: lock modifier on double-press.
    pub sticky_lock_on_double: bool,
    /// Filter Keys: ignore brief/repeated keystrokes.
    pub filter_keys: bool,
    /// Filter Keys: minimum hold time (ms) to register.
    pub filter_min_hold_ms: u32,
    /// Filter Keys: minimum time between accepted repeats (ms).
    pub filter_debounce_ms: u32,
    /// Toggle Keys: play sound when Caps/Num/Scroll Lock changes.
    pub toggle_keys_sound: bool,
    /// Bounce Keys: ignore rapid repeat of the same key.
    pub bounce_keys: bool,
    /// Bounce Keys: minimum ms between same-key presses.
    pub bounce_ms: u32,

    // --- Input behavior ---
    /// Whether Compose key sequences are enabled.
    pub compose_key: bool,
    /// Whether to use Ctrl+Alt as AltGr (for layouts that need it).
    pub ctrl_alt_as_altgr: bool,
}

impl Default for KeyboardConfig {
    fn default() -> Self {
        Self {
            repeat_delay_ms: 500,
            repeat_rate_ms: 50,
            preset: RepeatPreset::Normal,
            numlock_boot: NumLockBoot::Unchanged,
            caps_lock_toggle: true,
            sticky_keys: false,
            sticky_lock_on_double: true,
            filter_keys: false,
            filter_min_hold_ms: 150,
            filter_debounce_ms: 300,
            toggle_keys_sound: false,
            bounce_keys: false,
            bounce_ms: 100,
            compose_key: false,
            ctrl_alt_as_altgr: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

struct State {
    config: KeyboardConfig,
    profiles: Vec<KeyboardProfile>,
    overrides: Vec<KeyOverride>,
}

impl State {
    const fn new() -> Self {
        Self {
            config: KeyboardConfig {
                repeat_delay_ms: 500,
                repeat_rate_ms: 50,
                preset: RepeatPreset::Normal,
                numlock_boot: NumLockBoot::Unchanged,
                caps_lock_toggle: true,
                sticky_keys: false,
                sticky_lock_on_double: true,
                filter_keys: false,
                filter_min_hold_ms: 150,
                filter_debounce_ms: 300,
                toggle_keys_sound: false,
                bounce_keys: false,
                bounce_ms: 100,
                compose_key: false,
                ctrl_alt_as_altgr: false,
            },
            profiles: Vec::new(),
            overrides: Vec::new(),
        }
    }
}

static STATE: Mutex<State> = Mutex::new(State::new());
static CHANGE_COUNT: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Repeat settings
// ---------------------------------------------------------------------------

/// Get the current config.
pub fn config() -> KeyboardConfig { STATE.lock().config.clone() }

/// Apply a repeat preset.
pub fn set_preset(preset: RepeatPreset) {
    let mut state = STATE.lock();
    state.config.preset = preset;
    let (d, r) = preset.timing();
    state.config.repeat_delay_ms = d;
    state.config.repeat_rate_ms = r;
    CHANGE_COUNT.fetch_add(1, Ordering::Relaxed);
}

/// Set custom repeat delay (ms). Automatically switches to Custom preset.
pub fn set_repeat_delay(ms: u32) {
    let mut state = STATE.lock();
    state.config.repeat_delay_ms = ms.clamp(100, 2000);
    state.config.preset = RepeatPreset::Custom;
    CHANGE_COUNT.fetch_add(1, Ordering::Relaxed);
}

/// Set custom repeat rate (ms between repeats). Switches to Custom preset.
pub fn set_repeat_rate(ms: u32) {
    let mut state = STATE.lock();
    state.config.repeat_rate_ms = ms.clamp(5, 500);
    state.config.preset = RepeatPreset::Custom;
    CHANGE_COUNT.fetch_add(1, Ordering::Relaxed);
}

/// Get repeat delay.
pub fn repeat_delay_ms() -> u32 { STATE.lock().config.repeat_delay_ms }

/// Get repeat rate.
pub fn repeat_rate_ms() -> u32 { STATE.lock().config.repeat_rate_ms }

/// Check if a key should generate a repeat event.
///
/// `held_ms` is how long the key has been held. Returns true if a repeat
/// event should fire at this moment.
pub fn should_repeat(keycode: u16, held_ms: u64) -> bool {
    let state = STATE.lock();

    // Check per-key override.
    if let Some(ovr) = state.overrides.iter().find(|o| o.keycode == keycode) {
        if ovr.no_repeat { return false; }
        let delay = if ovr.delay_ms > 0 { ovr.delay_ms } else { state.config.repeat_delay_ms };
        let rate = if ovr.rate_ms > 0 { ovr.rate_ms } else { state.config.repeat_rate_ms };
        if held_ms < delay as u64 { return false; }
        let since_first = held_ms - delay as u64;
        return since_first.is_multiple_of(rate as u64);
    }

    let delay = state.config.repeat_delay_ms as u64;
    let rate = state.config.repeat_rate_ms as u64;
    if held_ms < delay { return false; }
    if rate == 0 { return false; }
    let since_first = held_ms - delay;
    since_first.is_multiple_of(rate)
}

// ---------------------------------------------------------------------------
// Per-key overrides
// ---------------------------------------------------------------------------

/// Add a per-key repeat override.
pub fn add_override(keycode: u16, delay_ms: u32, rate_ms: u32, no_repeat: bool) -> KernelResult<()> {
    let mut state = STATE.lock();
    // Replace existing.
    state.overrides.retain(|o| o.keycode != keycode);
    if state.overrides.len() >= MAX_OVERRIDES {
        return Err(KernelError::ResourceExhausted);
    }
    state.overrides.push(KeyOverride { keycode, delay_ms, rate_ms, no_repeat });
    CHANGE_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// Remove a per-key override.
pub fn remove_override(keycode: u16) -> KernelResult<()> {
    let mut state = STATE.lock();
    let len = state.overrides.len();
    state.overrides.retain(|o| o.keycode != keycode);
    if state.overrides.len() == len {
        return Err(KernelError::NotFound);
    }
    Ok(())
}

/// List overrides.
pub fn list_overrides() -> Vec<KeyOverride> {
    STATE.lock().overrides.clone()
}

// ---------------------------------------------------------------------------
// Lock key settings
// ---------------------------------------------------------------------------

pub fn set_numlock_boot(v: NumLockBoot) {
    STATE.lock().config.numlock_boot = v;
    CHANGE_COUNT.fetch_add(1, Ordering::Relaxed);
}

pub fn set_caps_lock_toggle(v: bool) {
    STATE.lock().config.caps_lock_toggle = v;
    CHANGE_COUNT.fetch_add(1, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Accessibility settings
// ---------------------------------------------------------------------------

pub fn set_sticky_keys(v: bool) {
    STATE.lock().config.sticky_keys = v;
    CHANGE_COUNT.fetch_add(1, Ordering::Relaxed);
}

pub fn set_sticky_lock_on_double(v: bool) {
    STATE.lock().config.sticky_lock_on_double = v;
    CHANGE_COUNT.fetch_add(1, Ordering::Relaxed);
}

pub fn set_filter_keys(v: bool) {
    STATE.lock().config.filter_keys = v;
    CHANGE_COUNT.fetch_add(1, Ordering::Relaxed);
}

pub fn set_filter_min_hold(ms: u32) {
    STATE.lock().config.filter_min_hold_ms = ms.clamp(50, 2000);
    CHANGE_COUNT.fetch_add(1, Ordering::Relaxed);
}

pub fn set_filter_debounce(ms: u32) {
    STATE.lock().config.filter_debounce_ms = ms.clamp(50, 2000);
    CHANGE_COUNT.fetch_add(1, Ordering::Relaxed);
}

pub fn set_toggle_keys_sound(v: bool) {
    STATE.lock().config.toggle_keys_sound = v;
    CHANGE_COUNT.fetch_add(1, Ordering::Relaxed);
}

pub fn set_bounce_keys(v: bool) {
    STATE.lock().config.bounce_keys = v;
    CHANGE_COUNT.fetch_add(1, Ordering::Relaxed);
}

pub fn set_bounce_ms(ms: u32) {
    STATE.lock().config.bounce_ms = ms.clamp(10, 2000);
    CHANGE_COUNT.fetch_add(1, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Input behavior
// ---------------------------------------------------------------------------

pub fn set_compose_key(v: bool) {
    STATE.lock().config.compose_key = v;
    CHANGE_COUNT.fetch_add(1, Ordering::Relaxed);
}

pub fn set_ctrl_alt_as_altgr(v: bool) {
    STATE.lock().config.ctrl_alt_as_altgr = v;
    CHANGE_COUNT.fetch_add(1, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Profiles
// ---------------------------------------------------------------------------

/// Create a named keyboard profile.
pub fn create_profile(name: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    if state.profiles.len() >= MAX_PROFILES {
        return Err(KernelError::ResourceExhausted);
    }
    if state.profiles.iter().any(|p| p.name == name) {
        return Err(KernelError::AlreadyExists);
    }
    let delay = state.config.repeat_delay_ms;
    let rate = state.config.repeat_rate_ms;
    let preset = state.config.preset;
    state.profiles.push(KeyboardProfile {
        name: String::from(name),
        active: false,
        repeat_delay_ms: delay,
        repeat_rate_ms: rate,
        preset,
    });
    Ok(())
}

/// Remove a profile.
pub fn remove_profile(name: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    let len = state.profiles.len();
    state.profiles.retain(|p| p.name != name);
    if state.profiles.len() == len { return Err(KernelError::NotFound); }
    Ok(())
}

/// Activate a profile (applies its settings).
pub fn activate_profile(name: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    // Deactivate all.
    for p in &mut state.profiles { p.active = false; }
    let prof = state.profiles.iter_mut().find(|p| p.name == name)
        .ok_or(KernelError::NotFound)?;
    prof.active = true;
    let delay = prof.repeat_delay_ms;
    let rate = prof.repeat_rate_ms;
    let preset = prof.preset;
    state.config.repeat_delay_ms = delay;
    state.config.repeat_rate_ms = rate;
    state.config.preset = preset;
    CHANGE_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// List profiles.
pub fn list_profiles() -> Vec<KeyboardProfile> {
    STATE.lock().profiles.clone()
}

// ---------------------------------------------------------------------------
// Defaults
// ---------------------------------------------------------------------------

/// Initialize default profiles.
pub fn init_defaults() {
    let mut state = STATE.lock();
    if !state.profiles.is_empty() { return; }

    state.profiles.push(KeyboardProfile {
        name: String::from("Normal"),
        active: true,
        repeat_delay_ms: 500,
        repeat_rate_ms: 50,
        preset: RepeatPreset::Normal,
    });
    state.profiles.push(KeyboardProfile {
        name: String::from("Gaming"),
        active: false,
        repeat_delay_ms: 200,
        repeat_rate_ms: 15,
        preset: RepeatPreset::VeryFast,
    });
    state.profiles.push(KeyboardProfile {
        name: String::from("Accessibility"),
        active: false,
        repeat_delay_ms: 700,
        repeat_rate_ms: 100,
        preset: RepeatPreset::Slow,
    });
}

// ---------------------------------------------------------------------------
// Stats
// ---------------------------------------------------------------------------

/// Returns (profile_count, override_count, changes).
pub fn stats() -> (usize, usize, u64) {
    let state = STATE.lock();
    (state.profiles.len(), state.overrides.len(), CHANGE_COUNT.load(Ordering::Relaxed))
}

pub fn reset_stats() { CHANGE_COUNT.store(0, Ordering::Relaxed); }

pub fn clear_all() {
    let mut state = STATE.lock();
    state.config = KeyboardConfig::default();
    state.profiles.clear();
    state.overrides.clear();
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

pub fn self_test() -> KernelResult<()> {
    use crate::serial_println;
    clear_all();
    reset_stats();

    // Test 1: Default config.
    serial_println!("  kbsettings::self_test 1: default config");
    let cfg = config();
    assert_eq!(cfg.repeat_delay_ms, 500);
    assert_eq!(cfg.repeat_rate_ms, 50);
    assert_eq!(cfg.preset, RepeatPreset::Normal);

    // Test 2: Presets.
    serial_println!("  kbsettings::self_test 2: presets");
    set_preset(RepeatPreset::Fast);
    let cfg2 = config();
    assert_eq!(cfg2.repeat_delay_ms, 300);
    assert_eq!(cfg2.repeat_rate_ms, 30);
    set_preset(RepeatPreset::VeryFast);
    assert_eq!(config().repeat_delay_ms, 200);

    // Test 3: Custom repeat.
    serial_println!("  kbsettings::self_test 3: custom repeat");
    set_repeat_delay(400);
    assert_eq!(config().preset, RepeatPreset::Custom);
    assert_eq!(config().repeat_delay_ms, 400);
    set_repeat_rate(25);
    assert_eq!(config().repeat_rate_ms, 25);

    // Test 4: should_repeat.
    serial_println!("  kbsettings::self_test 4: should_repeat");
    set_repeat_delay(500);
    set_repeat_rate(50);
    assert!(!should_repeat(0x1E, 200)); // Before delay.
    assert!(should_repeat(0x1E, 500));  // At delay.
    assert!(should_repeat(0x1E, 550));  // First repeat.

    // Test 5: Per-key overrides.
    serial_println!("  kbsettings::self_test 5: key overrides");
    add_override(0x39, 0, 0, true)?; // Disable repeat for space.
    assert!(!should_repeat(0x39, 1000)); // No repeat even after long hold.
    add_override(0x1C, 300, 20, false)?; // Fast repeat for Enter.
    assert!(!should_repeat(0x1C, 200));
    assert!(should_repeat(0x1C, 300));
    remove_override(0x39)?;
    remove_override(0x1C)?;

    // Test 6: Accessibility settings.
    serial_println!("  kbsettings::self_test 6: accessibility");
    set_sticky_keys(true);
    assert!(config().sticky_keys);
    set_filter_keys(true);
    set_filter_min_hold(200);
    assert_eq!(config().filter_min_hold_ms, 200);
    set_bounce_keys(true);
    set_bounce_ms(150);
    assert_eq!(config().bounce_ms, 150);
    set_toggle_keys_sound(true);
    assert!(config().toggle_keys_sound);

    // Test 7: Profiles.
    serial_println!("  kbsettings::self_test 7: profiles");
    init_defaults();
    let profs = list_profiles();
    assert!(profs.len() >= 3);
    assert!(profs.iter().any(|p| p.name == "Gaming"));
    activate_profile("Gaming")?;
    assert_eq!(config().repeat_delay_ms, 200);
    activate_profile("Normal")?;
    assert_eq!(config().repeat_delay_ms, 500);

    let (pc, oc, changes) = stats();
    assert!(pc >= 3);
    assert_eq!(oc, 0);
    assert!(changes > 0);

    clear_all();
    reset_stats();
    serial_println!("  kbsettings: all tests passed");
    Ok(())
}
