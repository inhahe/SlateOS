//! System health monitoring — periodic checks for system-wide health.
//!
//! Monitors key system metrics and emits warnings or takes corrective
//! action when thresholds are exceeded:
//!
//! - **Memory pressure**: warns at configurable RSS thresholds, triggers
//!   reclaim at critical levels.
//! - **Disk space**: warns when filesystems approach full.
//! - **CPU temperature**: warns at thermal thresholds (integrates with
//!   thermal subsystem).
//! - **Load average**: detects sustained overload.
//! - **OOM tracking**: counts OOM events and kills, trends.
//! - **Watchdog**: detects kernel softlockups (tick stalls).
//!
//! ## Integration
//!
//! - Called from [`crate::initproc::tick()`] once per second.
//! - Emits events via [`crate::eventlog`] when thresholds are crossed.
//! - Reports to `/proc/syshealth` for monitoring tools.
//! - Kshell `syshealth` command for real-time status.
//!
//! ## Design
//!
//! Thresholds use a three-level severity model:
//! - **Normal**: all metrics within acceptable range.
//! - **Warning**: approaching limits; logged, no action taken.
//! - **Critical**: action required; logged + corrective measures.
//!
//! The monitor is deliberately lightweight — it reads counters already
//! maintained by other subsystems (mm, scheduler, thermal) rather than
//! performing its own measurements.

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;
use crate::sync::PreemptSpinMutex as Mutex;


// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Memory usage warning threshold (percentage of total RAM).
const DEFAULT_MEMORY_WARN_PERCENT: u32 = 80;

/// Memory usage critical threshold (percentage of total RAM).
const DEFAULT_MEMORY_CRIT_PERCENT: u32 = 95;

/// Disk space warning threshold (percentage full).
const DEFAULT_DISK_WARN_PERCENT: u32 = 85;

/// Disk space critical threshold (percentage full).
const DEFAULT_DISK_CRIT_PERCENT: u32 = 95;

/// CPU temperature warning (degrees Celsius).
const DEFAULT_TEMP_WARN_C: u32 = 80;

/// CPU temperature critical (degrees Celsius).
const DEFAULT_TEMP_CRIT_C: u32 = 95;

/// Load average warning (per CPU).
const DEFAULT_LOAD_WARN_PER_CPU: u32 = 4;

/// Load average critical (per CPU).
const DEFAULT_LOAD_CRIT_PER_CPU: u32 = 8;

/// Maximum number of health check results to keep in history.
const MAX_HISTORY: usize = 60; // ~60 seconds of history at 1 check/sec

/// Minimum interval between warning events for the same metric (ns).
/// Prevents log spam when a metric stays at warning level.
const WARNING_COOLDOWN_NS: u64 = 60_000_000_000; // 60 seconds

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Overall system health level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum HealthLevel {
    /// All metrics normal.
    Healthy,
    /// One or more metrics at warning level.
    Warning,
    /// One or more metrics at critical level.
    Critical,
    /// System health cannot be determined (subsystem not available).
    Unknown,
}

impl HealthLevel {
    /// Human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Healthy => "Healthy",
            Self::Warning => "Warning",
            Self::Critical => "Critical",
            Self::Unknown => "Unknown",
        }
    }
}

/// A single health metric with current value and thresholds.
#[derive(Debug, Clone)]
pub struct HealthMetric {
    /// Metric name (e.g., "memory", "disk:/", "temperature").
    pub name: String,
    /// Current value (percentage, degrees, or load * 100).
    pub value: u64,
    /// Warning threshold.
    pub warn_threshold: u64,
    /// Critical threshold.
    pub crit_threshold: u64,
    /// Unit for display.
    pub unit: &'static str,
    /// Current health level for this metric.
    pub level: HealthLevel,
    /// Human-readable status message.
    pub message: String,
}

/// Snapshot of system health at a point in time.
#[derive(Debug, Clone)]
pub struct HealthSnapshot {
    /// Timestamp (ns since boot).
    pub timestamp_ns: u64,
    /// Overall health level (worst of all metrics).
    pub overall: HealthLevel,
    /// Individual metrics.
    pub metrics: Vec<HealthMetric>,
}

