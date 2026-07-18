//! ZRAM Statistics — compressed RAM swap monitoring.
//!
//! Tracks ZRAM device usage, compression ratios, read/write
//! operations, and memory savings. Essential for understanding
//! compressed swap performance.
//!
//! ## Architecture
//!
//! ```text
//! ZRAM monitoring
//!   → zramstat::record_write(dev, orig, compr) → compressed write
//!   → zramstat::record_read(dev) → read from ZRAM
//!   → zramstat::record_discard(dev, bytes) → discard/free
//!   → zramstat::per_device() → per-device stats
//!
//! Integration:
//!   → swapmon (swap monitoring)
//!   → mempress (memory pressure)
//!   → compstat (compaction)
//!   → pagestat (page allocator)
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

/// ZRAM device stats.
#[derive(Debug, Clone)]
pub struct ZramDevice {
    pub dev_id: u32,
    pub name: String,
    pub disk_size: u64,
    pub orig_data_size: u64,
    pub compr_data_size: u64,
    pub mem_used: u64,
    pub reads: u64,
    pub writes: u64,
    pub discards: u64,
    pub zero_pages: u64,
    pub same_pages: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_DEVICES: usize = 16;

struct State {
    devices: Vec<ZramDevice>,
    next_id: u32,
    total_orig: u64,
    total_compr: u64,
    total_reads: u64,
    total_writes: u64,
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

/// Initialise an **empty** ZRAM statistics table.
///
/// Seeds NO devices and zero counters.  Real ZRAM accounting is wired through
/// [`create_device`] (one row per ZRAM swap device the mm/swap layer brings up)
/// and the `record_write`/`record_read`/`record_discard` functions; until those
/// are called the table is genuinely empty, so `/proc/zramstat` and the
/// `zramstat` kshell command report zeros rather than fabricated numbers — the
/// kernel's hard "never invent data in procfs" rule.
///
/// NOTE: this previously seeded one fictional device ("zram0": 4GB disk / 2GB
/// original data / 800MB compressed / 850MB mem used / 5M reads / 10M writes / 3M
/// discards / 200k zero pages / 100k same pages) plus invented aggregate totals
/// (total_orig 2GB, total_compr 800MB, total_reads 5M, total_writes 10M), which
/// `/proc/zramstat` (and the `per_device`/`compression_ratio_x100` views) then
/// displayed as if they were real measured compressed-swap usage.  That demo data
/// was removed; the self-test now builds its own fixtures explicitly via the real
/// API (see [`self_test`]).  The swap layer is expected to call [`create_device`]
/// when a ZRAM device is configured and the record functions on every
/// write/read/discard.
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        devices: Vec::new(),
        next_id: 0,
        total_orig: 0,
        total_compr: 0,
        total_reads: 0,
        total_writes: 0,
        ops: 0,
    });
}

/// Create a ZRAM device.
pub fn create_device(name: &str, disk_size: u64) -> KernelResult<u32> {
    with_state(|state| {
        if state.devices.len() >= MAX_DEVICES { return Err(KernelError::ResourceExhausted); }
        let id = state.next_id;
        state.next_id += 1;
        state.devices.push(ZramDevice {
            dev_id: id, name: String::from(name), disk_size,
            orig_data_size: 0, compr_data_size: 0, mem_used: 0,
            reads: 0, writes: 0, discards: 0, zero_pages: 0, same_pages: 0,
        });
        Ok(id)
    })
}

/// Remove a ZRAM device.
pub fn remove_device(dev_id: u32) -> KernelResult<()> {
    with_state(|state| {
        let idx = state.devices.iter().position(|d| d.dev_id == dev_id)
            .ok_or(KernelError::NotFound)?;
        state.devices.remove(idx);
        Ok(())
    })
}

/// Record a compressed write.
pub fn record_write(dev_id: u32, orig_bytes: u64, compr_bytes: u64) -> KernelResult<()> {
    with_state(|state| {
        let d = state.devices.iter_mut().find(|d| d.dev_id == dev_id)
            .ok_or(KernelError::NotFound)?;
        d.writes += 1;
        d.orig_data_size += orig_bytes;
        d.compr_data_size += compr_bytes;
        d.mem_used += compr_bytes;
        if orig_bytes == 0 { d.zero_pages += 1; }
        state.total_orig += orig_bytes;
        state.total_compr += compr_bytes;
        state.total_writes += 1;
        Ok(())
    })
}

/// Record a read.
pub fn record_read(dev_id: u32) -> KernelResult<()> {
    with_state(|state| {
        let d = state.devices.iter_mut().find(|d| d.dev_id == dev_id)
            .ok_or(KernelError::NotFound)?;
        d.reads += 1;
        state.total_reads += 1;
        Ok(())
    })
}

