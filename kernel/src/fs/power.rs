//! Power management — system power state, sleep timers, and battery.
//!
//! Manages power button actions, sleep/hibernate policies, screen
//! timeout, battery thresholds, and power profiles.
//!
//! ## Design Reference
//!
//! design.txt lines 1302-1316:
//! - Power button actions (shutdown/sleep/hibernate/custom)
//! - Laptop lid close action
//! - Screen off after N minutes
//! - Sleep after N minutes
//! - Battery threshold actions
//!
//! ## Architecture
//!
//! ```text
//! ACPI / hardware events
//!   → power::handle_power_button()
//!   → power::handle_lid_close()
//!   → power::handle_battery_update(percent, minutes_left)
//!
//! Settings panel
//!   → power::config() / set_config()
//!
//! Idle timer daemon
//!   → power::check_idle(idle_seconds)
//!   → returns PowerAction to execute
//! ```

#![allow(dead_code)]

use alloc::string::String;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::KernelResult;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A power action the system can take.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PowerAction {
    /// Do nothing.
    Nothing,
    /// Turn off the display only.
    ScreenOff,
    /// Enter sleep (suspend to RAM).
    Sleep,
    /// Enter hibernation (suspend to disk).
    Hibernate,
    /// Shut down the system.
    ShutDown,
    /// Log out the current user.
    LogOut,
    /// Lock the screen.
    LockScreen,
    /// Run a custom command before the action.
    RunThenSleep(String),
    /// Run a custom command before shutdown.
    RunThenShutdown(String),
}

impl PowerAction {
    /// Display label.
    pub fn label(&self) -> &str {
        match self {
            Self::Nothing => "Nothing",
            Self::ScreenOff => "Screen Off",
            Self::Sleep => "Sleep",
            Self::Hibernate => "Hibernate",
            Self::ShutDown => "Shut Down",
            Self::LogOut => "Log Out",
            Self::LockScreen => "Lock Screen",
            Self::RunThenSleep(_) => "Run then Sleep",
            Self::RunThenShutdown(_) => "Run then Shutdown",
        }
    }

    /// Parse from string.
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "nothing" | "none" => Some(Self::Nothing),
            "screenoff" | "screen-off" | "display-off" => Some(Self::ScreenOff),
            "sleep" | "suspend" => Some(Self::Sleep),
            "hibernate" | "hib" => Some(Self::Hibernate),
            "shutdown" | "poweroff" => Some(Self::ShutDown),
            "logout" | "logoff" => Some(Self::LogOut),
            "lock" | "lockscreen" => Some(Self::LockScreen),
            _ => None,
        }
    }
}

/// Power profile (performance vs battery life tradeoff).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerProfile {
    /// Maximum performance, no power saving.
    Performance,
    /// Balanced — moderate power saving.
    Balanced,
    /// Power saver — aggressive power reduction.
    PowerSaver,
}

impl PowerProfile {
    /// Display label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Performance => "Performance",
            Self::Balanced => "Balanced",
            Self::PowerSaver => "Power Saver",
        }
    }

    /// Parse from string.
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "performance" | "perf" | "high" => Some(Self::Performance),
            "balanced" | "bal" | "normal" => Some(Self::Balanced),
            "powersaver" | "saver" | "low" | "eco" => Some(Self::PowerSaver),
            _ => None,
        }
    }
}

/// Power source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerSource {
    /// Running on AC power.
    AC,
    /// Running on battery.
    Battery,
}

impl PowerSource {
    /// Display label.
    pub fn label(self) -> &'static str {
        match self {
            Self::AC => "AC Power",
            Self::Battery => "Battery",
        }
    }
}

/// Battery status.
#[derive(Debug, Clone)]
pub struct BatteryStatus {
    /// Whether a battery is present.
    pub present: bool,
    /// Current charge percentage (0-100).
    pub percent: u8,
    /// Estimated minutes remaining (-1 = unknown, 0 = calculating).
    pub minutes_left: i32,
    /// Whether the battery is currently charging.
    pub charging: bool,
    /// Battery health percentage (0-100, from design capacity).
    pub health: u8,
    /// Current power source.
    pub source: PowerSource,
}

