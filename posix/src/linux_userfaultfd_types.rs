//! `<linux/userfaultfd.h>` — Userfaultfd constants.
//!
//! Userfaultfd allows userspace to handle page faults in user memory.
//! A monitor process/thread reads fault events from the userfaultfd
//! descriptor and resolves them by copying page data or mapping zero
//! pages. Used for live migration, post-copy migration, CRIU
//! checkpoint/restore, and userspace-managed memory (e.g., garbage
//! collectors).

// ---------------------------------------------------------------------------
// userfaultfd ioctl commands
// ---------------------------------------------------------------------------

/// Initialize and handshake with userfaultfd.
pub const UFFDIO_API: u32 = 0xC018_AA3F;
/// Register a memory range for fault handling.
pub const UFFDIO_REGISTER: u32 = 0xC020_AA00;
/// Unregister a memory range.
pub const UFFDIO_UNREGISTER: u32 = 0x8010_AA01;
/// Copy page data to resolve a fault.
pub const UFFDIO_COPY: u32 = 0xC028_AA03;
/// Zero-fill a page to resolve a fault.
pub const UFFDIO_ZEROPAGE: u32 = 0xC020_AA04;
/// Wake threads waiting on a resolved range.
pub const UFFDIO_WAKE: u32 = 0x8010_AA02;
/// Write-protect pages (soft-dirty tracking).
pub const UFFDIO_WRITEPROTECT: u32 = 0xC018_AA06;
/// Continue a page (minor fault resolution, no data copy).
pub const UFFDIO_CONTINUE: u32 = 0xC020_AA07;
/// Poison pages (simulate memory errors).
pub const UFFDIO_POISON: u32 = 0xC020_AA08;

// ---------------------------------------------------------------------------
// userfaultfd feature flags (UFFDIO_API handshake)
// ---------------------------------------------------------------------------

/// Page-fault events reported.
pub const UFFD_FEATURE_PAGEFAULT_FLAG_WP: u64 = 1 << 0;
/// fork() event delivered to monitor.
pub const UFFD_FEATURE_EVENT_FORK: u64 = 1 << 1;
/// remap (mremap) event.
pub const UFFD_FEATURE_EVENT_REMAP: u64 = 1 << 2;
/// madvise(DONTNEED)/fallocate(PUNCH_HOLE) remove event.
pub const UFFD_FEATURE_EVENT_REMOVE: u64 = 1 << 3;
/// Missing fault on shmem/hugetlb.
pub const UFFD_FEATURE_MISSING_SHMEM: u64 = 1 << 4;
/// Missing fault on hugetlbfs.
pub const UFFD_FEATURE_MISSING_HUGETLBFS: u64 = 1 << 5;
/// unmap event.
pub const UFFD_FEATURE_EVENT_UNMAP: u64 = 1 << 6;
/// Signal-based wakeup for thread safety.
pub const UFFD_FEATURE_SIGBUS: u64 = 1 << 7;
/// Thread-ID in page fault messages.
pub const UFFD_FEATURE_THREAD_ID: u64 = 1 << 8;
/// Minor faults on shmem.
pub const UFFD_FEATURE_MINOR_SHMEM: u64 = 1 << 10;
/// Minor faults on hugetlbfs.
pub const UFFD_FEATURE_MINOR_HUGETLBFS: u64 = 1 << 9;
/// Exact virtual address in fault message.
pub const UFFD_FEATURE_EXACT_ADDRESS: u64 = 1 << 11;
/// Write-protect on userfaultfd unpopulated pages.
pub const UFFD_FEATURE_WP_HUGETLBFS_SHMEM: u64 = 1 << 12;
/// Unprivileged userfaultfd.
pub const UFFD_FEATURE_WP_UNPOPULATED: u64 = 1 << 13;
/// Poison page support.
pub const UFFD_FEATURE_POISON: u64 = 1 << 14;

// ---------------------------------------------------------------------------
// Register mode flags (UFFDIO_REGISTER)
// ---------------------------------------------------------------------------

/// Register for missing-page faults.
pub const UFFDIO_REGISTER_MODE_MISSING: u64 = 1 << 0;
/// Register for write-protect faults.
pub const UFFDIO_REGISTER_MODE_WP: u64 = 1 << 1;
/// Register for minor faults.
pub const UFFDIO_REGISTER_MODE_MINOR: u64 = 1 << 2;

// ---------------------------------------------------------------------------
// Page fault event flags
// ---------------------------------------------------------------------------

/// Fault was a write access.
pub const UFFD_PAGEFAULT_FLAG_WRITE: u64 = 1 << 0;
/// Fault was a write-protect violation.
pub const UFFD_PAGEFAULT_FLAG_WP: u64 = 1 << 1;
/// Minor fault (page present but not mapped).
pub const UFFD_PAGEFAULT_FLAG_MINOR: u64 = 1 << 2;

// ---------------------------------------------------------------------------
// userfaultfd API version
// ---------------------------------------------------------------------------

/// Current userfaultfd API version.
pub const UFFD_API: u64 = 0xAA;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ioctl_commands_distinct() {
        let cmds = [
            UFFDIO_API,
            UFFDIO_REGISTER,
            UFFDIO_UNREGISTER,
            UFFDIO_COPY,
            UFFDIO_ZEROPAGE,
            UFFDIO_WAKE,
            UFFDIO_WRITEPROTECT,
            UFFDIO_CONTINUE,
            UFFDIO_POISON,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_feature_flags_no_overlap() {
        let features = [
            UFFD_FEATURE_PAGEFAULT_FLAG_WP,
            UFFD_FEATURE_EVENT_FORK,
            UFFD_FEATURE_EVENT_REMAP,
            UFFD_FEATURE_EVENT_REMOVE,
            UFFD_FEATURE_MISSING_SHMEM,
            UFFD_FEATURE_MISSING_HUGETLBFS,
            UFFD_FEATURE_EVENT_UNMAP,
            UFFD_FEATURE_SIGBUS,
            UFFD_FEATURE_THREAD_ID,
            UFFD_FEATURE_MINOR_HUGETLBFS,
            UFFD_FEATURE_MINOR_SHMEM,
            UFFD_FEATURE_EXACT_ADDRESS,
            UFFD_FEATURE_WP_HUGETLBFS_SHMEM,
            UFFD_FEATURE_WP_UNPOPULATED,
            UFFD_FEATURE_POISON,
        ];
        for i in 0..features.len() {
            assert!(features[i].is_power_of_two());
            for j in (i + 1)..features.len() {
                assert_eq!(features[i] & features[j], 0);
            }
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
            assert!(modes[i].is_power_of_two());
            for j in (i + 1)..modes.len() {
                assert_eq!(modes[i] & modes[j], 0);
            }
        }
    }

    #[test]
    fn test_pagefault_flags_no_overlap() {
        let flags = [
            UFFD_PAGEFAULT_FLAG_WRITE,
            UFFD_PAGEFAULT_FLAG_WP,
            UFFD_PAGEFAULT_FLAG_MINOR,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_api_version() {
        assert_eq!(UFFD_API, 0xAA);
    }
}
