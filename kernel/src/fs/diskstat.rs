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

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

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

/// Initialise an **empty** disk statistics table.
///
/// Seeds NO devices and zero totals.  Real block-device I/O accounting is wired
/// through [`register`] (one row per discovered block device, zeroed) and the
/// `record_read`/`record_write`/`record_discard`/`record_flush`/`record_merge`
/// functions; until those are called the table is genuinely empty, so the
/// `/proc/diskstat` file and the `diskstat` kshell command report zeros rather
/// than fabricated numbers — the kernel's hard "never invent data in procfs"
/// rule.
///
/// NOTE: this previously seeded two fictional devices ("sda" with 50M reads /
/// 200GB read / 30M writes / 120GB written; "nvme0n1" with 100M reads / 500GB
/// read / 80M writes / 400GB written) plus invented aggregate totals
/// (total_reads 150M, total_writes 110M, total_read_bytes 700GB,
/// total_write_bytes 520GB), which `/proc/diskstat` then displayed as if they
/// were real per-device I/O throughput measurements.  That demo data was
/// removed; the self-test now builds its own fixtures explicitly via the real
/// API (see [`self_test`]).  The block layer is expected to call [`register`]
/// when a device is discovered and the record_* functions as I/O completes.
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        devices: Vec::new(),
        total_reads: 0,
        total_writes: 0,
        total_read_bytes: 0,
        total_write_bytes: 0,
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
    // Begin from a clean, EMPTY table and build every fixture via the real
    // API, so the test exercises genuine accounting paths and never relies on
    // fabricated seed data (which /proc/diskstat must never surface).
    // Resetting first clears any residue from a prior `diskstat test` run so the
    // totals asserted below are exact.
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty after init — no fabricated devices or totals.
    assert_eq!(per_device().len(), 0);
    let (c0, r0, w0, rb0, wb0, _o0) = stats();
    assert_eq!((c0, r0, w0, rb0, wb0), (0, 0, 0, 0, 0));
    crate::serial_println!("  [1/8] empty init: OK");

    // 2: Register a device (zeroed); duplicate name fails.
    register("test_disk").expect("register");
    assert_eq!(per_device().len(), 1);
    assert!(register("test_disk").is_err());
    let d = per_device().iter().find(|d| d.name == "test_disk").cloned().expect("dev");
    assert_eq!(d.reads, 0);
    assert_eq!(d.read_bytes, 0);
    crate::serial_println!("  [2/8] register: OK");

    // 3: Read records count + bytes exactly from zero.
    record_read("test_disk", 4096, 1000).expect("read");
    let d = per_device().iter().find(|d| d.name == "test_disk").cloned().expect("dev");
    assert_eq!(d.reads, 1);
    assert_eq!(d.read_bytes, 4096);
    assert_eq!(d.read_ns, 1000);
    crate::serial_println!("  [3/8] read: OK");

    // 4: Write records count + bytes exactly from zero.
    record_write("test_disk", 8192, 2000).expect("write");
    let d = per_device().iter().find(|d| d.name == "test_disk").cloned().expect("dev");
    assert_eq!(d.writes, 1);
    assert_eq!(d.write_bytes, 8192);
    assert_eq!(d.write_ns, 2000);
    crate::serial_println!("  [4/8] write: OK");

    // 5: Discard + flush increment exactly from zero.
    record_discard("test_disk").expect("discard");
    record_flush("test_disk").expect("flush");
    let d = per_device().iter().find(|d| d.name == "test_disk").cloned().expect("dev");
    assert_eq!(d.discards, 1);
    assert_eq!(d.flushes, 1);
    crate::serial_println!("  [5/8] discard/flush: OK");

    // 6: Merge counters split read vs write exactly.
    record_merge("test_disk", false).expect("merge_r");
    record_merge("test_disk", true).expect("merge_w");
    let d = per_device().iter().find(|d| d.name == "test_disk").cloned().expect("dev");
    assert_eq!(d.merges_read, 1);
    assert_eq!(d.merges_write, 1);
    crate::serial_println!("  [6/8] merge: OK");

    // 7: Operations on an unregistered device fail with NotFound.
    assert!(record_read("nonesuch", 1, 1).is_err());
    assert!(record_write("nonesuch", 1, 1).is_err());
    crate::serial_println!("  [7/8] not found: OK");

    // 8: Aggregate totals equal the exact sums of the operations above.
    let (devs, reads, writes, rb, wb, ops) = stats();
    assert_eq!(devs, 1);
    assert_eq!(reads, 1);
    assert_eq!(writes, 1);
    assert_eq!(rb, 4096);
    assert_eq!(wb, 8192);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Leave NO residue: a diagnostic self-test must not populate the live
    // /proc/diskstat table with its fixtures.  Reset to the uninitialised state
    // so production reads report an empty table until the block layer wires
    // real accounting.
    *STATE.lock() = None;

    crate::serial_println!("diskstat::self_test() — all 8 tests passed");
}
