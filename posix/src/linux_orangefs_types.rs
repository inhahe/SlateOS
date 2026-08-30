//! `<linux/orangefs.h>` — OrangeFS (PVFS2) distributed filesystem constants.
//!
//! OrangeFS is a parallel/distributed filesystem (formerly PVFS2).
//! These constants define IOCTL commands, hint keys,
//! and buffer sizes.

// ---------------------------------------------------------------------------
// Magic
// ---------------------------------------------------------------------------

/// OrangeFS super magic.
pub const ORANGEFS_SUPER_MAGIC: u32 = 0x20030528;

// ---------------------------------------------------------------------------
// IOCTL commands
// ---------------------------------------------------------------------------

/// Get extended attributes.
pub const ORANGEFS_IOC_GET_FEATURES: u32 = 0x01;
/// Set extended attributes.
pub const ORANGEFS_IOC_SET_FEATURES: u32 = 0x02;

// ---------------------------------------------------------------------------
// Distribution types
// ---------------------------------------------------------------------------

/// Simple stripe.
pub const ORANGEFS_DIST_SIMPLE_STRIPE: u32 = 0;
/// Variable stripe.
pub const ORANGEFS_DIST_VARSTRIP: u32 = 1;
/// Two-dimensional stripe.
pub const ORANGEFS_DIST_TWOD_STRIPE: u32 = 2;

// ---------------------------------------------------------------------------
// Default parameters
// ---------------------------------------------------------------------------

/// Default stripe size (64 KiB).
pub const ORANGEFS_DEFAULT_STRIPE_SIZE: u32 = 65536;
/// Default number of data files.
pub const ORANGEFS_DEFAULT_NUM_DFILES: u32 = 0;

// ---------------------------------------------------------------------------
// Buffer/name sizes
// ---------------------------------------------------------------------------

/// Max path length.
pub const ORANGEFS_MAX_PATH_LEN: u32 = 4096;
/// Max file name length.
pub const ORANGEFS_MAX_NAME_LEN: u32 = 256;
/// Max server address length.
pub const ORANGEFS_MAX_SERVER_ADDR_LEN: u32 = 256;
/// I/O buffer size.
pub const ORANGEFS_BUFMAP_DEFAULT_DESC_SIZE: u32 = 524288;
/// Max descriptor count.
pub const ORANGEFS_BUFMAP_DEFAULT_DESC_COUNT: u32 = 5;

// ---------------------------------------------------------------------------
// Credential sizes
// ---------------------------------------------------------------------------

/// Max groups.
pub const ORANGEFS_MAX_NUM_GROUPS: u32 = 32;
/// Capability timeout (seconds).
pub const ORANGEFS_DEFAULT_CAPABILITY_TIMEOUT: u32 = 600;

// ---------------------------------------------------------------------------
// Object types
// ---------------------------------------------------------------------------

/// No type.
pub const ORANGEFS_TYPE_NONE: u32 = 0;
/// Metafile.
pub const ORANGEFS_TYPE_METAFILE: u32 = 1 << 0;
/// Datafile.
pub const ORANGEFS_TYPE_DATAFILE: u32 = 1 << 1;
/// Directory.
pub const ORANGEFS_TYPE_DIRECTORY: u32 = 1 << 2;
/// Symlink.
pub const ORANGEFS_TYPE_SYMLINK: u32 = 1 << 3;
/// Dir data (directory hint).
pub const ORANGEFS_TYPE_DIRDATA: u32 = 1 << 4;
/// Internal (system).
pub const ORANGEFS_TYPE_INTERNAL: u32 = 1 << 5;

// ---------------------------------------------------------------------------
// Permission bits
// ---------------------------------------------------------------------------

/// Owner execute.
pub const ORANGEFS_PERM_EXECUTE: u32 = 1;
/// Owner write.
pub const ORANGEFS_PERM_WRITE: u32 = 2;
/// Owner read.
pub const ORANGEFS_PERM_READ: u32 = 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_super_magic() {
        assert_eq!(ORANGEFS_SUPER_MAGIC, 0x20030528);
    }

    #[test]
    fn test_dist_types_sequential() {
        assert_eq!(ORANGEFS_DIST_SIMPLE_STRIPE, 0);
        assert_eq!(ORANGEFS_DIST_VARSTRIP, 1);
        assert_eq!(ORANGEFS_DIST_TWOD_STRIPE, 2);
    }

    #[test]
    fn test_object_types_power_of_two() {
        let types = [
            ORANGEFS_TYPE_METAFILE,
            ORANGEFS_TYPE_DATAFILE,
            ORANGEFS_TYPE_DIRECTORY,
            ORANGEFS_TYPE_SYMLINK,
            ORANGEFS_TYPE_DIRDATA,
            ORANGEFS_TYPE_INTERNAL,
        ];
        for t in &types {
            assert!(t.is_power_of_two(), "{} not power of two", t);
        }
    }

    #[test]
    fn test_object_types_distinct() {
        let types = [
            ORANGEFS_TYPE_NONE,
            ORANGEFS_TYPE_METAFILE,
            ORANGEFS_TYPE_DATAFILE,
            ORANGEFS_TYPE_DIRECTORY,
            ORANGEFS_TYPE_SYMLINK,
            ORANGEFS_TYPE_DIRDATA,
            ORANGEFS_TYPE_INTERNAL,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_permissions() {
        assert_eq!(ORANGEFS_PERM_EXECUTE, 1);
        assert_eq!(ORANGEFS_PERM_WRITE, 2);
        assert_eq!(ORANGEFS_PERM_READ, 4);
    }

    #[test]
    fn test_default_stripe_size() {
        assert_eq!(ORANGEFS_DEFAULT_STRIPE_SIZE, 65536);
        assert!(ORANGEFS_DEFAULT_STRIPE_SIZE.is_power_of_two());
    }

    #[test]
    fn test_bufmap_defaults() {
        assert_eq!(ORANGEFS_BUFMAP_DEFAULT_DESC_SIZE, 512 * 1024);
        assert_eq!(ORANGEFS_BUFMAP_DEFAULT_DESC_COUNT, 5);
    }
}
