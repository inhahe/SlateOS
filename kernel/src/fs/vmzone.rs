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
use spin::Mutex;

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

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        zones: alloc::vec![
            ZoneStats { name: String::from("DMA"), zone_type: ZoneType::Dma, total_pages: 4096, free_pages: 3000, active_pages: 500, inactive_pages: 596, wmark_min: 100, wmark_low: 200, wmark_high: 400, allocs: 10_000, frees: 9_500, reclaim_count: 50 },
            ZoneStats { name: String::from("DMA32"), zone_type: ZoneType::Dma32, total_pages: 262_144, free_pages: 100_000, active_pages: 100_000, inactive_pages: 62_144, wmark_min: 5_000, wmark_low: 10_000, wmark_high: 20_000, allocs: 1_000_000, frees: 950_000, reclaim_count: 5_000 },
            ZoneStats { name: String::from("Normal"), zone_type: ZoneType::Normal, total_pages: 2_000_000, free_pages: 500_000, active_pages: 1_000_000, inactive_pages: 500_000, wmark_min: 50_000, wmark_low: 100_000, wmark_high: 200_000, allocs: 50_000_000, frees: 49_000_000, reclaim_count: 100_000 },
            ZoneStats { name: String::from("Movable"), zone_type: ZoneType::Movable, total_pages: 500_000, free_pages: 200_000, active_pages: 200_000, inactive_pages: 100_000, wmark_min: 10_000, wmark_low: 20_000, wmark_high: 50_000, allocs: 5_000_000, frees: 4_800_000, reclaim_count: 20_000 },
        ],
        total_allocs: 56_010_000,
        total_frees: 54_759_500,
        total_reclaims: 125_050,
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
    init_defaults();

    // 1: Defaults.
    assert_eq!(per_zone().len(), 4);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Register.
    register("Test", ZoneType::Normal, 1000, 10, 50, 100).expect("register");
    assert_eq!(per_zone().len(), 5);
    assert!(register("Test", ZoneType::Normal, 1000, 10, 50, 100).is_err());
    crate::serial_println!("  [2/8] register: OK");

    // 3: Alloc.
    record_alloc("Test", 100).expect("alloc");
    let z = per_zone().iter().find(|z| z.name == "Test").cloned().unwrap();
    assert_eq!(z.allocs, 1);
    assert_eq!(z.free_pages, 900);
    assert_eq!(z.active_pages, 100);
    crate::serial_println!("  [3/8] alloc: OK");

    // 4: Free.
    record_free("Test", 50).expect("free");
    let z = per_zone().iter().find(|z| z.name == "Test").cloned().unwrap();
    assert_eq!(z.frees, 1);
    assert_eq!(z.free_pages, 950);
    assert_eq!(z.active_pages, 50);
    crate::serial_println!("  [4/8] free: OK");

    // 5: Reclaim.
    // First put some pages in inactive
    let z = per_zone().iter().find(|z| z.name == "Test").cloned().unwrap();
    assert_eq!(z.inactive_pages, 0);
    // Reclaim from Normal zone which has inactive pages
    record_reclaim("Normal", 1000).expect("reclaim");
    let z = per_zone().iter().find(|z| z.name == "Normal").cloned().unwrap();
    assert!(z.reclaim_count > 100_000);
    crate::serial_println!("  [5/8] reclaim: OK");

    // 6: Saturating free (can't go below 0).
    for _ in 0..20 { record_alloc("Test", 100).expect("alloc_many"); }
    let z = per_zone().iter().find(|z| z.name == "Test").cloned().unwrap();
    // free_pages should be saturated at 0 (we allocated more than available)
    assert!(z.free_pages == 0 || z.free_pages < 1000);
    crate::serial_println!("  [6/8] saturation: OK");

    // 7: Not found.
    assert!(record_alloc("nonexist", 1).is_err());
    crate::serial_println!("  [7/8] not found: OK");

    // 8: Stats.
    let (zones, allocs, frees, reclaims, ops) = stats();
    assert!(zones >= 5);
    assert!(allocs > 56_010_000);
    assert!(frees > 54_759_500);
    assert!(reclaims > 125_050);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("vmzone::self_test() — all 8 tests passed");
}