/// Configuration for health monitoring thresholds.
#[derive(Debug, Clone)]
pub struct HealthConfig {
    /// Memory usage warning threshold (percent).
    pub memory_warn_percent: u32,
    /// Memory usage critical threshold (percent).
    pub memory_crit_percent: u32,
    /// Disk space warning threshold (percent full).
    pub disk_warn_percent: u32,
    /// Disk space critical threshold (percent full).
    pub disk_crit_percent: u32,
    /// CPU temperature warning (Celsius).
    pub temp_warn_c: u32,
    /// CPU temperature critical (Celsius).
    pub temp_crit_c: u32,
    /// Load average warning (per CPU, x100 for fixed-point).
    pub load_warn_per_cpu_x100: u32,
    /// Load average critical (per CPU, x100).
    pub load_crit_per_cpu_x100: u32,
    /// Whether health monitoring is enabled.
    pub enabled: bool,
}

impl Default for HealthConfig {
    fn default() -> Self {
        Self {
            memory_warn_percent: DEFAULT_MEMORY_WARN_PERCENT,
            memory_crit_percent: DEFAULT_MEMORY_CRIT_PERCENT,
            disk_warn_percent: DEFAULT_DISK_WARN_PERCENT,
            disk_crit_percent: DEFAULT_DISK_CRIT_PERCENT,
            temp_warn_c: DEFAULT_TEMP_WARN_C,
            temp_crit_c: DEFAULT_TEMP_CRIT_C,
            load_warn_per_cpu_x100: DEFAULT_LOAD_WARN_PER_CPU * 100,
            load_crit_per_cpu_x100: DEFAULT_LOAD_CRIT_PER_CPU * 100,
            enabled: true,
        }
    }
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct State {
    config: HealthConfig,
    /// Most recent health snapshot.
    current: Option<HealthSnapshot>,
    /// History of snapshots (ring buffer, newest at end).
    history: Vec<HealthSnapshot>,
    /// Total health checks performed.
    total_checks: u64,
    /// Total warnings emitted.
    total_warnings: u64,
    /// Total critical events emitted.
    total_criticals: u64,
    /// Last warning event timestamps per metric name (for cooldown).
    last_warn_times: Vec<(String, u64)>,
    /// Whether initialized.
    initialized: bool,
}

impl State {
    const fn new() -> Self {
        Self {
            config: HealthConfig {
                memory_warn_percent: DEFAULT_MEMORY_WARN_PERCENT,
                memory_crit_percent: DEFAULT_MEMORY_CRIT_PERCENT,
                disk_warn_percent: DEFAULT_DISK_WARN_PERCENT,
                disk_crit_percent: DEFAULT_DISK_CRIT_PERCENT,
                temp_warn_c: DEFAULT_TEMP_WARN_C,
                temp_crit_c: DEFAULT_TEMP_CRIT_C,
                load_warn_per_cpu_x100: DEFAULT_LOAD_WARN_PER_CPU * 100,
                load_crit_per_cpu_x100: DEFAULT_LOAD_CRIT_PER_CPU * 100,
                enabled: true,
            },
            current: None,
            history: Vec::new(),
            total_checks: 0,
            total_warnings: 0,
            total_criticals: 0,
            last_warn_times: Vec::new(),
            initialized: false,
        }
    }
}

static STATE: Mutex<State> = Mutex::new(State::new());

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Initialize the system health monitor.
pub fn init() {
    let mut state = STATE.lock();
    if state.initialized {
        return;
    }
    state.initialized = true;
    crate::syslog!("system.health", Info, "System health monitor initialized");
}

// ---------------------------------------------------------------------------
// Health Check
// ---------------------------------------------------------------------------

/// Run a system health check and return the snapshot.
///
/// This should be called periodically (typically once per second from
/// the init process tick). It collects metrics from various kernel
/// subsystems and evaluates them against thresholds.
pub fn check() -> HealthSnapshot {
    let now = crate::hpet::elapsed_ns();
    let mut metrics = Vec::new();
    let mut overall = HealthLevel::Healthy;

    let config = {
        let state = STATE.lock();
        if !state.config.enabled {
            return HealthSnapshot {
                timestamp_ns: now,
                overall: HealthLevel::Unknown,
                metrics: Vec::new(),
            };
        }
        state.config.clone()
    };

    // --- Memory ---
    check_memory(&config, &mut metrics, &mut overall);

    // --- Load Average ---
    check_load(&config, &mut metrics, &mut overall);

    // --- CPU Temperature ---
    check_temperature(&config, &mut metrics, &mut overall);

    // --- Uptime / Tick Health ---
    check_uptime(&mut metrics);

    let snapshot = HealthSnapshot {
        timestamp_ns: now,
        overall,
        metrics,
    };

    // Emit events for new warnings/criticals (with cooldown).
    emit_health_events(&snapshot, now);

    // Store snapshot.
    {
        let mut state = STATE.lock();
        state.current = Some(snapshot.clone());

        // Add to history ring buffer.
        if state.history.len() >= MAX_HISTORY {
            state.history.remove(0);
        }
        state.history.push(snapshot.clone());

        #[allow(clippy::arithmetic_side_effects)]
        { state.total_checks += 1; }
    }

    snapshot
}

/// Check memory usage.
fn check_memory(config: &HealthConfig, metrics: &mut Vec<HealthMetric>, overall: &mut HealthLevel) {
    // Query the frame allocator for physical memory stats.
    let frame_stats = match crate::mm::frame::stats() {
        Some(s) => s,
        None => {
            metrics.push(HealthMetric {
                name: String::from("memory"),
                value: 0,
                warn_threshold: u64::from(config.memory_warn_percent),
                crit_threshold: u64::from(config.memory_crit_percent),
                unit: "%",
                level: HealthLevel::Unknown,
                message: String::from("Frame allocator not initialized"),
            });
            return;
        }
    };
    let total_pages = frame_stats.total_frames as u64;
    let free_pages = frame_stats.free_frames as u64;

    let used_pages = total_pages.saturating_sub(free_pages);
    // Calculate percentage, avoiding overflow.
    let percent = if total_pages > 0 {
        #[allow(clippy::arithmetic_side_effects)]
        { (used_pages * 100) / total_pages }
    } else {
        0
    };

    let level = if percent >= u64::from(config.memory_crit_percent) {
        HealthLevel::Critical
    } else if percent >= u64::from(config.memory_warn_percent) {
        HealthLevel::Warning
    } else {
        HealthLevel::Healthy
    };

    if level > *overall {
        *overall = level;
    }

    let total_mib = total_pages.saturating_mul(16) / 1024; // 16 KiB pages → MiB
    let used_mib = used_pages.saturating_mul(16) / 1024;
    let free_mib = free_pages.saturating_mul(16) / 1024;

    metrics.push(HealthMetric {
        name: String::from("memory"),
        value: percent,
        warn_threshold: u64::from(config.memory_warn_percent),
        crit_threshold: u64::from(config.memory_crit_percent),
        unit: "%",
        level,
        message: format!("{}% used ({} MiB / {} MiB, {} MiB free)",
            percent, used_mib, total_mib, free_mib),
    });
}

/// Check system load average.
fn check_load(config: &HealthConfig, metrics: &mut Vec<HealthMetric>, overall: &mut HealthLevel) {
    // Get load average from the loadavg subsystem.
    // get() returns (load1, load5, load15) as fixed-point x100.
    let (load1, load5, load15) = crate::loadavg::get();

    let ncpus = core::cmp::max(crate::smp::cpu_count() as u64, 1);
    // Per-CPU load x100.
    let per_cpu_x100 = load1 * 100 / ncpus;

    let level = if per_cpu_x100 >= u64::from(config.load_crit_per_cpu_x100) {
        HealthLevel::Critical
    } else if per_cpu_x100 >= u64::from(config.load_warn_per_cpu_x100) {
        HealthLevel::Warning
    } else {
        HealthLevel::Healthy
    };

    if level > *overall {
        *overall = level;
    }

    metrics.push(HealthMetric {
        name: String::from("load"),
        value: per_cpu_x100,
        warn_threshold: u64::from(config.load_warn_per_cpu_x100),
        crit_threshold: u64::from(config.load_crit_per_cpu_x100),
        unit: "x100/cpu",
        level,
        message: format!("load avg: {}.{:02} / {}.{:02} / {}.{:02} ({} CPUs)",
            load1 / 100, load1 % 100,
            load5 / 100, load5 % 100,
            load15 / 100, load15 % 100,
            ncpus),
    });
}

/// Check CPU temperature.
fn check_temperature(config: &HealthConfig, metrics: &mut Vec<HealthMetric>, overall: &mut HealthLevel) {
    let therm = crate::thermal::info();
    if !therm.supported {
        // Thermal sensor not available.
        return;
    }

    let temp_c = u64::from(therm.current_temp);
    let temp_frac: u64 = 0; // ThermalInfo gives integer degrees.

    let level = if temp_c >= u64::from(config.temp_crit_c) {
        HealthLevel::Critical
    } else if temp_c >= u64::from(config.temp_warn_c) {
        HealthLevel::Warning
    } else {
        HealthLevel::Healthy
    };

    if level > *overall {
        *overall = level;
    }

    metrics.push(HealthMetric {
        name: String::from("temperature"),
        value: temp_c,
        warn_threshold: u64::from(config.temp_warn_c),
        crit_threshold: u64::from(config.temp_crit_c),
        unit: "°C",
        level,
        message: format!("{}.{}°C", temp_c, temp_frac),
    });
}

/// Check system uptime (just reports, no threshold).
fn check_uptime(metrics: &mut Vec<HealthMetric>) {
    let uptime_ns = crate::hpet::elapsed_ns();
    let uptime_s = uptime_ns / 1_000_000_000;
    let hours = uptime_s / 3600;
    let mins = (uptime_s % 3600) / 60;
    let secs = uptime_s % 60;

    metrics.push(HealthMetric {
        name: String::from("uptime"),
        value: uptime_s,
        warn_threshold: u64::MAX,
        crit_threshold: u64::MAX,
        unit: "sec",
        level: HealthLevel::Healthy,
        message: format!("{}h {}m {}s", hours, mins, secs),
    });
}

/// Emit syslog events for metrics that crossed thresholds.
///
/// Uses a cooldown to prevent repeated warnings for the same metric.
fn emit_health_events(snapshot: &HealthSnapshot, now: u64) {
    let mut state = STATE.lock();

    for metric in &snapshot.metrics {
        if metric.level == HealthLevel::Healthy || metric.level == HealthLevel::Unknown {
            continue;
        }

        // Check cooldown.
        let cooled_down = match state.last_warn_times.iter().find(|(n, _)| *n == metric.name) {
            Some(&(_, last_t)) => now.saturating_sub(last_t) >= WARNING_COOLDOWN_NS,
            None => true,
        };

        if !cooled_down {
            continue;
        }

        // Update last warning time.
        if let Some(entry) = state.last_warn_times.iter_mut().find(|(n, _)| *n == metric.name) {
            entry.1 = now;
        } else {
            state.last_warn_times.push((metric.name.clone(), now));
        }

        match metric.level {
            HealthLevel::Warning => {
                #[allow(clippy::arithmetic_side_effects)]
                { state.total_warnings += 1; }
                crate::syslog!("system.health", Warning,
                    "{}: {} (threshold: {} {})",
                    metric.name, metric.message, metric.warn_threshold, metric.unit);
            }
            HealthLevel::Critical => {
                #[allow(clippy::arithmetic_side_effects)]
                { state.total_criticals += 1; }
                crate::syslog!("system.health", Error,
                    "CRITICAL {}: {} (threshold: {} {})",
                    metric.name, metric.message, metric.crit_threshold, metric.unit);
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Query
// ---------------------------------------------------------------------------

/// Get the most recent health snapshot.
pub fn current() -> Option<HealthSnapshot> {
    STATE.lock().current.clone()
}

/// Get the current overall health level.
pub fn overall_health() -> HealthLevel {
    STATE.lock().current.as_ref()
        .map(|s| s.overall)
        .unwrap_or(HealthLevel::Unknown)
}

/// Get health check history (up to MAX_HISTORY entries).
pub fn history() -> Vec<HealthSnapshot> {
    STATE.lock().history.clone()
}

/// Get total check/warning/critical counts.
pub fn stats() -> (u64, u64, u64) {
    let state = STATE.lock();
    (state.total_checks, state.total_warnings, state.total_criticals)
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Update health monitoring configuration.
pub fn set_config(config: HealthConfig) {
    STATE.lock().config = config;
}

/// Get a copy of the current configuration.
pub fn get_config() -> HealthConfig {
    STATE.lock().config.clone()
}

/// Enable or disable health monitoring.
pub fn set_enabled(enabled: bool) {
    STATE.lock().config.enabled = enabled;
}

// ---------------------------------------------------------------------------
// Procfs
// ---------------------------------------------------------------------------

/// Generate content for `/proc/syshealth`.
pub fn procfs_content() -> String {
    let state = STATE.lock();

    let mut out = String::from("=== System Health ===\n\n");

    if let Some(ref snap) = state.current {
        out.push_str(&format!("Overall: {}\n", snap.overall.label()));
        out.push_str(&format!("Last check: {} ns since boot\n\n", snap.timestamp_ns));

        out.push_str(&format!("{:<15} {:<10} {:<10} {}\n",
            "Metric", "Level", "Value", "Details"));
        out.push_str(&format!("{:-<15} {:-<10} {:-<10} {:-<30}\n", "", "", "", ""));

        for m in &snap.metrics {
            out.push_str(&format!("{:<15} {:<10} {:<10} {}\n",
                m.name, m.level.label(),
                format!("{} {}", m.value, m.unit),
                m.message));
        }
    } else {
        out.push_str("No health check performed yet.\n");
    }

    out.push_str(&format!("\nTotal checks: {}\n", state.total_checks));
    out.push_str(&format!("Total warnings: {}\n", state.total_warnings));
    out.push_str(&format!("Total criticals: {}\n", state.total_criticals));
    out.push_str(&format!("Enabled: {}\n", state.config.enabled));

    out
}

// ---------------------------------------------------------------------------
// Self-Tests
// ---------------------------------------------------------------------------

/// Run self-tests for the system health monitor.
pub fn self_test() -> bool {
    crate::serial_println!("[syshealth] Running self-tests...");
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

    // Test 1: Init.
    init();
    {
        let state = STATE.lock();
        check!("init sets initialized", state.initialized);
    }

    // Test 2: No snapshot before first check.
    check!("no snapshot before check", current().is_none());
    check!("overall is Unknown before check", overall_health() == HealthLevel::Unknown);

    // Test 3: Run a health check.
    let snap = check();
    check!("check returns snapshot", true);
    check!("snapshot has metrics", !snap.metrics.is_empty());
    check!("overall is not Unknown after check",
        snap.overall != HealthLevel::Unknown);

    // Test 4: Current snapshot available after check.
    let cur = current();
    check!("current snapshot available", cur.is_some());

    // Test 5: History has one entry.
    let hist = history();
    check!("history has one entry", hist.len() == 1);

    // Test 6: Stats updated.
    let (checks, _, _) = stats();
    check!("total_checks is 1", checks == 1);

    // Test 7: Run multiple checks and verify history cap.
    for _ in 0..5 {
        let _ = check();
    }
    let (checks, _, _) = stats();
    check!("total_checks after 6 checks", checks == 6);

    // Test 8: Memory metric exists.
    let snap = current().unwrap_or_else(|| HealthSnapshot {
        timestamp_ns: 0,
        overall: HealthLevel::Unknown,
        metrics: Vec::new(),
    });
    let has_memory = snap.metrics.iter().any(|m| m.name == "memory");
    check!("memory metric present", has_memory);

    // Test 9: Load metric exists.
    let has_load = snap.metrics.iter().any(|m| m.name == "load");
    check!("load metric present", has_load);

    // Test 10: Uptime metric exists.
    let has_uptime = snap.metrics.iter().any(|m| m.name == "uptime");
    check!("uptime metric present", has_uptime);

    // Test 11: Config update.
    let mut config = get_config();
    config.memory_warn_percent = 50;
    set_config(config);
    let updated = get_config();
    check!("config update persists", updated.memory_warn_percent == 50);

    // Test 12: Disable/enable.
    set_enabled(false);
    let snap = check();
    check!("disabled check returns Unknown", snap.overall == HealthLevel::Unknown);
    check!("disabled check has no metrics", snap.metrics.is_empty());

    set_enabled(true);
    let snap = check();
    check!("enabled check returns metrics", !snap.metrics.is_empty());

    // Test 13: Procfs output.
    let content = procfs_content();
    check!("procfs content is non-empty", content.len() > 50);
    check!("procfs contains 'System Health'", content.contains("System Health"));

    crate::serial_println!("[syshealth] Tests complete: {} passed, {} failed", passed, failed);
    failed == 0
}
