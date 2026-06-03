//! `<linux/userfaultfd.h>` — userfaultfd(2) protocol constants.
//!
//! userfaultfd lets one process handle another's page faults in
//! userspace. CRIU uses it for post-copy migration; database engines
//! use it to implement on-demand paging; emulators use it to
//! reflect guest faults.

// ---------------------------------------------------------------------------
// Protocol versions and API magic
// ---------------------------------------------------------------------------

/// `UFFD_API` — magic for the UFFDIO_API ioctl arg.
pub const UFFD_API: u64 = 0xAA;

// ---------------------------------------------------------------------------
// UFFD feature bits (uffdio_api.features)
// ---------------------------------------------------------------------------

/// PAGEFAULT_FLAG_WP — write-protect faults.
pub const UFFD_FEATURE_PAGEFAULT_FLAG_WP: u64 = 1 << 0;
/// EVENT_FORK — child copies UFFD on fork().
pub const UFFD_FEATURE_EVENT_FORK: u64 = 1 << 1;
/// EVENT_REMAP — mremap notifications.
pub const UFFD_FEATURE_EVENT_REMAP: u64 = 1 << 2;
/// EVENT_REMOVE — madvise(REMOVE) notifications.
pub const UFFD_FEATURE_EVENT_REMOVE: u64 = 1 << 3;
/// MISSING_HUGETLBFS — hugetlb missing-page faults.
pub const UFFD_FEATURE_MISSING_HUGETLBFS: u64 = 1 << 4;
/// MISSING_SHMEM — shmem missing-page faults.
pub const UFFD_FEATURE_MISSING_SHMEM: u64 = 1 << 5;
/// EVENT_UNMAP — munmap notifications.
pub const UFFD_FEATURE_EVENT_UNMAP: u64 = 1 << 6;
/// SIGBUS — deliver SIGBUS to faulter instead of waiting on UFFD.
pub const UFFD_FEATURE_SIGBUS: u64 = 1 << 7;
/// THREAD_ID — include tid in fault message.
pub const UFFD_FEATURE_THREAD_ID: u64 = 1 << 8;
/// MINOR_HUGETLBFS — minor (write-after-WP) faults on hugetlbfs.
pub const UFFD_FEATURE_MINOR_HUGETLBFS: u64 = 1 << 9;
/// MINOR_SHMEM — minor faults on shmem.
pub const UFFD_FEATURE_MINOR_SHMEM: u64 = 1 << 10;
/// EXACT_ADDRESS — faulting address reported exactly (not aligned).
pub const UFFD_FEATURE_EXACT_ADDRESS: u64 = 1 << 11;
/// WP_HUGETLBFS_SHMEM — write-protect on hugetlb+shmem.
pub const UFFD_FEATURE_WP_HUGETLBFS_SHMEM: u64 = 1 << 12;

// ---------------------------------------------------------------------------
// UFFD register modes (uffdio_register.mode)
// ---------------------------------------------------------------------------

/// Register for missing-page faults.
pub const UFFDIO_REGISTER_MODE_MISSING: u64 = 1 << 0;
/// Register for write-protect faults.
pub const UFFDIO_REGISTER_MODE_WP: u64 = 1 << 1;
/// Register for minor faults (write-after-WP, or hugetlb minor).
pub const UFFDIO_REGISTER_MODE_MINOR: u64 = 1 << 2;

// ---------------------------------------------------------------------------
// Event message types (struct uffd_msg.event)
// ---------------------------------------------------------------------------

/// PAGEFAULT.
pub const UFFD_EVENT_PAGEFAULT: u8 = 0x12;
/// FORK.
pub const UFFD_EVENT_FORK: u8 = 0x13;
/// REMAP.
pub const UFFD_EVENT_REMAP: u8 = 0x14;
/// REMOVE.
pub const UFFD_EVENT_REMOVE: u8 = 0x15;
/// UNMAP.
pub const UFFD_EVENT_UNMAP: u8 = 0x16;

// ---------------------------------------------------------------------------
// PAGEFAULT.flags
// ---------------------------------------------------------------------------

/// Fault is a write.
pub const UFFD_PAGEFAULT_FLAG_WRITE: u64 = 1 << 0;
/// Fault is a write-protect violation (vs missing page).
pub const UFFD_PAGEFAULT_FLAG_WP: u64 = 1 << 1;
/// Fault is a minor fault.
pub const UFFD_PAGEFAULT_FLAG_MINOR: u64 = 1 << 2;

// ---------------------------------------------------------------------------
// ioctls (numbered _AA-relative)
// ---------------------------------------------------------------------------

