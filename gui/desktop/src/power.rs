//! Power management and screen saver module for the desktop shell.
//!
//! Provides:
//! - Power state management (screen off, suspend, hibernate, shutdown, reboot)
//! - Screen timeout with configurable delays
//! - Screen saver with multiple styles (blank, clock, matrix rain, starfield)
//! - Battery monitoring with low-battery warnings
//! - Power profiles (Balanced, Performance, Power Saver, Custom)
//! - Lid close / power button actions
//! - Wake-on-LAN configuration
//! - The power menu the start menu opens (shutdown, restart, sleep, lock,
//!   log out)
//!
//! Designed to integrate with the taskbar's power/battery indicator
//! and the settings app's power management page.

use crate::Rect;
use appearance::Palette;
use guitk::color::Color;
use guitk::render::{FontWeightHint, RenderCommand, TextOverflow};
use guitk::rng::{RandomSource, SeededRng};
use guitk::style::{Border, CornerRadii};
use guitk::text;

// ============================================================================
// Colour
// ============================================================================
//
// This module used to keep nine `const COL_*: Color` of its own, hard-coded to
// Catppuccin Mocha, which is why the battery icon stayed dark when the user
// picked the light theme. Every colour now comes from the `Palette` the caller
// resolved. Four judgements are worth stating, because a reader will otherwise
// assume the module simply forgot to follow the theme:
//
// 1. *The screen saver follows neither the mode nor the accent.* It paints the
//    whole display black in both modes — that is the point of a screen saver,
//    and a light one would be a lamp pointed at a sleeping user. Latte's roles
//    are picked to sit on white: its `text` is nearly black and would vanish on
//    that surface. And nothing the saver draws is a position or an invitation —
//    a clock, a star field, rain, a logo — so nothing in it takes the accent
//    either. It therefore takes no palette argument at all, which makes its
//    independence structural rather than something a test has to keep asserting.
//    See [`screen_palette`].
//
// 2. *The battery gauge's colour is a measurement, not a decoration.* Red at
//    critical, yellow at low, green when nearly full, blue otherwise — a scale
//    the user reads a number off. A scale that moved with the accent would say
//    something different on every machine, so it is frozen to named hues. Note
//    the "otherwise" branch is `p.blue` and *not* `p.accent`, even though the
//    stock accent happens to be blue.
//
// 3. *The power-profile badge is a category, for the same reason.* Four
//    profiles, four fixed hues, none of them the accent.
//
// 4. *The star field and the matrix rain compute their colours.* A star's grey
//    is its depth and a glyph's green is its age in the column; both are ramps,
//    not roles, and neither is a palette lookup that was missed.

/// The palette the screen saver draws with.
///
/// The saver's background is black whatever the user's mode is, so the light
/// palette is the wrong palette for that surface — see judgement 1 above. These
/// are still the project's roles rather than nine numbers copied into this file;
/// what is pinned is *which* palette, not the values in it.
fn screen_palette() -> Palette {
    Palette::for_mode(false)
}

// ============================================================================
// Power states and actions
// ============================================================================

/// System power state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerState {
    /// Normal operation, screen on.
    Active,
    /// Screen dimmed (pre-timeout warning).
    Dimmed,
    /// Screen off, system still running.
    ScreenOff,
    /// Screen saver active.
    ScreenSaver,
    /// System suspended to RAM (S3).
    Suspended,
    /// System hibernated to disk (S4).
    Hibernated,
    /// System shutting down.
    ShuttingDown,
    /// System rebooting.
    Rebooting,
}

/// Action to take on a power event (lid close, power button, etc.).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerAction {
    /// Do nothing.
    Nothing,
    /// Turn off the screen only.
    ScreenOff,
    /// Suspend to RAM.
    Suspend,
    /// Hibernate to disk.
    Hibernate,
    /// Shut down the system.
    Shutdown,
    /// Lock the screen.
    Lock,
}

/// Power profile presets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerProfile {
    /// Balanced power and performance (default).
    Balanced,
    /// Maximum performance, ignore battery life.
    Performance,
    /// Maximum battery life, reduce performance.
    PowerSaver,
    /// User-customized settings.
    Custom,
}

/// Battery charge state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatteryState {
    /// Running on AC power, battery full or not present.
    AcPower,
    /// Charging on AC power.
    Charging,
    /// Discharging on battery.
    Discharging,
    /// Battery critically low (< 5%).
    Critical,
    /// No battery present (desktop PC).
    NoBattery,
}

/// Screen saver style.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenSaverStyle {
    /// Blank screen (just black).
    Blank,
    /// Floating clock.
    Clock,
    /// Matrix-style falling characters.
    MatrixRain,
    /// Starfield simulation.
    Starfield,
    /// Bouncing logo.
    BouncingLogo,
    /// Disabled (go straight to screen off).
    Disabled,
}

// ============================================================================
// Configuration
// ============================================================================

/// Power management configuration.
#[derive(Debug, Clone)]
pub struct PowerConfig {
    /// Active power profile.
    pub profile: PowerProfile,
    /// Screen dim timeout in seconds (0 = never).
    pub dim_timeout_secs: u32,
    /// Screen off timeout in seconds (0 = never, must be > dim_timeout).
    pub screen_off_timeout_secs: u32,
    /// Suspend timeout in seconds (0 = never).
    pub suspend_timeout_secs: u32,
    /// Screen saver activation timeout in seconds (0 = disabled).
    pub screensaver_timeout_secs: u32,
    /// Screen saver style.
    pub screensaver_style: ScreenSaverStyle,
    /// Action on lid close (laptops).
    pub lid_close_action: PowerAction,
    /// Action on power button press.
    pub power_button_action: PowerAction,
    /// Action on sleep button press.
    pub sleep_button_action: PowerAction,
    /// Low battery warning threshold (percent).
    pub low_battery_pct: u8,
    /// Critical battery action threshold (percent).
    pub critical_battery_pct: u8,
    /// Action when battery reaches critical level.
    pub critical_battery_action: PowerAction,
    /// Whether to enable Wake-on-LAN.
    pub wake_on_lan: bool,
    /// CPU performance governor hint.
    pub cpu_governor: CpuGovernor,
    /// Brightness level when dimmed (percent, 0-100).
    pub dim_brightness_pct: u8,
    /// Whether to show battery percentage in taskbar.
    pub show_battery_pct: bool,
}

/// CPU performance governor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CpuGovernor {
    /// Let the OS choose dynamically.
    Auto,
    /// Always max frequency.
    Performance,
    /// Balance frequency dynamically.
    OnDemand,
    /// Always min frequency.
    PowerSave,
}

impl Default for PowerConfig {
    fn default() -> Self {
        Self {
            profile: PowerProfile::Balanced,
            dim_timeout_secs: 180,        // 3 minutes
            screen_off_timeout_secs: 300, // 5 minutes
            suspend_timeout_secs: 900,    // 15 minutes
            screensaver_timeout_secs: 0,  // disabled by default
            screensaver_style: ScreenSaverStyle::Clock,
            lid_close_action: PowerAction::Suspend,
            power_button_action: PowerAction::Shutdown,
            sleep_button_action: PowerAction::Suspend,
            low_battery_pct: 20,
            critical_battery_pct: 5,
            critical_battery_action: PowerAction::Hibernate,
            wake_on_lan: false,
            cpu_governor: CpuGovernor::Auto,
            dim_brightness_pct: 30,
            show_battery_pct: true,
        }
    }
}

impl PowerConfig {
    /// Apply a power profile preset, overriding relevant fields.
    pub fn apply_profile(&mut self, profile: PowerProfile) {
        self.profile = profile;
        match profile {
            PowerProfile::Balanced => {
                self.dim_timeout_secs = 180;
                self.screen_off_timeout_secs = 300;
                self.suspend_timeout_secs = 900;
                self.cpu_governor = CpuGovernor::Auto;
            }
            PowerProfile::Performance => {
                self.dim_timeout_secs = 600;
                self.screen_off_timeout_secs = 1800;
                self.suspend_timeout_secs = 0; // never
                self.cpu_governor = CpuGovernor::Performance;
            }
            PowerProfile::PowerSaver => {
                self.dim_timeout_secs = 60;
                self.screen_off_timeout_secs = 120;
                self.suspend_timeout_secs = 300;
                self.cpu_governor = CpuGovernor::PowerSave;
            }
            PowerProfile::Custom => {
                // Don't change anything — user values are kept.
            }
        }
    }

    /// Serialize config to key=value text format.
    pub fn to_config_string(&self) -> String {
        let mut out = String::with_capacity(512);
        out.push_str("# Power Management Configuration\n");
        out.push_str(&format!("profile={}\n", profile_str(self.profile)));
        out.push_str(&format!("dim_timeout={}\n", self.dim_timeout_secs));
        out.push_str(&format!(
            "screen_off_timeout={}\n",
            self.screen_off_timeout_secs
        ));
        out.push_str(&format!("suspend_timeout={}\n", self.suspend_timeout_secs));
        out.push_str(&format!(
            "screensaver_timeout={}\n",
            self.screensaver_timeout_secs
        ));
        out.push_str(&format!(
            "screensaver_style={}\n",
            screensaver_str(self.screensaver_style)
        ));
        out.push_str(&format!(
            "lid_close_action={}\n",
            action_str(self.lid_close_action)
        ));
        out.push_str(&format!(
            "power_button_action={}\n",
            action_str(self.power_button_action)
        ));
        out.push_str(&format!(
            "sleep_button_action={}\n",
            action_str(self.sleep_button_action)
        ));
        out.push_str(&format!("low_battery_pct={}\n", self.low_battery_pct));
        out.push_str(&format!(
            "critical_battery_pct={}\n",
            self.critical_battery_pct
        ));
        out.push_str(&format!(
            "critical_battery_action={}\n",
            action_str(self.critical_battery_action)
        ));
        out.push_str(&format!("wake_on_lan={}\n", self.wake_on_lan));
        out.push_str(&format!(
            "cpu_governor={}\n",
            governor_str(self.cpu_governor)
        ));
        out.push_str(&format!("dim_brightness={}\n", self.dim_brightness_pct));
        out.push_str(&format!("show_battery_pct={}\n", self.show_battery_pct));
        out
    }

    /// Parse config from key=value text.
    pub fn from_config_string(text: &str) -> Self {
        let mut cfg = Self::default();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((key, val)) = line.split_once('=') {
                let key = key.trim();
                let val = val.trim();
                match key {
                    "profile" => cfg.profile = parse_profile(val),
                    "dim_timeout" => {
                        if let Ok(v) = val.parse::<u32>() {
                            cfg.dim_timeout_secs = v;
                        }
                    }
                    "screen_off_timeout" => {
                        if let Ok(v) = val.parse::<u32>() {
                            cfg.screen_off_timeout_secs = v;
                        }
                    }
                    "suspend_timeout" => {
                        if let Ok(v) = val.parse::<u32>() {
                            cfg.suspend_timeout_secs = v;
                        }
                    }
                    "screensaver_timeout" => {
                        if let Ok(v) = val.parse::<u32>() {
                            cfg.screensaver_timeout_secs = v;
                        }
                    }
                    "screensaver_style" => cfg.screensaver_style = parse_screensaver(val),
                    "lid_close_action" => cfg.lid_close_action = parse_action(val),
                    "power_button_action" => cfg.power_button_action = parse_action(val),
                    "sleep_button_action" => cfg.sleep_button_action = parse_action(val),
                    "low_battery_pct" => {
                        if let Ok(v) = val.parse::<u8>() {
                            cfg.low_battery_pct = v.min(100);
                        }
                    }
                    "critical_battery_pct" => {
                        if let Ok(v) = val.parse::<u8>() {
                            cfg.critical_battery_pct = v.min(100);
                        }
                    }
                    "critical_battery_action" => cfg.critical_battery_action = parse_action(val),
                    "wake_on_lan" => cfg.wake_on_lan = val == "true",
                    "cpu_governor" => cfg.cpu_governor = parse_governor(val),
                    "dim_brightness" => {
                        if let Ok(v) = val.parse::<u8>() {
                            cfg.dim_brightness_pct = v.min(100);
                        }
                    }
                    "show_battery_pct" => cfg.show_battery_pct = val == "true",
                    _ => {} // Ignore unknown keys for forward compat.
                }
            }
        }
        cfg
    }
}

