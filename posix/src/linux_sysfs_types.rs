//! `<linux/sysfs.h>` — sysfs attribute and group constants.
//!
//! sysfs is the virtual filesystem that exports kernel object
//! attributes to userspace. It provides a structured hierarchy
//! under `/sys` for devices, drivers, buses, and other kernel
//! subsystems. These constants define attribute permissions and
//! group visibility modes.

// ---------------------------------------------------------------------------
// sysfs attribute permissions (match standard POSIX bits)
// ---------------------------------------------------------------------------

/// Owner read permission.
pub const SYSFS_ATTR_R: u32 = 0o444;
/// Owner write permission.
pub const SYSFS_ATTR_W: u32 = 0o200;
/// Owner read-write.
pub const SYSFS_ATTR_RW: u32 = 0o644;
/// Root-only write, all read.
pub const SYSFS_ATTR_ROOT_RW: u32 = 0o600;

// ---------------------------------------------------------------------------
// sysfs group types / flags
// ---------------------------------------------------------------------------

/// Standard attribute group (no special flags).
pub const SYSFS_GROUP_NORMAL: u32 = 0;
/// Binary attribute (not text-based).
pub const SYSFS_GROUP_BINARY: u32 = 1;
/// Attribute group that can be conditionally visible.
pub const SYSFS_GROUP_CONDITIONAL: u32 = 2;

// ---------------------------------------------------------------------------
// sysfs kobject types (kobj_type indicators)
// ---------------------------------------------------------------------------

/// Device kobject.
pub const SYSFS_KOBJ_DEVICE: u32 = 0;
/// Driver kobject.
pub const SYSFS_KOBJ_DRIVER: u32 = 1;
/// Module kobject.
pub const SYSFS_KOBJ_MODULE: u32 = 2;
/// Bus kobject.
pub const SYSFS_KOBJ_BUS: u32 = 3;
/// Class kobject.
pub const SYSFS_KOBJ_CLASS: u32 = 4;
/// Firmware kobject.
pub const SYSFS_KOBJ_FIRMWARE: u32 = 5;

// ---------------------------------------------------------------------------
// sysfs special namespace tags
// ---------------------------------------------------------------------------

/// No namespace.
pub const SYSFS_NS_NONE: u32 = 0;
/// Network namespace.
pub const SYSFS_NS_NET: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_permissions() {
        assert_eq!(SYSFS_ATTR_R, 0o444);
        assert_eq!(SYSFS_ATTR_W, 0o200);
        assert_eq!(SYSFS_ATTR_RW, 0o644);
        assert_eq!(SYSFS_ATTR_ROOT_RW, 0o600);
    }

    #[test]
    fn test_group_types_distinct() {
        let types = [
            SYSFS_GROUP_NORMAL,
            SYSFS_GROUP_BINARY,
            SYSFS_GROUP_CONDITIONAL,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_kobj_types_distinct() {
        let types = [
            SYSFS_KOBJ_DEVICE,
            SYSFS_KOBJ_DRIVER,
            SYSFS_KOBJ_MODULE,
            SYSFS_KOBJ_BUS,
            SYSFS_KOBJ_CLASS,
            SYSFS_KOBJ_FIRMWARE,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_ns_types() {
        assert_eq!(SYSFS_NS_NONE, 0);
        assert_eq!(SYSFS_NS_NET, 1);
        assert_ne!(SYSFS_NS_NONE, SYSFS_NS_NET);
    }
}
