//! Performance monitor — CPU, RAM, disk, and network resource tracking.
//!
//! Collects periodic samples of system resource usage to enable
//! Task Manager "Performance" tab style graphs.  Stores a rolling
//! history window.
//!
//! ## Design Reference
//!
//! design.txt line 863: "a system-wide resource monitor view
//!   (CPU/RAM/disk/network graphs over time, like Windows Task
//!   Manager's Performance tab or htop)"
//!
//! ## Architecture
//!
//! ```text
//! Timer tick / idle hook
//!   → perfmon::record_sample(CpuSample, MemSample, ...)
//!
//! Task Manager GUI / Performance tab
//!   → perfmon::cpu_history()
//!   → perfmon::mem_history()
//!   → perfmon::disk_history()
//!   → perfmon::net_history()
//!   → renders time-series graphs
//! ```

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use alloc::vec;
use alloc::format;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// CPU usage sample.
#[derive(Debug, Clone)]
pub struct CpuSample {
    /// Timestamp (ns).
    pub timestamp_ns: u64,
    /// Overall CPU usage (0-100).
    pub usage_pct: u32,
    /// Per-core usage (0-100 each).
    pub per_core: Vec<u32>,
    /// System (kernel) time percentage.
    pub system_pct: u32,
    /// User time percentage.
    pub user_pct: u32,
    /// Current frequency (MHz).
    pub freq_mhz: u32,
    /// Temperature (millidegrees C, 0 = unknown).
    pub temp_mc: u32,
    /// Number of running processes.
    pub process_count: u32,
    /// Number of running threads.
    pub thread_count: u32,
}

/// Memory usage sample.
#[derive(Debug, Clone)]
pub struct MemSample {
    /// Timestamp (ns).
    pub timestamp_ns: u64,
    /// Used physical memory (bytes).
    pub used_bytes: u64,
    /// Available (bytes).
    pub available_bytes: u64,
    /// Cached (bytes).
    pub cached_bytes: u64,
    /// Swap used (bytes).
    pub swap_used_bytes: u64,
    /// Page faults since last sample.
    pub page_faults: u64,
}

/// Disk I/O sample.
#[derive(Debug, Clone)]
pub struct DiskSample {
    /// Timestamp (ns).
    pub timestamp_ns: u64,
    /// Device name.
    pub device: String,
    /// Bytes read since last sample.
    pub read_bytes: u64,
    /// Bytes written since last sample.
    pub write_bytes: u64,
    /// Read IOPS.
    pub read_iops: u32,
    /// Write IOPS.
    pub write_iops: u32,
    /// Queue depth.
    pub queue_depth: u32,
    /// Disk busy percentage (0-100).
    pub busy_pct: u32,
}

/// Network I/O sample.
#[derive(Debug, Clone)]
pub struct NetSample {
    /// Timestamp (ns).
    pub timestamp_ns: u64,
    /// Interface name.
    pub interface: String,
    /// Bytes received since last sample.
    pub rx_bytes: u64,
    /// Bytes transmitted since last sample.
    pub tx_bytes: u64,
    /// Packets received.
    pub rx_packets: u64,
    /// Packets transmitted.
    pub tx_packets: u64,
    /// Errors.
    pub errors: u64,
}

/// Monitor configuration.
#[derive(Debug, Clone)]
pub struct MonitorConfig {
    /// Sample interval in milliseconds.
    pub sample_interval_ms: u32,
    /// Maximum history samples to keep.
    pub max_samples: usize,
    /// Whether CPU monitoring is enabled.
    pub cpu_enabled: bool,
    /// Whether memory monitoring is enabled.
    pub mem_enabled: bool,
    /// Whether disk monitoring is enabled.
    pub disk_enabled: bool,
    /// Whether network monitoring is enabled.
    pub net_enabled: bool,
    /// Alert threshold: CPU usage percentage.
    pub cpu_alert_pct: u32,
    /// Alert threshold: memory usage percentage.
    pub mem_alert_pct: u32,
    /// Alert threshold: disk busy percentage.
    pub disk_alert_pct: u32,
}