/// Full power configuration.
#[derive(Debug, Clone)]
pub struct PowerConfig {
    /// Action when power button is pressed.
    pub power_button_action: PowerAction,
    /// Action when laptop lid is closed.
    pub lid_close_action: PowerAction,
    /// Minutes of idle before screen off (0 = disabled).
    pub screen_off_minutes: u32,
    /// Minutes of idle before sleep (0 = disabled).
    pub sleep_minutes: u32,
    /// Battery percentage threshold for low battery action.
    pub low_battery_percent: u8,
    /// Action when battery is below threshold.
    pub low_battery_action: PowerAction,
    /// Minutes remaining threshold for critical battery.
    pub critical_battery_minutes: u32,
    /// Action for critical battery.
    pub critical_battery_action: PowerAction,
    /// Active power profile.
    pub profile: PowerProfile,
    /// Whether to automatically switch to Power Saver on battery.
    pub auto_power_saver: bool,
    /// Custom command to run before sleep (design.txt line 1313).
    pub pre_sleep_command: String,
    /// Custom command to run before shutdown.
    pub pre_shutdown_command: String,
    /// Screen brightness on battery (0-100, 0 = don't change).
    pub battery_brightness: u8,
    /// Whether USB selective suspend is enabled.
    pub usb_suspend: bool,
    /// CPU frequency scaling policy on battery.
    pub cpu_battery_limit: u8,
}

impl PowerConfig {
    fn new() -> Self {
        Self {
            power_button_action: PowerAction::Sleep,
            lid_close_action: PowerAction::Sleep,
            screen_off_minutes: 5,
            sleep_minutes: 15,
            low_battery_percent: 15,
            low_battery_action: PowerAction::Hibernate,
            critical_battery_minutes: 5,
            critical_battery_action: PowerAction::Hibernate,
            profile: PowerProfile::Balanced,
            auto_power_saver: true,
            pre_sleep_command: String::new(),
            pre_shutdown_command: String::new(),
            battery_brightness: 60,
            usb_suspend: true,
            cpu_battery_limit: 80,
        }
    }
}

// ---------------------------------------------------------------------------
// Internal state
// ---------------------------------------------------------------------------

struct PowerState {
    config: PowerConfig,
    battery: BatteryStatus,
    /// Last idle check time (ns).
    last_idle_check_ns: u64,
    /// Whether screen is currently off.
    screen_off: bool,
    /// Whether system is about to sleep (pre-sleep phase).
    sleep_pending: bool,
}

impl PowerState {
    const fn new() -> Self {
        Self {
            config: PowerConfig {
                power_button_action: PowerAction::Sleep,
                lid_close_action: PowerAction::Sleep,
                screen_off_minutes: 5,
                sleep_minutes: 15,
                low_battery_percent: 15,
                low_battery_action: PowerAction::Hibernate,
                critical_battery_minutes: 5,
                critical_battery_action: PowerAction::Hibernate,
                profile: PowerProfile::Balanced,
                auto_power_saver: true,
                pre_sleep_command: String::new(),
                pre_shutdown_command: String::new(),
                battery_brightness: 60,
                usb_suspend: true,
                cpu_battery_limit: 80,
            },
            battery: BatteryStatus {
                present: false,
                percent: 100,
                minutes_left: -1,
                charging: false,
                health: 100,
                source: PowerSource::AC,
            },
            last_idle_check_ns: 0,
            screen_off: false,
            sleep_pending: false,
        }
    }
}

static POWER: Mutex<PowerState> = Mutex::new(PowerState::new());
static EVENT_COUNT: AtomicU64 = AtomicU64::new(0);
static IDLE_CHECK_COUNT: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Get current power configuration.
pub fn config() -> PowerConfig {
    POWER.lock().config.clone()
}

/// Set power button action.
pub fn set_power_button_action(action: PowerAction) {
    POWER.lock().config.power_button_action = action;
}

/// Set lid close action.
pub fn set_lid_close_action(action: PowerAction) {
    POWER.lock().config.lid_close_action = action;
}

/// Set screen-off timeout (minutes, 0 to disable).
pub fn set_screen_off_minutes(minutes: u32) {
    POWER.lock().config.screen_off_minutes = minutes;
}

/// Set sleep timeout (minutes, 0 to disable).
pub fn set_sleep_minutes(minutes: u32) {
    POWER.lock().config.sleep_minutes = minutes;
}

/// Set low battery threshold and action.
pub fn set_low_battery(percent: u8, action: PowerAction) {
    let mut state = POWER.lock();
    state.config.low_battery_percent = percent.min(100);
    state.config.low_battery_action = action;
}

/// Set critical battery threshold and action.
pub fn set_critical_battery(minutes: u32, action: PowerAction) {
    let mut state = POWER.lock();
    state.config.critical_battery_minutes = minutes;
    state.config.critical_battery_action = action;
}

/// Set power profile.
pub fn set_profile(profile: PowerProfile) {
    POWER.lock().config.profile = profile;
}

