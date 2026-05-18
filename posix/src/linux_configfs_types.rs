//! `<linux/configfs.h>` — configfs item type and attribute constants.
//!
//! configfs is a RAM-based filesystem for kernel object configuration.
//! Unlike sysfs (which exports kernel state), configfs allows
//! userspace to *create* and *configure* kernel objects by making
//! directories and writing to attribute files. Used by USB gadgets,
//! target (iSCSI/FC), and NVMET subsystems.

// ---------------------------------------------------------------------------
// configfs item types
// ---------------------------------------------------------------------------

/// Simple config item (file-like attributes).
pub const CONFIGFS_ITEM_ATTR: u32 = 0;
/// Config group (directory, can contain sub-items).
pub const CONFIGFS_ITEM_GROUP: u32 = 1;
/// Default group (auto-created by parent).
pub const CONFIGFS_ITEM_DEFAULT_GROUP: u32 = 2;
/// Binary attribute item.
pub const CONFIGFS_ITEM_BIN_ATTR: u32 = 3;

// ---------------------------------------------------------------------------
// configfs attribute permissions
// ---------------------------------------------------------------------------

/// Read-only attribute.
pub const CONFIGFS_ATTR_RO: u32 = 0o444;
/// Write-only attribute.
pub const CONFIGFS_ATTR_WO: u32 = 0o200;
/// Read-write attribute.
pub const CONFIGFS_ATTR_RW: u32 = 0o644;

// ---------------------------------------------------------------------------
// configfs filesystem magic
// ---------------------------------------------------------------------------

/// configfs filesystem magic number.
pub const CONFIGFS_MAGIC: u64 = 0x62656570;

// ---------------------------------------------------------------------------
// configfs size limits
// ---------------------------------------------------------------------------

/// Maximum attribute value size (one page, 4096 on most).
pub const CONFIGFS_MAX_ATTR_SIZE: u32 = 4096;
/// Maximum name length for items/groups.
pub const CONFIGFS_MAX_NAME_LEN: u32 = 255;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_item_types_distinct() {
        let types = [
            CONFIGFS_ITEM_ATTR, CONFIGFS_ITEM_GROUP,
            CONFIGFS_ITEM_DEFAULT_GROUP, CONFIGFS_ITEM_BIN_ATTR,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_attr_item_is_zero() {
        assert_eq!(CONFIGFS_ITEM_ATTR, 0);
    }

    #[test]
    fn test_permissions_distinct() {
        let perms = [CONFIGFS_ATTR_RO, CONFIGFS_ATTR_WO, CONFIGFS_ATTR_RW];
        for i in 0..perms.len() {
            for j in (i + 1)..perms.len() {
                assert_ne!(perms[i], perms[j]);
            }
        }
    }

    #[test]
    fn test_magic() {
        assert_eq!(CONFIGFS_MAGIC, 0x62656570);
    }

    #[test]
    fn test_limits() {
        assert_eq!(CONFIGFS_MAX_ATTR_SIZE, 4096);
        assert_eq!(CONFIGFS_MAX_NAME_LEN, 255);
    }
}