// ============================================================================
// Battery info
// ============================================================================

/// Battery status information.
#[derive(Debug, Clone)]
pub struct BatteryInfo {
    /// Whether a battery is present.
    pub present: bool,
    /// Current charge percentage (0-100).
    pub charge_pct: u8,
    /// Current state.
    pub state: BatteryState,
    /// Estimated time remaining in seconds (None if unknown or charging).
    pub time_remaining_secs: Option<u32>,
    /// Battery health percentage (0-100, design capacity vs actual).
    pub health_pct: u8,
    /// Cycle count (number of full charge/discharge cycles).
    pub cycle_count: u32,
    /// Current draw in milliwatts (positive = discharging, negative = charging).
    pub power_draw_mw: i32,
    /// Battery temperature in tenths of degrees Celsius.
    pub temperature_deci_c: i16,
}

impl Default for BatteryInfo {
    fn default() -> Self {
        Self {
            present: false,
            charge_pct: 100,
            state: BatteryState::NoBattery,
            time_remaining_secs: None,
            health_pct: 100,
            cycle_count: 0,
            power_draw_mw: 0,
            temperature_deci_c: 250, // 25.0°C
        }
    }
}

impl BatteryInfo {
    /// Create info for a desktop (no battery).
    pub fn no_battery() -> Self {
        Self::default()
    }

    /// Create info for a laptop with battery.
    pub fn with_battery(charge_pct: u8, state: BatteryState) -> Self {
        Self {
            present: true,
            charge_pct: charge_pct.min(100),
            state,
            time_remaining_secs: None,
            health_pct: 100,
            cycle_count: 0,
            power_draw_mw: 0,
            temperature_deci_c: 250,
        }
    }

    /// Format remaining time as "Xh Ym" string.
    ///
    /// The battery readout in the tray and the one on the Power settings page
    /// (`power_settings::BatteryInfo::remaining_formatted`) render the same
    /// estimate from the same driver, one in seconds and one in minutes. They
    /// now share a formatter so they cannot drift apart.
    pub fn time_remaining_str(&self) -> Option<String> {
        self.time_remaining_secs
            .map(|secs| guitk::duration::coarse_minutes(u64::from(secs)))
    }

    /// Whether battery is in a warning state.
    pub fn is_warning(&self, config: &PowerConfig) -> bool {
        self.present
            && self.charge_pct <= config.low_battery_pct
            && self.state == BatteryState::Discharging
    }

    /// Whether battery is in a critical state.
    pub fn is_critical(&self, config: &PowerConfig) -> bool {
        self.present
            && self.charge_pct <= config.critical_battery_pct
            && (self.state == BatteryState::Discharging || self.state == BatteryState::Critical)
    }
}

// ============================================================================
// Power manager
// ============================================================================

/// Manages power state transitions, timeouts, and battery monitoring.
pub struct PowerManager {
    config: PowerConfig,
    battery: BatteryInfo,
    state: PowerState,
    /// Seconds since last user input (key, mouse, touch).
    idle_secs: u32,
    /// Whether a low-battery warning has been shown this discharge cycle.
    low_battery_warned: bool,
    /// Whether the critical battery action has been triggered.
    critical_action_taken: bool,
    /// History of power state transitions for diagnostics.
    transition_log: Vec<PowerTransition>,
    /// Maximum transitions to keep in log.
    max_log_entries: usize,
    /// Inhibit reasons preventing sleep/suspend.
    inhibitors: Vec<PowerInhibitor>,
    /// Next unique inhibitor ID.
    next_inhibitor_id: u32,
}

/// A recorded power state transition.
#[derive(Debug, Clone)]
pub struct PowerTransition {
    /// Timestamp (seconds since boot).
    pub timestamp_secs: u64,
    /// Previous state.
    pub from: PowerState,
    /// New state.
    pub to: PowerState,
    /// Reason for the transition.
    pub reason: TransitionReason,
}

/// Why a power transition occurred.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransitionReason {
    /// User idle timeout.
    IdleTimeout,
    /// User activity (wake from idle).
    UserActivity,
    /// Lid closed.
    LidClose,
    /// Lid opened.
    LidOpen,
    /// Power button pressed.
    PowerButton,
    /// Sleep button pressed.
    SleepButton,
    /// Critical battery level reached.
    CriticalBattery,
    /// User-initiated via menu/shortcut.
    UserRequest,
    /// System startup/boot.
    SystemBoot,
    /// Screen saver timeout.
    ScreenSaverTimeout,
}

/// An inhibitor preventing sleep/suspend.
#[derive(Debug, Clone)]
pub struct PowerInhibitor {
    /// Unique ID for this inhibitor.
    pub id: u32,
    /// Application or service name.
    pub app_name: String,
    /// Human-readable reason.
    pub reason: String,
    /// What is being inhibited.
    pub what: InhibitTarget,
}

/// What power action is being inhibited.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InhibitTarget {
    /// Prevent screen dimming.
    ScreenDim,
    /// Prevent screen off.
    ScreenOff,
    /// Prevent suspend/hibernate.
    Suspend,
    /// Prevent all idle actions.
    All,
}

impl PowerManager {
    /// Create a new power manager with default configuration.
    pub fn new() -> Self {
        Self {
            config: PowerConfig::default(),
            battery: BatteryInfo::default(),
            state: PowerState::Active,
            idle_secs: 0,
            low_battery_warned: false,
            critical_action_taken: false,
            transition_log: Vec::new(),
            max_log_entries: 100,
            inhibitors: Vec::new(),
            next_inhibitor_id: 1,
        }
    }

    /// Create with a specific configuration.
    pub fn with_config(config: PowerConfig) -> Self {
        Self {
            config,
            battery: BatteryInfo::default(),
            state: PowerState::Active,
            idle_secs: 0,
            low_battery_warned: false,
            critical_action_taken: false,
            transition_log: Vec::new(),
            max_log_entries: 100,
            inhibitors: Vec::new(),
            next_inhibitor_id: 1,
        }
    }

    /// Get current power state.
    pub fn state(&self) -> PowerState {
        self.state
    }

    /// Get current config.
    pub fn config(&self) -> &PowerConfig {
        &self.config
    }

    /// Get mutable config for updating.
    pub fn config_mut(&mut self) -> &mut PowerConfig {
        &mut self.config
    }

    /// Get battery info.
    pub fn battery(&self) -> &BatteryInfo {
        &self.battery
    }

    /// Update battery information (called periodically by battery monitor).
    pub fn update_battery(&mut self, info: BatteryInfo) {
        // Reset warning flags when we start charging.
        if info.state == BatteryState::Charging || info.state == BatteryState::AcPower {
            self.low_battery_warned = false;
            self.critical_action_taken = false;
        }
        self.battery = info;
    }

    /// Get idle time in seconds.
    pub fn idle_secs(&self) -> u32 {
        self.idle_secs
    }

    /// Report user activity — resets idle timer and wakes from idle states.
    pub fn on_user_activity(&mut self, now_secs: u64) {
        self.idle_secs = 0;
        match self.state {
            PowerState::Dimmed | PowerState::ScreenOff | PowerState::ScreenSaver => {
                self.transition_to(PowerState::Active, now_secs, TransitionReason::UserActivity);
            }
            _ => {}
        }
    }

    /// Called once per second to update idle time and check timeouts.
    /// Returns an optional `PowerAction` that the system should execute.
    pub fn tick(&mut self, now_secs: u64) -> Option<PowerAction> {
        self.idle_secs = self.idle_secs.saturating_add(1);

        // Check battery thresholds (only when discharging).
        if let Some(action) = self.check_battery_thresholds(now_secs) {
            return Some(action);
        }

        // Only process idle timeouts when active or dimmed.
        match self.state {
            PowerState::Active => self.check_active_timeouts(now_secs),
            PowerState::Dimmed => self.check_dim_timeouts(now_secs),
            PowerState::ScreenSaver => self.check_screensaver_timeouts(now_secs),
            _ => None,
        }
    }

    /// Handle lid close event.
    pub fn on_lid_close(&mut self, now_secs: u64) -> PowerAction {
        let action = self.config.lid_close_action;
        if action != PowerAction::Nothing {
            let target_state = action_to_state(action);
            self.transition_to(target_state, now_secs, TransitionReason::LidClose);
        }
        action
    }

    /// Handle lid open event.
    pub fn on_lid_open(&mut self, now_secs: u64) {
        match self.state {
            PowerState::ScreenOff | PowerState::Dimmed | PowerState::ScreenSaver => {
                self.transition_to(PowerState::Active, now_secs, TransitionReason::LidOpen);
            }
            _ => {}
        }
    }

    /// Handle power button press.
    pub fn on_power_button(&mut self, now_secs: u64) -> PowerAction {
        let action = self.config.power_button_action;
        if action != PowerAction::Nothing {
            let target_state = action_to_state(action);
            self.transition_to(target_state, now_secs, TransitionReason::PowerButton);
        }
        action
    }

    /// Handle sleep button press.
    pub fn on_sleep_button(&mut self, now_secs: u64) -> PowerAction {
        let action = self.config.sleep_button_action;
        if action != PowerAction::Nothing {
            let target_state = action_to_state(action);
            self.transition_to(target_state, now_secs, TransitionReason::SleepButton);
        }
        action
    }

    /// User-requested power action (from menu, shortcut, etc.).
    pub fn request_action(&mut self, action: PowerAction, now_secs: u64) {
        if action != PowerAction::Nothing {
            let target_state = action_to_state(action);
            self.transition_to(target_state, now_secs, TransitionReason::UserRequest);
        }
    }

    /// Add an inhibitor preventing certain power actions.
    /// Returns the inhibitor ID for later removal.
    pub fn add_inhibitor(&mut self, app_name: &str, reason: &str, what: InhibitTarget) -> u32 {
        let id = self.next_inhibitor_id;
        self.next_inhibitor_id = self.next_inhibitor_id.saturating_add(1);
        self.inhibitors.push(PowerInhibitor {
            id,
            app_name: app_name.to_string(),
            reason: reason.to_string(),
            what,
        });
        id
    }

    /// Remove an inhibitor by ID.
    pub fn remove_inhibitor(&mut self, id: u32) -> bool {
        if let Some(pos) = self.inhibitors.iter().position(|i| i.id == id) {
            self.inhibitors.remove(pos);
            true
        } else {
            false
        }
    }

    /// Get all active inhibitors.
    pub fn inhibitors(&self) -> &[PowerInhibitor] {
        &self.inhibitors
    }

    /// Check if a specific action is inhibited.
    pub fn is_inhibited(&self, target: InhibitTarget) -> bool {
        self.inhibitors
            .iter()
            .any(|i| i.what == target || i.what == InhibitTarget::All)
    }

    /// Get the transition log.
    pub fn transition_log(&self) -> &[PowerTransition] {
        &self.transition_log
    }

    /// Apply a power profile preset.
    pub fn apply_profile(&mut self, profile: PowerProfile) {
        self.config.apply_profile(profile);
    }

    // ----------------------------------------------------------------
    // Internal helpers
    // ----------------------------------------------------------------

    fn check_active_timeouts(&mut self, now_secs: u64) -> Option<PowerAction> {
        let cfg = &self.config;

        // Screen saver first (if enabled and before dim).
        if cfg.screensaver_timeout_secs > 0
            && self.idle_secs >= cfg.screensaver_timeout_secs
            && !self.is_inhibited(InhibitTarget::ScreenDim)
        {
            self.transition_to(
                PowerState::ScreenSaver,
                now_secs,
                TransitionReason::ScreenSaverTimeout,
            );
            return None; // Screen saver is internal, no system action needed.
        }

        // Dim.
        if cfg.dim_timeout_secs > 0
            && self.idle_secs >= cfg.dim_timeout_secs
            && !self.is_inhibited(InhibitTarget::ScreenDim)
        {
            self.transition_to(PowerState::Dimmed, now_secs, TransitionReason::IdleTimeout);
            return None; // Dimming is a visual change, not a system action.
        }

        None
    }

