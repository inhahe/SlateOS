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

#![allow(dead_code)]

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

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

fn empty_level(level: CacheLevel) -> CacheLevelInfo {
    CacheLevelInfo {
        level, size_kb: 0, line_size: 0, ways: 0, sets: 0, shared_cpus: 0,
        hits: 0, misses: 0, evictions: 0,
    }
}

/// Initialise the CPU-cache statistics state.
///
/// The four cache levels (L1d, L1i, L2, L3) are a fixed taxonomy so the rows
/// are always present, but with ZEROED geometry and hit/miss/eviction
/// counters. Real cache geometry (size, line size, ways, sets, shared-CPU
/// count) is filled in through [`set_geometry`] once a CPUID-probe routine has
/// read it from the hardware, and the hit/miss/eviction counters advance only
/// through real [`record_hit`] / [`record_miss`] / [`record_eviction`] calls.
/// The `/proc/cpucache` generator and the `cpucache` kshell command surface
/// this table (and [`topology`] / [`hit_rate`]) as if it reflects the real
/// cache hierarchy and its measured activity, so seeding it with invented
/// geometry or counters would be fabricated procfs data.
///
/// (Previously this seeded a plausible-looking but unprobed hierarchy —
/// 32KB 8-way L1d/L1i, 256KB L2, 8MB 16-way L3 shared by 4 CPUs — with
/// fabricated activity of 25,000,000,000 total hits and 850,000,000 misses
/// across the levels.)
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        caches: [
            empty_level(CacheLevel::L1d),
            empty_level(CacheLevel::L1i),
            empty_level(CacheLevel::L2),
            empty_level(CacheLevel::L3),
        ],
        total_hits: 0,
        total_misses: 0,
        ops: 0,
    });
}

/// Set the geometry for a cache level from probed CPUID data.
///
/// This populates the hardware description of a level (size, line size,
/// associativity ways, sets, and the number of CPUs sharing the cache) without
/// touching its hit/miss/eviction counters. Intended to be called by the
/// CPUID cache-topology probe at startup.
pub fn set_geometry(level: CacheLevel, size_kb: u32, line_size: u32, ways: u32, sets: u32, shared_cpus: u32) -> KernelResult<()> {
    with_state(|state| {
        let c = &mut state.caches[level.index()];
        c.size_kb = size_kb;
        c.line_size = line_size;
        c.ways = ways;
        c.sets = sets;
        c.shared_cpus = shared_cpus;
        Ok(())
    })
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
    // Start from a clean, empty state so the assertions below are exact and
    // no fixtures leak into the live cache table afterwards.
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty defaults — four zeroed levels, zero totals, zero overall rate.
    let topo = topology();
    assert_eq!(topo.len(), 4);
    for c in &topo {
        assert_eq!((c.size_kb, c.line_size, c.ways, c.sets, c.shared_cpus), (0, 0, 0, 0, 0));
        assert_eq!((c.hits, c.misses, c.evictions), (0, 0, 0));
    }
    let (l0, h0, m0, _) = stats();
    assert_eq!((l0, h0, m0), (4, 0, 0));
    assert_eq!(overall_hit_rate(), 0);
    crate::serial_println!("  [1/8] empty defaults: OK");

    // 2: Geometry — set_geometry populates a level's hardware description
    //    without touching its counters.
    set_geometry(CacheLevel::L1d, 32, 64, 8, 64, 1).expect("geometry");
    let c = topology()[CacheLevel::L1d.index()].clone();
    assert_eq!((c.size_kb, c.line_size, c.ways, c.sets, c.shared_cpus), (32, 64, 8, 64, 1));
    assert_eq!((c.hits, c.misses), (0, 0));
    crate::serial_println!("  [2/8] geometry: OK");

    // 3: Hits — three L1d hits advance the level and the global total.
    for _ in 0..3 { record_hit(CacheLevel::L1d).expect("hit"); }
    assert_eq!(topology()[CacheLevel::L1d.index()].hits, 3);
    assert_eq!(stats().1, 3); // total_hits
    crate::serial_println!("  [3/8] hit: OK");

    // 4: Misses — one L1d miss and one L2 miss; per-level and global advance.
    record_miss(CacheLevel::L1d).expect("miss_l1d");
    record_miss(CacheLevel::L2).expect("miss_l2");
    assert_eq!(topology()[CacheLevel::L1d.index()].misses, 1);
    assert_eq!(topology()[CacheLevel::L2.index()].misses, 1);
    assert_eq!(stats().2, 2); // total_misses
    crate::serial_println!("  [4/8] miss: OK");

    // 5: Eviction — L3 eviction counter advances (no global eviction total).
    record_eviction(CacheLevel::L3).expect("evict");
    assert_eq!(topology()[CacheLevel::L3.index()].evictions, 1);
    crate::serial_println!("  [5/8] eviction: OK");

    // 6: Hit rate — L1d saw 3 hits / 1 miss → 7500 (75.00%); an untouched level
    //    (L1i) reports 0.
    assert_eq!(hit_rate(CacheLevel::L1d), 7500);
    assert_eq!(hit_rate(CacheLevel::L1i), 0);
    crate::serial_println!("  [6/8] hit rate: OK");

    // 7: Overall hit rate — 3 hits / 2 misses across all levels → 6000 (60.00%).
    assert_eq!(overall_hit_rate(), 6000);
    crate::serial_println!("  [7/8] overall rate: OK");

    // 8: Final stats reflect only the real activity above; the level→index
    //    mapping is intact (each row's level matches its slot).
    let topo = topology();
    for (i, c) in topo.iter().enumerate() {
        assert_eq!(c.level.index(), i);
    }
    let (levels, hits, misses, ops) = stats();
    assert_eq!((levels, hits, misses), (4, 3, 2));
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Leave no residue in the live state.
    *STATE.lock() = None;
    crate::serial_println!("cpucache::self_test() — all 8 tests passed");
}
