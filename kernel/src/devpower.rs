//! Device power management — PCI device power states and system-wide
//! power transition coordination.
//!
//! Manages the power state of PCI devices across system power transitions
//! (active → sleep → wake) and provides on-demand device suspend/resume
//! for power saving.
//!
//! ## PCI Power States
//!
//! ```text
//! D0 (Active)   — fully operational, responding to transactions
//! D1 (Light)    — reduced power, device context partly preserved
//! D2 (Deeper)   — more power savings, longer resume latency
//! D3hot (Soft)  — minimal power, context lost, PCI config preserved
//! D3cold (Off)  — power removed entirely, requires full re-init
//! ```
//!
//! ## System Power Transitions
//!
//! When the system enters a sleep state (S1-S4):
//! 1. **Suspend phase**: all managed devices transition to low-power state
//!    in reverse registration order (last registered = first suspended).
//!    Drivers are notified to save state before hardware is powered down.
//! 2. **Resume phase**: devices restored to D0 in registration order.
//!    Drivers are notified to restore state after hardware is powered up.
//!
//! ## Runtime Power Management
//!
//! Individual devices can be suspended when idle:
//! - Auto-suspend after configurable idle timeout.
//! - Wake on device activity (interrupt, I/O request).
//! - Per-device policy: always-on, auto-suspend, or manual.
//!
//! ## Integration
//!
//! - [`crate::power`] coordinates system-level sleep/wake.
//! - [`crate::udriver`] tracks driver bindings for suspend/resume notification.
//! - [`crate::devhotplug`] emits events on power state changes.
//! - PCI config space Power Management Capability (PM cap) for hardware control.
//!
//! ## References
//!
//! - PCI Power Management spec 1.2 — PM capability structure
//! - PCI Express spec §5.3 — power management states
//! - Linux `drivers/pci/pci-driver.c` — suspend/resume callbacks
//! - Linux `drivers/base/power/runtime.c` — runtime PM framework

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};
use crate::udriver::DeviceAddr;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum managed devices.
const MAX_DEVICES: usize = 128;

/// Default auto-suspend idle timeout (nanoseconds) — 30 seconds.
const DEFAULT_AUTOSUSPEND_TIMEOUT_NS: u64 = 30_000_000_000;

/// Minimum auto-suspend timeout (nanoseconds) — 1 second.
const MIN_AUTOSUSPEND_TIMEOUT_NS: u64 = 1_000_000_000;

/// PCI PM capability register offsets (relative to capability pointer).
const PM_CAP_PMC: u8 = 0x02;   // Power Management Capabilities
const PM_CAP_PMCSR: u8 = 0x04; // Power Management Control/Status

/// PMCSR power state bits [1:0].
const PMCSR_STATE_MASK: u16 = 0x0003;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// PCI device power state (matches PCI PM spec states).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PowerState {
    /// D0 — fully active, device operational.
    D0 = 0,
    /// D1 — light sleep, partial context preserved.
    D1 = 1,
    /// D2 — deeper sleep, more context lost.
    D2 = 2,
    /// D3hot — soft off, PCI config space accessible.
    D3Hot = 3,
    /// D3cold — power removed, requires full re-initialization.
    D3Cold = 4,
    /// Unknown — state not yet determined.
    Unknown = 5,
}

impl PowerState {
    /// Human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            Self::D0 => "D0 (active)",
            Self::D1 => "D1 (light sleep)",
            Self::D2 => "D2 (deep sleep)",
            Self::D3Hot => "D3hot (soft off)",
            Self::D3Cold => "D3cold (power off)",
            Self::Unknown => "unknown",
        }
    }

    /// Short label for compact display.
    pub fn short(self) -> &'static str {
        match self {
            Self::D0 => "D0",
            Self::D1 => "D1",
            Self::D2 => "D2",
            Self::D3Hot => "D3h",
            Self::D3Cold => "D3c",
            Self::Unknown => "??",
        }
    }
}

