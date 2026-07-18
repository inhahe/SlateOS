//! Page Table Statistics — page table allocation and usage monitoring.
//!
//! Tracks page table page allocations, levels used, TLB flushes,
//! and page walk costs. Essential for virtual memory performance
//! diagnostics.
//!
//! ## Architecture
//!
//! ```text
//! Page table monitoring
//!   → pgtable::record_alloc(level) → page table page allocated
//!   → pgtable::record_free(level) → page table page freed
//!   → pgtable::record_walk(levels) → page walk event
//!   → pgtable::record_tlb_flush(scope) → TLB flush
//!
//! Integration:
//!   → pagestat (page allocator)
//!   → tlbstat (TLB statistics)
//!   → pftrack (page fault tracking)
//!   → thpstat (transparent huge pages)
//! ```

#![allow(dead_code)]

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Page table level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PtLevel {
    Pml4,  // Level 4 (PGD)
    Pdpt,  // Level 3 (PUD)
    Pd,    // Level 2 (PMD)
    Pt,    // Level 1 (PTE)
}

impl PtLevel {
    pub fn label(self) -> &'static str {
        match self {
            Self::Pml4 => "PML4",
            Self::Pdpt => "PDPT",
            Self::Pd => "PD",
            Self::Pt => "PT",
        }
    }
    pub fn index(self) -> usize {
        match self {
            Self::Pml4 => 0,
            Self::Pdpt => 1,
            Self::Pd => 2,
            Self::Pt => 3,
        }
    }
}

/// TLB flush scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlushScope {
    Single,    // Single page INVLPG
    Range,     // Range of pages
    Full,      // Full TLB flush (mov cr3)
    Global,    // Cross-CPU IPI shootdown
}

impl FlushScope {
    pub fn label(self) -> &'static str {
        match self {
            Self::Single => "single",
            Self::Range => "range",
            Self::Full => "full",
            Self::Global => "global",
        }
    }
}

/// Per-level stats.
#[derive(Debug, Clone)]
pub struct LevelStats {
    pub level: PtLevel,
    pub allocated: u64,
    pub freed: u64,
    pub active: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct State {
    level_allocs: [u64; 4],
    level_frees: [u64; 4],
    walks: u64,
    walk_levels_total: u64,   // Sum of levels walked (for average)
    flush_single: u64,
    flush_range: u64,
    flush_full: u64,
    flush_global: u64,
    total_pages_used: u64,
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

/// Initialise the page-table statistics state.
///
/// Starts with all per-level allocation/free counters, page-walk counters,
/// TLB-flush counters and the active-page total at zero. The four page-table
/// levels (PML4, PDPT, PD, PT) are a fixed dimension so [`per_level`] always
/// returns four rows, but with zeroed counters; they advance only through real
/// [`record_alloc`] / [`record_free`] / [`record_walk`] / [`record_tlb_flush`]
/// calls. The `/proc/pgtable` generator and the `pgtable` kshell command
/// surface this table (and [`per_level`] / [`flush_stats`] /
/// [`avg_walk_depth_x100`]) as if it reflects real page-table activity, so
/// seeding it with invented counts would be fabricated procfs data.
///
/// (Previously this seeded fabricated activity — per-level allocs of
/// [1, 512, 50,000, 2,000,000] and frees of [0, 10, 5,000, 500,000],
/// 100,000,000 page walks summing 350,000,000 levels, TLB flushes of
/// 50,000,000 single / 1,000,000 range / 500,000 full / 100,000 global, and
/// 1,550,503 active page-table pages.)
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        level_allocs: [0; 4],
        level_frees: [0; 4],
        walks: 0,
        walk_levels_total: 0,
        flush_single: 0,
        flush_range: 0,
        flush_full: 0,
        flush_global: 0,
        total_pages_used: 0,
        ops: 0,
    });
}

/// Record a page table page allocation.
pub fn record_alloc(level: PtLevel) -> KernelResult<()> {
    with_state(|state| {
        state.level_allocs[level.index()] += 1;
        state.total_pages_used += 1;
        Ok(())
    })
}

/// Record a page table page free.
pub fn record_free(level: PtLevel) -> KernelResult<()> {
    with_state(|state| {
        state.level_frees[level.index()] += 1;
        state.total_pages_used = state.total_pages_used.saturating_sub(1);
        Ok(())
    })
}

/// Record a page walk.
pub fn record_walk(levels_traversed: u32) -> KernelResult<()> {
    with_state(|state| {
        state.walks += 1;
        state.walk_levels_total += levels_traversed as u64;
        Ok(())
    })
}

/// Record a TLB flush.
pub fn record_tlb_flush(scope: FlushScope) -> KernelResult<()> {
    with_state(|state| {
        match scope {
            FlushScope::Single => state.flush_single += 1,
            FlushScope::Range => state.flush_range += 1,
            FlushScope::Full => state.flush_full += 1,
            FlushScope::Global => state.flush_global += 1,
        }
        Ok(())
    })
}

