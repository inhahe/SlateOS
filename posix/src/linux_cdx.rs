//! `<linux/cdx/cdx_bus.h>` — CDX (Composable DMA eXtensible) bus constants.
//!
//! CDX is a bus framework for FPGA-based hardware accelerators,
//! particularly AMD/Xilinx SmartNIC and compute acceleration cards.
//! It provides a standardized way to discover, configure, and manage
//! DMA-capable hardware functions implemented in FPGA fabric.

// ---------------------------------------------------------------------------
// CDX device class codes
// ---------------------------------------------------------------------------

/// Network device.
pub const CDX_DEV_CLASS_NET: u16 = 0x0200;
/// Processing/compute device.
pub const CDX_DEV_CLASS_PROCESSING: u16 = 0x1200;
/// Crypto/security device.
pub const CDX_DEV_CLASS_CRYPTO: u16 = 0x0B00;

// ---------------------------------------------------------------------------
// CDX device flags
// ---------------------------------------------------------------------------

/// Device supports bus mastering (DMA).
pub const CDX_DEV_FLAG_BUS_MASTER: u32 = 1 << 0;
/// Device supports MSI interrupts.
pub const CDX_DEV_FLAG_MSI: u32 = 1 << 1;
/// Device is in reset state.
pub const CDX_DEV_FLAG_RESET: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// CDX bus commands
// ---------------------------------------------------------------------------

/// Enable device.
pub const CDX_BUS_ENABLE: u8 = 0;
/// Disable device.
pub const CDX_BUS_DISABLE: u8 = 1;
/// Reset device.
pub const CDX_BUS_RESET: u8 = 2;
/// Rescan bus for new devices.
pub const CDX_BUS_RESCAN: u8 = 3;

// ---------------------------------------------------------------------------
// CDX region types
// ---------------------------------------------------------------------------

/// MMIO region.
pub const CDX_REGION_TYPE_MMIO: u8 = 0;
/// Memory region.
pub const CDX_REGION_TYPE_MEM: u8 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_class_codes_distinct() {
        let classes = [CDX_DEV_CLASS_NET, CDX_DEV_CLASS_PROCESSING, CDX_DEV_CLASS_CRYPTO];
        for i in 0..classes.len() {
            for j in (i + 1)..classes.len() {
                assert_ne!(classes[i], classes[j]);
            }
        }
    }

    #[test]
    fn test_flags_no_overlap() {
        let flags = [CDX_DEV_FLAG_BUS_MASTER, CDX_DEV_FLAG_MSI, CDX_DEV_FLAG_RESET];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_commands_distinct() {
        let cmds = [CDX_BUS_ENABLE, CDX_BUS_DISABLE, CDX_BUS_RESET, CDX_BUS_RESCAN];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_region_types_distinct() {
        assert_ne!(CDX_REGION_TYPE_MMIO, CDX_REGION_TYPE_MEM);
    }
}