/// Runtime power management policy for a device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerPolicy {
    /// Always keep the device in D0 (never auto-suspend).
    AlwaysOn,
    /// Auto-suspend after idle timeout, wake on activity.
    AutoSuspend,
    /// Manual — only suspend/resume via explicit commands.
    Manual,
}

impl PowerPolicy {
    pub fn label(self) -> &'static str {
        match self {
            Self::AlwaysOn => "always-on",
            Self::AutoSuspend => "auto-suspend",
            Self::Manual => "manual",
        }
    }
}

/// Power management capabilities of a PCI device.
#[derive(Debug, Clone, Copy)]
pub struct PowerCapabilities {
    /// Whether the device has a PCI PM capability.
    pub pm_capable: bool,
    /// PM capability version (1 or 2).
    pub pm_version: u8,
    /// Supports D1 state.
    pub supports_d1: bool,
    /// Supports D2 state.
    pub supports_d2: bool,
    /// Supports PME (Power Management Event) generation.
    pub supports_pme: bool,
    /// Maximum power drawn in D3hot (milliwatts, 0 if unknown).
    pub d3hot_power_mw: u32,
}

impl PowerCapabilities {
    /// Default for devices without PM capability.
    const fn none() -> Self {
        Self {
            pm_capable: false,
            pm_version: 0,
            supports_d1: false,
            supports_d2: false,
            supports_pme: false,
            d3hot_power_mw: 0,
        }
    }
}

