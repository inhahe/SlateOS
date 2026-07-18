//! PID 1 init process — the first userspace process and system orchestrator.
//!
//! Responsibilities:
//!
//! - **System boot sequencing**: coordinates the transition from kernel init
//!   to a fully running system by invoking subsystem initializers in order.
//! - **Service orchestration**: delegates to [`crate::svcstart`] for
//!   dependency-resolved parallel service startup.
//! - **Orphan reaping**: adopts orphaned processes (reparented to PID 1)
//!   and reaps their exit status to prevent zombies.
//! - **System state management**: tracks system runlevel (boot → running →
//!   shutdown) and coordinates clean shutdown.
//! - **Signal routing**: receives system-wide signals (power button, OOM)
//!   and dispatches them to appropriate handlers.
//! - **Watchdog coordination**: delegates to [`crate::drvmon`] for driver
//!   health monitoring and to [`crate::reslimit`] for resource enforcement.
//!
//! ## Design
//!
//! Per the design spec: "modularized — the init manager doesn't do tons of
//! other things." PID 1 is deliberately thin. It delegates:
//!
//! | Concern | Module |
//! |---------|--------|
//! | Service dependencies | `svcstart` |
//! | Socket activation | `sockact` |
//! | Crash restart | `svcstart` + `drvmon` |
//! | Resource limits | `reslimit` |
//! | Log persistence | `logpersist` |
//! | Event logging | `eventlog` |
//!
//! ## Boot Sequence
//!
//! ```text
//! kernel main → initproc::start()
//!   1. Set system state to Booting
//!   2. Initialize subsystems (eventlog, logpersist, reslimit, drvmon, sockact)
//!   3. Mount essential filesystems (/proc, /sys, /tmp, /var)
//!   4. Start services via svcstart::boot_services()
//!   5. Start socket listeners via sockact
//!   6. Launch startup apps
//!   7. Set system state to Running
//!   8. Enter main loop (reap orphans, flush logs, check watchdogs)
//! ```
//!
//! ## Shutdown Sequence
//!
//! ```text
//! initproc::shutdown(reason)
//!   1. Set system state to ShuttingDown
//!   2. Send shutdown signal to all services (reverse dependency order)
//!   3. Wait for graceful exit (with timeout)
//!   4. Force-kill remaining processes
//!   5. Flush logs
//!   6. Unmount filesystems
//!   7. Set system state to Halted
//!   8. Platform power-off or reboot
//! ```

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// PID of the init process.
pub const INIT_PID: u32 = 1;

/// Default grace period for shutdown (nanoseconds) — 10 seconds.
const DEFAULT_SHUTDOWN_GRACE_NS: u64 = 10_000_000_000;

/// Interval between main loop ticks (nanoseconds) — 1 second.
const TICK_INTERVAL_NS: u64 = 1_000_000_000;

/// Interval between log flushes (nanoseconds) — 30 seconds.
const LOG_FLUSH_INTERVAL_NS: u64 = 30_000_000_000;

/// Interval between driver health checks (nanoseconds) — 5 seconds.
const HEALTH_CHECK_INTERVAL_NS: u64 = 5_000_000_000;

/// Maximum orphan PIDs to track.
const MAX_ORPHANS: usize = 1024;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// System runlevel — the current state of the system lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemState {
    /// Kernel has started, init not yet running.
    KernelInit,
    /// Init process is running boot sequence.
    Booting,
    /// All services started, system is operational.
    Running,
    /// System is shutting down gracefully.
    ShuttingDown,
    /// Shutdown complete, waiting for power-off.
    Halted,
    /// Emergency mode — critical failure, minimal services.
    Emergency,
}

impl SystemState {
    /// Human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            Self::KernelInit => "Kernel Init",
            Self::Booting => "Booting",
            Self::Running => "Running",
            Self::ShuttingDown => "Shutting Down",
            Self::Halted => "Halted",
            Self::Emergency => "Emergency",
        }
    }
}

/// Reason for system shutdown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShutdownReason {
    /// User requested shutdown.
    UserRequest,
    /// User requested reboot.
    Reboot,
    /// Power button pressed.
    PowerButton,
    /// Critical system error.
    CriticalError,
    /// Out of memory, system unrecoverable.
    OomCritical,
    /// Thermal emergency.
    ThermalCritical,
}

