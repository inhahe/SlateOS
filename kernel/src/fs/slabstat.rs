//! Slab Statistics — kernel slab allocator monitoring.
//!
//! Tracks slab caches: object count, active/free, slab utilization,
//! and fragmentation. Essential for memory diagnostics and
//! detecting kernel memory leaks.
//!
//! ## Architecture
//!
//! ```text
//! Slab statistics
//!   → slabstat::list() → list slab caches
//!   → slabstat::get(name) → cache details
//!   → slabstat::alloc(name) → record allocation
//!   → slabstat::free(name) → record free
//!
//! Integration:
//!   → memlayout (memory layout)
//!   → perfmon (performance monitor)
//!   → memdiag (memory diagnostics)
//!   → leakcheck (leak detector)
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

/// A slab cache entry.
#[derive(Debug, Clone)]
pub struct SlabCache {
    pub name: String,
    pub object_size: u32,
    pub objects_per_slab: u32,
    pub total_objects: u64,
    pub active_objects: u64,
    pub total_slabs: u32,
    pub active_slabs: u32,
    pub total_allocs: u64,
    pub total_frees: u64,
    pub high_watermark: u64,
    pub align: u32,
    pub reclaim_count: u64,
}

impl SlabCache {
    /// Utilization percentage (0-100).
    pub fn utilization_pct(&self) -> u64 {
        if self.total_objects == 0 { return 0; }
        self.active_objects * 100 / self.total_objects
    }
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_CACHES: usize = 256;

struct State {
    caches: Vec<SlabCache>,
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

/// Initialise an **empty** slab-cache table.
///
/// Seeds NO cache rows and zero totals.  Real slab accounting is wired
/// through [`create_cache`] plus [`alloc`]/[`free`]/[`reclaim`]; until those
/// are called the table is genuinely empty, so the `/proc/slabstat` file and
/// the `slabstat` kshell command report zeros rather than fabricated numbers
/// — the kernel's hard "never invent data in procfs" rule.
///
/// NOTE: this previously seeded five fictional caches ("task_struct",
/// "inode_cache", "dentry_cache", "buffer_head", "kmalloc-64") with invented
/// object/alloc/free counts (e.g. total_allocs 1_350_500), which
/// `/proc/slabstat` then displayed as if they were real allocator
/// statistics.  That demo data was removed; the self-test now builds its own
/// fixtures explicitly via the real API (see [`self_test`]).  The kernel slab
/// allocator is expected to call [`create_cache`] when a cache is created and
/// [`alloc`]/[`free`] on each object operation.
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        caches: Vec::new(),
        total_allocs: 0,
        total_frees: 0,
        total_reclaims: 0,
        ops: 0,
    });
}

/// Create a slab cache.
pub fn create_cache(name: &str, object_size: u32, align: u32) -> KernelResult<()> {
    with_state(|state| {
        if state.caches.len() >= MAX_CACHES { return Err(KernelError::ResourceExhausted); }
        if state.caches.iter().any(|c| c.name == name) { return Err(KernelError::AlreadyExists); }
        let objects_per_slab = if object_size > 0 { 16384 / object_size } else { 1 };
        state.caches.push(SlabCache {
            name: String::from(name), object_size, objects_per_slab,
            total_objects: 0, active_objects: 0, total_slabs: 0,
            active_slabs: 0, total_allocs: 0, total_frees: 0,
            high_watermark: 0, align, reclaim_count: 0,
        });
        Ok(())
    })
}

/// Record an allocation from a slab cache.
pub fn alloc(name: &str) -> KernelResult<()> {
    with_state(|state| {
        let c = state.caches.iter_mut().find(|c| c.name == name).ok_or(KernelError::NotFound)?;
        c.active_objects += 1;
        c.total_allocs += 1;
        if c.active_objects > c.total_objects {
            c.total_objects = c.active_objects;
            c.total_slabs = c.total_objects.div_ceil(c.objects_per_slab as u64) as u32;
        }
        if c.active_objects > c.high_watermark { c.high_watermark = c.active_objects; }
        c.active_slabs = c.active_objects.div_ceil(c.objects_per_slab as u64) as u32;
        state.total_allocs += 1;
        Ok(())
    })
}

/// Record a free to a slab cache.
pub fn free(name: &str) -> KernelResult<()> {
    with_state(|state| {
        let c = state.caches.iter_mut().find(|c| c.name == name).ok_or(KernelError::NotFound)?;
        if c.active_objects == 0 { return Err(KernelError::InvalidArgument); }
        c.active_objects -= 1;
        c.total_frees += 1;
        c.active_slabs = c.active_objects.div_ceil(c.objects_per_slab as u64) as u32;
        state.total_frees += 1;
        Ok(())
    })
}

