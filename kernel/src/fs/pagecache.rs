//! Page Cache — file-backed page cache monitoring.
//!
//! Tracks page cache hits, misses, evictions, readahead
//! effectiveness, and dirty page ratios. Essential for
//! diagnosing I/O performance and memory pressure.
//!
//! ## Architecture
//!
//! ```text
//! Page cache monitoring
//!   → pagecache::record_hit() → cache hit
//!   → pagecache::record_miss() → cache miss (disk read)
//!   → pagecache::record_eviction(pages) → pages evicted
//!   → pagecache::record_readahead(requested, useful) → readahead stats
//!
//! Integration:
//!   → writeback (dirty page writeback)
//!   → inodestat (inode cache)
//!   → pagestat (page allocator)
//!   → fscache (filesystem cache)
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

/// Cache operation type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheOp {
    Hit,
    Miss,
    Eviction,
    Writeback,
    Readahead,
    Invalidate,
}

impl CacheOp {
    pub fn label(self) -> &'static str {
        match self {
            Self::Hit => "hit",
            Self::Miss => "miss",
            Self::Eviction => "eviction",
            Self::Writeback => "writeback",
            Self::Readahead => "readahead",
            Self::Invalidate => "invalidate",
        }
    }
}

/// Per-device cache stats.
#[derive(Debug, Clone)]
pub struct DeviceCacheStats {
    pub device: String,
    pub cached_pages: u64,
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    pub dirty_pages: u64,
    pub writeback_pages: u64,
    pub readahead_pages: u64,
    pub readahead_useful: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_DEVICES: usize = 16;

struct State {
    devices: Vec<DeviceCacheStats>,
    total_hits: u64,
    total_misses: u64,
    total_evictions: u64,
    total_readahead: u64,
    total_readahead_useful: u64,
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

/// Initialise an **empty** page-cache table.
///
/// Seeds NO devices and zero counters.  Real cache accounting is wired through
/// [`register_device`] (one row per backing device the page cache tracks) and
/// the `record_hit`/`record_miss`/`record_eviction`/`record_readahead`
/// functions; until those are called the table is genuinely empty, so
/// `/proc/pagecache` and the `pagecache` kshell command report zeros rather than
/// fabricated numbers — the kernel's hard "never invent data in procfs" rule.
///
/// NOTE: this previously seeded two fictional devices ("sda": 500k cached pages /
/// 100M hits / 5M misses / 2M evictions / 5000 dirty / 200 writeback / 10M
/// readahead / 8M useful; "nvme0n1": 2M cached / 500M hits / 10M misses / 5M
/// evictions / 2000 dirty / 50 writeback / 50M readahead / 45M useful) plus
/// invented aggregate totals (total_hits 600M, total_misses 15M, total_evictions
/// 7M, total_readahead 60M, total_readahead_useful 53M), which `/proc/pagecache`
/// (and the `per_device`/`hit_rate`/`readahead_rate` views) then displayed as if
/// they were real measured cache traffic — a 97.5% hit rate and 88% readahead
/// effectiveness conjured from nothing.  That demo data was removed; the
/// self-test now builds its own fixtures explicitly via the real API (see
/// [`self_test`]).  The VFS/mm layer is expected to call [`register_device`] when
/// a backing device is tracked and the record functions on every cache event.
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        devices: Vec::new(),
        total_hits: 0,
        total_misses: 0,
        total_evictions: 0,
        total_readahead: 0,
        total_readahead_useful: 0,
        ops: 0,
    });
}

/// Register a backing device for page-cache accounting.
///
/// Creates a zeroed [`DeviceCacheStats`] row.  Duplicate device names return
/// [`KernelError::AlreadyExists`]; exceeding [`MAX_DEVICES`] returns
/// [`KernelError::ResourceExhausted`].
pub fn register_device(device: &str) -> KernelResult<()> {
    with_state(|state| {
        if state.devices.len() >= MAX_DEVICES { return Err(KernelError::ResourceExhausted); }
        if state.devices.iter().any(|d| d.device == device) { return Err(KernelError::AlreadyExists); }
        state.devices.push(DeviceCacheStats {
            device: String::from(device), cached_pages: 0, hits: 0, misses: 0,
            evictions: 0, dirty_pages: 0, writeback_pages: 0,
            readahead_pages: 0, readahead_useful: 0,
        });
        Ok(())
    })
}

/// Record a cache hit.
pub fn record_hit(device: &str) -> KernelResult<()> {
    with_state(|state| {
        let dev = state.devices.iter_mut().find(|d| d.device == device)
            .ok_or(KernelError::NotFound)?;
        dev.hits += 1;
        state.total_hits += 1;
        Ok(())
    })
}

/// Record a cache miss.
pub fn record_miss(device: &str) -> KernelResult<()> {
    with_state(|state| {
        let dev = state.devices.iter_mut().find(|d| d.device == device)
            .ok_or(KernelError::NotFound)?;
        dev.misses += 1;
        dev.cached_pages += 1; // Page now cached.
        state.total_misses += 1;
        Ok(())
    })
}

/// Record page eviction.
pub fn record_eviction(device: &str, pages: u64) -> KernelResult<()> {
    with_state(|state| {
        let dev = state.devices.iter_mut().find(|d| d.device == device)
            .ok_or(KernelError::NotFound)?;
        dev.evictions += pages;
        dev.cached_pages = dev.cached_pages.saturating_sub(pages);
        state.total_evictions += pages;
        Ok(())
    })
}