/// Get current power profile.
pub fn current_profile() -> PowerProfile {
    POWER.lock().config.profile
}

/// Set auto power-saver on battery.
pub fn set_auto_power_saver(enabled: bool) {
    POWER.lock().config.auto_power_saver = enabled;
}

/// Set pre-sleep command.
pub fn set_pre_sleep_command(cmd: &str) {
    POWER.lock().config.pre_sleep_command = String::from(cmd);
}

/// Set pre-shutdown command.
pub fn set_pre_shutdown_command(cmd: &str) {
    POWER.lock().config.pre_shutdown_command = String::from(cmd);
}

/// Set battery brightness target.
pub fn set_battery_brightness(percent: u8) {
    POWER.lock().config.battery_brightness = percent.min(100);
}

/// Set USB selective suspend.
pub fn set_usb_suspend(enabled: bool) {
    POWER.lock().config.usb_suspend = enabled;
}

/// Set CPU battery limit percentage.
pub fn set_cpu_battery_limit(percent: u8) {
    POWER.lock().config.cpu_battery_limit = percent.min(100);
}

// ---------------------------------------------------------------------------
// Hardware event handlers
// ---------------------------------------------------------------------------

/// Handle power button press — returns the action to execute.
pub fn handle_power_button() -> PowerAction {
    EVENT_COUNT.fetch_add(1, Ordering::Relaxed);
    POWER.lock().config.power_button_action.clone()
}

/// Handle laptop lid close — returns the action to execute.
pub fn handle_lid_close() -> PowerAction {
    EVENT_COUNT.fetch_add(1, Ordering::Relaxed);
    POWER.lock().config.lid_close_action.clone()
}

/// Handle laptop lid open.
pub fn handle_lid_open() {
    EVENT_COUNT.fetch_add(1, Ordering::Relaxed);
    let mut state = POWER.lock();
    state.screen_off = false;
}

/// Update battery status from ACPI/driver.
pub fn handle_battery_update(percent: u8, minutes_left: i32, charging: bool) {
    EVENT_COUNT.fetch_add(1, Ordering::Relaxed);
    let mut state = POWER.lock();
    state.battery.present = true;
    state.battery.percent = percent.min(100);
    state.battery.minutes_left = minutes_left;
    state.battery.charging = charging;
    state.battery.source = if charging { PowerSource::AC } else { PowerSource::Battery };

    // Auto power-saver.
    if state.config.auto_power_saver && !charging {
        state.config.profile = PowerProfile::PowerSaver;
    } else if state.config.auto_power_saver && charging
        && state.config.profile == PowerProfile::PowerSaver
    {
        state.config.profile = PowerProfile::Balanced;
    }
}

/// Set battery health (from ACPI design capacity vs current capacity).
pub fn set_battery_health(health: u8) {
    POWER.lock().battery.health = health.min(100);
}

/// Get current battery status.
pub fn battery_status() -> BatteryStatus {
    POWER.lock().battery.clone()
}

// ---------------------------------------------------------------------------
// Idle check
// ---------------------------------------------------------------------------

/// Check if any power action should be triggered based on idle time.
///
/// `idle_seconds` is how long since the last user input event.
/// Returns the action to take (or Nothing if no threshold reached).
pub fn check_idle(idle_seconds: u64) -> PowerAction {
    IDLE_CHECK_COUNT.fetch_add(1, Ordering::Relaxed);
    let now = crate::timekeeping::clock_monotonic();
    let mut state = POWER.lock();
    state.last_idle_check_ns = now;

    // Sleep check first (longer timeout, higher priority action).
    if state.config.sleep_minutes > 0 {
        let threshold = (state.config.sleep_minutes as u64).saturating_mul(60);
        if idle_seconds >= threshold {
            return PowerAction::Sleep;
        }
    }

    // Screen off check.
    if state.config.screen_off_minutes > 0 && !state.screen_off {
        let threshold = (state.config.screen_off_minutes as u64).saturating_mul(60);
        if idle_seconds >= threshold {
            state.screen_off = true;
            return PowerAction::ScreenOff;
        }
    }

    PowerAction::Nothing
}

/// Check if battery level triggers an action.
pub fn check_battery() -> PowerAction {
    let state = POWER.lock();
    if !state.battery.present || state.battery.charging {
        return PowerAction::Nothing;
    }

    // Critical: check minutes remaining.
    if state.config.critical_battery_minutes > 0
        && state.battery.minutes_left >= 0
        && (state.battery.minutes_left as u32) <= state.config.critical_battery_minutes
    {
        return state.config.critical_battery_action.clone();
    }

    // Low: check percentage.
    if state.battery.percent <= state.config.low_battery_percent {
        return state.config.low_battery_action.clone();
    }

    PowerAction::Nothing
}

