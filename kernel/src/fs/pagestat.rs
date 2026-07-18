//! Page Statistics — page allocator performance monitoring.
//!
//! Tracks page allocations, frees, reclaims, huge page usage,
//! and memory fragmentation per zone. Essential for tuning
//! the physical memory allocator and huge page configuration.
//!
//! ## Architecture
//!
//! ```text
//! Page statistics
//!   → pagestat::record_alloc(zone, order) → track allocation
//!   → pagestat::record_free(zone, order) → track free
//!   → pagestat::record_reclaim(zone, pages) → track reclaim
//!   → pagestat::zone_stats() → per-zone statistics
//!
//! Integration:
//!   → slabstat (slab allocator stats)
//!   → memcg (memory cgroup)
//!   → numastat (NUMA statistics)
//!   → pftrack (page fault tracking)
//! ```

#![allow(dead_code)]

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Memory zone type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Zone {
    Dma,
    Dma32,
    Normal,
    HighMem,
    Movable,
}

impl Zone {
    pub fn label(self) -> &'static str {
        match self {
            Self::Dma => "DMA",
            Self::Dma32 => "DMA32",
            Self::Normal => "Normal",
            Self::HighMem => "HighMem",
            Self::Movable => "Movable",
        }
    }
}

/// Per-zone page statistics.
#[derive(Debug, Clone)]
pub struct ZoneStats {
    pub zone: Zone,
    pub total_pages: u64,
    pub free_pages: u64,
    pub allocated: u64,
    pub freed: u64,
    pub reclaimed: u64,
    pub failed: u64,
    pub hugepages_total: u64,
    pub hugepages_free: u64,
    pub hugepages_reserved: u64,
    pub fragmentation_pct: u32, // 0-100.
}

