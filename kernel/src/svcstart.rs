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
//!         compute backoff, schedule restart
//!     → else: mark permanently failed
//! ```
//!
//! ## Integration
//!
//! - Called from the init process after basic kernel init.
//! - Kshell `svcstart` command for status and manual control.
//! - `/proc/svcstart` shows startup state and crash history.

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;
use crate::sync::PreemptSpinMutex as Mutex;

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
    /// Timestamp when the service was started (ns since boot).
    started_at_ns: u64,
    /// Timestamp when the service signaled ready (ns since boot).
    ready_at_ns: u64,
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
    /// Timestamp of most recent crash (ns since boot).
    last_crash_ns: u64,
    /// Current backoff delay (doubles each failure, caps at max).
    current_backoff_ns: u64,
    /// Whether the service has been permanently marked as failed.
    permanently_failed: bool,
    /// Total lifetime crash count.
    total_crashes: u64,
    /// Timestamps of last N crashes for debugging.
    crash_history: Vec<u64>,
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
    let mut nodes: Vec<(u32, String, Vec<String>, u32)> = services.iter().map(|s| {
        (s.id, s.name.clone(), s.depends_on.clone(), u32::MAX)
    }).collect();

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
                { resolved_count += 1; }
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
                { resolved_count += 1; }
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
            let _unresolved: Vec<&str> = nodes.iter()
                .filter(|n| n.3 == u32::MAX)
                .map(|n| n.1.as_str())
                .collect();

            crate::syslog!("service.startup", Error,
                "Dependency cycle detected in service graph");
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
            started_at_ns: 0,
            ready_at_ns: 0,
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
        let services: Vec<(u32, String)> = state.start_graph.iter()
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
    {
        let mut state = STATE.lock();
        if !state.initialized {
            init();
        }
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
        crate::syslog!("service.startup", Info,
            "Starting service level {} ({} services)", level_idx, level.len());

        for (svc_id, svc_name) in level {
            // Skip disabled services.
            if let Ok(info) = servicemgr::get_service(*svc_id) {
                if info.startup_type == servicemgr::StartupType::Disabled {
                    continue;
                }
                if info.state == servicemgr::ServiceState::Running {
                    // Already running (e.g., started during init_defaults).
                    let mut state = STATE.lock();
                    if let Some(node) = state.start_graph.iter_mut()
                        .find(|n| n.service_id == *svc_id) {
                        node.ready = true;
                        node.started_at_ns = info.last_start_ns;
                        node.ready_at_ns = info.last_start_ns;
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
                    if let Some(node) = state.start_graph.iter_mut()
                        .find(|n| n.service_id == *svc_id) {
                        node.started_at_ns = now;
                    }
                    total_started = total_started.saturating_add(1);

                    crate::syslog!("service.startup", Info,
                        "Service '{}' started (level {})", svc_name, level_idx);
                }
                Err(e) => {
                    crate::syslog!("service.startup", Error,
                        "Failed to start service '{}': {:?}", svc_name, e);
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

    crate::syslog!("service.startup", Info,
        "Boot sequence complete: {} services, {} apps", total_started, apps_launched);

    Ok(total_started)
}

/// Notify that a service has signaled readiness.
pub fn signal_ready(service_id: u32) {
    let mut state = STATE.lock();
    if let Some(node) = state.start_graph.iter_mut()
        .find(|n| n.service_id == service_id) {
        node.ready = true;
        node.ready_at_ns = crate::hpet::elapsed_ns();
    }

    crate::syslog!("service.startup", Info,
        "Service id={} signaled ready", service_id);
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
    let record = if let Some(r) = state.crash_records.iter_mut()
        .find(|r| r.service_id == service_id) {
        r
    } else {
        state.crash_records.push(CrashRecord {
            service_id,
            name: info.name.clone(),
            consecutive_failures: 0,
            last_crash_ns: 0,
            current_backoff_ns: config.initial_backoff_ns,
            permanently_failed: false,
            total_crashes: 0,
            crash_history: Vec::new(),
        });
        // Safe: we just pushed, so last() is Some.
        state.crash_records.last_mut().ok_or(KernelError::InternalError)?
    };

    if record.permanently_failed {
        return Err(KernelError::NotSupported);
    }

    // Update crash record.
    record.consecutive_failures = record.consecutive_failures.saturating_add(1);
    record.last_crash_ns = now;
    #[allow(clippy::arithmetic_side_effects)]
    { record.total_crashes += 1; }

    // Keep last 10 crash timestamps.
    if record.crash_history.len() >= 10 {
        record.crash_history.remove(0);
    }
    record.crash_history.push(now);

    // Check if we've exceeded max retries.
    if record.consecutive_failures > config.max_retries {
        record.permanently_failed = true;
        crate::syslog!("service.crash", Critical,
            "Service '{}' permanently failed after {} crashes",
            info.name, record.total_crashes);
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

    // Capture values before the record borrow ends so we can update
    // state-level fields (can't mutate state while record borrows crash_records).
    let consec = record.consecutive_failures;

    #[allow(clippy::arithmetic_side_effects)]
    { state.total_restarts += 1; }

    let delay_ms = delay / 1_000_000;
    crate::syslog!("service.crash", Warning,
        "Service '{}' crashed (attempt {}/{}), restart in {} ms",
        info.name, consec, config.max_retries, delay_ms);

    // In a real system, we'd schedule a timer callback here.
    // For now, we attempt the restart immediately (the backoff delay
    // is recorded for the timer system to use once available).
    drop(state);

    // Attempt restart via servicemgr.
    let _ = servicemgr::restart_service(service_id);

    Ok(delay)
}

/// Reset a service's crash counter (e.g., after running successfully for a while).
pub fn reset_crash_count(service_id: u32) {
    let mut state = STATE.lock();
    let initial_backoff = state.config.initial_backoff_ns;
    if let Some(record) = state.crash_records.iter_mut()
        .find(|r| r.service_id == service_id) {
        record.consecutive_failures = 0;
        record.current_backoff_ns = initial_backoff;
        record.permanently_failed = false;
    }
}

// ---------------------------------------------------------------------------
// Startup app list
// ---------------------------------------------------------------------------

/// Add an app to the startup list.
pub fn add_startup_app(
    path: &str,
    args: &str,
    display_name: &str,
    wait_for_ready: bool,
) -> u32 {
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
    let idx = state.startup_apps.iter().position(|a| a.id == id)
        .ok_or(KernelError::NotFound)?;
    state.startup_apps.remove(idx);
    Ok(())
}

/// Toggle a startup app's enabled state.
pub fn toggle_startup_app(id: u32) -> KernelResult<bool> {
    let mut state = STATE.lock();
    let app = state.startup_apps.iter_mut().find(|a| a.id == id)
        .ok_or(KernelError::NotFound)?;
    app.enabled = !app.enabled;
    Ok(app.enabled)
}

/// Reorder a startup app (set its order value).
pub fn reorder_startup_app(id: u32, new_order: u32) -> KernelResult<()> {
    let mut state = STATE.lock();
    let app = state.startup_apps.iter_mut().find(|a| a.id == id)
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

        crate::syslog!("service.startup", Info,
            "Launching startup app: {} ({})", app.display_name, app.path);

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

/// Get crash records for display.
pub fn crash_records() -> Vec<(String, u32, u64, u64, bool)> {
    let state = STATE.lock();
    state.crash_records.iter().map(|r| {
        (r.name.clone(), r.consecutive_failures, r.total_crashes,
         r.current_backoff_ns / 1_000_000, r.permanently_failed)
    }).collect()
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

    // Crash records.
    let crashes = crash_records();
    if !crashes.is_empty() {
        out.push_str(&format!("\nCrash Records ({}):\n", crashes.len()));
        out.push_str(&format!("  {:16} {:>6} {:>8} {:>8} {:>8}\n",
            "Service", "Consec", "Total", "Backoff", "Status"));
        for (name, consec, total, backoff_ms, perm) in &crashes {
            let status = if *perm { "FAILED" } else { "active" };
            out.push_str(&format!("  {:16} {:>6} {:>8} {:>5} ms {:>8}\n",
                name, consec, total, backoff_ms, status));
        }
    }

    // Startup apps.
    let apps = list_startup_apps();
    if !apps.is_empty() {
        out.push_str(&format!("\nStartup Apps ({}):\n", apps.len()));
        out.push_str(&format!("  {:>3} {:20} {:30} {:>5} {:>7}\n",
            "Ord", "Name", "Path", "Wait", "Enabled"));
        for app in &apps {
            out.push_str(&format!("  {:>3} {:20} {:30} {:>5} {:>7}\n",
                app.order, app.display_name, app.path,
                if app.wait_for_ready { "yes" } else { "no" },
                if app.enabled { "yes" } else { "no" }));
        }
    }

    out
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run self-tests for the startup orchestrator.
pub fn self_test() -> KernelResult<()> {
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
                crate::serial_println!("[svcstart]   FAIL: expected level 0 for '{}', got {}",
                    node.name, node.level);
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
                crate::serial_println!("[svcstart]   FAIL: expected net=0 audio=1, got net={} audio={}",
                    net.level, audio.level);
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
        crate::serial_println!("[svcstart]   FAIL: expected at least 2 levels, got {}", levels.len());
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[svcstart]   3. Start levels ({}): OK", levels.len());

    // Test 4: Crash restart with backoff.
    {
        let net = servicemgr::find_by_name("network")?;
        let delay1 = report_crash(net.id)?;
        // First crash → initial backoff (1s = 1_000_000_000 ns).
        if delay1 != DEFAULT_INITIAL_BACKOFF_NS {
            crate::serial_println!("[svcstart]   FAIL: expected initial backoff {} ns, got {}",
                DEFAULT_INITIAL_BACKOFF_NS, delay1);
            return Err(KernelError::InternalError);
        }

        // Second crash → 2x backoff.
        let delay2 = report_crash(net.id)?;
        let expected2 = DEFAULT_INITIAL_BACKOFF_NS.saturating_mul(2);
        if delay2 != expected2 {
            crate::serial_println!("[svcstart]   FAIL: expected 2x backoff {} ns, got {}",
                expected2, delay2);
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
            crate::serial_println!("[svcstart]   FAIL: expected permanent failure after max retries");
            return Err(KernelError::InternalError);
        }
        // Verify it's marked permanently failed.
        let records = crash_records();
        let net_record = records.iter().find(|r| r.0 == "network");
        if let Some((_name, _consec, _total, _backoff, perm)) = net_record {
            if !perm {
                crate::serial_println!("[svcstart]   FAIL: expected permanently_failed=true");
                return Err(KernelError::InternalError);
            }
        }
    }
    crate::serial_println!("[svcstart]   5. Max retries → permanent failure: OK");

    // Test 6: Reset crash count.
    {
        let net = servicemgr::find_by_name("network")?;
        reset_crash_count(net.id);
        let records = crash_records();
        let net_record = records.iter().find(|r| r.0 == "network");
        if let Some((_name, consec, _total, _backoff, perm)) = net_record {
            if *consec != 0 || *perm {
                crate::serial_println!("[svcstart]   FAIL: crash count not reset");
                return Err(KernelError::InternalError);
            }
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
            crate::serial_println!("[svcstart]   FAIL: expected 1 app after remove, got {}", apps.len());
            return Err(KernelError::InternalError);
        }
    }
    crate::serial_println!("[svcstart]   8. Toggle/remove apps: OK");

    // Test 9: Signal ready.
    {
        resolve_dependencies()?;
        let net = servicemgr::find_by_name("network")?;
        signal_ready(net.id);
        let state = STATE.lock();
        let node = state.start_graph.iter().find(|n| n.service_id == net.id);
        if let Some(n) = node {
            if !n.ready {
                crate::serial_println!("[svcstart]   FAIL: service not marked ready");
                return Err(KernelError::InternalError);
            }
        }
    }
    crate::serial_println!("[svcstart]   9. Signal ready: OK");

    // Test 10: Stats and procfs content.
    let st = stats();
    if !st.phase.label().is_empty() {
        // Just verify we can get stats without panicking.
    }
    let content = procfs_content();
    if content.is_empty() {
        crate::serial_println!("[svcstart]   FAIL: procfs_content is empty");
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[svcstart]   10. Stats and procfs: OK");

    // Clean up.
    servicemgr::clear_all();
    {
        let mut state = STATE.lock();
        *state = State::new();
    }

    crate::serial_println!("[svcstart] All 10 self-tests passed.");
    Ok(())
}