    fn check_dim_timeouts(&mut self, now_secs: u64) -> Option<PowerAction> {
        let cfg = &self.config;

        // Screen off.
        if cfg.screen_off_timeout_secs > 0
            && self.idle_secs >= cfg.screen_off_timeout_secs
            && !self.is_inhibited(InhibitTarget::ScreenOff)
        {
            self.transition_to(
                PowerState::ScreenOff,
                now_secs,
                TransitionReason::IdleTimeout,
            );
            return Some(PowerAction::ScreenOff);
        }

        None
    }

    fn check_screensaver_timeouts(&mut self, now_secs: u64) -> Option<PowerAction> {
        let cfg = &self.config;

        // After screen saver, go to screen off if configured.
        if cfg.screen_off_timeout_secs > 0
            && self.idle_secs >= cfg.screen_off_timeout_secs
            && !self.is_inhibited(InhibitTarget::ScreenOff)
        {
            self.transition_to(
                PowerState::ScreenOff,
                now_secs,
                TransitionReason::IdleTimeout,
            );
            return Some(PowerAction::ScreenOff);
        }

        // After screen off, suspend if configured.
        if cfg.suspend_timeout_secs > 0
            && self.idle_secs >= cfg.suspend_timeout_secs
            && !self.is_inhibited(InhibitTarget::Suspend)
        {
            self.transition_to(
                PowerState::Suspended,
                now_secs,
                TransitionReason::IdleTimeout,
            );
            return Some(PowerAction::Suspend);
        }

        None
    }

    fn check_battery_thresholds(&mut self, now_secs: u64) -> Option<PowerAction> {
        if !self.battery.present {
            return None;
        }
        if self.battery.state != BatteryState::Discharging
            && self.battery.state != BatteryState::Critical
        {
            return None;
        }

        // Critical threshold — take action once.
        if !self.critical_action_taken && self.battery.is_critical(&self.config) {
            self.critical_action_taken = true;
            let action = self.config.critical_battery_action;
            if action != PowerAction::Nothing {
                let target = action_to_state(action);
                self.transition_to(target, now_secs, TransitionReason::CriticalBattery);
                return Some(action);
            }
        }

        None
    }

    fn transition_to(&mut self, new_state: PowerState, now_secs: u64, reason: TransitionReason) {
        if self.state == new_state {
            return;
        }
        let transition = PowerTransition {
            timestamp_secs: now_secs,
            from: self.state,
            to: new_state,
            reason,
        };
        self.transition_log.push(transition);
        if self.transition_log.len() > self.max_log_entries {
            self.transition_log.remove(0);
        }
        self.state = new_state;
    }
}

impl Default for PowerManager {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Screen saver engine
// ============================================================================

/// A single star in the starfield screen saver.
#[derive(Debug, Clone)]
struct Star {
    x: f32,
    y: f32,
    z: f32, // depth (1.0 = far, 0.01 = close)
    speed: f32,
}

/// A falling column in the matrix rain screen saver.
#[derive(Debug, Clone)]
struct MatrixColumn {
    x: u32,
    y: f32,
    speed: f32,
    chars: Vec<u8>,
    length: u32,
}

/// How wide one column of matrix rain is, in pixels.
const COLUMN_WIDTH_PX: u32 = 14;

/// How tall one glyph of matrix rain is, in pixels.
const GLYPH_HEIGHT_PX: f32 = 16.0;

/// The shortest and longest a column of rain may be, in glyphs.
const MIN_COLUMN_GLYPHS: i64 = 5;
const MAX_COLUMN_GLYPHS: i64 = 24;

/// The printable ASCII range the rain is drawn from: `!` through `~`.
const FIRST_GLYPH: u8 = b'!';
const LAST_GLYPH: u8 = b'~';

/// How much dimmer each glyph is than the one above it, out of 255.
const FADE_PER_GLYPH: u32 = 8;

/// The most a tail glyph may be dimmed, so it never fades to invisible.
const TAIL_DIMMING: u32 = 200;

/// Screen saver renderer.
pub struct ScreenSaver {
    style: ScreenSaverStyle,
    /// Animation frame counter.
    frame: u64,
    /// Stars for starfield.
    stars: Vec<Star>,
    /// Columns for matrix rain.
    columns: Vec<MatrixColumn>,
    /// Clock position for floating clock (x, y in 0.0-1.0 range).
    clock_pos: (f32, f32),
    /// Clock velocity for floating clock.
    clock_vel: (f32, f32),
    /// Logo position for bouncing logo.
    logo_pos: (f32, f32),
    /// Logo velocity.
    logo_vel: (f32, f32),
    /// Width of the screen.
    width: u32,
    /// Height of the screen.
    height: u32,
    /// Where the scatter comes from.
    ///
    /// A screen saver wants variety, not secrecy, so a seeded generator is
    /// exactly right here — see `guitk::rng`. What was here before was the
    /// same xorshift written out three times in this one file: once as
    /// `next_random`, and twice more inlined into `render_starfield` to dodge
    /// a borrow that a disjoint field borrow solves properly.
    rng: SeededRng,
}

impl ScreenSaver {
    /// Create a new screen saver.
    pub fn new(style: ScreenSaverStyle, width: u32, height: u32) -> Self {
        let mut ss = Self {
            style,
            frame: 0,
            stars: Vec::new(),
            columns: Vec::new(),
            clock_pos: (0.3, 0.4),
            clock_vel: (0.002, 0.0015),
            logo_pos: (0.5, 0.5),
            logo_vel: (0.003, 0.002),
            width,
            height,
            rng: SeededRng::new(0x1234_5678_9ABC_DEF0),
        };
        ss.init();
        ss
    }

    fn init(&mut self) {
        match self.style {
            ScreenSaverStyle::Starfield => self.init_starfield(),
            ScreenSaverStyle::MatrixRain => self.init_matrix(),
            _ => {}
        }
    }

    fn init_starfield(&mut self) {
        self.stars.clear();
        for _ in 0..200 {
            let x = self.rng.between_f32(-1.0, 1.0);
            let y = self.rng.between_f32(-1.0, 1.0);
            let z = self.rng.between_f32(0.01, 1.0);
            let speed = self.rng.between_f32(0.002, 0.012);
            self.stars.push(Star { x, y, z, speed });
        }
    }

    fn init_matrix(&mut self) {
        self.columns.clear();
        let col_count = self.width / COLUMN_WIDTH_PX;
        for i in 0..col_count {
            let len = self.rng.between(MIN_COLUMN_GLYPHS, MAX_COLUMN_GLYPHS);
            let glyphs = usize::try_from(len).unwrap_or(0);
            let mut chars = Vec::with_capacity(glyphs);
            for _ in 0..glyphs {
                chars.push(self.random_glyph());
            }
            let col_y = -self.rng.between_f32(0.0, self.height as f32);
            let col_speed = self.rng.between_f32(1.0, 4.0);
            self.columns.push(MatrixColumn {
                // `col_count` is `width / COLUMN_WIDTH_PX`, so this cannot
                // reach the screen edge, let alone overflow — but saying so in
                // the arithmetic beats saying so in a comment.
                x: i.saturating_mul(COLUMN_WIDTH_PX),
                y: col_y,
                speed: col_speed,
                chars,
                length: u32::try_from(glyphs).unwrap_or(0),
            });
        }
    }

    /// One printable ASCII character, which is what the rain is made of.
    fn random_glyph(&mut self) -> u8 {
        let code = self
            .rng
            .between(i64::from(FIRST_GLYPH), i64::from(LAST_GLYPH));
        u8::try_from(code).unwrap_or(FIRST_GLYPH)
    }

    /// Advance one frame and produce render commands.
    pub fn render_frame(&mut self) -> Vec<RenderCommand> {
        self.frame = self.frame.wrapping_add(1);
        match self.style {
            ScreenSaverStyle::Blank => self.render_blank(),
            ScreenSaverStyle::Clock => self.render_clock(),
            ScreenSaverStyle::MatrixRain => self.render_matrix(),
            ScreenSaverStyle::Starfield => self.render_starfield(),
            ScreenSaverStyle::BouncingLogo => self.render_bouncing_logo(),
            ScreenSaverStyle::Disabled => Vec::new(),
        }
    }

    fn render_blank(&self) -> Vec<RenderCommand> {
        vec![RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.width as f32,
            height: self.height as f32,
            color: Color::from_hex(0x000000),
            corner_radii: CornerRadii::ZERO,
        }]
    }

