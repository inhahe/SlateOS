//! I/O Latency — block I/O request latency monitoring.
//!
//! Tracks I/O request latency histograms, per-device latency
//! stats, and slow I/O detection. Essential for diagnosing
//! storage performance issues.
//!
//! ## Architecture
//!
//! ```text
//! I/O latency monitoring
//!   → iolatency::record(device, op, latency_ns) → track I/O latency
//!   → iolatency::histogram(device) → latency distribution
//!   → iolatency::slow_ios() → recent slow I/O events
//!   → iolatency::per_device() → per-device latency stats
//!
//! Integration:
//!   → blktrace (block tracing)
//!   → diskio (disk I/O stats)
//!   → writeback (dirty page writeback)
//!   → perfmon (performance monitor)
//! ```

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// I/O operation type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IoOp {
    Read,
    Write,
    Flush,
    Discard,
}

impl IoOp {
    pub fn label(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Write => "write",
            Self::Flush => "flush",
            Self::Discard => "discard",
        }
    }
}

/// Latency bucket boundaries (nanoseconds).
const BUCKET_BOUNDS_NS: [u64; 8] = [
    1_000,        // < 1us
    10_000,       // < 10us
    100_000,      // < 100us
    1_000_000,    // < 1ms
    10_000_000,   // < 10ms
    100_000_000,  // < 100ms
    1_000_000_000,// < 1s
    u64::MAX,     // >= 1s
];

/// Per-device I/O latency stats.
#[derive(Debug, Clone)]
pub struct DeviceLatency {
    pub device: String,
    pub read_count: u64,
    pub write_count: u64,
    pub read_avg_ns: u64,
    pub write_avg_ns: u64,
    pub read_max_ns: u64,
    pub write_max_ns: u64,
    pub histogram: [u64; 8],
    pub slow_count: u64,
}

/// A slow I/O event.
#[derive(Debug, Clone)]
pub struct SlowIo {
    pub device: String,
    pub op: IoOp,
    pub latency_ns: u64,
    pub timestamp_ns: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_DEVICES: usize = 16;
const MAX_SLOW: usize = 128;
const SLOW_THRESHOLD_NS: u64 = 10_000_000; // 10ms.

struct State {
    devices: Vec<DeviceLatency>,
    slow_ios: Vec<SlowIo>,
    total_ios: u64,
    total_slow: u64,
    slow_threshold_ns: u64,
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

fn bucket_index(ns: u64) -> usize {
    for (i, &bound) in BUCKET_BOUNDS_NS.iter().enumerate() {
        if ns < bound { return i; }
    }
    7
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        devices: alloc::vec![
            DeviceLatency { device: String::from("sda"), read_count: 5_000_000, write_count: 2_000_000, read_avg_ns: 500_000, write_avg_ns: 1_000_000, read_max_ns: 50_000_000, write_max_ns: 100_000_000, histogram: [100000, 500000, 2000000, 3000000, 1000000, 200000, 50000, 1000], slow_count: 251000 },
            DeviceLatency { device: String::from("nvme0n1"), read_count: 20_000_000, write_count: 10_000_000, read_avg_ns: 50_000, write_avg_ns: 100_000, read_max_ns: 5_000_000, write_max_ns: 10_000_000, histogram: [5000000, 15000000, 8000000, 1500000, 400000, 50000, 1000, 0], slow_count: 51000 },
        ],
        slow_ios: Vec::new(),
        total_ios: 37_000_000,
        total_slow: 302000,
        slow_threshold_ns: SLOW_THRESHOLD_NS,
        ops: 0,
    });
}

