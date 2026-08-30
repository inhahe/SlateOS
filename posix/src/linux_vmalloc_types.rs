//! `<linux/vmalloc.h>` — vmalloc subsystem constants.
//!
//! vmalloc allocates virtually contiguous memory from the kernel's
//! virtual address space, using non-contiguous physical pages. This
//! is useful for large allocations (module loading, ioremap, large
//! buffers) where physically contiguous memory isn't required.
//! vmalloc memory is more expensive to access than kmalloc memory
//! (requires page table walks, may cause TLB misses) but can satisfy
//! much larger allocations.

// ---------------------------------------------------------------------------
// vmalloc flags
// ---------------------------------------------------------------------------

/// Standard vmalloc (allocate pages, map into vmalloc area).
pub const VM_ALLOC: u32 = 0x0000_0002;
/// Map existing pages (don't allocate, just create mapping).
pub const VM_MAP: u32 = 0x0000_0004;
/// I/O remapping (ioremap, device MMIO).
pub const VM_IOREMAP: u32 = 0x0000_0001;
/// User-mappable vmalloc area (can be mmap'd to userspace).
pub const VM_USERMAP: u32 = 0x0000_0008;
/// DMA-coherent mapping.
pub const VM_DMA_COHERENT: u32 = 0x0000_0010;
/// Use huge pages if possible.
pub const VM_ALLOW_HUGE_VMAP: u32 = 0x0000_0020;
/// No read-ahead (don't prefault neighboring pages).
pub const VM_NO_GUARD: u32 = 0x0000_0040;
/// Flush caches after mapping.
pub const VM_FLUSH_RESET_PERMS: u32 = 0x0000_0100;

// ---------------------------------------------------------------------------
// vmalloc GFP flags (allocation context)
// ---------------------------------------------------------------------------

/// Normal allocation (can sleep, can reclaim).
pub const GFP_VMALLOC_NORMAL: u32 = 0x0000_00D0;
/// Atomic allocation (no sleeping, no reclaim).
pub const GFP_VMALLOC_ATOMIC: u32 = 0x0000_0020;

// ---------------------------------------------------------------------------
// vmalloc area limits
// ---------------------------------------------------------------------------

/// Default vmalloc area size on x86_64 (128 TiB).
pub const VMALLOC_SIZE_X86_64: u64 = 128 * 1024 * 1024 * 1024 * 1024;
/// Guard page size between vmalloc regions.
pub const VMALLOC_GUARD_SIZE: u32 = 4096;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vm_flags_distinct() {
        let flags = [
            VM_ALLOC,
            VM_MAP,
            VM_IOREMAP,
            VM_USERMAP,
            VM_DMA_COHERENT,
            VM_ALLOW_HUGE_VMAP,
            VM_NO_GUARD,
            VM_FLUSH_RESET_PERMS,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_gfp_flags_distinct() {
        assert_ne!(GFP_VMALLOC_NORMAL, GFP_VMALLOC_ATOMIC);
    }

    #[test]
    fn test_guard_size() {
        assert!(VMALLOC_GUARD_SIZE.is_power_of_two());
    }
}