    fn render_clock(&mut self) -> Vec<RenderCommand> {
        let mut cmds = Vec::with_capacity(4);

        // Black background.
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.width as f32,
            height: self.height as f32,
            color: Color::from_hex(0x000000),
            corner_radii: CornerRadii::ZERO,
        });

        // Bounce clock around.
        self.clock_pos.0 += self.clock_vel.0;
        self.clock_pos.1 += self.clock_vel.1;
        if self.clock_pos.0 <= 0.0 || self.clock_pos.0 >= 0.8 {
            self.clock_vel.0 = -self.clock_vel.0;
            self.clock_pos.0 = self.clock_pos.0.clamp(0.0, 0.8);
        }
        if self.clock_pos.1 <= 0.0 || self.clock_pos.1 >= 0.85 {
            self.clock_vel.1 = -self.clock_vel.1;
            self.clock_pos.1 = self.clock_pos.1.clamp(0.0, 0.85);
        }

        let x = self.clock_pos.0 * self.width as f32;
        let y = self.clock_pos.1 * self.height as f32;

        // Render "HH:MM" placeholder (real time would come from system).
        cmds.push(RenderCommand::Text {
            x,
            y,
            text: "12:00".to_string(),
            color: screen_palette().lavender,
            font_size: 72.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        cmds
    }

    fn render_starfield(&mut self) -> Vec<RenderCommand> {
        let mut cmds = Vec::with_capacity(self.stars.len().saturating_add(1));

        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.width as f32,
            height: self.height as f32,
            color: Color::from_hex(0x000000),
            corner_radii: CornerRadii::ZERO,
        });

        let cx = self.width as f32 / 2.0;
        let cy = self.height as f32 / 2.0;
        let w = self.width as f32;
        let h = self.height as f32;

        // Borrow the two fields separately rather than the whole `self`, so
        // the generator can be used while the stars are being walked. The
        // previous version copied the PRNG state into a local and inlined the
        // xorshift twice to work around the borrow — which is how this file
        // came to contain three copies of one generator.
        let Self { stars, rng, .. } = self;
        for star in stars.iter_mut() {
            star.z -= star.speed;
            if star.z <= 0.01 {
                // Recycle the star to a fresh position at the far plane.
                star.x = rng.between_f32(-1.0, 1.0);
                star.y = rng.between_f32(-1.0, 1.0);
                star.z = 1.0;
            }

            let sx = cx + (star.x / star.z) * cx;
            let sy = cy + (star.y / star.z) * cy;

            if sx >= 0.0 && sx < w && sy >= 0.0 && sy < h {
                let brightness = ((1.0 - star.z) * 255.0) as u8;
                let size = ((1.0 - star.z) * 3.0 + 1.0).max(1.0);
                cmds.push(RenderCommand::FillRect {
                    x: sx,
                    y: sy,
                    width: size,
                    height: size,
                    color: Color::rgba(brightness, brightness, brightness, 255),
                    corner_radii: CornerRadii::ZERO,
                });
            }
        }

        cmds
    }

    fn render_matrix(&mut self) -> Vec<RenderCommand> {
        // A rough guess at the glyph count, to save a few reallocations.
        let mut cmds = Vec::with_capacity(self.columns.len().saturating_mul(10).saturating_add(1));

        // Semi-transparent black overlay for trail effect.
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.width as f32,
            height: self.height as f32,
            color: Color::rgba(0, 0, 0, 220),
            corner_radii: CornerRadii::ZERO,
        });

        let h = self.height as f32;
        let height = self.height as f32;
        let Self { columns, rng, .. } = self;
        for col in columns.iter_mut() {
            col.y += col.speed;
            if col.y > h + (col.length as f32 * GLYPH_HEIGHT_PX) {
                col.y = -(col.length as f32 * GLYPH_HEIGHT_PX);
                // Draw a fresh column. This used to add 7 to each existing
                // character code — the same shift for every column, every
                // time, so the "randomised" rain repeated a fixed cycle of
                // ninety-four frames and every column showed the same one.
                for c in &mut col.chars {
                    let code = rng.between(i64::from(FIRST_GLYPH), i64::from(LAST_GLYPH));
                    *c = u8::try_from(code).unwrap_or(FIRST_GLYPH);
                }
            }

            for (i, ch) in col.chars.iter().enumerate() {
                let cy = col.y + (i as f32 * GLYPH_HEIGHT_PX);
                if cy < 0.0 || cy >= height {
                    continue;
                }
                // The head of the column is brightest and the tail fades, but
                // never past `TAIL_DIMMING` so the oldest glyphs stay legible.
                let fade = u32::try_from(i)
                    .unwrap_or(u32::MAX)
                    .saturating_mul(FADE_PER_GLYPH)
                    .min(TAIL_DIMMING);
                let green =
                    u8::try_from(u32::from(u8::MAX).saturating_sub(fade)).unwrap_or(u8::MAX);
                cmds.push(RenderCommand::Text {
                    x: col.x as f32,
                    y: cy,
                    text: String::from(*ch as char),
                    color: Color::rgba(0, green, 0, 255),
                    font_size: 14.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(COLUMN_WIDTH_PX as f32),
                    overflow: TextOverflow::Ellipsis,
                });
            }
        }

        cmds
    }

    fn render_bouncing_logo(&mut self) -> Vec<RenderCommand> {
        let mut cmds = Vec::with_capacity(4);

        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.width as f32,
            height: self.height as f32,
            color: Color::from_hex(0x000000),
            corner_radii: CornerRadii::ZERO,
        });

        // Bounce.
        self.logo_pos.0 += self.logo_vel.0;
        self.logo_pos.1 += self.logo_vel.1;

        let logo_w: f32 = 120.0;
        let logo_h: f32 = 60.0;
        let max_x = (self.width as f32 - logo_w) / self.width as f32;
        let max_y = (self.height as f32 - logo_h) / self.height as f32;

        if self.logo_pos.0 <= 0.0 || self.logo_pos.0 >= max_x {
            self.logo_vel.0 = -self.logo_vel.0;
            self.logo_pos.0 = self.logo_pos.0.clamp(0.0, max_x);
        }
        if self.logo_pos.1 <= 0.0 || self.logo_pos.1 >= max_y {
            self.logo_vel.1 = -self.logo_vel.1;
            self.logo_pos.1 = self.logo_pos.1.clamp(0.0, max_y);
        }

        let x = self.logo_pos.0 * self.width as f32;
        let y = self.logo_pos.1 * self.height as f32;

        // Logo background.
        let radii = CornerRadii::all(8.0);
        let sp = screen_palette();
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width: logo_w,
            height: logo_h,
            color: sp.blue,
            corner_radii: radii,
        });

        // Logo text. Derived from the fill rather than named, because it is ink
        // *on* the logo: name it and the day the logo's hue changes the label
        // stops being readable, silently and only on that one screen.
        cmds.push(RenderCommand::Text {
            x: x + 15.0,
            y: y + 15.0,
            text: "Slate OS".to_string(),
            color: appearance::readable_on(sp.blue),
            font_size: 28.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(logo_w - 30.0),
            overflow: TextOverflow::Ellipsis,
        });

        cmds
    }
}

// ============================================================================
// Battery icon rendering
// ============================================================================

/// Render a battery indicator for the taskbar.
pub fn render_battery_icon(
    battery: &BatteryInfo,
    config: &PowerConfig,
    p: &Palette,
    x: f32,
    y: f32,
) -> Vec<RenderCommand> {
    let mut cmds = Vec::with_capacity(6);

    if !battery.present {
        // No battery — show AC power icon hint.
        cmds.push(RenderCommand::Text {
            x,
            y: y + 2.0,
            text: "AC".to_string(),
            color: p.subtext0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(24.0),
            overflow: TextOverflow::Ellipsis,
        });
        return cmds;
    }

    let batt_w: f32 = 22.0;
    let batt_h: f32 = 12.0;
    let tip_w: f32 = 3.0;
    let tip_h: f32 = 6.0;

    // Battery outline.
    cmds.push(RenderCommand::StrokeRect {
        x,
        y: y + 2.0,
        width: batt_w,
        height: batt_h,
        color: p.text,
        line_width: 1.0,
        corner_radii: CornerRadii::all(2.0),
    });

    // Battery tip.
    cmds.push(RenderCommand::FillRect {
        x: x + batt_w,
        y: y + 2.0 + (batt_h - tip_h) / 2.0,
        width: tip_w,
        height: tip_h,
        color: p.text,
        corner_radii: CornerRadii {
            top_left: 0.0,
            top_right: 2.0,
            bottom_right: 2.0,
            bottom_left: 0.0,
        },
    });

    // Fill level.
    let fill_pct = battery.charge_pct as f32;
    let fill_w = ((batt_w - 4.0) * fill_pct) / 100.0;
    // A measurement, not a decoration: see judgement 2 at the top of the file.
    // The last arm is `p.blue` and must stay `p.blue` — an accent here would
    // make "enough charge" mean a different colour on every machine, and would
    // collide with whatever accent a user picked for red or green.
    let fill_color = if battery.is_critical(config) {
        p.red
    } else if battery.is_warning(config) {
        p.yellow
    } else if battery.charge_pct > 80 {
        p.green
    } else {
        p.blue
    };

    if fill_w > 0.0 {
        cmds.push(RenderCommand::FillRect {
            x: x + 2.0,
            y: y + 4.0,
            width: fill_w,
            height: batt_h - 4.0,
            color: fill_color,
            corner_radii: CornerRadii::ZERO,
        });
    }

    // Charging indicator.
    if battery.state == BatteryState::Charging {
        cmds.push(RenderCommand::Text {
            x: x + 5.0,
            y: y + 2.0,
            text: "\u{26A1}".to_string(), // ⚡
            color: p.yellow,
            font_size: 10.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(batt_w),
            overflow: TextOverflow::Ellipsis,
        });
    }

    // Percentage text.
    if config.show_battery_pct {
        cmds.push(RenderCommand::Text {
            x: x + batt_w + tip_w + 4.0,
            y: y + 2.0,
            text: format!("{}%", battery.charge_pct),
            color: p.subtext0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(36.0),
            overflow: TextOverflow::Ellipsis,
        });
    }

    cmds
}

/// Render a power profile indicator (for settings or quick settings).
pub fn render_power_profile_badge(
    profile: PowerProfile,
    p: &Palette,
    x: f32,
    y: f32,
) -> Vec<RenderCommand> {
    // A category, not a ranking: see judgement 3 at the top of the file. None of
    // these is the accent, including Balanced — the accent means "this is where
    // you are", and every machine is in one of these four states at all times.
    let (label, color) = match profile {
        PowerProfile::Balanced => ("Balanced", p.blue),
        PowerProfile::Performance => ("Performance", p.peach),
        PowerProfile::PowerSaver => ("Power Saver", p.green),
        PowerProfile::Custom => ("Custom", p.lavender),
    };

    let badge_w = text::padded_width(label, 8.0, 12.0, FontWeightHint::Regular);
    let badge_h: f32 = 22.0;

    vec![
        RenderCommand::FillRect {
            x,
            y,
            width: badge_w,
            height: badge_h,
            color: Color::rgba(color.r, color.g, color.b, 40),
            corner_radii: CornerRadii::all(4.0),
        },
        RenderCommand::Text {
            x: x + 8.0,
            y: y + 4.0,
            text: label.to_string(),
            color,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(badge_w - 16.0),
            overflow: TextOverflow::Ellipsis,
        },
    ]
}

// ============================================================================
// Power menu
// ============================================================================

/// One entry of the power menu — "Shutdown", "Restart", …
///
/// The rectangle is supplied rather than computed here because the shell's hit
/// test has to agree with the drawing to the pixel, and the hit test lives with
/// the rest of the shell's geometry. See the `Rect` documentation in `main.rs`.
#[derive(Clone, Copy, Debug)]
pub struct PowerMenuRow<'a> {
    /// What the user reads.
    pub label: &'a str,
    /// Where the row is drawn, and where a click on it is accepted.
    pub rect: Rect,
}

/// The colours and sizes a power menu is drawn with.
///
/// Passed in whole rather than taken from this module's own palette: the popup
/// belongs to the start menu it opens from and has to follow the user's theme,
/// font size and display scaling — all of which the shell owns and this module
/// has no way to see.
#[derive(Clone, Copy, Debug)]
pub struct PowerMenuStyle {
    /// The popup's panel colour.
    pub background: Color,
    /// Label colour.
    pub foreground: Color,
    /// The panel's outline.
    pub border: Border,
    /// Corner rounding of the panel.
    pub radii: CornerRadii,
    /// Label size in physical pixels — already scaled by the caller.
    pub font_size: f32,
    /// Distance from a row's left edge to the start of its label.
    pub text_inset: f32,
}

/// Draw a power menu: a panel, and one label per row.
///
/// The drop shadow is not drawn here. Every floating surface in the shell casts
/// the same one, and only when the user has shadows switched on — that is one
/// decision belonging to the shell, not five surfaces each making it again.
#[must_use]
pub fn render_power_menu(
    panel: Rect,
    rows: &[PowerMenuRow<'_>],
    style: PowerMenuStyle,
) -> Vec<RenderCommand> {
    // Panel, outline, and one label per row.
    let mut cmds = Vec::with_capacity(rows.len().saturating_add(2));

    cmds.push(RenderCommand::FillRect {
        x: panel.x,
        y: panel.y,
        width: panel.w,
        height: panel.h,
        color: style.background,
        corner_radii: style.radii,
    });
    cmds.push(RenderCommand::StrokeRect {
        x: panel.x,
        y: panel.y,
        width: panel.w,
        height: panel.h,
        color: style.border.color,
        line_width: style.border.width,
        corner_radii: style.radii,
    });

    for row in rows {
        cmds.push(RenderCommand::Text {
            x: row.rect.x + style.text_inset,
            // Centred in the row rather than offset by a constant, so a larger
            // font size does not drift the label towards the row's bottom edge.
            y: row.rect.y + (row.rect.h - style.font_size).max(0.0) / 2.0,
            text: row.label.to_string(),
            color: style.foreground,
            font_size: style.font_size,
            font_weight: FontWeightHint::Regular,
            max_width: Some((row.rect.w - style.text_inset * 2.0).max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });
    }

    cmds
}

// ============================================================================
// Config serialization helpers
// ============================================================================

fn profile_str(p: PowerProfile) -> &'static str {
    match p {
        PowerProfile::Balanced => "balanced",
        PowerProfile::Performance => "performance",
        PowerProfile::PowerSaver => "powersaver",
        PowerProfile::Custom => "custom",
    }
}

fn parse_profile(s: &str) -> PowerProfile {
    match s {
        "balanced" => PowerProfile::Balanced,
        "performance" => PowerProfile::Performance,
        "powersaver" => PowerProfile::PowerSaver,
        "custom" => PowerProfile::Custom,
        _ => PowerProfile::Balanced,
    }
}

fn action_str(a: PowerAction) -> &'static str {
    match a {
        PowerAction::Nothing => "nothing",
        PowerAction::ScreenOff => "screenoff",
        PowerAction::Suspend => "suspend",
        PowerAction::Hibernate => "hibernate",
        PowerAction::Shutdown => "shutdown",
        PowerAction::Lock => "lock",
    }
}

