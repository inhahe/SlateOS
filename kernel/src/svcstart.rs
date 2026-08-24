//! Service startup orchestration — dependency resolution, parallel start, crash restart.
//!
//! Builds on [`crate::fs::servicemgr`] to provide:
//!
//! - **Dependency-based parallel startup**: topological sort of the service
//!   dependency graph, grouping into start levels so independent services
//!   launch simultaneously.
//! - **Cycle detection**: rejects startup if the dependency graph has cycles.
//! - **Crash restart with exponential backoff**: tracks failure timestamps
//!   and delays restarts (1s → 2s → 4s → … → 60s cap, configurable max
//!   retries).
//! - **"Service ready" notification**: services signal readiness; dependents
//!   only start once their dependencies are ready (not just running).
//! - **Startup app list**: ordered list of applications to launch after all
//!   services are up, with configurable wait-for-ready and disk-idle heuristic.
//!
//! ## Architecture
//!
//! ```text
//! Boot sequence
//!   → svcstart::boot_services()
//!     → resolve dependency graph (topological sort)
//!     → for each start level (parallel within level):
//!         → servicemgr::start_service(id)
//!         → wait for "ready" signal or timeout
//!     → svcstart::run_startup_apps()
//!         → launch each app in order
//!         → wait for ready or disk-idle timeout
//!
//! Crash handling
//!   → svcstart::report_crash(service_id)
//!     → record failure timestamp
//!     → if auto_restart && retries < max:
//!         compute backoff, arm a ktimer for it
//!           → (backoff elapses) → servicemgr::restart_service(id)
//!     → else: mark permanently failed
//! ```
//!
//! The restart is genuinely deferred: `report_crash` returns as soon as the
//! timer is armed, and `ktimer` runs the restart on the workqueue once the
//! backoff expires. This is load-bearing rather than incidental — a service
//! that fails on startup would otherwise respawn as fast as it can die, which
//! is the failure the backoff exists to bound.
//!
//! ## Integration
//!
//! - Called from the init process after basic kernel init.
//! - Kshell `svcstart` command for status and manual control.
//! - `/proc/svcstart` shows startup state and crash history.

#![allow(dead_code)]

use crate::sync::PreemptSpinMutex as Mutex;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::error::{KernelError, KernelResult};
use crate::fs::servicemgr;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Default initial restart delay (nanoseconds) — 1 second.
const DEFAULT_INITIAL_BACKOFF_NS: u64 = 1_000_000_000;

/// Default maximum backoff cap (nanoseconds) — 60 seconds.
const DEFAULT_MAX_BACKOFF_NS: u64 = 60_000_000_000;

/// Default maximum restart attempts before permanent failure.
const DEFAULT_MAX_RETRIES: u32 = 5;

/// Default timeout waiting for a service to signal ready (nanoseconds) — 10s.
const DEFAULT_READY_TIMEOUT_NS: u64 = 10_000_000_000;

/// Default disk-idle timeout for startup apps (nanoseconds) — 3s.
const DEFAULT_DISK_IDLE_TIMEOUT_NS: u64 = 3_000_000_000;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A single service in the startup dependency graph.
#[derive(Debug, Clone)]
struct StartNode {
    /// Service ID in servicemgr.
    service_id: u32,
    /// Service name (cached for convenience).
    name: String,
    /// Start level (0 = no dependencies, 1 = depends on level-0 services, ...).
    level: u32,
    /// Whether the service has signaled readiness.
    ready: bool,
    /// Timestamp when the service was started (ns since boot), or `None` if it
    /// has not been started.
    ///
    /// Not a `u64` with `0` meaning "not started": this clock counts
    /// nanoseconds since boot and starts *at* zero, so `0` is a legal instant
    /// — and it is the instant the earliest service in the graph carries,
    /// which is exactly the one a boot-time report most wants to name.
    started_at_ns: Option<u64>,
    /// Timestamp when the service signaled ready (ns since boot), or `None` if
    /// it has not signalled.
    ///
    /// `Option` for the same reason as `started_at_ns`, and here the
    /// distinction is load-bearing in a second way: a service that never
    /// signals ready and one that signalled instantly are the two things
    /// [`startup_timings`] exists to tell apart.
    ready_at_ns: Option<u64>,
}

/// Crash history for a single service.
#[derive(Debug, Clone)]
struct CrashRecord {
    /// Service ID.
    service_id: u32,
    /// Service name.
    name: String,
    /// Consecutive failure count (resets on successful long-running period).
    consecutive_failures: u32,
    /// Timestamp of most recent crash (ns since boot), or `None` if the
    /// service has not crashed since this record was created.
    ///
    /// Not a `u64` with `0` meaning "never": the clock counts nanoseconds
    /// since boot and starts *at* zero, so `0` is a legal instant — and a
    /// service crashing at uptime 0 is not a hypothetical, it is a service
    /// that fails during boot, which is exactly the case this record exists
    /// to track.
    last_crash_ns: Option<u64>,
    /// Current backoff delay (doubles each failure, caps at max).
    current_backoff_ns: u64,
    /// Whether the service has been permanently marked as failed.
    permanently_failed: bool,
    /// Total lifetime crash count.
    total_crashes: u64,
    /// Timestamps of last N crashes for debugging.
    crash_history: Vec<u64>,
    /// The timer that will perform the pending restart, if one is armed.
    ///
    /// Held so the restart can be cancelled — by [`cancel_pending_restarts`]
    /// when the crash records are torn down, and by a second crash report
    /// before the first restart has fired. Without it a fired-and-forgotten
    /// timer would restart a service whose record no longer exists.
    pending_restart: Option<crate::ktimer::TimerHandle>,
}

/// A public snapshot of one service's crash history.
///
/// A named struct rather than the tuple this used to be: the tuple had five
/// positional fields of which three were integers, so every caller
/// destructured by position and any new field silently shifted the meaning of
/// the ones after it. That is a bad trade at three fields and an untenable one
/// at seven.
#[derive(Debug, Clone)]
pub struct CrashInfo {
    /// Service name.
    pub name: String,
    /// Consecutive failures since the last reset.
    pub consecutive_failures: u32,
    /// Lifetime crash count.
    pub total_crashes: u64,
    /// Current backoff delay, in milliseconds.
    pub backoff_ms: u64,
    /// Whether the service has been given up on.
    pub permanently_failed: bool,
    /// When the service last crashed (ns since boot); `None` if it has not.
    pub last_crash_ns: Option<u64>,
    /// Whether a backoff restart is currently armed and waiting to fire.
    pub restart_pending: bool,
    /// Timestamps (ns since boot) of the last few crashes, oldest first.
    ///
    /// `last_crash_ns` gives the most recent instant only, which cannot
    /// distinguish a service that crashed ten times in two seconds from one
    /// that crashed ten times over a week — and that distinction is the whole
    /// point of looking at a crash record. The record has always collected
    /// these timestamps; until now nothing read them.
    pub crash_history: Vec<u64>,
}

