//! Cache Info — CPU cache hierarchy and utilization monitoring.
//!
//! Tracks L1/L2/L3 cache sizes, hit/miss rates, and
//! cache line utilization. Essential for performance
//! optimization and cache-aware scheduling.
//!
//! ## Architecture
//!
//! ```text
//! CPU cache monitoring
//!   → cacheinfo::record_hit(level) → cache hit
//!   → cacheinfo::record_miss(level) → cache miss
//!   → cacheinfo::topology() → cache hierarchy
//!   → cacheinfo::hit_rate(level) → hit rate %
//!
//! Integration:
//!   → cpustat (CPU utilization)
//!   → cputopo (CPU topology)
//!   → tlbstat (TLB stats)
//!   → perfmon (performance monitor)
//! ```

use alloc::vec::Vec;
use alloc::format;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Cache level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheLevel {
    L1d,
    L1i,
    L2,
    L3,
}

impl CacheLevel {
    pub fn label(self) -> &'static str {
        match self {
            Self::L1d => "L1d",
            Self::L1i => "L1i",
            Self::L2 => "L2",
            Self::L3 => "L3",
        }
    }

    pub fn index(self) -> usize {
        match self {
            Self::L1d => 0,
            Self::L1i => 1,
            Self::L2 => 2,
            Self::L3 => 3,
        }
    }
}

/// Per-level cache info.
#[derive(Debug, Clone)]
pub struct CacheLevelInfo {
    pub level: CacheLevel,
    pub size_kb: u32,
    pub line_size: u32,
    pub ways: u32,
    pub sets: u32,
    pub shared_cpus: u32,
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct State {
    caches: [CacheLevelInfo; 4],
    total_hits: u64,
    total_misses: u64,
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
        caches: [
            CacheLevelInfo { level: CacheLevel::L1d, size_kb: 32, line_size: 64, ways: 8, sets: 64, shared_cpus: 1, hits: 10_000_000_000, misses: 500_000_000, evictions: 400_000_000 },
            CacheLevelInfo { level: CacheLevel::L1i, size_kb: 32, line_size: 64, ways: 8, sets: 64, shared_cpus: 1, hits: 8_000_000_000, misses: 200_000_000, evictions: 150_000_000 },
            CacheLevelInfo { level: CacheLevel::L2, size_kb: 256, line_size: 64, ways: 8, sets: 512, shared_cpus: 1, hits: 5_000_000_000, misses: 100_000_000, evictions: 80_000_000 },
            CacheLevelInfo { level: CacheLevel::L3, size_kb: 8192, line_size: 64, ways: 16, sets: 8192, shared_cpus: 4, hits: 2_000_000_000, misses: 50_000_000, evictions: 40_000_000 },
        ],
        total_hits: 25_000_000_000,
        total_misses: 850_000_000,
        ops: 0,
    });
}

/// Record a cache hit.
pub fn record_hit(level: CacheLevel) -> KernelResult<()> {
    with_state(|state| {
        state.caches[level.index()].hits += 1;
        state.total_hits += 1;
        Ok(())
    })
}

/// Record a cache miss.
pub fn record_miss(level: CacheLevel) -> KernelResult<()> {
    with_state(|state| {
        state.caches[level.index()].misses += 1;
        state.total_misses += 1;
        Ok(())
    })
}

/// Record eviction.
pub fn record_eviction(level: CacheLevel) -> KernelResult<()> {
    with_state(|state| {
        state.caches[level.index()].evictions += 1;
        Ok(())
    })
}

/// Cache topology/info.
pub fn topology() -> Vec<CacheLevelInfo> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.caches.to_vec())
}

/// Hit rate for a cache level, percentage * 100.
pub fn hit_rate(level: CacheLevel) -> u64 {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => {
            let c = &s.caches[level.index()];
            let total = c.hits + c.misses;
            if total == 0 { return 0; }
            c.hits * 10000 / total
        }
        None => 0,
    }
}

/// Overall hit rate * 100.
pub fn overall_hit_rate() -> u64 {
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

/// Statistics: (levels, total_hits, total_misses, ops).
pub fn stats() -> (usize, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (4, s.total_hits, s.total_misses, s.ops),
        None => (0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("cpucache::self_test() — running tests...");
    init_defaults();

    // 1: Defaults.
    assert_eq!(topology().len(), 4);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Hit.
    let before = topology()[0].hits;
    record_hit(CacheLevel::L1d).expect("hit");
    let after = topology()[0].hits;
    assert_eq!(after, before + 1);
    crate::serial_println!("  [2/8] hit: OK");

    // 3: Miss.
    let before = topology()[2].misses;
    record_miss(CacheLevel::L2).expect("miss");
    let after = topology()[2].misses;
    assert_eq!(after, before + 1);
    crate::serial_println!("  [3/8] miss: OK");

    // 4: Eviction.
    let before = topology()[3].evictions;
    record_eviction(CacheLevel::L3).expect("evict");
    let after = topology()[3].evictions;
    assert_eq!(after, before + 1);
    crate::serial_println!("  [4/8] eviction: OK");

    // 5: Hit rate per level.
    let rate = hit_rate(CacheLevel::L1d);
    assert!(rate > 9000); // > 90%.
    crate::serial_println!("  [5/8] hit rate: OK");

    // 6: Overall hit rate.
    let rate = overall_hit_rate();
    assert!(rate > 9000);
    crate::serial_println!("  [6/8] overall rate: OK");

    // 7: Topology.
    let topo = topology();
    assert_eq!(topo[0].size_kb, 32);
    assert_eq!(topo[3].size_kb, 8192);
    assert_eq!(topo[3].shared_cpus, 4);
    crate::serial_println!("  [7/8] topology: OK");

    // 8: Stats.
    let (levels, hits, misses, ops) = stats();
    assert_eq!(levels, 4);
    assert!(hits > 25_000_000_000);
    assert!(misses > 850_000_000);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("cpucache::self_test() — all 8 tests passed");
}
