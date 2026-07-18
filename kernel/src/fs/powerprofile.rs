//! Power profiles — balanced, performance, and power-saver modes.
//!
//! Manages system power profiles that control CPU governor, display
//! brightness, suspend timing, and background activity.  Supports
//! automatic profile switching based on AC/battery state.
//!
//! ## Architecture
//!
//! ```text
//! Settings panel → Power → Power Profile
//!   → powerprofile::set_profile()
//!
//! System tray indicator
//!   → powerprofile::active_profile() / battery_status()
//!
//! Integration:
//!   → power (suspend/hibernate)
//!   → display (brightness)
//!   → schedtune (CPU governor)
//!   → focusassist (auto-DND in power saver)
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

const MAX_CUSTOM_PROFILES: usize = 8;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Built-in power profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileType {
    /// Balanced — default, trades performance for battery life.
    Balanced,
    /// Performance — maximum CPU/GPU performance, no throttling.
    Performance,
    /// Power Saver — aggressive power savings, reduced brightness.
    PowerSaver,
    /// Custom user-defined profile.
    Custom,
}

impl ProfileType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Balanced => "Balanced",
            Self::Performance => "Performance",
            Self::PowerSaver => "Power Saver",
            Self::Custom => "Custom",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::Balanced => "⚖",
            Self::Performance => "🚀",
            Self::PowerSaver => "🔋",
            Self::Custom => "⚙",
        }
    }
}

/// CPU governor policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CpuGovernor {
    /// Performance — max frequency.
    Performance,
    /// On-demand — scale with load.
    OnDemand,
    /// Conservative — slowly ramp up.
    Conservative,
    /// Powersave — minimum frequency.
    Powersave,
    /// Schedutil — scheduler-driven.
    Schedutil,
}

impl CpuGovernor {
    pub fn label(self) -> &'static str {
        match self {
            Self::Performance => "performance",
            Self::OnDemand => "ondemand",
            Self::Conservative => "conservative",
            Self::Powersave => "powersave",
            Self::Schedutil => "schedutil",
        }
    }
}

/// Battery state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatteryState {
    /// Charging.
    Charging,
    /// Discharging.
    Discharging,
    /// Full.
    Full,
    /// No battery (desktop).
    NoBattery,
}

impl BatteryState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Charging => "Charging",
            Self::Discharging => "Discharging",
            Self::Full => "Full",
            Self::NoBattery => "No Battery",
        }
    }
}

/// Power profile settings.
#[derive(Debug, Clone)]
pub struct PowerProfile {
    /// Profile name.
    pub name: String,
    /// Profile type.
    pub profile_type: ProfileType,
    /// CPU governor.
    pub cpu_governor: CpuGovernor,
    /// CPU max frequency percentage (10-100).
    pub cpu_max_pct: u32,
    /// CPU min frequency percentage (0-100).
    pub cpu_min_pct: u32,
    /// CPU boost (turbo) enabled.
    pub cpu_boost: bool,
    /// Display brightness percentage (1-100).
    pub brightness_pct: u32,
    /// Display dim after idle (seconds, 0 = never).
    pub dim_after_seconds: u32,
    /// Screen off after idle (seconds, 0 = never).
    pub screen_off_seconds: u32,
    /// Suspend after idle (seconds, 0 = never).
    pub suspend_seconds: u32,
    /// Reduce background activity.
    pub reduce_background: bool,
    /// Disable animations for power saving.
    pub disable_animations: bool,
    /// Hard drive spin-down time (seconds, 0 = never).
    pub disk_spindown_seconds: u32,
    /// WiFi power save.
    pub wifi_power_save: bool,
    /// Bluetooth auto-disable.
    pub bt_auto_disable: bool,
}

