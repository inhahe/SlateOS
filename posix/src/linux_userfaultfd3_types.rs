//! `<linux/userfaultfd.h>` — Additional userfaultfd constants (batch 3).
//!
//! Supplementary userfaultfd constants covering feature flags,
//! register modes, and UFFDIO ioctl numbers.

// ---------------------------------------------------------------------------
// Userfaultfd feature flags (UFFD_FEATURE_*)
// ---------------------------------------------------------------------------

/// Page fault on missing pages.
pub const UFFD_FEATURE_PAGEFAULT_FLAG_WP: u64 = 1 << 0;
/// Event: fork.
pub const UFFD_FEATURE_EVENT_FORK: u64 = 1 << 1;
/// Event: remap (mremap).
pub const UFFD_FEATURE_EVENT_REMAP: u64 = 1 << 2;
/// Event: remove (madvise DONTNEED/munmap).
pub const UFFD_FEATURE_EVENT_REMOVE: u64 = 1 << 3;
/// Event: unmap.
pub const UFFD_FEATURE_EVENT_UNMAP: u64 = 1 << 4;
/// Missing huge pages.
pub const UFFD_FEATURE_MISSING_HUGETLBFS: u64 = 1 << 5;
/// Missing shared memory.
pub const UFFD_FEATURE_MISSING_SHMEM: u64 = 1 << 6;
/// Sigbus instead of page fault.
pub const UFFD_FEATURE_SIGBUS: u64 = 1 << 7;
/// Thread ID in fault messages.
pub const UFFD_FEATURE_THREAD_ID: u64 = 1 << 8;
/// Minor fault handling.
pub const UFFD_FEATURE_MINOR_HUGETLBFS: u64 = 1 << 9;
/// Minor fault for shmem.
pub const UFFD_FEATURE_MINOR_SHMEM: u64 = 1 << 10;
/// Exact address in fault messages.
pub const UFFD_FEATURE_EXACT_ADDRESS: u64 = 1 << 11;
/// Write-protect unpopulated.
pub const UFFD_FEATURE_WP_HUGETLBFS_SHMEM: u64 = 1 << 12;
/// Write-protect async.
pub const UFFD_FEATURE_WP_ASYNC: u64 = 1 << 13;
/// Write-protect unpopulated PTEs.
pub const UFFD_FEATURE_WP_UNPOPULATED: u64 = 1 << 14;
/// Poison pages.
pub const UFFD_FEATURE_POISON: u64 = 1 << 15;

// ---------------------------------------------------------------------------
// Register modes (UFFDIO_REGISTER_MODE_*)
// ---------------------------------------------------------------------------

/// Register: missing fault mode.
pub const UFFDIO_REGISTER_MODE_MISSING: u64 = 1 << 0;
/// Register: write-protect mode.
pub const UFFDIO_REGISTER_MODE_WP: u64 = 1 << 1;
/// Register: minor fault mode.
pub const UFFDIO_REGISTER_MODE_MINOR: u64 = 1 << 2;

// ---------------------------------------------------------------------------
// Copy/continue flags
// ---------------------------------------------------------------------------

/// Copy: write-protect destination.
pub const UFFDIO_COPY_MODE_DONTWAKE: u64 = 1 << 0;
/// Copy: write-protect pages.
pub const UFFDIO_COPY_MODE_WP: u64 = 1 << 1;

/// Continue: don't wake waiters.
pub const UFFDIO_CONTINUE_MODE_DONTWAKE: u64 = 1 << 0;
/// Continue: write-protect.
pub const UFFDIO_CONTINUE_MODE_WP: u64 = 1 << 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_feature_flags_power_of_two() {
        let flags: [u64; 16] = [
            UFFD_FEATURE_PAGEFAULT_FLAG_WP, UFFD_FEATURE_EVENT_FORK,
            UFFD_FEATURE_EVENT_REMAP, UFFD_FEATURE_EVENT_REMOVE,
            UFFD_FEATURE_EVENT_UNMAP, UFFD_FEATURE_MISSING_HUGETLBFS,
            UFFD_FEATURE_MISSING_SHMEM, UFFD_FEATURE_SIGBUS,
            UFFD_FEATURE_THREAD_ID, UFFD_FEATURE_MINOR_HUGETLBFS,
            UFFD_FEATURE_MINOR_SHMEM, UFFD_FEATURE_EXACT_ADDRESS,
            UFFD_FEATURE_WP_HUGETLBFS_SHMEM, UFFD_FEATURE_WP_ASYNC,
            UFFD_FEATURE_WP_UNPOPULATED, UFFD_FEATURE_POISON,
        ];
        for f in &flags {
            assert!(f.is_power_of_two(), "0x{:016x} not power of two", f);
        }
    }

    #[test]
    fn test_feature_flags_no_overlap() {
        let flags: [u64; 16] = [
            UFFD_FEATURE_PAGEFAULT_FLAG_WP, UFFD_FEATURE_EVENT_FORK,
            UFFD_FEATURE_EVENT_REMAP, UFFD_FEATURE_EVENT_REMOVE,
            UFFD_FEATURE_EVENT_UNMAP, UFFD_FEATURE_MISSING_HUGETLBFS,
            UFFD_FEATURE_MISSING_SHMEM, UFFD_FEATURE_SIGBUS,
            UFFD_FEATURE_THREAD_ID, UFFD_FEATURE_MINOR_HUGETLBFS,
            UFFD_FEATURE_MINOR_SHMEM, UFFD_FEATURE_EXACT_ADDRESS,
            UFFD_FEATURE_WP_HUGETLBFS_SHMEM, UFFD_FEATURE_WP_ASYNC,
            UFFD_FEATURE_WP_UNPOPULATED, UFFD_FEATURE_POISON,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_register_modes_power_of_two() {
        assert!(UFFDIO_REGISTER_MODE_MISSING.is_power_of_two());
        assert!(UFFDIO_REGISTER_MODE_WP.is_power_of_two());
        assert!(UFFDIO_REGISTER_MODE_MINOR.is_power_of_two());
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

    #[test]
    fn test_copy_modes_no_overlap() {
        assert_eq!(UFFDIO_COPY_MODE_DONTWAKE & UFFDIO_COPY_MODE_WP, 0);
    }

    #[test]
    fn test_continue_modes_no_overlap() {
        assert_eq!(
            UFFDIO_CONTINUE_MODE_DONTWAKE & UFFDIO_CONTINUE_MODE_WP,
            0
        );
    }
}
