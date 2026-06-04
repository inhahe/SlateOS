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
pub const UFFD_FLAGS_VALID: u32 = O_CLOEXEC_UFFD | O_NONBLOCK_UFFD | UFFD_USER_MODE_ONLY;

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
/// Linux semantics (`fs/userfaultfd.c::SYSCALL_DEFINE1(userfaultfd, ...)`
/// → `new_userfaultfd`):
///
/// 1. **Phase 183 — privilege gate (runs first in the syscall entry).**
///    Linux does:
///    ```c
///    if (!sysctl_unprivileged_userfaultfd
///        && (flags & UFFD_USER_MODE_ONLY) == 0
///        && !capable(CAP_SYS_PTRACE))
///        return -EPERM;
///    ```
///    The upstream and Debian/Ubuntu default is
///    `sysctl_unprivileged_userfaultfd = 0`, so unprivileged
///    callers can only get a userfaultfd if they set
///    `UFFD_USER_MODE_ONLY` (which restricts the resulting fd to
///    user-mode faults, mitigating the kernel-attack vectors that
///    motivated the sysctl).  We model the strictest setting (0).
/// 2. `new_userfaultfd` then runs `if (flags & ~(UFFD_USER_MODE_ONLY
///    | UFFD_SHARED_FCNTL_FLAGS)) return -EINVAL;` so any unknown
///    flag bit → EINVAL.
/// 3. Otherwise we'd return a new userfaultfd fd; we return ENOSYS
///    instead because no userfaultfd subsystem exists.
///
/// Ordering note: the cap check happens **before** the flag-mask
/// EINVAL check on real Linux.  So a caller passing
/// `flags = 0xdeadbeee` (USER_MODE_ONLY bit clear) without
/// CAP_SYS_PTRACE sees EPERM, not EINVAL.  A caller passing
/// `flags = 0xdeadbeef` (USER_MODE_ONLY bit set) bypasses the cap
/// check and gets EINVAL for the other unknown bits.  We mirror
/// that exact ordering.
///
/// Note: the `flags` parameter is declared `i32` to match the Linux C
/// signature, but Linux treats it as a bitfield. We cast to `u32` for the
/// mask check, which means setting the sign bit (`flags == i32::MIN`)
/// turns into a bit outside the valid mask and gets EINVAL — same behavior
/// as the kernel.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn userfaultfd(flags: i32) -> i32 {
    let flags_u = flags as u32;

    // Phase 183: privilege gate, BEFORE flag-mask EINVAL — matching
    // Linux's syscall entry-point order.  Assumes
    // sysctl_unprivileged_userfaultfd=0 (upstream default).  A caller
    // that sets UFFD_USER_MODE_ONLY bypasses the cap check; everyone
    // else needs CAP_SYS_PTRACE.
    if (flags_u & UFFD_USER_MODE_ONLY) == 0
        && !crate::sys_capability::has_capability(crate::sys_capability::CAP_SYS_PTRACE)
    {
        errno::set_errno(errno::EPERM);
        return -1;
    }

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
        let r = userfaultfd((UFFD_USER_MODE_ONLY | O_CLOEXEC_UFFD | O_NONBLOCK_UFFD) as i32);
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

    // ----------------------------------------------------------------------
    // Phase 183: userfaultfd — CAP_SYS_PTRACE gate on non-USER_MODE_ONLY
    // requests.
    //
    // Pre-Phase-183 behaviour: every flag-valid userfaultfd() call fell
    // through to ENOSYS regardless of capability.  That let unprivileged
    // code probe for userfaultfd availability without ever seeing the
    // EPERM Linux gives under default `sysctl_unprivileged_userfaultfd=0`
    // — misleading CRIU's lazy-pages probe, QEMU's postcopy-migration
    // probe, V8's GC, and the libuserfaultfd self-test into thinking
    // the kernel just lacks CONFIG_USERFAULTFD when actually it's a
    // policy denial.  Now we surface the distinction.
    //
    // Linux semantics (fs/userfaultfd.c::SYSCALL_DEFINE1(userfaultfd)):
    //     if (!sysctl_unprivileged_userfaultfd
    //         && (flags & UFFD_USER_MODE_ONLY) == 0
    //         && !capable(CAP_SYS_PTRACE))
    //         return -EPERM;
    //     return new_userfaultfd(flags);
    //
    // And new_userfaultfd then validates the flag mask:
    //     if (flags & ~(USER_MODE_ONLY | SHARED_FCNTL_FLAGS))
    //         return -EINVAL;
    //
    // Ordering: cap check fires BEFORE the EINVAL flag-mask check.
    // USER_MODE_ONLY set bypasses the cap requirement (it restricts
    // the resulting fd to user-mode faults, which is the security
    // hardening that motivated the sysctl).
    //
    // We assume sysctl=0 (upstream default).  Errno is EPERM (Linux's
    // chosen errno for this gate — `capable()` failure).
    // ----------------------------------------------------------------------

    mod userfaultfd_cap_phase183 {
        use super::*;

        struct CapGuard {

            lo: u32,

            hi: u32,

            // Held for the lifetime of the guard. See

            // `sys_capability::CAP_TEST_LOCK` for why.

            _lock: crate::sys_capability::CapTestLockGuard,

        }
        impl CapGuard {
            fn snapshot() -> Self {
            // Re-entrant lock guard: outermost acquire on the
            // thread takes the global mutex; nested acquires
            // (some tests stack a scoped CapGuard inside an
            // outer one) are no-ops for the lock but still
            // snapshot/restore caps independently.
            let lock = crate::sys_capability::CapTestLockGuard::acquire();
            let (lo, hi) = crate::sys_capability::current_caps_effective();
            Self { lo, hi, _lock: lock }
            }
        }
        impl Drop for CapGuard {
            fn drop(&mut self) {
                let mut hdr = crate::sys_capability::CapUserHeader {
                    version: crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
                    pid: 0,
                };
                let data = [
                    crate::sys_capability::CapUserData {
                        effective: self.lo,
                        permitted: u32::MAX,
                        inheritable: 0,
                    },
                    crate::sys_capability::CapUserData {
                        effective: self.hi,
                        permitted: u32::MAX,
                        inheritable: 0,
                    },
                ];
                let _ = crate::sys_capability::capset(&mut hdr, data.as_ptr());
            }
        }

        fn drop_cap(cap: u32) {
            let (lo, hi) = crate::sys_capability::current_caps_effective();
            let (new_lo, new_hi) = if cap < 32 {
                (lo & !(1u32 << cap), hi)
            } else {
                (lo, hi & !(1u32 << (cap - 32)))
            };
            let mut hdr = crate::sys_capability::CapUserHeader {
                version: crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
                pid: 0,
            };
            let data = [
                crate::sys_capability::CapUserData {
                    effective: new_lo,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
                crate::sys_capability::CapUserData {
                    effective: new_hi,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
            ];
            let rc = crate::sys_capability::capset(&mut hdr, data.as_ptr());
            assert_eq!(rc, 0, "capset must succeed");
            assert!(!crate::sys_capability::has_capability(cap));
        }

        fn drop_sys_ptrace() {
            drop_cap(crate::sys_capability::CAP_SYS_PTRACE);
        }

        // -- Per-error-class ----------------------------------------------

        /// Zero flags (no USER_MODE_ONLY) without CAP_SYS_PTRACE →
        /// -1/EPERM.  The smallest valid call exercises the gate.
        #[test]
        fn test_uffd_phase183_zero_flags_no_cap_returns_eperm() {
            let _g = CapGuard::snapshot();
            drop_sys_ptrace();
            errno::set_errno(0);
            assert_eq!(userfaultfd(0), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        /// O_CLOEXEC alone (no USER_MODE_ONLY) without cap → EPERM.
        #[test]
        fn test_uffd_phase183_cloexec_only_no_cap_returns_eperm() {
            let _g = CapGuard::snapshot();
            drop_sys_ptrace();
            errno::set_errno(0);
            assert_eq!(userfaultfd(O_CLOEXEC_UFFD as i32), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        /// USER_MODE_ONLY set bypasses the cap check — exactly the
        /// Linux design (USER_MODE_ONLY mitigates the kernel-attack
        /// vectors that motivated the sysctl, so it's safe for
        /// unprivileged use).  Falls through to ENOSYS.
        #[test]
        fn test_uffd_phase183_user_mode_only_bypasses_cap_check() {
            let _g = CapGuard::snapshot();
            drop_sys_ptrace();
            errno::set_errno(0);
            assert_eq!(userfaultfd(UFFD_USER_MODE_ONLY as i32), -1);
            assert_eq!(errno::get_errno(), errno::ENOSYS);
        }

        /// USER_MODE_ONLY + O_CLOEXEC (both valid) without cap →
        /// ENOSYS (USER_MODE_ONLY bypass).
        #[test]
        fn test_uffd_phase183_user_mode_only_with_cloexec_no_cap_enosys() {
            let _g = CapGuard::snapshot();
            drop_sys_ptrace();
            let f = UFFD_USER_MODE_ONLY | O_CLOEXEC_UFFD;
            errno::set_errno(0);
            assert_eq!(userfaultfd(f as i32), -1);
            assert_eq!(errno::get_errno(), errno::ENOSYS);
        }

        /// Errno is EPERM (Linux's chosen errno for capable()
        /// failure), not EACCES (policy denial) and not EINVAL
        /// (arg mismatch).
        #[test]
        fn test_uffd_phase183_errno_is_eperm_not_eacces() {
            let _g = CapGuard::snapshot();
            drop_sys_ptrace();
            errno::set_errno(0);
            assert_eq!(userfaultfd(0), -1);
            assert_ne!(errno::get_errno(), errno::EACCES);
            assert_ne!(errno::get_errno(), errno::EINVAL);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        // -- Ordering matrix ----------------------------------------------

        /// EPERM beats EINVAL for unknown flag bits when
        /// USER_MODE_ONLY is NOT set.  Linux's syscall entry runs
        /// the cap check before new_userfaultfd's flag-mask check.
        /// A no-cap caller passing flags = 0xdeadbeee
        /// (USER_MODE_ONLY bit cleared) sees EPERM, not EINVAL.
        #[test]
        fn test_uffd_phase183_eperm_beats_einval_when_user_mode_off() {
            let _g = CapGuard::snapshot();
            drop_sys_ptrace();
            // 0xdeadbeee has bit 0 clear (USER_MODE_ONLY = bit 0 = 1).
            errno::set_errno(0);
            assert_eq!(userfaultfd(0xdeadbeeeu32 as i32), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        /// EINVAL beats EPERM when USER_MODE_ONLY IS set.  Linux's
        /// cap check is bypassed (USER_MODE_ONLY set), then
        /// new_userfaultfd's flag-mask EINVAL fires.  Cap state
        /// doesn't matter.
        #[test]
        fn test_uffd_phase183_einval_beats_eperm_when_user_mode_on() {
            let _g = CapGuard::snapshot();
            drop_sys_ptrace();
            // 0xdeadbeef has bit 0 set (USER_MODE_ONLY) plus other
            // junk bits.
            errno::set_errno(0);
            assert_eq!(userfaultfd(0xdeadbeefu32 as i32), -1);
            assert_eq!(errno::get_errno(), errno::EINVAL);
        }

        /// i32::MIN (sign bit set) without USER_MODE_ONLY → EPERM,
        /// not EINVAL.  Confirms cap gate fires for "negative" int
        /// flag values when the USER_MODE_ONLY bit is clear.
        #[test]
        fn test_uffd_phase183_intmin_no_cap_returns_eperm() {
            let _g = CapGuard::snapshot();
            drop_sys_ptrace();
            // i32::MIN = 0x80000000 — bit 0 (USER_MODE_ONLY) is clear.
            errno::set_errno(0);
            assert_eq!(userfaultfd(i32::MIN), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        // -- Workflow -----------------------------------------------------

        /// CRIU lazy-restore workflow: probe without USER_MODE_ONLY
        /// → EPERM (CRIU's logs surface "userfaultfd denied by
        /// sysctl/cap; falling back to normal restore"); retry with
        /// USER_MODE_ONLY → ENOSYS (CRIU then knows the kernel
        /// just lacks CONFIG_USERFAULTFD and disables lazy mode).
        #[test]
        fn test_uffd_phase183_workflow_criu_lazy_restore_retry() {
            let _g = CapGuard::snapshot();
            drop_sys_ptrace();
            // 1st probe: no USER_MODE_ONLY, no cap → EPERM.
            errno::set_errno(0);
            assert_eq!(userfaultfd(O_CLOEXEC_UFFD as i32), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
            // 2nd probe: USER_MODE_ONLY set → ENOSYS.
            errno::set_errno(0);
            let f = O_CLOEXEC_UFFD | UFFD_USER_MODE_ONLY;
            assert_eq!(userfaultfd(f as i32), -1);
            assert_eq!(errno::get_errno(), errno::ENOSYS);
        }

        /// Sandbox workflow: privileged daemon reaches ENOSYS,
        /// sandboxed child after dropping CAP_SYS_PTRACE sees EPERM
        /// for the same call.
        #[test]
        fn test_uffd_phase183_workflow_sandbox_drops_then_denied() {
            let _g = CapGuard::snapshot();
            // Parent with cap: ENOSYS.
            errno::set_errno(0);
            assert_eq!(userfaultfd(0), -1);
            assert_eq!(errno::get_errno(), errno::ENOSYS);
            // After sandbox drop: EPERM.
            drop_sys_ptrace();
            errno::set_errno(0);
            assert_eq!(userfaultfd(0), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        // -- Recovery -----------------------------------------------------

        /// After EPERM, restoring CAP_SYS_PTRACE lets the same call
        /// reach ENOSYS.  Confirms dynamic per-call cap evaluation.
        #[test]
        fn test_uffd_phase183_recovery_restore_cap_reaches_enosys() {
            let _g = CapGuard::snapshot();
            drop_sys_ptrace();
            errno::set_errno(0);
            assert_eq!(userfaultfd(0), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
            // Restore caps.
            let mut hdr = crate::sys_capability::CapUserHeader {
                version: crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
                pid: 0,
            };
            let data = [
                crate::sys_capability::CapUserData {
                    effective: u32::MAX,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
                crate::sys_capability::CapUserData {
                    effective: (1u32 << 9) - 1,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
            ];
            assert_eq!(crate::sys_capability::capset(&mut hdr, data.as_ptr()), 0);
            errno::set_errno(0);
            assert_eq!(userfaultfd(0), -1);
            assert_eq!(errno::get_errno(), errno::ENOSYS);
        }

        // -- No-side-effect ----------------------------------------------

        /// EPERM rejection leaves capability sets unchanged.
        #[test]
        fn test_uffd_phase183_eperm_preserves_other_caps() {
            let _g = CapGuard::snapshot();
            drop_sys_ptrace();
            let (lo_before, hi_before) = crate::sys_capability::current_caps_effective();
            let _ = userfaultfd(0);
            let (lo_after, hi_after) = crate::sys_capability::current_caps_effective();
            assert_eq!(lo_before, lo_after);
            assert_eq!(hi_before, hi_after);
        }

        // -- Sentinel ----------------------------------------------------

        /// CAP_SYS_ADMIN alone does NOT satisfy the gate — only
        /// CAP_SYS_PTRACE does.  Linux specifically gates this on
        /// CAP_SYS_PTRACE (since the threat model is "userfaultfd
        /// is debugger-like and can be abused to attack the
        /// kernel"), not on the historical CAP_SYS_ADMIN catch-all.
        #[test]
        fn test_uffd_phase183_cap_sys_admin_alone_does_not_satisfy() {
            let _g = CapGuard::snapshot();
            drop_sys_ptrace();
            // CAP_SYS_ADMIN still held.
            assert!(crate::sys_capability::has_capability(
                crate::sys_capability::CAP_SYS_ADMIN
            ));
            errno::set_errno(0);
            assert_eq!(userfaultfd(0), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        // -- Cross-checks ------------------------------------------------

        /// USER_MODE_ONLY bypass is symmetric: with or without
        /// CAP_SYS_PTRACE held, USER_MODE_ONLY → ENOSYS.  Confirms
        /// the bypass branch isn't accidentally cap-gated.
        #[test]
        fn test_uffd_phase183_user_mode_only_symmetric_with_cap() {
            let _g = CapGuard::snapshot();
            // With cap: ENOSYS.
            errno::set_errno(0);
            assert_eq!(userfaultfd(UFFD_USER_MODE_ONLY as i32), -1);
            assert_eq!(errno::get_errno(), errno::ENOSYS);
            // Drop cap, repeat: still ENOSYS.
            drop_sys_ptrace();
            errno::set_errno(0);
            assert_eq!(userfaultfd(UFFD_USER_MODE_ONLY as i32), -1);
            assert_eq!(errno::get_errno(), errno::ENOSYS);
        }

        /// USER_MODE_ONLY value is bit 0 (= 1) — Linux uapi value.
        /// Sanity-check the constant we rely on for the cap-bypass
        /// branch.
        #[test]
        fn test_uffd_phase183_user_mode_only_is_bit_zero() {
            assert_eq!(UFFD_USER_MODE_ONLY, 1u32);
        }
    }
}
