//! `<linux/slab.h>` — Slab allocator constants.
//!
//! The slab allocator (SLUB in modern Linux) provides efficient
//! allocation of fixed-size kernel objects. Objects are pre-allocated
//! in "slabs" (groups of pages). When an object is freed, it returns
//! to the slab for reuse — no page allocation/free overhead. Each
//! object type gets its own cache (kmem_cache) with size-matched
//! slabs. There are also general-purpose size classes (kmalloc-8,
//! kmalloc-16, ..., kmalloc-8192) for variable-size allocations.

// ---------------------------------------------------------------------------
// SLUB/SLAB flags (kmem_cache creation)
// ---------------------------------------------------------------------------

/// Align objects to hardware cache line.
pub const SLAB_HWCACHE_ALIGN: u32 = 0x0000_2000;
/// No external fragmentation (one slab per page order).
pub const SLAB_CACHE_DMA: u32 = 0x0000_4000;
/// DMA32 zone allocation.
pub const SLAB_CACHE_DMA32: u32 = 0x0000_8000;
/// Panic if allocation fails.
pub const SLAB_PANIC: u32 = 0x0004_0000;
/// Account memory to memcg.
pub const SLAB_ACCOUNT: u32 = 0x0008_0000;
/// Objects are reclaimable (shrinker integration).
pub const SLAB_RECLAIM_ACCOUNT: u32 = 0x0002_0000;
/// No merge with similar caches (keep separate for debugging).
pub const SLAB_NO_MERGE: u32 = 0x0010_0000;
/// Typesafe by RCU (objects freed after RCU grace period).
pub const SLAB_TYPESAFE_BY_RCU: u32 = 0x0008_0000;

// ---------------------------------------------------------------------------
// kmalloc size classes
// ---------------------------------------------------------------------------

/// Minimum kmalloc allocation size (8 bytes).
pub const KMALLOC_MIN_SIZE: u32 = 8;
/// Maximum kmalloc allocation size (before falling back to vmalloc).
pub const KMALLOC_MAX_SIZE: u32 = 8192;
/// Number of kmalloc size classes.
pub const KMALLOC_NUM_CLASSES: u32 = 14;

// ---------------------------------------------------------------------------
// GFP flags for slab allocations
// ---------------------------------------------------------------------------

/// Can wait/sleep (common case in process context).
pub const GFP_KERNEL: u32 = 0x0000_00D0;
/// Cannot sleep (interrupt context, atomic).
pub const GFP_ATOMIC: u32 = 0x0000_0020;
/// Userspace allocation (can be swapped/reclaimed).
pub const GFP_USER: u32 = 0x0000_0150;
/// High-priority allocation (access emergency reserves).
pub const GFP_HIGH: u32 = 0x0000_0020;
/// Zero-fill the allocation.
pub const GFP_ZERO: u32 = 0x0000_8000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slab_flags_distinct_subset() {
        // Test a subset that should definitely be distinct
        let flags = [
            SLAB_HWCACHE_ALIGN, SLAB_CACHE_DMA, SLAB_CACHE_DMA32,
            SLAB_PANIC, SLAB_RECLAIM_ACCOUNT, SLAB_NO_MERGE,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_kmalloc_sizes() {
        assert!(KMALLOC_MIN_SIZE.is_power_of_two());
        assert!(KMALLOC_MAX_SIZE.is_power_of_two());
        assert!(KMALLOC_MAX_SIZE > KMALLOC_MIN_SIZE);
    }

    #[test]
    fn test_gfp_flags_nonzero() {
        assert!(GFP_KERNEL > 0);
        assert!(GFP_ATOMIC > 0);
        assert!(GFP_USER > 0);
        assert!(GFP_ZERO > 0);
    }
}
