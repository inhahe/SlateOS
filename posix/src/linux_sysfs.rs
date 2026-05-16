//! `<linux/sysfs.h>` — sysfs virtual filesystem constants.
//!
//! sysfs is a virtual filesystem that exports kernel data structures,
//! device attributes, and driver parameters as files and directories.
//! It is typically mounted at /sys. This module defines attribute
//! permissions, group flags, and standard sysfs paths.

// ---------------------------------------------------------------------------
// Attribute permissions (octal mode bits)
// ---------------------------------------------------------------------------

/// Read-only for all.
pub const SYSFS_PERM_RONLY: u32 = 0o444;
/// Read-write for owner, read-only for others.
pub const SYSFS_PERM_RW: u32 = 0o644;
/// Write-only for owner.
pub const SYSFS_PERM_WONLY: u32 = 0o200;
/// Read-only for owner.
pub const SYSFS_PERM_RONLY_OWNER: u32 = 0o400;
/// Read-write for owner only.
pub const SYSFS_PERM_RW_OWNER: u32 = 0o600;

// ---------------------------------------------------------------------------
// Attribute flags
// ---------------------------------------------------------------------------

/// Binary attribute (raw data, not text).
pub const SYSFS_ATTR_BINARY: u32 = 1 << 0;
/// Prealloc attribute buffer.
pub const SYSFS_ATTR_PREALLOC: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// Standard sysfs mount point and paths
// ---------------------------------------------------------------------------

/// Default sysfs mount point.
pub const SYSFS_MOUNT: &str = "/sys";
/// Block devices.
pub const SYSFS_BLOCK: &str = "/sys/block";
/// Bus types.
pub const SYSFS_BUS: &str = "/sys/bus";
/// Device classes.
pub const SYSFS_CLASS: &str = "/sys/class";
/// All devices.
pub const SYSFS_DEVICES: &str = "/sys/devices";
/// Firmware interfaces.
pub const SYSFS_FIRMWARE: &str = "/sys/firmware";
/// Kernel subsystem.
pub const SYSFS_KERNEL: &str = "/sys/kernel";
/// Module parameters.
pub const SYSFS_MODULE: &str = "/sys/module";
/// Power management.
pub const SYSFS_POWER: &str = "/sys/power";
/// File systems.
pub const SYSFS_FS: &str = "/sys/fs";

// ---------------------------------------------------------------------------
// Binary attribute limits
// ---------------------------------------------------------------------------

/// Default page size for binary attributes.
pub const SYSFS_BIN_ATTR_SIZE: usize = 4096;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_permissions_distinct() {
        let perms = [
            SYSFS_PERM_RONLY, SYSFS_PERM_RW, SYSFS_PERM_WONLY,
            SYSFS_PERM_RONLY_OWNER, SYSFS_PERM_RW_OWNER,
        ];
        for i in 0..perms.len() {
            for j in (i + 1)..perms.len() {
                assert_ne!(perms[i], perms[j]);
            }
        }
    }

    #[test]
    fn test_permission_values() {
        assert_eq!(SYSFS_PERM_RONLY, 0o444);
        assert_eq!(SYSFS_PERM_RW, 0o644);
        assert_eq!(SYSFS_PERM_WONLY, 0o200);
    }

    #[test]
    fn test_attr_flags_powers_of_two() {
        assert!(SYSFS_ATTR_BINARY.is_power_of_two());
        assert!(SYSFS_ATTR_PREALLOC.is_power_of_two());
    }

    #[test]
    fn test_attr_flags_no_overlap() {
        assert_eq!(SYSFS_ATTR_BINARY & SYSFS_ATTR_PREALLOC, 0);
    }

    #[test]
    fn test_paths_distinct() {
        let paths = [
            SYSFS_MOUNT, SYSFS_BLOCK, SYSFS_BUS, SYSFS_CLASS,
            SYSFS_DEVICES, SYSFS_FIRMWARE, SYSFS_KERNEL,
            SYSFS_MODULE, SYSFS_POWER, SYSFS_FS,
        ];
        for i in 0..paths.len() {
            for j in (i + 1)..paths.len() {
                assert_ne!(paths[i], paths[j]);
            }
        }
    }

    #[test]
    fn test_all_paths_start_with_sys() {
        let paths = [
            SYSFS_MOUNT, SYSFS_BLOCK, SYSFS_BUS, SYSFS_CLASS,
            SYSFS_DEVICES, SYSFS_FIRMWARE, SYSFS_KERNEL,
            SYSFS_MODULE, SYSFS_POWER, SYSFS_FS,
        ];
        for path in &paths {
            assert!(path.starts_with("/sys"), "{}", path);
        }
    }

    #[test]
    fn test_bin_attr_size() {
        assert_eq!(SYSFS_BIN_ATTR_SIZE, 4096);
    }
}