/// Battery information.
#[derive(Debug, Clone)]
pub struct BatteryInfo {
    /// Battery state.
    pub state: BatteryState,
    /// Charge percentage (0-100).
    pub charge_pct: u8,
    /// Estimated minutes remaining.
    pub minutes_remaining: u32,
    /// Battery health percentage (0-100).
    pub health_pct: u8,
    /// Cycle count.
    pub cycle_count: u32,
    /// Design capacity (mWh).
    pub design_capacity_mwh: u32,
    /// Current capacity (mWh).
    pub current_capacity_mwh: u32,
    /// Charge rate (mW, 0 if discharging).
    pub charge_rate_mw: u32,
    /// Discharge rate (mW, 0 if charging).
    pub discharge_rate_mw: u32,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct PowerProfileState {
    /// Built-in profiles.
    balanced: PowerProfile,
    performance: PowerProfile,
    power_saver: PowerProfile,
    /// Custom profiles.
    custom_profiles: Vec<PowerProfile>,
    /// Active profile name.
    active_profile: String,
    /// Auto-switch to power saver on battery.
    auto_switch_on_battery: bool,
    /// Auto-switch threshold (battery %).
    auto_switch_threshold: u8,
    /// Battery info.
    battery: BatteryInfo,
    /// Profile switch count.
    switch_count: u64,
    ops: u64,
}

static STATE: Mutex<Option<PowerProfileState>> = Mutex::new(None);
static OPS: AtomicU64 = AtomicU64::new(0);

fn with_state<F, R>(f: F) -> KernelResult<R>
where
    F: FnOnce(&mut PowerProfileState) -> KernelResult<R>,
{
    let mut guard = STATE.lock();
    let state = guard.as_mut().ok_or(KernelError::NotSupported)?;
    let result = f(state)?;
    state.ops += 1;
    OPS.store(state.ops, Ordering::Relaxed);
    Ok(result)
}

fn make_balanced() -> PowerProfile {
    PowerProfile {
        name: String::from("Balanced"),
        profile_type: ProfileType::Balanced,
        cpu_governor: CpuGovernor::Schedutil,
        cpu_max_pct: 100,
        cpu_min_pct: 20,
        cpu_boost: true,
        brightness_pct: 70,
        dim_after_seconds: 120,
        screen_off_seconds: 300,
        suspend_seconds: 900,
        reduce_background: false,
        disable_animations: false,
        disk_spindown_seconds: 600,
        wifi_power_save: false,
        bt_auto_disable: false,
    }
}

fn make_performance() -> PowerProfile {
    PowerProfile {
        name: String::from("Performance"),
        profile_type: ProfileType::Performance,
        cpu_governor: CpuGovernor::Performance,
        cpu_max_pct: 100,
        cpu_min_pct: 50,
        cpu_boost: true,
        brightness_pct: 100,
        dim_after_seconds: 0,
        screen_off_seconds: 0,
        suspend_seconds: 0,
        reduce_background: false,
        disable_animations: false,
        disk_spindown_seconds: 0,
        wifi_power_save: false,
        bt_auto_disable: false,
    }
}

fn make_power_saver() -> PowerProfile {
    PowerProfile {
        name: String::from("Power Saver"),
        profile_type: ProfileType::PowerSaver,
        cpu_governor: CpuGovernor::Powersave,
        cpu_max_pct: 60,
        cpu_min_pct: 10,
        cpu_boost: false,
        brightness_pct: 40,
        dim_after_seconds: 30,
        screen_off_seconds: 60,
        suspend_seconds: 300,
        reduce_background: true,
        disable_animations: true,
        disk_spindown_seconds: 120,
        wifi_power_save: true,
        bt_auto_disable: true,
    }
}

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Initialize the power profiles subsystem.
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() {
        return;
    }

    *guard = Some(PowerProfileState {
        balanced: make_balanced(),
        performance: make_performance(),
        power_saver: make_power_saver(),
        custom_profiles: Vec::new(),
        active_profile: String::from("Balanced"),
        auto_switch_on_battery: true,
        auto_switch_threshold: 20,
        battery: BatteryInfo {
            state: BatteryState::NoBattery,
            charge_pct: 0,
            minutes_remaining: 0,
            health_pct: 100,
            cycle_count: 0,
            design_capacity_mwh: 0,
            current_capacity_mwh: 0,
            charge_rate_mw: 0,
            discharge_rate_mw: 0,
        },
        switch_count: 0,
        ops: 0,
    });
}

// ---------------------------------------------------------------------------
// Profile management
// ---------------------------------------------------------------------------

/// Get the active profile.
pub fn active_profile() -> KernelResult<PowerProfile> {
    let guard = STATE.lock();
    let state = guard.as_ref().ok_or(KernelError::NotSupported)?;
    find_profile(state, &state.active_profile.clone())
        .ok_or(KernelError::NotFound)
}

/// Get the active profile name.
pub fn active_profile_name() -> String {
    let guard = STATE.lock();
    guard.as_ref().map_or_else(
        || String::from("Unknown"),
        |s| s.active_profile.clone(),
    )
}

