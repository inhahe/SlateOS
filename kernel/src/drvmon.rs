//! Driver monitor — watchdog, crash detection, and automatic restart for drivers.
//!
//! Monitors registered drivers for health, detects crashes (process exit,
//! missed heartbeats), and coordinates automatic restart with I/O draining.
//!
//! ## Architecture
//!
//! ```text
//! Driver registration
//!   → drvmon::register("nvme", DriverPolicy { ... })
//!
//! Monitoring loop (periodic, ~1 Hz)
//!   → for each registered driver:
//!       if heartbeat_expected && heartbeat_overdue:
//!           mark_unhealthy → restart
//!       if process_exited:
//!           crash_detected → restart
//!
//! Restart sequence
//!   1. Emit driver.crash event
//!   2. Drain pending I/O (notify block/net layers to pause)
//!   3. Reset hardware (driver-specific reset callback)
//!   4. Reinitialize driver (call driver init function)
//!   5. Emit driver.restart event
//!   6. Resume I/O
//! ```
//!
//! ## Design references
//!
//! - Windows driver verifier + WDF device recovery
//! - Linux device error recovery (pci_error_handlers)
//! - Fuchsia driver host crash restart
//!
//! Microkernel advantage: drivers run in userspace, so a driver crash
//! doesn't take down the kernel.  This module manages the recovery.

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// How aggressively to restart a driver on failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestartPolicy {
    /// Restart immediately on crash.
    Immediate,
    /// Restart with a fixed delay (nanoseconds).
    Delayed(u64),
    /// Restart with exponential backoff (initial_ns, max_ns).
    ExponentialBackoff,
    /// Never restart — mark as failed and require manual intervention.
    Never,
}

/// Health check mode for a driver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthMode {
    /// No health checking — only react to process exit.
    None,
    /// Expect periodic heartbeat pings; declare dead if overdue.
    Heartbeat,
    /// Check if the driver process is still alive.
    ProcessAlive,
}

/// Current health state of a monitored driver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriverHealth {
    /// Driver is running normally.
    Healthy,
    /// Driver missed one heartbeat — watching closely.
    Degraded,
    /// Driver is unresponsive (multiple missed heartbeats or process gone).
    Unresponsive,
    /// Driver crashed and is being restarted.
    Restarting,
    /// Driver permanently failed (exceeded max restarts).
    Failed,
    /// Driver is stopped (not running, not monitored).
    Stopped,
}

impl DriverHealth {
    /// Human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Healthy => "Healthy",
            Self::Degraded => "Degraded",
            Self::Unresponsive => "Unresponsive",
            Self::Restarting => "Restarting",
            Self::Failed => "FAILED",
            Self::Stopped => "Stopped",
        }
    }
}

/// Bus type where the driver's device lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BusType {
    Pci,
    Usb,
    Platform,
    Virtual,
}

impl BusType {
    /// Human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Pci => "PCI",
            Self::Usb => "USB",
            Self::Platform => "Platform",
            Self::Virtual => "Virtual",
        }
    }
}

/// Driver monitoring configuration.
#[derive(Clone)]
pub struct DriverPolicy {
    /// Restart behaviour on crash.
    pub restart_policy: RestartPolicy,
    /// Health check mode.
    pub health_mode: HealthMode,
    /// Maximum heartbeat interval before declaring unhealthy (ns).
    /// Default: 5 seconds.
    pub heartbeat_timeout_ns: u64,
    /// How many missed heartbeats before declaring unresponsive (triggers restart).
    /// Default: 3.
    pub max_missed_heartbeats: u32,
    /// Maximum restart attempts before permanent failure.
    pub max_restarts: u32,
    /// Initial backoff delay for ExponentialBackoff (ns).
    pub initial_backoff_ns: u64,
    /// Maximum backoff delay (ns).
    pub max_backoff_ns: u64,
    /// Whether to drain I/O before restart.
    pub drain_io_on_restart: bool,
}

