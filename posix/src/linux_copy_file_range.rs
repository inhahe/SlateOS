//! `copy_file_range(2)` — Server-side file copy constants.
//!
//! copy_file_range(2) copies data between files without routing
//! through userspace. It can exploit filesystem-level copy
//! optimizations (reflinks on btrfs/XFS, server-side copy on
//! NFS/CIFS) for near-instantaneous copies.

// ---------------------------------------------------------------------------
// copy_file_range flags
// ---------------------------------------------------------------------------

/// No flags (standard copy).
pub const COPY_FR_DEFAULT: u32 = 0;

// Note: As of Linux 6.x, no flags are currently defined for
// copy_file_range. The flags parameter is reserved for future use.
// Passing any non-zero flag currently returns EINVAL.

// ---------------------------------------------------------------------------
// Limits
// ---------------------------------------------------------------------------

/// Maximum bytes per single copy_file_range call.
/// Same as MAX_RW_COUNT in the kernel (ssize_t max rounded to page).
pub const COPY_FR_MAX_BYTES: u64 = 0x7FFF_F000;

/// Suggested chunk size for iterative copies (1 GiB).
pub const COPY_FR_CHUNK_SIZE: u64 = 1024 * 1024 * 1024;

// ---------------------------------------------------------------------------
// Filesystem support status
// ---------------------------------------------------------------------------

/// Filesystem supports reflink (instant copy-on-write copy).
pub const COPY_FR_REFLINK: u32 = 1;
/// Filesystem supports server-side copy (NFS/CIFS offload).
pub const COPY_FR_SERVER_SIDE: u32 = 2;
/// Filesystem uses generic byte-by-byte fallback.
pub const COPY_FR_GENERIC: u32 = 3;

// ---------------------------------------------------------------------------
// Related ioctl for reflink
// ---------------------------------------------------------------------------

/// FICLONE ioctl number (clone entire file, btrfs/XFS).
pub const FICLONE: u32 = 0x40049409;
/// FICLONERANGE ioctl number (clone a range).
pub const FICLONERANGE: u32 = 0x4020940D;
/// FIDEDUPERANGE ioctl (find and deduplicate ranges).
pub const FIDEDUPERANGE: u32 = 0xC0189436;

// ---------------------------------------------------------------------------
// Dedup status values (from FIDEDUPERANGE result)
// ---------------------------------------------------------------------------

/// Range was successfully deduplicated.
pub const FILE_DEDUPE_RANGE_SAME: u32 = 0;
/// Range differs (cannot deduplicate).
pub const FILE_DEDUPE_RANGE_DIFFERS: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_flags() {
        assert_eq!(COPY_FR_DEFAULT, 0);
    }

    #[test]
    fn test_max_bytes() {
        assert_eq!(COPY_FR_MAX_BYTES, 0x7FFF_F000);
        assert!(COPY_FR_MAX_BYTES > 0);
    }

    #[test]
    fn test_chunk_size() {
        assert_eq!(COPY_FR_CHUNK_SIZE, 1024 * 1024 * 1024);
        assert!(COPY_FR_CHUNK_SIZE <= COPY_FR_MAX_BYTES);
    }

    #[test]
    fn test_support_types_distinct() {
        let types = [COPY_FR_REFLINK, COPY_FR_SERVER_SIDE, COPY_FR_GENERIC];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_ioctls_distinct() {
        let ioctls = [FICLONE, FICLONERANGE, FIDEDUPERANGE];
        for i in 0..ioctls.len() {
            for j in (i + 1)..ioctls.len() {
                assert_ne!(ioctls[i], ioctls[j]);
            }
        }
    }

    #[test]
    fn test_dedupe_status_distinct() {
        assert_ne!(FILE_DEDUPE_RANGE_SAME, FILE_DEDUPE_RANGE_DIFFERS);
    }
}
