//! Telemetry — system telemetry collection and reporting.
//!
//! Collects system health metrics, usage patterns, error rates,
//! and performance data. Supports metric registration, collection
//! intervals, and export.
//!
//! ## Architecture
//!
//! ```text
//! Telemetry collection
//!   → telemetry::register(name, type) → register metric
//!   → telemetry::record(name, value) → record data point
//!   → telemetry::query(name) → read metric
//!   → telemetry::export() → export all metrics
//!
//! Integration:
//!   → perfmon (performance monitor)
//!   → sysdiag (system diagnostics)
//!   → sysinfo (system information)
//!   → eventlog (event logging)
//! ```

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Metric type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetricType {
    Counter,     // Monotonically increasing.
    Gauge,       // Current value (can go up/down).
    Histogram,   // Distribution of values.
    Rate,        // Events per second.
}

impl MetricType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Counter => "Counter",
            Self::Gauge => "Gauge",
            Self::Histogram => "Histogram",
            Self::Rate => "Rate",
        }
    }
}

/// Metric category for grouping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetricCategory {
    System,
    Memory,
    Disk,
    Network,
    Process,
    Custom,
}

impl MetricCategory {
    pub fn label(self) -> &'static str {
        match self {
            Self::System => "System",
            Self::Memory => "Memory",
            Self::Disk => "Disk",
            Self::Network => "Network",
            Self::Process => "Process",
            Self::Custom => "Custom",
        }
    }
}

/// A registered metric.
#[derive(Debug, Clone)]
pub struct Metric {
    pub name: String,
    pub metric_type: MetricType,
    pub category: MetricCategory,
    pub value: u64,
    pub min_value: u64,
    pub max_value: u64,
    pub sample_count: u64,
    pub total_sum: u64,
    pub last_updated_ns: u64,
    pub unit: String,
    pub enabled: bool,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_METRICS: usize = 512;

struct State {
    metrics: Vec<Metric>,
    collection_enabled: bool,
    collection_interval_ms: u64,
    total_samples: u64,
    total_exports: u64,
    ops: u64,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);
static OPS: AtomicU64 = AtomicU64::new(0);

fn with_state<F, R>(f: F) -> KernelResult<R>
where
    F: FnOnce(&mut State) -> KernelResult<R>,
{
    let mut guard = STATE.lock();
    let state = guard.as_mut().ok_or(KernelError::NotSupported)?;
    state.ops += 1;
    OPS.store(state.ops, Ordering::Relaxed);
    f(state)
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    let now = crate::hpet::elapsed_ns();
    *guard = Some(State {
        metrics: alloc::vec![
            Metric {
                name: String::from("cpu.usage_pct"), metric_type: MetricType::Gauge,
                category: MetricCategory::System, value: 15, min_value: 0,
                max_value: 100, sample_count: 1, total_sum: 15,
                last_updated_ns: now, unit: String::from("%"), enabled: true,
            },
            Metric {
                name: String::from("mem.used_mb"), metric_type: MetricType::Gauge,
                category: MetricCategory::Memory, value: 512, min_value: 256,
                max_value: 1024, sample_count: 1, total_sum: 512,
                last_updated_ns: now, unit: String::from("MB"), enabled: true,
            },
            Metric {
                name: String::from("disk.iops"), metric_type: MetricType::Rate,
                category: MetricCategory::Disk, value: 1200, min_value: 0,
                max_value: 50000, sample_count: 1, total_sum: 1200,
                last_updated_ns: now, unit: String::from("ops/s"), enabled: true,
            },
            Metric {
                name: String::from("net.rx_bytes"), metric_type: MetricType::Counter,
                category: MetricCategory::Network, value: 1048576, min_value: 0,
                max_value: 1048576, sample_count: 1, total_sum: 1048576,
                last_updated_ns: now, unit: String::from("bytes"), enabled: true,
            },
        ],
        collection_enabled: true,
        collection_interval_ms: 5000,
        total_samples: 4,
        total_exports: 0,
        ops: 0,
    });
}

/// Register a new metric.
pub fn register_metric(name: &str, mtype: MetricType, category: MetricCategory, unit: &str) -> KernelResult<()> {
    with_state(|state| {
        if state.metrics.len() >= MAX_METRICS {
            return Err(KernelError::ResourceExhausted);
        }
        if state.metrics.iter().any(|m| m.name == name) {
            return Err(KernelError::AlreadyExists);
        }
        state.metrics.push(Metric {
            name: String::from(name), metric_type: mtype, category,
            value: 0, min_value: u64::MAX, max_value: 0,
            sample_count: 0, total_sum: 0,
            last_updated_ns: 0, unit: String::from(unit), enabled: true,
        });
        Ok(())
    })
}

/// Record a data point.
pub fn record(name: &str, value: u64) -> KernelResult<()> {
    with_state(|state| {
        if !state.collection_enabled {
            return Err(KernelError::PermissionDenied);
        }
        let now = crate::hpet::elapsed_ns();
        let metric = state.metrics.iter_mut().find(|m| m.name == name)
            .ok_or(KernelError::NotFound)?;
        if !metric.enabled {
            return Err(KernelError::PermissionDenied);
        }
        match metric.metric_type {
            MetricType::Counter => metric.value += value,
            _ => metric.value = value,
        }
        if value < metric.min_value { metric.min_value = value; }
        if value > metric.max_value { metric.max_value = value; }
        metric.sample_count += 1;
        metric.total_sum += value;
        metric.last_updated_ns = now;
        state.total_samples += 1;
        Ok(())
    })
}

/// Query a metric by name.
pub fn query(name: &str) -> Option<Metric> {
    STATE.lock().as_ref().and_then(|s| s.metrics.iter().find(|m| m.name == name).cloned())
}

/// List all metrics.
pub fn list_metrics() -> Vec<Metric> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.metrics.clone())
}

