//! `<linux/bcache.h>` continuation — superblock magic and journal
//! constants used by the `bcache` block-cache driver.
//!
//! bcache stores a backing-device set + one or more cache devices,
//! each labeled with an on-disk superblock at LBA 1 (8 KiB offset).
//! These constants are the wire format that `bcache-tools` writes.

// ---------------------------------------------------------------------------
// Superblock magic and offset
// ---------------------------------------------------------------------------

/// First 8 bytes of the bcache superblock (`SB_MAGIC` lower half).
pub const BCACHE_SB_MAGIC_LO: u64 = 0xf67385c6_1a4e0688;
/// Last 8 bytes (`SB_MAGIC` upper half).
pub const BCACHE_SB_MAGIC_HI: u64 = 0x4dd0cd60_70fae429;

/// Superblock location: 4096-byte offset into the device.
pub const BCACHE_SB_OFFSET: u64 = 4096;
/// Superblock structure size (fits in one 4 KiB sector).
pub const BCACHE_SB_SIZE: usize = 4096;
/// Number of 512-byte sectors covered by the superblock.
pub const BCACHE_SB_SECTORS: u32 = 8;

// ---------------------------------------------------------------------------
// Superblock version numbers
// ---------------------------------------------------------------------------

pub const BCACHE_SB_VERSION_CDEV: u32 = 0;
pub const BCACHE_SB_VERSION_BDEV: u32 = 1;
pub const BCACHE_SB_VERSION_CDEV_WITH_UUID: u32 = 3;
pub const BCACHE_SB_VERSION_BDEV_WITH_OFFSET: u32 = 4;
pub const BCACHE_SB_MAX_VERSION: u32 = 4;

// ---------------------------------------------------------------------------
// Cache mode (`bcache_super.cache_mode`)
// ---------------------------------------------------------------------------

pub const BCACHE_CACHE_MODE_WRITETHROUGH: u8 = 0;
pub const BCACHE_CACHE_MODE_WRITEBACK: u8 = 1;
pub const BCACHE_CACHE_MODE_WRITEAROUND: u8 = 2;
pub const BCACHE_CACHE_MODE_NONE: u8 = 3;

// ---------------------------------------------------------------------------
// Journal block addressing
// ---------------------------------------------------------------------------

/// Journal block magic (`bcache_jset.magic`).
pub const BCACHE_JSET_MAGIC: u64 = 0x245235c1a3625032;
/// Journal entry header size (bytes).
pub const BCACHE_JSET_HDR_SIZE: usize = 32;
/// Maximum journal entry size (one bucket = 1 MiB).
pub const BCACHE_JSET_MAX_SIZE: usize = 1 << 20;

// ---------------------------------------------------------------------------
// Bucket priority constants
// ---------------------------------------------------------------------------

/// Priority value for "metadata bucket — never evict".
pub const BCACHE_PRIO_METADATA: u16 = 0xFFFF;
/// Initial priority for a freshly populated bucket.
pub const BCACHE_INITIAL_PRIO: u16 = 32_768;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_superblock_geometry() {
        assert_eq!(BCACHE_SB_OFFSET, 4096);
        assert_eq!(BCACHE_SB_SIZE, 4096);
        // Superblock occupies sectors 8..16 (4096..8192) → 8 sectors.
        assert_eq!(BCACHE_SB_SECTORS, 8);
        // 8 sectors of 512 bytes = 4 KiB.
        assert_eq!(BCACHE_SB_SECTORS as usize * 512, BCACHE_SB_SIZE);
    }

    #[test]
    fn test_superblock_magic_distinct_halves() {
        // The 16-byte magic is split into two distinct 64-bit halves.
        assert_ne!(BCACHE_SB_MAGIC_LO, BCACHE_SB_MAGIC_HI);
        // Neither half is zero or all-ones (would indicate a wiped sector).
        assert_ne!(BCACHE_SB_MAGIC_LO, 0);
        assert_ne!(BCACHE_SB_MAGIC_HI, 0);
        assert_ne!(BCACHE_SB_MAGIC_LO, u64::MAX);
        assert_ne!(BCACHE_SB_MAGIC_HI, u64::MAX);
    }

    #[test]
    fn test_version_numbers_ordered() {
        // Versions form an ordered sequence; v2 is reserved (skipped).
        assert!(BCACHE_SB_VERSION_CDEV < BCACHE_SB_VERSION_BDEV);
        assert!(
            BCACHE_SB_VERSION_BDEV < BCACHE_SB_VERSION_CDEV_WITH_UUID
        );
        assert!(
            BCACHE_SB_VERSION_CDEV_WITH_UUID
                < BCACHE_SB_VERSION_BDEV_WITH_OFFSET
        );
        assert_eq!(
            BCACHE_SB_MAX_VERSION,
            BCACHE_SB_VERSION_BDEV_WITH_OFFSET
        );
        // Reserved version 2 left a gap.
        assert_eq!(BCACHE_SB_VERSION_CDEV_WITH_UUID - BCACHE_SB_VERSION_BDEV, 2);
    }

    #[test]
    fn test_cache_modes_dense_0_to_3() {
        let m = [
            BCACHE_CACHE_MODE_WRITETHROUGH,
            BCACHE_CACHE_MODE_WRITEBACK,
            BCACHE_CACHE_MODE_WRITEAROUND,
            BCACHE_CACHE_MODE_NONE,
        ];
        for (i, &v) in m.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_journal_constants() {
        // Journal magic is distinct from superblock magic halves.
        assert_ne!(BCACHE_JSET_MAGIC, BCACHE_SB_MAGIC_LO);
        assert_ne!(BCACHE_JSET_MAGIC, BCACHE_SB_MAGIC_HI);
        assert_eq!(BCACHE_JSET_HDR_SIZE, 32);
        // Max journal entry is one bucket = 1 MiB.
        assert_eq!(BCACHE_JSET_MAX_SIZE, 1 << 20);
        assert!(BCACHE_JSET_MAX_SIZE.is_power_of_two());
    }

    #[test]
    fn test_priority_bounds() {
        // Metadata is pinned at the max 16-bit value.
        assert_eq!(BCACHE_PRIO_METADATA, u16::MAX);
        // Initial priority sits at the midpoint of the 16-bit range.
        assert_eq!(BCACHE_INITIAL_PRIO, 1 << 15);
        assert!(BCACHE_INITIAL_PRIO < BCACHE_PRIO_METADATA);
    }
}
