//! `<linux/vfio.h>` — high-level VFIO container/group/device ioctls.
//!
//! VFIO is the kernel's userspace-driver framework: a process opens
//! `/dev/vfio/vfio` (container), associates a group fd from
//! `/dev/vfio/N`, and fetches device fds. QEMU's `-device vfio-pci`,
//! DPDK, and SPDK use these ioctls to bind a PCI(e) device for
//! userspace DMA.

// ---------------------------------------------------------------------------
// ioctl group letter
// ---------------------------------------------------------------------------

/// Magic letter for VFIO ioctls (';').
pub const VFIO_TYPE: u8 = b';';
/// Base number reserved for the VFIO API.
pub const VFIO_BASE: u32 = 100;

// ---------------------------------------------------------------------------
// Container / group / device ioctls
// ---------------------------------------------------------------------------

/// `VFIO_GET_API_VERSION` — read the version exposed by /dev/vfio/vfio.
pub const VFIO_GET_API_VERSION: u32 = 0x0000_3B64;
/// `VFIO_CHECK_EXTENSION` — query whether an IOMMU type is supported.
pub const VFIO_CHECK_EXTENSION: u32 = 0x0000_3B65;
/// `VFIO_SET_IOMMU` — bind the container to a given IOMMU type.
pub const VFIO_SET_IOMMU: u32 = 0x0000_3B66;
/// `VFIO_GROUP_GET_STATUS` — read group flags (viable / set_container).
pub const VFIO_GROUP_GET_STATUS: u32 = 0x0000_3B67;
/// `VFIO_GROUP_SET_CONTAINER` — attach group to container fd.
pub const VFIO_GROUP_SET_CONTAINER: u32 = 0x0000_3B68;
/// `VFIO_GROUP_UNSET_CONTAINER` — detach group.
pub const VFIO_GROUP_UNSET_CONTAINER: u32 = 0x0000_3B69;
/// `VFIO_GROUP_GET_DEVICE_FD` — fetch a device fd by name.
pub const VFIO_GROUP_GET_DEVICE_FD: u32 = 0x0000_3B6A;
/// `VFIO_DEVICE_GET_INFO` — query device capabilities.
pub const VFIO_DEVICE_GET_INFO: u32 = 0x0000_3B6B;
/// `VFIO_DEVICE_GET_REGION_INFO` — query a BAR/PCI region.
pub const VFIO_DEVICE_GET_REGION_INFO: u32 = 0x0000_3B6C;
/// `VFIO_DEVICE_GET_IRQ_INFO` — query an MSI/MSI-X/legacy IRQ.
pub const VFIO_DEVICE_GET_IRQ_INFO: u32 = 0x0000_3B6D;
/// `VFIO_DEVICE_SET_IRQS` — install eventfds for an IRQ index.
pub const VFIO_DEVICE_SET_IRQS: u32 = 0x0000_3B6E;
/// `VFIO_DEVICE_RESET` — function-level reset.
pub const VFIO_DEVICE_RESET: u32 = 0x0000_3B6F;
/// `VFIO_IOMMU_GET_INFO` — query IOMMU info.
pub const VFIO_IOMMU_GET_INFO: u32 = 0x0000_3B70;
/// `VFIO_IOMMU_MAP_DMA` — install a DMA mapping.
pub const VFIO_IOMMU_MAP_DMA: u32 = 0x0000_3B71;
/// `VFIO_IOMMU_UNMAP_DMA` — remove a DMA mapping.
pub const VFIO_IOMMU_UNMAP_DMA: u32 = 0x0000_3B72;

// ---------------------------------------------------------------------------
// IOMMU types (argument to VFIO_SET_IOMMU)
// ---------------------------------------------------------------------------

/// Type1 IOMMU (modern x86_64 / arm64 default).
pub const VFIO_TYPE1_IOMMU: u32 = 1;
/// SPAPR TCE IOMMU v1 (PPC).
pub const VFIO_SPAPR_TCE_IOMMU: u32 = 2;
/// Type1v2 IOMMU (preferred over Type1 — supports dirty tracking).
pub const VFIO_TYPE1v2_IOMMU: u32 = 3;
/// `VFIO_DMA_CC_IOMMU` — coherent (CC) DMA capability test.
pub const VFIO_DMA_CC_IOMMU: u32 = 4;
/// EEH-extension test.
pub const VFIO_EEH: u32 = 5;
/// SPAPR TCE v2.
pub const VFIO_SPAPR_TCE_v2_IOMMU: u32 = 7;
/// no-IOMMU mode (insecure — root only).
pub const VFIO_NOIOMMU_IOMMU: u32 = 8;

// ---------------------------------------------------------------------------
// VFIO_DEVICE_GET_INFO flags
// ---------------------------------------------------------------------------

