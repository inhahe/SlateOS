//! `<linux/cdx/cdx_bus.h>` — AMD/Xilinx CDX (Composable DMA-capable
//! eXchange) bus user surface.
//!
//! CDX is the bus that connects programmable-logic (PL) endpoints on
//! AMD-Xilinx Versal devices to the host. It uses a sysfs interface
//! at `/sys/bus/cdx/` plus a control character device for rescan /
//! reset operations.

// ---------------------------------------------------------------------------
// Sysfs / device paths
// ---------------------------------------------------------------------------

pub const SYSFS_CDX_BUS: &str = "/sys/bus/cdx";
pub const SYSFS_CDX_DEVICES: &str = "/sys/bus/cdx/devices";
pub const SYSFS_CDX_DRIVERS: &str = "/sys/bus/cdx/drivers";

/// Control character device for rescan/reset.
pub const CDX_CONTROL_DEV: &str = "/dev/cdx-control";

// ---------------------------------------------------------------------------
// Control device ioctls (type 0x0E)
// ---------------------------------------------------------------------------

/// `_IO(0x0E, 0x01)` — rescan the CDX bus.
pub const CDX_IOCTL_RESCAN: u32 = 0x0000_0E01;

/// `_IO(0x0E, 0x02)` — reset all CDX devices.
pub const CDX_IOCTL_RESET: u32 = 0x0000_0E02;

// ---------------------------------------------------------------------------
// Device-attribute names (sysfs files)
// ---------------------------------------------------------------------------

pub const CDX_ATTR_VENDOR: &str = "vendor";
pub const CDX_ATTR_DEVICE: &str = "device";
pub const CDX_ATTR_REVISION: &str = "revision";
pub const CDX_ATTR_DRIVER_OVERRIDE: &str = "driver_override";
pub const CDX_ATTR_REMOVE: &str = "remove";
pub const CDX_ATTR_RESET: &str = "reset";

// ---------------------------------------------------------------------------
// Address / size limits
// ---------------------------------------------------------------------------

/// CDX device-ID width (16 bits).
pub const CDX_DEVICE_ID_MAX: u32 = 0xFFFF;

/// CDX vendor-ID width (16 bits).
pub const CDX_VENDOR_ID_MAX: u32 = 0xFFFF;

/// Maximum number of resources (BAR-equivalent) per CDX device.
pub const CDX_MAX_RESOURCES: u32 = 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sysfs_paths_under_bus_root() {
        assert!(SYSFS_CDX_BUS.starts_with("/sys/bus/"));
        assert!(SYSFS_CDX_DEVICES.starts_with(SYSFS_CDX_BUS));
        assert!(SYSFS_CDX_DRIVERS.starts_with(SYSFS_CDX_BUS));
    }

    #[test]
    fn test_control_dev_path_in_dev() {
        assert_eq!(CDX_CONTROL_DEV, "/dev/cdx-control");
        assert!(CDX_CONTROL_DEV.starts_with("/dev/"));
    }

    #[test]
    fn test_ioctls_in_cdx_type_byte() {
        for v in [CDX_IOCTL_RESCAN, CDX_IOCTL_RESET] {
            assert_eq!((v >> 8) & 0xFF, 0x0E);
        }
        // Numbers 1 and 2 are adjacent.
        assert_eq!(CDX_IOCTL_RESET - CDX_IOCTL_RESCAN, 1);
        // Both are pure _IO (no direction bits set).
        assert_eq!(CDX_IOCTL_RESCAN >> 30, 0);
        assert_eq!(CDX_IOCTL_RESET >> 30, 0);
    }

    #[test]
    fn test_attribute_names_unique_lowercase() {
        let a = [
            CDX_ATTR_VENDOR,
            CDX_ATTR_DEVICE,
            CDX_ATTR_REVISION,
            CDX_ATTR_DRIVER_OVERRIDE,
            CDX_ATTR_REMOVE,
            CDX_ATTR_RESET,
        ];
        for (i, &x) in a.iter().enumerate() {
            for &y in &a[i + 1..] {
                assert_ne!(x, y);
            }
            for c in x.chars() {
                assert!(c.is_ascii_lowercase() || c == '_');
            }
        }
    }

    #[test]
    fn test_id_widths_are_16_bit() {
        // Both IDs are u16-wide (matches PCI convention).
        assert_eq!(CDX_DEVICE_ID_MAX, 0xFFFF);
        assert_eq!(CDX_VENDOR_ID_MAX, 0xFFFF);
        assert_eq!(CDX_DEVICE_ID_MAX, (1u32 << 16) - 1);
    }

    #[test]
    fn test_resource_count_bound() {
        assert_eq!(CDX_MAX_RESOURCES, 4);
        // Plenty of headroom for future expansion.
        assert!(CDX_MAX_RESOURCES <= 8);
    }
}
