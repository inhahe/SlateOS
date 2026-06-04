//! `<linux/device/bus.h>` and sysfs `/sys/bus/*` — driver-bus surface.
//!
//! Every Linux device hangs off one of a small set of named buses
//! (`pci`, `usb`, `platform`, `i2c`, …). Userspace introspects buses
//! through sysfs and triggers uevent rescans through the `uevent`
//! attribute. This module covers the well-known sysfs paths, the
//! uevent action strings, and the match-table flags.

// ---------------------------------------------------------------------------
// sysfs roots
// ---------------------------------------------------------------------------

pub const SYSFS_BUS_ROOT: &str = "/sys/bus";
pub const SYSFS_CLASS_ROOT: &str = "/sys/class";
pub const SYSFS_DEVICES_ROOT: &str = "/sys/devices";
pub const SYSFS_DRIVERS_DIR: &str = "drivers";
pub const SYSFS_DEVICES_DIR: &str = "devices";

// ---------------------------------------------------------------------------
// Bus names (well-known)
// ---------------------------------------------------------------------------

pub const BUS_NAME_PCI: &str = "pci";
pub const BUS_NAME_USB: &str = "usb";
pub const BUS_NAME_PLATFORM: &str = "platform";
pub const BUS_NAME_I2C: &str = "i2c";
pub const BUS_NAME_SPI: &str = "spi";
pub const BUS_NAME_VIRTIO: &str = "virtio";
pub const BUS_NAME_NVME: &str = "nvme";
pub const BUS_NAME_AUXILIARY: &str = "auxiliary";

// ---------------------------------------------------------------------------
// uevent ACTION strings
// ---------------------------------------------------------------------------

pub const UEVENT_ACTION_ADD: &str = "add";
pub const UEVENT_ACTION_REMOVE: &str = "remove";
pub const UEVENT_ACTION_CHANGE: &str = "change";
pub const UEVENT_ACTION_MOVE: &str = "move";
pub const UEVENT_ACTION_ONLINE: &str = "online";
pub const UEVENT_ACTION_OFFLINE: &str = "offline";
pub const UEVENT_ACTION_BIND: &str = "bind";
pub const UEVENT_ACTION_UNBIND: &str = "unbind";

// ---------------------------------------------------------------------------
// netlink kobject_uevent group and max payload
// ---------------------------------------------------------------------------

/// Multicast group used by `kobject_uevent_env()`.
pub const KOBJECT_UEVENT_NETLINK_GROUP: u32 = 1 << 0;

/// Maximum bytes a single uevent payload can carry.
pub const UEVENT_BUFFER_SIZE: usize = 2048;

/// Maximum number of environment variables in a uevent.
pub const UEVENT_NUM_ENVP: usize = 64;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sysfs_roots_canonical() {
        assert_eq!(SYSFS_BUS_ROOT, "/sys/bus");
        assert_eq!(SYSFS_CLASS_ROOT, "/sys/class");
        assert_eq!(SYSFS_DEVICES_ROOT, "/sys/devices");
        // All sit directly under /sys/.
        for p in [SYSFS_BUS_ROOT, SYSFS_CLASS_ROOT, SYSFS_DEVICES_ROOT] {
            assert!(p.starts_with("/sys/"));
        }
    }

    #[test]
    fn test_bus_names_lowercase_alpha() {
        for n in [
            BUS_NAME_PCI,
            BUS_NAME_USB,
            BUS_NAME_PLATFORM,
            BUS_NAME_I2C,
            BUS_NAME_SPI,
            BUS_NAME_VIRTIO,
            BUS_NAME_NVME,
            BUS_NAME_AUXILIARY,
        ] {
            assert!(!n.is_empty());
            // Sysfs names are lowercase ASCII (allowing digits like "i2c").
            for c in n.chars() {
                assert!(c.is_ascii_lowercase() || c.is_ascii_digit());
            }
        }
    }

    #[test]
    fn test_uevent_actions_distinct() {
        let a = [
            UEVENT_ACTION_ADD,
            UEVENT_ACTION_REMOVE,
            UEVENT_ACTION_CHANGE,
            UEVENT_ACTION_MOVE,
            UEVENT_ACTION_ONLINE,
            UEVENT_ACTION_OFFLINE,
            UEVENT_ACTION_BIND,
            UEVENT_ACTION_UNBIND,
        ];
        for (i, &x) in a.iter().enumerate() {
            for &y in &a[i + 1..] {
                assert_ne!(x, y);
            }
        }
        // add/remove pair and bind/unbind pair both end in matching verbs.
        assert!(UEVENT_ACTION_REMOVE.ends_with("remove"));
        assert!(UEVENT_ACTION_UNBIND.ends_with("bind"));
    }

    #[test]
    fn test_uevent_buffer_sizes() {
        assert_eq!(UEVENT_BUFFER_SIZE, 2048);
        assert_eq!(UEVENT_NUM_ENVP, 64);
        // Buffer is a power of two so it round-fills a kmalloc slab.
        assert!(UEVENT_BUFFER_SIZE.is_power_of_two());
        // Plenty of envp slots — typical event uses < 16.
        assert!(UEVENT_NUM_ENVP >= 32);
    }

    #[test]
    fn test_kobject_netlink_group_single_bit() {
        assert!(KOBJECT_UEVENT_NETLINK_GROUP.is_power_of_two());
        assert_eq!(KOBJECT_UEVENT_NETLINK_GROUP, 1);
    }

    #[test]
    fn test_sysfs_subdir_strings() {
        assert_eq!(SYSFS_DRIVERS_DIR, "drivers");
        assert_eq!(SYSFS_DEVICES_DIR, "devices");
    }
}
