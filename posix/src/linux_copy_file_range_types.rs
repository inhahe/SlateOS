//! `<linux/fs.h>` — copy_file_range() and file cloning constants.
//!
//! `copy_file_range()` performs an in-kernel file copy, potentially
//! using filesystem-specific optimizations like reflinks (CoW) or
//! server-side copy on NFS. These constants define flags and related
//! ioctl numbers for file cloning operations.

// ---------------------------------------------------------------------------
// copy_file_range() flags
// ---------------------------------------------------------------------------

/// No flags (default copy behavior).
pub const COPY_FILE_RANGE_FLAGS_NONE: u32 = 0;

// ---------------------------------------------------------------------------
// FICLONE / FICLONERANGE ioctl numbers
// ---------------------------------------------------------------------------

/// Clone entire file (reflink).
pub const FICLONE: u32 = 0x40049409;
/// Clone a range from one file to another.
pub const FICLONERANGE: u32 = 0x4020940D;
/// Deduplicate file ranges.
pub const FIDEDUPERANGE: u32 = 0xC0189436;

// ---------------------------------------------------------------------------
// Dedup extent status flags
// ---------------------------------------------------------------------------

/// Extent was successfully deduplicated.
pub const FILE_DEDUPE_RANGE_SAME: u32 = 0;
/// Extent differs (not deduped).
pub const FILE_DEDUPE_RANGE_DIFFERS: u32 = 1;

// ---------------------------------------------------------------------------
// sendfile() related limits
// ---------------------------------------------------------------------------

/// Maximum single sendfile transfer (2 GiB - 1 page).
pub const SENDFILE_MAX_COUNT: u64 = 0x7FFF_F000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flags_none_is_zero() {
        assert_eq!(COPY_FILE_RANGE_FLAGS_NONE, 0);
    }

    #[test]
    fn test_clone_ioctls_distinct() {
        let ioctls = [FICLONE, FICLONERANGE, FIDEDUPERANGE];
        for i in 0..ioctls.len() {
            for j in (i + 1)..ioctls.len() {
                assert_ne!(ioctls[i], ioctls[j]);
            }
        }
    }

    #[test]
    fn test_dedupe_status() {
        assert_eq!(FILE_DEDUPE_RANGE_SAME, 0);
        assert_eq!(FILE_DEDUPE_RANGE_DIFFERS, 1);
        assert_ne!(FILE_DEDUPE_RANGE_SAME, FILE_DEDUPE_RANGE_DIFFERS);
    }

    #[test]
    fn test_sendfile_max() {
        assert_eq!(SENDFILE_MAX_COUNT, 0x7FFF_F000);
        // Should be less than i64::MAX
        assert!(SENDFILE_MAX_COUNT < i64::MAX as u64);
    }
}