fn find_profile(state: &PowerProfileState, name: &str) -> Option<PowerProfile> {
    match name {
        "Balanced" => Some(state.balanced.clone()),
        "Performance" => Some(state.performance.clone()),
        "Power Saver" => Some(state.power_saver.clone()),
        _ => state.custom_profiles.iter()
            .find(|p| p.name == name)
            .cloned(),
    }
}

/// Set the active power profile by name.
pub fn set_profile(name: &str) -> KernelResult<()> {
    with_state(|state| {
        // Verify profile exists.
        if find_profile(state, name).is_none() {
            return Err(KernelError::NotFound);
        }
        state.active_profile = String::from(name);
        state.switch_count += 1;
        Ok(())
    })
}

/// List all available profiles.
pub fn list_profiles() -> Vec<PowerProfile> {
    let guard = STATE.lock();
    guard.as_ref().map_or_else(Vec::new, |s| {
        let mut profiles = alloc::vec![
            s.balanced.clone(),
            s.performance.clone(),
            s.power_saver.clone(),
        ];
        for p in &s.custom_profiles {
            profiles.push(p.clone());
        }
        profiles
    })
}

/// Create a custom profile.
pub fn create_profile(name: &str) -> KernelResult<()> {
    if name.is_empty() || name == "Balanced" || name == "Performance" || name == "Power Saver" {
        return Err(KernelError::InvalidArgument);
    }
    with_state(|state| {
        if state.custom_profiles.len() >= MAX_CUSTOM_PROFILES {
            return Err(KernelError::ResourceExhausted);
        }
        if state.custom_profiles.iter().any(|p| p.name == name) {
            return Err(KernelError::AlreadyExists);
        }
        // Start from balanced defaults.
        let mut profile = make_balanced();
        profile.name = String::from(name);
        profile.profile_type = ProfileType::Custom;
        state.custom_profiles.push(profile);
        Ok(())
    })
}

/// Remove a custom profile.
pub fn remove_profile(name: &str) -> KernelResult<()> {
    if name == "Balanced" || name == "Performance" || name == "Power Saver" {
        return Err(KernelError::InvalidArgument);
    }
    with_state(|state| {
        if let Some(pos) = state.custom_profiles.iter().position(|p| p.name == name) {
            // If removing the active profile, switch to Balanced.
            if state.active_profile == name {
                state.active_profile = String::from("Balanced");
            }
            state.custom_profiles.remove(pos);
            Ok(())
        } else {
            Err(KernelError::NotFound)
        }
    })
}

// ---------------------------------------------------------------------------
// Profile parameter modification
// ---------------------------------------------------------------------------

/// Modify a profile parameter (works on active profile).
pub fn set_cpu_governor(governor: CpuGovernor) -> KernelResult<()> {
    with_state(|state| {
        let name = state.active_profile.clone();
        let profile = find_profile_mut(state, &name)?;
        profile.cpu_governor = governor;
        Ok(())
    })
}

pub fn set_cpu_max_pct(pct: u32) -> KernelResult<()> {
    if pct < 10 || pct > 100 {
        return Err(KernelError::InvalidArgument);
    }
    with_state(|state| {
        let name = state.active_profile.clone();
        let profile = find_profile_mut(state, &name)?;
        profile.cpu_max_pct = pct;
        Ok(())
    })
}

pub fn set_cpu_boost(enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        let name = state.active_profile.clone();
        let profile = find_profile_mut(state, &name)?;
        profile.cpu_boost = enabled;
        Ok(())
    })
}

pub fn set_brightness(pct: u32) -> KernelResult<()> {
    if pct < 1 || pct > 100 {
        return Err(KernelError::InvalidArgument);
    }
    with_state(|state| {
        let name = state.active_profile.clone();
        let profile = find_profile_mut(state, &name)?;
        profile.brightness_pct = pct;
        Ok(())
    })
}

pub fn set_dim_after(seconds: u32) -> KernelResult<()> {
    with_state(|state| {
        let name = state.active_profile.clone();
        let profile = find_profile_mut(state, &name)?;
        profile.dim_after_seconds = seconds;
        Ok(())
    })
}

pub fn set_screen_off(seconds: u32) -> KernelResult<()> {
    with_state(|state| {
        let name = state.active_profile.clone();
        let profile = find_profile_mut(state, &name)?;
        profile.screen_off_seconds = seconds;
        Ok(())
    })
}