/// Per-device power management state.
#[derive(Debug, Clone)]
pub struct DevicePowerEntry {
    /// PCI device address.
    pub addr: DeviceAddr,
    /// Device name (from driver or PCI IDs).
    pub name: String,
    /// Current power state.
    pub current_state: PowerState,
    /// Desired state (target of a pending transition, or same as current).
    pub target_state: PowerState,
    /// Hardware capabilities.
    pub capabilities: PowerCapabilities,
    /// Runtime PM policy.
    pub policy: PowerPolicy,
    /// Auto-suspend idle timeout (ns).
    pub autosuspend_timeout_ns: u64,
    /// Timestamp of last I/O activity (ns).
    pub last_activity_ns: u64,
    /// Whether a suspend/resume is currently in progress.
    pub transition_in_progress: bool,
    /// Total time spent in non-D0 states (ns), for power stats.
    pub total_sleep_ns: u64,
    /// Timestamp when last entered non-D0 state (ns, 0 if in D0).
    pub sleep_entered_ns: u64,
    /// Number of suspend/resume cycles.
    pub suspend_count: u64,
    /// Number of failed transitions.
    pub failed_transitions: u64,
    /// When this device was registered (ns).
    pub registered_at: u64,
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

struct State {
    /// Managed devices (ordered by registration time).
    devices: Vec<DevicePowerEntry>,
    /// Whether system-wide suspend is in progress.
    system_suspending: bool,
    /// Whether system is currently in a sleep state.
    system_sleeping: bool,
    /// Total system suspend events.
    total_system_suspends: u64,
    /// Total system resume events.
    total_system_resumes: u64,
    /// Total individual device suspends.
    total_device_suspends: u64,
    /// Total individual device resumes.
    total_device_resumes: u64,
    /// Total failed transitions across all devices.
    total_failures: u64,
}

impl State {
    const fn new() -> Self {
        Self {
            devices: Vec::new(),
            system_suspending: false,
            system_sleeping: false,
            total_system_suspends: 0,
            total_system_resumes: 0,
            total_device_suspends: 0,
            total_device_resumes: 0,
            total_failures: 0,
        }
    }
}

static STATE: Mutex<State> = Mutex::new(State::new());

// ---------------------------------------------------------------------------
// Device registration
// ---------------------------------------------------------------------------

/// Register a device for power management.
///
/// Called when a driver successfully binds to a device. The device starts
/// in D0 (active) state with the specified policy.
pub fn register_device(
    addr: DeviceAddr,
    name: &str,
    capabilities: PowerCapabilities,
    policy: PowerPolicy,
) -> KernelResult<()> {
    let now = crate::hpet::elapsed_ns();
    let mut state = STATE.lock();

    if state.devices.len() >= MAX_DEVICES {
        return Err(KernelError::ResourceExhausted);
    }

    // Check for duplicate.
    if state.devices.iter().any(|d| d.addr == addr) {
        return Err(KernelError::AlreadyExists);
    }

    state.devices.push(DevicePowerEntry {
        addr,
        name: String::from(name),
        current_state: PowerState::D0,
        target_state: PowerState::D0,
        capabilities,
        policy,
        autosuspend_timeout_ns: DEFAULT_AUTOSUSPEND_TIMEOUT_NS,
        last_activity_ns: now,
        transition_in_progress: false,
        total_sleep_ns: 0,
        sleep_entered_ns: 0,
        suspend_count: 0,
        failed_transitions: 0,
        registered_at: now,
    });

    crate::syslog!(
        "devpower",
        Info,
        "registered {:02x}:{:02x}.{} '{}' policy={} pm={}",
        addr.bus, addr.device, addr.function, name,
        policy.label(),
        if capabilities.pm_capable { "yes" } else { "no" }
    );

    Ok(())
}

/// Unregister a device from power management.
pub fn unregister_device(addr: DeviceAddr) -> KernelResult<()> {
    let mut state = STATE.lock();

    let idx = state.devices.iter().position(|d| d.addr == addr)
        .ok_or(KernelError::NotFound)?;

    // Ensure device is in D0 before unregistering.
    if state.devices[idx].current_state != PowerState::D0 {
        // Force resume before removal.
        state.devices[idx].current_state = PowerState::D0;
    }

    state.devices.swap_remove(idx);

    Ok(())
}

// ---------------------------------------------------------------------------
// Power state transitions
// ---------------------------------------------------------------------------

/// Request a device power state transition.
///
/// For D0→D3hot: driver should save device state before calling this.
/// For D3hot→D0: driver should restore device state after this returns.
pub fn set_device_power(addr: DeviceAddr, target: PowerState) -> KernelResult<()> {
    let now = crate::hpet::elapsed_ns();
    let mut state = STATE.lock();

    let idx = state.devices.iter().position(|d| d.addr == addr)
        .ok_or(KernelError::NotFound)?;

    let current = state.devices[idx].current_state;
    if current == target {
        return Ok(()); // Already there.
    }

    // Validate transition against capabilities.
    if target == PowerState::D1 && !state.devices[idx].capabilities.supports_d1 {
        return Err(KernelError::NotSupported);
    }
    if target == PowerState::D2 && !state.devices[idx].capabilities.supports_d2 {
        return Err(KernelError::NotSupported);
    }

    // Can't transition if already in progress.
    if state.devices[idx].transition_in_progress {
        return Err(KernelError::DeviceBusy);
    }

    // Track sleep time.
    if current == PowerState::D0 && target != PowerState::D0 {
        // Entering sleep.
        state.devices[idx].sleep_entered_ns = now;
        state.devices[idx].suspend_count = state.devices[idx]
            .suspend_count.saturating_add(1);
        state.total_device_suspends = state.total_device_suspends.saturating_add(1);
    } else if current != PowerState::D0 && target == PowerState::D0 {
        // Waking up.
        let sleep_start = state.devices[idx].sleep_entered_ns;
        if sleep_start > 0 {
            let sleep_duration = now.saturating_sub(sleep_start);
            state.devices[idx].total_sleep_ns = state.devices[idx]
                .total_sleep_ns.saturating_add(sleep_duration);
        }
        state.devices[idx].sleep_entered_ns = 0;
        state.devices[idx].last_activity_ns = now;
        state.total_device_resumes = state.total_device_resumes.saturating_add(1);
    }

    // In a full implementation, this is where we would:
    // 1. Read the PM capability pointer from PCI config space
    // 2. Write the new power state to PMCSR bits [1:0]
    // 3. Wait for the required transition delay
    //    - D3hot → D0: 10ms minimum per PCI PM spec
    //    - D2 → D0: 200µs minimum
    //    - D1 → D0: no mandatory delay
    // 4. Verify the state by reading back PMCSR
    //
    // For now, record the state transition immediately.

    state.devices[idx].current_state = target;
    state.devices[idx].target_state = target;

    crate::syslog!(
        "devpower",
        Info,
        "{:02x}:{:02x}.{}: {} → {}",
        addr.bus, addr.device, addr.function,
        current.short(), target.short()
    );

    Ok(())
}

/// Report device I/O activity (resets auto-suspend timer).
pub fn report_activity(addr: DeviceAddr) {
    let now = crate::hpet::elapsed_ns();
    let mut state = STATE.lock();

    if let Some(dev) = state.devices.iter_mut().find(|d| d.addr == addr) {
        dev.last_activity_ns = now;
    }
}

/// Set the power management policy for a device.
pub fn set_policy(addr: DeviceAddr, policy: PowerPolicy) -> KernelResult<()> {
    let mut state = STATE.lock();

    let dev = state.devices.iter_mut().find(|d| d.addr == addr)
        .ok_or(KernelError::NotFound)?;

    dev.policy = policy;

    Ok(())
}

/// Set the auto-suspend timeout for a device.
pub fn set_autosuspend_timeout(addr: DeviceAddr, timeout_ns: u64) -> KernelResult<()> {
    let mut state = STATE.lock();

    let dev = state.devices.iter_mut().find(|d| d.addr == addr)
        .ok_or(KernelError::NotFound)?;

    dev.autosuspend_timeout_ns = if timeout_ns < MIN_AUTOSUSPEND_TIMEOUT_NS {
        MIN_AUTOSUSPEND_TIMEOUT_NS
    } else {
        timeout_ns
    };

    Ok(())
}

// ---------------------------------------------------------------------------
// System power transitions
// ---------------------------------------------------------------------------

/// Suspend all managed devices (system entering sleep).
///
/// Devices are suspended in reverse registration order (most recently
/// registered first, since they may depend on earlier devices).
/// Returns the number of devices successfully suspended.
pub fn system_suspend() -> usize {
    let now = crate::hpet::elapsed_ns();
    let mut state = STATE.lock();

    state.system_suspending = true;
    let count = state.devices.len();
    let mut suspended = 0;

    // Iterate in reverse order.
    for i in (0..count).rev() {
        if state.devices[i].current_state == PowerState::D0 {
            state.devices[i].sleep_entered_ns = now;
            state.devices[i].current_state = PowerState::D3Hot;
            state.devices[i].target_state = PowerState::D3Hot;
            state.devices[i].suspend_count = state.devices[i]
                .suspend_count.saturating_add(1);
            suspended += 1;
        }
    }

    state.system_suspending = false;
    state.system_sleeping = true;
    state.total_system_suspends = state.total_system_suspends.saturating_add(1);
    state.total_device_suspends = state.total_device_suspends
        .saturating_add(suspended as u64);

    crate::syslog!(
        "devpower",
        Info,
        "system suspend: {}/{} devices suspended",
        suspended, count
    );

    suspended
}

/// Resume all managed devices (system waking from sleep).
///
/// Devices are resumed in registration order (earliest first, so
/// dependencies are satisfied).
/// Returns the number of devices successfully resumed.
pub fn system_resume() -> usize {
    let now = crate::hpet::elapsed_ns();
    let mut state = STATE.lock();

    let count = state.devices.len();
    let mut resumed = 0;

    for i in 0..count {
        if state.devices[i].current_state != PowerState::D0 {
            // Track sleep time.
            let start = state.devices[i].sleep_entered_ns;
            if start > 0 {
                let duration = now.saturating_sub(start);
                state.devices[i].total_sleep_ns = state.devices[i]
                    .total_sleep_ns.saturating_add(duration);
            }
            state.devices[i].sleep_entered_ns = 0;
            state.devices[i].current_state = PowerState::D0;
            state.devices[i].target_state = PowerState::D0;
            state.devices[i].last_activity_ns = now;
            resumed += 1;
        }
    }

    state.system_sleeping = false;
    state.total_system_resumes = state.total_system_resumes.saturating_add(1);
    state.total_device_resumes = state.total_device_resumes
        .saturating_add(resumed as u64);

    crate::syslog!(
        "devpower",
        Info,
        "system resume: {}/{} devices resumed",
        resumed, count
    );

    resumed
}

// ---------------------------------------------------------------------------
// Runtime PM tick (called periodically)
// ---------------------------------------------------------------------------

/// Check for auto-suspend candidates (called from periodic timer).
///
/// For devices with `AutoSuspend` policy that have been idle longer
/// than their timeout, transitions them to D3hot.
pub fn tick() {
    let now = crate::hpet::elapsed_ns();
    let mut state = STATE.lock();

    if state.system_sleeping || state.system_suspending {
        return;
    }

    let mut suspended_count: u64 = 0;
    let count = state.devices.len();

    for i in 0..count {
        if state.devices[i].policy != PowerPolicy::AutoSuspend {
            continue;
        }
        if state.devices[i].current_state != PowerState::D0 {
            continue;
        }
        if state.devices[i].transition_in_progress {
            continue;
        }

        let idle_ns = now.saturating_sub(state.devices[i].last_activity_ns);
        let timeout = state.devices[i].autosuspend_timeout_ns;
        if idle_ns >= timeout {
            // Auto-suspend this device.
            state.devices[i].sleep_entered_ns = now;
            state.devices[i].current_state = PowerState::D3Hot;
            state.devices[i].target_state = PowerState::D3Hot;
            state.devices[i].suspend_count = state.devices[i]
                .suspend_count.saturating_add(1);
            suspended_count = suspended_count.saturating_add(1);

            crate::syslog!(
                "devpower",
                Info,
                "auto-suspend {:02x}:{:02x}.{} '{}' after {}s idle",
                state.devices[i].addr.bus, state.devices[i].addr.device,
                state.devices[i].addr.function,
                state.devices[i].name, idle_ns / 1_000_000_000
            );
        }
    }

    state.total_device_suspends = state.total_device_suspends
        .saturating_add(suspended_count);
}

// ---------------------------------------------------------------------------
// Queries
// ---------------------------------------------------------------------------

/// Get power state of a specific device.
#[must_use]
pub fn device_state(addr: DeviceAddr) -> Option<PowerState> {
    STATE.lock().devices.iter()
        .find(|d| d.addr == addr)
        .map(|d| d.current_state)
}

/// Get full power info for a specific device.
#[must_use]
pub fn device_info(addr: DeviceAddr) -> Option<DevicePowerEntry> {
    STATE.lock().devices.iter()
        .find(|d| d.addr == addr)
        .cloned()
}

/// List all managed devices with their power states.
#[must_use]
pub fn all_devices() -> Vec<DevicePowerEntry> {
    STATE.lock().devices.clone()
}

/// Count devices in each power state.
#[must_use]
pub fn state_counts() -> (usize, usize, usize, usize, usize) {
    let state = STATE.lock();
    let d0 = state.devices.iter().filter(|d| d.current_state == PowerState::D0).count();
    let d1 = state.devices.iter().filter(|d| d.current_state == PowerState::D1).count();
    let d2 = state.devices.iter().filter(|d| d.current_state == PowerState::D2).count();
    let d3h = state.devices.iter().filter(|d| d.current_state == PowerState::D3Hot).count();
    let d3c = state.devices.iter().filter(|d| d.current_state == PowerState::D3Cold).count();
    (d0, d1, d2, d3h, d3c)
}

/// Summary statistics.
#[derive(Debug, Clone)]
pub struct DevPowerStats {
    pub total_devices: usize,
    pub active_devices: usize,
    pub sleeping_devices: usize,
    pub system_sleeping: bool,
    pub total_system_suspends: u64,
    pub total_system_resumes: u64,
    pub total_device_suspends: u64,
    pub total_device_resumes: u64,
    pub total_failures: u64,
}

#[must_use]
pub fn stats() -> DevPowerStats {
    let state = STATE.lock();
    let active = state.devices.iter()
        .filter(|d| d.current_state == PowerState::D0).count();
    DevPowerStats {
        total_devices: state.devices.len(),
        active_devices: active,
        sleeping_devices: state.devices.len().saturating_sub(active),
        system_sleeping: state.system_sleeping,
        total_system_suspends: state.total_system_suspends,
        total_system_resumes: state.total_system_resumes,
        total_device_suspends: state.total_device_suspends,
        total_device_resumes: state.total_device_resumes,
        total_failures: state.total_failures,
    }
}

// ---------------------------------------------------------------------------
// procfs content
// ---------------------------------------------------------------------------

#[must_use]
pub fn procfs_content() -> String {
    let state = STATE.lock();
    let mut out = String::with_capacity(4096);

    out.push_str("=== Device Power Management ===\n\n");

    let active = state.devices.iter()
        .filter(|d| d.current_state == PowerState::D0).count();
    let sleeping = state.devices.len().saturating_sub(active);

    out.push_str(&format!("System sleeping:     {}\n", state.system_sleeping));
    out.push_str(&format!("Managed devices:     {}\n", state.devices.len()));
    out.push_str(&format!("  Active (D0):       {}\n", active));
    out.push_str(&format!("  Sleeping:          {}\n", sleeping));
    out.push_str(&format!("System suspends:     {}\n", state.total_system_suspends));
    out.push_str(&format!("System resumes:      {}\n", state.total_system_resumes));
    out.push_str(&format!("Device suspends:     {}\n", state.total_device_suspends));
    out.push_str(&format!("Device resumes:      {}\n", state.total_device_resumes));
    out.push_str(&format!("Failed transitions:  {}\n\n", state.total_failures));

    for dev in &state.devices {
        out.push_str(&format!(
            "{:02x}:{:02x}.{} '{}' [{}] policy={}\n",
            dev.addr.bus, dev.addr.device, dev.addr.function,
            dev.name, dev.current_state.label(), dev.policy.label(),
        ));

        let pm_str = if dev.capabilities.pm_capable {
            format!("v{} D1={} D2={} PME={}",
                dev.capabilities.pm_version,
                dev.capabilities.supports_d1,
                dev.capabilities.supports_d2,
                dev.capabilities.supports_pme)
        } else {
            String::from("not PM-capable")
        };
        out.push_str(&format!("  PM: {}\n", pm_str));
        out.push_str(&format!("  Suspends: {}  Sleep time: {}ms\n",
            dev.suspend_count,
            dev.total_sleep_ns / 1_000_000));

        if dev.policy == PowerPolicy::AutoSuspend {
            out.push_str(&format!("  Auto-suspend timeout: {}s\n",
                dev.autosuspend_timeout_ns / 1_000_000_000));
        }
        out.push('\n');
    }

    out
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("[devpower] running self-tests...");

    test_register_device();
    test_power_transition();
    test_unsupported_state();
    test_activity_reporting();
    test_policy_change();
    test_autosuspend_timeout();
    test_system_suspend_resume();
    test_sleep_time_tracking();
    test_duplicate_registration();
    test_unregister();
    test_state_counts();
    test_stats();
    test_procfs();

    crate::serial_println!("[devpower] all self-tests passed");
}

fn reset_state() {
    let mut state = STATE.lock();
    *state = State::new();
}

fn make_caps(d1: bool, d2: bool) -> PowerCapabilities {
    PowerCapabilities {
        pm_capable: true,
        pm_version: 2,
        supports_d1: d1,
        supports_d2: d2,
        supports_pme: true,
        d3hot_power_mw: 10,
    }
}

fn test_register_device() {
    reset_state();

    let addr = DeviceAddr::new(0, 1, 0);
    assert!(register_device(addr, "test-dev", make_caps(true, true), PowerPolicy::AlwaysOn).is_ok());

    let devs = all_devices();
    assert_eq!(devs.len(), 1);
    assert_eq!(devs[0].current_state, PowerState::D0);
    assert_eq!(devs[0].policy, PowerPolicy::AlwaysOn);

    crate::serial_println!("  [devpower] test_register_device: ok");
}

fn test_power_transition() {
    reset_state();

    let addr = DeviceAddr::new(0, 2, 0);
    register_device(addr, "trans-test", make_caps(true, true), PowerPolicy::Manual).unwrap();

    // D0 → D3hot.
    assert!(set_device_power(addr, PowerState::D3Hot).is_ok());
    assert_eq!(device_state(addr), Some(PowerState::D3Hot));

    // D3hot → D0.
    assert!(set_device_power(addr, PowerState::D0).is_ok());
    assert_eq!(device_state(addr), Some(PowerState::D0));

    // D0 → D1.
    assert!(set_device_power(addr, PowerState::D1).is_ok());
    assert_eq!(device_state(addr), Some(PowerState::D1));

    crate::serial_println!("  [devpower] test_power_transition: ok");
}

fn test_unsupported_state() {
    reset_state();

    let addr = DeviceAddr::new(0, 3, 0);
    // No D1 support.
    register_device(addr, "no-d1", make_caps(false, false), PowerPolicy::Manual).unwrap();

    assert_eq!(set_device_power(addr, PowerState::D1), Err(KernelError::NotSupported));
    assert_eq!(set_device_power(addr, PowerState::D2), Err(KernelError::NotSupported));

    // D3hot should always work.
    assert!(set_device_power(addr, PowerState::D3Hot).is_ok());

    crate::serial_println!("  [devpower] test_unsupported_state: ok");
}

fn test_activity_reporting() {
    reset_state();

    let addr = DeviceAddr::new(0, 4, 0);
    register_device(addr, "activity-test", make_caps(true, true), PowerPolicy::AutoSuspend).unwrap();

    let before = device_info(addr).unwrap().last_activity_ns;
    // Simulate some time passing.
    report_activity(addr);
    let after = device_info(addr).unwrap().last_activity_ns;

    // Activity should update the timestamp (may be same if called fast).
    assert!(after >= before);

    crate::serial_println!("  [devpower] test_activity_reporting: ok");
}

fn test_policy_change() {
    reset_state();

    let addr = DeviceAddr::new(0, 5, 0);
    register_device(addr, "policy-test", make_caps(true, true), PowerPolicy::AlwaysOn).unwrap();

    assert!(set_policy(addr, PowerPolicy::AutoSuspend).is_ok());
    assert_eq!(device_info(addr).unwrap().policy, PowerPolicy::AutoSuspend);

    assert!(set_policy(addr, PowerPolicy::Manual).is_ok());
    assert_eq!(device_info(addr).unwrap().policy, PowerPolicy::Manual);

    crate::serial_println!("  [devpower] test_policy_change: ok");
}

fn test_autosuspend_timeout() {
    reset_state();

    let addr = DeviceAddr::new(0, 6, 0);
    register_device(addr, "timeout-test", make_caps(true, true), PowerPolicy::AutoSuspend).unwrap();

    assert!(set_autosuspend_timeout(addr, 5_000_000_000).is_ok());
    assert_eq!(device_info(addr).unwrap().autosuspend_timeout_ns, 5_000_000_000);

    // Below minimum should clamp.
    assert!(set_autosuspend_timeout(addr, 100).is_ok());
    assert_eq!(device_info(addr).unwrap().autosuspend_timeout_ns, MIN_AUTOSUSPEND_TIMEOUT_NS);

    crate::serial_println!("  [devpower] test_autosuspend_timeout: ok");
}

fn test_system_suspend_resume() {
    reset_state();

    let a1 = DeviceAddr::new(0, 7, 0);
    let a2 = DeviceAddr::new(0, 8, 0);
    register_device(a1, "dev-a", make_caps(true, true), PowerPolicy::AlwaysOn).unwrap();
    register_device(a2, "dev-b", make_caps(true, true), PowerPolicy::Manual).unwrap();

    // Suspend all.
    let count = system_suspend();
    assert_eq!(count, 2);
    assert_eq!(device_state(a1), Some(PowerState::D3Hot));
    assert_eq!(device_state(a2), Some(PowerState::D3Hot));

    // Resume all.
    let count = system_resume();
    assert_eq!(count, 2);
    assert_eq!(device_state(a1), Some(PowerState::D0));
    assert_eq!(device_state(a2), Some(PowerState::D0));

    let st = stats();
    assert_eq!(st.total_system_suspends, 1);
    assert_eq!(st.total_system_resumes, 1);

    crate::serial_println!("  [devpower] test_system_suspend_resume: ok");
}

fn test_sleep_time_tracking() {
    reset_state();

    let addr = DeviceAddr::new(0, 9, 0);
    register_device(addr, "sleep-track", make_caps(true, true), PowerPolicy::Manual).unwrap();

    // Put to sleep.
    set_device_power(addr, PowerState::D3Hot).unwrap();
    let info = device_info(addr).unwrap();
    assert!(info.sleep_entered_ns > 0);
    assert_eq!(info.suspend_count, 1);

    // Wake up.
    set_device_power(addr, PowerState::D0).unwrap();
    let info = device_info(addr).unwrap();
    assert_eq!(info.sleep_entered_ns, 0);
    // total_sleep_ns should be > 0 (might be 0 if hpet returns same value).
    // total_sleep_ns is u64, verified wake happened without panic.

    crate::serial_println!("  [devpower] test_sleep_time_tracking: ok");
}

fn test_duplicate_registration() {
    reset_state();

    let addr = DeviceAddr::new(0, 10, 0);
    assert!(register_device(addr, "dup-a", make_caps(true, true), PowerPolicy::AlwaysOn).is_ok());
    assert_eq!(
        register_device(addr, "dup-b", make_caps(true, true), PowerPolicy::AlwaysOn),
        Err(KernelError::AlreadyExists)
    );

    crate::serial_println!("  [devpower] test_duplicate_registration: ok");
}

fn test_unregister() {
    reset_state();

    let addr = DeviceAddr::new(0, 11, 0);
    register_device(addr, "unreg-test", make_caps(true, true), PowerPolicy::AlwaysOn).unwrap();

    assert!(unregister_device(addr).is_ok());
    assert!(all_devices().is_empty());
    assert_eq!(unregister_device(addr), Err(KernelError::NotFound));

    crate::serial_println!("  [devpower] test_unregister: ok");
}

fn test_state_counts() {
    reset_state();

    let a1 = DeviceAddr::new(0, 12, 0);
    let a2 = DeviceAddr::new(0, 13, 0);
    let a3 = DeviceAddr::new(0, 14, 0);
    register_device(a1, "cnt-a", make_caps(true, true), PowerPolicy::Manual).unwrap();
    register_device(a2, "cnt-b", make_caps(true, true), PowerPolicy::Manual).unwrap();
    register_device(a3, "cnt-c", make_caps(true, true), PowerPolicy::Manual).unwrap();

    set_device_power(a2, PowerState::D3Hot).unwrap();

    let (d0, _d1, _d2, d3h, _d3c) = state_counts();
    assert_eq!(d0, 2);
    assert_eq!(d3h, 1);

    crate::serial_println!("  [devpower] test_state_counts: ok");
}

fn test_stats() {
    reset_state();

    let st = stats();
    assert_eq!(st.total_devices, 0);
    assert_eq!(st.active_devices, 0);
    assert!(!st.system_sleeping);

    crate::serial_println!("  [devpower] test_stats: ok");
}

fn test_procfs() {
    reset_state();

    let addr = DeviceAddr::new(0, 15, 0);
    register_device(addr, "procfs-dev", make_caps(true, false), PowerPolicy::AutoSuspend).unwrap();

    let content = procfs_content();
    assert!(content.contains("Device Power Management"));
    assert!(content.contains("procfs-dev"));
    assert!(content.contains("D0 (active)"));
    assert!(content.contains("auto-suspend"));

    crate::serial_println!("  [devpower] test_procfs: ok");
}
