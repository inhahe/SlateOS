//! Cgroup Memory Statistics — per-cgroup memory accounting.
//!
//! Tracks per-cgroup memory usage, limits, RSS, cache, swap,
//! and OOM events. Essential for container memory isolation
//! and resource management.
//!
//! ## Architecture
//!
//! ```text
//! Cgroup memory monitoring
//!   → cgmem::create(name, limit) → create cgroup
//!   → cgmem::record_charge(cg, pages) → charge pages
//!   → cgmem::record_uncharge(cg, pages) → uncharge pages
//!   → cgmem::per_cgroup() → per-cgroup stats
//!
//! Integration:
//!   → memcg (memory cgroup)
//!   → cgiostat (cgroup I/O)
//!   → oomkiller (OOM killer)
//!   → mempress (memory pressure)
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

/// Per-cgroup memory stats.
#[derive(Debug, Clone)]
pub struct CgroupMemStats {
    pub cg_id: u32,
    pub name: String,
    pub limit_pages: u64,
    pub usage_pages: u64,
    pub rss_pages: u64,
    pub cache_pages: u64,
    pub swap_pages: u64,
    pub charges: u64,
    pub uncharges: u64,
    pub oom_kills: u64,
    pub high_events: u64, // Times usage exceeded high watermark
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_CGROUPS: usize = 256;

struct State {
    cgroups: Vec<CgroupMemStats>,
    next_id: u32,
    total_charges: u64,
    total_uncharges: u64,
    total_oom_kills: u64,
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

/// Initialise the per-cgroup memory-statistics state.
///
/// Starts with no cgroups and all charge/uncharge/OOM totals at zero. The
/// `/proc/cgmem` generator and the `cgmem` kshell command surface this
/// table as if it reflects real per-cgroup page accounting, so seeding it
/// with invented cgroups and charge counts would be fabricated procfs
/// data. Cgroups are created at runtime through [`create`] and pages are
/// accounted only through real [`record_charge`] / [`record_uncharge`]
/// calls.
///
/// (Previously this seeded three fictional cgroups — "root" 500k usage
/// pages / 10M charges; "system" 1M limit / 400k usage / 5M charges / 2
/// OOM; "user" 2M limit / 800k usage / 20M charges / 5 OOM — plus invented
/// totals (35M charges, 33.3M uncharges, 7 OOM kills).)
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        cgroups: Vec::new(),
        next_id: 1,
        total_charges: 0,
        total_uncharges: 0,
        total_oom_kills: 0,
        ops: 0,
    });
}

/// Create a cgroup.
pub fn create(name: &str, limit_pages: u64) -> KernelResult<u32> {
    with_state(|state| {
        if state.cgroups.len() >= MAX_CGROUPS { return Err(KernelError::ResourceExhausted); }
        let id = state.next_id;
        state.next_id += 1;
        state.cgroups.push(CgroupMemStats {
            cg_id: id, name: String::from(name), limit_pages,
            usage_pages: 0, rss_pages: 0, cache_pages: 0, swap_pages: 0,
            charges: 0, uncharges: 0, oom_kills: 0, high_events: 0,
        });
        Ok(id)
    })
}

/// Remove a cgroup.
pub fn remove(cg_id: u32) -> KernelResult<()> {
    with_state(|state| {
        let idx = state.cgroups.iter().position(|c| c.cg_id == cg_id)
            .ok_or(KernelError::NotFound)?;
        state.cgroups.remove(idx);
        Ok(())
    })
}

/// Charge pages to a cgroup.
pub fn record_charge(cg_id: u32, pages: u64, is_cache: bool) -> KernelResult<()> {
    with_state(|state| {
        let c = state.cgroups.iter_mut().find(|c| c.cg_id == cg_id)
            .ok_or(KernelError::NotFound)?;
        c.usage_pages += pages;
        if is_cache { c.cache_pages += pages; } else { c.rss_pages += pages; }
        c.charges += 1;
        if c.usage_pages > c.limit_pages {
            c.high_events += 1;
        }
        state.total_charges += 1;
        Ok(())
    })
}

/// Uncharge pages from a cgroup.
pub fn record_uncharge(cg_id: u32, pages: u64, is_cache: bool) -> KernelResult<()> {
    with_state(|state| {
        let c = state.cgroups.iter_mut().find(|c| c.cg_id == cg_id)
            .ok_or(KernelError::NotFound)?;
        c.usage_pages = c.usage_pages.saturating_sub(pages);
        if is_cache { c.cache_pages = c.cache_pages.saturating_sub(pages); }
        else { c.rss_pages = c.rss_pages.saturating_sub(pages); }
        c.uncharges += 1;
        state.total_uncharges += 1;
        Ok(())
    })
}