impl CrashInfo {
    /// Render the crash history as ages relative to `now_ns`, oldest first.
    ///
    /// Ages rather than raw uptimes: an operator reading a crash record wants
    /// the *shape* of the failure — "3s 2s 1s" is a service in a tight loop,
    /// "9000s 4000s 1s" is one that fails occasionally — and neither is
    /// legible as three ten-digit nanosecond counts.
    ///
    /// Returns an empty string when there is no history, so a caller can skip
    /// the line entirely rather than print an empty list.
    #[must_use]
    pub fn history_ages(&self, now_ns: u64) -> String {
        if self.crash_history.is_empty() {
            return String::new();
        }
        let mut out = String::with_capacity(self.crash_history.len().saturating_mul(6));
        for (i, ts) in self.crash_history.iter().enumerate() {
            if i > 0 {
                out.push(' ');
            }
            // saturating: a timestamp from the future would mean the clock ran
            // backwards, which is worth showing as 0 rather than wrapping to
            // 584 years.
            let age_ms = now_ns.saturating_sub(*ts) / 1_000_000;
            if age_ms < 1000 {
                out.push_str(&format!("{age_ms}ms"));
            } else {
                out.push_str(&format!("{}.{}s", age_ms / 1000, (age_ms % 1000) / 100));
            }
        }
        out
    }
}

/// An entry in the startup app list.
#[derive(Debug, Clone)]
pub struct StartupApp {
    /// Unique ID for this entry.
    pub id: u32,
    /// Application path (executable).
    pub path: String,
    /// Command-line arguments.
    pub args: String,
    /// Whether to wait for this app to signal ready before starting the next.
    pub wait_for_ready: bool,
    /// Whether this entry is enabled.
    pub enabled: bool,
    /// Display name for UI.
    pub display_name: String,
    /// Sort order (lower = starts earlier).
    pub order: u32,
}

/// Boot phase state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootPhase {
    /// Not started.
    Idle,
    /// Resolving dependency graph.
    Resolving,
    /// Starting services (level by level).
    StartingServices,
    /// Running startup apps.
    StartingApps,
    /// All startup complete.
    Complete,
    /// Failed — dependency cycle or critical service failure.
    Failed,
}

impl BootPhase {
    /// Human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Idle => "Idle",
            Self::Resolving => "Resolving",
            Self::StartingServices => "Starting Services",
            Self::StartingApps => "Starting Apps",
            Self::Complete => "Complete",
            Self::Failed => "Failed",
        }
    }
}

/// Configuration for the startup orchestrator.
#[derive(Clone)]
pub struct StartupConfig {
    /// Initial backoff delay for crash restarts (ns).
    pub initial_backoff_ns: u64,
    /// Maximum backoff cap (ns).
    pub max_backoff_ns: u64,
    /// Maximum restart attempts before permanent failure.
    pub max_retries: u32,
    /// Timeout for service ready signal (ns).
    pub ready_timeout_ns: u64,
    /// Disk-idle timeout for startup apps (ns).
    pub disk_idle_timeout_ns: u64,
}

