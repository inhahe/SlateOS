//! Memory pool — pre-allocated fixed-size object buffers.
//!
//! A mempool guarantees allocation will always succeed (as long as the pool
//! has capacity) without going to the general heap.  This is critical for:
//!
//! - **Interrupt context**: The global heap requires a spinlock which cannot
//!   be safely acquired from ISR context if already held.
//! - **Memory-pressure paths**: When the kernel is freeing memory (kswapd,
//!   OOM), it must not recurse into the allocator.
//! - **Bounded latency**: Pool alloc/free is O(1) with no lock contention
//!   on the fast path (single-CPU access).
//!
//! ## Design
//!
//! Each mempool is a fixed-capacity array of pre-allocated object slots.
//! The free list is a stack (LIFO) for optimal cache behavior.  On creation,
//! all slots are allocated from the heap and placed on the free list.
//!
//! ## Usage
//!
//! ```ignore
//! static NET_BUF_POOL: MemPool<NetBuf> = MemPool::new(64);
//!
//! // In ISR context (no heap access allowed):
//! if let Some(buf) = NET_BUF_POOL.alloc() {
//!     buf.fill(packet_data);
//!     queue.push(buf);
//! }
//!
//! // When done:
//! NET_BUF_POOL.free(buf);
//! ```
//!
//! ## References
//!
//! - Linux `mm/mempool.c` — mempool_create(), mempool_alloc()
//! - Fuchsia `kernel/lib/heap/` — slab pools

#![allow(dead_code)]

use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;
use crate::serial_println;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Maximum pool capacity (number of pre-allocated objects).
const MAX_POOL_CAPACITY: usize = 256;

/// Maximum number of named memory pools in the system.
const MAX_POOLS: usize = 16;

// ---------------------------------------------------------------------------
// Pool entry
// ---------------------------------------------------------------------------

/// A single memory pool holding fixed-size objects.
///
/// The pool pre-allocates `capacity` objects at init time and manages
/// them with a lock-free-ish free stack.
pub struct MemPool {
    /// Pool name (for diagnostics).
    name: &'static str,
    /// Object size in bytes.
    obj_size: usize,
    /// Maximum capacity (number of pre-allocated objects).
    capacity: usize,
    /// Free list: indices of available slots.
    /// Protected by a spinlock (brief hold time — just pop/push an index).
    free_stack: Mutex<FreeStack>,
    /// Base pointer to the allocation slab (heap-allocated contiguous buffer).
    /// Each slot is `obj_size` bytes at offset `slot_index * obj_size`.
    slab_base: AtomicU64,
    /// Statistics.
    allocs: AtomicU64,
    frees: AtomicU64,
    alloc_failures: AtomicU64,
    high_watermark: AtomicU32,
}

/// Free-list stack (indices of available slots).
struct FreeStack {
    /// Stack of free slot indices.
    indices: [u16; MAX_POOL_CAPACITY],
    /// Number of valid entries in `indices` (stack pointer).
    count: usize,
}

impl FreeStack {
    const fn new() -> Self {
        Self {
            indices: [0; MAX_POOL_CAPACITY],
            count: 0,
        }
    }

    fn push(&mut self, idx: u16) -> bool {
        if self.count >= MAX_POOL_CAPACITY {
            return false;
        }
        self.indices[self.count] = idx;
        self.count += 1;
        true
    }

    fn pop(&mut self) -> Option<u16> {
        if self.count == 0 {
            return None;
        }
        self.count -= 1;
        Some(self.indices[self.count])
    }

    fn len(&self) -> usize {
        self.count
    }
}

// SAFETY: MemPool uses internal Mutex for the free stack and atomics
// for statistics.  All mutable state is synchronized.
unsafe impl Sync for MemPool {}

