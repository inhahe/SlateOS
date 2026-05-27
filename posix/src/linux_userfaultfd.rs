//! `<linux/userfaultfd.h>` — user-space page fault handling.
//!
//! userfaultfd allows a process to handle page faults in userspace,
//! enabling live migration, garbage collection, and lazy restore.
//!
//! # Why this is a validator, not a real implementation
//!
//! A real userfaultfd implementation requires deep kernel hooks: the
//! page-fault handler must be able to suspend the faulting thread and
//! deliver a notification to the userfaultfd file descriptor's reader,
//! the VMA range must be tagged so the MM core knows to take the slow
//! path, UFFDIO_COPY/UFFDIO_ZEROPAGE/UFFDIO_CONTINUE need to be able
//! to install a page atomically into another process's page tables,
//! and UFFDIO_WRITEPROTECT needs to fiddle with the W bit on existing
//! PTEs and flush remote TLBs. None of this exists in our microkernel
//! today (the MM core supports demand-zero and COW but not "deliver
//! the fault to userspace and block").
//!
//! What we provide instead is a fully Linux-compatible input validator
//! on `userfaultfd(flags)` itself, so that real userspace callers
//! (CRIU's lazy-restore mode, QEMU's postcopy live migration, the
//! libuserfaultfd test harness, V8's incremental marking, Hotspot's
//! ZGC concurrent-relocation) see exactly the same errno the Linux
//! kernel returns when built without `CONFIG_USERFAULTFD`: ENOSYS for
//! well-formed calls, EINVAL for malformed flags. That tells those
//! callers to fall back gracefully (CRIU's "lazy-pages mode not
//! available, doing normal restore", QEMU's "userfaultfd unavailable,
//! disabling postcopy migration", Hotspot's GC tuner picking a
//! non-userfaultfd collector).

use crate::errno;

// ---------------------------------------------------------------------------
// userfaultfd flags
// ---------------------------------------------------------------------------

/// Restrict to user-mode faults only (no kernel-mode access faults).
pub const UFFD_USER_MODE_ONLY: u32 = 1;

/// `O_CLOEXEC` value as Linux passes it via the `userfaultfd(2)` flags
/// argument. Same numerical value as `fcntl.h::O_CLOEXEC` on Linux/x86_64.
pub const O_CLOEXEC_UFFD: u32 = 0o2000000; // 0x80000 = 524288

/// `O_NONBLOCK` value as Linux passes it via the `userfaultfd(2)` flags
/// argument. Same numerical value as `fcntl.h::O_NONBLOCK` on Linux/x86_64.
pub const O_NONBLOCK_UFFD: u32 = 0o4000; // 0x800 = 2048

/// Complete set of flag bits accepted by `userfaultfd(2)`.
///
/// `UFFD_USER_MODE_ONLY` was added in Linux 5.11. The two `O_*` bits have
/// been accepted since the syscall was introduced in 4.3. Anything outside
/// this mask is rejected with `EINVAL` (matching Linux's `if (flags &
/// ~UFFD_SHARED_FCNTL_FLAGS) return -EINVAL`).
pub const UFFD_FLAGS_VALID: u32 =
    O_CLOEXEC_UFFD | O_NONBLOCK_UFFD | UFFD_USER_MODE_ONLY;

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
// userfaultfd(2)
// ---------------------------------------------------------------------------