/// Resource alert.
#[derive(Debug, Clone)]
pub struct Alert {
    /// Alert ID.
    pub id: u64,
    /// Resource type.
    pub resource: String,
    /// Description.
    pub message: String,
    /// Current value.
    pub value: u32,
    /// Threshold.
    pub threshold: u32,
    /// Timestamp (ns).
    pub timestamp_ns: u64,
    /// Whether dismissed.
    pub dismissed: bool,
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const DEFAULT_MAX_SAMPLES: usize = 300; // ~5 minutes at 1s interval
const MAX_ALERTS: usize = 64;

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct State {
    config: MonitorConfig,
    cpu_history: Vec<CpuSample>,
    mem_history: Vec<MemSample>,
    disk_history: Vec<DiskSample>,
    net_history: Vec<NetSample>,
    alerts: Vec<Alert>,
    next_alert_id: u64,
    changes: u64,
}

static STATE: Mutex<State> = Mutex::new(State {
    config: MonitorConfig {
        sample_interval_ms: 1000,
        max_samples: DEFAULT_MAX_SAMPLES,
        cpu_enabled: true,
        mem_enabled: true,
        disk_enabled: true,
        net_enabled: true,
        cpu_alert_pct: 90,
        mem_alert_pct: 90,
        disk_alert_pct: 95,
    },
    cpu_history: Vec::new(),
    mem_history: Vec::new(),
    disk_history: Vec::new(),
    net_history: Vec::new(),
    alerts: Vec::new(),
    next_alert_id: 1,
    changes: 0,
});

static OP_COUNT: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Get monitor configuration.
pub fn get_config() -> MonitorConfig {
    STATE.lock().config.clone()
}

/// Set sample interval.
pub fn set_interval(ms: u32) {
    let mut state = STATE.lock();
    state.config.sample_interval_ms = ms.clamp(100, 60000);
    state.changes += 1;
}

/// Set max history.
pub fn set_max_samples(n: usize) {
    let mut state = STATE.lock();
    state.config.max_samples = n.clamp(10, 10000);
    state.changes += 1;
}

/// Enable/disable CPU monitoring.
pub fn set_cpu_enabled(enabled: bool) {
    let mut state = STATE.lock();
    state.config.cpu_enabled = enabled;
    state.changes += 1;
}

/// Enable/disable memory monitoring.
pub fn set_mem_enabled(enabled: bool) {
    let mut state = STATE.lock();
    state.config.mem_enabled = enabled;
    state.changes += 1;
}

/// Enable/disable disk monitoring.
pub fn set_disk_enabled(enabled: bool) {
    let mut state = STATE.lock();
    state.config.disk_enabled = enabled;
    state.changes += 1;
}

/// Enable/disable network monitoring.
pub fn set_net_enabled(enabled: bool) {
    let mut state = STATE.lock();
    state.config.net_enabled = enabled;
    state.changes += 1;
}

/// Set CPU alert threshold.
pub fn set_cpu_alert(pct: u32) {
    let mut state = STATE.lock();
    state.config.cpu_alert_pct = pct.min(100);
    state.changes += 1;
}

/// Set memory alert threshold.
pub fn set_mem_alert(pct: u32) {
    let mut state = STATE.lock();
    state.config.mem_alert_pct = pct.min(100);
    state.changes += 1;
}

/// Set disk alert threshold.
pub fn set_disk_alert(pct: u32) {
    let mut state = STATE.lock();
    state.config.disk_alert_pct = pct.min(100);
    state.changes += 1;
}

// ---------------------------------------------------------------------------
// Recording samples
// ---------------------------------------------------------------------------

/// Record a CPU sample.
pub fn record_cpu(sample: CpuSample) {
    let mut state = STATE.lock();
    if !state.config.cpu_enabled { return; }
    let max = state.config.max_samples;
    if state.cpu_history.len() >= max {
        state.cpu_history.remove(0);
    }

    // Check alert.
    let alert_threshold = state.config.cpu_alert_pct;
    if sample.usage_pct >= alert_threshold {
        let id = state.next_alert_id;
        state.next_alert_id += 1;
        if state.alerts.len() >= MAX_ALERTS {
            state.alerts.remove(0);
        }
        let usage = sample.usage_pct;
        let ts = sample.timestamp_ns;
        state.alerts.push(Alert {
            id,
            resource: String::from("CPU"),
            message: format!("CPU usage {}% exceeds threshold {}%",
                usage, alert_threshold),
            value: usage,
            threshold: alert_threshold,
            timestamp_ns: ts,
            dismissed: false,
        });
    }

    state.cpu_history.push(sample);
    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
}

/// Record a memory sample.
pub fn record_mem(sample: MemSample) {
    let mut state = STATE.lock();
    if !state.config.mem_enabled { return; }
    let max = state.config.max_samples;
    if state.mem_history.len() >= max {
        state.mem_history.remove(0);
    }
    state.mem_history.push(sample);
    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
}

/// Record a disk I/O sample.
pub fn record_disk(sample: DiskSample) {
    let mut state = STATE.lock();
    if !state.config.disk_enabled { return; }
    let max = state.config.max_samples;
    if state.disk_history.len() >= max {
        state.disk_history.remove(0);
    }
    state.disk_history.push(sample);
    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
}

/// Record a network sample.
pub fn record_net(sample: NetSample) {
    let mut state = STATE.lock();
    if !state.config.net_enabled { return; }
    let max = state.config.max_samples;
    if state.net_history.len() >= max {
        state.net_history.remove(0);
    }
    state.net_history.push(sample);
    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// History retrieval
// ---------------------------------------------------------------------------

/// Get CPU history.
pub fn cpu_history() -> Vec<CpuSample> {
    STATE.lock().cpu_history.clone()
}

/// Get most recent CPU sample.
pub fn cpu_latest() -> Option<CpuSample> {
    STATE.lock().cpu_history.last().cloned()
}

/// Get memory history.
pub fn mem_history() -> Vec<MemSample> {
    STATE.lock().mem_history.clone()
}

/// Get most recent memory sample.
pub fn mem_latest() -> Option<MemSample> {
    STATE.lock().mem_history.last().cloned()
}

/// Get disk history.
pub fn disk_history() -> Vec<DiskSample> {
    STATE.lock().disk_history.clone()
}

/// Get network history.
pub fn net_history() -> Vec<NetSample> {
    STATE.lock().net_history.clone()
}

// ---------------------------------------------------------------------------
// Alerts
// ---------------------------------------------------------------------------

/// Get active (undismissed) alerts.
pub fn active_alerts() -> Vec<Alert> {
    STATE.lock().alerts.iter()
        .filter(|a| !a.dismissed)
        .cloned()
        .collect()
}

/// Get all alerts.
pub fn all_alerts() -> Vec<Alert> {
    STATE.lock().alerts.clone()
}

/// Dismiss an alert.
pub fn dismiss_alert(id: u64) -> KernelResult<()> {
    let mut state = STATE.lock();
    let alert = state.alerts.iter_mut().find(|a| a.id == id)
        .ok_or(KernelError::NotFound)?;
    alert.dismissed = true;
    state.changes += 1;
    Ok(())
}

/// Dismiss all alerts.
pub fn dismiss_all_alerts() {
    let mut state = STATE.lock();
    for a in &mut state.alerts {
        a.dismissed = true;
    }
    state.changes += 1;
}

// ---------------------------------------------------------------------------
// Init / stats
// ---------------------------------------------------------------------------

/// Initialise with default config and sample data.
pub fn init_defaults() {
    let mut state = STATE.lock();
    state.config = MonitorConfig {
        sample_interval_ms: 1000,
        max_samples: DEFAULT_MAX_SAMPLES,
        cpu_enabled: true,
        mem_enabled: true,
        disk_enabled: true,
        net_enabled: true,
        cpu_alert_pct: 90,
        mem_alert_pct: 90,
        disk_alert_pct: 95,
    };
    state.cpu_history.clear();
    state.mem_history.clear();
    state.disk_history.clear();
    state.net_history.clear();
    state.alerts.clear();
    state.next_alert_id = 1;
    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
}

/// Return (cpu_samples, mem_samples, disk_samples, net_samples, alerts, ops).
pub fn stats() -> (usize, usize, usize, usize, usize, u64) {
    let state = STATE.lock();
    (state.cpu_history.len(),
     state.mem_history.len(),
     state.disk_history.len(),
     state.net_history.len(),
     state.alerts.len(),
     OP_COUNT.load(Ordering::Relaxed))
}

pub fn reset_stats() {
    OP_COUNT.store(0, Ordering::Relaxed);
}

pub fn clear_all() {
    let mut state = STATE.lock();
    state.cpu_history.clear();
    state.mem_history.clear();
    state.disk_history.clear();
    state.net_history.clear();
    state.alerts.clear();
    state.next_alert_id = 1;
    state.changes = 0;
    OP_COUNT.store(0, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

pub fn self_test() -> KernelResult<()> {
    use crate::serial_println;

    clear_all();

    // Test 1: default config.
    serial_println!("perfmon::self_test 1: config");
    let cfg = get_config();
    assert_eq!(cfg.sample_interval_ms, 1000);
    assert!(cfg.cpu_enabled);

    // Test 2: record CPU sample.
    serial_println!("perfmon::self_test 2: CPU sample");
    let ts = crate::hpet::elapsed_ns();
    record_cpu(CpuSample {
        timestamp_ns: ts,
        usage_pct: 42,
        per_core: vec![50, 35, 40, 43],
        system_pct: 12,
        user_pct: 30,
        freq_mhz: 4200,
        temp_mc: 65000,
        process_count: 120,
        thread_count: 450,
    });
    let hist = cpu_history();
    assert_eq!(hist.len(), 1);
    assert_eq!(hist[0].usage_pct, 42);

    // Test 3: latest.
    serial_println!("perfmon::self_test 3: latest");
    let latest = cpu_latest();
    assert!(latest.is_some());
    assert_eq!(latest.unwrap().usage_pct, 42);

    // Test 4: record memory.
    serial_println!("perfmon::self_test 4: memory");
    record_mem(MemSample {
        timestamp_ns: ts,
        used_bytes: 8_000_000_000,
        available_bytes: 8_000_000_000,
        cached_bytes: 2_000_000_000,
        swap_used_bytes: 0,
        page_faults: 100,
    });
    assert_eq!(mem_history().len(), 1);

    // Test 5: record disk.
    serial_println!("perfmon::self_test 5: disk");
    record_disk(DiskSample {
        timestamp_ns: ts,
        device: String::from("nvme0n1"),
        read_bytes: 1_000_000,
        write_bytes: 500_000,
        read_iops: 1000,
        write_iops: 500,
        queue_depth: 4,
        busy_pct: 25,
    });
    assert_eq!(disk_history().len(), 1);

    // Test 6: record network.
    serial_println!("perfmon::self_test 6: network");
    record_net(NetSample {
        timestamp_ns: ts,
        interface: String::from("eth0"),
        rx_bytes: 100_000,
        tx_bytes: 50_000,
        rx_packets: 200,
        tx_packets: 150,
        errors: 0,
    });
    assert_eq!(net_history().len(), 1);

    // Test 7: CPU alert.
    serial_println!("perfmon::self_test 7: alert");
    set_cpu_alert(50);
    record_cpu(CpuSample {
        timestamp_ns: ts + 1000,
        usage_pct: 75,
        per_core: vec![80, 70],
        system_pct: 25,
        user_pct: 50,
        freq_mhz: 4500,
        temp_mc: 72000,
        process_count: 130,
        thread_count: 460,
    });
    let alerts = active_alerts();
    assert_eq!(alerts.len(), 1);
    assert_eq!(alerts[0].resource, "CPU");

    // Test 8: dismiss.
    serial_println!("perfmon::self_test 8: dismiss");
    dismiss_alert(alerts[0].id)?;
    assert!(active_alerts().is_empty());

    // Test 9: disabled monitoring.
    serial_println!("perfmon::self_test 9: disabled");
    set_cpu_enabled(false);
    let before = cpu_history().len();
    record_cpu(CpuSample {
        timestamp_ns: ts + 2000,
        usage_pct: 50,
        per_core: Vec::new(),
        system_pct: 20,
        user_pct: 30,
        freq_mhz: 4000,
        temp_mc: 60000,
        process_count: 100,
        thread_count: 400,
    });
    assert_eq!(cpu_history().len(), before); // Should not grow.
    set_cpu_enabled(true);

    // Test 10: history limit.
    serial_println!("perfmon::self_test 10: limit");
    set_max_samples(5);
    for i in 0..10u32 {
        record_mem(MemSample {
            timestamp_ns: ts + i as u64 * 1000,
            used_bytes: 1000 * i as u64,
            available_bytes: 10000,
            cached_bytes: 500,
            swap_used_bytes: 0,
            page_faults: i as u64,
        });
    }
    assert_eq!(mem_history().len(), 5);

    // Test 11: stats.
    serial_println!("perfmon::self_test 11: stats");
    let (cpus, mems, disks, nets, alerts, _ops) = stats();
    assert!(cpus > 0);
    assert_eq!(mems, 5);
    assert_eq!(disks, 1);
    assert_eq!(nets, 1);
    assert!(alerts > 0);

    clear_all();
    serial_println!("perfmon::self_test: all 11 tests passed");
    Ok(())
}
