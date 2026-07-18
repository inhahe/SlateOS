//! VM Zone Statistics — virtual memory zone monitoring.
//!
//! Tracks per-zone memory usage, watermarks, allocation pressure,
//! and reclaim activity. Essential for NUMA-aware memory management
//! and understanding where memory pressure occurs.
//!
//! ## Architecture
//!
//! ```text
//! VM zone monitoring
//!   → vmzone::register(name, type) → register zone
//!   → vmzone::update_pages(name, free, active, inactive) → update
//!   → vmzone::record_alloc(name, pages) → allocation from zone
//!   → vmzone::per_zone() → per-zone stats
//!
//! Integration:
//!   → pagestat (page allocator)
//!   → numastat (NUMA stats)
//!   → mempress (memory pressure)
//!   → oomkiller (OOM killer)
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

/// Zone type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZoneType {
    Dma,       // DMA-capable memory (< 16MB)
    Dma32,     // 32-bit DMA (< 4GB)
    Normal,    // Regular memory
    HighMem,   // High memory (if applicable)
    Movable,   // Movable pages for CMA
}

impl ZoneType {
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

/// Per-zone stats.
#[derive(Debug, Clone)]
pub struct ZoneStats {
    pub name: String,
    pub zone_type: ZoneType,
    pub total_pages: u64,
    pub free_pages: u64,
    pub active_pages: u64,
    pub inactive_pages: u64,
    pub wmark_min: u64,
    pub wmark_low: u64,
    pub wmark_high: u64,
    pub allocs: u64,
    pub frees: u64,
    pub reclaim_count: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_ZONES: usize = 32;

struct State {
    zones: Vec<ZoneStats>,
    total_allocs: u64,
    total_frees: u64,
    total_reclaims: u64,
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

/// Initialise the VM-zone statistics state.
///
/// Starts with no zones and all alloc/free/reclaim totals at zero. The
/// `/proc/vmzone` generator and the `vmzone` kshell command surface this
/// table as if it reflects the real page allocator's per-zone state, so
/// seeding it with invented zones, page counts, and activity would be
/// fabricated procfs data. The page allocator registers its real zones
/// through [`register`] (with their actual page totals and watermarks) and
/// publishes activity only through real [`record_alloc`] / [`record_free`]
/// / [`record_reclaim`] calls.
///
/// (Previously this seeded four fictional zones — DMA 4096 pages / 10k
/// allocs; DMA32 262k pages / 1M allocs; Normal 2M pages / 50M allocs /
/// 100k reclaims; Movable 500k pages / 5M allocs — plus invented totals
/// (56.01M allocs, 54.76M frees, 125.05k reclaims).)
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        zones: Vec::new(),
        total_allocs: 0,
        total_frees: 0,
        total_reclaims: 0,
        ops: 0,
    });
}

/// Register a zone.
pub fn register(name: &str, zone_type: ZoneType, total_pages: u64, wmark_min: u64, wmark_low: u64, wmark_high: u64) -> KernelResult<()> {
    with_state(|state| {
        if state.zones.len() >= MAX_ZONES { return Err(KernelError::ResourceExhausted); }
        if state.zones.iter().any(|z| z.name == name) { return Err(KernelError::AlreadyExists); }
        state.zones.push(ZoneStats {
            name: String::from(name), zone_type, total_pages,
            free_pages: total_pages, active_pages: 0, inactive_pages: 0,
            wmark_min, wmark_low, wmark_high,
            allocs: 0, frees: 0, reclaim_count: 0,
        });
        Ok(())
    })
}

/// Record an allocation.
pub fn record_alloc(name: &str, pages: u64) -> KernelResult<()> {
    with_state(|state| {
        let z = state.zones.iter_mut().find(|z| z.name == name)
            .ok_or(KernelError::NotFound)?;
        z.allocs += 1;
        z.free_pages = z.free_pages.saturating_sub(pages);
        z.active_pages += pages;
        state.total_allocs += 1;
        Ok(())
    })
}

/// Record a free.
pub fn record_free(name: &str, pages: u64) -> KernelResult<()> {
    with_state(|state| {
        let z = state.zones.iter_mut().find(|z| z.name == name)
            .ok_or(KernelError::NotFound)?;
        z.frees += 1;
        z.free_pages += pages;
        z.active_pages = z.active_pages.saturating_sub(pages);
        state.total_frees += 1;
        Ok(())
    })
}