fn parse_action(s: &str) -> PowerAction {
    match s {
        "nothing" => PowerAction::Nothing,
        "screenoff" => PowerAction::ScreenOff,
        "suspend" => PowerAction::Suspend,
        "hibernate" => PowerAction::Hibernate,
        "shutdown" => PowerAction::Shutdown,
        "lock" => PowerAction::Lock,
        _ => PowerAction::Nothing,
    }
}

fn screensaver_str(s: ScreenSaverStyle) -> &'static str {
    match s {
        ScreenSaverStyle::Blank => "blank",
        ScreenSaverStyle::Clock => "clock",
        ScreenSaverStyle::MatrixRain => "matrix",
        ScreenSaverStyle::Starfield => "starfield",
        ScreenSaverStyle::BouncingLogo => "bouncing",
        ScreenSaverStyle::Disabled => "disabled",
    }
}

fn parse_screensaver(s: &str) -> ScreenSaverStyle {
    match s {
        "blank" => ScreenSaverStyle::Blank,
        "clock" => ScreenSaverStyle::Clock,
        "matrix" => ScreenSaverStyle::MatrixRain,
        "starfield" => ScreenSaverStyle::Starfield,
        "bouncing" => ScreenSaverStyle::BouncingLogo,
        "disabled" => ScreenSaverStyle::Disabled,
        _ => ScreenSaverStyle::Clock,
    }
}

fn governor_str(g: CpuGovernor) -> &'static str {
    match g {
        CpuGovernor::Auto => "auto",
        CpuGovernor::Performance => "performance",
        CpuGovernor::OnDemand => "ondemand",
        CpuGovernor::PowerSave => "powersave",
    }
}

fn parse_governor(s: &str) -> CpuGovernor {
    match s {
        "auto" => CpuGovernor::Auto,
        "performance" => CpuGovernor::Performance,
        "ondemand" => CpuGovernor::OnDemand,
        "powersave" => CpuGovernor::PowerSave,
        _ => CpuGovernor::Auto,
    }
}

