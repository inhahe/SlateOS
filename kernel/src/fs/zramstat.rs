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

use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

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

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        devices: alloc::vec![
            ZramDevice {
                dev_id: 0, name: String::from("zram0"), disk_size: 4_000_000_000,
                orig_data_size: 2_000_000_000, compr_data_size: 800_000_000,
                mem_used: 850_000_000, reads: 5_000_000, writes: 10_000_000,
                discards: 3_000_000, zero_pages: 200_000, same_pages: 100_000,
            },
        ],
        next_id: 1,
        total_orig: 2_000_000_000,
        total_compr: 800_000_000,
        total_reads: 5_000_000,
        total_writes: 10_000_000,
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
    init_defaults();

    // 1: Defaults.
    assert_eq!(per_device().len(), 1);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Create device.
    let id = create_device("zram1", 2_000_000_000).expect("create");
    assert!(id >= 1);
    assert_eq!(per_device().len(), 2);
    crate::serial_println!("  [2/8] create: OK");

    // 3: Write.
    record_write(id, 4096, 2048).expect("write");
    let d = per_device().iter().find(|d| d.dev_id == id).cloned().unwrap();
    assert_eq!(d.writes, 1);
    assert_eq!(d.orig_data_size, 4096);
    assert_eq!(d.compr_data_size, 2048);
    crate::serial_println!("  [3/8] write: OK");

    // 4: Read.
    record_read(id).expect("read");
    let d = per_device().iter().find(|d| d.dev_id == id).cloned().unwrap();
    assert_eq!(d.reads, 1);
    crate::serial_println!("  [4/8] read: OK");

    // 5: Discard.
    let before = per_device().iter().find(|d| d.dev_id == id).cloned().unwrap().mem_used;
    record_discard(id, 1024).expect("discard");
    let after = per_device().iter().find(|d| d.dev_id == id).cloned().unwrap().mem_used;
    assert_eq!(after, before - 1024);
    crate::serial_println!("  [5/8] discard: OK");

    // 6: Compression ratio.
    let ratio = compression_ratio_x100(id);
    assert_eq!(ratio, 200); // 4096/2048 * 100 = 200
    crate::serial_println!("  [6/8] ratio: OK");

    // 7: Remove.
    remove_device(id).expect("remove");
    assert_eq!(per_device().len(), 1);
    assert!(remove_device(id).is_err());
    crate::serial_println!("  [7/8] remove: OK");

    // 8: Stats.
    let (devs, orig, compr, reads, writes, ops) = stats();
    assert_eq!(devs, 1);
    assert!(orig > 2_000_000_000);
    assert!(compr > 800_000_000);
    assert!(reads > 5_000_000);
    assert!(writes > 10_000_000);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("zramstat::self_test() — all 8 tests passed");
}