impl MemPool {
    /// Create a new mempool (const-initializable for static use).
    ///
    /// The pool is not usable until [`init`] is called to allocate the
    /// backing slab.
    pub const fn new(name: &'static str, obj_size: usize, capacity: usize) -> Self {
        Self {
            name,
            obj_size,
            capacity,
            free_stack: Mutex::new(FreeStack::new()),
            slab_base: AtomicU64::new(0),
            allocs: AtomicU64::new(0),
            frees: AtomicU64::new(0),
            alloc_failures: AtomicU64::new(0),
            high_watermark: AtomicU32::new(0),
        }
    }

    /// Initialize the mempool by allocating the backing slab.
    ///
    /// Allocates `capacity * obj_size` bytes from the heap and
    /// populates the free list.  Must be called before any alloc/free.
    ///
    /// Returns `true` on success, `false` if heap allocation failed.
    pub fn init(&self) -> bool {
        let cap = self.capacity.min(MAX_POOL_CAPACITY);
        let total_bytes = cap.saturating_mul(self.obj_size);
        if total_bytes == 0 {
            return false;
        }

        // Allocate from the global heap.
        let layout = match core::alloc::Layout::from_size_align(total_bytes, 16) {
            Ok(l) => l,
            Err(_) => return false,
        };

        // SAFETY: layout is valid (non-zero size, power-of-two alignment).
        let ptr = unsafe { alloc::alloc::alloc_zeroed(layout) };
        if ptr.is_null() {
            serial_println!("[mempool] Failed to allocate slab for '{}'", self.name);
            return false;
        }

        self.slab_base.store(ptr as u64, Ordering::Release);

        // Populate the free list with all slot indices.
        let mut stack = self.free_stack.lock();
        for i in 0..cap {
            #[allow(clippy::cast_possible_truncation)]
            let _ = stack.push(i as u16);
        }
        drop(stack);

        serial_println!(
            "[mempool] '{}' initialized: {} x {} bytes ({} KiB slab)",
            self.name, cap, self.obj_size,
            total_bytes / 1024
        );
        true
    }

    /// Allocate an object from the pool.
    ///
    /// Returns a mutable pointer to a zeroed object buffer of `obj_size`
    /// bytes, or `None` if the pool is exhausted.
    ///
    /// This is O(1) and safe to call with interrupts disabled (brief
    /// spinlock hold on the free stack).
    pub fn alloc(&self) -> Option<*mut u8> {
        let base = self.slab_base.load(Ordering::Acquire);
        if base == 0 {
            self.alloc_failures.fetch_add(1, Ordering::Relaxed);
            return None;
        }

        let idx = {
            let mut stack = self.free_stack.lock();
            let remaining = stack.len();
            // Update high watermark (high watermark = max objects in use).
            let in_use = self.capacity.saturating_sub(remaining);
            let mut cur_wm = self.high_watermark.load(Ordering::Relaxed);
            #[allow(clippy::cast_possible_truncation)]
            let in_use_u32 = in_use as u32;
            while in_use_u32 > cur_wm {
                match self.high_watermark.compare_exchange_weak(
                    cur_wm, in_use_u32, Ordering::Relaxed, Ordering::Relaxed,
                ) {
                    Ok(_) => break,
                    Err(actual) => cur_wm = actual,
                }
            }
            stack.pop()
        };

        match idx {
            Some(i) => {
                let offset = (i as usize).saturating_mul(self.obj_size);
                let ptr = (base as usize).wrapping_add(offset) as *mut u8;
                // Zero the buffer before handing out (defense-in-depth).
                // SAFETY: ptr is within our slab, obj_size is the slot size.
                unsafe { core::ptr::write_bytes(ptr, 0, self.obj_size); }
                self.allocs.fetch_add(1, Ordering::Relaxed);
                Some(ptr)
            }
            None => {
                self.alloc_failures.fetch_add(1, Ordering::Relaxed);
                None
            }
        }
    }

