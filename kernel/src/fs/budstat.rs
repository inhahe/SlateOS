//! Buddy Allocator Info — page allocator fragmentation monitoring.
//!
//! Tracks free pages at each buddy order (0..MAX_ORDER), per-zone
//! fragmentation levels, and compaction effectiveness. Essential
//! for understanding memory fragmentation.
//!
//! ## Architecture
//!
//! ```text
//! Buddy allocator monitoring
//!   → buddyinfo::update_zone(name, counts) → update free counts
//!   → buddyinfo::record_split(zone, order) → buddy split
//!   → buddyinfo::record_coalesce(zone, order) → buddy coalesce
//!   → buddyinfo::per_zone() → per-zone stats
//!
//! Integration:
//!   → vmzone (VM zones)
//!   → pagestat (page allocator)
//!   → compstat (compaction stats)
//!   → vmfrag (fragmentation index)
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

/// Maximum buddy order (0..=MAX_ORDER).
pub const MAX_ORDER: usize = 11;

/// Per-zone buddy info.
#[derive(Debug, Clone)]
pub struct ZoneBuddyInfo {
    pub zone_name: String,
    pub free_counts: [u64; MAX_ORDER],  // Free pages at each order
    pub splits: [u64; MAX_ORDER],
    pub coalesces: [u64; MAX_ORDER],
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_ZONES: usize = 16;

struct State {
    zones: Vec<ZoneBuddyInfo>,
    total_splits: u64,
    total_coalesces: u64,
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

/// Initialise an **empty** buddy-allocator info table.
///
/// Seeds NO zones and zero totals.  Real fragmentation accounting is wired
/// through [`register_zone`] (one row per memory zone the page allocator
/// brings online) and the `update_free`/`record_split`/`record_coalesce`
/// functions; until those are called the table is genuinely empty, so the
/// `/proc/buddyinfo` file and the `budstat` kshell command report zeros rather
/// than fabricated numbers — the kernel's hard "never invent data in procfs"
/// rule.
///
/// NOTE: this previously seeded two fictional zones ("DMA32" and "Normal") with
/// invented per-order free_counts, splits (up to 1_000_000 at order 0), and
/// coalesces, plus invented aggregate totals (total_splits 2_189_600,
/// total_coalesces 2_079_570), which `/proc/buddyinfo` then displayed as if
/// they were real per-zone fragmentation measurements.  That demo data was
/// removed; the self-test now builds its own fixtures explicitly via the real
/// API (see [`self_test`]).  The page allocator is expected to call
/// [`register_zone`] per online zone and the update/record functions as the
/// buddy structure splits and coalesces.
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        zones: Vec::new(),
        total_splits: 0,
        total_coalesces: 0,
        ops: 0,
    });
}

/// Register a zone.
pub fn register_zone(name: &str) -> KernelResult<()> {
    with_state(|state| {
        if state.zones.len() >= MAX_ZONES { return Err(KernelError::ResourceExhausted); }
        if state.zones.iter().any(|z| z.zone_name == name) { return Err(KernelError::AlreadyExists); }
        state.zones.push(ZoneBuddyInfo {
            zone_name: String::from(name),
            free_counts: [0; MAX_ORDER],
            splits: [0; MAX_ORDER],
            coalesces: [0; MAX_ORDER],
        });
        Ok(())
    })
}

/// Update free counts for a zone.
pub fn update_free(name: &str, order: usize, count: u64) -> KernelResult<()> {
    with_state(|state| {
        if order >= MAX_ORDER { return Err(KernelError::InvalidArgument); }
        let z = state.zones.iter_mut().find(|z| z.zone_name == name)
            .ok_or(KernelError::NotFound)?;
        z.free_counts[order] = count;
        Ok(())
    })
}

/// Record a buddy split.
pub fn record_split(name: &str, order: usize) -> KernelResult<()> {
    with_state(|state| {
        if order >= MAX_ORDER { return Err(KernelError::InvalidArgument); }
        let z = state.zones.iter_mut().find(|z| z.zone_name == name)
            .ok_or(KernelError::NotFound)?;
        z.splits[order] += 1;
        state.total_splits += 1;
        Ok(())
    })
}

