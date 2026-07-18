//! System Resource — resource monitoring dashboard.
//!
//! Provides a unified view of system resource usage including CPU,
//! memory, disk, network, and GPU utilization with history tracking.
//!
//! ## Architecture
//!
//! ```text
//! Periodic sampling
//!   → sysresource::sample() → record current usage
//!   → sysresource::get_current() → latest snapshot
//!   → sysresource::get_history(duration) → usage over time
//!
//! Integration:
//!   → perfmon (performance monitoring)
//!   → hwmonitor (hardware sensors)
//!   → taskmon (process monitoring)
//!   → battery (power usage)
//! ```

#![allow(dead_code)]

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A resource usage snapshot.
#[derive(Debug, Clone)]
pub struct ResourceSnapshot {
    pub timestamp_ns: u64,
    pub cpu_percent: u32,
    pub memory_used_kb: u64,
    pub memory_total_kb: u64,
    pub swap_used_kb: u64,
    pub swap_total_kb: u64,
    pub disk_read_kb_s: u64,
    pub disk_write_kb_s: u64,
    pub net_rx_kb_s: u64,
    pub net_tx_kb_s: u64,
    pub gpu_percent: u32,
    pub gpu_memory_used_kb: u64,
    pub process_count: u32,
    pub thread_count: u32,
}

/// Resource alert threshold.
#[derive(Debug, Clone)]
pub struct AlertThreshold {
    pub resource: ResourceType,
    pub threshold_percent: u32,
    pub alert_enabled: bool,
    pub triggered: bool,
    pub trigger_count: u64,
}

/// Resource type for alerts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceType {
    Cpu,
    Memory,
    Swap,
    Disk,
    Network,
    Gpu,
}

impl ResourceType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Cpu => "CPU",
            Self::Memory => "Memory",
            Self::Swap => "Swap",
            Self::Disk => "Disk",
            Self::Network => "Network",
            Self::Gpu => "GPU",
        }
    }
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_HISTORY: usize = 300; // ~5 minutes at 1 sample/sec.

struct State {
    history: Vec<ResourceSnapshot>,
    alerts: Vec<AlertThreshold>,
    sampling_interval_ms: u64,
    total_samples: u64,
    total_alerts: u64,
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
    *guard = Some(State {
        history: Vec::new(),
        alerts: alloc::vec![
            AlertThreshold { resource: ResourceType::Cpu, threshold_percent: 90, alert_enabled: true, triggered: false, trigger_count: 0 },
            AlertThreshold { resource: ResourceType::Memory, threshold_percent: 85, alert_enabled: true, triggered: false, trigger_count: 0 },
            AlertThreshold { resource: ResourceType::Swap, threshold_percent: 80, alert_enabled: false, triggered: false, trigger_count: 0 },
            AlertThreshold { resource: ResourceType::Gpu, threshold_percent: 95, alert_enabled: false, triggered: false, trigger_count: 0 },
        ],
        sampling_interval_ms: 1000,
        total_samples: 0,
        total_alerts: 0,
        ops: 0,
    });
}

/// Record a resource snapshot.
pub fn sample(snap: ResourceSnapshot) -> KernelResult<Vec<ResourceType>> {
    with_state(|state| {
        if state.history.len() >= MAX_HISTORY {
            state.history.remove(0);
        }
        state.total_samples += 1;

        // Check alerts.
        let mut triggered = Vec::new();
        for alert in &mut state.alerts {
            if !alert.alert_enabled { continue; }
            let current = match alert.resource {
                ResourceType::Cpu => snap.cpu_percent,
                ResourceType::Memory => {
                    if snap.memory_total_kb > 0 {
                        ((snap.memory_used_kb * 100) / snap.memory_total_kb) as u32
                    } else { 0 }
                }
                ResourceType::Swap => {
                    if snap.swap_total_kb > 0 {
                        ((snap.swap_used_kb * 100) / snap.swap_total_kb) as u32
                    } else { 0 }
                }
                ResourceType::Gpu => snap.gpu_percent,
                _ => 0,
            };
            if current >= alert.threshold_percent {
                if !alert.triggered {
                    alert.triggered = true;
                    alert.trigger_count += 1;
                    state.total_alerts += 1;
                    triggered.push(alert.resource);
                }
            } else {
                alert.triggered = false;
            }
        }

        state.history.push(snap);
        Ok(triggered)
    })
}

/// Get the latest snapshot.
pub fn get_current() -> Option<ResourceSnapshot> {
    STATE.lock().as_ref().and_then(|s| s.history.last().cloned())
}

/// Get history (most recent first).
pub fn get_history(max: usize) -> Vec<ResourceSnapshot> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        let mut h = s.history.clone();
        h.reverse();
        h.truncate(max);
        h
    })
}

