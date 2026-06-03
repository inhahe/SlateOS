//! `<linux/fpga-dfl.h>` — FPGA Device-Feature-List userspace constants.
//!
//! Constants for the DFL (Device Feature List) FPGA framework that
//! presents Intel/Altera FPGA Management Engine and Accelerator
//! Function Units (FME/AFU) to userspace via /dev/dfl-fme.* and
//! /dev/dfl-port.*.

// ---------------------------------------------------------------------------
// API version
// ---------------------------------------------------------------------------

/// Current API version returned by DFL_FPGA_GET_API_VERSION.
pub const DFL_FPGA_API_VERSION: u32 = 1;

// ---------------------------------------------------------------------------
// Port capability flags (DFL_FPGA_PORT_GET_INFO.capability)
// ---------------------------------------------------------------------------

/// Port supports power-management requests.
pub const DFL_PORT_CAP_ERR_IRQ: u32 = 0x0001;
/// Port reports user clock frequencies via ioctl.
pub const DFL_PORT_CAP_UMSG_IRQ: u32 = 0x0002;

// ---------------------------------------------------------------------------
// Port-region flags (DFL_FPGA_PORT_GET_REGION_INFO.flags)
// ---------------------------------------------------------------------------

/// Region is readable.
pub const DFL_PORT_REGION_READ: u32 = 0x0001;
/// Region is writable.
pub const DFL_PORT_REGION_WRITE: u32 = 0x0002;
/// Region is mmap-able.
pub const DFL_PORT_REGION_MMAP: u32 = 0x0004;

// ---------------------------------------------------------------------------
// Port region indexes
// ---------------------------------------------------------------------------

/// AFU MMIO region.
pub const DFL_PORT_REGION_INDEX_AFU: u32 = 0;
/// STP (signal-tap protocol) region.
pub const DFL_PORT_REGION_INDEX_STP: u32 = 1;

// ---------------------------------------------------------------------------
// DMA-buffer flags
// ---------------------------------------------------------------------------

/// DMA buffer is for reading from device.
pub const DFL_FPGA_PORT_DMA_MAP_FLAG_READ: u32 = 0x0001;
/// DMA buffer is for writing to device.
pub const DFL_FPGA_PORT_DMA_MAP_FLAG_WRITE: u32 = 0x0002;

// ---------------------------------------------------------------------------
// FME PR (partial-reconfiguration) flags
// ---------------------------------------------------------------------------

/// Bitstream is encrypted.
pub const DFL_FPGA_FME_PORT_PR_ENCRYPTED: u32 = 0x0001;

// ---------------------------------------------------------------------------
// Error reporting
// ---------------------------------------------------------------------------

/// Maximum number of errors a single status query may report.
pub const DFL_FPGA_FME_ERR_NUM: u32 = 64;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_api_version_positive() {
        assert!(DFL_FPGA_API_VERSION >= 1);
    }

    #[test]
    fn test_cap_flags_distinct() {
        assert!(DFL_PORT_CAP_ERR_IRQ.is_power_of_two());
        assert!(DFL_PORT_CAP_UMSG_IRQ.is_power_of_two());
        assert_ne!(DFL_PORT_CAP_ERR_IRQ, DFL_PORT_CAP_UMSG_IRQ);
    }

    #[test]
    fn test_region_flags_distinct_bits() {
        let flags = [
            DFL_PORT_REGION_READ,
            DFL_PORT_REGION_WRITE,
            DFL_PORT_REGION_MMAP,
        ];
        for &f in &flags {
            assert!(f.is_power_of_two());
        }
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_region_indexes_distinct() {
        assert_ne!(DFL_PORT_REGION_INDEX_AFU, DFL_PORT_REGION_INDEX_STP);
    }

    #[test]
    fn test_dma_flags_distinct_bits() {
        assert!(DFL_FPGA_PORT_DMA_MAP_FLAG_READ.is_power_of_two());
        assert!(DFL_FPGA_PORT_DMA_MAP_FLAG_WRITE.is_power_of_two());
        assert_ne!(
            DFL_FPGA_PORT_DMA_MAP_FLAG_READ,
            DFL_FPGA_PORT_DMA_MAP_FLAG_WRITE
        );
    }

    #[test]
    fn test_pr_flag_single_bit() {
        assert!(DFL_FPGA_FME_PORT_PR_ENCRYPTED.is_power_of_two());
    }

    #[test]
    fn test_err_num_sensible() {
        assert!(DFL_FPGA_FME_ERR_NUM.is_power_of_two());
        assert!(DFL_FPGA_FME_ERR_NUM >= 8);
    }
}
