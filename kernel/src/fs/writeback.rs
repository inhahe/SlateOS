//! Writeback — dirty page writeback and flusher thread monitoring.
//!
//! Tracks dirty page counts, writeback rates, flusher thread
//! activity, and I/O congestion. Essential for diagnosing
//! write performance and memory pressure.
//!
//! ## Architecture
//!
//! ```text
//! Writeback statistics
//!   → writeback::record_dirty(pages) → track page dirtying
//!   → writeback::record_written(pages) → track writeback completion
//!   → writeback::start_flush(reason) → begin flush cycle
//!   → writeback::flusher_stats() → flusher thread state
//!
//! Integration:
//!   → pagestat (page statistics)
//!   → iolatency (I/O latency)
//!   → blktrace (block tracing)
//!   → memcg (memory cgroup)
//! ```

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Writeback trigger reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlushReason {
    Periodic,
    Threshold,
    Sync,
    MemPressure,
    Shutdown,
    Explicit,
}

impl FlushReason {
    pub fn label(self) -> &'static str {
        match self {
            Self::Periodic => "periodic",
            Self::Threshold => "threshold",
            Self::Sync => "sync",
            Self::MemPressure => "mem_pressure",
            Self::Shutdown => "shutdown",
            Self::Explicit => "explicit",
        }
    }
}

/// Per-device writeback state.
#[derive(Debug, Clone)]
pub struct DeviceWriteback {
    pub device: String,
    pub dirty_pages: u64,
    pub writeback_pages: u64,
    pub written_pages: u64,
    pub written_bytes: u64,
    pub flushes: u64,
    pub congestion_count: u64,
}

/// Flusher thread info.
#[derive(Debug, Clone)]
pub struct FlusherThread {
    pub id: u32,
    pub device: String,
    pub active: bool,
    pub pages_written: u64,
    pub last_flush_ns: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_DEVICES: usize = 32;
const MAX_FLUSHERS: usize = 16;

struct State {
    devices: Vec<DeviceWriteback>,
    flushers: Vec<FlusherThread>,
    total_dirty: u64,
    total_written: u64,
    total_flushes: u64,
    dirty_threshold_pct: u32,
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
            DeviceWriteback { device: String::from("sda"), dirty_pages: 5000, writeback_pages: 200, written_pages: 10_000_000, written_bytes: 40_960_000_000, flushes: 50000, congestion_count: 100 },
            DeviceWriteback { device: String::from("nvme0n1"), dirty_pages: 2000, writeback_pages: 50, written_pages: 50_000_000, written_bytes: 204_800_000_000, flushes: 100000, congestion_count: 20 },
        ],
        flushers: alloc::vec![
            FlusherThread { id: 1, device: String::from("sda"), active: false, pages_written: 10_000_000, last_flush_ns: 0 },
            FlusherThread { id: 2, device: String::from("nvme0n1"), active: false, pages_written: 50_000_000, last_flush_ns: 0 },
        ],
        total_dirty: 7000,
        total_written: 60_000_000,
        total_flushes: 150000,
        dirty_threshold_pct: 20,
        ops: 0,
    });
}

/// Record pages becoming dirty.
pub fn record_dirty(device: &str, pages: u64) -> KernelResult<()> {
    with_state(|state| {
        let dev = state.devices.iter_mut().find(|d| d.device == device)
            .ok_or(KernelError::NotFound)?;
        dev.dirty_pages += pages;
        state.total_dirty += pages;
        Ok(())
    })
}

/// Record pages written back.
pub fn record_written(device: &str, pages: u64) -> KernelResult<()> {
    with_state(|state| {
        let dev = state.devices.iter_mut().find(|d| d.device == device)
            .ok_or(KernelError::NotFound)?;
        dev.dirty_pages = dev.dirty_pages.saturating_sub(pages);
        dev.writeback_pages = dev.writeback_pages.saturating_sub(pages);
        dev.written_pages += pages;
        dev.written_bytes += pages * 4096;
        state.total_written += pages;
        Ok(())
    })
}

/// Start a flush cycle.
pub fn start_flush(device: &str, reason: FlushReason) -> KernelResult<()> {
    with_state(|state| {
        let now = crate::hpet::elapsed_ns();
        if let Some(dev) = state.devices.iter_mut().find(|d| d.device == device) {
            dev.flushes += 1;
            dev.writeback_pages = dev.dirty_pages / 2; // Move half to writeback.
        }
        if let Some(fl) = state.flushers.iter_mut().find(|f| f.device == device) {
            fl.active = true;
            fl.last_flush_ns = now;
        }
        state.total_flushes += 1;
        Ok(())
    })
}

/// Set dirty threshold percentage.
pub fn set_threshold(pct: u32) -> KernelResult<()> {
    with_state(|state| {
        state.dirty_threshold_pct = pct.min(100);
        Ok(())
    })
}

/// Get device writeback stats.
pub fn device_stats() -> Vec<DeviceWriteback> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.devices.clone())
}

/// Get flusher thread info.
pub fn flusher_stats() -> Vec<FlusherThread> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.flushers.clone())
}

/// Statistics: (device_count, total_dirty, total_written, total_flushes, threshold_pct, ops).
pub fn stats() -> (usize, u64, u64, u64, u32, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.devices.len(), s.total_dirty, s.total_written, s.total_flushes, s.dirty_threshold_pct, s.ops),
        None => (0, 0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("writeback::self_test() — running tests...");
    init_defaults();

    // 1: Defaults.
    assert_eq!(device_stats().len(), 2);
    assert_eq!(flusher_stats().len(), 2);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Record dirty.
    let before = device_stats()[0].dirty_pages;
    record_dirty("sda", 100).expect("dirty");
    let after = device_stats()[0].dirty_pages;
    assert_eq!(after, before + 100);
    crate::serial_println!("  [2/8] dirty: OK");

    // 3: Record written.
    let before = device_stats()[0].written_pages;
    record_written("sda", 50).expect("written");
    let after = device_stats()[0].written_pages;
    assert_eq!(after, before + 50);
    crate::serial_println!("  [3/8] written: OK");

    // 4: Start flush.
    start_flush("sda", FlushReason::Periodic).expect("flush");
    let fl = flusher_stats();
    assert!(fl[0].active);
    crate::serial_println!("  [4/8] flush: OK");

    // 5: Set threshold.
    set_threshold(30).expect("threshold");
    let (_, _, _, _, thr, _) = stats();
    assert_eq!(thr, 30);
    crate::serial_println!("  [5/8] threshold: OK");

    // 6: Device not found.
    assert!(record_dirty("fake", 1).is_err());
    crate::serial_println!("  [6/8] not found: OK");

    // 7: NVMe device.
    record_dirty("nvme0n1", 200).expect("nvme_dirty");
    record_written("nvme0n1", 100).expect("nvme_written");
    let dev = device_stats().iter().find(|d| d.device == "nvme0n1").cloned().unwrap();
    assert!(dev.written_pages > 50_000_000);
    crate::serial_println!("  [7/8] nvme: OK");

    // 8: Stats.
    let (devs, dirty, written, flushes, threshold, ops) = stats();
    assert_eq!(devs, 2);
    assert!(dirty > 7000);
    assert!(written > 60_000_000);
    assert!(flushes > 150000);
    assert_eq!(threshold, 30);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("writeback::self_test() — all 8 tests passed");
}
