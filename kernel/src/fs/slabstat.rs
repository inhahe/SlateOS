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
use spin::Mutex;

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

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        caches: alloc::vec![
            SlabCache { name: String::from("task_struct"), object_size: 4096, objects_per_slab: 4, total_objects: 256, active_objects: 45, total_slabs: 64, active_slabs: 12, total_allocs: 500, total_frees: 455, high_watermark: 60, align: 64, reclaim_count: 0 },
            SlabCache { name: String::from("inode_cache"), object_size: 512, objects_per_slab: 16, total_objects: 2048, active_objects: 1200, total_slabs: 128, active_slabs: 80, total_allocs: 50000, total_frees: 48800, high_watermark: 1500, align: 64, reclaim_count: 5 },
            SlabCache { name: String::from("dentry_cache"), object_size: 256, objects_per_slab: 32, total_objects: 4096, active_objects: 2500, total_slabs: 128, active_slabs: 85, total_allocs: 100000, total_frees: 97500, high_watermark: 3000, align: 64, reclaim_count: 10 },
            SlabCache { name: String::from("buffer_head"), object_size: 128, objects_per_slab: 64, total_objects: 8192, active_objects: 3000, total_slabs: 128, active_slabs: 50, total_allocs: 200000, total_frees: 197000, high_watermark: 5000, align: 32, reclaim_count: 20 },
            SlabCache { name: String::from("kmalloc-64"), object_size: 64, objects_per_slab: 128, total_objects: 16384, active_objects: 800, total_slabs: 128, active_slabs: 10, total_allocs: 1000000, total_frees: 999200, high_watermark: 2000, align: 8, reclaim_count: 50 },
        ],
        total_allocs: 1350500,
        total_frees: 1342955,
        total_reclaims: 85,
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
        sorted.sort_by(|a, b| b.active_objects.cmp(&a.active_objects));
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
    init_defaults();

    // 1: Defaults.
    assert_eq!(list().len(), 5);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Get cache.
    let c = get("inode_cache").expect("get");
    assert_eq!(c.object_size, 512);
    assert!(c.utilization_pct() > 0);
    crate::serial_println!("  [2/8] get: OK");

    // 3: Create cache.
    create_cache("test_cache", 128, 16).expect("create");
    assert_eq!(list().len(), 6);
    assert!(create_cache("test_cache", 128, 16).is_err());
    crate::serial_println!("  [3/8] create: OK");

    // 4: Alloc.
    alloc("test_cache").expect("alloc");
    alloc("test_cache").expect("alloc2");
    let c = get("test_cache").expect("get2");
    assert_eq!(c.active_objects, 2);
    assert_eq!(c.total_allocs, 2);
    crate::serial_println!("  [4/8] alloc: OK");

    // 5: Free.
    free("test_cache").expect("free");
    let c = get("test_cache").expect("get3");
    assert_eq!(c.active_objects, 1);
    assert_eq!(c.total_frees, 1);
    crate::serial_println!("  [5/8] free: OK");

    // 6: High watermark.
    assert_eq!(c.high_watermark, 2);
    crate::serial_println!("  [6/8] watermark: OK");

    // 7: Top active.
    let top = top_active(3);
    assert_eq!(top.len(), 3);
    crate::serial_println!("  [7/8] top active: OK");

    // 8: Stats.
    let (count, allocs, frees, reclaims, ops) = stats();
    assert_eq!(count, 6);
    assert!(allocs > 1350500);
    assert!(frees > 1342955);
    let _ = reclaims;
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("slabstat::self_test() — all 8 tests passed");
}