/// List by category.
pub fn by_category(category: MetricCategory) -> Vec<Metric> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        s.metrics.iter().filter(|m| m.category == category).cloned().collect()
    })
}

/// Remove a metric.
pub fn remove_metric(name: &str) -> KernelResult<()> {
    with_state(|state| {
        let before = state.metrics.len();
        state.metrics.retain(|m| m.name != name);
        if state.metrics.len() == before { return Err(KernelError::NotFound); }
        Ok(())
    })
}

/// Enable/disable collection.
pub fn set_collection_enabled(enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        state.collection_enabled = enabled;
        Ok(())
    })
}

/// Set collection interval.
pub fn set_interval(ms: u64) -> KernelResult<()> {
    with_state(|state| {
        if ms == 0 { return Err(KernelError::InvalidArgument); }
        state.collection_interval_ms = ms;
        Ok(())
    })
}

/// Export all metrics (marks export count).
pub fn export() -> KernelResult<Vec<Metric>> {
    with_state(|state| {
        state.total_exports += 1;
        Ok(state.metrics.clone())
    })
}

/// Statistics: (metric_count, total_samples, total_exports, collection_enabled, ops).
pub fn stats() -> (usize, u64, u64, bool, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.metrics.len(), s.total_samples, s.total_exports, s.collection_enabled, s.ops),
        None => (0, 0, 0, false, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("telemetry::self_test() — running tests...");
    init_defaults();

    // 1: Default metrics.
    assert_eq!(list_metrics().len(), 4);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Query metric.
    let m = query("cpu.usage_pct").expect("query");
    assert_eq!(m.metric_type, MetricType::Gauge);
    assert_eq!(m.value, 15);
    crate::serial_println!("  [2/8] query: OK");

    // 3: Record data.
    record("cpu.usage_pct", 42).expect("record");
    let m = query("cpu.usage_pct").expect("query2");
    assert_eq!(m.value, 42);
    assert_eq!(m.sample_count, 2);
    crate::serial_println!("  [3/8] record: OK");

    // 4: Counter increment.
    record("net.rx_bytes", 4096).expect("counter");
    let m = query("net.rx_bytes").expect("query3");
    assert_eq!(m.value, 1048576 + 4096);
    crate::serial_println!("  [4/8] counter: OK");

    // 5: Register custom.
    register_metric("custom.test", MetricType::Gauge, MetricCategory::Custom, "units").expect("reg");
    assert_eq!(list_metrics().len(), 5);
    assert!(register_metric("custom.test", MetricType::Gauge, MetricCategory::Custom, "").is_err());
    crate::serial_println!("  [5/8] register: OK");

    // 6: By category.
    let sys = by_category(MetricCategory::System);
    assert_eq!(sys.len(), 1);
    let custom = by_category(MetricCategory::Custom);
    assert_eq!(custom.len(), 1);
    crate::serial_println!("  [6/8] by_category: OK");

    // 7: Export.
    let exported = export().expect("export");
    assert_eq!(exported.len(), 5);
    crate::serial_println!("  [7/8] export: OK");

    // 8: Stats.
    let (count, samples, exports, enabled, ops) = stats();
    assert_eq!(count, 5);
    assert!(samples > 0);
    assert!(exports >= 1);
    assert!(enabled);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("telemetry::self_test() — all 8 tests passed");
}