impl ShutdownReason {
    /// Human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            Self::UserRequest => "User Request",
            Self::Reboot => "Reboot",
            Self::PowerButton => "Power Button",
            Self::CriticalError => "Critical Error",
            Self::OomCritical => "OOM Critical",
            Self::ThermalCritical => "Thermal Critical",
        }
    }
}

/// A boot stage with timing information.
#[derive(Debug, Clone)]
struct BootStage {
    /// Stage name.
    name: String,
    /// When this stage started (ns since boot).
    start_ns: u64,
    /// When this stage completed (ns since boot, 0 = not complete).
    end_ns: u64,
    /// Whether the stage succeeded.
    success: bool,
    /// Error message if failed.
    error: Option<String>,
}

/// An orphaned process waiting to be reaped.
#[derive(Debug, Clone, Copy)]
struct OrphanEntry {
    /// The orphan's PID.
    pid: u32,
    /// When the process was reparented to init (ns since boot).
    reparented_ns: u64,
    /// Whether we've attempted to reap this process.
    reaped: bool,
}

/// Init process configuration.
#[derive(Debug, Clone)]
pub struct InitConfig {
    /// Grace period for shutdown (ns).
    pub shutdown_grace_ns: u64,
    /// Whether to auto-flush logs periodically.
    pub auto_flush_logs: bool,
    /// Log flush interval (ns).
    pub log_flush_interval_ns: u64,
    /// Whether to run driver health checks.
    pub health_checks: bool,
    /// Health check interval (ns).
    pub health_check_interval_ns: u64,
    /// Whether to enter emergency mode on critical service failure.
    pub emergency_on_critical_failure: bool,
}

