//! `<linux/dax.h>` — DAX (Direct Access) constants.
//!
//! DAX allows applications to directly access persistent memory
//! (PMEM/NVDIMM) by memory-mapping the storage device without going
//! through the page cache. This eliminates double-copying and enables
//! byte-addressable access to persistent storage at DRAM speeds.

// ---------------------------------------------------------------------------
// DAX device flags
// ---------------------------------------------------------------------------

/// Device supports DAX.
pub const DAXDEV_F_SYNC: u32 = 1 << 0;
/// DAX device is alive/active.
pub const DAXDEV_F_ALIVE: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// DAX access modes
// ---------------------------------------------------------------------------

/// Read access.
pub const DAX_ACCESS_READ: u32 = 0;
/// Write access.
pub const DAX_ACCESS_WRITE: u32 = 1;

// ---------------------------------------------------------------------------
// DAX page fault sizes
// ---------------------------------------------------------------------------

/// Regular page (4 KiB on x86_64).
pub const DAX_PMD_FAULT_FALLBACK: u32 = 0;
/// PMD-sized page (2 MiB on x86_64).
pub const DAX_PMD_FAULT: u32 = 1;
/// PUD-sized page (1 GiB on x86_64).
pub const DAX_PUD_FAULT: u32 = 2;

// ---------------------------------------------------------------------------
// DAX recovery flags
// ---------------------------------------------------------------------------

/// Recovery write (bypass poisoned pages).
pub const DAX_RECOVERY_WRITE: u32 = 1 << 0;

// ---------------------------------------------------------------------------
// NVDIMM / persistent memory region types
// ---------------------------------------------------------------------------

/// Persistent memory region.
pub const DAX_REGION_PMEM: u8 = 0;
/// Volatile (RAM) region exposed as DAX.
pub const DAX_REGION_VOLATILE: u8 = 1;

// ---------------------------------------------------------------------------
// DAX alignment requirements
// ---------------------------------------------------------------------------

/// Minimum DAX alignment (page size, 4096).
pub const DAX_MIN_ALIGNMENT: u32 = 4096;
/// Preferred alignment for large mappings (2 MiB).
pub const DAX_PMD_ALIGNMENT: u32 = 2 * 1024 * 1024;
/// Huge alignment (1 GiB).
pub const DAX_PUD_ALIGNMENT: u32 = 1024 * 1024 * 1024;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_flags_no_overlap() {
        assert_ne!(DAXDEV_F_SYNC, DAXDEV_F_ALIVE);
        assert!(DAXDEV_F_SYNC.is_power_of_two());
        assert!(DAXDEV_F_ALIVE.is_power_of_two());
        assert_eq!(DAXDEV_F_SYNC & DAXDEV_F_ALIVE, 0);
    }

    #[test]
    fn test_access_modes_distinct() {
        assert_ne!(DAX_ACCESS_READ, DAX_ACCESS_WRITE);
    }

    #[test]
    fn test_fault_sizes_distinct() {
        let sizes = [DAX_PMD_FAULT_FALLBACK, DAX_PMD_FAULT, DAX_PUD_FAULT];
        for i in 0..sizes.len() {
            for j in (i + 1)..sizes.len() {
                assert_ne!(sizes[i], sizes[j]);
            }
        }
    }

    #[test]
    fn test_region_types_distinct() {
        assert_ne!(DAX_REGION_PMEM, DAX_REGION_VOLATILE);
    }

    #[test]
    fn test_alignments() {
        assert!(DAX_MIN_ALIGNMENT.is_power_of_two());
        assert!(DAX_PMD_ALIGNMENT.is_power_of_two());
        assert!(DAX_PUD_ALIGNMENT.is_power_of_two());
        assert!(DAX_MIN_ALIGNMENT < DAX_PMD_ALIGNMENT);
        assert!(DAX_PMD_ALIGNMENT < DAX_PUD_ALIGNMENT);
    }
}
