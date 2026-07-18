//! Disk I/O — per-device I/O statistics and monitoring.
//!
//! Tracks read/write operations, bytes transferred, latency,
//! and throughput for each storage device. Provides iostat-like
//! reporting for performance analysis.
//!
//! ## Architecture
//!
//! ```text
//! I/O statistics
//!   → diskio::record_read(dev, bytes, latency_ns) → log read op
//!   → diskio::record_write(dev, bytes, latency_ns) → log write op
//!   → diskio::device_stats(dev) → per-device summary
//!   → diskio::all_stats() → full overview
//!
//! Integration:
//!   → diskhealth (disk health)
//!   → disksmart (SMART monitoring)
//!   → perfmon (performance monitor)
//!   → hwmonitor (hardware monitor)
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

/// Per-device I/O statistics.
#[derive(Debug, Clone)]
pub struct DeviceIoStats {
    pub device_name: String,
    pub reads: u64,
    pub writes: u64,
    pub bytes_read: u64,
    pub bytes_written: u64,
    pub read_latency_total_ns: u64,
    pub write_latency_total_ns: u64,
    pub read_latency_max_ns: u64,
    pub write_latency_max_ns: u64,
    pub read_errors: u64,
    pub write_errors: u64,
    pub queue_depth: u32,
    pub first_io_ns: u64,
    pub last_io_ns: u64,
}

impl DeviceIoStats {
    /// Average read latency in nanoseconds.
    pub fn avg_read_latency_ns(&self) -> u64 {
        if self.reads == 0 { 0 } else { self.read_latency_total_ns / self.reads }
    }

    /// Average write latency in nanoseconds.
    pub fn avg_write_latency_ns(&self) -> u64 {
        if self.writes == 0 { 0 } else { self.write_latency_total_ns / self.writes }
    }

    /// Total I/O operations.
    pub fn total_ops(&self) -> u64 {
        self.reads + self.writes
    }

    /// Total bytes transferred.
    pub fn total_bytes(&self) -> u64 {
        self.bytes_read + self.bytes_written
    }
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_DEVICES: usize = 32;

struct State {
    devices: Vec<DeviceIoStats>,
    global_reads: u64,
    global_writes: u64,
    global_bytes_read: u64,
    global_bytes_written: u64,
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

fn find_or_create_device(state: &mut State, name: &str) -> KernelResult<usize> {
    if let Some(idx) = state.devices.iter().position(|d| d.device_name == name) {
        return Ok(idx);
    }
    if state.devices.len() >= MAX_DEVICES {
        return Err(KernelError::ResourceExhausted);
    }
    let now = crate::hpet::elapsed_ns();
    state.devices.push(DeviceIoStats {
        device_name: String::from(name),
        reads: 0, writes: 0, bytes_read: 0, bytes_written: 0,
        read_latency_total_ns: 0, write_latency_total_ns: 0,
        read_latency_max_ns: 0, write_latency_max_ns: 0,
        read_errors: 0, write_errors: 0, queue_depth: 0,
        first_io_ns: now, last_io_ns: now,
    });
    Ok(state.devices.len() - 1)
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        devices: Vec::new(),
        global_reads: 0,
        global_writes: 0,
        global_bytes_read: 0,
        global_bytes_written: 0,
        ops: 0,
    });
}

/// Record a read operation.
pub fn record_read(device: &str, bytes: u64, latency_ns: u64) -> KernelResult<()> {
    with_state(|state| {
        let now = crate::hpet::elapsed_ns();
        let idx = find_or_create_device(state, device)?;
        let dev = &mut state.devices[idx];
        dev.reads += 1;
        dev.bytes_read += bytes;
        dev.read_latency_total_ns += latency_ns;
        if latency_ns > dev.read_latency_max_ns {
            dev.read_latency_max_ns = latency_ns;
        }
        dev.last_io_ns = now;
        state.global_reads += 1;
        state.global_bytes_read += bytes;
        Ok(())
    })
}

