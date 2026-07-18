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

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

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

/// Initialise an **empty** latency table.
///
/// Seeds NO device rows and zero totals.  Real per-device latency is wired
/// through [`register_device`] plus [`record`]; until those are called the
/// table is genuinely empty, so the `/proc/iolatency` file and the
/// `iolatency` kshell command report zeros rather than fabricated numbers —
/// the kernel's hard "never invent data in procfs" rule.
///
/// NOTE: this previously seeded two fictional devices ("sda"/"nvme0n1") with
/// invented I/O counts (e.g. 37M total_ios, 302k slow I/Os, fabricated
/// histograms), which `/proc/iolatency` then displayed as if they were real
/// storage statistics.  That demo data was removed; the self-test now builds
/// its own fixtures explicitly via the real API (see [`self_test`]).  The
/// block layer is expected to call [`register_device`] when a disk appears
/// and [`record`] on each completed I/O.
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        devices: Vec::new(),
        slow_ios: Vec::new(),
        total_ios: 0,
        total_slow: 0,
        slow_threshold_ns: SLOW_THRESHOLD_NS,
        ops: 0,
    });
}

/// Register a block device for latency tracking.
///
/// Mirrors how a real block layer would announce a disk before recording
/// per-I/O latencies against it.  Returns [`KernelError::AlreadyExists`] if
/// the device is already tracked and [`KernelError::ResourceExhausted`] once
/// [`MAX_DEVICES`] rows are in use.
pub fn register_device(device: &str) -> KernelResult<()> {
    with_state(|state| {
        if state.devices.iter().any(|d| d.device == device) {
            return Err(KernelError::AlreadyExists);
        }
        if state.devices.len() >= MAX_DEVICES {
            return Err(KernelError::ResourceExhausted);
        }
        state.devices.push(DeviceLatency {
            device: String::from(device),
            read_count: 0, write_count: 0,
            read_avg_ns: 0, write_avg_ns: 0,
            read_max_ns: 0, write_max_ns: 0,
            histogram: [0; 8], slow_count: 0,
        });
        Ok(())
    })
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
                // EWMA, seeded exactly on the first sample to avoid a
                // cold-start bias toward zero (the row starts at avg = 0).
                dev.read_avg_ns = if dev.read_count == 0 {
                    latency_ns
                } else {
                    (dev.read_avg_ns * 7 + latency_ns) / 8
                };
                if latency_ns > dev.read_max_ns { dev.read_max_ns = latency_ns; }
                dev.read_count += 1;
            }
            IoOp::Write => {
                dev.write_avg_ns = if dev.write_count == 0 {
                    latency_ns
                } else {
                    (dev.write_avg_ns * 7 + latency_ns) / 8
                };
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
    // Begin from a clean, EMPTY table and build every fixture via the real
    // API, so the test exercises genuine accounting paths and never relies on
    // fabricated seed data (which /proc/iolatency must never surface).
    // Resetting first clears any residue from a prior `iolatency test` run so
    // the totals asserted below are exact.
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty after init — no fabricated rows.
    assert_eq!(per_device().len(), 0);
    let (devs0, ios0, slow0, _t0, _o0) = stats();
    assert_eq!(devs0, 0);
    assert_eq!(ios0, 0);
    assert_eq!(slow0, 0);
    crate::serial_println!("  [1/8] empty init: OK");

    // 2: Register devices, then record a fast read (EWMA seeds exactly).
    register_device("sda").expect("register sda");
    register_device("nvme0n1").expect("register nvme0n1");
    assert!(register_device("sda").is_err()); // duplicate rejected
    record("sda", IoOp::Read, 500_000).expect("read");
    let dev = per_device().iter().find(|d| d.device == "sda").cloned().expect("sda");
    assert_eq!(dev.read_count, 1);
    assert_eq!(dev.read_avg_ns, 500_000); // first-sample seed, exact
    crate::serial_println!("  [2/8] register + fast read: OK");

    // 3: Record slow write (>= 10ms default threshold ⇒ logged as slow).
    record("sda", IoOp::Write, 50_000_000).expect("slow_write");
    let slow = slow_ios(5);
    assert_eq!(slow.len(), 1);
    assert_eq!(slow[0].latency_ns, 50_000_000);
    crate::serial_println!("  [3/8] slow write: OK");

    // 4: Histogram — the two recorded I/Os land in their buckets.
    let hist = histogram("sda");
    assert_eq!(hist.iter().sum::<u64>(), 2);
    crate::serial_println!("  [4/8] histogram: OK");

    // 5: Raise threshold; a 50ms read is now under it (not counted slow).
    set_threshold(100_000_000).expect("threshold");
    let slow_before = stats().2;
    record("sda", IoOp::Read, 50_000_000).expect("under_new_threshold");
    let (_, _, slow_after, threshold, _) = stats();
    assert_eq!(threshold, 100_000_000);
    assert_eq!(slow_after, slow_before); // unchanged: under the new threshold
    crate::serial_println!("  [5/8] threshold: OK");

    // 6: NVMe device accounts independently.
    record("nvme0n1", IoOp::Read, 25_000).expect("nvme_read");
    let nvme = per_device().iter().find(|d| d.device == "nvme0n1").cloned().expect("nvme");
    assert_eq!(nvme.read_count, 1);
    assert_eq!(nvme.read_avg_ns, 25_000);
    crate::serial_println!("  [6/8] nvme: OK");

    // 7: Recording on an unregistered device fails.
    assert!(record("fake", IoOp::Read, 0).is_err());
    crate::serial_println!("  [7/8] not found: OK");

    // 8: Aggregate totals equal the exact count of recorded I/Os.
    let (devs, ios, slow, _threshold, ops) = stats();
    assert_eq!(devs, 2);
    assert_eq!(ios, 4); // sda: read+write+read, nvme0n1: read
    assert_eq!(slow, 1); // only the 50ms write under the original 10ms threshold
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Leave NO residue: a diagnostic self-test must not populate the live
    // /proc/iolatency table with its fixtures.  Reset to the uninitialised
    // state so production reads report an empty table until the block layer
    // wires real latency accounting.
    *STATE.lock() = None;

    crate::serial_println!("iolatency::self_test() — all 8 tests passed");
}
