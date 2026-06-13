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
//!
//! Designed to integrate with the taskbar's power/battery indicator
//! and the settings app's power management page.

use guitk::color::Color;
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

// ============================================================================
// Catppuccin Mocha theme constants
// ============================================================================

const COL_BASE: Color = Color::from_hex(0x1E1E2E);
const COL_SURFACE0: Color = Color::from_hex(0x313244);
const COL_SURFACE1: Color = Color::from_hex(0x45475A);
const COL_SURFACE2: Color = Color::from_hex(0x585B70);
const COL_OVERLAY0: Color = Color::from_hex(0x6C7086);
const COL_TEXT: Color = Color::from_hex(0xCDD6F4);
const COL_SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const COL_BLUE: Color = Color::from_hex(0x89B4FA);
const COL_GREEN: Color = Color::from_hex(0xA6E3A1);
const COL_YELLOW: Color = Color::from_hex(0xF9E2AF);
const COL_RED: Color = Color::from_hex(0xF38BA8);
const COL_PEACH: Color = Color::from_hex(0xFAB387);
const COL_LAVENDER: Color = Color::from_hex(0xB4BEFE);
const COL_MANTLE: Color = Color::from_hex(0x181825);

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
            screen_off_timeout_secs: 300,  // 5 minutes
            suspend_timeout_secs: 900,     // 15 minutes
            screensaver_timeout_secs: 0,   // disabled by default
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
        out.push_str(&format!("screen_off_timeout={}\n", self.screen_off_timeout_secs));
        out.push_str(&format!("suspend_timeout={}\n", self.suspend_timeout_secs));
        out.push_str(&format!("screensaver_timeout={}\n", self.screensaver_timeout_secs));
        out.push_str(&format!("screensaver_style={}\n", screensaver_str(self.screensaver_style)));
        out.push_str(&format!("lid_close_action={}\n", action_str(self.lid_close_action)));
        out.push_str(&format!("power_button_action={}\n", action_str(self.power_button_action)));
        out.push_str(&format!("sleep_button_action={}\n", action_str(self.sleep_button_action)));
        out.push_str(&format!("low_battery_pct={}\n", self.low_battery_pct));
        out.push_str(&format!("critical_battery_pct={}\n", self.critical_battery_pct));
        out.push_str(&format!("critical_battery_action={}\n", action_str(self.critical_battery_action)));
        out.push_str(&format!("wake_on_lan={}\n", self.wake_on_lan));
        out.push_str(&format!("cpu_governor={}\n", governor_str(self.cpu_governor)));
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
    pub fn time_remaining_str(&self) -> Option<String> {
        self.time_remaining_secs.map(|secs| {
            let hours = secs / 3600;
            let mins = (secs % 3600) / 60;
            if hours > 0 {
                format!("{}h {}m", hours, mins)
            } else {
                format!("{}m", mins)
            }
        })
    }

    /// Whether battery is in a warning state.
    pub fn is_warning(&self, config: &PowerConfig) -> bool {
        self.present && self.charge_pct <= config.low_battery_pct
            && self.state == BatteryState::Discharging
    }

    /// Whether battery is in a critical state.
    pub fn is_critical(&self, config: &PowerConfig) -> bool {
        self.present && self.charge_pct <= config.critical_battery_pct
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
    pub fn add_inhibitor(
        &mut self,
        app_name: &str,
        reason: &str,
        what: InhibitTarget,
    ) -> u32 {
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
        self.inhibitors.iter().any(|i| i.what == target || i.what == InhibitTarget::All)
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
            self.transition_to(PowerState::ScreenOff, now_secs, TransitionReason::IdleTimeout);
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
            self.transition_to(PowerState::ScreenOff, now_secs, TransitionReason::IdleTimeout);
            return Some(PowerAction::ScreenOff);
        }

        // After screen off, suspend if configured.
        if cfg.suspend_timeout_secs > 0
            && self.idle_secs >= cfg.suspend_timeout_secs
            && !self.is_inhibited(InhibitTarget::Suspend)
        {
            self.transition_to(PowerState::Suspended, now_secs, TransitionReason::IdleTimeout);
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

// ============================================================================
// Screen saver engine
// ============================================================================

/// A single star in the starfield screen saver.
#[derive(Debug, Clone)]
struct Star {
    x: f32,
    y: f32,
    z: f32,  // depth (1.0 = far, 0.01 = close)
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
    /// Simple PRNG state.
    rng_state: u64,
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
            rng_state: 0x12345678_9ABCDEF0,
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
            let x = self.next_random_f32() * 2.0 - 1.0;
            let y = self.next_random_f32() * 2.0 - 1.0;
            let z = self.next_random_f32() * 0.99 + 0.01;
            let speed = self.next_random_f32() * 0.01 + 0.002;
            self.stars.push(Star { x, y, z, speed });
        }
    }

    fn init_matrix(&mut self) {
        self.columns.clear();
        let col_count = self.width / 14; // ~14px per character column
        for i in 0..col_count {
            let len = self.next_random_u32() % 20 + 5;
            let mut chars = Vec::with_capacity(len as usize);
            for _ in 0..len {
                chars.push((self.next_random_u32() % 94 + 33) as u8); // printable ASCII
            }
            let col_y = -(self.next_random_f32() * self.height as f32);
            let col_speed = self.next_random_f32() * 3.0 + 1.0;
            self.columns.push(MatrixColumn {
                x: i * 14,
                y: col_y,
                speed: col_speed,
                chars,
                length: len,
            });
        }
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
            color: COL_LAVENDER,
            font_size: 72.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        cmds
    }

    fn render_starfield(&mut self) -> Vec<RenderCommand> {
        let mut cmds = Vec::with_capacity(self.stars.len() + 1);

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

        // Operate on rng_state directly to avoid borrowing &mut self while
        // iterating over &mut self.stars.
        let mut rng = self.rng_state;
        for star in &mut self.stars {
            star.z -= star.speed;
            if star.z <= 0.01 {
                rng ^= rng << 13;
                rng ^= rng >> 7;
                rng ^= rng << 17;
                star.x = ((rng & 0xFFFFFFFF) as f32 / u32::MAX as f32) * 2.0 - 1.0;
                rng ^= rng << 13;
                rng ^= rng >> 7;
                rng ^= rng << 17;
                star.y = ((rng & 0xFFFFFFFF) as f32 / u32::MAX as f32) * 2.0 - 1.0;
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
        self.rng_state = rng;

        cmds
    }

    fn render_matrix(&mut self) -> Vec<RenderCommand> {
        let mut cmds = Vec::with_capacity(self.columns.len() * 10 + 1);

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
        for col in &mut self.columns {
            col.y += col.speed;
            if col.y > h + (col.length as f32 * 16.0) {
                col.y = -(col.length as f32 * 16.0);
                // Randomize chars.
                for c in &mut col.chars {
                    // Simple variation without full PRNG access.
                    *c = ((*c as u32).wrapping_add(7) % 94 + 33) as u8;
                }
            }

            for (i, ch) in col.chars.iter().enumerate() {
                let cy = col.y + (i as f32 * 16.0);
                if cy < 0.0 || cy >= self.height as f32 {
                    continue;
                }
                let green = if i == 0 {
                    255u8 // brightest at the head
                } else {
                    (255u32.saturating_sub((i as u32 * 8).min(200))) as u8
                };
                cmds.push(RenderCommand::Text {
                    x: col.x as f32,
                    y: cy,
                    text: String::from(*ch as char),
                    color: Color::rgba(0, green, 0, 255),
                    font_size: 14.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(14.0),
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
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width: logo_w,
            height: logo_h,
            color: COL_BLUE,
            corner_radii: radii,
        });

        // Logo text.
        cmds.push(RenderCommand::Text {
            x: x + 15.0,
            y: y + 15.0,
            text: "Slate OS".to_string(),
            color: COL_BASE,
            font_size: 28.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(logo_w - 30.0),
        });

        cmds
    }

    /// Simple xorshift64 PRNG.
    fn next_random(&mut self) -> u64 {
        let mut x = self.rng_state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng_state = x;
        x
    }

    fn next_random_u32(&mut self) -> u32 {
        (self.next_random() & 0xFFFFFFFF) as u32
    }

    fn next_random_f32(&mut self) -> f32 {
        (self.next_random_u32() as f32) / (u32::MAX as f32)
    }

    /// Random without &mut self (uses internal state mutation via frame counter trick).
    fn next_random_f32_from_state(&mut self) -> f32 {
        self.next_random_f32()
    }
}

// ============================================================================
// Battery icon rendering
// ============================================================================

/// Render a battery indicator for the taskbar.
pub fn render_battery_icon(
    battery: &BatteryInfo,
    config: &PowerConfig,
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
            color: COL_SUBTEXT0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(24.0),
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
        color: COL_TEXT,
        line_width: 1.0,
        corner_radii: CornerRadii::all(2.0),
    });

    // Battery tip.
    cmds.push(RenderCommand::FillRect {
        x: x + batt_w,
        y: y + 2.0 + (batt_h - tip_h) / 2.0,
        width: tip_w,
        height: tip_h,
        color: COL_TEXT,
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
    let fill_color = if battery.is_critical(config) {
        COL_RED
    } else if battery.is_warning(config) {
        COL_YELLOW
    } else if battery.charge_pct > 80 {
        COL_GREEN
    } else {
        COL_BLUE
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
            color: COL_YELLOW,
            font_size: 10.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(batt_w),
        });
    }

    // Percentage text.
    if config.show_battery_pct {
        cmds.push(RenderCommand::Text {
            x: x + batt_w + tip_w + 4.0,
            y: y + 2.0,
            text: format!("{}%", battery.charge_pct),
            color: COL_SUBTEXT0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(36.0),
        });
    }

    cmds
}

/// Render a power profile indicator (for settings or quick settings).
pub fn render_power_profile_badge(
    profile: PowerProfile,
    x: f32,
    y: f32,
) -> Vec<RenderCommand> {
    let (label, color) = match profile {
        PowerProfile::Balanced => ("Balanced", COL_BLUE),
        PowerProfile::Performance => ("Performance", COL_PEACH),
        PowerProfile::PowerSaver => ("Power Saver", COL_GREEN),
        PowerProfile::Custom => ("Custom", COL_LAVENDER),
    };

    let text_width = label.len() as f32 * 7.0;
    let badge_w = text_width + 16.0;
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
        },
    ]
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
                if i % 2 == 0 { PowerState::Dimmed } else { PowerState::Active },
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

    #[test]
    fn test_battery_icon_no_battery() {
        let b = BatteryInfo::no_battery();
        let cfg = PowerConfig::default();
        let cmds = render_battery_icon(&b, &cfg, 10.0, 10.0);
        assert!(!cmds.is_empty()); // Shows "AC" text.
    }

    #[test]
    fn test_battery_icon_with_battery() {
        let b = BatteryInfo::with_battery(75, BatteryState::Discharging);
        let cfg = PowerConfig::default();
        let cmds = render_battery_icon(&b, &cfg, 10.0, 10.0);
        assert!(cmds.len() >= 3); // outline + tip + fill
    }

    #[test]
    fn test_battery_icon_charging() {
        let b = BatteryInfo::with_battery(50, BatteryState::Charging);
        let cfg = PowerConfig::default();
        let cmds = render_battery_icon(&b, &cfg, 10.0, 10.0);
        assert!(cmds.len() >= 4); // outline + tip + fill + charging symbol
    }

    #[test]
    fn test_power_profile_badge() {
        let cmds = render_power_profile_badge(PowerProfile::Performance, 0.0, 0.0);
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
            let cmds = render_power_profile_badge(*profile, 0.0, 0.0);
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
}