/// Allocation order histogram entry.
#[derive(Debug, Clone)]
pub struct OrderStats {
    pub order: u32,
    pub allocs: u64,
    pub frees: u64,
    pub fails: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_ORDER: usize = 12; // Orders 0..11.

struct State {
    zones: Vec<ZoneStats>,
    order_stats: Vec<OrderStats>,
    total_allocs: u64,
    total_frees: u64,
    total_reclaims: u64,
    total_fails: u64,
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

/// Initialise an **empty** page-allocator statistics table.
///
/// Seeds NO memory zones and zero global counters.  The per-order histogram is
/// initialised to its real fixed structure — one bucket per allocation order
/// `0..MAX_ORDER` with all counters zeroed (the buddy allocator always has
/// these order buckets; the *counts* are what must start at zero).  Real
/// per-zone accounting is wired through [`register_zone`] (one row per memory
/// zone the physical allocator brings online, with its true `total_pages`) and
/// the `record_alloc`/`record_free`/`record_reclaim` functions; [`set_hugepages`]
/// declares a zone's huge-page pool.  Until those are called the zone table is
/// genuinely empty, so `/proc/pagestat` and the `pagestat` kshell command report
/// zeros rather than fabricated numbers — the kernel's hard "never invent data
/// in procfs" rule.
///
/// NOTE: this previously seeded four fictional zones (DMA: total 4096 / free
/// 1024 / allocated 100k; DMA32: total 64k / allocated 500k / hugepages 32;
/// Normal: total 1M / allocated 10M / hugepages 512; Movable: total 512k /
/// allocated 2M / hugepages 256) plus a fabricated order histogram (allocs
/// 1_000_000 >> order, frees 95% of that) and invented aggregate totals
/// (total_allocs 12.6M, total_frees 11_977_000, total_reclaims 255500,
/// total_fails 65), which `/proc/pagestat` then displayed as if they were real
/// measured allocator activity.  That demo data was removed; the self-test now
/// builds its own fixtures explicitly via the real API (see [`self_test`]).
/// The physical page allocator is expected to call [`register_zone`] as it
/// brings each zone online and the record functions on every page alloc/free.
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    // The order histogram has one bucket per buddy-allocator order; this is real
    // structure, so create all MAX_ORDER buckets but with zeroed counters.
    let mut order_stats = Vec::new();
    for o in 0..MAX_ORDER as u32 {
        order_stats.push(OrderStats { order: o, allocs: 0, frees: 0, fails: 0 });
    }
    *guard = Some(State {
        zones: Vec::new(),
        order_stats,
        total_allocs: 0,
        total_frees: 0,
        total_reclaims: 0,
        total_fails: 0,
        ops: 0,
    });
}

/// Register a memory zone the physical allocator has brought online.
///
/// `total_pages` is the zone's real page count; the zone starts fully free
/// (`free_pages == total_pages`) with all activity counters zeroed.  Returns
/// [`KernelError::AlreadyExists`] if the zone is already registered and
/// [`KernelError::ResourceExhausted`] once five zones (one per [`Zone`] variant)
/// exist.
pub fn register_zone(zone: Zone, total_pages: u64) -> KernelResult<()> {
    with_state(|state| {
        if state.zones.iter().any(|z| z.zone == zone) {
            return Err(KernelError::AlreadyExists);
        }
        if state.zones.len() >= 5 {
            return Err(KernelError::ResourceExhausted);
        }
        state.zones.push(ZoneStats {
            zone,
            total_pages,
            free_pages: total_pages,
            allocated: 0,
            freed: 0,
            reclaimed: 0,
            failed: 0,
            hugepages_total: 0,
            hugepages_free: 0,
            hugepages_reserved: 0,
            fragmentation_pct: 0,
        });
        Ok(())
    })
}

/// Declare a zone's huge-page pool: total, free, and reserved counts.
pub fn set_hugepages(zone: Zone, total: u64, free: u64, reserved: u64) -> KernelResult<()> {
    with_state(|state| {
        let zs = state.zones.iter_mut().find(|z| z.zone == zone)
            .ok_or(KernelError::NotFound)?;
        zs.hugepages_total = total;
        zs.hugepages_free = free;
        zs.hugepages_reserved = reserved;
        Ok(())
    })
}

/// Record a page allocation.
pub fn record_alloc(zone: Zone, order: u32) -> KernelResult<()> {
    with_state(|state| {
        let zs = state.zones.iter_mut().find(|z| z.zone == zone)
            .ok_or(KernelError::NotFound)?;
        let pages = 1u64 << order.min(11);
        if zs.free_pages < pages {
            zs.failed += 1;
            state.total_fails += 1;
            if let Some(os) = state.order_stats.get_mut(order.min(11) as usize) { os.fails += 1; }
            return Err(KernelError::OutOfMemory);
        }
        zs.allocated += 1;
        zs.free_pages -= pages;
        state.total_allocs += 1;
        if let Some(os) = state.order_stats.get_mut(order.min(11) as usize) { os.allocs += 1; }
        Ok(())
    })
}

/// Record a page free.
pub fn record_free(zone: Zone, order: u32) -> KernelResult<()> {
    with_state(|state| {
        let zs = state.zones.iter_mut().find(|z| z.zone == zone)
            .ok_or(KernelError::NotFound)?;
        let pages = 1u64 << order.min(11);
        zs.freed += 1;
        zs.free_pages += pages;
        state.total_frees += 1;
        if let Some(os) = state.order_stats.get_mut(order.min(11) as usize) { os.frees += 1; }
        Ok(())
    })
}

/// Record page reclamation.
pub fn record_reclaim(zone: Zone, pages: u64) -> KernelResult<()> {
    with_state(|state| {
        let zs = state.zones.iter_mut().find(|z| z.zone == zone)
            .ok_or(KernelError::NotFound)?;
        zs.reclaimed += pages;
        zs.free_pages += pages;
        state.total_reclaims += pages;
        Ok(())
    })
}

/// Get per-zone statistics.
pub fn zone_stats() -> Vec<ZoneStats> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.zones.clone())
}

/// Get order histogram.
pub fn order_histogram() -> Vec<OrderStats> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.order_stats.clone())
}

/// Get total huge page info: (total, free, reserved).
pub fn hugepage_info() -> (u64, u64, u64) {
    STATE.lock().as_ref().map_or((0, 0, 0), |s| {
        let total: u64 = s.zones.iter().map(|z| z.hugepages_total).sum();
        let free: u64 = s.zones.iter().map(|z| z.hugepages_free).sum();
        let reserved: u64 = s.zones.iter().map(|z| z.hugepages_reserved).sum();
        (total, free, reserved)
    })
}