/// Per-level allocation stats.
pub fn per_level() -> Vec<LevelStats> {
    let guard = STATE.lock();
    guard.as_ref().map_or(Vec::new(), |s| {
        let levels = [PtLevel::Pml4, PtLevel::Pdpt, PtLevel::Pd, PtLevel::Pt];
        levels.iter().enumerate().map(|(i, &lvl)| LevelStats {
            level: lvl,
            allocated: s.level_allocs[i],
            freed: s.level_frees[i],
            active: s.level_allocs[i].saturating_sub(s.level_frees[i]),
        }).collect()
    })
}

/// TLB flush stats: (single, range, full, global).
pub fn flush_stats() -> (u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.flush_single, s.flush_range, s.flush_full, s.flush_global),
        None => (0, 0, 0, 0),
    }
}

/// Average walk depth (x100 for integer precision).
pub fn avg_walk_depth_x100() -> u64 {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) if s.walks > 0 => s.walk_levels_total * 100 / s.walks,
        _ => 0,
    }
}

/// Statistics: (total_pages, walks, total_flushes, avg_depth_x100, ops).
pub fn stats() -> (u64, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => {
            let total_flushes = s.flush_single + s.flush_range + s.flush_full + s.flush_global;
            let avg = if s.walks > 0 { s.walk_levels_total * 100 / s.walks } else { 0 };
            (s.total_pages_used, s.walks, total_flushes, avg, s.ops)
        }
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("pgtable::self_test() — running tests...");
    // Start from a clean, empty state so the assertions below are exact and
    // no fixtures leak into the live page-table stats afterwards.
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty defaults — four zeroed level rows, zero totals, zero avg depth.
    let levels = per_level();
    assert_eq!(levels.len(), 4);
    for l in &levels {
        assert_eq!((l.allocated, l.freed, l.active), (0, 0, 0));
    }
    let (p0, w0, f0, a0, _) = stats();
    assert_eq!((p0, w0, f0, a0), (0, 0, 0, 0));
    assert_eq!(flush_stats(), (0, 0, 0, 0));
    assert_eq!(avg_walk_depth_x100(), 0);
    crate::serial_println!("  [1/8] empty defaults: OK");

    // 2: Alloc — two PT allocs and one PD alloc advance per-level counters and
    //    the active-page total.
    record_alloc(PtLevel::Pt).expect("alloc_pt1");
    record_alloc(PtLevel::Pt).expect("alloc_pt2");
    record_alloc(PtLevel::Pd).expect("alloc_pd");
    assert_eq!(per_level()[PtLevel::Pt.index()].allocated, 2);
    assert_eq!(per_level()[PtLevel::Pd.index()].allocated, 1);
    assert_eq!(stats().0, 3); // total_pages_used
    crate::serial_println!("  [2/8] alloc: OK");

    // 3: Free — one PT free advances the free counter and drops the active total.
    record_free(PtLevel::Pt).expect("free");
    assert_eq!(per_level()[PtLevel::Pt.index()].freed, 1);
    assert_eq!(stats().0, 2); // total_pages_used (3 allocs - 1 free)
    crate::serial_println!("  [3/8] free: OK");

    // 4: Walk — three walks summing 11 levels (4 + 4 + 3).
    record_walk(4).expect("walk1");
    record_walk(4).expect("walk2");
    record_walk(3).expect("walk3");
    assert_eq!(stats().1, 3); // walks
    crate::serial_println!("  [4/8] walk: OK");

    // 5: TLB flush — one of each scope.
    record_tlb_flush(FlushScope::Single).expect("f_single");
    record_tlb_flush(FlushScope::Range).expect("f_range");
    record_tlb_flush(FlushScope::Full).expect("f_full");
    record_tlb_flush(FlushScope::Global).expect("f_global");
    assert_eq!(flush_stats(), (1, 1, 1, 1));
    crate::serial_println!("  [5/8] tlb flush: OK");

    // 6: Active pages — per level, active == allocated - freed (PT: 2-1=1,
    //    PD: 1-0=1, others 0).
    let levels = per_level();
    for l in &levels {
        assert!(l.allocated >= l.freed);
        assert_eq!(l.active, l.allocated - l.freed);
    }
    assert_eq!(per_level()[PtLevel::Pt.index()].active, 1);
    assert_eq!(per_level()[PtLevel::Pd.index()].active, 1);
    crate::serial_println!("  [6/8] active pages: OK");

    // 7: Average walk depth — 11 levels / 3 walks → 366 (×100, integer div).
    assert_eq!(avg_walk_depth_x100(), 366);
    crate::serial_println!("  [7/8] avg depth: OK");

    // 8: Final stats reflect only the real activity above: 2 active pages,
    //    3 walks, 4 flushes, avg depth 366.
    let (pages, walks, flushes, avg, ops) = stats();
    assert_eq!((pages, walks, flushes, avg), (2, 3, 4, 366));
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Leave no residue in the live state.
    *STATE.lock() = None;
    crate::serial_println!("pgtable::self_test() — all 8 tests passed");
}
