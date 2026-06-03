//! `<linux/cxl_mem.h>` — CXL memory device constants.
//!
//! CXL (Compute Express Link) is a cache-coherent interconnect built
//! on PCIe that enables memory expansion and pooling. CXL memory
//! devices (Type 3) attach additional DRAM or persistent memory to
//! the system's memory hierarchy with cache coherency. The CXL
//! driver exposes memory devices via /dev/cxlN and provides IOCTLs
//! for device management, health monitoring, and firmware updates.

// ---------------------------------------------------------------------------
// CXL memory device IOCTLs
// ---------------------------------------------------------------------------

/// Send a CXL mailbox command to the device.
pub const CXL_MEM_SEND_COMMAND: u32 = 0x01;
/// Query the number of supported commands.
pub const CXL_MEM_QUERY_COMMANDS: u32 = 0x02;

// ---------------------------------------------------------------------------
// CXL device types
// ---------------------------------------------------------------------------

/// Type 1 device (CXL.io + CXL.cache, accelerator).
pub const CXL_DEV_TYPE1: u32 = 1;
/// Type 2 device (CXL.io + CXL.cache + CXL.mem, GPU/accelerator with mem).
pub const CXL_DEV_TYPE2: u32 = 2;
/// Type 3 device (CXL.io + CXL.mem, memory expander).
pub const CXL_DEV_TYPE3: u32 = 3;

// ---------------------------------------------------------------------------
// CXL memory interleave granularity
// ---------------------------------------------------------------------------

/// 256 bytes interleave.
pub const CXL_INTERLEAVE_256B: u32 = 256;
/// 512 bytes interleave.
pub const CXL_INTERLEAVE_512B: u32 = 512;
/// 1 KiB interleave.
pub const CXL_INTERLEAVE_1K: u32 = 1024;
/// 2 KiB interleave.
pub const CXL_INTERLEAVE_2K: u32 = 2048;
/// 4 KiB interleave.
pub const CXL_INTERLEAVE_4K: u32 = 4096;
/// 8 KiB interleave.
pub const CXL_INTERLEAVE_8K: u32 = 8192;
/// 16 KiB interleave.
pub const CXL_INTERLEAVE_16K: u32 = 16384;

// ---------------------------------------------------------------------------
// CXL memory region types
// ---------------------------------------------------------------------------

/// RAM region (volatile CXL memory).
pub const CXL_REGION_RAM: u32 = 0;
/// PMEM region (persistent CXL memory).
pub const CXL_REGION_PMEM: u32 = 1;

// ---------------------------------------------------------------------------
// CXL health info flags
// ---------------------------------------------------------------------------

/// Device health: normal.
pub const CXL_HEALTH_NORMAL: u32 = 0;
/// Device health: non-critical (degraded performance).
pub const CXL_HEALTH_NONCRITICAL: u32 = 1;
/// Device health: critical (risk of data loss).
pub const CXL_HEALTH_CRITICAL: u32 = 2;
/// Device health: fatal (device unusable).
pub const CXL_HEALTH_FATAL: u32 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ioctls_distinct() {
        assert_ne!(CXL_MEM_SEND_COMMAND, CXL_MEM_QUERY_COMMANDS);
    }

    #[test]
    fn test_device_types_distinct() {
        let types = [CXL_DEV_TYPE1, CXL_DEV_TYPE2, CXL_DEV_TYPE3];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_interleave_powers_of_two() {
        let sizes = [
            CXL_INTERLEAVE_256B,
            CXL_INTERLEAVE_512B,
            CXL_INTERLEAVE_1K,
            CXL_INTERLEAVE_2K,
            CXL_INTERLEAVE_4K,
            CXL_INTERLEAVE_8K,
            CXL_INTERLEAVE_16K,
        ];
        for s in sizes {
            assert!(s.is_power_of_two());
        }
        for i in 0..sizes.len() {
            for j in (i + 1)..sizes.len() {
                assert_ne!(sizes[i], sizes[j]);
            }
        }
    }

    #[test]
    fn test_region_types_distinct() {
        assert_ne!(CXL_REGION_RAM, CXL_REGION_PMEM);
    }

    #[test]
    fn test_health_ordered() {
        assert!(CXL_HEALTH_NORMAL < CXL_HEALTH_NONCRITICAL);
        assert!(CXL_HEALTH_NONCRITICAL < CXL_HEALTH_CRITICAL);
        assert!(CXL_HEALTH_CRITICAL < CXL_HEALTH_FATAL);
    }
}