/// `UFFDIO_API` — negotiate API version.
pub const UFFDIO_API: u32 = 0xc01b_aa3f;
/// `UFFDIO_REGISTER` — install a fault-watch range.
pub const UFFDIO_REGISTER: u32 = 0xc020_aa00;
/// `UFFDIO_UNREGISTER` — remove a watch range.
pub const UFFDIO_UNREGISTER: u32 = 0x8010_aa01;
/// `UFFDIO_WAKE` — wake any threads parked on the range.
pub const UFFDIO_WAKE: u32 = 0x8010_aa02;
/// `UFFDIO_COPY` — copy data into the missing page.
pub const UFFDIO_COPY: u32 = 0xc028_aa03;
/// `UFFDIO_ZEROPAGE` — install a zero page.
pub const UFFDIO_ZEROPAGE: u32 = 0xc020_aa04;
/// `UFFDIO_WRITEPROTECT` — toggle WP on a range.
pub const UFFDIO_WRITEPROTECT: u32 = 0xc018_aa06;
/// `UFFDIO_CONTINUE` — minor-fault completion.
pub const UFFDIO_CONTINUE: u32 = 0xc020_aa07;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_api_magic() {
        // Userspace passes UFFD_API in uffdio_api.api — kernel rejects
        // anything else with EINVAL.
        assert_eq!(UFFD_API, 0xAA);
    }

    #[test]
    fn test_features_pow2_distinct() {
        let f = [
            UFFD_FEATURE_PAGEFAULT_FLAG_WP,
            UFFD_FEATURE_EVENT_FORK,
            UFFD_FEATURE_EVENT_REMAP,
            UFFD_FEATURE_EVENT_REMOVE,
            UFFD_FEATURE_MISSING_HUGETLBFS,
            UFFD_FEATURE_MISSING_SHMEM,
            UFFD_FEATURE_EVENT_UNMAP,
            UFFD_FEATURE_SIGBUS,
            UFFD_FEATURE_THREAD_ID,
            UFFD_FEATURE_MINOR_HUGETLBFS,
            UFFD_FEATURE_MINOR_SHMEM,
            UFFD_FEATURE_EXACT_ADDRESS,
            UFFD_FEATURE_WP_HUGETLBFS_SHMEM,
        ];
        for &b in &f {
            assert!(b.is_power_of_two());
        }
        for i in 0..f.len() {
            for j in (i + 1)..f.len() {
                assert_ne!(f[i], f[j]);
            }
        }
    }

    #[test]
    fn test_register_modes_pow2_distinct() {
        let m = [
            UFFDIO_REGISTER_MODE_MISSING,
            UFFDIO_REGISTER_MODE_WP,
            UFFDIO_REGISTER_MODE_MINOR,
        ];
        for &b in &m {
            assert!(b.is_power_of_two());
        }
        for i in 0..m.len() {
            for j in (i + 1)..m.len() {
                assert_ne!(m[i], m[j]);
            }
        }
    }

    #[test]
    fn test_event_msg_types_distinct() {
        let e = [
            UFFD_EVENT_PAGEFAULT,
            UFFD_EVENT_FORK,
            UFFD_EVENT_REMAP,
            UFFD_EVENT_REMOVE,
            UFFD_EVENT_UNMAP,
        ];
        for i in 0..e.len() {
            for j in (i + 1)..e.len() {
                assert_ne!(e[i], e[j]);
            }
        }
    }

    #[test]
    fn test_pagefault_flag_bits_pow2_distinct() {
        let f = [
            UFFD_PAGEFAULT_FLAG_WRITE,
            UFFD_PAGEFAULT_FLAG_WP,
            UFFD_PAGEFAULT_FLAG_MINOR,
        ];
        for &b in &f {
            assert!(b.is_power_of_two());
        }
        for i in 0..f.len() {
            for j in (i + 1)..f.len() {
                assert_ne!(f[i], f[j]);
            }
        }
    }

    #[test]
    fn test_ioctls_distinct_and_use_letter_aa() {
        let ops = [
            UFFDIO_API,
            UFFDIO_REGISTER,
            UFFDIO_UNREGISTER,
            UFFDIO_WAKE,
            UFFDIO_COPY,
            UFFDIO_ZEROPAGE,
            UFFDIO_WRITEPROTECT,
            UFFDIO_CONTINUE,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
            // Type byte 0xAA in bits 8..15.
            assert_eq!((ops[i] >> 8) & 0xff, 0xAA);
        }
    }
}
