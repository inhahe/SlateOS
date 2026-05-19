//! `<linux/btrfs.h>` — Additional Btrfs constants (part 3).
//!
//! Supplementary Btrfs constants covering snapshot flags,
//! balance filters, and scrub status values.

// ---------------------------------------------------------------------------
// Btrfs snapshot/subvolume flags
// ---------------------------------------------------------------------------

/// Read-only snapshot.
pub const BTRFS_SUBVOL_RDONLY: u64 = 1 << 1;
/// Qgroup inherit.
pub const BTRFS_SUBVOL_QGROUP_INHERIT: u64 = 1 << 2;

// ---------------------------------------------------------------------------
// Btrfs balance flags
// ---------------------------------------------------------------------------

/// Balance data chunks.
pub const BTRFS_BALANCE_DATA: u64 = 1 << 0;
/// Balance system chunks.
pub const BTRFS_BALANCE_SYSTEM: u64 = 1 << 1;
/// Balance metadata chunks.
pub const BTRFS_BALANCE_METADATA: u64 = 1 << 2;
/// Force balance.
pub const BTRFS_BALANCE_FORCE: u64 = 1 << 3;
/// Resume interrupted balance.
pub const BTRFS_BALANCE_RESUME: u64 = 1 << 4;

// ---------------------------------------------------------------------------
// Btrfs balance filter flags
// ---------------------------------------------------------------------------

/// Filter by profiles.
pub const BTRFS_BALANCE_ARGS_PROFILES: u64 = 1 << 0;
/// Filter by usage percent.
pub const BTRFS_BALANCE_ARGS_USAGE: u64 = 1 << 1;
/// Filter by device id.
pub const BTRFS_BALANCE_ARGS_DEVID: u64 = 1 << 2;
/// Filter by physical offset range.
pub const BTRFS_BALANCE_ARGS_DRANGE: u64 = 1 << 3;
/// Filter by virtual address range.
pub const BTRFS_BALANCE_ARGS_VRANGE: u64 = 1 << 4;
/// Limit number of chunks.
pub const BTRFS_BALANCE_ARGS_LIMIT: u64 = 1 << 5;
/// Filter by usage range.
pub const BTRFS_BALANCE_ARGS_USAGE_RANGE: u64 = 1 << 10;
/// Soft filter — convert only.
pub const BTRFS_BALANCE_ARGS_CONVERT: u64 = 1 << 8;
/// Soft mode.
pub const BTRFS_BALANCE_ARGS_SOFT: u64 = 1 << 9;

// ---------------------------------------------------------------------------
// Btrfs scrub status
// ---------------------------------------------------------------------------

/// Scrub started.
pub const BTRFS_SCRUB_STARTED: u32 = 1;
/// Scrub running.
pub const BTRFS_SCRUB_RUNNING: u32 = 2;
/// Scrub paused.
pub const BTRFS_SCRUB_PAUSED: u32 = 3;

// ---------------------------------------------------------------------------
// Btrfs block group types
// ---------------------------------------------------------------------------

/// Data block group.
pub const BTRFS_BLOCK_GROUP_DATA: u64 = 1 << 0;
/// System block group.
pub const BTRFS_BLOCK_GROUP_SYSTEM: u64 = 1 << 1;
/// Metadata block group.
pub const BTRFS_BLOCK_GROUP_METADATA: u64 = 1 << 2;
/// RAID0.
pub const BTRFS_BLOCK_GROUP_RAID0: u64 = 1 << 3;
/// RAID1.
pub const BTRFS_BLOCK_GROUP_RAID1: u64 = 1 << 4;
/// DUP.
pub const BTRFS_BLOCK_GROUP_DUP: u64 = 1 << 5;
/// RAID10.
pub const BTRFS_BLOCK_GROUP_RAID10: u64 = 1 << 6;
/// RAID5.
pub const BTRFS_BLOCK_GROUP_RAID5: u64 = 1 << 7;
/// RAID6.
pub const BTRFS_BLOCK_GROUP_RAID6: u64 = 1 << 8;
/// RAID1C3.
pub const BTRFS_BLOCK_GROUP_RAID1C3: u64 = 1 << 9;
/// RAID1C4.
pub const BTRFS_BLOCK_GROUP_RAID1C4: u64 = 1 << 10;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_subvol_flags_no_overlap() {
        assert_eq!(BTRFS_SUBVOL_RDONLY & BTRFS_SUBVOL_QGROUP_INHERIT, 0);
    }

    #[test]
    fn test_balance_flags_no_overlap() {
        let flags = [
            BTRFS_BALANCE_DATA, BTRFS_BALANCE_SYSTEM,
            BTRFS_BALANCE_METADATA, BTRFS_BALANCE_FORCE,
            BTRFS_BALANCE_RESUME,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_balance_args_no_overlap() {
        let args = [
            BTRFS_BALANCE_ARGS_PROFILES, BTRFS_BALANCE_ARGS_USAGE,
            BTRFS_BALANCE_ARGS_DEVID, BTRFS_BALANCE_ARGS_DRANGE,
            BTRFS_BALANCE_ARGS_VRANGE, BTRFS_BALANCE_ARGS_LIMIT,
            BTRFS_BALANCE_ARGS_CONVERT, BTRFS_BALANCE_ARGS_SOFT,
            BTRFS_BALANCE_ARGS_USAGE_RANGE,
        ];
        for i in 0..args.len() {
            for j in (i + 1)..args.len() {
                assert_eq!(args[i] & args[j], 0);
            }
        }
    }

    #[test]
    fn test_scrub_statuses_distinct() {
        let statuses = [BTRFS_SCRUB_STARTED, BTRFS_SCRUB_RUNNING, BTRFS_SCRUB_PAUSED];
        for i in 0..statuses.len() {
            for j in (i + 1)..statuses.len() {
                assert_ne!(statuses[i], statuses[j]);
            }
        }
    }

    #[test]
    fn test_block_group_types_no_overlap() {
        let types = [
            BTRFS_BLOCK_GROUP_DATA, BTRFS_BLOCK_GROUP_SYSTEM,
            BTRFS_BLOCK_GROUP_METADATA, BTRFS_BLOCK_GROUP_RAID0,
            BTRFS_BLOCK_GROUP_RAID1, BTRFS_BLOCK_GROUP_DUP,
            BTRFS_BLOCK_GROUP_RAID10, BTRFS_BLOCK_GROUP_RAID5,
            BTRFS_BLOCK_GROUP_RAID6, BTRFS_BLOCK_GROUP_RAID1C3,
            BTRFS_BLOCK_GROUP_RAID1C4,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_eq!(types[i] & types[j], 0);
            }
        }
    }
}