/// Statistics: (zone_count, total_allocs, total_frees, total_reclaims, total_fails, ops).
pub fn stats() -> (usize, u64, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.zones.len(), s.total_allocs, s.total_frees, s.total_reclaims, s.total_fails, s.ops),
        None => (0, 0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("pagestat::self_test() — running tests...");
    // Begin from a clean, EMPTY table and build every fixture via the real API,
    // so the test exercises genuine accounting paths and never relies on
    // fabricated seed data (which /proc/pagestat must never surface).
    // Resetting first clears any residue from a prior `pagestat test` run so the
    // totals asserted below are exact.
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty after init — no fabricated zones or totals; histogram is the real
    //    fixed 12-bucket structure but every count starts at zero.
    assert_eq!(zone_stats().len(), 0);
    let (z0, a0, f0, r0, fa0, _o0) = stats();
    assert_eq!((z0, a0, f0, r0, fa0), (0, 0, 0, 0, 0));
    let hist0 = order_histogram();
    assert_eq!(hist0.len(), 12);
    assert!(hist0.iter().all(|o| o.allocs == 0 && o.frees == 0 && o.fails == 0));
    assert_eq!(hugepage_info(), (0, 0, 0));
    crate::serial_println!("  [1/8] empty init: OK");

    // 2: Register zones — fully free, zeroed counters; dup fails.
    register_zone(Zone::Normal, 1000).expect("reg normal");
    register_zone(Zone::Dma, 256).expect("reg dma");
    assert!(register_zone(Zone::Normal, 1).is_err()); // AlreadyExists
    assert_eq!(zone_stats().len(), 2);
    let normal = zone_stats().iter().find(|z| z.zone == Zone::Normal).cloned().expect("normal");
    assert_eq!((normal.total_pages, normal.free_pages, normal.allocated), (1000, 1000, 0));
    crate::serial_println!("  [2/8] register: OK");

    // 3: Alloc — allocated up, free_pages down by 2^order, histogram bucket up.
    record_alloc(Zone::Normal, 0).expect("alloc o0"); // 1 page
    record_alloc(Zone::Normal, 2).expect("alloc o2"); // 4 pages
    let normal = zone_stats().iter().find(|z| z.zone == Zone::Normal).cloned().expect("normal");
    assert_eq!(normal.allocated, 2);
    assert_eq!(normal.free_pages, 1000 - 1 - 4);
    assert_eq!(order_histogram()[0].allocs, 1);
    assert_eq!(order_histogram()[2].allocs, 1);
    crate::serial_println!("  [3/8] alloc: OK");

    // 4: Free — freed up, free_pages restored, histogram free bucket up.
    record_free(Zone::Normal, 0).expect("free o0");
    let normal = zone_stats().iter().find(|z| z.zone == Zone::Normal).cloned().expect("normal");
    assert_eq!(normal.freed, 1);
    assert_eq!(normal.free_pages, 1000 - 4); // freed the 1-page alloc back
    assert_eq!(order_histogram()[0].frees, 1);
    crate::serial_println!("  [4/8] free: OK");

    // 5: Reclaim adds pages back to the zone exactly.
    record_reclaim(Zone::Dma, 100).expect("reclaim");
    let dma = zone_stats().iter().find(|z| z.zone == Zone::Dma).cloned().expect("dma");
    assert_eq!(dma.reclaimed, 100);
    assert_eq!(dma.free_pages, 256 + 100);
    crate::serial_println!("  [5/8] reclaim: OK");

    // 6: Alloc failure when the zone lacks free pages — fail counters bump, no
    //    OOM phantom allocation.
    register_zone(Zone::Dma32, 1).expect("reg dma32");
    assert!(record_alloc(Zone::Dma32, 5).is_err()); // needs 32 pages, only 1 free
    let dma32 = zone_stats().iter().find(|z| z.zone == Zone::Dma32).cloned().expect("dma32");
    assert_eq!(dma32.failed, 1);
    assert_eq!(dma32.allocated, 0);
    crate::serial_println!("  [6/8] alloc fail: OK");

    // 7: Hugepages set + reported; unregistered zone → NotFound.
    set_hugepages(Zone::Normal, 8, 6, 2).expect("hugepages");
    assert_eq!(hugepage_info(), (8, 6, 2));
    assert!(record_alloc(Zone::HighMem, 0).is_err());
    assert!(set_hugepages(Zone::HighMem, 1, 1, 0).is_err());
    crate::serial_println!("  [7/8] hugepages + not found: OK");

    // 8: Aggregate totals equal the exact sums of the operations above.
    let (zones, allocs, frees, reclaims, fails, ops) = stats();
    assert_eq!(zones, 3);
    assert_eq!(allocs, 2);     // 2 successful allocs
    assert_eq!(frees, 1);      // 1 free
    assert_eq!(reclaims, 100); // 100 pages reclaimed
    assert_eq!(fails, 1);      // 1 failed alloc
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Leave NO residue: a diagnostic self-test must not populate the live
    // /proc/pagestat table with its fixtures.  Reset to the uninitialised state
    // so production reads report an empty table until the page allocator wires
    // real accounting.
    *STATE.lock() = None;

    crate::serial_println!("pagestat::self_test() — all 8 tests passed");
}