/// Record readahead pages.
pub fn record_readahead(device: &str, pages: u64, useful: u64) -> KernelResult<()> {
    with_state(|state| {
        let dev = state.devices.iter_mut().find(|d| d.device == device)
            .ok_or(KernelError::NotFound)?;
        dev.readahead_pages += pages;
        dev.readahead_useful += useful;
        dev.cached_pages += pages;
        state.total_readahead += pages;
        state.total_readahead_useful += useful;
        Ok(())
    })
}

/// Get per-device cache stats.
pub fn per_device() -> Vec<DeviceCacheStats> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.devices.clone())
}

/// Cache hit rate as percentage * 100 (integer math).
pub fn hit_rate() -> u64 {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => {
            let total = s.total_hits + s.total_misses;
            if total == 0 { return 0; }
            s.total_hits * 10000 / total
        }
        None => 0,
    }
}

/// Readahead effectiveness as percentage * 100.
pub fn readahead_rate() -> u64 {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => {
            if s.total_readahead == 0 { return 0; }
            s.total_readahead_useful * 10000 / s.total_readahead
        }
        None => 0,
    }
}

/// Statistics: (device_count, total_hits, total_misses, total_evictions, total_readahead, ops).
pub fn stats() -> (usize, u64, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.devices.len(), s.total_hits, s.total_misses, s.total_evictions, s.total_readahead, s.ops),
        None => (0, 0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("pagecache::self_test() — running tests...");
    // Begin from a clean, EMPTY table and build every fixture via the real API,
    // so the test exercises genuine accounting paths and never relies on
    // fabricated seed data (which /proc/pagecache must never surface).  Resetting
    // first clears any residue from a prior `pagecache test` run so the totals
    // and rates asserted below are exact.
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty after init — no fabricated devices or counters; rates default to
    // 0 with no traffic; record on an unregistered device fails.
    assert_eq!(per_device().len(), 0);
    let (c0, h0, m0, e0, r0, _o0) = stats();
    assert_eq!((c0, h0, m0, e0, r0), (0, 0, 0, 0, 0));
    assert_eq!(hit_rate(), 0);
    assert_eq!(readahead_rate(), 0);
    assert!(record_hit("sda").is_err()); // no phantom device exists yet
    crate::serial_println!("  [1/8] empty init: OK");

    // 2: Register — zeroed counters; dup fails.
    register_device("sda").expect("register");
    let d = per_device().into_iter().find(|d| d.device == "sda").expect("find");
    assert_eq!((d.hits, d.misses, d.evictions, d.cached_pages), (0, 0, 0, 0));
    assert!(register_device("sda").is_err());
    crate::serial_println!("  [2/8] register: OK");

    // 3: Cache hit — per-device + total hits rise by one.
    record_hit("sda").expect("hit");
    assert_eq!(per_device()[0].hits, 1);
    crate::serial_println!("  [3/8] hit: OK");

    // 4: Cache miss — miss caches the page (cached_pages +1).
    record_miss("sda").expect("miss");
    let d = &per_device()[0];
    assert_eq!(d.misses, 1);
    assert_eq!(d.cached_pages, 1);
    crate::serial_println!("  [4/8] miss: OK");

    // 5: Eviction — cached_pages drops, saturating at 0 on over-eviction.
    record_eviction("sda", 1).expect("evict");
    assert_eq!(per_device()[0].cached_pages, 0);
    record_eviction("sda", 100).expect("evict over"); // saturating_sub guard
    assert_eq!(per_device()[0].cached_pages, 0);
    crate::serial_println!("  [5/8] eviction: OK");

    // 6: Readahead + rates — 100 readahead / 80 useful = 80% effectiveness;
    // 1 hit / 1 miss = 50% hit rate (rates are pct*100 integer math).
    record_readahead("sda", 100, 80).expect("readahead");
    let d = &per_device()[0];
    assert_eq!(d.readahead_pages, 100);
    assert_eq!(d.readahead_useful, 80);
    assert_eq!(readahead_rate(), 8000); // 80 * 10000 / 100
    assert_eq!(hit_rate(), 5000);       // 1 * 10000 / (1 + 1)
    crate::serial_println!("  [6/8] readahead + rates: OK");

    // 7: Unknown device → NotFound on every record path.
    assert!(record_hit("fake").is_err());
    assert!(record_miss("fake").is_err());
    assert!(record_eviction("fake", 1).is_err());
    assert!(record_readahead("fake", 1, 1).is_err());
    crate::serial_println!("  [7/8] not found: OK");

    // 8: Aggregate totals are exact: 1 hit, 1 miss, 101 evicted pages (1 + 100
    // attempted; total counts all attempts), 100 readahead pages.
    let (devs, hits, misses, evictions, readahead, ops) = stats();
    assert_eq!(devs, 1);
    assert_eq!(hits, 1);
    assert_eq!(misses, 1);
    assert_eq!(evictions, 101);
    assert_eq!(readahead, 100);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Leave NO residue: reset to the uninitialised state so a diagnostic run
    // never leaves fixtures resident in the live /proc/pagecache table.
    *STATE.lock() = None;

    crate::serial_println!("pagecache::self_test() — all 8 tests passed");
}