fn action_to_state(action: PowerAction) -> PowerState {
    match action {
        PowerAction::Nothing => PowerState::Active,
        PowerAction::ScreenOff => PowerState::ScreenOff,
        PowerAction::Suspend => PowerState::Suspended,
        PowerAction::Hibernate => PowerState::Hibernated,
        PowerAction::Shutdown => PowerState::ShuttingDown,
        PowerAction::Lock => PowerState::Active, // Lock doesn't change power state.
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    // A test module's job is to fail loudly the instant the code under test is
    // wrong, so the defensive lints that forbid exactly that in production code
    // are off here — as `CLAUDE.md` prescribes.
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects
    )]
    // Two screen savers built from the same seed run the same arithmetic in the
    // same order, so their star fields agree bit for bit. Exactness is the
    // assertion here, not an accident of it — an approximate comparison would
    // pass even if the generator had gone non-deterministic.
    #![allow(clippy::float_cmp)]

    use super::*;

    #[test]
    fn test_default_config() {
        let cfg = PowerConfig::default();
        assert_eq!(cfg.profile, PowerProfile::Balanced);
        assert_eq!(cfg.dim_timeout_secs, 180);
        assert_eq!(cfg.screen_off_timeout_secs, 300);
        assert_eq!(cfg.suspend_timeout_secs, 900);
        assert_eq!(cfg.lid_close_action, PowerAction::Suspend);
        assert_eq!(cfg.power_button_action, PowerAction::Shutdown);
    }

    #[test]
    fn test_apply_performance_profile() {
        let mut cfg = PowerConfig::default();
        cfg.apply_profile(PowerProfile::Performance);
        assert_eq!(cfg.profile, PowerProfile::Performance);
        assert_eq!(cfg.dim_timeout_secs, 600);
        assert_eq!(cfg.screen_off_timeout_secs, 1800);
        assert_eq!(cfg.suspend_timeout_secs, 0);
        assert_eq!(cfg.cpu_governor, CpuGovernor::Performance);
    }

    #[test]
    fn test_apply_powersaver_profile() {
        let mut cfg = PowerConfig::default();
        cfg.apply_profile(PowerProfile::PowerSaver);
        assert_eq!(cfg.profile, PowerProfile::PowerSaver);
        assert_eq!(cfg.dim_timeout_secs, 60);
        assert_eq!(cfg.screen_off_timeout_secs, 120);
        assert_eq!(cfg.suspend_timeout_secs, 300);
        assert_eq!(cfg.cpu_governor, CpuGovernor::PowerSave);
    }

    #[test]
    fn test_custom_profile_preserves_values() {
        let mut cfg = PowerConfig::default();
        cfg.dim_timeout_secs = 42;
        cfg.apply_profile(PowerProfile::Custom);
        assert_eq!(cfg.dim_timeout_secs, 42); // Not overwritten.
    }

    #[test]
    fn test_config_round_trip() {
        let mut cfg = PowerConfig::default();
        cfg.profile = PowerProfile::Performance;
        cfg.dim_timeout_secs = 999;
        cfg.wake_on_lan = true;
        cfg.screensaver_style = ScreenSaverStyle::MatrixRain;
        cfg.lid_close_action = PowerAction::Hibernate;
        cfg.cpu_governor = CpuGovernor::OnDemand;
        cfg.show_battery_pct = false;

        let text = cfg.to_config_string();
        let parsed = PowerConfig::from_config_string(&text);

        assert_eq!(parsed.profile, PowerProfile::Performance);
        assert_eq!(parsed.dim_timeout_secs, 999);
        assert!(parsed.wake_on_lan);
        assert_eq!(parsed.screensaver_style, ScreenSaverStyle::MatrixRain);
        assert_eq!(parsed.lid_close_action, PowerAction::Hibernate);
        assert_eq!(parsed.cpu_governor, CpuGovernor::OnDemand);
        assert!(!parsed.show_battery_pct);
    }

    #[test]
    fn test_config_parse_ignores_unknown_keys() {
        let text = "profile=balanced\nunknown_key=value\ndim_timeout=123\n";
        let cfg = PowerConfig::from_config_string(text);
        assert_eq!(cfg.profile, PowerProfile::Balanced);
        assert_eq!(cfg.dim_timeout_secs, 123);
    }

    #[test]
    fn test_config_parse_ignores_comments() {
        let text = "# comment\nprofile=powersaver\n# another\n";
        let cfg = PowerConfig::from_config_string(text);
        assert_eq!(cfg.profile, PowerProfile::PowerSaver);
    }

    #[test]
    fn test_battery_no_battery() {
        let b = BatteryInfo::no_battery();
        assert!(!b.present);
        assert_eq!(b.state, BatteryState::NoBattery);
    }

    #[test]
    fn test_battery_with_charge() {
        let b = BatteryInfo::with_battery(75, BatteryState::Discharging);
        assert!(b.present);
        assert_eq!(b.charge_pct, 75);
        assert_eq!(b.state, BatteryState::Discharging);
    }

    #[test]
    fn test_battery_clamp_100() {
        let b = BatteryInfo::with_battery(200, BatteryState::Charging);
        assert_eq!(b.charge_pct, 100);
    }

    #[test]
    fn test_battery_time_remaining_str() {
        let mut b = BatteryInfo::with_battery(50, BatteryState::Discharging);
        b.time_remaining_secs = Some(3661);
        assert_eq!(b.time_remaining_str(), Some("1h 1m".to_string()));
    }

    #[test]
    fn test_battery_time_remaining_minutes_only() {
        let mut b = BatteryInfo::with_battery(50, BatteryState::Discharging);
        b.time_remaining_secs = Some(300);
        assert_eq!(b.time_remaining_str(), Some("5m".to_string()));
    }

    #[test]
    fn test_battery_time_remaining_none() {
        let b = BatteryInfo::with_battery(50, BatteryState::Charging);
        assert_eq!(b.time_remaining_str(), None);
    }

    #[test]
    fn test_battery_warning_detection() {
        let cfg = PowerConfig::default(); // low = 20%
        let b = BatteryInfo::with_battery(15, BatteryState::Discharging);
        assert!(b.is_warning(&cfg));
        let b2 = BatteryInfo::with_battery(25, BatteryState::Discharging);
        assert!(!b2.is_warning(&cfg));
        // Charging doesn't warn even if low.
        let b3 = BatteryInfo::with_battery(10, BatteryState::Charging);
        assert!(!b3.is_warning(&cfg));
    }

    #[test]
    fn test_battery_critical_detection() {
        let cfg = PowerConfig::default(); // critical = 5%
        let b = BatteryInfo::with_battery(3, BatteryState::Discharging);
        assert!(b.is_critical(&cfg));
        let b2 = BatteryInfo::with_battery(10, BatteryState::Discharging);
        assert!(!b2.is_critical(&cfg));
    }

    #[test]
    fn test_power_manager_initial_state() {
        let pm = PowerManager::new();
        assert_eq!(pm.state(), PowerState::Active);
        assert_eq!(pm.idle_secs(), 0);
    }

    #[test]
    fn test_idle_progression() {
        let mut pm = PowerManager::new();
        for _ in 0..10 {
            pm.tick(100);
        }
        assert_eq!(pm.idle_secs(), 10);
    }

    #[test]
    fn test_user_activity_resets_idle() {
        let mut pm = PowerManager::new();
        for _ in 0..50 {
            pm.tick(100);
        }
        assert_eq!(pm.idle_secs(), 50);
        pm.on_user_activity(150);
        assert_eq!(pm.idle_secs(), 0);
    }

    #[test]
    fn test_dim_timeout() {
        let mut pm = PowerManager::new();
        // Default dim = 180s.
        for i in 0..180 {
            pm.tick(i as u64);
        }
        assert_eq!(pm.state(), PowerState::Dimmed);
    }

    #[test]
    fn test_screen_off_after_dim() {
        let mut pm = PowerManager::new();
        // Default: dim at 180, screen off at 300.
        for i in 0..300 {
            pm.tick(i as u64);
        }
        assert_eq!(pm.state(), PowerState::ScreenOff);
    }

    #[test]
    fn test_wake_from_dim_on_activity() {
        let mut pm = PowerManager::new();
        for i in 0..180 {
            pm.tick(i as u64);
        }
        assert_eq!(pm.state(), PowerState::Dimmed);
        pm.on_user_activity(200);
        assert_eq!(pm.state(), PowerState::Active);
    }

    #[test]
    fn test_wake_from_screen_off_on_activity() {
        let mut pm = PowerManager::new();
        for i in 0..300 {
            pm.tick(i as u64);
        }
        assert_eq!(pm.state(), PowerState::ScreenOff);
        pm.on_user_activity(400);
        assert_eq!(pm.state(), PowerState::Active);
    }

    #[test]
    fn test_lid_close_suspends() {
        let mut pm = PowerManager::new();
        let action = pm.on_lid_close(10);
        assert_eq!(action, PowerAction::Suspend);
        assert_eq!(pm.state(), PowerState::Suspended);
    }

    #[test]
    fn test_power_button_shuts_down() {
        let mut pm = PowerManager::new();
        let action = pm.on_power_button(10);
        assert_eq!(action, PowerAction::Shutdown);
        assert_eq!(pm.state(), PowerState::ShuttingDown);
    }

    #[test]
    fn test_sleep_button_suspends() {
        let mut pm = PowerManager::new();
        let action = pm.on_sleep_button(10);
        assert_eq!(action, PowerAction::Suspend);
        assert_eq!(pm.state(), PowerState::Suspended);
    }

    #[test]
    fn test_lid_open_wakes_from_screen_off() {
        let mut pm = PowerManager::new();
        for i in 0..300 {
            pm.tick(i as u64);
        }
        assert_eq!(pm.state(), PowerState::ScreenOff);
        pm.on_lid_open(400);
        assert_eq!(pm.state(), PowerState::Active);
    }

    #[test]
    fn test_critical_battery_action() {
        let mut pm = PowerManager::new();
        pm.update_battery(BatteryInfo::with_battery(3, BatteryState::Discharging));
        let action = pm.tick(100);
        assert_eq!(action, Some(PowerAction::Hibernate));
        assert_eq!(pm.state(), PowerState::Hibernated);
    }

    #[test]
    fn test_critical_battery_only_once() {
        let mut pm = PowerManager::new();
        pm.update_battery(BatteryInfo::with_battery(3, BatteryState::Discharging));
        let first = pm.tick(100);
        assert_eq!(first, Some(PowerAction::Hibernate));
        // Reset state to active to test the flag.
        pm.state = PowerState::Active;
        let second = pm.tick(101);
        assert_eq!(second, None); // Not triggered again.
    }

    #[test]
    fn test_charging_resets_critical_flag() {
        let mut pm = PowerManager::new();
        pm.update_battery(BatteryInfo::with_battery(3, BatteryState::Discharging));
        let _ = pm.tick(100);
        // Plug in charger.
        pm.update_battery(BatteryInfo::with_battery(4, BatteryState::Charging));
        pm.state = PowerState::Active;
        // Unplug at critical again.
        pm.update_battery(BatteryInfo::with_battery(3, BatteryState::Discharging));
        let action = pm.tick(200);
        assert_eq!(action, Some(PowerAction::Hibernate));
    }

    #[test]
    fn test_inhibitor_add_remove() {
        let mut pm = PowerManager::new();
        let id = pm.add_inhibitor("video_player", "Playing video", InhibitTarget::ScreenDim);
        assert_eq!(pm.inhibitors().len(), 1);
        assert!(pm.is_inhibited(InhibitTarget::ScreenDim));
        assert!(!pm.is_inhibited(InhibitTarget::Suspend));

        assert!(pm.remove_inhibitor(id));
        assert_eq!(pm.inhibitors().len(), 0);
        assert!(!pm.is_inhibited(InhibitTarget::ScreenDim));
    }

    #[test]
    fn test_inhibitor_all_blocks_everything() {
        let mut pm = PowerManager::new();
        let _id = pm.add_inhibitor("game", "Fullscreen game", InhibitTarget::All);
        assert!(pm.is_inhibited(InhibitTarget::ScreenDim));
        assert!(pm.is_inhibited(InhibitTarget::ScreenOff));
        assert!(pm.is_inhibited(InhibitTarget::Suspend));
    }

    #[test]
    fn test_inhibitor_prevents_dim() {
        let mut pm = PowerManager::new();
        let _id = pm.add_inhibitor("media", "Playing", InhibitTarget::ScreenDim);
        for i in 0..250 {
            pm.tick(i as u64);
        }
        // Should still be active despite being past dim timeout.
        assert_eq!(pm.state(), PowerState::Active);
    }

    #[test]
    fn test_remove_nonexistent_inhibitor() {
        let mut pm = PowerManager::new();
        assert!(!pm.remove_inhibitor(999));
    }

    #[test]
    fn test_transition_log() {
        let mut pm = PowerManager::new();
        for i in 0..180 {
            pm.tick(i as u64);
        }
        let log = pm.transition_log();
        assert!(!log.is_empty());
        assert_eq!(log[0].from, PowerState::Active);
        assert_eq!(log[0].to, PowerState::Dimmed);
        assert_eq!(log[0].reason, TransitionReason::IdleTimeout);
    }

    #[test]
    fn test_transition_log_cap() {
        let mut pm = PowerManager::new();
        pm.max_log_entries = 3;
        for i in 0..5 {
            pm.transition_to(
                if i % 2 == 0 {
                    PowerState::Dimmed
                } else {
                    PowerState::Active
                },
                i as u64,
                TransitionReason::UserActivity,
            );
        }
        assert!(pm.transition_log().len() <= 3);
    }

    #[test]
    fn test_request_action_shutdown() {
        let mut pm = PowerManager::new();
        pm.request_action(PowerAction::Shutdown, 10);
        assert_eq!(pm.state(), PowerState::ShuttingDown);
    }

    #[test]
    fn test_request_action_nothing() {
        let mut pm = PowerManager::new();
        pm.request_action(PowerAction::Nothing, 10);
        assert_eq!(pm.state(), PowerState::Active);
    }

    #[test]
    fn test_screensaver_creation() {
        let ss = ScreenSaver::new(ScreenSaverStyle::Blank, 1920, 1080);
        assert_eq!(ss.width, 1920);
        assert_eq!(ss.height, 1080);
    }

    #[test]
    fn test_screensaver_blank_renders() {
        let mut ss = ScreenSaver::new(ScreenSaverStyle::Blank, 800, 600);
        let cmds = ss.render_frame();
        assert_eq!(cmds.len(), 1);
    }

    #[test]
    fn test_screensaver_clock_renders() {
        let mut ss = ScreenSaver::new(ScreenSaverStyle::Clock, 800, 600);
        let cmds = ss.render_frame();
        assert!(cmds.len() >= 2); // bg + text
    }

    #[test]
    fn test_screensaver_starfield_init() {
        let ss = ScreenSaver::new(ScreenSaverStyle::Starfield, 1920, 1080);
        assert_eq!(ss.stars.len(), 200);
    }

    #[test]
    fn test_screensaver_starfield_renders() {
        let mut ss = ScreenSaver::new(ScreenSaverStyle::Starfield, 800, 600);
        let cmds = ss.render_frame();
        assert!(cmds.len() > 1);
    }

    #[test]
    fn test_screensaver_matrix_init() {
        let ss = ScreenSaver::new(ScreenSaverStyle::MatrixRain, 1920, 1080);
        assert!(!ss.columns.is_empty());
    }

    #[test]
    fn test_screensaver_bouncing_logo_renders() {
        let mut ss = ScreenSaver::new(ScreenSaverStyle::BouncingLogo, 800, 600);
        let cmds = ss.render_frame();
        assert!(cmds.len() >= 3); // bg + rect + text
    }

    #[test]
    fn test_screensaver_disabled() {
        let mut ss = ScreenSaver::new(ScreenSaverStyle::Disabled, 800, 600);
        let cmds = ss.render_frame();
        assert!(cmds.is_empty());
    }

    /// Every column must start inside the screen and be drawn from printable
    /// characters, whatever the screen size — including sizes narrower than a
    /// single column, where the loop must simply produce none.
    #[test]
    fn matrix_columns_start_on_screen_and_use_printable_characters() {
        for width in [0, 1, 13, 14, 15, 800, 3840] {
            let ss = ScreenSaver::new(ScreenSaverStyle::MatrixRain, width, 600);
            assert_eq!(
                ss.columns.len() as u32,
                width / COLUMN_WIDTH_PX,
                "width={width}"
            );
            for col in &ss.columns {
                assert!(
                    col.x < width.max(1),
                    "column at {} off a {width}px screen",
                    col.x
                );
                assert!(
                    (MIN_COLUMN_GLYPHS..=MAX_COLUMN_GLYPHS).contains(&i64::from(col.length)),
                    "column of {} glyphs",
                    col.length
                );
                assert_eq!(col.chars.len() as u32, col.length);
                for &c in &col.chars {
                    assert!((FIRST_GLYPH..=LAST_GLYPH).contains(&c), "glyph {c:#x}");
                }
                assert!(col.y <= 0.0, "a column must start above the screen");
                assert!(col.speed >= 1.0, "a stalled column never falls");
            }
        }
    }

    /// The rain used to "randomise" a recycled column by adding 7 to every
    /// character code. That is not random at all: every column advanced by the
    /// same step, so they all showed the same ninety-four-frame cycle, in
    /// lockstep. A recycled column must now differ from its predecessor.
    #[test]
    fn a_recycled_matrix_column_gets_genuinely_new_characters() {
        let mut ss = ScreenSaver::new(ScreenSaverStyle::MatrixRain, 800, 600);
        assert!(ss.columns.len() > 1);
        let before: Vec<Vec<u8>> = ss.columns.iter().map(|c| c.chars.clone()).collect();

        // Drive every column past the bottom so all of them recycle.
        for col in &mut ss.columns {
            col.y = 10_000.0;
        }
        let _ = ss.render_frame();

        let after: Vec<Vec<u8>> = ss.columns.iter().map(|c| c.chars.clone()).collect();
        assert_ne!(before, after, "recycling must change the characters");

        // The old code shifted every column by the same amount, so the
        // difference between old and new was identical across columns. It must
        // not be.
        let shifts: Vec<Vec<i32>> = before
            .iter()
            .zip(&after)
            .map(|(old, new)| {
                old.iter()
                    .zip(new)
                    .map(|(o, n)| i32::from(*n) - i32::from(*o))
                    .collect()
            })
            .collect();
        assert!(
            shifts.windows(2).any(|pair| pair[0] != pair[1]),
            "every column changed by the same amount — that is a cycle, not randomness"
        );

        for col in &ss.columns {
            for &c in &col.chars {
                assert!((FIRST_GLYPH..=LAST_GLYPH).contains(&c), "glyph {c:#x}");
            }
            assert!(col.y < 0.0, "a recycled column restarts above the screen");
        }
    }

    /// A star that reaches the viewer is recycled to the far plane with a new
    /// position. Both coordinates must land in the visible span — the borrow
    /// dodge this replaced updated them through an inlined generator, so the
    /// two were easy to get out of step with `init_starfield`.
    #[test]
    fn a_recycled_star_returns_to_the_far_plane_inside_the_field() {
        let mut ss = ScreenSaver::new(ScreenSaverStyle::Starfield, 800, 600);
        for star in &mut ss.stars {
            star.z = 0.005;
        }
        let _ = ss.render_frame();

        for star in &ss.stars {
            assert!((-1.0..=1.0).contains(&star.x), "x={}", star.x);
            assert!((-1.0..=1.0).contains(&star.y), "y={}", star.y);
            assert!(star.z > 0.0 && star.z <= 1.0, "z={}", star.z);
        }
        // And they must not all have been given the same position.
        let first = (ss.stars.first().map(|s| s.x), ss.stars.first().map(|s| s.y));
        assert!(
            ss.stars.iter().any(|s| (Some(s.x), Some(s.y)) != first),
            "every star was recycled to the same place"
        );
    }

    /// Two screen savers built the same way must animate the same way: the
    /// generator is seeded, deliberately, so a screen saver is reproducible.
    #[test]
    fn two_screensavers_of_the_same_style_and_size_agree() {
        let mut first = ScreenSaver::new(ScreenSaverStyle::Starfield, 800, 600);
        let mut second = ScreenSaver::new(ScreenSaverStyle::Starfield, 800, 600);
        for _ in 0..20 {
            assert_eq!(first.render_frame().len(), second.render_frame().len());
        }
        assert!(
            first
                .stars
                .iter()
                .zip(&second.stars)
                .all(|(a, b)| a.x == b.x && a.y == b.y && a.z == b.z)
        );
    }

    /// A screen with no room for a column, and one with no stars to draw, must
    /// still render a frame rather than panic on an empty buffer.
    #[test]
    fn a_screen_too_small_for_anything_still_renders() {
        for style in [ScreenSaverStyle::MatrixRain, ScreenSaverStyle::Starfield] {
            let mut ss = ScreenSaver::new(style, 0, 0);
            let cmds = ss.render_frame();
            assert!(!cmds.is_empty(), "{style:?} drew nothing at all");
        }
    }

    #[test]
    fn test_battery_icon_no_battery() {
        let b = BatteryInfo::no_battery();
        let cfg = PowerConfig::default();
        let cmds = render_battery_icon(&b, &cfg, &Palette::for_mode(false), 10.0, 10.0);
        assert!(!cmds.is_empty()); // Shows "AC" text.
    }

    #[test]
    fn test_battery_icon_with_battery() {
        let b = BatteryInfo::with_battery(75, BatteryState::Discharging);
        let cfg = PowerConfig::default();
        let cmds = render_battery_icon(&b, &cfg, &Palette::for_mode(false), 10.0, 10.0);
        assert!(cmds.len() >= 3); // outline + tip + fill
    }

    #[test]
    fn test_battery_icon_charging() {
        let b = BatteryInfo::with_battery(50, BatteryState::Charging);
        let cfg = PowerConfig::default();
        let cmds = render_battery_icon(&b, &cfg, &Palette::for_mode(false), 10.0, 10.0);
        assert!(cmds.len() >= 4); // outline + tip + fill + charging symbol
    }

    #[test]
    fn test_power_profile_badge() {
        let cmds = render_power_profile_badge(
            PowerProfile::Performance,
            &Palette::for_mode(false),
            0.0,
            0.0,
        );
        assert_eq!(cmds.len(), 2); // bg + text
    }

    #[test]
    fn test_all_profiles_render() {
        for profile in &[
            PowerProfile::Balanced,
            PowerProfile::Performance,
            PowerProfile::PowerSaver,
            PowerProfile::Custom,
        ] {
            let cmds = render_power_profile_badge(*profile, &Palette::for_mode(false), 0.0, 0.0);
            assert_eq!(cmds.len(), 2);
        }
    }

    #[test]
    fn test_no_dim_when_timeout_zero() {
        let mut pm = PowerManager::new();
        pm.config_mut().dim_timeout_secs = 0;
        for i in 0..500 {
            pm.tick(i as u64);
        }
        // Should not dim because timeout is disabled.
        assert_ne!(pm.state(), PowerState::Dimmed);
    }

    #[test]
    fn test_screensaver_timeout() {
        let mut pm = PowerManager::new();
        pm.config_mut().screensaver_timeout_secs = 60;
        pm.config_mut().dim_timeout_secs = 0; // Disable dim to test screensaver only.
        for i in 0..60 {
            pm.tick(i as u64);
        }
        assert_eq!(pm.state(), PowerState::ScreenSaver);
    }

    #[test]
    fn test_user_activity_wakes_screensaver() {
        let mut pm = PowerManager::new();
        pm.config_mut().screensaver_timeout_secs = 10;
        pm.config_mut().dim_timeout_secs = 0;
        for i in 0..10 {
            pm.tick(i as u64);
        }
        assert_eq!(pm.state(), PowerState::ScreenSaver);
        pm.on_user_activity(20);
        assert_eq!(pm.state(), PowerState::Active);
    }

    #[test]
    fn test_config_clamp_battery_pct() {
        let text = "low_battery_pct=255\ncritical_battery_pct=200\ndim_brightness=200\n";
        let cfg = PowerConfig::from_config_string(text);
        assert_eq!(cfg.low_battery_pct, 100);
        assert_eq!(cfg.critical_battery_pct, 100);
        assert_eq!(cfg.dim_brightness_pct, 100);
    }

    // ========================================================================
    // Colour — the palette conversion (module 27 of 49)
    // ========================================================================
    //
    // This file used to hold nine Mocha constants of its own. What follows
    // proves they are gone and that what replaced them is in the role it
    // claims. Six lessons from earlier modules are applied here up front, and
    // this module adds a seventh:
    //
    // 1. The sweep is only as wide as the render it is given: enumerate the
    //    renderer's `if`s, not its colours.
    // 2. A representative sample is not a per-site check: n *source* sites give
    //    n assertions, not n rendered instances and not n kinds.
    // 3. A site that only *selects* a colour is a source site too, even though
    //    it draws nothing itself.
    // 4. Render under an accent that is in neither palette. Testing a
    //    conversion in the palette it was converted *from* hides exactly the
    //    failures that conversion causes.
    // 5. An expectation written in terms of the code under test cannot fail.
    //    State the role literal, never `X.color(p)`.
    // 6. You cannot recognise the legitimate instances of a property by testing
    //    for that property — the bug has it too. Count them instead.
    //
    // 7. NEW HERE: *pinning a surface to one mode disarms the membership sweep
    //    for that surface.* The screen saver draws with the dark palette in both
    //    modes (judgement 1 at the top of the file), so every value it draws is
    //    a member of its palette in both modes — and a `COL_LAVENDER` left
    //    behind would be bit-for-bit identical to `screen_palette().lavender`.
    //    The sweep below is still worth running, because it catches an inline
    //    literal that is not a role at all, but it can no longer catch the
    //    defect it was written for. For a surface that does not follow the mode
    //    the per-site tables are not a supplement to the sweep: they are the
    //    only thing left. Anyone who trims them is removing the whole proof.

    use crate::palette_check::assert_drawn_from;
    use appearance::readable_on;

    /// An accent that is in neither palette, so a site reaching for the accent
    /// where it must not is visible rather than merely plausible. The stock
    /// accent *is* `blue`, and three sites in this file legitimately draw
    /// `p.blue`; under the stock accent those cases are indistinguishable.
    const OFF_PALETTE: Color = Color::from_hex(0x00FF_8C1A);

    fn rgb(c: Color) -> (u8, u8, u8) {
        (c.r, c.g, c.b)
    }

    /// Both modes, each with an accent no palette contains.
    fn table_palettes() -> Vec<(String, Palette)> {
        [false, true]
            .into_iter()
            .map(|light| {
                let mut p = Palette::for_mode(light);
                p.accent = OFF_PALETTE;
                (format!("light={light}"), p)
            })
            .collect()
    }

    /// Every state the battery icon can be drawn in, one per branch.
    fn battery_states() -> Vec<(String, BatteryInfo, PowerConfig)> {
        let cfg = PowerConfig::default();
        let mut hidden_pct = PowerConfig::default();
        hidden_pct.show_battery_pct = false;
        vec![
            (
                "no battery".to_string(),
                BatteryInfo::no_battery(),
                cfg.clone(),
            ),
            (
                "critical".to_string(),
                BatteryInfo::with_battery(3, BatteryState::Discharging),
                cfg.clone(),
            ),
            (
                "low".to_string(),
                BatteryInfo::with_battery(15, BatteryState::Discharging),
                cfg.clone(),
            ),
            (
                "nearly full".to_string(),
                BatteryInfo::with_battery(90, BatteryState::Discharging),
                cfg.clone(),
            ),
            (
                "normal".to_string(),
                BatteryInfo::with_battery(50, BatteryState::Discharging),
                cfg.clone(),
            ),
            (
                "charging".to_string(),
                BatteryInfo::with_battery(50, BatteryState::Charging),
                cfg.clone(),
            ),
            (
                "flat".to_string(),
                BatteryInfo::with_battery(0, BatteryState::Discharging),
                cfg,
            ),
            (
                "percentage hidden".to_string(),
                BatteryInfo::with_battery(50, BatteryState::Discharging),
                hidden_pct,
            ),
        ]
    }

    fn every_profile() -> [PowerProfile; 4] {
        [
            PowerProfile::Balanced,
            PowerProfile::Performance,
            PowerProfile::PowerSaver,
            PowerProfile::Custom,
        ]
    }

    fn every_style() -> [ScreenSaverStyle; 6] {
        [
            ScreenSaverStyle::Blank,
            ScreenSaverStyle::Clock,
            ScreenSaverStyle::MatrixRain,
            ScreenSaverStyle::Starfield,
            ScreenSaverStyle::BouncingLogo,
            ScreenSaverStyle::Disabled,
        ]
    }

    fn every_color(cmds: &[RenderCommand]) -> Vec<Color> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::FillRect { color, .. }
                | RenderCommand::StrokeRect { color, .. }
                | RenderCommand::Text { color, .. } => Some(*color),
                _ => None,
            })
            .collect()
    }

    /// The colour of the one text with this exact string *and* size.
    ///
    /// Both discriminators are needed: this module draws "AC" once but a module
    /// that drew one word in two roles would need the size to tell them apart,
    /// and a lookup that silently took the first match would test whichever site
    /// happened to be drawn first.
    fn text_exact(cmds: &[RenderCommand], want: &str, size: f32) -> Color {
        let hits: Vec<Color> = cmds
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text {
                    text,
                    color,
                    font_size,
                    ..
                } if text == want && *font_size == size => Some(*color),
                _ => None,
            })
            .collect();
        assert_eq!(
            hits.len(),
            1,
            "{want:?} at {size}pt: {} matches",
            hits.len()
        );
        hits[0]
    }

    /// The colour of the one filled rectangle of this height.
    fn fill_of_height(cmds: &[RenderCommand], h: f32) -> Color {
        let hits: Vec<Color> = cmds
            .iter()
            .filter_map(|c| match c {
                RenderCommand::FillRect { height, color, .. } if *height == h => Some(*color),
                _ => None,
            })
            .collect();
        assert_eq!(hits.len(), 1, "fill {h} high: {} matches", hits.len());
        hits[0]
    }

    /// The colour of the one stroked rectangle of this size.
    fn stroke_of_size(cmds: &[RenderCommand], w: f32, h: f32) -> Color {
        let hits: Vec<Color> = cmds
            .iter()
            .filter_map(|c| match c {
                RenderCommand::StrokeRect {
                    width,
                    height,
                    color,
                    ..
                } if *width == w && *height == h => Some(*color),
                _ => None,
            })
            .collect();
        assert_eq!(hits.len(), 1, "stroke {w}x{h}: {} matches", hits.len());
        hits[0]
    }

    /// Lesson 1: the sweep must see every branch, so it walks every fixture of
    /// both renderers that take a palette, in both modes.
    #[test]
    fn every_colour_the_taskbar_and_settings_draw_comes_from_their_palette() {
        for (mode, p) in table_palettes() {
            for (what, battery, cfg) in battery_states() {
                let cmds = render_battery_icon(&battery, &cfg, &p, 10.0, 10.0);
                assert_drawn_from(&p, &cmds, &[], &format!("power battery {what} ({mode})"));
            }
            for profile in every_profile() {
                let cmds = render_power_profile_badge(profile, &p, 0.0, 0.0);
                assert_drawn_from(&p, &cmds, &[], &format!("power badge {profile:?} ({mode})"));
            }
        }
    }

    /// The saver's sweep, and lesson 7 in practice.
    ///
    /// It runs against the pinned dark palette because that is the palette the
    /// saver draws with. It cannot catch a leftover Mocha constant — see lesson
    /// 7 above — but it does catch an inline literal that is no role at all, and
    /// it pins the two computed ramps to the two shapes they are allowed to
    /// have: a grey (a star's depth) and a green (a glyph's age).
    #[test]
    fn every_colour_the_screen_saver_draws_is_a_dark_role_or_one_of_its_two_ramps() {
        let mut derived: Vec<Color> = (0..=u8::MAX).map(|v| Color::rgba(v, v, v, 255)).collect();
        derived.extend((0..=u8::MAX).map(|v| Color::rgba(0, v, 0, 255)));
        let sp = screen_palette();
        assert!(!sp.light, "the saver must draw with the dark palette");
        for style in every_style() {
            let mut ss = ScreenSaver::new(style, 800, 600);
            // Several frames: the matrix rain only recycles a column after it
            // has fallen off the bottom, and the recycled glyphs are the ones
            // drawn from the generator rather than the constructor.
            for frame in 0..40 {
                let cmds = ss.render_frame();
                assert_drawn_from(
                    &sp,
                    &cmds,
                    &derived,
                    &format!("saver {style:?} frame {frame}"),
                );
            }
        }
    }

    /// Lesson 1 again, from the other end: prove the fixtures reach the
    /// branches, so a sweep that passes cannot be passing vacuously.
    #[test]
    fn the_fixtures_take_every_branch_this_module_has() {
        let p = Palette::for_mode(false);

        // The gauge's four steps, the branch that draws no gauge at all, the
        // charging bolt, and the percentage the user can switch off.
        let mut gauges = Vec::new();
        let (mut with_bolt, mut without_bolt) = (0, 0);
        let (mut with_pct, mut without_pct) = (0, 0);
        let mut no_gauge = 0;
        let mut ac_only = 0;
        for (what, battery, cfg) in battery_states() {
            let cmds = render_battery_icon(&battery, &cfg, &p, 10.0, 10.0);
            let texts: Vec<String> = cmds
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::Text { text, .. } => Some(text.clone()),
                    _ => None,
                })
                .collect();
            if texts.iter().any(|t| t == "AC") {
                ac_only += 1;
                assert_eq!(cmds.len(), 1, "{what}: an absent battery drew a gauge");
                continue;
            }
            if cmds
                .iter()
                .any(|c| matches!(c, RenderCommand::FillRect { height, .. } if *height == 8.0))
            {
                gauges.push(rgb(fill_of_height(&cmds, 8.0)));
            } else {
                no_gauge += 1;
            }
            if texts.iter().any(|t| t == "\u{26A1}") {
                with_bolt += 1;
            } else {
                without_bolt += 1;
            }
            if texts.iter().any(|t| t.ends_with('%')) {
                with_pct += 1;
            } else {
                without_pct += 1;
            }
        }
        gauges.sort_unstable();
        gauges.dedup();
        assert_eq!(
            gauges.len(),
            4,
            "the fixtures reach {} of the gauge's four steps",
            gauges.len()
        );
        assert_eq!(ac_only, 1, "no fixture has the battery missing");
        assert_eq!(no_gauge, 1, "no fixture has a battery too flat to draw");
        assert!(with_bolt >= 1 && without_bolt >= 1, "charging is untested");
        assert!(
            with_pct >= 1 && without_pct >= 1,
            "the percentage is untested"
        );

        // Every profile, and every saver style.
        assert_eq!(every_profile().len(), 4);
        for style in every_style() {
            let mut ss = ScreenSaver::new(style, 800, 600);
            let cmds = ss.render_frame();
            assert_eq!(
                cmds.is_empty(),
                style == ScreenSaverStyle::Disabled,
                "{style:?} drew the wrong amount of nothing"
            );
        }
    }

    /// Lesson 2: one assertion per *source* site, not per rendered instance.
    #[test]
    fn every_text_this_module_draws_is_in_the_role_it_claims() {
        for (mode, p) in table_palettes() {
            let cfg = PowerConfig::default();

            // The AC hint, when there is no battery to draw.
            let ac = render_battery_icon(&BatteryInfo::no_battery(), &cfg, &p, 10.0, 10.0);
            assert_eq!(rgb(text_exact(&ac, "AC", 11.0)), rgb(p.subtext0), "{mode}");

            // The charging bolt is a state marker, so it is a named hue and not
            // the accent — the bolt means "charging" on every machine.
            let charging = render_battery_icon(
                &BatteryInfo::with_battery(50, BatteryState::Charging),
                &cfg,
                &p,
                10.0,
                10.0,
            );
            assert_eq!(
                rgb(text_exact(&charging, "\u{26A1}", 10.0)),
                rgb(p.yellow),
                "{mode}"
            );

            // The percentage readout is secondary text beside the gauge.
            assert_eq!(
                rgb(text_exact(&charging, "50%", 11.0)),
                rgb(p.subtext0),
                "{mode}"
            );

            // The badge's label takes the badge's own hue, so it is asserted
            // against the role literal for each profile below, not here.
            let badge = render_power_profile_badge(PowerProfile::Balanced, &p, 0.0, 0.0);
            assert_eq!(
                rgb(text_exact(&badge, "Balanced", 12.0)),
                rgb(p.blue),
                "{mode}"
            );
        }

        // The saver's two labels, against its own pinned palette.
        let sp = screen_palette();
        let mut clock = ScreenSaver::new(ScreenSaverStyle::Clock, 800, 600);
        let cmds = clock.render_frame();
        assert_eq!(rgb(text_exact(&cmds, "12:00", 72.0)), rgb(sp.lavender));
        let mut logo = ScreenSaver::new(ScreenSaverStyle::BouncingLogo, 800, 600);
        let cmds = logo.render_frame();
        assert_eq!(
            rgb(text_exact(&cmds, "Slate OS", 28.0)),
            rgb(readable_on(sp.blue)),
        );
    }

    /// Lesson 2 for the rectangles.
    #[test]
    fn every_rectangle_this_module_draws_is_in_the_role_it_claims() {
        for (mode, p) in table_palettes() {
            let cfg = PowerConfig::default();
            let cmds = render_battery_icon(
                &BatteryInfo::with_battery(50, BatteryState::Discharging),
                &cfg,
                &p,
                10.0,
                10.0,
            );
            // Outline and tip are the icon's structure: foreground ink.
            assert_eq!(
                rgb(stroke_of_size(&cmds, 22.0, 12.0)),
                rgb(p.text),
                "{mode}"
            );
            assert_eq!(rgb(fill_of_height(&cmds, 6.0)), rgb(p.text), "{mode}");

            // The badge's wash is the badge's own hue at low alpha — derived
            // from the choice, so it must track it rather than name a role.
            for profile in every_profile() {
                let badge = render_power_profile_badge(profile, &p, 0.0, 0.0);
                let colors = every_color(&badge);
                assert_eq!(colors.len(), 2, "{profile:?} ({mode})");
                assert_eq!(rgb(colors[0]), rgb(colors[1]), "{profile:?} ({mode})");
                assert_eq!(colors[0].a, 40, "{profile:?} ({mode}): the wash is opaque");
                assert_eq!(
                    colors[1].a, 255,
                    "{profile:?} ({mode}): the label is washed"
                );
            }
        }

        // The saver's logo plate.
        let sp = screen_palette();
        let mut logo = ScreenSaver::new(ScreenSaverStyle::BouncingLogo, 800, 600);
        let cmds = logo.render_frame();
        assert_eq!(rgb(fill_of_height(&cmds, 60.0)), rgb(sp.blue));
    }

    /// Lesson 3: the two `match`/`if` chains that pick a colour and hand it to
    /// a shared draw call. Neither draws anything itself, so neither shows up
    /// in the tables above — the gauge is one rectangle and the badge is one
    /// label, whatever they are coloured.
    #[test]
    fn every_choice_this_module_makes_hands_over_the_role_it_claims() {
        for (mode, p) in table_palettes() {
            let cfg = PowerConfig::default();
            // Lesson 5: the expected values are role literals. Written as
            // "whatever `is_critical` picks" the arms could be swapped and both
            // sides of the comparison would move together.
            let gauge = |pct: u8, state: BatteryState| {
                let cmds = render_battery_icon(
                    &BatteryInfo::with_battery(pct, state),
                    &cfg,
                    &p,
                    10.0,
                    10.0,
                );
                rgb(fill_of_height(&cmds, 8.0))
            };
            assert_eq!(gauge(3, BatteryState::Discharging), rgb(p.red), "{mode}");
            assert_eq!(
                gauge(15, BatteryState::Discharging),
                rgb(p.yellow),
                "{mode}"
            );
            assert_eq!(gauge(90, BatteryState::Discharging), rgb(p.green), "{mode}");
            assert_eq!(gauge(50, BatteryState::Discharging), rgb(p.blue), "{mode}");

            let badge = |profile: PowerProfile| {
                let cmds = render_power_profile_badge(profile, &p, 0.0, 0.0);
                rgb(every_color(&cmds)[1])
            };
            assert_eq!(badge(PowerProfile::Balanced), rgb(p.blue), "{mode}");
            assert_eq!(badge(PowerProfile::Performance), rgb(p.peach), "{mode}");
            assert_eq!(badge(PowerProfile::PowerSaver), rgb(p.green), "{mode}");
            assert_eq!(badge(PowerProfile::Custom), rgb(p.lavender), "{mode}");
        }
    }

    /// Lesson 6, in its simplest form: nothing here is a position or an
    /// invitation, so the allowed count is zero and "count them" collapses to
    /// "none of them". A gauge or a badge that followed the accent would mean
    /// something different on every machine.
    #[test]
    fn nothing_this_module_draws_moves_when_the_accent_does() {
        const A: Color = Color::from_hex(0x00FF_8C1A);
        const B: Color = Color::from_hex(0x0012_9E7D);

        for light in [false, true] {
            let (mut pa, mut pb) = (Palette::for_mode(light), Palette::for_mode(light));
            pa.accent = A;
            pb.accent = B;
            for (what, battery, cfg) in battery_states() {
                let ca = render_battery_icon(&battery, &cfg, &pa, 10.0, 10.0);
                let cb = render_battery_icon(&battery, &cfg, &pb, 10.0, 10.0);
                assert_eq!(
                    ca.len(),
                    cb.len(),
                    "{what}: the accent changed how much is drawn"
                );
                for (i, (x, y)) in every_color(&ca).iter().zip(every_color(&cb)).enumerate() {
                    assert_eq!(
                        rgb(*x),
                        rgb(y),
                        "battery {what} (light={light}): command {i} follows the accent, \
                         but a charge gauge is a measurement and must read the same \
                         on every machine"
                    );
                }
            }
            for profile in every_profile() {
                let ca = render_power_profile_badge(profile, &pa, 0.0, 0.0);
                let cb = render_power_profile_badge(profile, &pb, 0.0, 0.0);
                for (i, (x, y)) in every_color(&ca).iter().zip(every_color(&cb)).enumerate() {
                    assert_eq!(
                        rgb(*x),
                        rgb(y),
                        "badge {profile:?} (light={light}): command {i} follows the \
                         accent, but the four profiles are a category, not a position"
                    );
                }
            }
        }
    }

    /// A scale whose steps are not told apart is not a scale. Two gauge steps
    /// that collided would leave the user unable to see the difference between
    /// "nearly flat" and "fine", in a palette where both happen to be legal.
    #[test]
    fn every_step_of_the_gauge_and_every_profile_stays_apart_from_the_others() {
        for (mode, p) in table_palettes() {
            let steps = [p.red, p.yellow, p.green, p.blue];
            for (i, a) in steps.iter().enumerate() {
                for b in steps.iter().skip(i + 1) {
                    assert_ne!(rgb(*a), rgb(*b), "two gauge steps collide in {mode}");
                }
            }
            let hues = [p.blue, p.peach, p.green, p.lavender];
            for (i, a) in hues.iter().enumerate() {
                for b in hues.iter().skip(i + 1) {
                    assert_ne!(rgb(*a), rgb(*b), "two profile badges collide in {mode}");
                }
            }
        }
    }

    /// Judgement 1: the saver blacks out the display, whatever the user's mode.
    ///
    /// This is the property that lets the saver pin its palette to dark, so if
    /// it ever stops holding the pinning becomes wrong too.
    #[test]
    fn the_screen_saver_blacks_out_the_display_in_every_style_it_draws() {
        for style in every_style() {
            let mut ss = ScreenSaver::new(style, 800, 600);
            let cmds = ss.render_frame();
            if style == ScreenSaverStyle::Disabled {
                assert!(cmds.is_empty(), "{style:?} drew over the desktop");
                continue;
            }
            match cmds[0] {
                RenderCommand::FillRect {
                    x,
                    y,
                    width,
                    height,
                    color,
                    ..
                } => {
                    assert_eq!((x, y, width, height), (0.0, 0.0, 800.0, 600.0), "{style:?}");
                    assert_eq!(
                        (color.r, color.g, color.b),
                        (0, 0, 0),
                        "{style:?} lit the display instead of blacking it out"
                    );
                }
                _ => panic!("{style:?} does not start with a background"),
            }
        }
    }

    /// The logo's label is ink *on* the logo, so it is derived from the plate.
    ///
    /// The plate never varies here, so — unlike the accent cases — this cannot
    /// prove derivation the way varying the fill would. What it does catch is
    /// the failure that matters: a named role in place of the derivation, which
    /// is how the label used to be written (`COL_BASE`, a dark role on a light
    /// plate that a change of plate would have made unreadable).
    #[test]
    fn the_logo_label_is_readable_on_the_logo_plate() {
        let mut logo = ScreenSaver::new(ScreenSaverStyle::BouncingLogo, 800, 600);
        let cmds = logo.render_frame();
        let plate = fill_of_height(&cmds, 60.0);
        let label = text_exact(&cmds, "Slate OS", 28.0);
        assert_eq!(
            rgb(label),
            rgb(readable_on(plate)),
            "the logo's label is not derived from its plate"
        );
    }
}