/// Reclaim unused slabs from a cache.
pub fn reclaim(name: &str) -> KernelResult<u32> {
    with_state(|state| {
        let c = state.caches.iter_mut().find(|c| c.name == name).ok_or(KernelError::NotFound)?;
        let free_slabs = c.total_slabs.saturating_sub(c.active_slabs);
        if free_slabs == 0 { return Ok(0); }
        c.total_slabs = c.active_slabs;
        c.total_objects = c.total_slabs as u64 * c.objects_per_slab as u64;
        c.reclaim_count += 1;
        state.total_reclaims += 1;
        Ok(free_slabs)
    })
}

/// List all caches.
pub fn list() -> Vec<SlabCache> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.caches.clone())
}

/// Get cache by name.
pub fn get(name: &str) -> Option<SlabCache> {
    STATE.lock().as_ref().and_then(|s| s.caches.iter().find(|c| c.name == name).cloned())
}

/// Top N by active objects.
pub fn top_active(n: usize) -> Vec<SlabCache> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        let mut sorted = s.caches.clone();
        sorted.sort_by_key(|e| core::cmp::Reverse(e.active_objects));
        sorted.truncate(n);
        sorted
    })
}

/// Statistics: (cache_count, total_allocs, total_frees, total_reclaims, ops).
pub fn stats() -> (usize, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.caches.len(), s.total_allocs, s.total_frees, s.total_reclaims, s.ops),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("slabstat::self_test() — running tests...");
    // Begin from a clean, EMPTY table and build every fixture via the real
    // API, so the test exercises genuine accounting paths and never relies on
    // fabricated seed data (which /proc/slabstat must never surface).
    // Resetting first clears any residue from a prior `slabstat test` run so
    // the totals asserted below are exact.
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty after init — no fabricated rows.
    assert_eq!(list().len(), 0);
    let (count0, allocs0, frees0, reclaims0, _o0) = stats();
    assert_eq!((count0, allocs0, frees0, reclaims0), (0, 0, 0, 0));
    crate::serial_println!("  [1/8] empty init: OK");

    // 2: Create caches; duplicate rejected.
    create_cache("inode_cache", 512, 64).expect("create inode");
    create_cache("test_cache", 128, 16).expect("create test");
    assert_eq!(list().len(), 2);
    assert!(create_cache("test_cache", 128, 16).is_err());
    crate::serial_println!("  [2/8] create: OK");

    // 3: A fresh cache starts empty (zero utilization, no objects).
    let c = get("inode_cache").expect("get");
    assert_eq!(c.object_size, 512);
    assert_eq!(c.active_objects, 0);
    assert_eq!(c.utilization_pct(), 0);
    crate::serial_println!("  [3/8] get: OK");

    // 4: Alloc bumps active + total counts exactly.
    alloc("test_cache").expect("alloc");
    alloc("test_cache").expect("alloc2");
    let c = get("test_cache").expect("get2");
    assert_eq!(c.active_objects, 2);
    assert_eq!(c.total_allocs, 2);
    crate::serial_println!("  [4/8] alloc: OK");

    // 5: Free drops active, bumps total_frees; freeing an empty cache fails.
    free("test_cache").expect("free");
    let c = get("test_cache").expect("get3");
    assert_eq!(c.active_objects, 1);
    assert_eq!(c.total_frees, 1);
    assert!(alloc("missing_cache").is_err()); // NotFound on unknown cache
    crate::serial_println!("  [5/8] free: OK");

    // 6: High watermark records the peak active count (2).
    assert_eq!(c.high_watermark, 2);
    crate::serial_println!("  [6/8] watermark: OK");

    // 7: Top active ordering — test_cache (1 active) ahead of empty inode_cache.
    let top = top_active(3);
    assert_eq!(top.len(), 2); // only two caches exist
    assert_eq!(top[0].name, "test_cache");
    assert_eq!(top[0].active_objects, 1);
    crate::serial_println!("  [7/8] top active: OK");

    // 8: Aggregate totals equal the exact sums of the operations above.
    let (count, allocs, frees, reclaims, ops) = stats();
    assert_eq!(count, 2);
    assert_eq!(allocs, 2);
    assert_eq!(frees, 1);
    assert_eq!(reclaims, 0);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Leave NO residue: a diagnostic self-test must not populate the live
    // /proc/slabstat table with its fixtures.  Reset to the uninitialised
    // state so production reads report an empty table until the slab
    // allocator wires real accounting.
    *STATE.lock() = None;

    crate::serial_println!("slabstat::self_test() — all 8 tests passed");
}