pub fn set_suspend_after(seconds: u32) -> KernelResult<()> {
    with_state(|state| {
        let name = state.active_profile.clone();
        let profile = find_profile_mut(state, &name)?;
        profile.suspend_seconds = seconds;
        Ok(())
    })
}

fn find_profile_mut<'a>(state: &'a mut PowerProfileState, name: &str) -> KernelResult<&'a mut PowerProfile> {
    match name {
        "Balanced" => Ok(&mut state.balanced),
        "Performance" => Ok(&mut state.performance),
        "Power Saver" => Ok(&mut state.power_saver),
        _ => {
            state.custom_profiles.iter_mut()
                .find(|p| p.name == name)
                .ok_or(KernelError::NotFound)
        }
    }
}

// ---------------------------------------------------------------------------
// Auto-switch
// ---------------------------------------------------------------------------

/// Set auto-switch on battery.
pub fn set_auto_switch(enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        state.auto_switch_on_battery = enabled;
        Ok(())
    })
}

/// Set auto-switch battery threshold.
pub fn set_auto_switch_threshold(pct: u8) -> KernelResult<()> {
    if pct > 100 {
        return Err(KernelError::InvalidArgument);
    }
    with_state(|state| {
        state.auto_switch_threshold = pct;
        Ok(())
    })
}

/// Check and apply auto-switch based on battery state.
/// Returns the name of the profile switched to, or None if no switch.
pub fn check_auto_switch() -> Option<String> {
    let mut guard = STATE.lock();
    let state = guard.as_mut()?;
    if !state.auto_switch_on_battery {
        return None;
    }

    match state.battery.state {
        BatteryState::Discharging if state.battery.charge_pct <= state.auto_switch_threshold => {
            if state.active_profile != "Power Saver" {
                state.active_profile = String::from("Power Saver");
                state.switch_count += 1;
                return Some(String::from("Power Saver"));
            }
        }
        BatteryState::Charging | BatteryState::Full => {
            if state.active_profile == "Power Saver" {
                state.active_profile = String::from("Balanced");
                state.switch_count += 1;
                return Some(String::from("Balanced"));
            }
        }
        _ => {}
    }
    None
}

// ---------------------------------------------------------------------------
// Battery
// ---------------------------------------------------------------------------

/// Update battery status.
pub fn update_battery(state_val: BatteryState, charge_pct: u8, minutes_remaining: u32) -> KernelResult<()> {
    with_state(|s| {
        s.battery.state = state_val;
        s.battery.charge_pct = charge_pct;
        s.battery.minutes_remaining = minutes_remaining;
        Ok(())
    })
}

/// Get battery info.
pub fn battery_info() -> BatteryInfo {
    let guard = STATE.lock();
    guard.as_ref().map_or_else(
        || BatteryInfo {
            state: BatteryState::NoBattery,
            charge_pct: 0,
            minutes_remaining: 0,
            health_pct: 100,
            cycle_count: 0,
            design_capacity_mwh: 0,
            current_capacity_mwh: 0,
            charge_rate_mw: 0,
            discharge_rate_mw: 0,
        },
        |s| s.battery.clone(),
    )
}

// ---------------------------------------------------------------------------
// Queries
// ---------------------------------------------------------------------------

/// Check if animations should be disabled.
pub fn should_disable_animations() -> bool {
    let guard = STATE.lock();
    guard.as_ref().is_some_and(|s| {
        find_profile(s, &s.active_profile.clone())
            .is_some_and(|p| p.disable_animations)
    })
}

/// Check if background activity should be reduced.
pub fn should_reduce_background() -> bool {
    let guard = STATE.lock();
    guard.as_ref().is_some_and(|s| {
        find_profile(s, &s.active_profile.clone())
            .is_some_and(|p| p.reduce_background)
    })
}

