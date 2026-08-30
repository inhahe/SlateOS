//! `<linux/iommufd.h>` / `<linux/iommu.h>` — IOMMU user ABI.
//!
//! `iommufd` is the modern userspace control plane for the IOMMU
//! (replacing `vfio`'s container/group interface in Linux 6.2+).
//! QEMU, DPDK with VFIO, and SR-IOV passthrough hypervisors create
//! IOAS objects, attach devices to PASIDs, and pin DMA-able buffers
//! through the commands below.

// ---------------------------------------------------------------------------
// Character device
// ---------------------------------------------------------------------------

/// Control path exposed by the iommufd subsystem.
pub const IOMMUFD_DEV_PATH: &str = "/dev/iommu";

// ---------------------------------------------------------------------------
// IOMMUFD ioctl numbers (raw values, _IO('|', n))
// ---------------------------------------------------------------------------

pub const IOMMU_DESTROY: u32 = 0x80;
pub const IOMMU_IOAS_ALLOC: u32 = 0x81;
pub const IOMMU_IOAS_ALLOW_IOVAS: u32 = 0x82;
pub const IOMMU_IOAS_COPY: u32 = 0x83;
pub const IOMMU_IOAS_IOVA_RANGES: u32 = 0x84;
pub const IOMMU_IOAS_MAP: u32 = 0x85;
pub const IOMMU_IOAS_UNMAP: u32 = 0x86;
pub const IOMMU_OPTION: u32 = 0x87;
pub const IOMMU_VFIO_IOAS: u32 = 0x88;
pub const IOMMU_HWPT_ALLOC: u32 = 0x89;
pub const IOMMU_GET_HW_INFO: u32 = 0x8A;

// ---------------------------------------------------------------------------
// IOMMU_IOAS_MAP flags
// ---------------------------------------------------------------------------

pub const IOMMU_IOAS_MAP_FIXED_IOVA: u32 = 1 << 0;
pub const IOMMU_IOAS_MAP_WRITEABLE: u32 = 1 << 1;
pub const IOMMU_IOAS_MAP_READABLE: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// IOMMU_OPTION operands
// ---------------------------------------------------------------------------

pub const IOMMU_OPTION_RLIMIT_MODE: u32 = 0;
pub const IOMMU_OPTION_HUGE_PAGES: u32 = 1;

// ---------------------------------------------------------------------------
// IOMMU_OPTION ops (struct iommu_option.op)
// ---------------------------------------------------------------------------

pub const IOMMU_OPTION_OP_SET: u32 = 0;
pub const IOMMU_OPTION_OP_GET: u32 = 1;

// ---------------------------------------------------------------------------
// IOMMU permission bitfield in older `<linux/iommu.h>`
// ---------------------------------------------------------------------------

pub const IOMMU_READ: u32 = 1 << 0;
pub const IOMMU_WRITE: u32 = 1 << 1;
pub const IOMMU_CACHE: u32 = 1 << 2;
pub const IOMMU_NOEXEC: u32 = 1 << 3;
pub const IOMMU_MMIO: u32 = 1 << 4;
pub const IOMMU_PRIV: u32 = 1 << 5;

// ---------------------------------------------------------------------------
// HWPT (hardware page-table) types
// ---------------------------------------------------------------------------

pub const IOMMU_HWPT_DATA_NONE: u32 = 0;
pub const IOMMU_HWPT_DATA_VTD_S1: u32 = 1;

// ---------------------------------------------------------------------------
// Hardware-info reply types
// ---------------------------------------------------------------------------

pub const IOMMU_HW_INFO_TYPE_NONE: u32 = 0;
pub const IOMMU_HW_INFO_TYPE_INTEL_VTD: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dev_path() {
        assert_eq!(IOMMUFD_DEV_PATH, "/dev/iommu");
    }

    #[test]
    fn test_ioctls_dense_0x80_to_0x8a() {
        let ops = [
            IOMMU_DESTROY,
            IOMMU_IOAS_ALLOC,
            IOMMU_IOAS_ALLOW_IOVAS,
            IOMMU_IOAS_COPY,
            IOMMU_IOAS_IOVA_RANGES,
            IOMMU_IOAS_MAP,
            IOMMU_IOAS_UNMAP,
            IOMMU_OPTION,
            IOMMU_VFIO_IOAS,
            IOMMU_HWPT_ALLOC,
            IOMMU_GET_HW_INFO,
        ];
        for (i, &v) in ops.iter().enumerate() {
            assert_eq!(v as usize, 0x80 + i);
        }
    }

    #[test]
    fn test_map_flags_pow2() {
        for &b in &[
            IOMMU_IOAS_MAP_FIXED_IOVA,
            IOMMU_IOAS_MAP_WRITEABLE,
            IOMMU_IOAS_MAP_READABLE,
        ] {
            assert!(b.is_power_of_two());
        }
    }

    #[test]
    fn test_option_op_codes_distinct() {
        assert_ne!(IOMMU_OPTION_OP_SET, IOMMU_OPTION_OP_GET);
        assert_ne!(IOMMU_OPTION_RLIMIT_MODE, IOMMU_OPTION_HUGE_PAGES);
    }

    #[test]
    fn test_iommu_perm_bits_pow2_and_distinct() {
        let p = [
            IOMMU_READ,
            IOMMU_WRITE,
            IOMMU_CACHE,
            IOMMU_NOEXEC,
            IOMMU_MMIO,
            IOMMU_PRIV,
        ];
        for &b in &p {
            assert!(b.is_power_of_two());
        }
        for i in 0..p.len() {
            for j in (i + 1)..p.len() {
                assert_ne!(p[i], p[j]);
            }
        }
    }

    #[test]
    fn test_hwpt_data_types_distinct() {
        assert_ne!(IOMMU_HWPT_DATA_NONE, IOMMU_HWPT_DATA_VTD_S1);
        assert_ne!(IOMMU_HW_INFO_TYPE_NONE, IOMMU_HW_INFO_TYPE_INTEL_VTD);
    }
}
