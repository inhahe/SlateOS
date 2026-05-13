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
use spin::Mutex;

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

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        zones: alloc::vec![
            ZoneBuddyInfo {
                zone_name: String::from("DMA32"),
                free_counts: [512, 256, 128, 64, 32, 16, 8, 4, 2, 1, 0],
                splits: [100_000, 50_000, 25_000, 12_000, 6_000, 3_000, 1_500, 700, 300, 100, 0],
                coalesces: [90_000, 45_000, 22_000, 11_000, 5_500, 2_800, 1_400, 650, 280, 90, 0],
            },
            ZoneBuddyInfo {
                zone_name: String::from("Normal"),
                free_counts: [4096, 2048, 1024, 512, 256, 128, 64, 32, 16, 8, 4],
                splits: [1_000_000, 500_000, 250_000, 125_000, 60_000, 30_000, 15_000, 7_000, 3_000, 1_000, 0],
                coalesces: [950_000, 475_000, 237_000, 118_000, 58_000, 29_000, 14_500, 6_800, 2_900, 950, 0],
            },
        ],
        total_splits: 2_189_600,
        total_coalesces: 2_079_570,
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
    init_defaults();

    // 1: Defaults.
    assert_eq!(per_zone().len(), 2);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Register.
    register_zone("Test").expect("register");
    assert_eq!(per_zone().len(), 3);
    assert!(register_zone("Test").is_err());
    crate::serial_println!("  [2/8] register: OK");

    // 3: Update free.
    update_free("Test", 0, 100).expect("update");
    let z = per_zone().iter().find(|z| z.zone_name == "Test").cloned().unwrap();
    assert_eq!(z.free_counts[0], 100);
    crate::serial_println!("  [3/8] update free: OK");

    // 4: Split.
    record_split("Test", 3).expect("split");
    let z = per_zone().iter().find(|z| z.zone_name == "Test").cloned().unwrap();
    assert_eq!(z.splits[3], 1);
    crate::serial_println!("  [4/8] split: OK");

    // 5: Coalesce.
    record_coalesce("Test", 2).expect("coalesce");
    let z = per_zone().iter().find(|z| z.zone_name == "Test").cloned().unwrap();
    assert_eq!(z.coalesces[2], 1);
    crate::serial_println!("  [5/8] coalesce: OK");

    // 6: Invalid order.
    assert!(record_split("Test", MAX_ORDER).is_err());
    assert!(update_free("Test", MAX_ORDER + 1, 1).is_err());
    crate::serial_println!("  [6/8] invalid order: OK");

    // 7: Not found.
    assert!(record_split("nonexist", 0).is_err());
    crate::serial_println!("  [7/8] not found: OK");

    // 8: Stats.
    let (zones, splits, coalesces, ops) = stats();
    assert!(zones >= 3);
    assert!(splits > 2_189_600);
    assert!(coalesces > 2_079_570);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("buddyinfo::self_test() — all 8 tests passed");
}