/// Record a buddy coalesce.
pub fn record_coalesce(name: &str, order: usize) -> KernelResult<()> {
    with_state(|state| {
        if order >= MAX_ORDER { return Err(KernelError::InvalidArgument); }
        let z = state.zones.iter_mut().find(|z| z.zone_name == name)
            .ok_or(KernelError::NotFound)?;
        z.coalesces[order] += 1;
        state.total_coalesces += 1;
        Ok(())
    })
}

/// Per-zone buddy info.
pub fn per_zone() -> Vec<ZoneBuddyInfo> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.zones.clone())
}

/// Statistics: (zone_count, total_splits, total_coalesces, ops).
pub fn stats() -> (usize, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.zones.len(), s.total_splits, s.total_coalesces, s.ops),
        None => (0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("buddyinfo::self_test() — running tests...");
    // Begin from a clean, EMPTY table and build every fixture via the real
    // API, so the test exercises genuine accounting paths and never relies on
    // fabricated seed data (which /proc/buddyinfo must never surface).
    // Resetting first clears any residue from a prior `budstat test` run so the
    // totals asserted below are exact.
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty after init — no fabricated zones or totals.
    assert_eq!(per_zone().len(), 0);
    let (z0, s0, c0, _o0) = stats();
    assert_eq!((z0, s0, c0), (0, 0, 0));
    crate::serial_println!("  [1/8] empty init: OK");

    // 2: Register zones (zeroed); duplicate name fails.
    register_zone("Test").expect("register");
    assert_eq!(per_zone().len(), 1);
    assert!(register_zone("Test").is_err());
    let z = per_zone().iter().find(|z| z.zone_name == "Test").cloned().expect("zone");
    assert_eq!(z.free_counts[0], 0);
    assert_eq!(z.splits[0], 0);
    crate::serial_println!("  [2/8] register: OK");

    // 3: Update free sets the per-order count exactly.
    update_free("Test", 0, 100).expect("update");
    let z = per_zone().iter().find(|z| z.zone_name == "Test").cloned().expect("zone");
    assert_eq!(z.free_counts[0], 100);
    crate::serial_println!("  [3/8] update free: OK");

    // 4: Split increments the per-order counter exactly from zero.
    record_split("Test", 3).expect("split");
    let z = per_zone().iter().find(|z| z.zone_name == "Test").cloned().expect("zone");
    assert_eq!(z.splits[3], 1);
    crate::serial_println!("  [4/8] split: OK");

    // 5: Coalesce increments the per-order counter exactly from zero.
    record_coalesce("Test", 2).expect("coalesce");
    let z = per_zone().iter().find(|z| z.zone_name == "Test").cloned().expect("zone");
    assert_eq!(z.coalesces[2], 1);
    crate::serial_println!("  [5/8] coalesce: OK");

    // 6: Out-of-range order is rejected with InvalidArgument.
    assert!(record_split("Test", MAX_ORDER).is_err());
    assert!(update_free("Test", MAX_ORDER + 1, 1).is_err());
    crate::serial_println!("  [6/8] invalid order: OK");

    // 7: Operations on an unregistered zone fail with NotFound.
    assert!(record_split("nonexist", 0).is_err());
    assert!(update_free("nonexist", 0, 1).is_err());
    crate::serial_println!("  [7/8] not found: OK");

    // 8: Aggregate totals equal the exact sums of the operations above.
    let (zones, splits, coalesces, ops) = stats();
    assert_eq!(zones, 1);
    assert_eq!(splits, 1);     // one record_split
    assert_eq!(coalesces, 1);  // one record_coalesce
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Leave NO residue: a diagnostic self-test must not populate the live
    // /proc/buddyinfo table with its fixtures.  Reset to the uninitialised
    // state so production reads report an empty table until the page allocator
    // wires real accounting.
    *STATE.lock() = None;

    crate::serial_println!("buddyinfo::self_test() — all 8 tests passed");
}