/// Record a discard (page freed from ZRAM).
pub fn record_discard(dev_id: u32, bytes: u64) -> KernelResult<()> {
    with_state(|state| {
        let d = state.devices.iter_mut().find(|d| d.dev_id == dev_id)
            .ok_or(KernelError::NotFound)?;
        d.discards += 1;
        d.mem_used = d.mem_used.saturating_sub(bytes);
        Ok(())
    })
}

/// Per-device stats.
pub fn per_device() -> Vec<ZramDevice> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.devices.clone())
}

/// Compression ratio (x100 for integer precision). Higher = better.
pub fn compression_ratio_x100(dev_id: u32) -> u64 {
    let guard = STATE.lock();
    guard.as_ref().and_then(|s| {
        s.devices.iter().find(|d| d.dev_id == dev_id).map(|d| {
            if d.compr_data_size > 0 { d.orig_data_size * 100 / d.compr_data_size } else { 0 }
        })
    }).unwrap_or(0)
}

/// Statistics: (device_count, total_orig, total_compr, total_reads, total_writes, ops).
pub fn stats() -> (usize, u64, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.devices.len(), s.total_orig, s.total_compr, s.total_reads, s.total_writes, s.ops),
        None => (0, 0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("zramstat::self_test() — running tests...");
    // Begin from a clean, EMPTY table and build every fixture via the real API,
    // so the test exercises genuine accounting paths and never relies on
    // fabricated seed data (which /proc/zramstat must never surface).  Resetting
    // first clears any residue from a prior `zramstat test` run so the totals
    // asserted below are exact.
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty after init — no fabricated devices or counters.
    assert_eq!(per_device().len(), 0);
    let (c0, o0, cp0, r0, w0, _op0) = stats();
    assert_eq!((c0, o0, cp0, r0, w0), (0, 0, 0, 0, 0));
    assert!(record_read(0).is_err()); // no phantom device exists yet
    crate::serial_println!("  [1/8] empty init: OK");

    // 2: Create device — zeroed counters, disk_size preserved.
    let id = create_device("zram0", 2_000_000_000).expect("create");
    assert_eq!(per_device().len(), 1);
    let d = per_device().into_iter().find(|d| d.dev_id == id).expect("find");
    assert_eq!(d.disk_size, 2_000_000_000);
    assert_eq!((d.orig_data_size, d.compr_data_size, d.reads, d.writes), (0, 0, 0, 0));
    crate::serial_println!("  [2/8] create: OK");

    // 3: Write — counts and sizes accumulate; mem_used grows by compressed size.
    record_write(id, 4096, 2048).expect("write");
    let d = per_device().into_iter().find(|d| d.dev_id == id).expect("find");
    assert_eq!(d.writes, 1);
    assert_eq!(d.orig_data_size, 4096);
    assert_eq!(d.compr_data_size, 2048);
    assert_eq!(d.mem_used, 2048);
    crate::serial_println!("  [3/8] write: OK");

    // 4: Read.
    record_read(id).expect("read");
    let d = per_device().into_iter().find(|d| d.dev_id == id).expect("find");
    assert_eq!(d.reads, 1);
    crate::serial_println!("  [4/8] read: OK");

    // 5: Discard — mem_used drops by the freed bytes (saturating).
    record_discard(id, 1024).expect("discard");
    let d = per_device().into_iter().find(|d| d.dev_id == id).expect("find");
    assert_eq!(d.discards, 1);
    assert_eq!(d.mem_used, 1024); // 2048 - 1024
    record_discard(id, 99_999).expect("over_discard"); // saturates, no underflow
    let d = per_device().into_iter().find(|d| d.dev_id == id).expect("find");
    assert_eq!(d.mem_used, 0);
    crate::serial_println!("  [5/8] discard: OK");

    // 6: Compression ratio = orig*100/compr = 4096*100/2048 = 200.
    assert_eq!(compression_ratio_x100(id), 200);
    crate::serial_println!("  [6/8] ratio: OK");

    // 7: Remove — gone from the table; second remove and records → NotFound.
    remove_device(id).expect("remove");
    assert_eq!(per_device().len(), 0);
    assert!(remove_device(id).is_err());
    assert!(record_write(id, 1, 1).is_err());
    assert!(record_read(id).is_err());
    crate::serial_println!("  [7/8] remove + not found: OK");

    // 8: Aggregate stats are exact: 1 device created+removed (count 0 now), the
    // 4096/2048 write counted, 1 read, 1 write.
    let (devs, orig, compr, reads, writes, ops) = stats();
    assert_eq!(devs, 0);
    assert_eq!(orig, 4096);
    assert_eq!(compr, 2048);
    assert_eq!(reads, 1);
    assert_eq!(writes, 1);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Leave NO residue: reset to the uninitialised state so a diagnostic run
    // never leaves fixtures resident in the live /proc/zramstat table.
    *STATE.lock() = None;

    crate::serial_println!("zramstat::self_test() — all 8 tests passed");
}
