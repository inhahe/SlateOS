//! `<linux/configfs.h>` — configfs userspace-driven kernel configuration.
//!
//! configfs is a writable counterpart to sysfs: userspace creates and
//! destroys kernel objects by mkdir/rmdir under /sys/kernel/config and
//! writes attribute files to configure them. Used by USB gadget,
//! target_core, netconsole, IIO triggers, etc.

// ---------------------------------------------------------------------------
// Mount point and filesystem type
// ---------------------------------------------------------------------------

pub const CONFIGFS_MOUNT_DEFAULT: &str = "/sys/kernel/config";
pub const CONFIGFS_FS_TYPE: &str = "configfs";

// ---------------------------------------------------------------------------
// Magic number (configfs superblock)
// ---------------------------------------------------------------------------

/// `CONFIGFS_MAGIC` — superblock magic (matches kernel `0x62656570`).
pub const CONFIGFS_MAGIC: u32 = 0x6265_6570;

// ---------------------------------------------------------------------------
// Attribute file modes
// ---------------------------------------------------------------------------

/// Default attribute mode: rw-r--r-- (0644).
pub const CONFIGFS_ATTR_MODE_DEFAULT: u32 = 0o644;

/// Read-only attribute mode: r--r--r-- (0444).
pub const CONFIGFS_ATTR_MODE_RO: u32 = 0o444;

/// Write-only attribute mode: -w--w---- (0220).
pub const CONFIGFS_ATTR_MODE_WO: u32 = 0o220;

// ---------------------------------------------------------------------------
// Attribute size limit (page size on most archs)
// ---------------------------------------------------------------------------

/// Maximum bytes that may be written to / read from a configfs attribute.
pub const CONFIGFS_ATTR_SIZE_MAX: usize = 4096;

// ---------------------------------------------------------------------------
// Name length limits
// ---------------------------------------------------------------------------

/// Maximum component name length (matches NAME_MAX).
pub const CONFIGFS_NAME_MAX: usize = 255;

/// Maximum nested group depth (kernel does not enforce; practical limit).
pub const CONFIGFS_MAX_DEPTH: usize = 32;

// ---------------------------------------------------------------------------
// Common subsystem names
// ---------------------------------------------------------------------------

pub const CONFIGFS_SUBSYS_USB_GADGET: &str = "usb_gadget";
pub const CONFIGFS_SUBSYS_TARGET: &str = "target";
pub const CONFIGFS_SUBSYS_NETCONSOLE: &str = "netconsole";
pub const CONFIGFS_SUBSYS_NULLB: &str = "nullb";
pub const CONFIGFS_SUBSYS_IIO_TRIGGERS: &str = "iio";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mount_defaults() {
        assert_eq!(CONFIGFS_MOUNT_DEFAULT, "/sys/kernel/config");
        assert_eq!(CONFIGFS_FS_TYPE, "configfs");
        assert!(CONFIGFS_MOUNT_DEFAULT.starts_with("/sys/"));
    }

    #[test]
    fn test_magic_is_beep_ascii() {
        // 0x62656570 = b"beep" little-endian = "peeb" big-endian.
        assert_eq!(CONFIGFS_MAGIC, 0x6265_6570);
        // Verify byte representation.
        let bytes = CONFIGFS_MAGIC.to_be_bytes();
        assert_eq!(&bytes, b"beep");
    }

    #[test]
    fn test_attr_modes_have_user_rw_bits() {
        // Default 0644: user rw, group r, other r.
        assert_eq!(CONFIGFS_ATTR_MODE_DEFAULT, 0o644);
        // Read-only 0444: user r, group r, other r.
        assert_eq!(CONFIGFS_ATTR_MODE_RO, 0o444);
        // Write-only 0220: user w, group w (no read for anyone).
        assert_eq!(CONFIGFS_ATTR_MODE_WO, 0o220);
        // RO and WO are disjoint.
        assert_eq!(CONFIGFS_ATTR_MODE_RO & CONFIGFS_ATTR_MODE_WO, 0);
    }

    #[test]
    fn test_attr_size_is_one_page() {
        assert_eq!(CONFIGFS_ATTR_SIZE_MAX, 4096);
        assert!(CONFIGFS_ATTR_SIZE_MAX.is_power_of_two());
    }

    #[test]
    fn test_name_max_matches_name_max() {
        // Linux NAME_MAX is 255.
        assert_eq!(CONFIGFS_NAME_MAX, 255);
    }

    #[test]
    fn test_subsystem_names_distinct_lowercase() {
        let s = [
            CONFIGFS_SUBSYS_USB_GADGET,
            CONFIGFS_SUBSYS_TARGET,
            CONFIGFS_SUBSYS_NETCONSOLE,
            CONFIGFS_SUBSYS_NULLB,
            CONFIGFS_SUBSYS_IIO_TRIGGERS,
        ];
        for (i, &x) in s.iter().enumerate() {
            for &y in &s[i + 1..] {
                assert_ne!(x, y);
            }
            for c in x.chars() {
                assert!(c.is_ascii_lowercase() || c == '_');
            }
        }
    }
}
