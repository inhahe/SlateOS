//! `<linux/fpga-dfl.h>` and `<linux/fpga/*>` — FPGA management userspace ABI.
//!
//! FPGA accelerators (Intel/Altera DFL, Xilinx, Lattice) appear in
//! Linux as character devices under `/dev/dfl-*`. fpgad, OPAE, and
//! kernel-bypass libraries program bitstreams and access AFUs via
//! the ioctls below.

// ---------------------------------------------------------------------------
// Device paths
// ---------------------------------------------------------------------------

/// FPGA Management Engine character device.
pub const FPGA_FME_DEV_PREFIX: &str = "/dev/dfl-fme.";
/// FPGA Accelerated Function Unit character device.
pub const FPGA_PORT_DEV_PREFIX: &str = "/dev/dfl-port.";

// ---------------------------------------------------------------------------
// ioctl magic ('B') and bases
// ---------------------------------------------------------------------------

/// DFL ioctl type byte.
pub const DFL_FPGA_MAGIC: u32 = b'B' as u32;
/// Base for common ioctls.
pub const DFL_FPGA_BASE: u32 = 0;
/// Base for port-class ioctls.
pub const DFL_PORT_BASE: u32 = 0x40;
/// Base for FME (management-engine) ioctls.
pub const DFL_FME_BASE: u32 = 0x80;

// ---------------------------------------------------------------------------
// Common ioctls
// ---------------------------------------------------------------------------

/// `DFL_FPGA_GET_API_VERSION`.
pub const DFL_FPGA_GET_API_VERSION: u32 = 0x0000_4200;
/// `DFL_FPGA_CHECK_EXTENSION`.
pub const DFL_FPGA_CHECK_EXTENSION: u32 = 0x0000_4201;

// ---------------------------------------------------------------------------
// FME (management) ioctls
// ---------------------------------------------------------------------------

/// `DFL_FME_PORT_RELEASE`.
pub const DFL_FME_PORT_RELEASE: u32 = 0x4004_4280;
/// `DFL_FME_PORT_ASSIGN`.
pub const DFL_FME_PORT_ASSIGN: u32 = 0x4004_4281;
/// `DFL_FME_PORT_PR` — partial reconfiguration (load bitstream).
pub const DFL_FME_PORT_PR: u32 = 0x4020_4282;

// ---------------------------------------------------------------------------
// Port (AFU) ioctls
// ---------------------------------------------------------------------------

/// `DFL_FPGA_PORT_RESET`.
pub const DFL_FPGA_PORT_RESET: u32 = 0x0000_4240;
/// `DFL_FPGA_PORT_GET_INFO`.
pub const DFL_FPGA_PORT_GET_INFO: u32 = 0xC010_4241;
/// `DFL_FPGA_PORT_DMA_MAP`.
pub const DFL_FPGA_PORT_DMA_MAP: u32 = 0xC020_4243;
/// `DFL_FPGA_PORT_DMA_UNMAP`.
pub const DFL_FPGA_PORT_DMA_UNMAP: u32 = 0xC008_4244;

// ---------------------------------------------------------------------------
// API version
// ---------------------------------------------------------------------------

/// Current DFL API version reported by the kernel.
pub const DFL_FPGA_API_VERSION: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_paths_and_magic() {
        assert!(FPGA_FME_DEV_PREFIX.starts_with("/dev/"));
        assert!(FPGA_PORT_DEV_PREFIX.starts_with("/dev/"));
        assert_eq!(DFL_FPGA_MAGIC, b'B' as u32);
    }

    #[test]
    fn test_base_grouping() {
        // Bases are 0x40 apart so common/port/FME don't overlap.
        assert_eq!(DFL_FPGA_BASE, 0);
        assert_eq!(DFL_PORT_BASE, 0x40);
        assert_eq!(DFL_FME_BASE, 0x80);
    }

    #[test]
    fn test_ioctls_distinct_and_use_letter_b() {
        let ops = [
            DFL_FPGA_GET_API_VERSION,
            DFL_FPGA_CHECK_EXTENSION,
            DFL_FME_PORT_RELEASE,
            DFL_FME_PORT_ASSIGN,
            DFL_FME_PORT_PR,
            DFL_FPGA_PORT_RESET,
            DFL_FPGA_PORT_GET_INFO,
            DFL_FPGA_PORT_DMA_MAP,
            DFL_FPGA_PORT_DMA_UNMAP,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
            // 'B' (0x42) is the magic byte in bits 8..15.
            assert_eq!((ops[i] >> 8) & 0xff, b'B' as u32);
        }
    }

    #[test]
    fn test_api_version_baseline() {
        assert_eq!(DFL_FPGA_API_VERSION, 1);
    }
}
