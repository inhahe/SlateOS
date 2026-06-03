//! `<sys/mman.h>` — `mremap(2)` flags and limits.
//!
//! `mremap` resizes (and optionally moves) an existing mapping in one
//! atomic operation. glibc and jemalloc use it as the fast path for
//! `realloc`-of-a-large-object, the JVM uses it to grow the heap, and
//! CRIU uses `MREMAP_DONTUNMAP` to dup-and-keep an address range while
//! moving its physical pages elsewhere.

// ---------------------------------------------------------------------------
// `mremap(2)` flags
// ---------------------------------------------------------------------------

/// Allow the kernel to relocate the mapping.
pub const MREMAP_MAYMOVE: u32 = 1;
/// Move the mapping to the specified `new_address` (requires MAYMOVE).
pub const MREMAP_FIXED: u32 = 2;
/// Move pages but keep the old VMA intact (Linux 5.7+).
pub const MREMAP_DONTUNMAP: u32 = 4;

/// Mask covering all defined `mremap` flags.
pub const MREMAP_VALID_FLAGS: u32 = MREMAP_MAYMOVE | MREMAP_FIXED | MREMAP_DONTUNMAP;

// ---------------------------------------------------------------------------
// Syscall number
// ---------------------------------------------------------------------------

/// `__NR_mremap` on x86_64.
pub const NR_MREMAP: u32 = 25;

// ---------------------------------------------------------------------------
// Conventional kernel limits referenced by `mremap`
// ---------------------------------------------------------------------------

/// Soft cap on `vm_area_struct` count per process. Override via
/// `/proc/sys/vm/max_map_count`.
pub const DEFAULT_MAX_MAP_COUNT: u32 = 65_530;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mremap_flags_dense_single_bit() {
        for f in [MREMAP_MAYMOVE, MREMAP_FIXED, MREMAP_DONTUNMAP] {
            assert!(f.is_power_of_two());
        }
        // Three dense bits.
        assert_eq!(MREMAP_VALID_FLAGS, 0x7);
    }

    #[test]
    fn test_fixed_requires_maymove_logically() {
        // FIXED is bit 1, MAYMOVE is bit 0 — the kernel rejects FIXED
        // without MAYMOVE, so the two are commonly OR'd. Verify the
        // numeric layout that lets userspace just OR them.
        assert_eq!(MREMAP_FIXED, 2);
        assert_eq!(MREMAP_MAYMOVE, 1);
        assert_eq!(MREMAP_MAYMOVE | MREMAP_FIXED, 3);
    }

    #[test]
    fn test_dontunmap_is_distinct() {
        assert_eq!(MREMAP_DONTUNMAP, 4);
        // DONTUNMAP added in Linux 5.7 — newer than FIXED/MAYMOVE.
        assert_ne!(MREMAP_DONTUNMAP, MREMAP_MAYMOVE);
        assert_ne!(MREMAP_DONTUNMAP, MREMAP_FIXED);
    }

    #[test]
    fn test_syscall_number_and_map_count() {
        // mremap is one of the original 64 syscalls on x86_64.
        assert_eq!(NR_MREMAP, 25);
        // 65530 is the conservative default; many distros raise it.
        assert_eq!(DEFAULT_MAX_MAP_COUNT, 65_530);
    }
}
