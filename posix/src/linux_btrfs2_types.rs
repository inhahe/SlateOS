//! `<linux/btrfs.h>` — Additional Btrfs filesystem constants.
//!
//! Supplementary Btrfs constants covering snapshot flags,
//! defrag range flags, balance filter types, and
//! scrub/device operations.

// ---------------------------------------------------------------------------
// Snapshot flags (BTRFS_SUBVOL_*)
// ---------------------------------------------------------------------------

/// Create as read-only snapshot.
pub const BTRFS_SUBVOL_RDONLY: u64 = 1 << 1;
/// Quota groups inherit.
pub const BTRFS_SUBVOL_QGROUP_INHERIT: u64 = 1 << 2;

// ---------------------------------------------------------------------------
// Defrag range flags (BTRFS_DEFRAG_RANGE_*)
// ---------------------------------------------------------------------------

/// Compress during defrag.
pub const BTRFS_DEFRAG_RANGE_COMPRESS: u32 = 1;
/// Start defrag synchronously.
pub const BTRFS_DEFRAG_RANGE_START_IO: u32 = 2;

// ---------------------------------------------------------------------------
// Balance filter types (BTRFS_BALANCE_*)
// ---------------------------------------------------------------------------

/// Balance data chunks.
pub const BTRFS_BALANCE_DATA: u32 = 1 << 0;
/// Balance system chunks.
pub const BTRFS_BALANCE_SYSTEM: u32 = 1 << 1;
/// Balance metadata chunks.
pub const BTRFS_BALANCE_METADATA: u32 = 1 << 2;
/// Force balance (degraded).
pub const BTRFS_BALANCE_FORCE: u32 = 1 << 3;
/// Resume balance.
pub const BTRFS_BALANCE_RESUME: u32 = 1 << 4;

// ---------------------------------------------------------------------------
// Balance filter args (BTRFS_BALANCE_ARGS_*)
// ---------------------------------------------------------------------------

/// Filter by profile.
pub const BTRFS_BALANCE_ARGS_PROFILES: u64 = 1 << 0;
/// Filter by usage percent.
pub const BTRFS_BALANCE_ARGS_USAGE: u64 = 1 << 1;
/// Filter by devid.
pub const BTRFS_BALANCE_ARGS_DEVID: u64 = 1 << 2;
/// Filter by drange.
pub const BTRFS_BALANCE_ARGS_DRANGE: u64 = 1 << 3;
/// Filter by vrange.
pub const BTRFS_BALANCE_ARGS_VRANGE: u64 = 1 << 4;
/// Convert to profile.
pub const BTRFS_BALANCE_ARGS_CONVERT: u64 = 1 << 8;
/// Soft convert.
pub const BTRFS_BALANCE_ARGS_SOFT: u64 = 1 << 9;
/// Filter by usage range.
pub const BTRFS_BALANCE_ARGS_USAGE_RANGE: u64 = 1 << 10;
/// Limit number of chunks.
pub const BTRFS_BALANCE_ARGS_LIMIT: u64 = 1 << 5;
/// Limit range of chunks.
pub const BTRFS_BALANCE_ARGS_LIMIT_RANGE: u64 = 1 << 6;
/// Filter by stripe range.
pub const BTRFS_BALANCE_ARGS_STRIPES_RANGE: u64 = 1 << 7;

// ---------------------------------------------------------------------------
// Balance state (BTRFS_BALANCE_STATE_*)
// ---------------------------------------------------------------------------

/// Balance is running.
pub const BTRFS_BALANCE_STATE_RUNNING: u32 = 1 << 0;
/// Balance is paused.
pub const BTRFS_BALANCE_STATE_PAUSE_REQ: u32 = 1 << 1;
/// Balance cancel requested.
pub const BTRFS_BALANCE_STATE_CANCEL_REQ: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// Scrub flags (BTRFS_SCRUB_*)
// ---------------------------------------------------------------------------

/// Read-only scrub (no repair).
pub const BTRFS_SCRUB_READONLY: u32 = 1;

// ---------------------------------------------------------------------------
// Device replace state (BTRFS_IOCTL_DEV_REPLACE_STATE_*)
// ---------------------------------------------------------------------------

/// Replace never started.
pub const BTRFS_IOCTL_DEV_REPLACE_STATE_NEVER_STARTED: u32 = 0;
/// Replace is running.
pub const BTRFS_IOCTL_DEV_REPLACE_STATE_STARTED: u32 = 1;
/// Replace finished.
pub const BTRFS_IOCTL_DEV_REPLACE_STATE_FINISHED: u32 = 2;
/// Replace canceled.
pub const BTRFS_IOCTL_DEV_REPLACE_STATE_CANCELED: u32 = 3;
/// Replace suspended.
pub const BTRFS_IOCTL_DEV_REPLACE_STATE_SUSPENDED: u32 = 4;

// ---------------------------------------------------------------------------
// Send flags (BTRFS_SEND_FLAG_*)
// ---------------------------------------------------------------------------

