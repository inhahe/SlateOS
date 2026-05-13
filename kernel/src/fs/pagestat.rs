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

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

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

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    let zones = alloc::vec![
        ZoneStats { zone: Zone::Dma, total_pages: 4096, free_pages: 1024, allocated: 100000, freed: 97000, reclaimed: 500, failed: 0, hugepages_total: 0, hugepages_free: 0, hugepages_reserved: 0, fragmentation_pct: 5 },
        ZoneStats { zone: Zone::Dma32, total_pages: 65536, free_pages: 16384, allocated: 500000, freed: 480000, reclaimed: 5000, failed: 10, hugepages_total: 32, hugepages_free: 8, hugepages_reserved: 4, fragmentation_pct: 12 },
        ZoneStats { zone: Zone::Normal, total_pages: 1048576, free_pages: 262144, allocated: 10_000_000, freed: 9_500_000, reclaimed: 200000, failed: 50, hugepages_total: 512, hugepages_free: 128, hugepages_reserved: 64, fragmentation_pct: 18 },
        ZoneStats { zone: Zone::Movable, total_pages: 524288, free_pages: 131072, allocated: 2_000_000, freed: 1_900_000, reclaimed: 50000, failed: 5, hugepages_total: 256, hugepages_free: 64, hugepages_reserved: 32, fragmentation_pct: 8 },
    ];
    let mut order_stats = Vec::new();
    for o in 0..MAX_ORDER as u32 {
        let base = 1_000_000u64 >> o;
        order_stats.push(OrderStats {
            order: o, allocs: base, frees: base * 95 / 100, fails: if o > 8 { 10 } else { 0 },
        });
    }
    *guard = Some(State {
        zones,
        order_stats,
        total_allocs: 12_600_000,
        total_frees: 11_977_000,
        total_reclaims: 255500,
        total_fails: 65,
        ops: 0,
    });
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
    init_defaults();

    // 1: Defaults.
    assert_eq!(zone_stats().len(), 4);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Record alloc.
    let before = zone_stats().iter().find(|z| z.zone == Zone::Normal).unwrap().allocated;
    record_alloc(Zone::Normal, 0).expect("alloc");
    let after = zone_stats().iter().find(|z| z.zone == Zone::Normal).unwrap().allocated;
    assert_eq!(after, before + 1);
    crate::serial_println!("  [2/8] alloc: OK");

    // 3: Record free.
    let before = zone_stats().iter().find(|z| z.zone == Zone::Normal).unwrap().freed;
    record_free(Zone::Normal, 0).expect("free");
    let after = zone_stats().iter().find(|z| z.zone == Zone::Normal).unwrap().freed;
    assert_eq!(after, before + 1);
    crate::serial_println!("  [3/8] free: OK");

    // 4: Record reclaim.
    let before = zone_stats().iter().find(|z| z.zone == Zone::Dma).unwrap().reclaimed;
    record_reclaim(Zone::Dma, 100).expect("reclaim");
    let after = zone_stats().iter().find(|z| z.zone == Zone::Dma).unwrap().reclaimed;
    assert_eq!(after, before + 100);
    crate::serial_println!("  [4/8] reclaim: OK");

    // 5: Order histogram.
    let hist = order_histogram();
    assert_eq!(hist.len(), 12);
    assert!(hist[0].allocs > hist[5].allocs); // Lower orders have more allocs.
    crate::serial_println!("  [5/8] order histogram: OK");

    // 6: Huge page info.
    let (total, free, reserved) = hugepage_info();
    assert!(total > 0);
    assert!(free <= total);
    assert!(reserved <= total);
    crate::serial_println!("  [6/8] hugepages: OK");

    // 7: Not found zone.
    assert!(record_alloc(Zone::HighMem, 0).is_err());
    crate::serial_println!("  [7/8] not found: OK");

    // 8: Stats.
    let (zones, allocs, frees, reclaims, fails, ops) = stats();
    assert_eq!(zones, 4);
    assert!(allocs > 12_600_000);
    assert!(frees > 11_977_000);
    assert!(reclaims > 255500);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("pagestat::self_test() — all 8 tests passed");
}