/// Set alert threshold.
pub fn set_alert(resource: ResourceType, threshold: u32, enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        if let Some(a) = state.alerts.iter_mut().find(|a| a.resource == resource) {
            a.threshold_percent = threshold.min(100);
            a.alert_enabled = enabled;
        } else {
            state.alerts.push(AlertThreshold {
                resource,
                threshold_percent: threshold.min(100),
                alert_enabled: enabled,
                triggered: false,
                trigger_count: 0,
            });
        }
        Ok(())
    })
}

/// Set sampling interval.
pub fn set_interval(ms: u64) -> KernelResult<()> {
    with_state(|state| {
        state.sampling_interval_ms = ms.clamp(100, 60000);
        Ok(())
    })
}

/// Get alerts.
pub fn get_alerts() -> Vec<AlertThreshold> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.alerts.clone())
}

/// Clear history.
pub fn clear_history() -> KernelResult<()> {
    with_state(|state| {
        state.history.clear();
        Ok(())
    })
}

/// Get average CPU over last N samples.
pub fn avg_cpu(n: usize) -> u32 {
    STATE.lock().as_ref().map_or(0, |s| {
        let count = n.min(s.history.len());
        if count == 0 { return 0; }
        let start = s.history.len() - count;
        let sum: u64 = s.history[start..].iter().map(|s| s.cpu_percent as u64).sum();
        (sum / count as u64) as u32
    })
}

/// Statistics: (sample_count, history_size, alert_count, total_alerts, ops).
pub fn stats() -> (u64, usize, usize, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.total_samples, s.history.len(), s.alerts.len(), s.total_alerts, s.ops),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("sysresource::self_test() — running tests...");
    // Start from a clean slate so the fixtures pushed below (sample snapshots,
    // a Network alert) can never leak into the live /proc/sysresource view.
    // sysresource::init_defaults is not wired into boot, so the natural state
    // is uninitialised — running `sysresource test` from kshell must leave it
    // that way rather than permanently injecting fabricated samples/alerts.
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty.
    assert!(get_current().is_none());
    crate::serial_println!("  [1/8] empty: OK");

    // 2: Sample.
    let snap = ResourceSnapshot {
        timestamp_ns: crate::hpet::elapsed_ns(),
        cpu_percent: 45, memory_used_kb: 4000000, memory_total_kb: 8000000,
        swap_used_kb: 0, swap_total_kb: 2000000,
        disk_read_kb_s: 1000, disk_write_kb_s: 500,
        net_rx_kb_s: 200, net_tx_kb_s: 100,
        gpu_percent: 30, gpu_memory_used_kb: 500000,
        process_count: 120, thread_count: 450,
    };
    let alerts = sample(snap).expect("sample");
    assert!(alerts.is_empty()); // Below thresholds.
    assert!(get_current().is_some());
    crate::serial_println!("  [2/8] sample: OK");

    // 3: High CPU triggers alert.
    let high_cpu = ResourceSnapshot {
        timestamp_ns: crate::hpet::elapsed_ns(),
        cpu_percent: 95, memory_used_kb: 4000000, memory_total_kb: 8000000,
        swap_used_kb: 0, swap_total_kb: 2000000,
        disk_read_kb_s: 0, disk_write_kb_s: 0,
        net_rx_kb_s: 0, net_tx_kb_s: 0,
        gpu_percent: 0, gpu_memory_used_kb: 0,
        process_count: 120, thread_count: 450,
    };
    let alerts = sample(high_cpu).expect("sample2");
    assert!(alerts.contains(&ResourceType::Cpu));
    crate::serial_println!("  [3/8] alert: OK");

    // 4: History.
    let hist = get_history(10);
    assert_eq!(hist.len(), 2);
    assert_eq!(hist[0].cpu_percent, 95); // Most recent first.
    crate::serial_println!("  [4/8] history: OK");

    // 5: Average CPU.
    let avg = avg_cpu(2);
    assert_eq!(avg, 70); // (45 + 95) / 2.
    crate::serial_println!("  [5/8] avg CPU: OK");

    // 6: Set alert threshold.
    set_alert(ResourceType::Network, 80, true).expect("alert");
    let alerts = get_alerts();
    assert_eq!(alerts.len(), 5);
    crate::serial_println!("  [6/8] set alert: OK");

    // 7: Clear history.
    clear_history().expect("clear");
    assert!(get_current().is_none());
    assert_eq!(get_history(10).len(), 0);
    crate::serial_println!("  [7/8] clear: OK");

    // 8: Stats.
    let (samples, hist_size, _alert_count, total_alerts, ops) = stats();
    assert_eq!(samples, 2);
    assert_eq!(hist_size, 0); // Cleared.
    assert_eq!(total_alerts, 1);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Reset so the test leaves no fixtures (2 samples, 1 alert, a 5th
    // threshold) behind in the live /proc/sysresource registry.
    *STATE.lock() = None;

    crate::serial_println!("sysresource::self_test() — all 8 tests passed");
}
