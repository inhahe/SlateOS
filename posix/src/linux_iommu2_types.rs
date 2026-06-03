//! `<linux/iommu.h>` — Additional IOMMU constants.
//!
//! Supplementary IOMMU constants covering page protection flags,
//! cache invalidation types, and hardware capability bits.

// ---------------------------------------------------------------------------
// IOMMU page protection flags
// ---------------------------------------------------------------------------

/// Read permission.
pub const IOMMU_READ: u32 = 1 << 0;
/// Write permission.
pub const IOMMU_WRITE: u32 = 1 << 1;
/// Cache coherency.
pub const IOMMU_CACHE: u32 = 1 << 2;
/// No execute.
pub const IOMMU_NOEXEC: u32 = 1 << 3;
/// Priv (privileged mode access).
pub const IOMMU_PRIV: u32 = 1 << 4;
/// MMIO region.
pub const IOMMU_MMIO: u32 = 1 << 5;

// ---------------------------------------------------------------------------
// IOMMU capabilities
// ---------------------------------------------------------------------------

/// Cache coherent DMA.
pub const IOMMU_CAP_CACHE_COHERENCY: u32 = 0;
/// Interrupt remapping.
pub const IOMMU_CAP_INTR_REMAP: u32 = 1;
/// No execute protection.
pub const IOMMU_CAP_NOEXEC: u32 = 2;
/// Pre-boot DMA protection.
pub const IOMMU_CAP_PRE_BOOT_PROTECTION: u32 = 3;
/// Enforce cache coherent.
pub const IOMMU_CAP_ENFORCE_CACHE_COHERENCY: u32 = 4;
/// Deferred flush.
pub const IOMMU_CAP_DEFERRED_FLUSH: u32 = 5;
/// Dirty tracking.
pub const IOMMU_CAP_DIRTY_TRACKING: u32 = 6;

// ---------------------------------------------------------------------------
// IOMMU hardware info types
// ---------------------------------------------------------------------------

/// None.
pub const IOMMU_HW_INFO_TYPE_NONE: u32 = 0;
/// Intel VT-d.
pub const IOMMU_HW_INFO_TYPE_INTEL_VTD: u32 = 1;
/// ARM SMMU v3.
pub const IOMMU_HW_INFO_TYPE_ARM_SMMUV3: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_page_flags_no_overlap() {
        let flags = [
            IOMMU_READ,
            IOMMU_WRITE,
            IOMMU_CACHE,
            IOMMU_NOEXEC,
            IOMMU_PRIV,
            IOMMU_MMIO,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_caps_distinct() {
        let caps = [
            IOMMU_CAP_CACHE_COHERENCY,
            IOMMU_CAP_INTR_REMAP,
            IOMMU_CAP_NOEXEC,
            IOMMU_CAP_PRE_BOOT_PROTECTION,
            IOMMU_CAP_ENFORCE_CACHE_COHERENCY,
            IOMMU_CAP_DEFERRED_FLUSH,
            IOMMU_CAP_DIRTY_TRACKING,
        ];
        for i in 0..caps.len() {
            for j in (i + 1)..caps.len() {
                assert_ne!(caps[i], caps[j]);
            }
        }
    }

    #[test]
    fn test_hw_info_types_distinct() {
        let types = [
            IOMMU_HW_INFO_TYPE_NONE,
            IOMMU_HW_INFO_TYPE_INTEL_VTD,
            IOMMU_HW_INFO_TYPE_ARM_SMMUV3,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }
}
