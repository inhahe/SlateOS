//! `<linux/dcache.h>` — Dentry (directory entry) cache constants.
//!
//! Dentries cache the mapping from pathname components to inodes.
//! Every path lookup (open, stat, readlink, etc.) traverses the dentry
//! tree. The dcache is one of the most performance-critical data
//! structures in the VFS — a cache miss requires reading the directory
//! from disk. Dentries can be positive (pointing to an inode), negative
//! (caching "file not found"), or in various lifecycle states during
//! lookup, creation, and eviction.

// ---------------------------------------------------------------------------
// Dentry flags
// ---------------------------------------------------------------------------

/// Dentry is hashed (in dcache hash table, reachable by lookup).
pub const DCACHE_ENTRY_HASHED: u32 = 0x0001;
/// Dentry is disconnected (not reachable from root).
pub const DCACHE_DISCONNECTED: u32 = 0x0002;
/// Dentry has been referenced recently (LRU management).
pub const DCACHE_REFERENCED: u32 = 0x0004;
/// Dentry cannot be freed (pinned by mount or open file).
pub const DCACHE_CANT_MOUNT: u32 = 0x0008;
/// Dentry is a mountpoint.
pub const DCACHE_MOUNTED: u32 = 0x0010;
/// Dentry needs lookup revalidation (network FS).
pub const DCACHE_NEED_AUTOMOUNT: u32 = 0x0020;
/// Dentry manages its own inode operations.
pub const DCACHE_MANAGE_TRANSIT: u32 = 0x0040;
/// Dentry is being shrunk (LRU eviction in progress).
pub const DCACHE_SHRINK_LIST: u32 = 0x0080;
/// Dentry has child dentries that need fsnotify.
pub const DCACHE_FSNOTIFY_PARENT: u32 = 0x0100;
/// Dentry lookup is case-insensitive.
pub const DCACHE_CASEFOLD: u32 = 0x0200;

// ---------------------------------------------------------------------------
// Dentry types
// ---------------------------------------------------------------------------

/// Regular dentry (positive, has inode).
pub const DENTRY_TYPE_POSITIVE: u32 = 0;
/// Negative dentry (caches "not found" result).
pub const DENTRY_TYPE_NEGATIVE: u32 = 1;
/// Whiteout dentry (overlayfs: masks lower layer).
pub const DENTRY_TYPE_WHITEOUT: u32 = 2;

// ---------------------------------------------------------------------------
// Dentry operations result codes
// ---------------------------------------------------------------------------

/// Dentry is valid (revalidation passed).
pub const DENTRY_VALID: i32 = 1;
/// Dentry is invalid (needs re-lookup).
pub const DENTRY_INVALID: i32 = 0;
/// Dentry revalidation error.
pub const DENTRY_ERROR: i32 = -1;

// ---------------------------------------------------------------------------
// dcache hash parameters
// ---------------------------------------------------------------------------

/// dcache hash table name length limit.
pub const DNAME_INLINE_LEN: u32 = 32;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flags_no_overlap() {
        let flags = [
            DCACHE_ENTRY_HASHED, DCACHE_DISCONNECTED, DCACHE_REFERENCED,
            DCACHE_CANT_MOUNT, DCACHE_MOUNTED, DCACHE_NEED_AUTOMOUNT,
            DCACHE_MANAGE_TRANSIT, DCACHE_SHRINK_LIST,
            DCACHE_FSNOTIFY_PARENT, DCACHE_CASEFOLD,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_dentry_types_distinct() {
        let types = [
            DENTRY_TYPE_POSITIVE, DENTRY_TYPE_NEGATIVE,
            DENTRY_TYPE_WHITEOUT,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_validation_values() {
        assert!(DENTRY_VALID > 0);
        assert_eq!(DENTRY_INVALID, 0);
        assert!(DENTRY_ERROR < 0);
    }

    #[test]
    fn test_name_limit() {
        assert!(DNAME_INLINE_LEN > 0);
    }
}
