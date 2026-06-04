//! `<linux/blkdev.h>` zoned-block-device user-facing constants.
//!
//! ZBC/ZNS storage devices partition their LBA space into zones,
//! each of which has a write-pointer and one of a small number of
//! states. This module covers the **sysfs surface** at
//! `/sys/block/<dev>/queue/zone*`, the zoned-mode strings, and the
//! per-device append/granularity bounds.

// ---------------------------------------------------------------------------
// Zoned-mode strings (read from `/sys/block/<dev>/queue/zoned`)
// ---------------------------------------------------------------------------

/// Device is not zoned (a regular block device).
pub const SYSFS_ZONED_NONE: &str = "none";

/// Host-aware: the host *may* respect zone boundaries.
pub const SYSFS_ZONED_HOST_AWARE: &str = "host-aware";

/// Host-managed: the host *must* respect zone boundaries.
pub const SYSFS_ZONED_HOST_MANAGED: &str = "host-managed";

// ---------------------------------------------------------------------------
// Zoned queue sysfs attribute names
// ---------------------------------------------------------------------------

pub const SYSFS_QUEUE_ZONED: &str = "zoned";
pub const SYSFS_QUEUE_NR_ZONES: &str = "nr_zones";
pub const SYSFS_QUEUE_CHUNK_SECTORS: &str = "chunk_sectors";
pub const SYSFS_QUEUE_ZONE_APPEND_MAX_BYTES: &str = "zone_append_max_bytes";
pub const SYSFS_QUEUE_ZONE_WRITE_GRANULARITY: &str = "zone_write_granularity";
pub const SYSFS_QUEUE_MAX_ACTIVE_ZONES: &str = "max_active_zones";
pub const SYSFS_QUEUE_MAX_OPEN_ZONES: &str = "max_open_zones";

// ---------------------------------------------------------------------------
// Zone-resource bounds (`unsigned int` in the kernel, 0 = unlimited)
// ---------------------------------------------------------------------------

/// Sentinel meaning "no per-device limit on open or active zones".
pub const BLK_ZONE_UNLIMITED: u32 = 0;

/// Default zone-append max size (4 MiB) when the device declines to advertise.
pub const BLK_ZONE_APPEND_DEFAULT_MAX_BYTES: u32 = 4 * 1024 * 1024;

/// Smallest legal zone-write granularity (one logical block: 512 B).
pub const BLK_ZONE_WRITE_GRANULARITY_MIN: u32 = 512;

/// Largest sensible zone-write granularity (matches our 16 KiB page).
pub const BLK_ZONE_WRITE_GRANULARITY_MAX: u32 = 16 * 1024;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_zoned_mode_strings_distinct() {
        let s = [
            SYSFS_ZONED_NONE,
            SYSFS_ZONED_HOST_AWARE,
            SYSFS_ZONED_HOST_MANAGED,
        ];
        for (i, &x) in s.iter().enumerate() {
            for &y in &s[i + 1..] {
                assert_ne!(x, y);
            }
        }
        // The two non-trivial modes share the "host-" prefix.
        assert!(SYSFS_ZONED_HOST_AWARE.starts_with("host-"));
        assert!(SYSFS_ZONED_HOST_MANAGED.starts_with("host-"));
        // "none" is the default for non-zoned devices.
        assert_eq!(SYSFS_ZONED_NONE, "none");
    }

    #[test]
    fn test_sysfs_zone_attr_names_distinct() {
        let a = [
            SYSFS_QUEUE_ZONED,
            SYSFS_QUEUE_NR_ZONES,
            SYSFS_QUEUE_CHUNK_SECTORS,
            SYSFS_QUEUE_ZONE_APPEND_MAX_BYTES,
            SYSFS_QUEUE_ZONE_WRITE_GRANULARITY,
            SYSFS_QUEUE_MAX_ACTIVE_ZONES,
            SYSFS_QUEUE_MAX_OPEN_ZONES,
        ];
        for (i, &x) in a.iter().enumerate() {
            for &y in &a[i + 1..] {
                assert_ne!(x, y);
            }
            assert!(!x.contains('/'));
        }
        // The zone-prefixed attributes form a cluster.
        let zone_prefixed = [
            SYSFS_QUEUE_ZONED,
            SYSFS_QUEUE_ZONE_APPEND_MAX_BYTES,
            SYSFS_QUEUE_ZONE_WRITE_GRANULARITY,
        ];
        for &v in &zone_prefixed {
            assert!(v.starts_with("zone"));
        }
    }

    #[test]
    fn test_active_open_max_attr_pair() {
        // `max_active_zones` and `max_open_zones` are paired limits.
        assert!(SYSFS_QUEUE_MAX_ACTIVE_ZONES.starts_with("max_"));
        assert!(SYSFS_QUEUE_MAX_OPEN_ZONES.starts_with("max_"));
        assert!(SYSFS_QUEUE_MAX_ACTIVE_ZONES.ends_with("_zones"));
        assert!(SYSFS_QUEUE_MAX_OPEN_ZONES.ends_with("_zones"));
    }

    #[test]
    fn test_resource_bounds() {
        // 0 is the "unlimited" sentinel — matches the kernel convention
        // for max_open_zones / max_active_zones.
        assert_eq!(BLK_ZONE_UNLIMITED, 0);
        // 4 MiB default append limit.
        assert_eq!(BLK_ZONE_APPEND_DEFAULT_MAX_BYTES, 4 * 1024 * 1024);
        assert!(BLK_ZONE_APPEND_DEFAULT_MAX_BYTES.is_power_of_two());
    }

    #[test]
    fn test_write_granularity_bounds() {
        assert_eq!(BLK_ZONE_WRITE_GRANULARITY_MIN, 512);
        assert_eq!(BLK_ZONE_WRITE_GRANULARITY_MAX, 16 * 1024);
        assert!(BLK_ZONE_WRITE_GRANULARITY_MIN.is_power_of_two());
        assert!(BLK_ZONE_WRITE_GRANULARITY_MAX.is_power_of_two());
        // 32x span between min and max (512 B .. 16 KiB).
        assert_eq!(
            BLK_ZONE_WRITE_GRANULARITY_MAX / BLK_ZONE_WRITE_GRANULARITY_MIN,
            32
        );
    }
}