/// Record a write operation.
pub fn record_write(device: &str, bytes: u64, latency_ns: u64) -> KernelResult<()> {
    with_state(|state| {
        let now = crate::hpet::elapsed_ns();
        let idx = find_or_create_device(state, device)?;
        let dev = &mut state.devices[idx];
        dev.writes += 1;
        dev.bytes_written += bytes;
        dev.write_latency_total_ns += latency_ns;
        if latency_ns > dev.write_latency_max_ns {
            dev.write_latency_max_ns = latency_ns;
        }
        dev.last_io_ns = now;
        state.global_writes += 1;
        state.global_bytes_written += bytes;
        Ok(())
    })
}

/// Record a read error.
pub fn record_read_error(device: &str) -> KernelResult<()> {
    with_state(|state| {
        let idx = find_or_create_device(state, device)?;
        state.devices[idx].read_errors += 1;
        Ok(())
    })
}

/// Record a write error.
pub fn record_write_error(device: &str) -> KernelResult<()> {
    with_state(|state| {
        let idx = find_or_create_device(state, device)?;
        state.devices[idx].write_errors += 1;
        Ok(())
    })
}

/// Get stats for a specific device.
pub fn device_stats(name: &str) -> Option<DeviceIoStats> {
    STATE.lock().as_ref().and_then(|s| {
        s.devices.iter().find(|d| d.device_name == name).cloned()
    })
}

/// Get stats for all devices.
pub fn all_devices() -> Vec<DeviceIoStats> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.devices.clone())
}

/// Reset stats for a device.
pub fn reset_device(name: &str) -> KernelResult<()> {
    with_state(|state| {
        let before = state.devices.len();
        state.devices.retain(|d| d.device_name != name);
        if state.devices.len() == before { return Err(KernelError::NotFound); }
        Ok(())
    })
}

/// Statistics: (device_count, global_reads, global_writes, global_bytes_read, global_bytes_written, ops).
pub fn stats() -> (usize, u64, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.devices.len(), s.global_reads, s.global_writes,
                     s.global_bytes_read, s.global_bytes_written, s.ops),
        None => (0, 0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("diskio::self_test() — running tests...");
    init_defaults();

    // 1: Empty state.
    assert!(all_devices().is_empty());
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Record read.
    record_read("sda", 4096, 500_000).expect("read");
    assert_eq!(all_devices().len(), 1);
    let d = device_stats("sda").expect("get sda");
    assert_eq!(d.reads, 1);
    assert_eq!(d.bytes_read, 4096);
    crate::serial_println!("  [2/8] record read: OK");

    // 3: Record write.
    record_write("sda", 8192, 1_000_000).expect("write");
    let d = device_stats("sda").expect("get sda2");
    assert_eq!(d.writes, 1);
    assert_eq!(d.bytes_written, 8192);
    crate::serial_println!("  [3/8] record write: OK");

    // 4: Multiple devices.
    record_read("nvme0", 16384, 100_000).expect("nvme read");
    assert_eq!(all_devices().len(), 2);
    crate::serial_println!("  [4/8] multi device: OK");

    // 5: Latency tracking.
    record_read("sda", 4096, 2_000_000).expect("read2");
    let d = device_stats("sda").expect("lat");
    assert_eq!(d.read_latency_max_ns, 2_000_000);
    assert_eq!(d.reads, 2);
    let avg = d.avg_read_latency_ns();
    assert!(avg > 0);
    crate::serial_println!("  [5/8] latency: OK");

    // 6: Error tracking.
    record_read_error("sda").expect("read_err");
    record_write_error("sda").expect("write_err");
    let d = device_stats("sda").expect("err");
    assert_eq!(d.read_errors, 1);
    assert_eq!(d.write_errors, 1);
    crate::serial_println!("  [6/8] errors: OK");

    // 7: Reset device.
    reset_device("nvme0").expect("reset");
    assert_eq!(all_devices().len(), 1);
    assert!(device_stats("nvme0").is_none());
    crate::serial_println!("  [7/8] reset: OK");

    // 8: Stats.
    let (dev_count, reads, writes, br, bw, ops) = stats();
    assert_eq!(dev_count, 1);
    assert!(reads >= 3);
    assert!(writes >= 1);
    assert!(br > 0);
    assert!(bw > 0);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("diskio::self_test() — all 8 tests passed");
}
