//! `<linux/kobject.h>` — Kernel object model constants.
//!
//! kobjects are the foundation of the Linux device model. Every device,
//! driver, bus, and class is represented as a kobject in sysfs. kobjects
//! provide reference counting, sysfs representation, and uevent
//! notification. They form a hierarchy: kobjects belong to ksets,
//! which can be nested to create the /sys directory tree.

// ---------------------------------------------------------------------------
// kobject types
// ---------------------------------------------------------------------------

/// Regular kobject (generic kernel object).
pub const KOBJ_TYPE_REGULAR: u32 = 0;
/// Device kobject (represents a device in /sys/devices/).
pub const KOBJ_TYPE_DEVICE: u32 = 1;
/// Driver kobject (represents a driver in /sys/bus/<bus>/drivers/).
pub const KOBJ_TYPE_DRIVER: u32 = 2;
/// Bus kobject (represents a bus in /sys/bus/).
pub const KOBJ_TYPE_BUS: u32 = 3;
/// Class kobject (represents a class in /sys/class/).
pub const KOBJ_TYPE_CLASS: u32 = 4;
/// Firmware kobject (represents firmware attributes).
pub const KOBJ_TYPE_FIRMWARE: u32 = 5;
/// Module kobject (represents a kernel module in /sys/module/).
pub const KOBJ_TYPE_MODULE: u32 = 6;

// ---------------------------------------------------------------------------
// kobject states
// ---------------------------------------------------------------------------

/// kobject is initialized but not registered.
pub const KOBJ_STATE_INITIALIZED: u32 = 0;
/// kobject is registered in sysfs.
pub const KOBJ_STATE_IN_SYSFS: u32 = 1;
/// kobject uevent has been sent.
pub const KOBJ_STATE_UEVENT_SENT: u32 = 2;
/// kobject is being removed.
pub const KOBJ_STATE_REMOVING: u32 = 3;

// ---------------------------------------------------------------------------
// kobject namespace types
// ---------------------------------------------------------------------------

/// No namespace (default).
pub const KOBJ_NS_TYPE_NONE: u32 = 0;
/// Network namespace.
pub const KOBJ_NS_TYPE_NET: u32 = 1;

// ---------------------------------------------------------------------------
// sysfs attribute permissions
// ---------------------------------------------------------------------------

/// Owner read.
pub const SYSFS_PERM_OWNER_READ: u32 = 0o400;
/// Owner write.
pub const SYSFS_PERM_OWNER_WRITE: u32 = 0o200;
/// Group read.
pub const SYSFS_PERM_GROUP_READ: u32 = 0o040;
/// Others read.
pub const SYSFS_PERM_OTHERS_READ: u32 = 0o004;
/// Standard read-only (0444).
pub const SYSFS_PERM_RO: u32 = 0o444;
/// Standard read-write (0644).
pub const SYSFS_PERM_RW: u32 = 0o644;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_types_distinct() {
        let types = [
            KOBJ_TYPE_REGULAR, KOBJ_TYPE_DEVICE, KOBJ_TYPE_DRIVER,
            KOBJ_TYPE_BUS, KOBJ_TYPE_CLASS, KOBJ_TYPE_FIRMWARE,
            KOBJ_TYPE_MODULE,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_states_distinct() {
        let states = [
            KOBJ_STATE_INITIALIZED, KOBJ_STATE_IN_SYSFS,
            KOBJ_STATE_UEVENT_SENT, KOBJ_STATE_REMOVING,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_ns_types_distinct() {
        assert_ne!(KOBJ_NS_TYPE_NONE, KOBJ_NS_TYPE_NET);
    }

    #[test]
    fn test_sysfs_perms() {
        assert_eq!(SYSFS_PERM_RO, 0o444);
        assert_eq!(SYSFS_PERM_RW, 0o644);
        assert!(SYSFS_PERM_OWNER_READ > SYSFS_PERM_GROUP_READ);
        assert!(SYSFS_PERM_GROUP_READ > SYSFS_PERM_OTHERS_READ);
    }
}