/// Wake up from screen off (user input detected).
pub fn wake_screen() {
    let mut state = POWER.lock();
    state.screen_off = false;
}

/// Check if screen is currently off.
pub fn is_screen_off() -> bool {
    POWER.lock().screen_off
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Returns (event_count, idle_checks, is_screen_off, battery_present).
pub fn stats() -> (u64, u64, bool, bool) {
    let state = POWER.lock();
    (
        EVENT_COUNT.load(Ordering::Relaxed),
        IDLE_CHECK_COUNT.load(Ordering::Relaxed),
        state.screen_off,
        state.battery.present,
    )
}

/// Reset counters.
pub fn reset_stats() {
    EVENT_COUNT.store(0, Ordering::Relaxed);
    IDLE_CHECK_COUNT.store(0, Ordering::Relaxed);
}

/// Clear all data.
pub fn clear_all() {
    let mut state = POWER.lock();
    state.config = PowerConfig::new();
    state.battery = BatteryStatus {
        present: false,
        percent: 100,
        minutes_left: -1,
        charging: false,
        health: 100,
        source: PowerSource::AC,
    };
    state.screen_off = false;
    state.sleep_pending = false;
    state.last_idle_check_ns = 0;
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run self-tests for the power management system.
pub fn self_test() -> KernelResult<()> {
    use crate::serial_println;

    clear_all();
    reset_stats();

    // Test 1: Default configuration.
    serial_println!("  power::test 1: defaults");
    let cfg = config();
    assert_eq!(cfg.profile, PowerProfile::Balanced);
    assert_eq!(cfg.screen_off_minutes, 5);
    assert_eq!(cfg.sleep_minutes, 15);
    assert_eq!(cfg.power_button_action, PowerAction::Sleep);

    // Test 2: Power button and lid actions.
    serial_println!("  power::test 2: hardware events");
    set_power_button_action(PowerAction::ShutDown);
    let action = handle_power_button();
    assert_eq!(action, PowerAction::ShutDown);
    set_lid_close_action(PowerAction::Hibernate);
    let action2 = handle_lid_close();
    assert_eq!(action2, PowerAction::Hibernate);

    // Test 3: Idle check — screen off.
    serial_println!("  power::test 3: idle screen off");
    set_screen_off_minutes(2);
    set_sleep_minutes(10);
    let a1 = check_idle(60);  // 1 min — nothing yet.
    assert_eq!(a1, PowerAction::Nothing);
    let a2 = check_idle(120); // 2 min — screen off.
    assert_eq!(a2, PowerAction::ScreenOff);
    assert!(is_screen_off());
    wake_screen();
    assert!(!is_screen_off());

    // Test 4: Idle check — sleep.
    serial_println!("  power::test 4: idle sleep");
    let a3 = check_idle(600); // 10 min — sleep.
    assert_eq!(a3, PowerAction::Sleep);

    // Test 5: Battery updates and auto power-saver.
    serial_println!("  power::test 5: battery and auto power-saver");
    set_auto_power_saver(true);
    set_profile(PowerProfile::Balanced);
    handle_battery_update(80, 120, false); // On battery.
    assert_eq!(current_profile(), PowerProfile::PowerSaver);
    handle_battery_update(90, -1, true); // Plugged in.
    assert_eq!(current_profile(), PowerProfile::Balanced);

    // Test 6: Battery threshold actions.
    serial_println!("  power::test 6: battery thresholds");
    set_low_battery(20, PowerAction::Hibernate);
    set_critical_battery(5, PowerAction::ShutDown);
    handle_battery_update(25, 30, false);
    assert_eq!(check_battery(), PowerAction::Nothing);
    handle_battery_update(15, 10, false);
    assert_eq!(check_battery(), PowerAction::Hibernate); // Below 20%.
    handle_battery_update(10, 3, false);
    assert_eq!(check_battery(), PowerAction::ShutDown); // Below 5 min.

    // Test 7: Profile and configuration.
    serial_println!("  power::test 7: profiles");
    set_profile(PowerProfile::Performance);
    assert_eq!(current_profile(), PowerProfile::Performance);
    set_battery_brightness(40);
    set_cpu_battery_limit(60);
    let cfg2 = config();
    assert_eq!(cfg2.battery_brightness, 40);
    assert_eq!(cfg2.cpu_battery_limit, 60);

    // Cleanup.
    clear_all();
    reset_stats();

    serial_println!("  power: all tests passed");
    Ok(())
}
