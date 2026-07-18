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

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

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

/// Default dirty-page writeback threshold (percent of memory).  This is a real
/// configuration default (akin to Linux `vm.dirty_ratio`), not a fabricated
/// observation, so it is legitimate to seed it; it is tunable via
/// [`set_threshold`].
const DEFAULT_DIRTY_THRESHOLD_PCT: u32 = 20;

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

/// Initialise an **empty** writeback table (real default threshold, no devices).
///
/// Seeds NO devices, NO flusher threads, and zero activity counters; only the
/// real configuration default ([`DEFAULT_DIRTY_THRESHOLD_PCT`]) is set.  Real
/// writeback accounting is wired through [`register_device`] (one device + its
/// flusher thread per block device the writeback layer manages) and the
/// `record_dirty`/`record_written`/`start_flush` functions; until those are
/// called the table has no devices, so `/proc/writeback` and the `writeback`
/// kshell command report zeros rather than fabricated numbers — the kernel's hard
/// "never invent data in procfs" rule.
///
/// NOTE: this previously seeded two fictional devices ("sda": 5000 dirty / 10M
/// written pages / ~41 GB written / 50k flushes; "nvme0n1": 2000 dirty / 50M
/// written pages / ~205 GB written / 100k flushes) plus matching fake flusher
/// threads (pages_written 10M and 50M) and invented aggregate totals (total_dirty
/// 7000, total_written 60M, total_flushes 150k), which `/proc/writeback` (and the
/// `device_stats`/`flusher_stats` views) then displayed as if they were real
/// measured dirty-page writeback traffic.  That demo data was removed; the
/// self-test now builds its own fixtures explicitly via the real API (see
/// [`self_test`]).  The writeback layer is expected to call [`register_device`]
/// when a block device comes online and the record functions on every page
/// dirtying / writeback completion / flush cycle.
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        devices: Vec::new(),
        flushers: Vec::new(),
        total_dirty: 0,
        total_written: 0,
        total_flushes: 0,
        dirty_threshold_pct: DEFAULT_DIRTY_THRESHOLD_PCT,
        ops: 0,
    });
}