/// Record an OOM kill in a cgroup.
pub fn record_oom(cg_id: u32) -> KernelResult<()> {
    with_state(|state| {
        let c = state.cgroups.iter_mut().find(|c| c.cg_id == cg_id)
            .ok_or(KernelError::NotFound)?;
        c.oom_kills += 1;
        state.total_oom_kills += 1;
        Ok(())
    })
}

/// Per-cgroup stats.
pub fn per_cgroup() -> Vec<CgroupMemStats> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.cgroups.clone())
}

/// Statistics: (cgroup_count, total_charges, total_uncharges, total_oom_kills, ops).
pub fn stats() -> (usize, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.cgroups.len(), s.total_charges, s.total_uncharges, s.total_oom_kills, s.ops),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("cgmem::self_test() — running tests...");
    // Start from a clean, empty state so the assertions below are exact and
    // no fixtures leak into the live cgroup table afterwards.
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty defaults — no fabricated cgroups, all totals zero.
    assert_eq!(per_cgroup().len(), 0);
    let (cg0, charges0, uncharges0, ooms0, _) = stats();
    assert_eq!((cg0, charges0, uncharges0, ooms0), (0, 0, 0, 0));
    crate::serial_println!("  [1/8] empty defaults: OK");

    // 2: Create — ids monotonic starting at 1; unknown-id charge errors.
    let id = create("test_cg", 10_000).expect("create");
    assert_eq!(id, 1);
    assert_eq!(per_cgroup().len(), 1);
    assert!(record_charge(99, 1, false).is_err());
    crate::serial_println!("  [2/8] create: OK");

    // 3: Charge RSS — usage and rss tracked, cache untouched.
    record_charge(id, 100, false).expect("charge_rss");
    let c = per_cgroup().into_iter().find(|c| c.cg_id == id).expect("cg");
    assert_eq!(c.usage_pages, 100);
    assert_eq!(c.rss_pages, 100);
    assert_eq!(c.cache_pages, 0);
    crate::serial_println!("  [3/8] charge rss: OK");

    // 4: Charge cache — separate bucket, usage is the sum.
    record_charge(id, 50, true).expect("charge_cache");
    let c = per_cgroup().into_iter().find(|c| c.cg_id == id).expect("cg");
    assert_eq!(c.cache_pages, 50);
    assert_eq!(c.usage_pages, 150);
    crate::serial_println!("  [4/8] charge cache: OK");

    // 5: Uncharge RSS — usage and rss drop, cache unaffected.
    record_uncharge(id, 30, false).expect("uncharge");
    let c = per_cgroup().into_iter().find(|c| c.cg_id == id).expect("cg");
    assert_eq!(c.rss_pages, 70);
    assert_eq!(c.cache_pages, 50);
    assert_eq!(c.usage_pages, 120);
    crate::serial_println!("  [5/8] uncharge: OK");

    // 6: High-event detection when usage exceeds the page limit.
    let id2 = create("small_cg", 10).expect("create2");
    record_charge(id2, 25, false).expect("charge over limit");
    let c = per_cgroup().into_iter().find(|c| c.cg_id == id2).expect("cg2");
    assert_eq!(c.high_events, 1);
    record_oom(id2).expect("oom");
    assert_eq!(per_cgroup().into_iter().find(|c| c.cg_id == id2).expect("cg2").oom_kills, 1);
    crate::serial_println!("  [6/8] high event + oom: OK");

    // 7: Remove — gone, and double-remove errors.
    remove(id).expect("remove");
    remove(id2).expect("remove2");
    assert_eq!(per_cgroup().len(), 0);
    assert!(remove(id).is_err());
    crate::serial_println!("  [7/8] remove: OK");

    // 8: Final stats reflect only the real activity above (3 charges, 1
    //    uncharge, 1 OOM; both cgroups removed).
    let (cgroups, charges, uncharges, ooms, ops) = stats();
    assert_eq!(cgroups, 0);
    assert_eq!(charges, 3);
    assert_eq!(uncharges, 1);
    assert_eq!(ooms, 1);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Leave no residue in the live state.
    *STATE.lock() = None;
    crate::serial_println!("cgmem::self_test() — all 8 tests passed");
}
