//! `<linux/vfio.h>` — VFIO IOMMU (I/O memory management) constants.
//!
//! VFIO's IOMMU backend manages DMA mappings for devices assigned to
//! userspace. When a device is passed through to a VM (or userspace
//! driver), the IOMMU translates device DMA addresses to physical
//! memory. The VFIO IOMMU API allows userspace to create DMA
//! mappings, query IOMMU capabilities, and configure dirty page
//! tracking for live migration.

// ---------------------------------------------------------------------------
// VFIO IOMMU types
// ---------------------------------------------------------------------------

/// Type1 IOMMU (standard x86/ARM IOMMU).
pub const VFIO_TYPE1_IOMMU: u32 = 1;
/// Type1 IOMMU v2 (supports VADDR).
pub const VFIO_TYPE1v2_IOMMU: u32 = 3;
/// SPAPR IOMMU (PowerPC).
pub const VFIO_SPAPR_TCE_IOMMU: u32 = 2;
/// SPAPR IOMMU v2.
pub const VFIO_SPAPR_TCE_v2_IOMMU: u32 = 7;
/// No IOMMU (for legacy devices without IOMMU).
pub const VFIO_NOIOMMU_IOMMU: u32 = 8;

// ---------------------------------------------------------------------------
// VFIO IOMMU IOCTLs
// ---------------------------------------------------------------------------

/// Get IOMMU info.
pub const VFIO_IOMMU_GET_INFO: u32 = 0x70;
/// Map DMA (create IOVA → physical mapping).
pub const VFIO_IOMMU_MAP_DMA: u32 = 0x71;
/// Unmap DMA.
pub const VFIO_IOMMU_UNMAP_DMA: u32 = 0x72;
/// Enable IOMMU.
pub const VFIO_IOMMU_ENABLE: u32 = 0x73;
/// Disable IOMMU.
pub const VFIO_IOMMU_DISABLE: u32 = 0x74;
/// Get dirty bitmap (for live migration).
pub const VFIO_IOMMU_DIRTY_PAGES: u32 = 0x75;

// ---------------------------------------------------------------------------
// DMA map flags
// ---------------------------------------------------------------------------

/// Mapping is readable by device.
pub const VFIO_DMA_MAP_FLAG_READ: u32 = 1 << 0;
/// Mapping is writable by device.
pub const VFIO_DMA_MAP_FLAG_WRITE: u32 = 1 << 1;
/// Use VADDR (virtual address, not physical).
pub const VFIO_DMA_MAP_FLAG_VADDR: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// DMA unmap flags
// ---------------------------------------------------------------------------

/// Get dirty bitmap on unmap.
pub const VFIO_DMA_UNMAP_FLAG_GET_DIRTY_BITMAP: u32 = 1 << 0;
/// Unmap all mappings.
pub const VFIO_DMA_UNMAP_FLAG_ALL: u32 = 1 << 1;
/// Unmap VADDR (not physical).
pub const VFIO_DMA_UNMAP_FLAG_VADDR: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// Dirty page tracking flags
// ---------------------------------------------------------------------------

/// Start dirty page tracking.
pub const VFIO_IOMMU_DIRTY_PAGES_FLAG_START: u32 = 1 << 0;
/// Stop dirty page tracking.
pub const VFIO_IOMMU_DIRTY_PAGES_FLAG_STOP: u32 = 1 << 1;
/// Get dirty bitmap.
pub const VFIO_IOMMU_DIRTY_PAGES_FLAG_GET_BITMAP: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// IOMMU info flags
// ---------------------------------------------------------------------------

/// IOMMU supports paging (page-granularity mapping).
pub const VFIO_IOMMU_INFO_PGSIZES: u32 = 1 << 0;
/// IOMMU supports DMA mapping with VADDR.
pub const VFIO_IOMMU_INFO_CAPS: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_iommu_types_distinct() {
        let types = [
            VFIO_TYPE1_IOMMU, VFIO_SPAPR_TCE_IOMMU,
            VFIO_TYPE1v2_IOMMU, VFIO_SPAPR_TCE_v2_IOMMU,
            VFIO_NOIOMMU_IOMMU,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_ioctls_distinct() {
        let ioctls = [
            VFIO_IOMMU_GET_INFO, VFIO_IOMMU_MAP_DMA,
            VFIO_IOMMU_UNMAP_DMA, VFIO_IOMMU_ENABLE,
            VFIO_IOMMU_DISABLE, VFIO_IOMMU_DIRTY_PAGES,
        ];
        for i in 0..ioctls.len() {
            for j in (i + 1)..ioctls.len() {
                assert_ne!(ioctls[i], ioctls[j]);
            }
        }
    }

    #[test]
    fn test_dma_map_flags_no_overlap() {
        let flags = [
            VFIO_DMA_MAP_FLAG_READ,
            VFIO_DMA_MAP_FLAG_WRITE,
            VFIO_DMA_MAP_FLAG_VADDR,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_dirty_pages_flags_no_overlap() {
        let flags = [
            VFIO_IOMMU_DIRTY_PAGES_FLAG_START,
            VFIO_IOMMU_DIRTY_PAGES_FLAG_STOP,
            VFIO_IOMMU_DIRTY_PAGES_FLAG_GET_BITMAP,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}
