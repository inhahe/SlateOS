//! Disk Statistics — block device I/O performance monitoring.
//!
//! Tracks per-device read/write IOPS, throughput, latency,
//! queue depth, and merge statistics. Essential for storage
//! performance tuning and bottleneck detection.
//!
//! ## Architecture
//!
//! ```text
//! Disk statistics monitoring
//!   → diskstat::register(name) → register device
//!   → diskstat::record_read(dev, bytes, ns) → read I/O
//!   → diskstat::record_write(dev, bytes, ns) → write I/O
//!   → diskstat::per_device() → per-device stats
//!
//! Integration:
//!   → iosched (I/O scheduler)
//!   → blktrace (block trace)
//!   → blkqueue (block queue)
//!   → iolatency (I/O latency)
//! ```

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Per-device disk stats.
#[derive(Debug, Clone)]
pub struct DevDiskStats {
    pub name: String,
    pub reads: u64,
    pub read_bytes: u64,
    pub read_ns: u64,
    pub writes: u64,
    pub write_bytes: u64,
    pub write_ns: u64,
    pub discards: u64,
    pub flushes: u64,
    pub merges_read: u64,
    pub merges_write: u64,
    pub queue_depth: u32,
    pub max_queue_depth: u32,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_DEVICES: usize = 64;

struct State {
    devices: Vec<DevDiskStats>,
    total_reads: u64,
    total_writes: u64,
    total_read_bytes: u64,
    total_write_bytes: u64,
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
        devices: alloc::vec![
            DevDiskStats { name: String::from("sda"), reads: 50_000_000, read_bytes: 200_000_000_000, read_ns: 500_000_000_000, writes: 30_000_000, write_bytes: 120_000_000_000, write_ns: 900_000_000_000, discards: 100_000, flushes: 50_000, merges_read: 5_000_000, merges_write: 3_000_000, queue_depth: 32, max_queue_depth: 128 },
            DevDiskStats { name: String::from("nvme0n1"), reads: 100_000_000, read_bytes: 500_000_000_000, read_ns: 200_000_000_000, writes: 80_000_000, write_bytes: 400_000_000_000, write_ns: 300_000_000_000, discards: 500_000, flushes: 200_000, merges_read: 10_000_000, merges_write: 8_000_000, queue_depth: 64, max_queue_depth: 1024 },
        ],
        total_reads: 150_000_000,
        total_writes: 110_000_000,
        total_read_bytes: 700_000_000_000,
        total_write_bytes: 520_000_000_000,
        ops: 0,
    });
}

/// Register a block device.
pub fn register(name: &str) -> KernelResult<()> {
    with_state(|state| {
        if state.devices.len() >= MAX_DEVICES { return Err(KernelError::ResourceExhausted); }
        if state.devices.iter().any(|d| d.name == name) { return Err(KernelError::AlreadyExists); }
        state.devices.push(DevDiskStats {
            name: String::from(name), reads: 0, read_bytes: 0, read_ns: 0,
            writes: 0, write_bytes: 0, write_ns: 0, discards: 0, flushes: 0,
            merges_read: 0, merges_write: 0, queue_depth: 0, max_queue_depth: 0,
        });
        Ok(())
    })
}

/// Record a read I/O.
pub fn record_read(name: &str, bytes: u64, ns: u64) -> KernelResult<()> {
    with_state(|state| {
        let d = state.devices.iter_mut().find(|d| d.name == name)
            .ok_or(KernelError::NotFound)?;
        d.reads += 1;
        d.read_bytes += bytes;
        d.read_ns += ns;
        state.total_reads += 1;
        state.total_read_bytes += bytes;
        Ok(())
    })
}

/// Record a write I/O.
pub fn record_write(name: &str, bytes: u64, ns: u64) -> KernelResult<()> {
    with_state(|state| {
        let d = state.devices.iter_mut().find(|d| d.name == name)
            .ok_or(KernelError::NotFound)?;
        d.writes += 1;
        d.write_bytes += bytes;
        d.write_ns += ns;
        state.total_writes += 1;
        state.total_write_bytes += bytes;
        Ok(())
    })
}

/// Record a discard.
pub fn record_discard(name: &str) -> KernelResult<()> {
    with_state(|state| {
        let d = state.devices.iter_mut().find(|d| d.name == name)
            .ok_or(KernelError::NotFound)?;
        d.discards += 1;
        Ok(())
    })
}

/// Record a flush.
pub fn record_flush(name: &str) -> KernelResult<()> {
    with_state(|state| {
        let d = state.devices.iter_mut().find(|d| d.name == name)
            .ok_or(KernelError::NotFound)?;
        d.flushes += 1;
        Ok(())
    })
}

/// Record a merge.
pub fn record_merge(name: &str, is_write: bool) -> KernelResult<()> {
    with_state(|state| {
        let d = state.devices.iter_mut().find(|d| d.name == name)
            .ok_or(KernelError::NotFound)?;
        if is_write { d.merges_write += 1; } else { d.merges_read += 1; }
        Ok(())
    })
}

/// Per-device stats.
pub fn per_device() -> Vec<DevDiskStats> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.devices.clone())
}

/// Statistics: (dev_count, total_reads, total_writes, total_read_bytes, total_write_bytes, ops).
pub fn stats() -> (usize, u64, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.devices.len(), s.total_reads, s.total_writes, s.total_read_bytes, s.total_write_bytes, s.ops),
        None => (0, 0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("diskstat::self_test() — running tests...");
    init_defaults();

    // 1: Defaults.
    assert_eq!(per_device().len(), 2);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Register.
    register("test_disk").expect("register");
    assert_eq!(per_device().len(), 3);
    assert!(register("test_disk").is_err());
    crate::serial_println!("  [2/8] register: OK");

    // 3: Read.
    record_read("test_disk", 4096, 1000).expect("read");
    let d = per_device().iter().find(|d| d.name == "test_disk").cloned().unwrap();
    assert_eq!(d.reads, 1);
    assert_eq!(d.read_bytes, 4096);
    crate::serial_println!("  [3/8] read: OK");

    // 4: Write.
    record_write("test_disk", 8192, 2000).expect("write");
    let d = per_device().iter().find(|d| d.name == "test_disk").cloned().unwrap();
    assert_eq!(d.writes, 1);
    assert_eq!(d.write_bytes, 8192);
    crate::serial_println!("  [4/8] write: OK");

    // 5: Discard.
    record_discard("test_disk").expect("discard");
    let d = per_device().iter().find(|d| d.name == "test_disk").cloned().unwrap();
    assert_eq!(d.discards, 1);
    crate::serial_println!("  [5/8] discard: OK");

    // 6: Flush.
    record_flush("test_disk").expect("flush");
    let d = per_device().iter().find(|d| d.name == "test_disk").cloned().unwrap();
    assert_eq!(d.flushes, 1);
    crate::serial_println!("  [6/8] flush: OK");

    // 7: Merge.
    record_merge("test_disk", false).expect("merge_r");
    record_merge("test_disk", true).expect("merge_w");
    let d = per_device().iter().find(|d| d.name == "test_disk").cloned().unwrap();
    assert_eq!(d.merges_read, 1);
    assert_eq!(d.merges_write, 1);
    crate::serial_println!("  [7/8] merge: OK");

    // 8: Stats.
    let (devs, reads, writes, rb, wb, ops) = stats();
    assert!(devs >= 3);
    assert!(reads > 150_000_000);
    assert!(writes > 110_000_000);
    assert!(rb > 700_000_000_000);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("diskstat::self_test() — all 8 tests passed");
}