impl Default for DriverPolicy {
    fn default() -> Self {
        Self {
            restart_policy: RestartPolicy::ExponentialBackoff,
            health_mode: HealthMode::ProcessAlive,
            heartbeat_timeout_ns: 5_000_000_000,    // 5 seconds
            max_missed_heartbeats: 3,
            max_restarts: 5,
            initial_backoff_ns: 1_000_000_000,       // 1 second
            max_backoff_ns: 30_000_000_000,          // 30 seconds
            drain_io_on_restart: true,
        }
    }
}

/// A monitored driver entry.
struct DriverEntry {
    /// Unique monitoring ID.
    id: u32,
    /// Driver short name (e.g., "nvme", "e1000", "ahci").
    name: String,
    /// Human-readable display name.
    display_name: String,
    /// Bus type.
    bus: BusType,
    /// Process ID of the driver (0 if in-kernel or not running).
    pid: u32,
    /// Monitoring policy.
    policy: DriverPolicy,
    /// Current health state.
    health: DriverHealth,
    /// Timestamp of last heartbeat received (ns since boot).
    last_heartbeat_ns: u64,
    /// Number of consecutive missed heartbeats.
    missed_heartbeats: u32,
    /// Total crash count.
    total_crashes: u64,
    /// Consecutive crash count (resets on successful long run).
    consecutive_crashes: u32,
    /// Current backoff delay for ExponentialBackoff (ns).
    current_backoff_ns: u64,
    /// Timestamp of last crash (ns since boot).
    last_crash_ns: u64,
    /// Timestamp of last successful restart (ns since boot).
    last_restart_ns: u64,
    /// Total successful restarts.
    total_restarts: u64,
    /// Whether monitoring is active.
    monitoring: bool,
    /// Registered timestamp (ns since boot).
    registered_ns: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_DRIVERS: usize = 64;

struct State {
    drivers: Vec<DriverEntry>,
    next_id: u32,
    /// Number of monitor ticks performed.
    tick_count: u64,
    /// Total crashes across all drivers.
    total_crashes: u64,
    /// Total restarts across all drivers.
    total_restarts: u64,
    /// Timestamp of last monitor tick.
    last_tick_ns: u64,
    initialized: bool,
}

impl State {
    const fn new() -> Self {
        Self {
            drivers: Vec::new(),
            next_id: 1,
            tick_count: 0,
            total_crashes: 0,
            total_restarts: 0,
            last_tick_ns: 0,
            initialized: false,
        }
    }
}

static STATE: Mutex<State> = Mutex::new(State::new());

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Initialize the driver monitor.
pub fn init() {
    let mut state = STATE.lock();
    if state.initialized {
        return;
    }
    state.initialized = true;

    crate::syslog!("driver.monitor", Info, "Driver monitor initialized");
}

/// Register a driver for monitoring.
///
/// Returns the monitoring ID.
pub fn register(
    name: &str,
    display_name: &str,
    bus: BusType,
    pid: u32,
    policy: DriverPolicy,
) -> KernelResult<u32> {
    let mut state = STATE.lock();
    if !state.initialized {
        init_inner(&mut state);
    }

    if state.drivers.len() >= MAX_DRIVERS {
        return Err(KernelError::ResourceExhausted);
    }

    // Reject duplicate names.
    if state.drivers.iter().any(|d| d.name == name) {
        return Err(KernelError::AlreadyExists);
    }

    let id = state.next_id;
    state.next_id = state.next_id.saturating_add(1);
    let now = crate::hpet::elapsed_ns();

    state.drivers.push(DriverEntry {
        id,
        name: String::from(name),
        display_name: String::from(display_name),
        bus,
        pid,
        policy,
        health: if pid > 0 { DriverHealth::Healthy } else { DriverHealth::Stopped },
        last_heartbeat_ns: now,
        missed_heartbeats: 0,
        total_crashes: 0,
        consecutive_crashes: 0,
        current_backoff_ns: 0,
        last_crash_ns: 0,
        last_restart_ns: 0,
        total_restarts: 0,
        monitoring: true,
        registered_ns: now,
    });

    crate::syslog!("driver.monitor", Info,
        "Driver '{}' registered for monitoring (id={}, bus={})",
        name, id, bus.label());

    Ok(id)
}

fn init_inner(state: &mut State) {
    state.initialized = true;
}

/// Unregister a driver from monitoring.
pub fn unregister(id: u32) -> KernelResult<()> {
    let mut state = STATE.lock();
    let idx = state.drivers.iter().position(|d| d.id == id)
        .ok_or(KernelError::NotFound)?;
    let drv = state.drivers.remove(idx);

    crate::syslog!("driver.monitor", Info,
        "Driver '{}' unregistered from monitoring", drv.name);

    Ok(())
}

/// Send a heartbeat from a driver (resets missed-heartbeat counter).
pub fn heartbeat(id: u32) -> KernelResult<()> {
    let mut state = STATE.lock();
    let drv = state.drivers.iter_mut().find(|d| d.id == id)
        .ok_or(KernelError::NotFound)?;

    drv.last_heartbeat_ns = crate::hpet::elapsed_ns();
    drv.missed_heartbeats = 0;

    // If degraded, restore to healthy on heartbeat.
    if drv.health == DriverHealth::Degraded {
        drv.health = DriverHealth::Healthy;
    }

    Ok(())
}

/// Report that a driver has crashed (called by process manager on exit).
pub fn report_crash(id: u32) -> KernelResult<()> {
    let now = crate::hpet::elapsed_ns();

    let mut state = STATE.lock();

    // Find the driver index (avoids holding a mutable borrow on state.drivers
    // while also needing to update state-level counters).
    let idx = state.drivers.iter().position(|d| d.id == id)
        .ok_or(KernelError::NotFound)?;

    // Update driver-level crash data.
    #[allow(clippy::arithmetic_side_effects, clippy::indexing_slicing)]
    { state.drivers[idx].total_crashes += 1; }
    #[allow(clippy::indexing_slicing)]
    {
        state.drivers[idx].consecutive_crashes =
            state.drivers[idx].consecutive_crashes.saturating_add(1);
        state.drivers[idx].last_crash_ns = now;
        state.drivers[idx].pid = 0;
    }

    // Capture values for logging and policy decisions.
    #[allow(clippy::indexing_slicing)]
    let name = state.drivers[idx].name.clone();
    #[allow(clippy::indexing_slicing)]
    let consec = state.drivers[idx].consecutive_crashes;
    #[allow(clippy::indexing_slicing)]
    let total = state.drivers[idx].total_crashes;
    #[allow(clippy::indexing_slicing)]
    let max = state.drivers[idx].policy.max_restarts;
    #[allow(clippy::indexing_slicing)]
    let initial_backoff = state.drivers[idx].policy.initial_backoff_ns;
    #[allow(clippy::indexing_slicing)]
    let max_backoff = state.drivers[idx].policy.max_backoff_ns;
    #[allow(clippy::indexing_slicing)]
    let policy = state.drivers[idx].policy.restart_policy;

    // Check if we've exceeded max restarts.
    if consec > max {
        #[allow(clippy::indexing_slicing)]
        { state.drivers[idx].health = DriverHealth::Failed; }
        #[allow(clippy::arithmetic_side_effects)]
        { state.total_crashes += 1; }

        crate::syslog!("driver.crash", Critical,
            "Driver '{}' permanently failed after {} crashes", name, total);
        return Ok(());
    }

    // Compute restart delay and update driver state.
    let delay_ns = match policy {
        RestartPolicy::Immediate => {
            #[allow(clippy::indexing_slicing)]
            { state.drivers[idx].health = DriverHealth::Restarting; }
            0u64
        }
        RestartPolicy::Delayed(d) => {
            #[allow(clippy::indexing_slicing)]
            { state.drivers[idx].health = DriverHealth::Restarting; }
            d
        }
        RestartPolicy::ExponentialBackoff => {
            let exp = consec.saturating_sub(1);
            let mut delay = initial_backoff;
            for _ in 0..exp {
                delay = delay.saturating_mul(2);
                if delay >= max_backoff {
                    delay = max_backoff;
                    break;
                }
            }
            #[allow(clippy::indexing_slicing)]
            {
                state.drivers[idx].current_backoff_ns = delay;
                state.drivers[idx].health = DriverHealth::Restarting;
            }
            delay
        }
        RestartPolicy::Never => {
            #[allow(clippy::indexing_slicing)]
            { state.drivers[idx].health = DriverHealth::Failed; }
            #[allow(clippy::arithmetic_side_effects)]
            { state.total_crashes += 1; }
            crate::syslog!("driver.crash", Error,
                "Driver '{}' crashed, restart policy=Never", name);
            return Ok(());
        }
    };

    #[allow(clippy::arithmetic_side_effects)]
    { state.total_crashes += 1; }

    let delay_ms = delay_ns / 1_000_000;
    crate::syslog!("driver.crash", Warning,
        "Driver '{}' crashed (attempt {}/{}), restart in {} ms",
        name, consec, max, delay_ms);

    // In a real implementation, we'd schedule a timer callback for
    // delayed/backoff restarts. For now, mark for restart.
    // The actual restart happens via restart_driver() called by
    // the timer or manually.

    Ok(())
}

/// Execute a driver restart.
///
/// This should be called after any backoff delay. In a real implementation
/// this would re-exec the driver binary. Currently simulates the restart.
pub fn restart_driver(id: u32, new_pid: u32) -> KernelResult<()> {
    let now = crate::hpet::elapsed_ns();
    let mut state = STATE.lock();
    let idx = state.drivers.iter().position(|d| d.id == id)
        .ok_or(KernelError::NotFound)?;

    if state.drivers[idx].health == DriverHealth::Failed {
        return Err(KernelError::NotSupported);
    }

    state.drivers[idx].pid = new_pid;
    state.drivers[idx].health = DriverHealth::Healthy;
    state.drivers[idx].last_restart_ns = now;
    state.drivers[idx].last_heartbeat_ns = now;
    state.drivers[idx].missed_heartbeats = 0;
    #[allow(clippy::arithmetic_side_effects)]
    { state.drivers[idx].total_restarts += 1; }

    let name = state.drivers[idx].name.clone();
    let restarts = state.drivers[idx].total_restarts;

    #[allow(clippy::arithmetic_side_effects)]
    { state.total_restarts += 1; }

    crate::syslog!("driver.restart", Info,
        "Driver '{}' restarted (pid={}, total restarts={})",
        name, new_pid, restarts);

    Ok(())
}

/// Reset a driver's crash counter (after successful long-running period).
pub fn reset_crash_count(id: u32) -> KernelResult<()> {
    let mut state = STATE.lock();
    let drv = state.drivers.iter_mut().find(|d| d.id == id)
        .ok_or(KernelError::NotFound)?;

    drv.consecutive_crashes = 0;
    drv.current_backoff_ns = 0;

    if drv.health == DriverHealth::Failed {
        drv.health = DriverHealth::Stopped;
    }

    Ok(())
}

/// Perform a monitoring tick — check all drivers for health.
///
/// Returns a list of driver IDs that need attention (missed heartbeats
/// or unresponsive).
pub fn tick() -> Vec<u32> {
    let now = crate::hpet::elapsed_ns();
    let mut state = STATE.lock();
    state.last_tick_ns = now;
    #[allow(clippy::arithmetic_side_effects)]
    { state.tick_count += 1; }

    let mut needs_attention = Vec::new();

    for drv in &mut state.drivers {
        if !drv.monitoring || drv.health == DriverHealth::Failed
            || drv.health == DriverHealth::Stopped
            || drv.health == DriverHealth::Restarting
        {
            continue;
        }

        match drv.policy.health_mode {
            HealthMode::None => {}
            HealthMode::Heartbeat => {
                let elapsed = now.saturating_sub(drv.last_heartbeat_ns);
                if elapsed > drv.policy.heartbeat_timeout_ns {
                    drv.missed_heartbeats = drv.missed_heartbeats.saturating_add(1);

                    if drv.missed_heartbeats >= drv.policy.max_missed_heartbeats {
                        drv.health = DriverHealth::Unresponsive;
                        needs_attention.push(drv.id);
                    } else if drv.missed_heartbeats >= 1 {
                        drv.health = DriverHealth::Degraded;
                    }
                }
            }
            HealthMode::ProcessAlive => {
                // Check whether the driver's process is still alive in the
                // process table.  A pid of 0 means the driver has no process
                // (kernel-internal or stopped), so we skip it.
                if drv.pid != 0
                    && !crate::proc::pcb::is_process_running(
                        u64::from(drv.pid),
                    )
                {
                    drv.health = DriverHealth::Unresponsive;
                    needs_attention.push(drv.id);
                }
            }
        }
    }

    needs_attention
}

/// Pause monitoring for a driver (e.g., during planned maintenance).
pub fn pause_monitoring(id: u32) -> KernelResult<()> {
    let mut state = STATE.lock();
    let drv = state.drivers.iter_mut().find(|d| d.id == id)
        .ok_or(KernelError::NotFound)?;
    drv.monitoring = false;
    Ok(())
}

/// Resume monitoring for a driver.
pub fn resume_monitoring(id: u32) -> KernelResult<()> {
    let mut state = STATE.lock();
    let drv = state.drivers.iter_mut().find(|d| d.id == id)
        .ok_or(KernelError::NotFound)?;
    drv.monitoring = true;
    drv.last_heartbeat_ns = crate::hpet::elapsed_ns();
    drv.missed_heartbeats = 0;
    Ok(())
}

// ---------------------------------------------------------------------------
// Query API
// ---------------------------------------------------------------------------

/// Public view of a driver entry.
pub struct DriverInfo {
    pub id: u32,
    pub name: String,
    pub display_name: String,
    pub bus: BusType,
    pub pid: u32,
    pub health: DriverHealth,
    pub total_crashes: u64,
    pub consecutive_crashes: u32,
    pub total_restarts: u64,
    pub monitoring: bool,
    pub current_backoff_ms: u64,
}

/// List all monitored drivers.
pub fn list() -> Vec<DriverInfo> {
    let state = STATE.lock();
    state.drivers.iter().map(|d| DriverInfo {
        id: d.id,
        name: d.name.clone(),
        display_name: d.display_name.clone(),
        bus: d.bus,
        pid: d.pid,
        health: d.health,
        total_crashes: d.total_crashes,
        consecutive_crashes: d.consecutive_crashes,
        total_restarts: d.total_restarts,
        monitoring: d.monitoring,
        current_backoff_ms: d.current_backoff_ns / 1_000_000,
    }).collect()
}

/// Find a driver by name.
pub fn find_by_name(name: &str) -> Option<DriverInfo> {
    let state = STATE.lock();
    state.drivers.iter().find(|d| d.name == name).map(|d| DriverInfo {
        id: d.id,
        name: d.name.clone(),
        display_name: d.display_name.clone(),
        bus: d.bus,
        pid: d.pid,
        health: d.health,
        total_crashes: d.total_crashes,
        consecutive_crashes: d.consecutive_crashes,
        total_restarts: d.total_restarts,
        monitoring: d.monitoring,
        current_backoff_ms: d.current_backoff_ns / 1_000_000,
    })
}

// ---------------------------------------------------------------------------
// Statistics and procfs
// ---------------------------------------------------------------------------

/// Aggregate statistics.
pub struct MonitorStats {
    pub total_drivers: usize,
    pub healthy: usize,
    pub degraded: usize,
    pub unresponsive: usize,
    pub restarting: usize,
    pub failed: usize,
    pub stopped: usize,
    pub tick_count: u64,
    pub total_crashes: u64,
    pub total_restarts: u64,
}

/// Get monitor statistics.
pub fn stats() -> MonitorStats {
    let state = STATE.lock();
    let mut st = MonitorStats {
        total_drivers: state.drivers.len(),
        healthy: 0,
        degraded: 0,
        unresponsive: 0,
        restarting: 0,
        failed: 0,
        stopped: 0,
        tick_count: state.tick_count,
        total_crashes: state.total_crashes,
        total_restarts: state.total_restarts,
    };

    for drv in &state.drivers {
        match drv.health {
            DriverHealth::Healthy => { st.healthy += 1; }
            DriverHealth::Degraded => { st.degraded += 1; }
            DriverHealth::Unresponsive => { st.unresponsive += 1; }
            DriverHealth::Restarting => { st.restarting += 1; }
            DriverHealth::Failed => { st.failed += 1; }
            DriverHealth::Stopped => { st.stopped += 1; }
        }
    }

    st
}

/// Generate content for /proc/drvmon.
pub fn procfs_content() -> String {
    let st = stats();
    let drivers = list();
    let mut out = String::with_capacity(1024);

    out.push_str("Driver Monitor\n");
    out.push_str("==============\n");
    out.push_str(&format!("Monitored:     {}\n", st.total_drivers));
    out.push_str(&format!("  Healthy:     {}\n", st.healthy));
    out.push_str(&format!("  Degraded:    {}\n", st.degraded));
    out.push_str(&format!("  Unresponsive:{}\n", st.unresponsive));
    out.push_str(&format!("  Restarting:  {}\n", st.restarting));
    out.push_str(&format!("  Failed:      {}\n", st.failed));
    out.push_str(&format!("  Stopped:     {}\n", st.stopped));
    out.push_str(&format!("Ticks:         {}\n", st.tick_count));
    out.push_str(&format!("Total crashes: {}\n", st.total_crashes));
    out.push_str(&format!("Total restarts:{}\n", st.total_restarts));

    if !drivers.is_empty() {
        out.push_str(&format!("\n{:>3} {:12} {:8} {:>6} {:12} {:>6} {:>6} {:>4}\n",
            "ID", "Name", "Bus", "PID", "Health", "Crash", "Rstrt", "Mon"));
        for d in &drivers {
            out.push_str(&format!("{:>3} {:12} {:8} {:>6} {:12} {:>6} {:>6} {:>4}\n",
                d.id, d.name, d.bus.label(), d.pid, d.health.label(),
                d.total_crashes, d.total_restarts,
                if d.monitoring { "yes" } else { "no" }));
        }
    }

    out
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run driver monitor self-tests.
pub fn self_test() -> KernelResult<()> {
    crate::serial_println!("[drvmon] Running driver monitor self-tests...");

    // Clean slate.
    {
        let mut state = STATE.lock();
        *state = State::new();
    }
    init();

    // Test 1: Register drivers.
    let nvme_id = register("nvme", "NVMe SSD Controller", BusType::Pci, 200, DriverPolicy::default())?;
    let net_id = register("e1000", "Intel Gigabit Ethernet", BusType::Pci, 201, DriverPolicy {
        health_mode: HealthMode::Heartbeat,
        ..DriverPolicy::default()
    })?;
    {
        let state = STATE.lock();
        if state.drivers.len() != 2 {
            crate::serial_println!("[drvmon]   FAIL: expected 2 drivers");
            return Err(KernelError::InternalError);
        }
    }
    crate::serial_println!("[drvmon]   1. Register drivers: OK");

    // Test 2: Duplicate rejection.
    let dup = register("nvme", "Dup", BusType::Pci, 202, DriverPolicy::default());
    if dup.is_ok() {
        crate::serial_println!("[drvmon]   FAIL: duplicate not rejected");
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[drvmon]   2. Duplicate rejection: OK");

    // Test 3: Heartbeat.
    heartbeat(net_id)?;
    {
        let state = STATE.lock();
        let drv = state.drivers.iter().find(|d| d.id == net_id);
        if let Some(d) = drv {
            if d.missed_heartbeats != 0 {
                crate::serial_println!("[drvmon]   FAIL: heartbeat didn't reset counter");
                return Err(KernelError::InternalError);
            }
        }
    }
    crate::serial_println!("[drvmon]   3. Heartbeat: OK");

    // Test 4: Report crash.
    report_crash(nvme_id)?;
    {
        let state = STATE.lock();
        let drv = state.drivers.iter().find(|d| d.id == nvme_id);
        if let Some(d) = drv {
            if d.total_crashes != 1 || d.health != DriverHealth::Restarting {
                crate::serial_println!("[drvmon]   FAIL: crash not recorded properly (crashes={}, health={:?})",
                    d.total_crashes, d.health);
                return Err(KernelError::InternalError);
            }
        }
    }
    crate::serial_println!("[drvmon]   4. Report crash: OK");

    // Test 5: Restart driver.
    restart_driver(nvme_id, 210)?;
    {
        let state = STATE.lock();
        let drv = state.drivers.iter().find(|d| d.id == nvme_id);
        if let Some(d) = drv {
            if d.pid != 210 || d.health != DriverHealth::Healthy || d.total_restarts != 1 {
                crate::serial_println!("[drvmon]   FAIL: restart didn't update state");
                return Err(KernelError::InternalError);
            }
        }
    }
    crate::serial_println!("[drvmon]   5. Restart driver: OK");

    // Test 6: Max restarts → permanent failure.
    for _ in 0..6 {
        report_crash(nvme_id)?;
    }
    {
        let state = STATE.lock();
        let drv = state.drivers.iter().find(|d| d.id == nvme_id);
        if let Some(d) = drv {
            if d.health != DriverHealth::Failed {
                crate::serial_println!("[drvmon]   FAIL: expected Failed after max restarts");
                return Err(KernelError::InternalError);
            }
        }
    }
    crate::serial_println!("[drvmon]   6. Max restarts → Failed: OK");

    // Test 7: Reset crash count.
    reset_crash_count(nvme_id)?;
    {
        let state = STATE.lock();
        let drv = state.drivers.iter().find(|d| d.id == nvme_id);
        if let Some(d) = drv {
            if d.consecutive_crashes != 0 || d.health != DriverHealth::Stopped {
                crate::serial_println!("[drvmon]   FAIL: crash count not reset");
                return Err(KernelError::InternalError);
            }
        }
    }
    crate::serial_println!("[drvmon]   7. Reset crash count: OK");

    // Test 8: Monitor tick.
    let needs = tick();
    // No unresponsive drivers expected (e1000 just had a heartbeat).
    if !needs.is_empty() {
        crate::serial_println!("[drvmon]   FAIL: unexpected unresponsive drivers");
        return Err(KernelError::InternalError);
    }
    {
        let state = STATE.lock();
        if state.tick_count != 1 {
            crate::serial_println!("[drvmon]   FAIL: tick_count should be 1");
            return Err(KernelError::InternalError);
        }
    }
    crate::serial_println!("[drvmon]   8. Monitor tick: OK");

    // Test 9: Pause/resume monitoring.
    pause_monitoring(net_id)?;
    {
        let state = STATE.lock();
        let drv = state.drivers.iter().find(|d| d.id == net_id);
        if let Some(d) = drv {
            if d.monitoring {
                crate::serial_println!("[drvmon]   FAIL: monitoring should be paused");
                return Err(KernelError::InternalError);
            }
        }
    }
    resume_monitoring(net_id)?;
    crate::serial_println!("[drvmon]   9. Pause/resume: OK");

    // Test 10: Find by name.
    let info = find_by_name("e1000");
    if info.is_none() || info.as_ref().is_some_and(|i| i.id != net_id) {
        crate::serial_println!("[drvmon]   FAIL: find_by_name failed");
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[drvmon]   10. Find by name: OK");

    // Test 11: Unregister.
    unregister(net_id)?;
    {
        let state = STATE.lock();
        if state.drivers.len() != 1 {
            crate::serial_println!("[drvmon]   FAIL: expected 1 driver after unregister");
            return Err(KernelError::InternalError);
        }
    }
    crate::serial_println!("[drvmon]   11. Unregister: OK");

    // Test 12: Stats and procfs.
    let st = stats();
    if st.total_crashes == 0 {
        crate::serial_println!("[drvmon]   FAIL: expected > 0 crashes");
        return Err(KernelError::InternalError);
    }
    let content = procfs_content();
    if !content.contains("Driver Monitor") {
        crate::serial_println!("[drvmon]   FAIL: procfs missing header");
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[drvmon]   12. Stats and procfs: OK");

    // Clean up.
    {
        let mut state = STATE.lock();
        *state = State::new();
    }

    crate::serial_println!("[drvmon] All 12 self-tests passed.");
    Ok(())
}