impl Default for StartupConfig {
    fn default() -> Self {
        Self {
            initial_backoff_ns: DEFAULT_INITIAL_BACKOFF_NS,
            max_backoff_ns: DEFAULT_MAX_BACKOFF_NS,
            max_retries: DEFAULT_MAX_RETRIES,
            ready_timeout_ns: DEFAULT_READY_TIMEOUT_NS,
            disk_idle_timeout_ns: DEFAULT_DISK_IDLE_TIMEOUT_NS,
        }
    }
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct State {
    config: StartupConfig,
    /// The resolved startup graph (populated during boot_services).
    start_graph: Vec<StartNode>,
    /// Maximum start level computed.
    max_level: u32,
    /// Crash history per service.
    crash_records: Vec<CrashRecord>,
    /// The startup app list.
    startup_apps: Vec<StartupApp>,
    /// Next startup app ID.
    next_app_id: u32,
    /// Current boot phase.
    phase: BootPhase,
    /// Current start level being processed.
    current_level: u32,
    /// Total services started during boot.
    services_started: u32,
    /// Total startup apps launched.
    apps_launched: u32,
    /// Total crash restarts performed.
    total_restarts: u64,
    /// Boot start timestamp (ns).
    boot_start_ns: u64,
    /// Boot end timestamp (ns).
    boot_end_ns: u64,
    /// Whether initialized.
    initialized: bool,
}

impl State {
    const fn new() -> Self {
        Self {
            config: StartupConfig {
                initial_backoff_ns: DEFAULT_INITIAL_BACKOFF_NS,
                max_backoff_ns: DEFAULT_MAX_BACKOFF_NS,
                max_retries: DEFAULT_MAX_RETRIES,
                ready_timeout_ns: DEFAULT_READY_TIMEOUT_NS,
                disk_idle_timeout_ns: DEFAULT_DISK_IDLE_TIMEOUT_NS,
            },
            start_graph: Vec::new(),
            max_level: 0,
            crash_records: Vec::new(),
            startup_apps: Vec::new(),
            next_app_id: 1,
            phase: BootPhase::Idle,
            current_level: 0,
            services_started: 0,
            apps_launched: 0,
            total_restarts: 0,
            boot_start_ns: 0,
            boot_end_ns: 0,
            initialized: false,
        }
    }
}

static STATE: Mutex<State> = Mutex::new(State::new());

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Initialize the startup orchestrator with default config.
pub fn init() {
    init_with_config(StartupConfig::default());
}

/// Initialize with a custom configuration.
pub fn init_with_config(config: StartupConfig) {
    let mut state = STATE.lock();
    if state.initialized {
        return;
    }
    state.config = config;
    state.initialized = true;
}

// ---------------------------------------------------------------------------
// Dependency resolution — topological sort
// ---------------------------------------------------------------------------

/// Resolve the service dependency graph and assign start levels.
///
/// Level 0 = services with no dependencies (start first).
/// Level 1 = services that depend only on level-0 services.
/// Level N = services that depend on services in levels 0..N-1.
///
/// Services within the same level can be started in parallel.
///
/// Returns an error if a dependency cycle is detected.
pub fn resolve_dependencies() -> KernelResult<()> {
    let mut state = STATE.lock();
    state.phase = BootPhase::Resolving;
    state.start_graph.clear();

    let services = servicemgr::list_services();
    if services.is_empty() {
        state.phase = BootPhase::Complete;
        return Ok(());
    }

    // Build the list of nodes, initially with level = u32::MAX (unresolved).
    let mut nodes: Vec<(u32, String, Vec<String>, u32)> = services
        .iter()
        .map(|s| (s.id, s.name.clone(), s.depends_on.clone(), u32::MAX))
        .collect();

    // Iteratively assign levels:
    // - Services with no deps → level 0.
    // - Services whose all deps are resolved → level = max(dep levels) + 1.
    // Repeat until no more progress (cycle if unresolved remain).
    let max_iterations = nodes.len();
    let mut resolved_count = 0usize;

    for _iteration in 0..=max_iterations {
        let mut progress = false;

        for i in 0..nodes.len() {
            // Skip already resolved.
            if nodes[i].3 != u32::MAX {
                continue;
            }

            let deps = &nodes[i].2;

            // If no dependencies, level 0.
            if deps.is_empty() {
                nodes[i].3 = 0;
                #[allow(clippy::arithmetic_side_effects)]
                {
                    resolved_count += 1;
                }
                progress = true;
                continue;
            }

            // Check if all dependencies are resolved.
            let mut all_resolved = true;
            let mut max_dep_level: u32 = 0;

            for dep_name in deps {
                if let Some(dep_node) = nodes.iter().find(|n| &n.1 == dep_name) {
                    if dep_node.3 == u32::MAX {
                        all_resolved = false;
                        break;
                    }
                    if dep_node.3 > max_dep_level {
                        max_dep_level = dep_node.3;
                    }
                } else {
                    // Dependency on unknown service — treat as optional, skip.
                    continue;
                }
            }

            if all_resolved {
                nodes[i].3 = max_dep_level.saturating_add(1);
                #[allow(clippy::arithmetic_side_effects)]
                {
                    resolved_count += 1;
                }
                progress = true;
            }
        }

        if resolved_count == nodes.len() {
            break;
        }

        if !progress {
            // No progress means a dependency cycle exists.
            state.phase = BootPhase::Failed;
            // Collect unresolved service names for the error message.
            let _unresolved: Vec<&str> = nodes
                .iter()
                .filter(|n| n.3 == u32::MAX)
                .map(|n| n.1.as_str())
                .collect();

            crate::syslog!(
                "service.startup",
                Error,
                "Dependency cycle detected in service graph"
            );
            return Err(KernelError::InvalidArgument);
        }
    }

    // Build the final start graph.
    let mut max_level: u32 = 0;
    for (id, name, _deps, level) in &nodes {
        if *level > max_level {
            max_level = *level;
        }
        state.start_graph.push(StartNode {
            service_id: *id,
            name: name.clone(),
            level: *level,
            ready: false,
            started_at_ns: None,
            ready_at_ns: None,
        });
    }

    state.max_level = max_level;
    Ok(())
}

/// Get the start order as levels: each entry is a list of service IDs
/// that can be started in parallel.
pub fn start_levels() -> Vec<Vec<(u32, String)>> {
    let state = STATE.lock();
    let mut levels: Vec<Vec<(u32, String)>> = Vec::new();

    for level_idx in 0..=state.max_level {
        let services: Vec<(u32, String)> = state
            .start_graph
            .iter()
            .filter(|n| n.level == level_idx)
            .map(|n| (n.service_id, n.name.clone()))
            .collect();
        if !services.is_empty() {
            levels.push(services);
        }
    }

    levels
}

// ---------------------------------------------------------------------------
// Boot sequence
// ---------------------------------------------------------------------------

/// Execute the full boot sequence: resolve deps → start services by level.
///
/// Returns the number of services started.
pub fn boot_services() -> KernelResult<u32> {
    // `init()` acquires STATE itself, so it must run with no guard held —
    // calling it from inside the block below re-acquired a non-reentrant lock
    // this task already held and wedged the CPU. The bound local makes the
    // guard's release a visible statement rather than something inferred from
    // temporary-lifetime rules, which is what got this wrong the first time.
    //
    // Not hypothetical: the normal path (`initproc`) calls `init()` before
    // `boot_services()`, so `initialized` is already true and the branch is not
    // taken — but the kernel shell's `boot` command calls `boot_services()`
    // directly, and that is exactly the caller for which it is false.
    let needs_init = !STATE.lock().initialized;
    if needs_init {
        init();
    }

    {
        let mut state = STATE.lock();
        state.boot_start_ns = crate::hpet::elapsed_ns();
        state.phase = BootPhase::Resolving;
    }

    // Step 1: resolve dependency graph.
    resolve_dependencies()?;

    // Step 2: start services level by level.
    let levels = start_levels();
    let mut total_started: u32 = 0;

    {
        let mut state = STATE.lock();
        state.phase = BootPhase::StartingServices;
    }

    for (level_idx, level) in levels.iter().enumerate() {
        crate::syslog!(
            "service.startup",
            Info,
            "Starting service level {} ({} services)",
            level_idx,
            level.len()
        );

        for (svc_id, svc_name) in level {
            // Skip disabled services.
            if let Ok(info) = servicemgr::get_service(*svc_id) {
                if info.startup_type == servicemgr::StartupType::Disabled {
                    continue;
                }
                if info.state == servicemgr::ServiceState::Running {
                    // Already running (e.g., started during init_defaults).
                    let mut state = STATE.lock();
                    if let Some(node) = state
                        .start_graph
                        .iter_mut()
                        .find(|n| n.service_id == *svc_id)
                    {
                        node.ready = true;
                        node.started_at_ns = Some(info.last_start_ns);
                        node.ready_at_ns = Some(info.last_start_ns);
                    }
                    total_started = total_started.saturating_add(1);
                    continue;
                }
            }

            // Attempt to start the service.
            match servicemgr::start_service(*svc_id) {
                Ok(()) => {
                    let now = crate::hpet::elapsed_ns();
                    let mut state = STATE.lock();
                    if let Some(node) = state
                        .start_graph
                        .iter_mut()
                        .find(|n| n.service_id == *svc_id)
                    {
                        node.started_at_ns = Some(now);
                    }
                    total_started = total_started.saturating_add(1);

                    crate::syslog!(
                        "service.startup",
                        Info,
                        "Service '{}' started (level {})",
                        svc_name,
                        level_idx
                    );
                }
                Err(e) => {
                    crate::syslog!(
                        "service.startup",
                        Error,
                        "Failed to start service '{}': {:?}",
                        svc_name,
                        e
                    );
                }
            }
        }
    }

    {
        let mut state = STATE.lock();
        state.services_started = total_started;
        state.phase = BootPhase::StartingApps;
    }

    // Step 3: run startup apps.
    let apps_launched = run_startup_apps();

    {
        let mut state = STATE.lock();
        state.apps_launched = apps_launched;
        state.boot_end_ns = crate::hpet::elapsed_ns();
        state.phase = BootPhase::Complete;
    }

    crate::syslog!(
        "service.startup",
        Info,
        "Boot sequence complete: {} services, {} apps",
        total_started,
        apps_launched
    );

    Ok(total_started)
}

/// Notify that a service has signaled readiness.
pub fn signal_ready(service_id: u32) {
    let mut state = STATE.lock();
    if let Some(node) = state
        .start_graph
        .iter_mut()
        .find(|n| n.service_id == service_id)
    {
        node.ready = true;
        node.ready_at_ns = Some(crate::hpet::elapsed_ns());
    }

    crate::syslog!(
        "service.startup",
        Info,
        "Service id={} signaled ready",
        service_id
    );
}

// ---------------------------------------------------------------------------
// Crash restart with exponential backoff
// ---------------------------------------------------------------------------

/// Report that a service has crashed. Handles automatic restart scheduling.
///
/// Returns `Ok(delay_ns)` if a restart was scheduled, or `Err` if the
/// service has exceeded max retries or is not configured for auto-restart.
pub fn report_crash(service_id: u32) -> KernelResult<u64> {
    let now = crate::hpet::elapsed_ns();

    // Look up the service to check auto_restart.
    let info = servicemgr::get_service(service_id)?;
    if !info.auto_restart {
        return Err(KernelError::NotSupported);
    }

    let mut state = STATE.lock();
    let config = state.config.clone();

    // Find or create crash record.
    let record = if let Some(r) = state
        .crash_records
        .iter_mut()
        .find(|r| r.service_id == service_id)
    {
        r
    } else {
        state.crash_records.push(CrashRecord {
            service_id,
            name: info.name.clone(),
            consecutive_failures: 0,
            last_crash_ns: None,
            current_backoff_ns: config.initial_backoff_ns,
            permanently_failed: false,
            total_crashes: 0,
            crash_history: Vec::new(),
            pending_restart: None,
        });
        // Safe: we just pushed, so last() is Some.
        state
            .crash_records
            .last_mut()
            .ok_or(KernelError::InternalError)?
    };

    if record.permanently_failed {
        return Err(KernelError::NotSupported);
    }

    // A crash arriving while a restart is still pending supersedes it. Leaving
    // the old timer armed would fire a restart on top of the one this report
    // is about to schedule, at the *previous* (shorter) backoff — turning a
    // doubling delay into two overlapping restarts.
    if let Some(handle) = record.pending_restart.take() {
        crate::ktimer::cancel(handle);
    }

    // Update crash record.
    record.consecutive_failures = record.consecutive_failures.saturating_add(1);
    record.last_crash_ns = Some(now);
    #[allow(clippy::arithmetic_side_effects)]
    {
        record.total_crashes += 1;
    }

    // Keep last 10 crash timestamps.
    if record.crash_history.len() >= 10 {
        record.crash_history.remove(0);
    }
    record.crash_history.push(now);

    // Check if we've exceeded max retries.
    if record.consecutive_failures > config.max_retries {
        record.permanently_failed = true;
        crate::syslog!(
            "service.crash",
            Critical,
            "Service '{}' permanently failed after {} crashes",
            info.name,
            record.total_crashes
        );
        return Err(KernelError::ResourceExhausted);
    }

    // Compute exponential backoff: initial * 2^(failures-1), capped.
    let exponent = record.consecutive_failures.saturating_sub(1);
    let mut delay = config.initial_backoff_ns;
    for _ in 0..exponent {
        delay = delay.saturating_mul(2);
        if delay >= config.max_backoff_ns {
            delay = config.max_backoff_ns;
            break;
        }
    }
    record.current_backoff_ns = delay;

    // Arm the restart for `delay` from now, rather than performing it here.
    //
    // This is what makes the backoff a backoff. It was previously computed,
    // logged to the operator as "restart in N ms", and then discarded: the
    // restart happened immediately on this very call. A service that crashes
    // on startup therefore respawned as fast as it could die, five times in a
    // row, burning a core and flooding the log inside a few milliseconds --
    // which is the precise failure mode exponential backoff exists to prevent.
    // The delay was not merely unused; it was contradicted by the code
    // directly beneath the message that announced it.
    //
    // `ktimer` runs the callback on the workqueue, in process context, so the
    // restart may allocate and block. The argument is the service id, which is
    // looked up again when the timer fires: nothing may be held across the
    // delay, since the service can be stopped or removed in the meantime.
    let handle =
        crate::ktimer::schedule_after_ns(restart_after_backoff, u64::from(service_id), delay);
    record.pending_restart = handle;

    // Capture values before the record borrow ends so we can update
    // state-level fields (can't mutate state while record borrows crash_records).
    let consec = record.consecutive_failures;

    #[allow(clippy::arithmetic_side_effects)]
    {
        state.total_restarts += 1;
    }

    let delay_ms = delay / 1_000_000;
    if handle.is_some() {
        crate::syslog!(
            "service.crash",
            Warning,
            "Service '{}' crashed (attempt {}/{}), restart in {} ms",
            info.name,
            consec,
            config.max_retries,
            delay_ms
        );
        drop(state);
    } else {
        // The timer table is full. Restarting immediately is the wrong
        // behaviour -- it is exactly the behaviour this change removed -- but
        // it beats never restarting the service at all, so do it and say so.
        // Silently falling back would recreate the original bug in a form that
        // only appears under load, which is worse than the original.
        crate::syslog!(
            "service.crash",
            Error,
            "Service '{}' crashed (attempt {}/{}): no timer slot for a {} ms backoff, restarting immediately",
            info.name,
            consec,
            config.max_retries,
            delay_ms
        );
        drop(state);
        let _ = servicemgr::restart_service(service_id);
    }

    Ok(delay)
}

/// Timer callback: perform a restart whose backoff has now elapsed.
///
/// Runs on the workqueue worker task, so allocation and blocking are
/// permitted. Takes the service id rather than any borrowed state, because the
/// backoff may be a full minute and nothing about the service is guaranteed to
/// still be true when it expires.
fn restart_after_backoff(arg: u64) {
    #[allow(clippy::cast_possible_truncation)]
    let service_id = arg as u32;

    // Clear the handle first. The timer has fired, so the stored handle is
    // stale; leaving it would let a later `cancel_pending_restarts` report a
    // cancellation that did not happen.
    {
        let mut state = STATE.lock();
        if let Some(record) = state
            .crash_records
            .iter_mut()
            .find(|r| r.service_id == service_id)
        {
            record.pending_restart = None;
            // A service marked permanently failed between the crash report and
            // now must not be resurrected by an in-flight timer.
            if record.permanently_failed {
                return;
            }
        } else {
            // The record is gone (records were cleared, or the service was
            // removed). Restarting on the strength of a record that no longer
            // exists would act on a decision nothing stands behind.
            return;
        }
    }

    if let Err(e) = servicemgr::restart_service(service_id) {
        crate::syslog!(
            "service.crash",
            Error,
            "Backoff restart of service id={} failed: {:?}",
            service_id,
            e
        );
    }
}

/// Cancel every pending backoff restart, returning how many were cancelled.
///
/// Needed wherever the crash records are discarded — teardown and the
/// self-test's pristine-state swap — because a timer outlives the record that
/// armed it. A restart firing against a discarded record would either act on a
/// service id that has been reused, or resurrect a service the caller had
/// just finished tearing down.
pub fn cancel_pending_restarts() -> usize {
    let mut state = STATE.lock();
    let mut cancelled = 0usize;
    for record in &mut state.crash_records {
        if let Some(handle) = record.pending_restart.take() {
            if crate::ktimer::cancel(handle) {
                cancelled = cancelled.saturating_add(1);
            }
        }
    }
    cancelled
}

/// Reset a service's crash counter (e.g., after running successfully for a while).
///
/// Also disarms any pending backoff restart. The counter is reset because the
/// service is now considered healthy, and a queued restart is a decision made
/// on the strength of the crash history this call has just declared void —
/// firing it would bounce a running service for a crash that no longer counts.
pub fn reset_crash_count(service_id: u32) {
    let mut state = STATE.lock();
    let initial_backoff = state.config.initial_backoff_ns;
    if let Some(record) = state
        .crash_records
        .iter_mut()
        .find(|r| r.service_id == service_id)
    {
        if let Some(handle) = record.pending_restart.take() {
            crate::ktimer::cancel(handle);
        }
        record.consecutive_failures = 0;
        record.current_backoff_ns = initial_backoff;
        record.permanently_failed = false;
    }
}

// ---------------------------------------------------------------------------
// Startup app list
// ---------------------------------------------------------------------------

/// Add an app to the startup list.
pub fn add_startup_app(path: &str, args: &str, display_name: &str, wait_for_ready: bool) -> u32 {
    let mut state = STATE.lock();
    let id = state.next_app_id;
    state.next_app_id = state.next_app_id.saturating_add(1);

    let order = state.startup_apps.len() as u32;

    state.startup_apps.push(StartupApp {
        id,
        path: String::from(path),
        args: String::from(args),
        display_name: String::from(display_name),
        wait_for_ready,
        enabled: true,
        order,
    });

    id
}

/// Remove a startup app by ID.
pub fn remove_startup_app(id: u32) -> KernelResult<()> {
    let mut state = STATE.lock();
    let idx = state
        .startup_apps
        .iter()
        .position(|a| a.id == id)
        .ok_or(KernelError::NotFound)?;
    state.startup_apps.remove(idx);
    Ok(())
}

/// Toggle a startup app's enabled state.
pub fn toggle_startup_app(id: u32) -> KernelResult<bool> {
    let mut state = STATE.lock();
    let app = state
        .startup_apps
        .iter_mut()
        .find(|a| a.id == id)
        .ok_or(KernelError::NotFound)?;
    app.enabled = !app.enabled;
    Ok(app.enabled)
}

/// Reorder a startup app (set its order value).
pub fn reorder_startup_app(id: u32, new_order: u32) -> KernelResult<()> {
    let mut state = STATE.lock();
    let app = state
        .startup_apps
        .iter_mut()
        .find(|a| a.id == id)
        .ok_or(KernelError::NotFound)?;
    app.order = new_order;
    Ok(())
}

/// List startup apps in order.
pub fn list_startup_apps() -> Vec<StartupApp> {
    let state = STATE.lock();
    let mut apps = state.startup_apps.clone();
    apps.sort_by_key(|a| a.order);
    apps
}

/// Run all enabled startup apps in order.
///
/// Returns the number of apps launched.
fn run_startup_apps() -> u32 {
    let apps = list_startup_apps();
    let mut launched: u32 = 0;

    for app in &apps {
        if !app.enabled {
            continue;
        }

        crate::syslog!(
            "service.startup",
            Info,
            "Launching startup app: {} ({})",
            app.display_name,
            app.path
        );

        // In a real implementation, this would spawn a process via the
        // process manager. For now, we just log the launch.
        launched = launched.saturating_add(1);

        // If wait_for_ready, we'd wait for the process to signal ready
        // or hit the disk-idle timeout. Placeholder for timer integration.
    }

    launched
}

// ---------------------------------------------------------------------------
// Statistics and procfs
// ---------------------------------------------------------------------------

/// Startup orchestrator statistics.
pub struct StartupStats {
    pub phase: BootPhase,
    pub max_level: u32,
    pub current_level: u32,
    pub services_started: u32,
    pub apps_launched: u32,
    pub total_restarts: u64,
    pub boot_start_ns: u64,
    pub boot_end_ns: u64,
    pub graph_size: usize,
    pub crash_records: usize,
    pub startup_apps: usize,
    pub max_retries: u32,
    pub initial_backoff_ms: u64,
    pub max_backoff_ms: u64,
}

/// Get startup statistics.
pub fn stats() -> StartupStats {
    let state = STATE.lock();
    StartupStats {
        phase: state.phase,
        max_level: state.max_level,
        current_level: state.current_level,
        services_started: state.services_started,
        apps_launched: state.apps_launched,
        total_restarts: state.total_restarts,
        boot_start_ns: state.boot_start_ns,
        boot_end_ns: state.boot_end_ns,
        graph_size: state.start_graph.len(),
        crash_records: state.crash_records.len(),
        startup_apps: state.startup_apps.len(),
        max_retries: state.config.max_retries,
        initial_backoff_ms: state.config.initial_backoff_ns / 1_000_000,
        max_backoff_ms: state.config.max_backoff_ns / 1_000_000,
    }
}

/// One service's contribution to boot time.
///
/// The fields are `Option` because "never started" and "started at uptime 0"
/// are different answers, and a boot-time report that conflates them blames
/// the wrong service — see [`StartNode::started_at_ns`].
#[derive(Debug, Clone)]
pub struct StartupTiming {
    /// Service ID in servicemgr.
    pub service_id: u32,
    /// Service name.
    pub name: String,
    /// Dependency level; services on the same level start together.
    pub level: u32,
    /// When the service was started, or `None` if it never was.
    pub started_at_ns: Option<u64>,
    /// When it signalled ready, or `None` if it never did.
    pub ready_at_ns: Option<u64>,
    /// How long it took to become ready, or `None` if either endpoint is
    /// missing.
    ///
    /// Saturating: `ready_at` should never precede `started_at` — both come
    /// from the same monotonic HPET — but reporting a duration of zero is a
    /// better failure than reporting one of several hundred years.
    pub ready_after_ns: Option<u64>,
}

/// Per-service boot timings, slowest first.
///
/// This is the `systemd-analyze blame` of the startup orchestrator: when boot
/// is slow, the question is always *which service* is slow, and the graph has
/// been recording the answer since it was written without anything ever asking
/// for it. Services that never became ready sort last — they have no duration
/// to compare, and their absence from the ranking is itself the finding.
#[must_use]
pub fn startup_timings() -> Vec<StartupTiming> {
    let mut out: Vec<StartupTiming> = {
        let state = STATE.lock();
        state
            .start_graph
            .iter()
            .map(|n| StartupTiming {
                service_id: n.service_id,
                name: n.name.clone(),
                level: n.level,
                started_at_ns: n.started_at_ns,
                ready_at_ns: n.ready_at_ns,
                ready_after_ns: match (n.started_at_ns, n.ready_at_ns) {
                    (Some(start), Some(ready)) => Some(ready.saturating_sub(start)),
                    _ => None,
                },
            })
            .collect()
    };
    // `None` sorts before `Some` under the derived ordering, so reversing puts
    // the slowest first and the never-ready ones at the end, which is the order
    // an operator reads: the blame list, then the stragglers.
    out.sort_by(|a, b| {
        b.ready_after_ns
            .cmp(&a.ready_after_ns)
            .then_with(|| a.name.cmp(&b.name))
    });
    out
}

/// Get crash records for display.
pub fn crash_records() -> Vec<CrashInfo> {
    let state = STATE.lock();
    state
        .crash_records
        .iter()
        .map(|r| CrashInfo {
            name: r.name.clone(),
            consecutive_failures: r.consecutive_failures,
            total_crashes: r.total_crashes,
            backoff_ms: r.current_backoff_ns / 1_000_000,
            permanently_failed: r.permanently_failed,
            last_crash_ns: r.last_crash_ns,
            restart_pending: r.pending_restart.is_some(),
            crash_history: r.crash_history.clone(),
        })
        .collect()
}

/// Generate content for /proc/svcstart.
pub fn procfs_content() -> String {
    let st = stats();
    let mut out = String::with_capacity(1024);

    out.push_str("Service Startup Orchestrator\n");
    out.push_str("============================\n");
    out.push_str(&format!("Phase:           {}\n", st.phase.label()));
    out.push_str(&format!("Max dep level:   {}\n", st.max_level));
    out.push_str(&format!("Graph size:      {} services\n", st.graph_size));
    out.push_str(&format!("Svc started:     {}\n", st.services_started));
    out.push_str(&format!("Apps launched:   {}\n", st.apps_launched));
    out.push_str(&format!("Total restarts:  {}\n", st.total_restarts));
    out.push_str(&format!("Max retries:     {}\n", st.max_retries));
    out.push_str(&format!("Init backoff:    {} ms\n", st.initial_backoff_ms));
    out.push_str(&format!("Max backoff:     {} ms\n", st.max_backoff_ms));

    if st.boot_end_ns > st.boot_start_ns {
        let boot_ms = (st.boot_end_ns.saturating_sub(st.boot_start_ns)) / 1_000_000;
        out.push_str(&format!("Boot time:       {} ms\n", boot_ms));
    }

    // Start levels.
    let levels = start_levels();
    if !levels.is_empty() {
        out.push_str("\nStart Levels:\n");
        for (i, level) in levels.iter().enumerate() {
            let names: Vec<&str> = level.iter().map(|(_, n)| n.as_str()).collect();
            out.push_str(&format!("  Level {}: {}\n", i, names.join(", ")));
        }
    }

    // Per-service boot timings, slowest first.
    let timings = startup_timings();
    if !timings.is_empty() {
        out.push_str("\nStartup Timings (slowest first):\n");
        out.push_str(&format!(
            "  {:16} {:>6} {:>12} {:>12}\n",
            "Service", "Level", "Started", "Ready after"
        ));
        for t in &timings {
            let started = match t.started_at_ns {
                Some(ns) => format!("{} ms", ns / 1_000_000),
                None => String::from("not started"),
            };
            let ready = match t.ready_after_ns {
                Some(ns) => format!("{} ms", ns / 1_000_000),
                None => String::from("never ready"),
            };
            out.push_str(&format!(
                "  {:16} {:>6} {:>12} {:>12}\n",
                t.name, t.level, started, ready
            ));
        }
    }

    // Crash records.
    let crashes = crash_records();
    if !crashes.is_empty() {
        let now = crate::hpet::elapsed_ns();
        out.push_str(&format!("\nCrash Records ({}):\n", crashes.len()));
        out.push_str(&format!(
            "  {:16} {:>6} {:>8} {:>8} {:>10}\n",
            "Service", "Consec", "Total", "Backoff", "Status"
        ));
        for c in &crashes {
            // "restarting" is distinct from "active": it says the backoff is
            // running right now, which is the state an operator watching a
            // flapping service most needs to see and previously could not.
            let status = if c.permanently_failed {
                "FAILED"
            } else if c.restart_pending {
                "restarting"
            } else {
                "active"
            };
            out.push_str(&format!(
                "  {:16} {:>6} {:>8} {:>5} ms {:>10}\n",
                c.name, c.consecutive_failures, c.total_crashes, c.backoff_ms, status
            ));
            let ages = c.history_ages(now);
            if !ages.is_empty() {
                out.push_str(&format!("  {:16} crashed: {} ago\n", "", ages));
            }
        }
    }

    // Startup apps.
    let apps = list_startup_apps();
    if !apps.is_empty() {
        out.push_str(&format!("\nStartup Apps ({}):\n", apps.len()));
        out.push_str(&format!(
            "  {:>3} {:20} {:30} {:>5} {:>7}\n",
            "Ord", "Name", "Path", "Wait", "Enabled"
        ));
        for app in &apps {
            out.push_str(&format!(
                "  {:>3} {:20} {:30} {:>5} {:>7}\n",
                app.order,
                app.display_name,
                app.path,
                if app.wait_for_ready { "yes" } else { "no" },
                if app.enabled { "yes" } else { "no" }
            ));
        }
    }

    out
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run the module's self-test suite against state of its own.
///
/// The suite mutates module state and asserts exact contents, and it used to
/// do that to the *live* state -- which, since it is also a kernel-shell
/// subcommand, changed or destroyed whatever the user had here and then
/// reported success.  It is moved aside for the duration and put back
/// afterwards; `crate::fs::selftest` records why this shape rather than the
/// alternatives.
///
/// Each pristine value is the `static`'s own initialiser, which is the one
/// spelling of "what a fresh boot holds" that cannot drift away from it.
///
/// The suite needs an empty *service manager* too — it calls `init_defaults`
/// and then asserts the exact set of services that produces — and it used to
/// get one by calling `servicemgr::clear_all()`, which deregisters every
/// service on the machine.  `with_pristine` would not have caught that: its
/// guarantee is that *this* module's state comes back, and it says nothing
/// about anything the suite reaches into.  So the service table is moved aside
/// as well, by [`servicemgr::with_pristine_state`].
/// A backoff restart is a `ktimer`, and a timer is not part of `STATE`: it
/// lives in the global timer table, so restoring the state on the way out
/// leaves it armed, pointing at a service id from the *pristine* registry that
/// no longer exists — or worse, one that has been reused by the real registry
/// the restore brings back. `with_pristine` cannot know about it, exactly as
/// it cannot know about a module's lock-free counter mirror. Disarm what the
/// suite armed, and do it inside the pristine window while the ids still mean
/// what the suite thinks they mean.
pub fn self_test() -> KernelResult<()> {
    servicemgr::with_pristine_state(|| {
        crate::fs::selftest::with_pristine(&STATE, State::new(), || {
            let result = self_test_inner();
            let cancelled = cancel_pending_restarts();
            crate::serial_println!(
                "[svcstart]   cleanup: disarmed {} pending backoff restart(s)",
                cancelled
            );
            result
        })
    })
}

fn self_test_inner() -> KernelResult<()> {
    crate::serial_println!("[svcstart] Running service startup self-tests...");

    // Clean slate.
    {
        let mut state = STATE.lock();
        *state = State::new();
    }
    servicemgr::clear_all();
    servicemgr::init_defaults();
    init();

    // Test 1: Resolve dependencies with no deps (all level 0).
    resolve_dependencies()?;
    {
        let state = STATE.lock();
        // All default services have no deps, so all should be level 0.
        for node in &state.start_graph {
            if node.level != 0 {
                crate::serial_println!(
                    "[svcstart]   FAIL: expected level 0 for '{}', got {}",
                    node.name,
                    node.level
                );
                return Err(KernelError::InternalError);
            }
        }
    }
    crate::serial_println!("[svcstart]   1. No-dep graph (all level 0): OK");

    // Test 2: Add dependencies and re-resolve.
    // audio depends on network → audio = level 1.
    {
        let audio = servicemgr::find_by_name("audio")?;
        servicemgr::add_dependency(audio.id, "network")?;
    }
    resolve_dependencies()?;
    {
        let state = STATE.lock();
        let net_node = state.start_graph.iter().find(|n| n.name == "network");
        let audio_node = state.start_graph.iter().find(|n| n.name == "audio");
        if let (Some(net), Some(audio)) = (net_node, audio_node) {
            if net.level != 0 || audio.level != 1 {
                crate::serial_println!(
                    "[svcstart]   FAIL: expected net=0 audio=1, got net={} audio={}",
                    net.level,
                    audio.level
                );
                return Err(KernelError::InternalError);
            }
        } else {
            crate::serial_println!("[svcstart]   FAIL: missing nodes");
            return Err(KernelError::InternalError);
        }
    }
    crate::serial_println!("[svcstart]   2. Dependency levels: OK");

    // Test 3: Start levels grouping.
    let levels = start_levels();
    if levels.len() < 2 {
        crate::serial_println!(
            "[svcstart]   FAIL: expected at least 2 levels, got {}",
            levels.len()
        );
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[svcstart]   3. Start levels ({}): OK", levels.len());

    // Test 4: Crash restart with backoff.
    {
        let net = servicemgr::find_by_name("network")?;
        let delay1 = report_crash(net.id)?;
        // First crash → initial backoff (1s = 1_000_000_000 ns).
        if delay1 != DEFAULT_INITIAL_BACKOFF_NS {
            crate::serial_println!(
                "[svcstart]   FAIL: expected initial backoff {} ns, got {}",
                DEFAULT_INITIAL_BACKOFF_NS,
                delay1
            );
            return Err(KernelError::InternalError);
        }

        // Second crash → 2x backoff.
        let delay2 = report_crash(net.id)?;
        let expected2 = DEFAULT_INITIAL_BACKOFF_NS.saturating_mul(2);
        if delay2 != expected2 {
            crate::serial_println!(
                "[svcstart]   FAIL: expected 2x backoff {} ns, got {}",
                expected2,
                delay2
            );
            return Err(KernelError::InternalError);
        }

        // The delay must be *armed*, not merely returned. Checking only the
        // return value is how this went wrong: the number was correct, and
        // the code beneath the log line that announced it restarted the
        // service immediately anyway. A test that inspects the computation
        // rather than the effect keeps inert machinery green indefinitely.
        let records = crash_records();
        let net_record = records
            .iter()
            .find(|r| r.name == "network")
            .ok_or(KernelError::InternalError)?;
        if !net_record.restart_pending {
            crate::serial_println!(
                "[svcstart]   FAIL: backoff computed but no restart timer armed"
            );
            return Err(KernelError::InternalError);
        }
        if net_record.last_crash_ns.is_none() {
            crate::serial_println!(
                "[svcstart]   FAIL: last_crash_ns is None after a crash report \
                 (`0` is a legal uptime, not `never`)"
            );
            return Err(KernelError::InternalError);
        }
        // Two crashes have been reported, so both must be in the history, in
        // order, with the newest agreeing with `last_crash_ns`. The history was
        // collected but never read by anything for as long as it existed;
        // checking it here is what stops it silently going stale again.
        if net_record.crash_history.len() != 2 {
            crate::serial_println!(
                "[svcstart]   FAIL: crash_history has {} entries after 2 crashes",
                net_record.crash_history.len()
            );
            return Err(KernelError::InternalError);
        }
        if net_record
            .crash_history
            .windows(2)
            .any(|w| matches!((w.first(), w.get(1)), (Some(a), Some(b)) if a > b))
        {
            crate::serial_println!("[svcstart]   FAIL: crash_history is not in time order");
            return Err(KernelError::InternalError);
        }
        if net_record.crash_history.last().copied() != net_record.last_crash_ns {
            crate::serial_println!(
                "[svcstart]   FAIL: newest crash_history entry disagrees with last_crash_ns"
            );
            return Err(KernelError::InternalError);
        }
        // And it must render: an empty string here would mean the display line
        // is silently skipped for a service that has in fact crashed.
        if net_record
            .history_ages(crate::hpet::elapsed_ns())
            .is_empty()
        {
            crate::serial_println!("[svcstart]   FAIL: history_ages empty for a crashed service");
            return Err(KernelError::InternalError);
        }
    }
    crate::serial_println!("[svcstart]   4. Exponential backoff: OK");

    // Test 5: Max retries leads to permanent failure.
    {
        let net = servicemgr::find_by_name("network")?;
        // Already 2 crashes. Need max_retries - 2 more to hit the limit.
        for _ in 0..(DEFAULT_MAX_RETRIES.saturating_sub(2)) {
            let _ = report_crash(net.id);
        }
        // Next one should exceed max_retries.
        let result = report_crash(net.id);
        if result.is_ok() {
            crate::serial_println!(
                "[svcstart]   FAIL: expected permanent failure after max retries"
            );
            return Err(KernelError::InternalError);
        }
        // Verify it's marked permanently failed.  `ok_or` rather than
        // `if let`: an absent record used to pass this test silently, so the
        // one outcome that would mean the crash tracking had stopped working
        // entirely was indistinguishable from success.
        let records = crash_records();
        let net_record = records
            .iter()
            .find(|r| r.name == "network")
            .ok_or(KernelError::InternalError)?;
        if !net_record.permanently_failed {
            crate::serial_println!("[svcstart]   FAIL: expected permanently_failed=true");
            return Err(KernelError::InternalError);
        }
        // Giving up on a service must also disarm the restart it was waiting
        // on. Otherwise the timer armed by the *previous* crash fires after
        // the verdict and brings the service back, which would make
        // "permanently failed" mean nothing at all.
        if net_record.restart_pending {
            crate::serial_println!(
                "[svcstart]   FAIL: permanently-failed service still has a restart armed"
            );
            return Err(KernelError::InternalError);
        }
        // The history is a ring of the last 10, not an unbounded log: a
        // service that crash-loops for a week must not grow the record without
        // limit.
        if net_record.crash_history.len() > 10 {
            crate::serial_println!(
                "[svcstart]   FAIL: crash_history grew past its 10-entry cap ({})",
                net_record.crash_history.len()
            );
            return Err(KernelError::InternalError);
        }
    }
    crate::serial_println!("[svcstart]   5. Max retries → permanent failure: OK");

    // Test 6: Reset crash count.
    {
        let net = servicemgr::find_by_name("network")?;
        reset_crash_count(net.id);
        let records = crash_records();
        let net_record = records
            .iter()
            .find(|r| r.name == "network")
            .ok_or(KernelError::InternalError)?;
        if net_record.consecutive_failures != 0 || net_record.permanently_failed {
            crate::serial_println!("[svcstart]   FAIL: crash count not reset");
            return Err(KernelError::InternalError);
        }
        if net_record.restart_pending {
            crate::serial_println!("[svcstart]   FAIL: reset_crash_count left a restart armed");
            return Err(KernelError::InternalError);
        }
    }
    crate::serial_println!("[svcstart]   6. Reset crash count: OK");

    // Test 7: Startup app list.
    let id1 = add_startup_app("/usr/bin/filemanager", "", "File Manager", false);
    let id2 = add_startup_app("/usr/bin/terminal", "--login", "Terminal", true);
    {
        let apps = list_startup_apps();
        if apps.len() != 2 {
            crate::serial_println!("[svcstart]   FAIL: expected 2 apps, got {}", apps.len());
            return Err(KernelError::InternalError);
        }
    }
    crate::serial_println!("[svcstart]   7. Add startup apps: OK");

    // Test 8: Toggle and remove.
    toggle_startup_app(id1)?;
    {
        let apps = list_startup_apps();
        let app = apps.iter().find(|a| a.id == id1);
        if let Some(a) = app {
            if a.enabled {
                crate::serial_println!("[svcstart]   FAIL: app should be disabled");
                return Err(KernelError::InternalError);
            }
        }
    }
    remove_startup_app(id2)?;
    {
        let apps = list_startup_apps();
        if apps.len() != 1 {
            crate::serial_println!(
                "[svcstart]   FAIL: expected 1 app after remove, got {}",
                apps.len()
            );
            return Err(KernelError::InternalError);
        }
    }
    crate::serial_println!("[svcstart]   8. Toggle/remove apps: OK");

    // Test 9: Signal ready.
    //
    // The `if let Some(node)` this used to be had no `else`: a graph missing
    // the node entirely — the failure mode that `resolve_dependencies` going
    // wrong would actually produce — skipped the assertion and reported OK.
    {
        resolve_dependencies()?;
        let net = servicemgr::find_by_name("network")?;

        // Before signalling: never-started, never-ready. This is the state the
        // old `0` encoding could not express, so assert it explicitly.
        let before = startup_timings();
        let net_before = before
            .iter()
            .find(|t| t.service_id == net.id)
            .ok_or(KernelError::InternalError)?;
        if net_before.ready_at_ns.is_some() || net_before.ready_after_ns.is_some() {
            crate::serial_println!(
                "[svcstart]   FAIL: a freshly resolved node already claims a ready time"
            );
            return Err(KernelError::InternalError);
        }

        signal_ready(net.id);

        let state = STATE.lock();
        let node = state
            .start_graph
            .iter()
            .find(|n| n.service_id == net.id)
            .ok_or(KernelError::InternalError)?;
        if !node.ready {
            crate::serial_println!("[svcstart]   FAIL: service not marked ready");
            return Err(KernelError::InternalError);
        }
        if node.ready_at_ns.is_none() {
            crate::serial_println!("[svcstart]   FAIL: ready service recorded no ready time");
            return Err(KernelError::InternalError);
        }
    }
    crate::serial_println!("[svcstart]   9. Signal ready: OK");

    // Test 9b: startup timings ordering.
    //
    // `signal_ready` above set a ready time but not a start time, so the node
    // has no duration and must sort into the never-ready tail rather than to
    // the top of the blame list. Getting this backwards would put the service
    // that told us the least at the head of the report.
    {
        let timings = startup_timings();
        if timings.is_empty() {
            crate::serial_println!("[svcstart]   FAIL: resolved graph produced no timings");
            return Err(KernelError::InternalError);
        }
        let mut prev: Option<Option<u64>> = None;
        for t in &timings {
            if let Some(p) = prev {
                // Descending, with `None` last: a `Some` after a `None` is the
                // ordering inversion this checks for.
                if p.is_none() && t.ready_after_ns.is_some() {
                    crate::serial_println!(
                        "[svcstart]   FAIL: '{}' has a duration but sorts after one that does not",
                        t.name
                    );
                    return Err(KernelError::InternalError);
                }
                if let (Some(pv), Some(tv)) = (p, t.ready_after_ns) {
                    if tv > pv {
                        crate::serial_println!(
                            "[svcstart]   FAIL: timings not sorted slowest-first at '{}'",
                            t.name
                        );
                        return Err(KernelError::InternalError);
                    }
                }
            }
            prev = Some(t.ready_after_ns);
        }
    }
    crate::serial_println!("[svcstart]   9b. Startup timings ordering: OK");

    // Test 10: Stats and procfs content.
    //
    // This used to be `if !st.phase.label().is_empty() { }` — an empty body,
    // so the one property it named was never actually required to hold.
    let st = stats();
    if st.phase.label().is_empty() {
        crate::serial_println!("[svcstart]   FAIL: boot phase has no label");
        return Err(KernelError::InternalError);
    }
    if st.graph_size == 0 {
        crate::serial_println!("[svcstart]   FAIL: stats report an empty graph after resolve");
        return Err(KernelError::InternalError);
    }
    let content = procfs_content();
    if content.is_empty() {
        crate::serial_println!("[svcstart]   FAIL: procfs_content is empty");
        return Err(KernelError::InternalError);
    }
    // The timings section must actually reach the operator, not merely exist as
    // an accessor no view calls — which is the defect this whole change fixes.
    if !content.contains("Startup Timings") {
        crate::serial_println!("[svcstart]   FAIL: procfs output omits the startup timings");
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[svcstart]   10. Stats and procfs: OK");

    // No clean-up: `self_test` runs this against substitutes for both this
    // module's state and the service manager's, and both are dropped on the way
    // out along with everything registered above.

    crate::serial_println!("[svcstart] All 10 self-tests passed.");
    Ok(())
}