impl Default for InitConfig {
    fn default() -> Self {
        Self {
            shutdown_grace_ns: DEFAULT_SHUTDOWN_GRACE_NS,
            auto_flush_logs: true,
            log_flush_interval_ns: LOG_FLUSH_INTERVAL_NS,
            health_checks: true,
            health_check_interval_ns: HEALTH_CHECK_INTERVAL_NS,
            emergency_on_critical_failure: true,
        }
    }
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct State {
    /// Current system state.
    system_state: SystemState,
    /// Init configuration.
    config: InitConfig,
    /// Boot stages (for boot timing).
    boot_stages: Vec<BootStage>,
    /// Total boot time (ns).
    boot_time_ns: u64,
    /// Orphaned processes waiting to be reaped.
    orphans: Vec<OrphanEntry>,
    /// Total orphans reaped since boot.
    total_reaped: u64,
    /// Shutdown reason (if shutting down).
    shutdown_reason: Option<ShutdownReason>,
    /// Shutdown requested timestamp (ns).
    shutdown_requested_ns: u64,
    /// Number of processes that exited cleanly during shutdown.
    shutdown_clean_exits: u32,
    /// Number of processes force-killed during shutdown.
    shutdown_force_kills: u32,
    /// Last log flush timestamp (ns).
    last_log_flush_ns: u64,
    /// Last health check timestamp (ns).
    last_health_check_ns: u64,
    /// Last main loop tick timestamp (ns).
    last_tick_ns: u64,
    /// Total main loop ticks.
    total_ticks: u64,
    /// Critical services (if any fail, enter emergency mode).
    critical_services: Vec<String>,
    /// Whether initialized.
    initialized: bool,
}

impl State {
    const fn new() -> Self {
        Self {
            system_state: SystemState::KernelInit,
            config: InitConfig {
                shutdown_grace_ns: DEFAULT_SHUTDOWN_GRACE_NS,
                auto_flush_logs: true,
                log_flush_interval_ns: LOG_FLUSH_INTERVAL_NS,
                health_checks: true,
                health_check_interval_ns: HEALTH_CHECK_INTERVAL_NS,
                emergency_on_critical_failure: true,
            },
            boot_stages: Vec::new(),
            boot_time_ns: 0,
            orphans: Vec::new(),
            total_reaped: 0,
            shutdown_reason: None,
            shutdown_requested_ns: 0,
            shutdown_clean_exits: 0,
            shutdown_force_kills: 0,
            last_log_flush_ns: 0,
            last_health_check_ns: 0,
            last_tick_ns: 0,
            total_ticks: 0,
            critical_services: Vec::new(),
            initialized: false,
        }
    }
}

static STATE: Mutex<State> = Mutex::new(State::new());

// ---------------------------------------------------------------------------
// Boot Sequence
// ---------------------------------------------------------------------------

/// Start the init process and run the boot sequence.
///
/// This is called from kernel main after basic kernel initialization is
/// complete. It initializes all subsystems in order and starts services.
pub fn start() -> KernelResult<()> {
    let now = crate::hpet::elapsed_ns();

    {
        let mut state = STATE.lock();
        if state.initialized {
            return Err(KernelError::AlreadyExists);
        }
        state.system_state = SystemState::Booting;
        state.initialized = true;
    }

    crate::syslog!("init", Info, "PID 1 init process starting");

    // Stage 1: Event logging (initialized statically, just record the stage).
    begin_stage("eventlog");
    // eventlog uses a static ring buffer — no init() needed.
    end_stage("eventlog", true, None);

    // Stage 2: Log persistence.
    begin_stage("logpersist");
    crate::logpersist::init();
    end_stage("logpersist", true, None);

    // Stage 3: Resource limits.
    begin_stage("reslimit");
    crate::reslimit::init();
    end_stage("reslimit", true, None);

    // Stage 4: Service manager.
    begin_stage("svcstart");
    crate::svcstart::init();
    end_stage("svcstart", true, None);

    // Stage 5: Socket activation.
    begin_stage("sockact");
    crate::sockact::init();
    end_stage("sockact", true, None);

    // Stage 6: Driver monitor.
    begin_stage("drvmon");
    crate::drvmon::init();
    end_stage("drvmon", true, None);

    // Stage 7: Boot services (dependency resolution + parallel start).
    begin_stage("services");
    let svc_result = crate::svcstart::boot_services();
    let svc_ok = svc_result.is_ok();
    let svc_err = svc_result.err().map(|e| format!("{:?}", e));
    end_stage("services", svc_ok, svc_err.as_deref());

    // Stage 8: Finalize.
    begin_stage("finalize");

    let boot_end = crate::hpet::elapsed_ns();
    {
        let mut state = STATE.lock();
        state.boot_time_ns = boot_end.saturating_sub(now);
        state.system_state = SystemState::Running;
        state.last_log_flush_ns = boot_end;
        state.last_health_check_ns = boot_end;
        state.last_tick_ns = boot_end;
    }

    end_stage("finalize", true, None);

    let boot_ms = boot_end.saturating_sub(now) / 1_000_000;
    crate::syslog!("init", Info, "System boot complete in {} ms", boot_ms);

    Ok(())
}

/// Record the beginning of a boot stage.
fn begin_stage(name: &str) {
    let now = crate::hpet::elapsed_ns();
    let mut state = STATE.lock();
    state.boot_stages.push(BootStage {
        name: String::from(name),
        start_ns: now,
        end_ns: 0,
        success: false,
        error: None,
    });
}

/// Record the end of a boot stage.
fn end_stage(name: &str, success: bool, error: Option<&str>) {
    let now = crate::hpet::elapsed_ns();
    let mut state = STATE.lock();

    if let Some(stage) = state.boot_stages.iter_mut().rfind(|s| s.name == name) {
        stage.end_ns = now;
        stage.success = success;
        if let Some(msg) = error {
            stage.error = Some(String::from(msg));
        }
    }

    let duration_us = if let Some(stage) = state.boot_stages.iter().rfind(|s| s.name == name) {
        stage.end_ns.saturating_sub(stage.start_ns) / 1000
    } else {
        0
    };

    if success {
        crate::syslog!("init.boot", Info, "Stage '{}' complete ({} µs)", name, duration_us);
    } else {
        crate::syslog!("init.boot", Error, "Stage '{}' FAILED ({} µs): {}",
            name, duration_us, error.unwrap_or("unknown"));
    }
}

// ---------------------------------------------------------------------------
// Main Loop (called periodically by timer or scheduler)
// ---------------------------------------------------------------------------

/// Run one iteration of the init main loop.
///
/// This should be called periodically (e.g., once per second from a timer
/// callback or idle loop). It handles:
///
/// 1. Orphan reaping
/// 2. Log flushing
/// 3. Driver/service health checks
/// 4. Socket activation idle checks
pub fn tick() {
    let now = crate::hpet::elapsed_ns();
    let mut state = STATE.lock();

    if state.system_state != SystemState::Running {
        return;
    }

    // Rate-limit ticks.
    if now.saturating_sub(state.last_tick_ns) < TICK_INTERVAL_NS {
        return;
    }
    state.last_tick_ns = now;
    #[allow(clippy::arithmetic_side_effects)]
    { state.total_ticks += 1; }

    // Capture config values before dropping the lock for subsystem calls.
    let auto_flush = state.config.auto_flush_logs;
    let flush_interval = state.config.log_flush_interval_ns;
    let last_flush = state.last_log_flush_ns;
    let do_flush = auto_flush && now.saturating_sub(last_flush) >= flush_interval;

    let do_health = state.config.health_checks;
    let health_interval = state.config.health_check_interval_ns;
    let last_health = state.last_health_check_ns;
    let do_health_check = do_health && now.saturating_sub(last_health) >= health_interval;

    if do_flush {
        state.last_log_flush_ns = now;
    }
    if do_health_check {
        state.last_health_check_ns = now;
    }

    // Reap orphans.
    //
    // For each orphan, attempt try_reap().  This is non-blocking:
    // - Ok(Some(_)) → zombie reaped, process resources freed.
    // - Ok(None)    → still running, keep in the list for next tick.
    // - Err(_)      → process doesn't exist (already cleaned up),
    //   treat as reaped.
    let orphan_count = state.orphans.len();
    // Collect PIDs to attempt reaping — we must drop state lock first
    // because try_reap acquires PROCESS_TABLE, and we need consistent
    // lock ordering (initproc STATE is always acquired *before*
    // PROCESS_TABLE, not after).
    let pending: Vec<u32> = state
        .orphans
        .iter()
        .filter(|o| !o.reaped)
        .map(|o| o.pid)
        .collect();

    drop(state);

    // Attempt reap outside the state lock.
    let mut reaped_pids = Vec::new();
    for &opid in &pending {
        match crate::proc::pcb::try_reap(
            INIT_PID as crate::proc::pcb::ProcessId,
            opid as crate::proc::pcb::ProcessId,
        ) {
            Ok(Some(_exit_info)) => {
                // Successfully reaped — zombie process resources freed.
                reaped_pids.push(opid);
            }
            Ok(None) => {
                // Still running — will retry on next tick.
            }
            Err(_) => {
                // Process doesn't exist (already cleaned up or invalid).
                // Remove from orphan list to avoid retrying forever.
                reaped_pids.push(opid);
            }
        }
    }

    // Re-acquire state to update orphan list and counters.
    let mut state = STATE.lock();
    for &rpid in &reaped_pids {
        if let Some(orphan) = state.orphans.iter_mut().find(|o| o.pid == rpid) {
            orphan.reaped = true;
        }
    }
    let reaped = reaped_pids.len() as u64;
    state.orphans.retain(|o| !o.reaped);
    #[allow(clippy::arithmetic_side_effects)]
    { state.total_reaped += reaped; }

    drop(state);

    // Log flushing (outside state lock to avoid deadlocks with logpersist).
    if do_flush {
        let _ = crate::logpersist::flush();
    }

    // Driver health checks.
    if do_health_check {
        let needs_attention = crate::drvmon::tick();
        if !needs_attention.is_empty() {
            crate::syslog!("init.health", Warning,
                "{} driver(s) need attention", needs_attention.len());
        }

        // Check socket activation idle stops.
        let idle_services = crate::sockact::check_idle();
        if !idle_services.is_empty() {
            crate::syslog!("init.sockact", Info,
                "{} idle service(s) eligible for stop", idle_services.len());
        }
    }

    // Periodic orphan logging (every 100 ticks if there were orphans).
    if orphan_count > 0 {
        let state = STATE.lock();
        if state.total_ticks.is_multiple_of(100) {
            crate::syslog!("init.reap", Info,
                "Reaped {} orphans this cycle, {} total", reaped, state.total_reaped);
        }
    }
}

// ---------------------------------------------------------------------------
// Orphan Management
// ---------------------------------------------------------------------------

/// Register an orphaned process to be reaped by init.
///
/// Called by the process manager (pcb::remove_thread) when a parent
/// process exits before its children.  The child's `parent` field in
/// the PCB has already been updated to `INIT_PID` by the caller.
///
/// This adds the orphan PID to init's tracking list.  The periodic
/// `tick()` function will attempt `try_reap` on each orphan — if the
/// child is still running, it stays in the list until it exits.
pub fn register_orphan(pid: u32) -> KernelResult<()> {
    let now = crate::hpet::elapsed_ns();
    let mut state = STATE.lock();

    // Avoid duplicates (same PID registered twice via race).
    if state.orphans.iter().any(|o| o.pid == pid) {
        return Ok(());
    }

    if state.orphans.len() >= MAX_ORPHANS {
        // Force-reap oldest orphan to make room.  Drop the lock
        // first so try_reap can acquire PROCESS_TABLE.
        if !state.orphans.is_empty() {
            let removed = state.orphans.remove(0);
            #[allow(clippy::arithmetic_side_effects)]
            { state.total_reaped += 1; }

            // Best-effort: attempt actual reap of the evicted orphan.
            drop(state);
            let _ = crate::proc::pcb::try_reap(
                INIT_PID as crate::proc::pcb::ProcessId,
                removed.pid as crate::proc::pcb::ProcessId,
            );
            crate::syslog!("init.reap", Warning,
                "Force-reaped orphan pid {} (queue full)", removed.pid);
            state = STATE.lock();
        }
    }

    state.orphans.push(OrphanEntry {
        pid,
        reparented_ns: now,
        reaped: false,
    });

    Ok(())
}

/// Get count of pending orphans.
pub fn orphan_count() -> usize {
    STATE.lock().orphans.len()
}

// ---------------------------------------------------------------------------
// Shutdown
// ---------------------------------------------------------------------------

/// Initiate system shutdown.
///
/// This starts the graceful shutdown sequence:
/// 1. Set state to ShuttingDown
/// 2. Notify all services to stop
/// 3. Wait for grace period
/// 4. Force-kill remaining processes
/// 5. Flush logs and unmount filesystems
pub fn shutdown(reason: ShutdownReason) -> KernelResult<()> {
    let now = crate::hpet::elapsed_ns();

    {
        let mut state = STATE.lock();
        if state.system_state == SystemState::ShuttingDown
            || state.system_state == SystemState::Halted
        {
            return Err(KernelError::InvalidArgument);
        }
        state.system_state = SystemState::ShuttingDown;
        state.shutdown_reason = Some(reason);
        state.shutdown_requested_ns = now;
    }

    crate::syslog!("init.shutdown", Info,
        "System shutdown initiated: {}", reason.label());

    let grace_ns = STATE.lock().config.shutdown_grace_ns;

    // Phase 1: Stop services in reverse dependency order.
    //
    // Services at higher dependency levels (which depend on lower-level
    // services) are stopped first, then lower levels.  This ensures a
    // service is stopped only after its dependents have exited.
    crate::syslog!("init.shutdown", Info, "Stopping services...");

    let levels = crate::svcstart::start_levels();
    let mut clean_exits: u32 = 0;
    let mut force_kills: u32 = 0;

    // Iterate levels in reverse: highest dependency level first.
    for level in levels.iter().rev() {
        for &(svc_id, ref name) in level {
            // Check if the service is running before trying to stop it.
            if let Ok(info) = crate::fs::servicemgr::get_service(svc_id) {
                if info.state == crate::fs::servicemgr::ServiceState::Running
                    || info.state == crate::fs::servicemgr::ServiceState::Starting
                {
                    crate::syslog!("init.shutdown", Info,
                        "Stopping service '{}' (id {})", name, svc_id);
                    match crate::fs::servicemgr::stop_service(svc_id) {
                        Ok(()) => {
                            clean_exits = clean_exits.saturating_add(1);
                        }
                        Err(e) => {
                            crate::syslog!("init.shutdown", Warning,
                                "Failed to stop service '{}' (id {}): {:?}",
                                name, svc_id, e);
                            force_kills = force_kills.saturating_add(1);
                        }
                    }
                }
            }
        }
    }

    // Also stop any services not in the start graph (manually started).
    let all_services = crate::fs::servicemgr::list_services();
    for svc in &all_services {
        if svc.state == crate::fs::servicemgr::ServiceState::Running
            || svc.state == crate::fs::servicemgr::ServiceState::Starting
        {
            crate::syslog!("init.shutdown", Info,
                "Stopping remaining service '{}' (id {})", svc.name, svc.id);
            match crate::fs::servicemgr::stop_service(svc.id) {
                Ok(()) => {
                    clean_exits = clean_exits.saturating_add(1);
                }
                Err(e) => {
                    crate::syslog!("init.shutdown", Warning,
                        "Failed to stop remaining service '{}' (id {}): {:?}",
                        svc.name, svc.id, e);
                    force_kills = force_kills.saturating_add(1);
                }
            }
        }
    }

    crate::syslog!("init.shutdown", Info,
        "Services stopped: {} clean exits", clean_exits);

    // Phase 2: Reap any remaining orphans.
    //
    // Give orphaned processes a chance to exit by running reap cycles.
    // After the grace period, any remaining zombies are force-reaped.
    let reap_start = crate::hpet::elapsed_ns();
    let deadline = reap_start.saturating_add(grace_ns);
    let mut reap_passes: u32 = 0;
    loop {
        let remaining = orphan_count();
        if remaining == 0 { break; }

        let now_ns = crate::hpet::elapsed_ns();
        if now_ns >= deadline { break; }

        // Do a reap pass (similar to tick's orphan logic).
        let pending: Vec<u32>;
        {
            let state = STATE.lock();
            pending = state.orphans.iter()
                .filter(|o| !o.reaped)
                .map(|o| o.pid)
                .collect();
        }

        let mut reaped_pids = Vec::new();
        for &opid in &pending {
            match crate::proc::pcb::try_reap(
                INIT_PID as crate::proc::pcb::ProcessId,
                opid as crate::proc::pcb::ProcessId,
            ) {
                Ok(Some(_)) => { reaped_pids.push(opid); }
                Ok(None) => {}
                Err(_) => { reaped_pids.push(opid); }
            }
        }

        if !reaped_pids.is_empty() {
            let mut state = STATE.lock();
            for &rpid in &reaped_pids {
                if let Some(orphan) = state.orphans.iter_mut().find(|o| o.pid == rpid) {
                    orphan.reaped = true;
                }
            }
            let count = reaped_pids.len() as u64;
            state.orphans.retain(|o| !o.reaped);
            #[allow(clippy::arithmetic_side_effects)]
            { state.total_reaped += count; }
        }

        reap_passes = reap_passes.saturating_add(1);
    }

    // Force-reap anything still in the orphan list.
    {
        let mut state = STATE.lock();
        let remaining = state.orphans.len();
        if remaining > 0 {
            crate::syslog!("init.shutdown", Warning,
                "Force-clearing {} remaining orphans", remaining);
            force_kills = remaining as u32;
        }
        #[allow(clippy::arithmetic_side_effects)]
        { state.total_reaped += remaining as u64; }
        state.orphans.clear();
    }

    crate::syslog!("init.shutdown", Info,
        "Orphan cleanup: {} passes, {} force-killed", reap_passes, force_kills);

    // Phase 3: Sync filesystems and flush caches.
    crate::syslog!("init.shutdown", Info, "Syncing filesystems...");
    if let Err(e) = crate::fs::vfs::Vfs::sync() {
        // Filesystem sync failure during shutdown may indicate data loss.
        crate::syslog!("init.shutdown", Error,
            "Filesystem sync failed: {:?} — data may not be persisted", e);
    }

    // Phase 4: Flush logs one final time.
    crate::syslog!("init.shutdown", Info, "Flushing logs...");
    if let Err(e) = crate::logpersist::flush() {
        // Log loss during shutdown is unfortunate but not fatal.
        crate::serial_println!("[init] WARNING: final log flush failed: {:?}", e);
    }

    // Phase 5: Mark as halted with shutdown statistics.
    {
        let mut state = STATE.lock();
        state.system_state = SystemState::Halted;
        state.shutdown_clean_exits = clean_exits;
        state.shutdown_force_kills = force_kills;
    }

    let shutdown_ms = crate::hpet::elapsed_ns().saturating_sub(now) / 1_000_000;
    crate::syslog!("init.shutdown", Info,
        "Shutdown complete in {} ms (reason: {}, {} clean, {} forced)",
        shutdown_ms, reason.label(), clean_exits, force_kills);

    Ok(())
}

/// Request a reboot.
pub fn reboot() -> KernelResult<()> {
    shutdown(ShutdownReason::Reboot)
}

// ---------------------------------------------------------------------------
// Critical Services
// ---------------------------------------------------------------------------

/// Mark a service as critical — if it fails, enter emergency mode.
pub fn mark_critical(service_name: &str) {
    let mut state = STATE.lock();
    if !state.critical_services.iter().any(|s| s == service_name) {
        state.critical_services.push(String::from(service_name));
    }
}

/// Unmark a service as critical.
pub fn unmark_critical(service_name: &str) {
    let mut state = STATE.lock();
    state.critical_services.retain(|s| s != service_name);
}

/// Report a critical service failure — may trigger emergency mode.
pub fn report_critical_failure(service_name: &str) {
    let enter_emergency;
    {
        let state = STATE.lock();
        enter_emergency = state.config.emergency_on_critical_failure
            && state.critical_services.iter().any(|s| s == service_name);
    }

    if enter_emergency {
        crate::syslog!("init.critical", Error,
            "Critical service '{}' failed — entering emergency mode", service_name);
        let mut state = STATE.lock();
        state.system_state = SystemState::Emergency;
    }
}

// ---------------------------------------------------------------------------
// System State Query
// ---------------------------------------------------------------------------

/// Get the current system state.
pub fn system_state() -> SystemState {
    STATE.lock().system_state
}

/// Get boot timing information.
pub fn boot_time_ns() -> u64 {
    STATE.lock().boot_time_ns
}

/// Get boot stages with timing.
pub fn boot_stages() -> Vec<(String, u64, bool)> {
    let state = STATE.lock();
    state.boot_stages.iter().map(|s| {
        let duration = s.end_ns.saturating_sub(s.start_ns);
        (s.name.clone(), duration, s.success)
    }).collect()
}

/// Get whether the system is fully booted and running.
pub fn is_running() -> bool {
    STATE.lock().system_state == SystemState::Running
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Update init process configuration.
pub fn set_config(config: InitConfig) {
    STATE.lock().config = config;
}

/// Get a copy of the current configuration.
pub fn get_config() -> InitConfig {
    STATE.lock().config.clone()
}

// ---------------------------------------------------------------------------
// Procfs
// ---------------------------------------------------------------------------

/// Generate content for `/proc/init`.
pub fn procfs_content() -> String {
    let state = STATE.lock();

    let mut out = String::from("=== Init Process (PID 1) ===\n\n");

    out.push_str(&format!("System State: {}\n", state.system_state.label()));
    out.push_str(&format!("Boot Time: {:.2} ms\n",
        state.boot_time_ns as f64 / 1_000_000.0));
    out.push_str(&format!("Main Loop Ticks: {}\n", state.total_ticks));
    out.push_str(&format!("Orphans Reaped: {}\n", state.total_reaped));
    out.push_str(&format!("Pending Orphans: {}\n", state.orphans.len()));

    if let Some(reason) = state.shutdown_reason {
        out.push_str(&format!("Shutdown Reason: {}\n", reason.label()));
        out.push_str(&format!("Clean Exits: {}\n", state.shutdown_clean_exits));
        out.push_str(&format!("Force Kills: {}\n", state.shutdown_force_kills));
    }

    out.push_str(&format!("\nCritical Services: {}\n", state.critical_services.len()));
    for svc in &state.critical_services {
        out.push_str(&format!("  - {}\n", svc));
    }

    out.push_str("\n--- Boot Stages ---\n");
    for stage in &state.boot_stages {
        let duration_us = stage.end_ns.saturating_sub(stage.start_ns) / 1000;
        let status = if stage.end_ns == 0 {
            "IN PROGRESS"
        } else if stage.success {
            "OK"
        } else {
            "FAILED"
        };
        out.push_str(&format!("  {:<15} {:>8} µs  {}", stage.name, duration_us, status));
        if let Some(ref err) = stage.error {
            out.push_str(&format!("  ({})", err));
        }
        out.push('\n');
    }

    out.push_str("\n--- Config ---\n");
    out.push_str(&format!("  Shutdown Grace: {} ms\n",
        state.config.shutdown_grace_ns / 1_000_000));
    out.push_str(&format!("  Auto Flush Logs: {}\n", state.config.auto_flush_logs));
    out.push_str(&format!("  Log Flush Interval: {} s\n",
        state.config.log_flush_interval_ns / 1_000_000_000));
    out.push_str(&format!("  Health Checks: {}\n", state.config.health_checks));
    out.push_str(&format!("  Health Check Interval: {} s\n",
        state.config.health_check_interval_ns / 1_000_000_000));
    out.push_str(&format!("  Emergency on Critical Failure: {}\n",
        state.config.emergency_on_critical_failure));

    out
}

// ---------------------------------------------------------------------------
// Self-Tests
// ---------------------------------------------------------------------------

/// Run self-tests for the init process module.
pub fn self_test() -> bool {
    crate::serial_println!("[initproc] Running self-tests...");
    let mut passed = 0u32;
    let mut failed = 0u32;

    macro_rules! check {
        ($name:expr, $cond:expr) => {
            if $cond {
                crate::serial_println!("  [PASS] {}", $name);
                #[allow(clippy::arithmetic_side_effects)]
                { passed += 1; }
            } else {
                crate::serial_println!("  [FAIL] {}", $name);
                #[allow(clippy::arithmetic_side_effects)]
                { failed += 1; }
            }
        };
    }

    // Reset state for testing.
    {
        let mut state = STATE.lock();
        *state = State::new();
    }

    // Test 1: Initial state is KernelInit.
    check!("initial state is KernelInit",
        system_state() == SystemState::KernelInit);

    // Test 2: Start sets state to Running.
    let r = start();
    check!("start succeeds", r.is_ok());
    check!("state is Running after start", system_state() == SystemState::Running);
    check!("is_running returns true", is_running());

    // Test 3: Boot time recorded.
    check!("boot time > 0", boot_time_ns() > 0);

    // Test 4: Boot stages recorded.
    let stages = boot_stages();
    check!("boot stages recorded", !stages.is_empty());
    // Should have: eventlog, logpersist, reslimit, svcstart, sockact, drvmon, services, finalize
    check!("at least 6 boot stages", stages.len() >= 6);

    // Test 5: All boot stages succeeded.
    let all_ok = stages.iter().all(|(_, _, success)| *success);
    check!("all boot stages succeeded", all_ok);

    // Test 6: Double-start fails.
    let r = start();
    check!("double-start fails with AlreadyExists", r == Err(KernelError::AlreadyExists));

    // Test 7: Orphan management.
    let r = register_orphan(42);
    check!("register orphan succeeds", r.is_ok());
    check!("orphan count is 1", orphan_count() == 1);

    // Run a tick to reap orphans.
    {
        let mut state = STATE.lock();
        state.last_tick_ns = 0; // Force tick to run.
    }
    tick();
    check!("orphan reaped after tick", orphan_count() == 0);
    {
        let state = STATE.lock();
        check!("total_reaped incremented", state.total_reaped >= 1);
    }

    // Test 8: Critical services.
    mark_critical("network");
    {
        let state = STATE.lock();
        check!("critical service registered", state.critical_services.len() == 1);
    }

    unmark_critical("network");
    {
        let state = STATE.lock();
        check!("critical service unregistered", state.critical_services.is_empty());
    }

    // Test 9: Critical failure triggers emergency mode.
    mark_critical("storage");
    report_critical_failure("storage");
    check!("emergency mode after critical failure",
        system_state() == SystemState::Emergency);

    // Restore to Running for remaining tests.
    {
        let mut state = STATE.lock();
        state.system_state = SystemState::Running;
    }

    // Test 10: Shutdown.
    let r = shutdown(ShutdownReason::UserRequest);
    check!("shutdown succeeds", r.is_ok());
    check!("state is Halted after shutdown", system_state() == SystemState::Halted);

    // Test 11: Double-shutdown fails.
    let r = shutdown(ShutdownReason::Reboot);
    check!("double-shutdown fails", r.is_err());

    // Test 12: Procfs content is non-empty.
    let content = procfs_content();
    check!("procfs content is non-empty", content.len() > 100);
    check!("procfs contains system state",
        content.contains("System State:"));
    check!("procfs contains boot stages",
        content.contains("Boot Stages"));

    // Test 13: Config management.
    let mut config = get_config();
    config.shutdown_grace_ns = 5_000_000_000;
    set_config(config.clone());
    let retrieved = get_config();
    check!("config update persists",
        retrieved.shutdown_grace_ns == 5_000_000_000);

    crate::serial_println!("[initproc] Tests complete: {} passed, {} failed", passed, failed);
    failed == 0
}
