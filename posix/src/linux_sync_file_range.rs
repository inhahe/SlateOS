//! `sync_file_range(2)` — Fine-grained file sync constants.
//!
//! sync_file_range(2) allows fine-grained control over syncing
//! file data to disk. Unlike fsync/fdatasync which sync the
//! entire file, this allows specifying which byte range to sync
//! and whether to wait for completion.

// ---------------------------------------------------------------------------
// sync_file_range flags
// ---------------------------------------------------------------------------

/// Wait for writeout of already-submitted pages in the range.
pub const SYNC_FILE_RANGE_WAIT_BEFORE: u32 = 1;

/// Start writeout of dirty pages in the range.
pub const SYNC_FILE_RANGE_WRITE: u32 = 2;

/// Wait for writeout of pages submitted by this call.
pub const SYNC_FILE_RANGE_WAIT_AFTER: u32 = 4;

/// Combined: equivalent to fdatasync behavior on range.
pub const SYNC_FILE_RANGE_WRITE_AND_WAIT: u32 =
    SYNC_FILE_RANGE_WAIT_BEFORE | SYNC_FILE_RANGE_WRITE | SYNC_FILE_RANGE_WAIT_AFTER;

// ---------------------------------------------------------------------------
// sync_file_range2 (ARM variant — flag argument position differs)
// ---------------------------------------------------------------------------

/// sync_file_range2 is functionally identical but has different
/// argument order on ARM due to ABI constraints.
pub const SYNC_FILE_RANGE2_WAIT_BEFORE: u32 = SYNC_FILE_RANGE_WAIT_BEFORE;
pub const SYNC_FILE_RANGE2_WRITE: u32 = SYNC_FILE_RANGE_WRITE;
pub const SYNC_FILE_RANGE2_WAIT_AFTER: u32 = SYNC_FILE_RANGE_WAIT_AFTER;

// ---------------------------------------------------------------------------
// Related sync operations
// ---------------------------------------------------------------------------

/// syncfs(2) — sync all data on filesystem containing fd.
pub const SYNCFS_OP: u32 = 0;

/// fsync(2) — sync file data + metadata.
pub const FSYNC_OP: u32 = 1;

/// fdatasync(2) — sync file data only.
pub const FDATASYNC_OP: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flags_powers_of_two() {
        let flags = [
            SYNC_FILE_RANGE_WAIT_BEFORE,
            SYNC_FILE_RANGE_WRITE,
            SYNC_FILE_RANGE_WAIT_AFTER,
        ];
        for flag in &flags {
            assert!(flag.is_power_of_two(), "0x{:x}", flag);
        }
    }

    #[test]
    fn test_flags_no_overlap() {
        let flags = [
            SYNC_FILE_RANGE_WAIT_BEFORE,
            SYNC_FILE_RANGE_WRITE,
            SYNC_FILE_RANGE_WAIT_AFTER,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_write_and_wait() {
        assert_eq!(SYNC_FILE_RANGE_WRITE_AND_WAIT, 7);
        assert_eq!(
            SYNC_FILE_RANGE_WRITE_AND_WAIT,
            SYNC_FILE_RANGE_WAIT_BEFORE | SYNC_FILE_RANGE_WRITE | SYNC_FILE_RANGE_WAIT_AFTER
        );
    }

    #[test]
    fn test_range2_matches_range() {
        assert_eq!(SYNC_FILE_RANGE2_WAIT_BEFORE, SYNC_FILE_RANGE_WAIT_BEFORE);
        assert_eq!(SYNC_FILE_RANGE2_WRITE, SYNC_FILE_RANGE_WRITE);
        assert_eq!(SYNC_FILE_RANGE2_WAIT_AFTER, SYNC_FILE_RANGE_WAIT_AFTER);
    }

    #[test]
    fn test_sync_ops_distinct() {
        let ops = [SYNCFS_OP, FSYNC_OP, FDATASYNC_OP];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
    }
}
