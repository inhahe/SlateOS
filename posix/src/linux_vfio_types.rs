//! `<linux/vfio.h>` — VFIO (Virtual Function I/O) constants.
//!
//! VFIO provides safe, userspace-driven device access for virtual
//! machines and userspace drivers. It uses IOMMU groups to ensure
//! isolation between devices, allowing direct hardware passthrough
//! to VMs (GPU passthrough, NVMe passthrough, network SR-IOV).

// ---------------------------------------------------------------------------
// VFIO device types
// ---------------------------------------------------------------------------

/// PCI device.
pub const VFIO_DEVICE_TYPE_PCI: u32 = 0;
/// Platform device (MMIO-based).
pub const VFIO_DEVICE_TYPE_PLATFORM: u32 = 1;
/// AMBA (ARM) device.
pub const VFIO_DEVICE_TYPE_AMBA: u32 = 2;
/// CDX device.
pub const VFIO_DEVICE_TYPE_CDX: u32 = 3;
/// AP (Adjunct Processor) device (s390).
pub const VFIO_DEVICE_TYPE_AP: u32 = 4;

// ---------------------------------------------------------------------------
// VFIO ioctl commands
// ---------------------------------------------------------------------------

/// Get API version.
pub const VFIO_GET_API_VERSION: u32 = 0x3B64;
/// Check extension support.
pub const VFIO_CHECK_EXTENSION: u32 = 0x3B65;
/// Set IOMMU type.
pub const VFIO_SET_IOMMU: u32 = 0x3B66;
/// Get group status.
pub const VFIO_GROUP_GET_STATUS: u32 = 0x3B67;
/// Set container for group.
pub const VFIO_GROUP_SET_CONTAINER: u32 = 0x3B68;
/// Unset container for group.
pub const VFIO_GROUP_UNSET_CONTAINER: u32 = 0x3B69;
/// Get device fd from group.
pub const VFIO_GROUP_GET_DEVICE_FD: u32 = 0x3B6A;
/// Get device info.
pub const VFIO_DEVICE_GET_INFO: u32 = 0x3B6B;
/// Get region info.
pub const VFIO_DEVICE_GET_REGION_INFO: u32 = 0x3B6C;
/// Get IRQ info.
pub const VFIO_DEVICE_GET_IRQ_INFO: u32 = 0x3B6D;
/// Set IRQs.
pub const VFIO_DEVICE_SET_IRQS: u32 = 0x3B6E;
/// Reset device.
pub const VFIO_DEVICE_RESET: u32 = 0x3B6F;

// ---------------------------------------------------------------------------
// IOMMU types
// ---------------------------------------------------------------------------

/// Type1 IOMMU (AMD-Vi, Intel VT-d).
pub const VFIO_TYPE1_IOMMU: u32 = 1;
/// Type1v2 IOMMU (with dirty tracking).
#[allow(non_upper_case_globals)]
pub const VFIO_TYPE1v2_IOMMU: u32 = 3;
/// No-IOMMU mode (unsafe, for development).
pub const VFIO_NOIOMMU_IOMMU: u32 = 8;

// ---------------------------------------------------------------------------
// DMA map/unmap flags
// ---------------------------------------------------------------------------

/// Map for read access.
pub const VFIO_DMA_MAP_FLAG_READ: u32 = 1 << 0;
/// Map for write access.
pub const VFIO_DMA_MAP_FLAG_WRITE: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// Group status flags
// ---------------------------------------------------------------------------

/// Group is viable (all devices bound to VFIO).
pub const VFIO_GROUP_FLAGS_VIABLE: u32 = 1 << 0;
/// Group has a container set.
pub const VFIO_GROUP_FLAGS_CONTAINER_SET: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_types_distinct() {
        let types = [
            VFIO_DEVICE_TYPE_PCI, VFIO_DEVICE_TYPE_PLATFORM,
            VFIO_DEVICE_TYPE_AMBA, VFIO_DEVICE_TYPE_CDX,
            VFIO_DEVICE_TYPE_AP,
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
            VFIO_GET_API_VERSION, VFIO_CHECK_EXTENSION, VFIO_SET_IOMMU,
            VFIO_GROUP_GET_STATUS, VFIO_GROUP_SET_CONTAINER,
            VFIO_GROUP_UNSET_CONTAINER, VFIO_GROUP_GET_DEVICE_FD,
            VFIO_DEVICE_GET_INFO, VFIO_DEVICE_GET_REGION_INFO,
            VFIO_DEVICE_GET_IRQ_INFO, VFIO_DEVICE_SET_IRQS,
            VFIO_DEVICE_RESET,
        ];
        for i in 0..ioctls.len() {
            for j in (i + 1)..ioctls.len() {
                assert_ne!(ioctls[i], ioctls[j]);
            }
        }
    }

    #[test]
    fn test_iommu_types_distinct() {
        let types = [VFIO_TYPE1_IOMMU, VFIO_TYPE1v2_IOMMU, VFIO_NOIOMMU_IOMMU];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_dma_flags_no_overlap() {
        assert_eq!(VFIO_DMA_MAP_FLAG_READ & VFIO_DMA_MAP_FLAG_WRITE, 0);
    }

    #[test]
    fn test_group_flags_no_overlap() {
        assert_eq!(VFIO_GROUP_FLAGS_VIABLE & VFIO_GROUP_FLAGS_CONTAINER_SET, 0);
    }
}
