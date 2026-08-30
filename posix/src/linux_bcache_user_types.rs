//! `<linux/bcache.h>` — bcache control / sysfs surface.
//!
//! Companion to `linux_bcache2_user_types`: that module covers the
//! on-disk superblock; this one covers the userspace control surface
//! exposed via sysfs and the chardev `/dev/bcache_extent_io_done`.

// ---------------------------------------------------------------------------
// sysfs roots
// ---------------------------------------------------------------------------

pub const SYS_FS_BCACHE: &str = "/sys/fs/bcache";
pub const SYS_BLOCK_BCACHE_PREFIX: &str = "bcache";

// ---------------------------------------------------------------------------
// Cache-set attribute names (under `/sys/fs/bcache/<UUID>/`)
// ---------------------------------------------------------------------------

pub const BCACHE_ATTR_REGISTER: &str = "register";
pub const BCACHE_ATTR_REGISTER_QUIET: &str = "register_quiet";
pub const BCACHE_ATTR_UNREGISTER: &str = "unregister";
pub const BCACHE_ATTR_FLASH_VOL_CREATE: &str = "flash_vol_create";
pub const BCACHE_ATTR_CLEAR_STATS: &str = "clear_stats";
pub const BCACHE_ATTR_AVERAGE_KEY_SIZE: &str = "average_key_size";
pub const BCACHE_ATTR_DIRTY_DATA: &str = "dirty_data";
pub const BCACHE_ATTR_ROOT_USAGE_PERCENT: &str = "root_usage_percent";
pub const BCACHE_ATTR_CACHE_AVAILABLE_PERCENT: &str = "cache_available_percent";

// ---------------------------------------------------------------------------
// Backing-device attribute names (under `/sys/block/<bcache>/bcache/`)
// ---------------------------------------------------------------------------

pub const BCACHE_BDEV_ATTR_ATTACH: &str = "attach";
pub const BCACHE_BDEV_ATTR_DETACH: &str = "detach";
pub const BCACHE_BDEV_ATTR_STOP: &str = "stop";
pub const BCACHE_BDEV_ATTR_RUNNING: &str = "running";
pub const BCACHE_BDEV_ATTR_LABEL: &str = "label";

// ---------------------------------------------------------------------------
// `cache_mode` string values written to sysfs
// ---------------------------------------------------------------------------

pub const BCACHE_MODE_NAME_WRITETHROUGH: &str = "writethrough";
pub const BCACHE_MODE_NAME_WRITEBACK: &str = "writeback";
pub const BCACHE_MODE_NAME_WRITEAROUND: &str = "writearound";
pub const BCACHE_MODE_NAME_NONE: &str = "none";

// ---------------------------------------------------------------------------
// Bucket-size limits
// ---------------------------------------------------------------------------

/// Minimum bucket size (must fit at least one full journal block).
pub const BCACHE_MIN_BUCKET_SIZE: u32 = 16 * 1024;
/// Maximum bucket size (single allocation accounted in 16-bit gen).
pub const BCACHE_MAX_BUCKET_SIZE: u32 = 16 * 1024 * 1024;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sysfs_roots() {
        assert_eq!(SYS_FS_BCACHE, "/sys/fs/bcache");
        assert_eq!(SYS_BLOCK_BCACHE_PREFIX, "bcache");
        assert!(SYS_FS_BCACHE.starts_with("/sys/fs/"));
        // The /sys/block/<bcache0,bcache1,...>/ device-name prefix is
        // a bare token, no leading slash.
        assert!(!SYS_BLOCK_BCACHE_PREFIX.starts_with('/'));
    }

    #[test]
    fn test_cache_set_attribute_names_distinct() {
        let a = [
            BCACHE_ATTR_REGISTER,
            BCACHE_ATTR_REGISTER_QUIET,
            BCACHE_ATTR_UNREGISTER,
            BCACHE_ATTR_FLASH_VOL_CREATE,
            BCACHE_ATTR_CLEAR_STATS,
            BCACHE_ATTR_AVERAGE_KEY_SIZE,
            BCACHE_ATTR_DIRTY_DATA,
            BCACHE_ATTR_ROOT_USAGE_PERCENT,
            BCACHE_ATTR_CACHE_AVAILABLE_PERCENT,
        ];
        for (i, &x) in a.iter().enumerate() {
            for &y in &a[i + 1..] {
                assert_ne!(x, y);
            }
            assert!(!x.contains('/'));
        }
        // The register/quiet variants share a prefix.
        assert!(BCACHE_ATTR_REGISTER_QUIET.starts_with(BCACHE_ATTR_REGISTER));
    }

    #[test]
    fn test_bdev_attribute_names_distinct() {
        let a = [
            BCACHE_BDEV_ATTR_ATTACH,
            BCACHE_BDEV_ATTR_DETACH,
            BCACHE_BDEV_ATTR_STOP,
            BCACHE_BDEV_ATTR_RUNNING,
            BCACHE_BDEV_ATTR_LABEL,
        ];
        for (i, &x) in a.iter().enumerate() {
            for &y in &a[i + 1..] {
                assert_ne!(x, y);
            }
        }
        // attach/detach share the trailing 'ttach' (verb pair).
        assert!(BCACHE_BDEV_ATTR_ATTACH.ends_with("attach"));
        assert!(BCACHE_BDEV_ATTR_DETACH.ends_with("etach"));
    }

    #[test]
    fn test_cache_mode_strings_lowercase() {
        let m = [
            BCACHE_MODE_NAME_WRITETHROUGH,
            BCACHE_MODE_NAME_WRITEBACK,
            BCACHE_MODE_NAME_WRITEAROUND,
            BCACHE_MODE_NAME_NONE,
        ];
        for &v in &m {
            assert!(v.bytes().all(|b| b.is_ascii_lowercase()));
        }
        // Three of the four start with "write".
        for &v in &[
            BCACHE_MODE_NAME_WRITETHROUGH,
            BCACHE_MODE_NAME_WRITEBACK,
            BCACHE_MODE_NAME_WRITEAROUND,
        ] {
            assert!(v.starts_with("write"));
        }
        assert_eq!(BCACHE_MODE_NAME_NONE, "none");
    }

    #[test]
    fn test_bucket_size_bounds_are_powers_of_two() {
        assert_eq!(BCACHE_MIN_BUCKET_SIZE, 16 * 1024);
        assert_eq!(BCACHE_MAX_BUCKET_SIZE, 16 * 1024 * 1024);
        assert!(BCACHE_MIN_BUCKET_SIZE.is_power_of_two());
        assert!(BCACHE_MAX_BUCKET_SIZE.is_power_of_two());
        // Max is 1024× min (10 power-of-two steps between 16 KiB and 16 MiB).
        assert_eq!(BCACHE_MAX_BUCKET_SIZE / BCACHE_MIN_BUCKET_SIZE, 1024);
    }
}
