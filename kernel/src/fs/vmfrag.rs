//! VM Fragmentation Index — memory fragmentation assessment.
//!
//! Computes and tracks fragmentation indices per zone and order,
//! helping determine when compaction is needed. Indices range
//! from 0 (no fragmentation) to 1000 (completely fragmented).
//!
//! ## Architecture
//!
//! ```text
//! VM fragmentation monitoring
//!   → vmfrag::compute(zone, order) → compute frag index
//!   → vmfrag::record_compaction(zone, success) → compaction event
//!   → vmfrag::per_zone() → per-zone indices
//!
//! Integration:
//!   → buddyinfo (buddy allocator)
//!   → vmzone (VM zones)
//!   → compstat (compaction stats)
//!   → thpstat (THP stats)
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

/// Max order for fragmentation tracking.
const MAX_ORDER: usize = 11;

/// Per-zone fragmentation info.
#[derive(Debug, Clone)]
pub struct ZoneFragInfo {
    pub zone_name: String,
    pub frag_index: [u32; MAX_ORDER], // 0-1000 (x1000)
    pub compactions: u64,
    pub compact_success: u64,
    pub compact_fail: u64,
    pub last_index_update: u64, // Timestamp ns
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_ZONES: usize = 16;

struct State {
    zones: Vec<ZoneFragInfo>,
    total_compactions: u64,
    total_success: u64,
    total_fail: u64,
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

/// Initialise an **empty** fragmentation-tracking table.
///
/// Seeds NO zones and zero totals.  Zones are registered through
/// [`register_zone`] and their fragmentation indices / compaction counts are
/// tracked through [`update_index`] / [`record_compaction`] as the memory manager
/// computes them; until that wiring exists, `/proc/vmfrag` and the `vmfrag` kshell
/// command report an empty table rather than fabricated indices — the kernel's
/// hard "never invent data in procfs" rule.
///
/// (Previously this seeded two fabricated zones — `DMA32` (frag_index
/// [0,50,120,250,400,550,700,800,850,900,950], 5000 compactions / 4000 success /
/// 1000 fail) and `Normal` (frag_index [0,30,80,150,300,450,600,700,780,850,920],
/// 50000 / 40000 / 10000) — plus invented totals (55000 compactions, 44000
/// success, 11000 fail), which `/proc/vmfrag` and the `per_zone` view then
/// displayed as if they were real measured fragmentation.  Those zone names are
/// Linux's (`DMA32`/`Normal`); this kernel has a single global buddy allocator
/// (`crate::mm::frame`, free_lists per order) with no named-zone taxonomy and no
/// memory-compaction subsystem, so none of register_zone/update_index/
/// record_compaction has a real caller — the module is entirely unwired.  See the
/// DEFERRED PROPER FIX note in todo.txt for computing real indices from the buddy
/// allocator's per-order free counts.  The self-test now builds its own fixtures
/// via the real API — see [`self_test`].)
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        zones: Vec::new(),
        total_compactions: 0,
        total_success: 0,
        total_fail: 0,
        ops: 0,
    });
}

/// Register a zone.
pub fn register_zone(name: &str) -> KernelResult<()> {
    with_state(|state| {
        if state.zones.len() >= MAX_ZONES { return Err(KernelError::ResourceExhausted); }
        if state.zones.iter().any(|z| z.zone_name == name) { return Err(KernelError::AlreadyExists); }
        state.zones.push(ZoneFragInfo {
            zone_name: String::from(name),
            frag_index: [0; MAX_ORDER],
            compactions: 0, compact_success: 0, compact_fail: 0,
            last_index_update: 0,
        });
        Ok(())
    })
}

/// Update fragmentation index for a zone/order.
pub fn update_index(name: &str, order: usize, index: u32) -> KernelResult<()> {
    with_state(|state| {
        if order >= MAX_ORDER { return Err(KernelError::InvalidArgument); }
        if index > 1000 { return Err(KernelError::InvalidArgument); }
        let z = state.zones.iter_mut().find(|z| z.zone_name == name)
            .ok_or(KernelError::NotFound)?;
        z.frag_index[order] = index;
        z.last_index_update = crate::hpet::elapsed_ns();
        Ok(())
    })
}

/// Record a compaction attempt.
pub fn record_compaction(name: &str, success: bool) -> KernelResult<()> {
    with_state(|state| {
        let z = state.zones.iter_mut().find(|z| z.zone_name == name)
            .ok_or(KernelError::NotFound)?;
        z.compactions += 1;
        if success { z.compact_success += 1; state.total_success += 1; }
        else { z.compact_fail += 1; state.total_fail += 1; }
        state.total_compactions += 1;
        Ok(())
    })
}

/// Per-zone fragmentation info.
pub fn per_zone() -> Vec<ZoneFragInfo> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.zones.clone())
}

/// Statistics: (zone_count, total_compactions, total_success, total_fail, ops).
pub fn stats() -> (usize, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.zones.len(), s.total_compactions, s.total_success, s.total_fail, s.ops),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("vmfrag::self_test() — running tests...");
    // Start from a clean slate so the fixtures built below can never leak into
    // the live /proc/vmfrag table (this self-test now runs at boot).
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty defaults — no fabricated zones.
    assert_eq!(per_zone().len(), 0);
    assert_eq!(stats(), (0, 0, 0, 0, 0));
    crate::serial_println!("  [1/8] empty defaults: OK");

    // 2: Register a zone (duplicate registration is rejected).
    register_zone("TestZone").expect("register");
    assert_eq!(per_zone().len(), 1);
    assert!(register_zone("TestZone").is_err());
    crate::serial_println!("  [2/8] register: OK");

    // 3: Update index.
    update_index("TestZone", 0, 100).expect("update");
    let z = per_zone().into_iter().find(|z| z.zone_name == "TestZone").expect("find");
    assert_eq!(z.frag_index[0], 100);
    crate::serial_println!("  [3/8] update index: OK");

    // 4: Invalid index / order are rejected.
    assert!(update_index("TestZone", 0, 1001).is_err());
    assert!(update_index("TestZone", MAX_ORDER, 100).is_err());
    crate::serial_println!("  [4/8] invalid: OK");

    // 5: Compaction success.
    record_compaction("TestZone", true).expect("compact_ok");
    let z = per_zone().into_iter().find(|z| z.zone_name == "TestZone").expect("find2");
    assert_eq!(z.compact_success, 1);
    crate::serial_println!("  [5/8] compact success: OK");

    // 6: Compaction fail.
    record_compaction("TestZone", false).expect("compact_fail");
    let z = per_zone().into_iter().find(|z| z.zone_name == "TestZone").expect("find3");
    assert_eq!(z.compact_fail, 1);
    assert_eq!(z.compactions, 2);
    crate::serial_println!("  [6/8] compact fail: OK");

    // 7: Not found.
    assert!(update_index("nonexist", 0, 100).is_err());
    assert!(record_compaction("nonexist", true).is_err());
    crate::serial_println!("  [7/8] not found: OK");

    // 8: Stats — exact totals (1 zone, 2 compactions, 1 success, 1 fail).
    let (zones, compactions, success, fail, ops) = stats();
    assert_eq!(zones, 1);
    assert_eq!(compactions, 2);
    assert_eq!(success, 1);
    assert_eq!(fail, 1);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Reset so the boot self-test leaves no fixtures behind in /proc/vmfrag.
    *STATE.lock() = None;

    crate::serial_println!("vmfrag::self_test() — all 8 tests passed");
}