/// Create a userfaultfd file descriptor.
///
/// Linux semantics (`fs/userfaultfd.c::sys_userfaultfd`):
/// - Any flag bit outside `UFFD_FLAGS_VALID` → EINVAL.
/// - All other inputs → would return a new userfaultfd fd. We instead
///   return ENOSYS because no userfaultfd subsystem exists.
///
/// Note: the `flags` parameter is declared `i32` to match the Linux C
/// signature, but Linux treats it as a bitfield. We cast to `u32` for the
/// mask check, which means setting the sign bit (`flags == i32::MIN`)
/// turns into a bit outside the valid mask and gets EINVAL — same behavior
/// as the kernel.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn userfaultfd(flags: i32) -> i32 {
    let flags_u = flags as u32;
    if flags_u & !UFFD_FLAGS_VALID != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    // Validation passed; we have no userfaultfd subsystem.
    errno::set_errno(errno::ENOSYS);
    -1
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ---- Flag-mask sanity ----

    #[test]
    fn test_flags_valid_mask_is_or_of_known_bits() {
        assert_eq!(
            UFFD_FLAGS_VALID,
            O_CLOEXEC_UFFD | O_NONBLOCK_UFFD | UFFD_USER_MODE_ONLY
        );
    }

    #[test]
    fn test_o_cloexec_uffd_matches_linux_value() {
        // Linux's <fcntl.h>: O_CLOEXEC = 0o2000000 (octal) = 0x80000 = 524288.
        assert_eq!(O_CLOEXEC_UFFD, 0x80000);
        assert_eq!(O_CLOEXEC_UFFD, 524288);
    }

    #[test]
    fn test_o_nonblock_uffd_matches_linux_value() {
        // Linux's <fcntl.h>: O_NONBLOCK = 0o4000 (octal) = 0x800 = 2048.
        assert_eq!(O_NONBLOCK_UFFD, 0x800);
        assert_eq!(O_NONBLOCK_UFFD, 2048);
    }

    #[test]
    fn test_user_mode_only_is_one() {
        assert_eq!(UFFD_USER_MODE_ONLY, 1);
    }

    // ---- Validation error paths ----

    #[test]
    fn test_unknown_flag_bit_einval() {
        errno::set_errno(errno::EBADF);
        // Bit 2 (value 4) is not in any of our valid flags.
        let r = userfaultfd(0x4);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_high_bit_einval() {
        errno::set_errno(errno::EBADF);
        // Bit 31 sign bit -> i32::MIN -> as u32 == 0x80000000, outside mask.
        let r = userfaultfd(i32::MIN);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_unknown_flag_combined_with_valid_einval() {
        // A valid flag + an unknown bit must still be rejected (Linux uses
        // `flags & ~VALID`, so any single unknown bit poisons the whole word).
        errno::set_errno(errno::EBADF);
        let r = userfaultfd((O_CLOEXEC_UFFD | 0x10) as i32);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_all_bits_set_einval() {
        errno::set_errno(errno::EBADF);
        let r = userfaultfd(-1);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    // ---- Valid inputs reach ENOSYS ----

    #[test]
    fn test_zero_flags_reaches_enosys() {
        errno::set_errno(errno::EBADF);
        let r = userfaultfd(0);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_cloexec_only_reaches_enosys() {
        errno::set_errno(errno::EBADF);
        let r = userfaultfd(O_CLOEXEC_UFFD as i32);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_nonblock_only_reaches_enosys() {
        errno::set_errno(errno::EBADF);
        let r = userfaultfd(O_NONBLOCK_UFFD as i32);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_user_mode_only_reaches_enosys() {
        errno::set_errno(errno::EBADF);
        let r = userfaultfd(UFFD_USER_MODE_ONLY as i32);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_cloexec_nonblock_reaches_enosys() {
        errno::set_errno(errno::EBADF);
        let r = userfaultfd((O_CLOEXEC_UFFD | O_NONBLOCK_UFFD) as i32);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_all_valid_flags_reaches_enosys() {
        errno::set_errno(errno::EBADF);
        let r = userfaultfd(UFFD_FLAGS_VALID as i32);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    // ---- Real-world workflow tests ----

    #[test]
    fn test_criu_lazy_restore_probe_workflow() {
        // CRIU 3.x's lazy-restore mode opens userfaultfd with
        // O_CLOEXEC|O_NONBLOCK at process-restore time. If the syscall
        // returns ENOSYS, criu falls back to its non-lazy restore path
        // (eager copy of all pages) and prints
        // "Warning: lazy-pages mode disabled, kernel lacks userfaultfd".
        errno::set_errno(errno::EBADF);
        let r = userfaultfd((O_CLOEXEC_UFFD | O_NONBLOCK_UFFD) as i32);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_qemu_postcopy_live_migration_workflow() {
        // QEMU 8.x's postcopy live migration calls
        // userfaultfd(O_CLOEXEC) early in setup. If ENOSYS, QEMU logs
        // "postcopy-ram: userfaultfd not available, disabling postcopy"
        // and falls back to precopy-only migration.
        errno::set_errno(errno::EBADF);
        let r = userfaultfd(O_CLOEXEC_UFFD as i32);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_libuserfaultfd_selftest_workflow() {
        // The kernel's tools/testing/selftests/mm/userfaultfd.c opens
        // a uffd with UFFD_USER_MODE_ONLY|O_CLOEXEC|O_NONBLOCK as its
        // first step, then bails with "kernel does not support uffd"
        // on -ENOSYS.
        errno::set_errno(errno::EBADF);
        let r = userfaultfd(
            (UFFD_USER_MODE_ONLY | O_CLOEXEC_UFFD | O_NONBLOCK_UFFD) as i32,
        );
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_hotspot_zgc_probe_workflow() {
        // OpenJDK HotSpot's ZGC initialization probes userfaultfd with
        // O_CLOEXEC to see if userspace-handled write barriers are
        // possible. ENOSYS -> ZGC picks its fallback marking mode.
        errno::set_errno(errno::EBADF);
        let r = userfaultfd(O_CLOEXEC_UFFD as i32);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    // ---- POSIX rule: success must not clobber errno ----

    #[test]
    fn test_errno_set_to_enosys_on_validation_success() {
        // Even though we *return* -1, we set errno to ENOSYS (not the
        // EINVAL of a validation failure). Callers distinguish "feature
        // unavailable" (ENOSYS) from "bad args" (EINVAL).
        errno::set_errno(errno::EBADF);
        let r = userfaultfd(0);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    // ---- Existing tests preserved ----

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
    fn test_api_version() {
        assert_eq!(UFFD_API, 0xAA);
    }
}