/// No file data (metadata only).
pub const BTRFS_SEND_FLAG_NO_FILE_DATA: u32 = 1;
/// Omit UUIDs.
pub const BTRFS_SEND_FLAG_OMIT_STREAM_HEADER: u32 = 2;
/// Omit end command.
pub const BTRFS_SEND_FLAG_OMIT_END_CMD: u32 = 4;
/// Send v2 format.
pub const BTRFS_SEND_FLAG_VERSION: u32 = 8;
/// Compressed data.
pub const BTRFS_SEND_FLAG_COMPRESSED: u32 = 16;

// ---------------------------------------------------------------------------
// Compression types (BTRFS_COMPRESS_*)
// ---------------------------------------------------------------------------

/// No compression.
pub const BTRFS_COMPRESS_NONE: u32 = 0;
/// Zlib compression.
pub const BTRFS_COMPRESS_ZLIB: u32 = 1;
/// LZO compression.
pub const BTRFS_COMPRESS_LZO: u32 = 2;
/// Zstd compression.
pub const BTRFS_COMPRESS_ZSTD: u32 = 3;

// ---------------------------------------------------------------------------
// Tree search (BTRFS_SEARCH_*)
// ---------------------------------------------------------------------------

/// Max search tree depth.
pub const BTRFS_SEARCH_ARGS_BUFSIZE: u32 = 4096 - 104;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_subvol_flags_distinct() {
        assert_ne!(BTRFS_SUBVOL_RDONLY, BTRFS_SUBVOL_QGROUP_INHERIT);
    }

    #[test]
    fn test_defrag_flags_distinct() {
        assert_ne!(BTRFS_DEFRAG_RANGE_COMPRESS, BTRFS_DEFRAG_RANGE_START_IO);
    }

    #[test]
    fn test_balance_types_power_of_two() {
        let types = [
            BTRFS_BALANCE_DATA,
            BTRFS_BALANCE_SYSTEM,
            BTRFS_BALANCE_METADATA,
            BTRFS_BALANCE_FORCE,
            BTRFS_BALANCE_RESUME,
        ];
        for t in &types {
            assert!(t.is_power_of_two(), "{} not power of two", t);
        }
    }

    #[test]
    fn test_balance_args_distinct() {
        let args: [u64; 11] = [
            BTRFS_BALANCE_ARGS_PROFILES,
            BTRFS_BALANCE_ARGS_USAGE,
            BTRFS_BALANCE_ARGS_DEVID,
            BTRFS_BALANCE_ARGS_DRANGE,
            BTRFS_BALANCE_ARGS_VRANGE,
            BTRFS_BALANCE_ARGS_CONVERT,
            BTRFS_BALANCE_ARGS_SOFT,
            BTRFS_BALANCE_ARGS_USAGE_RANGE,
            BTRFS_BALANCE_ARGS_LIMIT,
            BTRFS_BALANCE_ARGS_LIMIT_RANGE,
            BTRFS_BALANCE_ARGS_STRIPES_RANGE,
        ];
        for i in 0..args.len() {
            for j in (i + 1)..args.len() {
                assert_ne!(args[i], args[j]);
            }
        }
    }

    #[test]
    fn test_balance_state_power_of_two() {
        let states = [
            BTRFS_BALANCE_STATE_RUNNING,
            BTRFS_BALANCE_STATE_PAUSE_REQ,
            BTRFS_BALANCE_STATE_CANCEL_REQ,
        ];
        for s in &states {
            assert!(s.is_power_of_two(), "{} not power of two", s);
        }
    }

    #[test]
    fn test_dev_replace_states_sequential() {
        assert_eq!(BTRFS_IOCTL_DEV_REPLACE_STATE_NEVER_STARTED, 0);
        assert_eq!(BTRFS_IOCTL_DEV_REPLACE_STATE_STARTED, 1);
        assert_eq!(BTRFS_IOCTL_DEV_REPLACE_STATE_FINISHED, 2);
        assert_eq!(BTRFS_IOCTL_DEV_REPLACE_STATE_CANCELED, 3);
        assert_eq!(BTRFS_IOCTL_DEV_REPLACE_STATE_SUSPENDED, 4);
    }

    #[test]
    fn test_send_flags_power_of_two() {
        let flags = [
            BTRFS_SEND_FLAG_NO_FILE_DATA,
            BTRFS_SEND_FLAG_OMIT_STREAM_HEADER,
            BTRFS_SEND_FLAG_OMIT_END_CMD,
            BTRFS_SEND_FLAG_VERSION,
            BTRFS_SEND_FLAG_COMPRESSED,
        ];
        for f in &flags {
            assert!(f.is_power_of_two(), "{} not power of two", f);
        }
    }

    #[test]
    fn test_compress_types_sequential() {
        assert_eq!(BTRFS_COMPRESS_NONE, 0);
        assert_eq!(BTRFS_COMPRESS_ZLIB, 1);
        assert_eq!(BTRFS_COMPRESS_LZO, 2);
        assert_eq!(BTRFS_COMPRESS_ZSTD, 3);
    }

    #[test]
    fn test_search_bufsize() {
        assert_eq!(BTRFS_SEARCH_ARGS_BUFSIZE, 3992);
    }

    #[test]
    fn test_scrub_readonly() {
        assert_eq!(BTRFS_SCRUB_READONLY, 1);
    }
}