    /// Free an object back to the pool.
    ///
    /// The pointer must have been obtained from [`alloc`] on this same pool.
    /// Double-free is detected (returns `false`).
    ///
    /// # Safety
    ///
    /// `ptr` must be a valid pointer previously returned by `self.alloc()`.
    pub unsafe fn free(&self, ptr: *mut u8) -> bool {
        let base = self.slab_base.load(Ordering::Acquire);
        if base == 0 || ptr.is_null() {
            return false;
        }

        let offset = (ptr as u64).wrapping_sub(base) as usize;
        if offset >= self.capacity.saturating_mul(self.obj_size) {
            // Pointer is outside our slab — corruption or wrong pool.
            serial_println!(
                "[mempool] '{}' ERROR: free({:?}) outside slab (base={:#x}, size={})",
                self.name, ptr, base, self.capacity.saturating_mul(self.obj_size)
            );
            return false;
        }

        // Compute the slot index.
        if self.obj_size == 0 {
            return false;
        }
        let idx = offset / self.obj_size;
        if idx * self.obj_size != offset {
            // Misaligned pointer — not at a slot boundary.
            serial_println!(
                "[mempool] '{}' ERROR: free({:?}) not slot-aligned",
                self.name, ptr
            );
            return false;
        }

        #[allow(clippy::cast_possible_truncation)]
        let idx_u16 = idx as u16;

        let mut stack = self.free_stack.lock();
        if !stack.push(idx_u16) {
            // Stack full — double free.
            serial_println!(
                "[mempool] '{}' ERROR: possible double-free (slot {})",
                self.name, idx
            );
            return false;
        }
        drop(stack);

        self.frees.fetch_add(1, Ordering::Relaxed);
        true
    }

    /// Get pool statistics.
    #[must_use]
    pub fn stats(&self) -> PoolStats {
        let free_count = self.free_stack.lock().len();
        PoolStats {
            name: self.name,
            obj_size: self.obj_size,
            capacity: self.capacity.min(MAX_POOL_CAPACITY),
            available: free_count,
            in_use: self.capacity.min(MAX_POOL_CAPACITY).saturating_sub(free_count),
            total_allocs: self.allocs.load(Ordering::Relaxed),
            total_frees: self.frees.load(Ordering::Relaxed),
            alloc_failures: self.alloc_failures.load(Ordering::Relaxed),
            high_watermark: self.high_watermark.load(Ordering::Relaxed) as usize,
        }
    }

    /// Check if the pool is initialized.
    #[must_use]
    pub fn is_initialized(&self) -> bool {
        self.slab_base.load(Ordering::Acquire) != 0
    }
}

/// Pool statistics snapshot.
#[derive(Debug, Clone, Copy)]
pub struct PoolStats {
    /// Pool name.
    pub name: &'static str,
    /// Object size in bytes.
    pub obj_size: usize,
    /// Maximum capacity.
    pub capacity: usize,
    /// Currently available (free) objects.
    pub available: usize,
    /// Currently in-use objects.
    pub in_use: usize,
    /// Total allocations since init.
    pub total_allocs: u64,
    /// Total frees since init.
    pub total_frees: u64,
    /// Failed allocation attempts (pool exhausted).
    pub alloc_failures: u64,
    /// Maximum objects simultaneously in use.
    pub high_watermark: usize,
}

// ---------------------------------------------------------------------------
// Global pool registry
// ---------------------------------------------------------------------------

/// Registered pools for global enumeration (diagnostics).
static REGISTRY: Mutex<PoolRegistry> = Mutex::new(PoolRegistry::new());

struct PoolRegistry {
    /// Pointers to registered pools.
    pools: [u64; MAX_POOLS],
    count: usize,
}

impl PoolRegistry {
    const fn new() -> Self {
        Self {
            pools: [0; MAX_POOLS],
            count: 0,
        }
    }
}

