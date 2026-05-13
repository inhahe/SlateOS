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

use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

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

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        devices: alloc::vec![
            DeviceCacheStats { device: String::from("sda"), cached_pages: 500_000, hits: 100_000_000, misses: 5_000_000, evictions: 2_000_000, dirty_pages: 5000, writeback_pages: 200, readahead_pages: 10_000_000, readahead_useful: 8_000_000 },
            DeviceCacheStats { device: String::from("nvme0n1"), cached_pages: 2_000_000, hits: 500_000_000, misses: 10_000_000, evictions: 5_000_000, dirty_pages: 2000, writeback_pages: 50, readahead_pages: 50_000_000, readahead_useful: 45_000_000 },
        ],
        total_hits: 600_000_000,
        total_misses: 15_000_000,
        total_evictions: 7_000_000,
        total_readahead: 60_000_000,
        total_readahead_useful: 53_000_000,
        ops: 0,
    });
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
    init_defaults();

    // 1: Defaults.
    assert_eq!(per_device().len(), 2);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Cache hit.
    let before = per_device()[0].hits;
    record_hit("sda").expect("hit");
    let after = per_device()[0].hits;
    assert_eq!(after, before + 1);
    crate::serial_println!("  [2/8] hit: OK");

    // 3: Cache miss.
    let before = per_device()[0].cached_pages;
    record_miss("sda").expect("miss");
    let after = per_device()[0].cached_pages;
    assert_eq!(after, before + 1);
    crate::serial_println!("  [3/8] miss: OK");

    // 4: Eviction.
    let before = per_device()[0].cached_pages;
    record_eviction("sda", 10).expect("evict");
    let after = per_device()[0].cached_pages;
    assert_eq!(after, before - 10);
    crate::serial_println!("  [4/8] eviction: OK");

    // 5: Readahead.
    record_readahead("sda", 100, 80).expect("readahead");
    let dev = per_device()[0].clone();
    assert!(dev.readahead_pages > 10_000_000);
    crate::serial_println!("  [5/8] readahead: OK");

    // 6: Hit rate.
    let rate = hit_rate();
    assert!(rate > 9000); // > 90%.
    crate::serial_println!("  [6/8] hit rate: OK");

    // 7: Not found.
    assert!(record_hit("fake").is_err());
    crate::serial_println!("  [7/8] not found: OK");

    // 8: Stats.
    let (devs, hits, misses, evictions, readahead, ops) = stats();
    assert_eq!(devs, 2);
    assert!(hits > 600_000_000);
    assert!(misses > 15_000_000);
    assert!(evictions > 7_000_000);
    assert!(readahead > 60_000_000);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("pagecache::self_test() — all 8 tests passed");
}