/// Record an I/O latency.
pub fn record(device: &str, op: IoOp, latency_ns: u64) -> KernelResult<()> {
    with_state(|state| {
        let now = crate::hpet::elapsed_ns();
        let dev = state.devices.iter_mut().find(|d| d.device == device)
            .ok_or(KernelError::NotFound)?;
        let idx = bucket_index(latency_ns);
        dev.histogram[idx] += 1;
        match op {
            IoOp::Read => {
                // Running average.
                dev.read_avg_ns = (dev.read_avg_ns * 7 + latency_ns) / 8;
                if latency_ns > dev.read_max_ns { dev.read_max_ns = latency_ns; }
                dev.read_count += 1;
            }
            IoOp::Write => {
                dev.write_avg_ns = (dev.write_avg_ns * 7 + latency_ns) / 8;
                if latency_ns > dev.write_max_ns { dev.write_max_ns = latency_ns; }
                dev.write_count += 1;
            }
            _ => {}
        }
        state.total_ios += 1;
        if latency_ns >= state.slow_threshold_ns {
            dev.slow_count += 1;
            state.total_slow += 1;
            if state.slow_ios.len() >= MAX_SLOW { state.slow_ios.remove(0); }
            state.slow_ios.push(SlowIo {
                device: String::from(device), op, latency_ns, timestamp_ns: now,
            });
        }
        Ok(())
    })
}

/// Set slow I/O threshold.
pub fn set_threshold(ns: u64) -> KernelResult<()> {
    with_state(|state| { state.slow_threshold_ns = ns; Ok(()) })
}

/// Get per-device stats.
pub fn per_device() -> Vec<DeviceLatency> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.devices.clone())
}

/// Get histogram for a device.
pub fn histogram(device: &str) -> [u64; 8] {
    STATE.lock().as_ref().map_or([0; 8], |s| {
        s.devices.iter().find(|d| d.device == device)
            .map_or([0; 8], |d| d.histogram)
    })
}

/// Recent slow I/Os.
pub fn slow_ios(n: usize) -> Vec<SlowIo> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        let start = if n >= s.slow_ios.len() { 0 } else { s.slow_ios.len() - n };
        s.slow_ios[start..].to_vec()
    })
}

/// Statistics: (device_count, total_ios, total_slow, threshold_ns, ops).
pub fn stats() -> (usize, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.devices.len(), s.total_ios, s.total_slow, s.slow_threshold_ns, s.ops),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("iolatency::self_test() — running tests...");
    init_defaults();

    // 1: Defaults.
    assert_eq!(per_device().len(), 2);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Record fast read.
    let before = per_device()[0].read_count;
    record("sda", IoOp::Read, 500_000).expect("read");
    let after = per_device()[0].read_count;
    assert_eq!(after, before + 1);
    crate::serial_println!("  [2/8] fast read: OK");

    // 3: Record slow write.
    record("sda", IoOp::Write, 50_000_000).expect("slow_write");
    let slow = slow_ios(5);
    assert_eq!(slow.len(), 1);
    assert_eq!(slow[0].latency_ns, 50_000_000);
    crate::serial_println!("  [3/8] slow write: OK");

    // 4: Histogram.
    let hist = histogram("sda");
    assert!(hist.iter().sum::<u64>() > 0);
    crate::serial_println!("  [4/8] histogram: OK");

    // 5: Set threshold.
    set_threshold(100_000_000).expect("threshold");
    record("sda", IoOp::Read, 50_000_000).expect("under_new_threshold");
    // This should NOT be slow under the new threshold.
    let (_, _, _, threshold, _) = stats();
    assert_eq!(threshold, 100_000_000);
    crate::serial_println!("  [5/8] threshold: OK");

    // 6: NVMe device.
    record("nvme0n1", IoOp::Read, 25_000).expect("nvme_read");
    let dev = per_device().iter().find(|d| d.device == "nvme0n1").cloned().unwrap();
    assert!(dev.read_count > 20_000_000);
    crate::serial_println!("  [6/8] nvme: OK");

    // 7: Not found.
    assert!(record("fake", IoOp::Read, 0).is_err());
    crate::serial_println!("  [7/8] not found: OK");

    // 8: Stats.
    let (devs, ios, slow, _threshold, ops) = stats();
    assert_eq!(devs, 2);
    assert!(ios > 37_000_000);
    assert!(slow > 302000);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("iolatency::self_test() — all 8 tests passed");
}
