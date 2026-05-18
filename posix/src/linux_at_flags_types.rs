//! `<fcntl.h>` — AT_* flag constants for *at() system calls.
//!
//! The *at() family of syscalls (openat, fstatat, linkat, etc.)
//! operate relative to a directory fd. These flags modify their
//! behavior, particularly around symlink handling and special
//! fd values.

// ---------------------------------------------------------------------------
// Special directory fd values
// ---------------------------------------------------------------------------

/// Use current working directory as base for relative paths.
pub const AT_FDCWD: i32 = -100;

// ---------------------------------------------------------------------------
// AT_* flags
// ---------------------------------------------------------------------------

/// Don't follow symbolic links (for fstatat, fchownat, etc.).
pub const AT_SYMLINK_NOFOLLOW: u32 = 0x100;
/// Follow symbolic links (for linkat).
pub const AT_SYMLINK_FOLLOW: u32 = 0x400;
/// Remove directory (for unlinkat).
pub const AT_REMOVEDIR: u32 = 0x200;
/// Suppress terminal automount traversal.
pub const AT_NO_AUTOMOUNT: u32 = 0x800;
/// Allow empty relative path (operate on dirfd itself).
pub const AT_EMPTY_PATH: u32 = 0x1000;
/// Apply to the link itself rather than the target.
pub const AT_EACCESS: u32 = 0x200;
/// Statx: force sync with backing store.
pub const AT_STATX_FORCE_SYNC: u32 = 0x2000;
/// Statx: don't sync with backing store.
pub const AT_STATX_DONT_SYNC: u32 = 0x4000;
/// Type mask for statx sync modes.
pub const AT_STATX_SYNC_TYPE: u32 = 0x6000;
/// Statx: synchronize as needed.
pub const AT_STATX_SYNC_AS_STAT: u32 = 0x0000;
/// Recursive: handle mounts recursively.
pub const AT_RECURSIVE: u32 = 0x8000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fdcwd() {
        assert_eq!(AT_FDCWD, -100);
    }

    #[test]
    fn test_symlink_flags_distinct() {
        assert_ne!(AT_SYMLINK_NOFOLLOW, AT_SYMLINK_FOLLOW);
    }

    #[test]
    fn test_key_flags_nonzero() {
        assert_ne!(AT_SYMLINK_NOFOLLOW, 0);
        assert_ne!(AT_SYMLINK_FOLLOW, 0);
        assert_ne!(AT_REMOVEDIR, 0);
        assert_ne!(AT_EMPTY_PATH, 0);
        assert_ne!(AT_NO_AUTOMOUNT, 0);
    }

    #[test]
    fn test_statx_sync_types() {
        assert_eq!(AT_STATX_SYNC_AS_STAT, 0);
        assert_ne!(AT_STATX_FORCE_SYNC, AT_STATX_DONT_SYNC);
    }

    #[test]
    fn test_statx_sync_mask() {
        assert_eq!(
            AT_STATX_SYNC_TYPE,
            AT_STATX_FORCE_SYNC | AT_STATX_DONT_SYNC
        );
    }

    #[test]
    fn test_recursive() {
        assert_eq!(AT_RECURSIVE, 0x8000);
    }
}