/// Device may issue a reset.
pub const VFIO_DEVICE_FLAGS_RESET: u32 = 1 << 0;
/// Device is PCI.
pub const VFIO_DEVICE_FLAGS_PCI: u32 = 1 << 1;
/// Device is platform (FDT).
pub const VFIO_DEVICE_FLAGS_PLATFORM: u32 = 1 << 2;
/// Device is AmbA.
pub const VFIO_DEVICE_FLAGS_AMBA: u32 = 1 << 3;
/// Device is CCW (s390).
pub const VFIO_DEVICE_FLAGS_CCW: u32 = 1 << 4;
/// Device is AP queue (s390).
pub const VFIO_DEVICE_FLAGS_AP: u32 = 1 << 5;
/// Device is FSL-MC.
pub const VFIO_DEVICE_FLAGS_FSL_MC: u32 = 1 << 6;
/// Caps available via the regions list.
pub const VFIO_DEVICE_FLAGS_CAPS: u32 = 1 << 7;

// ---------------------------------------------------------------------------
// DMA map flags
// ---------------------------------------------------------------------------

/// `VFIO_DMA_MAP_FLAG_READ` — readable.
pub const VFIO_DMA_MAP_FLAG_READ: u32 = 1 << 0;
/// `VFIO_DMA_MAP_FLAG_WRITE` — writable.
pub const VFIO_DMA_MAP_FLAG_WRITE: u32 = 1 << 1;
/// `VFIO_DMA_MAP_FLAG_VADDR` — map by virt addr (uses Type1v2 helper).
pub const VFIO_DMA_MAP_FLAG_VADDR: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// Device paths
// ---------------------------------------------------------------------------

/// Container device path.
pub const VFIO_CONTAINER_PATH: &str = "/dev/vfio/vfio";
/// Per-group device prefix.
pub const VFIO_GROUP_DIR: &str = "/dev/vfio";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_magic_letter_semicolon() {
        // VFIO uses ';' as the ioctl type — unusual but stable.
        assert_eq!(VFIO_TYPE, b';');
        assert_eq!(VFIO_BASE, 100);
    }

    #[test]
    fn test_ioctls_distinct_and_use_type() {
        let ops = [
            VFIO_GET_API_VERSION,
            VFIO_CHECK_EXTENSION,
            VFIO_SET_IOMMU,
            VFIO_GROUP_GET_STATUS,
            VFIO_GROUP_SET_CONTAINER,
            VFIO_GROUP_UNSET_CONTAINER,
            VFIO_GROUP_GET_DEVICE_FD,
            VFIO_DEVICE_GET_INFO,
            VFIO_DEVICE_GET_REGION_INFO,
            VFIO_DEVICE_GET_IRQ_INFO,
            VFIO_DEVICE_SET_IRQS,
            VFIO_DEVICE_RESET,
            VFIO_IOMMU_GET_INFO,
            VFIO_IOMMU_MAP_DMA,
            VFIO_IOMMU_UNMAP_DMA,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
            // Type byte ';' (0x3B) in bits 8..15.
            assert_eq!((ops[i] >> 8) & 0xff, b';' as u32);
            // Numbers all ≥ VFIO_BASE.
            assert!(ops[i] & 0xff >= VFIO_BASE);
        }
    }

    #[test]
    fn test_iommu_types_distinct() {
        let t = [
            VFIO_TYPE1_IOMMU,
            VFIO_SPAPR_TCE_IOMMU,
            VFIO_TYPE1v2_IOMMU,
            VFIO_DMA_CC_IOMMU,
            VFIO_EEH,
            VFIO_SPAPR_TCE_v2_IOMMU,
            VFIO_NOIOMMU_IOMMU,
        ];
        for i in 0..t.len() {
            for j in (i + 1)..t.len() {
                assert_ne!(t[i], t[j]);
            }
        }
        // TYPE1=1 has been the userspace default since v3.6.
        assert_eq!(VFIO_TYPE1_IOMMU, 1);
    }

    #[test]
    fn test_device_flags_pow2_distinct() {
        let f = [
            VFIO_DEVICE_FLAGS_RESET,
            VFIO_DEVICE_FLAGS_PCI,
            VFIO_DEVICE_FLAGS_PLATFORM,
            VFIO_DEVICE_FLAGS_AMBA,
            VFIO_DEVICE_FLAGS_CCW,
            VFIO_DEVICE_FLAGS_AP,
            VFIO_DEVICE_FLAGS_FSL_MC,
            VFIO_DEVICE_FLAGS_CAPS,
        ];
        for &b in &f {
            assert!(b.is_power_of_two());
        }
        for i in 0..f.len() {
            for j in (i + 1)..f.len() {
                assert_ne!(f[i], f[j]);
            }
        }
    }

    #[test]
    fn test_dma_map_flags_pow2_distinct() {
        let f = [
            VFIO_DMA_MAP_FLAG_READ,
            VFIO_DMA_MAP_FLAG_WRITE,
            VFIO_DMA_MAP_FLAG_VADDR,
        ];
        for &b in &f {
            assert!(b.is_power_of_two());
        }
        for i in 0..f.len() {
            for j in (i + 1)..f.len() {
                assert_ne!(f[i], f[j]);
            }
        }
    }

    #[test]
    fn test_paths_under_dev_vfio() {
        assert_eq!(VFIO_CONTAINER_PATH, "/dev/vfio/vfio");
        assert_eq!(VFIO_GROUP_DIR, "/dev/vfio");
        assert!(VFIO_CONTAINER_PATH.starts_with(VFIO_GROUP_DIR));
    }
}
