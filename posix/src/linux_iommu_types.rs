//! `<linux/iommu.h>` — I/O Memory Management Unit constants.
//!
//! The IOMMU provides address translation and protection for DMA
//! between devices and system memory. It prevents devices from
//! accessing memory outside their assigned regions, enables DMA
//! remapping for virtualization, and provides scatter-gather for
//! non-contiguous physical memory.

// ---------------------------------------------------------------------------
// IOMMU domain types
// ---------------------------------------------------------------------------

/// Unmanaged domain (caller manages page table).
pub const IOMMU_DOMAIN_UNMANAGED: u8 = 0;
/// DMA domain (kernel-managed IOVA allocator).
pub const IOMMU_DOMAIN_DMA: u8 = 1;
/// Identity (passthrough) domain.
pub const IOMMU_DOMAIN_IDENTITY: u8 = 2;
/// Blocked domain (all accesses fault).
pub const IOMMU_DOMAIN_BLOCKED: u8 = 3;
/// SVA (Shared Virtual Addressing) domain.
pub const IOMMU_DOMAIN_SVA: u8 = 4;

// ---------------------------------------------------------------------------
// IOMMU map/unmap flags
// ---------------------------------------------------------------------------

/// Readable by device.
pub const IOMMU_READ: u32 = 1 << 0;
/// Writable by device.
pub const IOMMU_WRITE: u32 = 1 << 1;
/// Cacheable (device can snoop CPU cache).
pub const IOMMU_CACHE: u32 = 1 << 2;
/// No-execute (for SVA).
pub const IOMMU_NOEXEC: u32 = 1 << 3;
/// Memory-mapped I/O (non-cacheable).
pub const IOMMU_MMIO: u32 = 1 << 4;
/// Privileged access (for PASID).
pub const IOMMU_PRIV: u32 = 1 << 5;

// ---------------------------------------------------------------------------
// IOMMU capabilities
// ---------------------------------------------------------------------------

/// Hardware supports page-level cache invalidation.
pub const IOMMU_CAP_CACHE_COHERENCY: u32 = 1 << 0;
/// Hardware supports I/O page faults.
pub const IOMMU_CAP_IOPF: u32 = 1 << 1;
/// Hardware supports dirty page tracking.
pub const IOMMU_CAP_DIRTY_TRACKING: u32 = 1 << 2;
/// Enforce dirty page tracking.
pub const IOMMU_CAP_ENFORCE_CACHE_COHERENCY: u32 = 1 << 3;

// ---------------------------------------------------------------------------
// IOMMU fault types
// ---------------------------------------------------------------------------

/// DMA read fault.
pub const IOMMU_FAULT_DMA_READ: u8 = 0;
/// DMA write fault.
pub const IOMMU_FAULT_DMA_WRITE: u8 = 1;
/// Page request (PRI).
pub const IOMMU_FAULT_PAGE_REQ: u8 = 2;

// ---------------------------------------------------------------------------
// IOMMU page sizes
// ---------------------------------------------------------------------------

/// 4 KiB page.
pub const IOMMU_PAGE_SIZE_4K: u64 = 4096;
/// 16 KiB page.
pub const IOMMU_PAGE_SIZE_16K: u64 = 16384;
/// 64 KiB page.
pub const IOMMU_PAGE_SIZE_64K: u64 = 65536;
/// 2 MiB huge page.
pub const IOMMU_PAGE_SIZE_2M: u64 = 2 * 1024 * 1024;
/// 1 GiB huge page.
pub const IOMMU_PAGE_SIZE_1G: u64 = 1024 * 1024 * 1024;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_domain_types_distinct() {
        let types = [
            IOMMU_DOMAIN_UNMANAGED,
            IOMMU_DOMAIN_DMA,
            IOMMU_DOMAIN_IDENTITY,
            IOMMU_DOMAIN_BLOCKED,
            IOMMU_DOMAIN_SVA,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_map_flags_no_overlap() {
        let flags = [
            IOMMU_READ,
            IOMMU_WRITE,
            IOMMU_CACHE,
            IOMMU_NOEXEC,
            IOMMU_MMIO,
            IOMMU_PRIV,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_map_flags_power_of_two() {
        let flags = [
            IOMMU_READ,
            IOMMU_WRITE,
            IOMMU_CACHE,
            IOMMU_NOEXEC,
            IOMMU_MMIO,
            IOMMU_PRIV,
        ];
        for f in &flags {
            assert!(f.is_power_of_two());
        }
    }

    #[test]
    fn test_capabilities_no_overlap() {
        let caps = [
            IOMMU_CAP_CACHE_COHERENCY,
            IOMMU_CAP_IOPF,
            IOMMU_CAP_DIRTY_TRACKING,
            IOMMU_CAP_ENFORCE_CACHE_COHERENCY,
        ];
        for i in 0..caps.len() {
            for j in (i + 1)..caps.len() {
                assert_eq!(caps[i] & caps[j], 0);
            }
        }
    }

    #[test]
    fn test_fault_types_distinct() {
        let faults = [
            IOMMU_FAULT_DMA_READ,
            IOMMU_FAULT_DMA_WRITE,
            IOMMU_FAULT_PAGE_REQ,
        ];
        for i in 0..faults.len() {
            for j in (i + 1)..faults.len() {
                assert_ne!(faults[i], faults[j]);
            }
        }
    }

    #[test]
    fn test_page_sizes_increasing() {
        assert!(IOMMU_PAGE_SIZE_4K < IOMMU_PAGE_SIZE_16K);
        assert!(IOMMU_PAGE_SIZE_16K < IOMMU_PAGE_SIZE_64K);
        assert!(IOMMU_PAGE_SIZE_64K < IOMMU_PAGE_SIZE_2M);
        assert!(IOMMU_PAGE_SIZE_2M < IOMMU_PAGE_SIZE_1G);
    }
}
