//! `<linux/shmem_fs.h>` — tmpfs/shmem mount option constants.
//!
//! tmpfs is a memory-backed filesystem used for `/tmp`, `/run`, and
//! shared memory (`/dev/shm`). These constants define mount options
//! that control size limits, huge page policies, and inode behavior.

// ---------------------------------------------------------------------------
// tmpfs mount option flags
// ---------------------------------------------------------------------------

/// Default size limit (half of RAM).
pub const TMPFS_DEFAULT_SIZE_PERCENT: u32 = 50;

/// Maximum tmpfs inode number (2^31 - 1 by default).
pub const TMPFS_MAX_INODES_DEFAULT: u64 = 0x7FFF_FFFF;

// ---------------------------------------------------------------------------
// tmpfs huge page policies (huge= mount option)
// ---------------------------------------------------------------------------

/// Never use huge pages.
pub const SHMEM_HUGE_NEVER: u32 = 0;
/// Always try to use huge pages.
pub const SHMEM_HUGE_ALWAYS: u32 = 1;
/// Use huge pages if VMA is naturally aligned.
pub const SHMEM_HUGE_WITHIN_SIZE: u32 = 2;
/// Use huge pages for madvise regions only.
pub const SHMEM_HUGE_ADVISE: u32 = 3;
/// Deny huge pages (global disable).
pub const SHMEM_HUGE_DENY: u32 = 4;
/// Force huge pages (global enable).
pub const SHMEM_HUGE_FORCE: u32 = 5;

// ---------------------------------------------------------------------------
// tmpfs filesystem magic
// ---------------------------------------------------------------------------

/// tmpfs filesystem magic number.
pub const TMPFS_MAGIC: u64 = 0x01021994;

/// ramfs filesystem magic number.
pub const RAMFS_MAGIC: u64 = 0x858458F6;

/// hugetlbfs filesystem magic number.
pub const HUGETLBFS_MAGIC: u64 = 0x958458F6;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_huge_policies_distinct() {
        let policies = [
            SHMEM_HUGE_NEVER,
            SHMEM_HUGE_ALWAYS,
            SHMEM_HUGE_WITHIN_SIZE,
            SHMEM_HUGE_ADVISE,
            SHMEM_HUGE_DENY,
            SHMEM_HUGE_FORCE,
        ];
        for i in 0..policies.len() {
            for j in (i + 1)..policies.len() {
                assert_ne!(policies[i], policies[j]);
            }
        }
    }

    #[test]
    fn test_huge_never_is_zero() {
        assert_eq!(SHMEM_HUGE_NEVER, 0);
    }

    #[test]
    fn test_default_size_percent() {
        assert_eq!(TMPFS_DEFAULT_SIZE_PERCENT, 50);
    }

    #[test]
    fn test_magic_numbers_distinct() {
        let magics = [TMPFS_MAGIC, RAMFS_MAGIC, HUGETLBFS_MAGIC];
        for i in 0..magics.len() {
            for j in (i + 1)..magics.len() {
                assert_ne!(magics[i], magics[j]);
            }
        }
    }

    #[test]
    fn test_tmpfs_magic() {
        assert_eq!(TMPFS_MAGIC, 0x01021994);
    }
}