/// Record a reclaim event.
pub fn record_reclaim(name: &str, pages: u64) -> KernelResult<()> {
    with_state(|state| {
        let z = state.zones.iter_mut().find(|z| z.name == name)
            .ok_or(KernelError::NotFound)?;
        z.reclaim_count += 1;
        z.inactive_pages = z.inactive_pages.saturating_sub(pages);
        z.free_pages += pages;
        state.total_reclaims += 1;
        Ok(())
    })
}

/// Per-zone stats.
pub fn per_zone() -> Vec<ZoneStats> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.zones.clone())
}

/// Statistics: (zone_count, total_allocs, total_frees, total_reclaims, ops).
pub fn stats() -> (usize, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.zones.len(), s.total_allocs, s.total_frees, s.total_reclaims, s.ops),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("vmzone::self_test() — running tests...");
    // Start from a clean, empty state so the assertions below are exact and
    // no fixtures leak into the live zone table afterwards.
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty defaults — no fabricated zones, all totals zero.
    assert_eq!(per_zone().len(), 0);
    let (zones0, allocs0, frees0, reclaims0, _) = stats();
    assert_eq!((zones0, allocs0, frees0, reclaims0), (0, 0, 0, 0));
    crate::serial_println!("  [1/8] empty defaults: OK");

    // 2: Register — new zone starts fully free; duplicate name errors.
    register("Test", ZoneType::Normal, 1000, 10, 50, 100).expect("register");
    assert_eq!(per_zone().len(), 1);
    let z = per_zone().into_iter().find(|z| z.name == "Test").expect("z");
    assert_eq!(z.free_pages, 1000);
    assert_eq!(z.active_pages, 0);
    assert!(register("Test", ZoneType::Normal, 1000, 10, 50, 100).is_err());
    crate::serial_println!("  [2/8] register: OK");

    // 3: Alloc moves pages free → active.
    record_alloc("Test", 100).expect("alloc");
    let z = per_zone().into_iter().find(|z| z.name == "Test").expect("z");
    assert_eq!(z.allocs, 1);
    assert_eq!(z.free_pages, 900);
    assert_eq!(z.active_pages, 100);
    crate::serial_println!("  [3/8] alloc: OK");

    // 4: Free moves pages active → free.
    record_free("Test", 50).expect("free");
    let z = per_zone().into_iter().find(|z| z.name == "Test").expect("z");
    assert_eq!(z.frees, 1);
    assert_eq!(z.free_pages, 950);
    assert_eq!(z.active_pages, 50);
    crate::serial_println!("  [4/8] free: OK");

    // 5: Reclaim moves inactive → free and counts the event. Seed inactive
    //    pages via a second zone we control.
    register("Reclaimable", ZoneType::Movable, 2000, 10, 50, 100).expect("register2");
    // Move some pages into inactive by allocating then aging is out of scope
    // here; record_reclaim simply saturates inactive at 0 and adds to free.
    record_reclaim("Reclaimable", 100).expect("reclaim");
    let z = per_zone().into_iter().find(|z| z.name == "Reclaimable").expect("z");
    assert_eq!(z.reclaim_count, 1);
    assert_eq!(z.inactive_pages, 0); // saturated, was 0
    assert_eq!(z.free_pages, 2100); // 2000 initial + 100 reclaimed
    crate::serial_println!("  [5/8] reclaim: OK");

    // 6: Allocs saturate free_pages at 0 (cannot go negative).
    for _ in 0..20 { record_alloc("Test", 100).expect("alloc_many"); }
    let z = per_zone().into_iter().find(|z| z.name == "Test").expect("z");
    assert_eq!(z.free_pages, 0);
    crate::serial_println!("  [6/8] saturation: OK");

    // 7: Operations on an unknown zone error.
    assert!(record_alloc("nonexist", 1).is_err());
    assert!(record_free("nonexist", 1).is_err());
    assert!(record_reclaim("nonexist", 1).is_err());
    crate::serial_println!("  [7/8] not found: OK");

    // 8: Final stats reflect only the real activity above (21 allocs total:
    //    1 in test 3 + 20 in test 6; 1 free; 1 reclaim; 2 zones).
    let (zones, allocs, frees, reclaims, ops) = stats();
    assert_eq!(zones, 2);
    assert_eq!(allocs, 21);
    assert_eq!(frees, 1);
    assert_eq!(reclaims, 1);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Leave no residue in the live state.
    *STATE.lock() = None;
    crate::serial_println!("vmzone::self_test() — all 8 tests passed");
}