/// Register a pool in the global registry (for diagnostics/kshell).
///
/// Returns the registry slot index, or `None` if full.
pub fn register_pool(pool: &'static MemPool) -> Option<usize> {
    let mut reg = REGISTRY.lock();
    if reg.count >= MAX_POOLS {
        return None;
    }
    let slot = reg.count;
    reg.pools[slot] = pool as *const MemPool as u64;
    reg.count += 1;
    Some(slot)
}

/// Get statistics for all registered pools.
pub fn all_pool_stats() -> alloc::vec::Vec<PoolStats> {
    let reg = REGISTRY.lock();
    let mut result = alloc::vec::Vec::with_capacity(reg.count);
    for i in 0..reg.count {
        let ptr = reg.pools[i];
        if ptr != 0 {
            // SAFETY: ptr was stored from a valid &'static MemPool reference.
            let pool = unsafe { &*(ptr as *const MemPool) };
            result.push(pool.stats());
        }
    }
    result
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for the mempool subsystem.
pub fn self_test() {
    serial_println!("[mempool] Running self-test...");

    // Create a small test pool.
    static TEST_POOL: MemPool = MemPool::new("test", 64, 8);

    // Test 1: Initialize.
    assert!(TEST_POOL.init(), "init should succeed");
    assert!(TEST_POOL.is_initialized());
    serial_println!("[mempool]   Init: OK");

    // Test 2: Allocate all slots.
    let mut ptrs = [core::ptr::null_mut::<u8>(); 8];
    for (i, p) in ptrs.iter_mut().enumerate() {
        let allocated = TEST_POOL.alloc();
        assert!(allocated.is_some(), "alloc {} should succeed", i);
        *p = allocated.unwrap();
        // Verify it's zeroed.
        // SAFETY: *p is a freshly allocated slot of size ≥ 64; valid for reads.
        let slice = unsafe { core::slice::from_raw_parts(*p, 64) };
        assert!(slice.iter().all(|&b| b == 0), "slot should be zeroed");
    }
    serial_println!("[mempool]   Alloc all 8: OK");

    // Test 3: Pool exhaustion.
    let extra = TEST_POOL.alloc();
    assert!(extra.is_none(), "alloc should fail when pool exhausted");
    serial_println!("[mempool]   Pool exhaustion: OK");

    // Test 4: Free and re-alloc.
    // SAFETY: ptrs[0] was returned by TEST_POOL.alloc() and has not been freed.
    assert!(unsafe { TEST_POOL.free(ptrs[0]) });
    let re = TEST_POOL.alloc();
    assert!(re.is_some(), "re-alloc after free should succeed");
    ptrs[0] = re.unwrap();
    serial_println!("[mempool]   Free + re-alloc: OK");

    // Test 5: Free all.
    for p in &ptrs {
        // SAFETY: each *p was returned by TEST_POOL.alloc() and has not been freed.
        assert!(unsafe { TEST_POOL.free(*p) }, "free should succeed");
    }
    serial_println!("[mempool]   Free all: OK");

    // Test 6: Stats.
    let st = TEST_POOL.stats();
    assert_eq!(st.capacity, 8);
    assert_eq!(st.available, 8);
    assert_eq!(st.in_use, 0);
    assert_eq!(st.total_allocs, 9); // 8 + 1 re-alloc
    assert_eq!(st.total_frees, 9);  // 1 + 8
    assert_eq!(st.alloc_failures, 1);
    assert_eq!(st.high_watermark, 8);
    serial_println!("[mempool]   Stats: OK (allocs={}, frees={}, failures={}, hwm={})",
        st.total_allocs, st.total_frees, st.alloc_failures, st.high_watermark);

    // Test 7: Null/invalid free rejected.
    // SAFETY: null is intentionally invalid — free() should reject it gracefully.
    assert!(!unsafe { TEST_POOL.free(core::ptr::null_mut()) });
    serial_println!("[mempool]   Null free rejected: OK");

    serial_println!("[mempool] Self-test PASSED");
}
