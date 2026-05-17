//! `<linux/fpga-dfl.h>` — FPGA Device Feature List (DFL) constants.
//!
//! The FPGA DFL framework provides a standardised way to enumerate
//! and manage FPGA features. FPGAs (Intel Stratix, Xilinx/AMD) expose
//! a Device Feature List in BAR space that describes available
//! accelerator functions. Used for FPGA management, partial
//! reconfiguration, and accessing hardware accelerators.

// ---------------------------------------------------------------------------
// FPGA DFL ioctl commands
// ---------------------------------------------------------------------------

/// Get FPGA API version.
pub const DFL_FPGA_GET_API_VERSION: u32 = 0xB600;
/// Check feature extensions.
pub const DFL_FPGA_CHECK_EXTENSION: u32 = 0xB601;

// ---------------------------------------------------------------------------
// FME (FPGA Management Engine) ioctls
// ---------------------------------------------------------------------------

/// Perform partial reconfiguration.
pub const DFL_FPGA_FME_PORT_PR: u32 = 0xC018_B680;
/// Get error info.
pub const DFL_FPGA_FME_ERR_GET_IRQ_NUM: u32 = 0x8004_B683;
/// Set error IRQ.
pub const DFL_FPGA_FME_ERR_SET_IRQ: u32 = 0x4008_B684;

// ---------------------------------------------------------------------------
// Port (AFU - Accelerator Functional Unit) ioctls
// ---------------------------------------------------------------------------

/// Get port info (region count, etc.).
pub const DFL_FPGA_PORT_GET_INFO: u32 = 0xC008_B641;
/// Get region info (mmap offsets).
pub const DFL_FPGA_PORT_GET_REGION_INFO: u32 = 0xC018_B642;
/// DMA map a buffer.
pub const DFL_FPGA_PORT_DMA_MAP: u32 = 0xC028_B643;
/// DMA unmap a buffer.
pub const DFL_FPGA_PORT_DMA_UNMAP: u32 = 0xC008_B644;
/// Reset the port (AFU).
pub const DFL_FPGA_PORT_RESET: u32 = 0xB645;
/// Get port error IRQ count.
pub const DFL_FPGA_PORT_ERR_GET_IRQ_NUM: u32 = 0x8004_B646;
/// Set port error IRQ.
pub const DFL_FPGA_PORT_ERR_SET_IRQ: u32 = 0x4008_B647;

// ---------------------------------------------------------------------------
// PR (Partial Reconfiguration) flags
// ---------------------------------------------------------------------------

/// PR operation in progress.
pub const DFL_FPGA_FME_PORT_PR_IN_PROGRESS: u32 = 1;
/// PR completed successfully.
pub const DFL_FPGA_FME_PORT_PR_COMPLETE: u32 = 0;

// ---------------------------------------------------------------------------
// Region types
// ---------------------------------------------------------------------------

/// MMIO region (memory-mapped I/O).
pub const DFL_PORT_REGION_MMIO: u32 = 0;
/// STP (SignalTap) region (debug).
pub const DFL_PORT_REGION_STP: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_base_ioctls_distinct() {
        assert_ne!(DFL_FPGA_GET_API_VERSION, DFL_FPGA_CHECK_EXTENSION);
    }

    #[test]
    fn test_fme_ioctls_distinct() {
        let cmds = [
            DFL_FPGA_FME_PORT_PR,
            DFL_FPGA_FME_ERR_GET_IRQ_NUM,
            DFL_FPGA_FME_ERR_SET_IRQ,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_port_ioctls_distinct() {
        let cmds = [
            DFL_FPGA_PORT_GET_INFO, DFL_FPGA_PORT_GET_REGION_INFO,
            DFL_FPGA_PORT_DMA_MAP, DFL_FPGA_PORT_DMA_UNMAP,
            DFL_FPGA_PORT_RESET, DFL_FPGA_PORT_ERR_GET_IRQ_NUM,
            DFL_FPGA_PORT_ERR_SET_IRQ,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_region_types_distinct() {
        assert_ne!(DFL_PORT_REGION_MMIO, DFL_PORT_REGION_STP);
    }

    #[test]
    fn test_pr_states_distinct() {
        assert_ne!(
            DFL_FPGA_FME_PORT_PR_IN_PROGRESS,
            DFL_FPGA_FME_PORT_PR_COMPLETE
        );
    }
}
