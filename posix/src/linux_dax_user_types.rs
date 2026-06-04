//! `<uapi/linux/dax.h>` and `/sys/bus/dax/` — Direct Access char-device.
//!
//! DAX (Direct Access) lets userspace mmap persistent-memory regions
//! directly, bypassing the page cache. Each region is exposed as
//! `/dev/daxN.M` and configured via `/sys/bus/dax/` sysfs attributes.
//! Stores survive across mappings; cache flushing is the caller's
//! responsibility (CLFLUSHOPT / CLWB / sfence).

// ---------------------------------------------------------------------------
// Sysfs / device paths
// ---------------------------------------------------------------------------

pub const DAX_SYSFS_BUS: &str = "/sys/bus/dax";
pub const DAX_SYSFS_DEVICES: &str = "/sys/bus/dax/devices";
pub const DAX_DEV_PREFIX: &str = "/dev/dax";

// ---------------------------------------------------------------------------
// Per-device sysfs attribute filenames
// ---------------------------------------------------------------------------

pub const DAX_ATTR_SIZE: &str = "size";
pub const DAX_ATTR_ALIGN: &str = "align";
pub const DAX_ATTR_TARGET_NODE: &str = "target_node";
pub const DAX_ATTR_RESOURCE: &str = "resource";
pub const DAX_ATTR_REGION_ID: &str = "id";
pub const DAX_ATTR_REGION_SIZE: &str = "region/size";
pub const DAX_ATTR_REGION_ALIGN: &str = "region/align";

// ---------------------------------------------------------------------------
// Supported alignment sizes (must equal a hugepage size on x86_64)
// ---------------------------------------------------------------------------

pub const DAX_ALIGN_4K: u64 = 4 * 1024;
pub const DAX_ALIGN_2M: u64 = 2 * 1024 * 1024;
pub const DAX_ALIGN_1G: u64 = 1024 * 1024 * 1024;

// ---------------------------------------------------------------------------
// Operating modes
// ---------------------------------------------------------------------------

/// "system-ram" mode: hotplug DAX region as kmem (NUMA node).
pub const DAX_MODE_SYSTEM_RAM: &str = "system-ram";
/// "devdax" mode: expose as /dev/daxN.M char device for mmap.
pub const DAX_MODE_DEVDAX: &str = "devdax";

// ---------------------------------------------------------------------------
// MAP flags relevant to DAX (subset of <sys/mman.h>)
// ---------------------------------------------------------------------------

pub const MAP_SHARED_DAX: u32 = 0x01;
pub const MAP_SHARED_VALIDATE_DAX: u32 = 0x03;
pub const MAP_SYNC_DAX: u32 = 0x080000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sysfs_paths_under_sys_bus_dax() {
        assert!(DAX_SYSFS_DEVICES.starts_with(DAX_SYSFS_BUS));
    }

    #[test]
    fn test_dev_prefix_under_dev() {
        assert_eq!(DAX_DEV_PREFIX, "/dev/dax");
        assert!(DAX_DEV_PREFIX.starts_with("/dev/"));
    }

    #[test]
    fn test_alignments_are_x86_64_pagesizes() {
        assert_eq!(DAX_ALIGN_4K, 4096);
        assert_eq!(DAX_ALIGN_2M, 2 * 1024 * 1024);
        assert_eq!(DAX_ALIGN_1G, 1024 * 1024 * 1024);
        for v in [DAX_ALIGN_4K, DAX_ALIGN_2M, DAX_ALIGN_1G] {
            assert!(v.is_power_of_two());
        }
        // Each alignment is 512x the previous.
        assert_eq!(DAX_ALIGN_2M / DAX_ALIGN_4K, 512);
        assert_eq!(DAX_ALIGN_1G / DAX_ALIGN_2M, 512);
    }

    #[test]
    fn test_modes_distinct() {
        assert_ne!(DAX_MODE_SYSTEM_RAM, DAX_MODE_DEVDAX);
        assert!(DAX_MODE_SYSTEM_RAM.contains("ram"));
        assert!(DAX_MODE_DEVDAX.contains("dax"));
    }

    #[test]
    fn test_region_attrs_have_region_prefix() {
        assert!(DAX_ATTR_REGION_SIZE.starts_with("region/"));
        assert!(DAX_ATTR_REGION_ALIGN.starts_with("region/"));
    }

    #[test]
    fn test_map_sync_is_a_high_bit() {
        // MAP_SYNC sits at 0x80000, well above the SHARED/PRIVATE family.
        assert!(MAP_SYNC_DAX > MAP_SHARED_VALIDATE_DAX);
        assert!(MAP_SYNC_DAX.is_power_of_two());
    }

    #[test]
    fn test_shared_validate_supersedes_shared() {
        // MAP_SHARED_VALIDATE = 0x03 = MAP_SHARED (0x01) | MAP_PRIVATE (0x02).
        assert_eq!(MAP_SHARED_VALIDATE_DAX & MAP_SHARED_DAX, MAP_SHARED_DAX);
    }

    #[test]
    fn test_attr_names_nonempty() {
        for a in [
            DAX_ATTR_SIZE,
            DAX_ATTR_ALIGN,
            DAX_ATTR_TARGET_NODE,
            DAX_ATTR_RESOURCE,
            DAX_ATTR_REGION_ID,
        ] {
            assert!(!a.is_empty());
        }
    }
}
