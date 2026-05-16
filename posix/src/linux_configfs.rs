//! `<linux/configfs.h>` — configfs virtual filesystem constants.
//!
//! configfs is a RAM-based filesystem for userspace-driven kernel
//! object configuration. Unlike sysfs (kernel→userspace), configfs
//! lets userspace create and configure kernel objects by creating
//! directories and writing attributes. Used by LIO (iSCSI target),
//! USB gadget, NVMET, and FPGA manager.

// ---------------------------------------------------------------------------
// configfs mount point
// ---------------------------------------------------------------------------

/// Default configfs mount point.
pub const CONFIGFS_MOUNT: &str = "/sys/kernel/config";

// ---------------------------------------------------------------------------
// Item type flags
// ---------------------------------------------------------------------------

/// Item is a group (can contain children).
pub const CONFIGFS_ITEM_GROUP: u32 = 1 << 0;
/// Item is a default group (auto-created).
pub const CONFIGFS_ITEM_DEFAULT: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// Attribute types
// ---------------------------------------------------------------------------

/// Read-only attribute.
pub const CONFIGFS_ATTR_RO: u32 = 0;
/// Write-only attribute.
pub const CONFIGFS_ATTR_WO: u32 = 1;
/// Read-write attribute.
pub const CONFIGFS_ATTR_RW: u32 = 2;

// ---------------------------------------------------------------------------
// Standard subsystem names
// ---------------------------------------------------------------------------

/// USB gadget configfs.
pub const CONFIGFS_USB_GADGET: &str = "usb_gadget";
/// iSCSI target configfs.
pub const CONFIGFS_TARGET: &str = "target";
/// NVMe target configfs.
pub const CONFIGFS_NVMET: &str = "nvmet";
/// FPGA configfs.
pub const CONFIGFS_FPGA: &str = "fpga";
/// PCI endpoint configfs.
pub const CONFIGFS_PCI_EP: &str = "pci_ep";

// ---------------------------------------------------------------------------
// Size limits
// ---------------------------------------------------------------------------

/// Maximum attribute name length.
pub const CONFIGFS_ITEM_NAME_LEN: usize = 256;
/// Maximum configfs path depth.
pub const CONFIGFS_MAX_DEPTH: usize = 64;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mount_point() {
        assert_eq!(CONFIGFS_MOUNT, "/sys/kernel/config");
    }

    #[test]
    fn test_item_flags_powers_of_two() {
        assert!(CONFIGFS_ITEM_GROUP.is_power_of_two());
        assert!(CONFIGFS_ITEM_DEFAULT.is_power_of_two());
    }

    #[test]
    fn test_item_flags_no_overlap() {
        assert_eq!(CONFIGFS_ITEM_GROUP & CONFIGFS_ITEM_DEFAULT, 0);
    }

    #[test]
    fn test_attr_types_distinct() {
        let types = [CONFIGFS_ATTR_RO, CONFIGFS_ATTR_WO, CONFIGFS_ATTR_RW];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_subsystem_names_distinct() {
        let names = [
            CONFIGFS_USB_GADGET, CONFIGFS_TARGET, CONFIGFS_NVMET,
            CONFIGFS_FPGA, CONFIGFS_PCI_EP,
        ];
        for i in 0..names.len() {
            for j in (i + 1)..names.len() {
                assert_ne!(names[i], names[j]);
            }
        }
    }

    #[test]
    fn test_limits() {
        assert_eq!(CONFIGFS_ITEM_NAME_LEN, 256);
        assert_eq!(CONFIGFS_MAX_DEPTH, 64);
    }
}