/// Get active CPU governor.
pub fn active_cpu_governor() -> CpuGovernor {
    let guard = STATE.lock();
    guard.as_ref().map_or(CpuGovernor::Schedutil, |s| {
        find_profile(s, &s.active_profile.clone())
            .map_or(CpuGovernor::Schedutil, |p| p.cpu_governor)
    })
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Returns (profile_count, active_name, switch_count, battery_pct, battery_state, ops).
pub fn stats() -> (usize, String, u64, u8, &'static str, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (
            3 + s.custom_profiles.len(),
            s.active_profile.clone(),
            s.switch_count,
            s.battery.charge_pct,
            s.battery.state.label(),
            s.ops,
        ),
        None => (0, String::from("n/a"), 0, 0, "n/a", 0),
    }
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run self-tests for the power profiles module.
pub fn self_test() {
    use crate::serial_println;

    serial_println!("[powerprofile] Running self-tests...");

    // Reset state.
    *STATE.lock() = None;
    init_defaults();

    // Test 1: default state.
    {
        let profile = active_profile().unwrap();
        assert_eq!(profile.name, "Balanced");
        assert_eq!(profile.profile_type, ProfileType::Balanced);
        assert_eq!(profile.cpu_governor, CpuGovernor::Schedutil);
    }
    serial_println!("[powerprofile]  1/11 default state OK");

    // Test 2: switch profile.
    {
        set_profile("Performance").unwrap();
        let p = active_profile().unwrap();
        assert_eq!(p.name, "Performance");
        assert_eq!(p.cpu_governor, CpuGovernor::Performance);
        assert!(p.cpu_boost);
    }
    serial_println!("[powerprofile]  2/11 switch profile OK");

    // Test 3: power saver.
    {
        set_profile("Power Saver").unwrap();
        let p = active_profile().unwrap();
        assert!(p.reduce_background);
        assert!(p.disable_animations);
        assert!(!p.cpu_boost);
        assert_eq!(p.cpu_max_pct, 60);
    }
    serial_println!("[powerprofile]  3/11 power saver OK");

    // Test 4: invalid profile.
    {
        assert!(set_profile("NonExistent").is_err());
    }
    serial_println!("[powerprofile]  4/11 invalid profile OK");

    // Test 5: list profiles.
    {
        let profiles = list_profiles();
        assert_eq!(profiles.len(), 3);
    }
    serial_println!("[powerprofile]  5/11 list profiles OK");

    // Test 6: custom profile.
    {
        create_profile("Gaming").unwrap();
        let profiles = list_profiles();
        assert_eq!(profiles.len(), 4);
        set_profile("Gaming").unwrap();
        assert_eq!(active_profile_name(), "Gaming");
    }
    serial_println!("[powerprofile]  6/11 custom profile OK");

    // Test 7: modify profile settings.
    {
        set_cpu_governor(CpuGovernor::Performance).unwrap();
        set_cpu_boost(true).unwrap();
        set_brightness(90).unwrap();
        let p = active_profile().unwrap();
        assert_eq!(p.cpu_governor, CpuGovernor::Performance);
        assert!(p.cpu_boost);
        assert_eq!(p.brightness_pct, 90);
    }
    serial_println!("[powerprofile]  7/11 modify profile OK");

    // Test 8: remove custom profile.
    {
        set_profile("Balanced").unwrap();
        remove_profile("Gaming").unwrap();
        assert_eq!(list_profiles().len(), 3);
        // Cannot remove built-in.
        assert!(remove_profile("Balanced").is_err());
    }
    serial_println!("[powerprofile]  8/11 remove profile OK");

    // Test 9: battery update.
    {
        update_battery(BatteryState::Discharging, 50, 120).unwrap();
        let bi = battery_info();
        assert_eq!(bi.state, BatteryState::Discharging);
        assert_eq!(bi.charge_pct, 50);
        assert_eq!(bi.minutes_remaining, 120);
    }
    serial_println!("[powerprofile]  9/11 battery update OK");

    // Test 10: auto-switch.
    {
        set_auto_switch(true).unwrap();
        set_auto_switch_threshold(20).unwrap();
        update_battery(BatteryState::Discharging, 15, 30).unwrap();
        let switched = check_auto_switch();
        assert_eq!(switched.as_deref(), Some("Power Saver"));
        assert_eq!(active_profile_name(), "Power Saver");
        // Plugging in restores to Balanced.
        update_battery(BatteryState::Charging, 15, 0).unwrap();
        let switched = check_auto_switch();
        assert_eq!(switched.as_deref(), Some("Balanced"));
    }
    serial_println!("[powerprofile] 10/11 auto-switch OK");

    // Test 11: query helpers.
    {
        set_profile("Power Saver").unwrap();
        assert!(should_disable_animations());
        assert!(should_reduce_background());
        set_profile("Balanced").unwrap();
        assert!(!should_disable_animations());
    }
    serial_println!("[powerprofile] 11/11 query helpers OK");

    serial_println!("[powerprofile] All self-tests passed.");
}
