//! `<linux/userfaultfd.h>` — Userfaultfd feature and ioctl constants.
//!
//! Userfaultfd allows userspace to handle page faults for a range
//! of virtual memory. This is used for post-copy live migration,
//! garbage collectors, and checkpoint/restore (CRIU).

// ---------------------------------------------------------------------------
// Userfaultfd ioctl commands
// ---------------------------------------------------------------------------

/// Register a memory range for fault handling.
pub const UFFDIO_REGISTER: u64 = 0xC020_AA00;
/// Unregister a memory range.
pub const UFFDIO_UNREGISTER: u64 = 0x8010_AA01;
/// Resolve a page fault by copying data.
pub const UFFDIO_COPY: u64 = 0xC028_AA03;
/// Resolve a page fault by zeroing.
pub const UFFDIO_ZEROPAGE: u64 = 0xC020_AA04;
/// Wake up waiting threads.
pub const UFFDIO_WAKE: u64 = 0x8010_AA02;
/// Write-protect pages.
pub const UFFDIO_WRITEPROTECT: u64 = 0xC018_AA06;
/// Continue a page (for minor faults).
pub const UFFDIO_CONTINUE: u64 = 0xC018_AA07;
/// Poison a page (mark as failed).
pub const UFFDIO_POISON: u64 = 0xC018_AA08;

// ---------------------------------------------------------------------------
// Userfaultfd feature flags
// ---------------------------------------------------------------------------

/// Support pagemap scanning.
pub const UFFD_FEATURE_PAGEFAULT_FLAG_WP: u64 = 1 << 0;
/// Support fork event notification.
pub const UFFD_FEATURE_EVENT_FORK: u64 = 1 << 1;
/// Support remap event notification.
pub const UFFD_FEATURE_EVENT_REMAP: u64 = 1 << 2;
/// Support remove event notification.
pub const UFFD_FEATURE_EVENT_REMOVE: u64 = 1 << 3;
/// Support unmap event notification.
pub const UFFD_FEATURE_EVENT_UNMAP: u64 = 1 << 4;
/// Support minor fault handling.
pub const UFFD_FEATURE_MINOR_HUGETLBFS: u64 = 1 << 5;
/// Support minor fault for shmem.
pub const UFFD_FEATURE_MINOR_SHMEM: u64 = 1 << 6;
/// Support exact address in fault messages.
pub const UFFD_FEATURE_EXACT_ADDRESS: u64 = 1 << 7;
/// Support WP on hugetlbfs.
pub const UFFD_FEATURE_WP_HUGETLBFS_SHMEM: u64 = 1 << 8;
/// Support WP on unpopulated PTEs.
pub const UFFD_FEATURE_WP_UNPOPULATED: u64 = 1 << 9;
/// Support poison pages.
pub const UFFD_FEATURE_POISON: u64 = 1 << 10;

// ---------------------------------------------------------------------------
// Register mode flags
// ---------------------------------------------------------------------------

/// Register for missing page faults.
pub const UFFDIO_REGISTER_MODE_MISSING: u64 = 1 << 0;
/// Register for write-protect faults.
pub const UFFDIO_REGISTER_MODE_WP: u64 = 1 << 1;
/// Register for minor faults.
pub const UFFDIO_REGISTER_MODE_MINOR: u64 = 1 << 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ioctls_distinct() {
        let ioctls = [
            UFFDIO_REGISTER,
            UFFDIO_UNREGISTER,
            UFFDIO_COPY,
            UFFDIO_ZEROPAGE,
            UFFDIO_WAKE,
            UFFDIO_WRITEPROTECT,
            UFFDIO_CONTINUE,
            UFFDIO_POISON,
        ];
        for i in 0..ioctls.len() {
            for j in (i + 1)..ioctls.len() {
                assert_ne!(ioctls[i], ioctls[j]);
            }
        }
    }

    #[test]
    fn test_features_no_overlap() {
        let feats = [
            UFFD_FEATURE_PAGEFAULT_FLAG_WP,
            UFFD_FEATURE_EVENT_FORK,
            UFFD_FEATURE_EVENT_REMAP,
            UFFD_FEATURE_EVENT_REMOVE,
            UFFD_FEATURE_EVENT_UNMAP,
            UFFD_FEATURE_MINOR_HUGETLBFS,
            UFFD_FEATURE_MINOR_SHMEM,
            UFFD_FEATURE_EXACT_ADDRESS,
            UFFD_FEATURE_WP_HUGETLBFS_SHMEM,
            UFFD_FEATURE_WP_UNPOPULATED,
            UFFD_FEATURE_POISON,
        ];
        for i in 0..feats.len() {
            for j in (i + 1)..feats.len() {
                assert_eq!(feats[i] & feats[j], 0);
            }
        }
    }

    #[test]
    fn test_features_power_of_two() {
        let feats = [
            UFFD_FEATURE_PAGEFAULT_FLAG_WP,
            UFFD_FEATURE_EVENT_FORK,
            UFFD_FEATURE_EVENT_REMAP,
            UFFD_FEATURE_EVENT_REMOVE,
            UFFD_FEATURE_EVENT_UNMAP,
            UFFD_FEATURE_MINOR_HUGETLBFS,
            UFFD_FEATURE_MINOR_SHMEM,
            UFFD_FEATURE_EXACT_ADDRESS,
            UFFD_FEATURE_WP_HUGETLBFS_SHMEM,
            UFFD_FEATURE_WP_UNPOPULATED,
            UFFD_FEATURE_POISON,
        ];
        for f in &feats {
            assert!(f.is_power_of_two());
        }
    }

    #[test]
    fn test_register_modes_no_overlap() {
        let modes = [
            UFFDIO_REGISTER_MODE_MISSING,
            UFFDIO_REGISTER_MODE_WP,
            UFFDIO_REGISTER_MODE_MINOR,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_eq!(modes[i] & modes[j], 0);
            }
        }
    }
}