/// Register a block device for writeback tracking, creating its (zeroed) device
/// row and an associated idle flusher thread.
///
/// Duplicate device name fails with [`KernelError::AlreadyExists`]; exceeding
/// [`MAX_DEVICES`] or [`MAX_FLUSHERS`] fails with
/// [`KernelError::ResourceExhausted`].  Returns the assigned flusher-thread id.
pub fn register_device(device: &str) -> KernelResult<u32> {
    with_state(|state| {
        if state.devices.len() >= MAX_DEVICES || state.flushers.len() >= MAX_FLUSHERS {
            return Err(KernelError::ResourceExhausted);
        }
        if state.devices.iter().any(|d| d.device == device) {
            return Err(KernelError::AlreadyExists);
        }
        // Flusher ids are monotonic: one above the current maximum (there is no
        // unregister path, so this stays unique for the table's lifetime).
        let id = state.flushers.iter().map(|f| f.id).max().unwrap_or(0) + 1;
        state.devices.push(DeviceWriteback {
            device: String::from(device), dirty_pages: 0, writeback_pages: 0,
            written_pages: 0, written_bytes: 0, flushes: 0, congestion_count: 0,
        });
        state.flushers.push(FlusherThread {
            id, device: String::from(device), active: false,
            pages_written: 0, last_flush_ns: 0,
        });
        Ok(id)
    })
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
pub fn start_flush(device: &str, _reason: FlushReason) -> KernelResult<()> {
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
    // Begin from a clean, EMPTY table and build every fixture via the real API,
    // so the test exercises genuine accounting paths and never relies on
    // fabricated seed data (which /proc/writeback must never surface).  Resetting
    // first clears any residue from a prior `writeback test` run so the totals
    // asserted below are exact.
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty after init — no devices/flushers, zero activity, but the real
    // default dirty threshold is set.
    assert_eq!(device_stats().len(), 0);
    assert_eq!(flusher_stats().len(), 0);
    let (c0, d0, w0, f0, thr0, _o0) = stats();
    assert_eq!((c0, d0, w0, f0), (0, 0, 0, 0));
    assert_eq!(thr0, DEFAULT_DIRTY_THRESHOLD_PCT);
    assert!(record_dirty("sda", 1).is_err()); // no phantom device exists yet
    crate::serial_println!("  [1/8] empty init: OK");

    // 2: Register devices — each gets a zeroed device row + an idle flusher with
    // a monotonic id; dup name fails.
    let id1 = register_device("sda").expect("reg sda");
    let id2 = register_device("nvme0n1").expect("reg nvme");
    assert_eq!((id1, id2), (1, 2));
    assert_eq!(device_stats().len(), 2);
    assert_eq!(flusher_stats().len(), 2);
    assert!(register_device("sda").is_err());
    let dev = device_stats().into_iter().find(|d| d.device == "sda").expect("find");
    assert_eq!((dev.dirty_pages, dev.written_pages, dev.flushes), (0, 0, 0));
    crate::serial_println!("  [2/8] register: OK");

    // 3: Record dirty — count and aggregate rise.
    record_dirty("sda", 100).expect("dirty");
    let dev = device_stats().into_iter().find(|d| d.device == "sda").expect("find");
    assert_eq!(dev.dirty_pages, 100);
    crate::serial_println!("  [3/8] dirty: OK");

    // 4: Record written — written counters rise (4096 bytes/page); dirty drops.
    record_written("sda", 50).expect("written");
    let dev = device_stats().into_iter().find(|d| d.device == "sda").expect("find");
    assert_eq!(dev.written_pages, 50);
    assert_eq!(dev.written_bytes, 50 * 4096);
    assert_eq!(dev.dirty_pages, 50); // 100 - 50
    crate::serial_println!("  [4/8] written: OK");

    // 5: Start flush — flusher goes active, flush counted, half the remaining
    // dirty pages move to writeback (50 / 2 = 25).
    start_flush("sda", FlushReason::Periodic).expect("flush");
    let dev = device_stats().into_iter().find(|d| d.device == "sda").expect("find");
    let fl = flusher_stats().into_iter().find(|f| f.device == "sda").expect("find fl");
    assert_eq!(dev.flushes, 1);
    assert_eq!(dev.writeback_pages, 25);
    assert!(fl.active);
    crate::serial_println!("  [5/8] flush: OK");

    // 6: Threshold is tunable and clamped to 100.
    set_threshold(30).expect("threshold");
    let (_, _, _, _, thr, _) = stats();
    assert_eq!(thr, 30);
    set_threshold(250).expect("threshold_clamp");
    let (_, _, _, _, thr, _) = stats();
    assert_eq!(thr, 100);
    set_threshold(30).expect("threshold_reset");
    crate::serial_println!("  [6/8] threshold: OK");

    // 7: Unknown device → NotFound on every record path.
    assert!(record_dirty("fake", 1).is_err());
    assert!(record_written("fake", 1).is_err());
    record_dirty("nvme0n1", 200).expect("nvme_dirty");
    record_written("nvme0n1", 100).expect("nvme_written");
    let dev = device_stats().into_iter().find(|d| d.device == "nvme0n1").expect("find");
    assert_eq!(dev.written_pages, 100);
    assert_eq!(dev.dirty_pages, 100); // 200 - 100
    crate::serial_println!("  [7/8] nvme + not found: OK");

    // 8: Aggregate totals are exact: dirty 100 (sda) + 200 (nvme) = 300;
    // written 50 (sda) + 100 (nvme) = 150; 1 flush; threshold 30.
    let (devs, dirty, written, flushes, threshold, ops) = stats();
    assert_eq!(devs, 2);
    assert_eq!(dirty, 300);
    assert_eq!(written, 150);
    assert_eq!(flushes, 1);
    assert_eq!(threshold, 30);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Leave NO residue: reset to the uninitialised state so a diagnostic run
    // never leaves fixtures resident in the live /proc/writeback table.
    *STATE.lock() = None;

    crate::serial_println!("writeback::self_test() — all 8 tests passed");
}
