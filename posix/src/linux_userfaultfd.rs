//! `<linux/userfaultfd.h>` — user-space page fault handling.
//!
//! userfaultfd allows a process to handle page faults in userspace,
//! enabling live migration, garbage collection, and lazy restore.

use crate::errno;

// ---------------------------------------------------------------------------
// userfaultfd flags
// ---------------------------------------------------------------------------

/// Close-on-exec.
pub const UFFD_USER_MODE_ONLY: u32 = 1;

// ---------------------------------------------------------------------------
// userfaultfd ioctl commands
// ---------------------------------------------------------------------------

/// Handshake with kernel, negotiate features.
pub const UFFDIO_API: u64 = 0xC018AA3F;
/// Register a memory range.
pub const UFFDIO_REGISTER: u64 = 0xC020AA00;
/// Unregister a memory range.
pub const UFFDIO_UNREGISTER: u64 = 0x8010AA01;
/// Copy page data to faulting address.
pub const UFFDIO_COPY: u64 = 0xC028AA03;
/// Zero-fill page at faulting address.
pub const UFFDIO_ZEROPAGE: u64 = 0xC020AA04;
/// Wake waiting threads.
pub const UFFDIO_WAKE: u64 = 0x8010AA02;
/// Write-protect pages.
pub const UFFDIO_WRITEPROTECT: u64 = 0xC018AA06;
/// Continue (for minor faults).
pub const UFFDIO_CONTINUE: u64 = 0xC018AA07;
/// Poison pages (make them generate SIGBUS).
pub const UFFDIO_POISON: u64 = 0xC018AA08;

// ---------------------------------------------------------------------------
// Feature flags (negotiated via UFFDIO_API)
// ---------------------------------------------------------------------------

/// Report page faults.
pub const UFFD_FEATURE_PAGEFAULT_FLAG_WP: u64 = 1 << 0;
/// Report fork events.
pub const UFFD_FEATURE_EVENT_FORK: u64 = 1 << 1;
/// Report remap events (mremap).
pub const UFFD_FEATURE_EVENT_REMAP: u64 = 1 << 2;
/// Report madvise(DONTNEED) events.
pub const UFFD_FEATURE_EVENT_REMOVE: u64 = 1 << 3;
/// Report unmap events.
pub const UFFD_FEATURE_EVENT_UNMAP: u64 = 1 << 4;
/// Missing hugetlbfs support.
pub const UFFD_FEATURE_MISSING_HUGETLBFS: u64 = 1 << 5;
/// Missing shmem support.
pub const UFFD_FEATURE_MISSING_SHMEM: u64 = 1 << 6;
/// Sigbus (non-fatal) mode.
pub const UFFD_FEATURE_SIGBUS: u64 = 1 << 7;
/// Thread ID in fault messages.
pub const UFFD_FEATURE_THREAD_ID: u64 = 1 << 8;
/// Minor page fault handling (shared memory).
pub const UFFD_FEATURE_MINOR_HUGETLBFS: u64 = 1 << 9;
/// Minor page fault handling (shmem).
pub const UFFD_FEATURE_MINOR_SHMEM: u64 = 1 << 10;
/// Exact address in fault report.
pub const UFFD_FEATURE_EXACT_ADDRESS: u64 = 1 << 11;
/// Write-protect on userfaultfd unpopulated.
pub const UFFD_FEATURE_WP_HUGETLBFS_SHMEM: u64 = 1 << 12;
/// Write-protect async mode.
pub const UFFD_FEATURE_WP_ASYNC: u64 = 1 << 14;

// ---------------------------------------------------------------------------
// Register mode flags
// ---------------------------------------------------------------------------

/// Track missing pages.
pub const UFFDIO_REGISTER_MODE_MISSING: u64 = 1 << 0;
/// Track write-protected pages.
pub const UFFDIO_REGISTER_MODE_WP: u64 = 1 << 1;
/// Track minor faults.
pub const UFFDIO_REGISTER_MODE_MINOR: u64 = 1 << 2;

// ---------------------------------------------------------------------------
// Page fault flags (in uffd_msg)
// ---------------------------------------------------------------------------

/// Write fault.
pub const UFFD_PAGEFAULT_FLAG_WRITE: u64 = 1 << 0;
/// Write-protect fault.
pub const UFFD_PAGEFAULT_FLAG_WP: u64 = 1 << 1;
/// Minor fault.
pub const UFFD_PAGEFAULT_FLAG_MINOR: u64 = 1 << 2;

// ---------------------------------------------------------------------------
// Copy/zeropage flags
// ---------------------------------------------------------------------------

/// Don't wake waiting threads after copy.
pub const UFFDIO_COPY_MODE_DONTWAKE: u64 = 1 << 0;
/// Write-protect the copied page.
pub const UFFDIO_COPY_MODE_WP: u64 = 1 << 1;
/// Don't wake after zeropage.
pub const UFFDIO_ZEROPAGE_MODE_DONTWAKE: u64 = 1 << 0;

// ---------------------------------------------------------------------------
// API version
// ---------------------------------------------------------------------------

/// Current userfaultfd API version.
pub const UFFD_API: u64 = 0xAA;

// ---------------------------------------------------------------------------
// Stub
// ---------------------------------------------------------------------------

/// Create a userfaultfd file descriptor.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn userfaultfd(_flags: i32) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ioctl_commands_distinct() {
        let cmds = [
            UFFDIO_API, UFFDIO_REGISTER, UFFDIO_UNREGISTER,
            UFFDIO_COPY, UFFDIO_ZEROPAGE, UFFDIO_WAKE,
            UFFDIO_WRITEPROTECT, UFFDIO_CONTINUE, UFFDIO_POISON,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_features_are_powers_of_two() {
        let feats = [
            UFFD_FEATURE_PAGEFAULT_FLAG_WP,
            UFFD_FEATURE_EVENT_FORK,
            UFFD_FEATURE_EVENT_REMAP,
            UFFD_FEATURE_EVENT_REMOVE,
            UFFD_FEATURE_EVENT_UNMAP,
            UFFD_FEATURE_MISSING_HUGETLBFS,
            UFFD_FEATURE_MISSING_SHMEM,
            UFFD_FEATURE_SIGBUS,
            UFFD_FEATURE_THREAD_ID,
            UFFD_FEATURE_MINOR_HUGETLBFS,
            UFFD_FEATURE_MINOR_SHMEM,
            UFFD_FEATURE_EXACT_ADDRESS,
            UFFD_FEATURE_WP_HUGETLBFS_SHMEM,
            UFFD_FEATURE_WP_ASYNC,
        ];
        for f in &feats {
            assert!(f.is_power_of_two(), "feature {f:#x} not a power of 2");
        }
    }

    #[test]
    fn test_register_modes() {
        assert_eq!(UFFDIO_REGISTER_MODE_MISSING, 1);
        assert_eq!(UFFDIO_REGISTER_MODE_WP, 2);
        assert_eq!(UFFDIO_REGISTER_MODE_MINOR, 4);
    }

    #[test]
    fn test_pagefault_flags() {
        assert_eq!(UFFD_PAGEFAULT_FLAG_WRITE, 1);
        assert_eq!(UFFD_PAGEFAULT_FLAG_WP, 2);
        assert_eq!(UFFD_PAGEFAULT_FLAG_MINOR, 4);
    }

    #[test]
    fn test_userfaultfd_stub() {
        let ret = userfaultfd(0);
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_api_version() {
        assert_eq!(UFFD_API, 0xAA);
    }
}
