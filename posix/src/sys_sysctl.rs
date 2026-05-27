//! `<sys/sysctl.h>` — system control (deprecated Linux interface).
//!
//! This interface is deprecated in Linux; modern code should use
//! `/proc/sys` or `sysctl(8)` instead.  Provided for compatibility
//! with older programs.
//!
//! # Status in Linux
//!
//! The `sysctl(2)` system call was **removed entirely from Linux 5.5**
//! (April 2020) — every call now returns `-ENOSYS` regardless of
//! arguments. Before 5.5, the kernel forwarded the call to the
//! `/proc/sys` parser, which was always buggy under nlen edge cases,
//! and the `compat_sys_sysctl` shim was the source of multiple CVEs
//! (CVE-2010-2240, CVE-2009-0029). Userspace migrated to reading and
//! writing `/proc/sys/<path>` files directly years before the removal.
//!
//! # Why this is still a validator, not a real implementation
//!
//! We follow Linux's modern (5.5+) behavior: every well-formed call
//! returns ENOSYS. But we still validate the `__sysctl_args` shape so
//! that callers using the syscall as a "feature probe" (which is what
//! glibc's `__sysctl(3)` wrapper does to decide whether to fall back
//! to `/proc/sys`) get meaningful EFAULT/EINVAL/ENOTDIR feedback on
//! truly malformed inputs and a clean ENOSYS on well-formed ones.

use crate::errno;

// ---------------------------------------------------------------------------
// CTL_* top-level names
// ---------------------------------------------------------------------------

/// Kernel parameters.
pub const CTL_KERN: i32 = 1;

/// Networking.
pub const CTL_NET: i32 = 3;

/// Virtual memory.
pub const CTL_VM: i32 = 2;

/// Filesystem.
pub const CTL_FS: i32 = 5;

/// Debug.
pub const CTL_DEBUG: i32 = 6;

/// Device.
pub const CTL_DEV: i32 = 7;

// ---------------------------------------------------------------------------
// KERN_* second-level names
// ---------------------------------------------------------------------------

/// OS type string.
pub const KERN_OSTYPE: i32 = 1;

/// OS release string.
pub const KERN_OSRELEASE: i32 = 2;

/// OS revision.
pub const KERN_OSREV: i32 = 3;

/// Kernel version string.
pub const KERN_VERSION: i32 = 4;

/// Maximum number of processes.
pub const KERN_MAXPROC: i32 = 6;

/// Maximum number of vnodes.
pub const KERN_MAXVNODES: i32 = 5;

/// Maximum number of files.
pub const KERN_MAXFILES: i32 = 7;

/// Hostname.
pub const KERN_HOSTNAME: i32 = 10;

/// Domain name.
pub const KERN_DOMAINNAME: i32 = 22;

// ---------------------------------------------------------------------------
// Limits
// ---------------------------------------------------------------------------

/// Maximum number of name components in a `sysctl()` path. Linux defined
/// this as `CTL_MAXNAME` in `<linux/sysctl.h>` — paths deeper than this
/// are rejected with `ENOTDIR`.
pub const CTL_MAXNAME: i32 = 10;

/// Maximum size we'll accept for the new-value buffer length (`newlen`).
/// Linux's old `do_sysctl` capped this at `PAGE_SIZE` (4096) per call;
/// we use 64 KiB to be generous to any caller that still uses the
/// interface (e.g., legacy network code writing a route table).
pub const SYSCTL_MAX_NEWLEN: usize = 64 * 1024;

// ---------------------------------------------------------------------------
// sysctl struct
// ---------------------------------------------------------------------------

/// Arguments for the `sysctl()` system call.
///
/// Matches Linux's `struct __sysctl_args` layout (`<linux/sysctl.h>`).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SysctlArgs {
    /// Array of name components.
    pub name: *mut i32,
    /// Number of name components.
    pub nlen: i32,
    /// Buffer for old value.
    pub oldval: *mut u8,
    /// Size of old value buffer.
    pub oldlenp: *mut usize,
    /// Buffer for new value.
    pub newval: *mut u8,
    /// Size of new value.
    pub newlen: usize,
}

// ---------------------------------------------------------------------------
// sysctl()
// ---------------------------------------------------------------------------

/// Read or write system parameters.
///
/// Linux semantics (matches pre-5.5 `kernel/sysctl_binary.c::do_sysctl`):
/// - NULL `args` → EFAULT.
/// - `args.nlen <= 0` → EINVAL (must have at least one name component).
/// - `args.nlen > CTL_MAXNAME (10)` → ENOTDIR (path too deep).
/// - `args.name == NULL && args.nlen > 0` → EFAULT.
/// - `args.oldval != NULL && args.oldlenp == NULL` → EFAULT (can't
///   tell us how big the buffer is).
/// - `args.newval == NULL && args.newlen != 0` → EFAULT (new-size set
///   but no buffer to read from).
/// - `args.newval != NULL && args.newlen == 0` → EINVAL (Linux's
///   `do_sysctl` rejects zero-length writes with a buffer).
/// - `args.newlen > SYSCTL_MAX_NEWLEN (65536)` → E2BIG.
/// - Valid → ENOSYS (matches Linux 5.5+).
///
/// All caller-supplied fields are read via `core::ptr::read_unaligned`
/// so an alignment-1 `args` pointer doesn't UB.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn sysctl(args: *mut SysctlArgs) -> i32 {
    if args.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    // SAFETY: We've verified args is non-null; read_unaligned tolerates any
    // alignment. The caller's struct may live in a memory region we don't
    // own, but reading it cannot violate Rust's aliasing rules because the
    // pointer is to a Copy POD type.
    let a = unsafe { core::ptr::read_unaligned(args) };

    if a.nlen <= 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    if a.nlen > CTL_MAXNAME {
        errno::set_errno(errno::ENOTDIR);
        return -1;
    }
    if a.name.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    if !a.oldval.is_null() && a.oldlenp.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    if a.newval.is_null() && a.newlen != 0 {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    if !a.newval.is_null() && a.newlen == 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    if a.newlen > SYSCTL_MAX_NEWLEN {
        errno::set_errno(errno::E2BIG);
        return -1;
    }

    // Linux 5.5+ removed sysctl; every well-formed call returns ENOSYS.
    errno::set_errno(errno::ENOSYS);
    -1
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_args() -> SysctlArgs {
        SysctlArgs {
            name: core::ptr::null_mut(),
            nlen: 0,
            oldval: core::ptr::null_mut(),
            oldlenp: core::ptr::null_mut(),
            newval: core::ptr::null_mut(),
            newlen: 0,
        }
    }

    #[test]
    fn test_null_args_efault() {
        errno::set_errno(errno::EBADF);
        let r = sysctl(core::ptr::null_mut());
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_zero_nlen_einval() {
        let mut a = make_args();
        a.nlen = 0;
        errno::set_errno(errno::EBADF);
        let r = sysctl(&mut a);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_negative_nlen_einval() {
        let mut a = make_args();
        a.nlen = -1;
        errno::set_errno(errno::EBADF);
        let r = sysctl(&mut a);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_huge_nlen_enotdir() {
        let mut name = [0i32; 16];
        let mut a = make_args();
        a.name = name.as_mut_ptr();
        a.nlen = 11; // > CTL_MAXNAME
        errno::set_errno(errno::EBADF);
        let r = sysctl(&mut a);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOTDIR);
    }

    #[test]
    fn test_nlen_at_max_ok() {
        let mut name = [CTL_KERN; 10];
        let mut a = make_args();
        a.name = name.as_mut_ptr();
        a.nlen = CTL_MAXNAME;
        errno::set_errno(errno::EBADF);
        let r = sysctl(&mut a);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_null_name_with_positive_nlen_efault() {
        let mut a = make_args();
        a.nlen = 1;
        a.name = core::ptr::null_mut();
        errno::set_errno(errno::EBADF);
        let r = sysctl(&mut a);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_oldval_without_oldlenp_efault() {
        let mut name = [CTL_KERN, KERN_OSTYPE];
        let mut buf = [0u8; 64];
        let mut a = make_args();
        a.name = name.as_mut_ptr();
        a.nlen = 2;
        a.oldval = buf.as_mut_ptr();
        a.oldlenp = core::ptr::null_mut();
        errno::set_errno(errno::EBADF);
        let r = sysctl(&mut a);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_oldval_with_oldlenp_reaches_enosys() {
        let mut name = [CTL_KERN, KERN_OSTYPE];
        let mut buf = [0u8; 64];
        let mut buf_len: usize = 64;
        let mut a = make_args();
        a.name = name.as_mut_ptr();
        a.nlen = 2;
        a.oldval = buf.as_mut_ptr();
        a.oldlenp = &raw mut buf_len;
        errno::set_errno(errno::EBADF);
        let r = sysctl(&mut a);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_null_newval_with_nonzero_newlen_efault() {
        let mut name = [CTL_KERN, KERN_HOSTNAME];
        let mut a = make_args();
        a.name = name.as_mut_ptr();
        a.nlen = 2;
        a.newval = core::ptr::null_mut();
        a.newlen = 16;
        errno::set_errno(errno::EBADF);
        let r = sysctl(&mut a);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_nonnull_newval_zero_newlen_einval() {
        let mut name = [CTL_KERN, KERN_HOSTNAME];
        let mut buf = [0u8; 1];
        let mut a = make_args();
        a.name = name.as_mut_ptr();
        a.nlen = 2;
        a.newval = buf.as_mut_ptr();
        a.newlen = 0;
        errno::set_errno(errno::EBADF);
        let r = sysctl(&mut a);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_huge_newlen_e2big() {
        let mut name = [CTL_KERN, KERN_HOSTNAME];
        let mut buf = [0u8; 4];
        let mut a = make_args();
        a.name = name.as_mut_ptr();
        a.nlen = 2;
        a.newval = buf.as_mut_ptr();
        a.newlen = SYSCTL_MAX_NEWLEN + 1;
        errno::set_errno(errno::EBADF);
        let r = sysctl(&mut a);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::E2BIG);
    }

    #[test]
    fn test_newlen_at_max_reaches_enosys() {
        let mut name = [CTL_KERN, KERN_HOSTNAME];
        let mut buf = [0u8; 4];
        let mut a = make_args();
        a.name = name.as_mut_ptr();
        a.nlen = 2;
        a.newval = buf.as_mut_ptr();
        a.newlen = SYSCTL_MAX_NEWLEN;
        errno::set_errno(errno::EBADF);
        let r = sysctl(&mut a);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_read_only_call_reaches_enosys() {
        // Pure read with no write side: name + oldval/oldlenp set, newval
        // NULL, newlen 0. This is glibc's typical sysctl(KERN_OSRELEASE).
        let mut name = [CTL_KERN, KERN_OSRELEASE];
        let mut buf = [0u8; 64];
        let mut buf_len: usize = 64;
        let mut a = make_args();
        a.name = name.as_mut_ptr();
        a.nlen = 2;
        a.oldval = buf.as_mut_ptr();
        a.oldlenp = &raw mut buf_len;
        a.newval = core::ptr::null_mut();
        a.newlen = 0;
        errno::set_errno(errno::EBADF);
        let r = sysctl(&mut a);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_size_only_query_reaches_enosys() {
        // Caller asking "how big is this value?" with oldval==NULL and
        // *oldlenp==0. Linux's API contract: writes the required size into
        // *oldlenp and returns 0. We return ENOSYS.
        let mut name = [CTL_KERN, KERN_VERSION];
        let mut buf_len: usize = 0;
        let mut a = make_args();
        a.name = name.as_mut_ptr();
        a.nlen = 2;
        a.oldval = core::ptr::null_mut();
        a.oldlenp = &raw mut buf_len;
        errno::set_errno(errno::EBADF);
        let r = sysctl(&mut a);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    // ---- Real-world workflow tests ----

    #[test]
    fn test_glibc_sysctl_feature_probe_workflow() {
        // glibc's __sysctl() wrapper (in sysdeps/unix/sysv/linux/sysctl.c
        // up to glibc 2.31) calls sysctl(KERN_OSRELEASE) as a feature
        // probe before deciding whether to fall back to reading
        // /proc/sys/kernel/osrelease. On ENOSYS, it switches to the
        // /proc fallback. We give it a clean ENOSYS.
        let mut name = [CTL_KERN, KERN_OSRELEASE];
        let mut buf = [0u8; 64];
        let mut buf_len: usize = 64;
        let mut a = make_args();
        a.name = name.as_mut_ptr();
        a.nlen = 2;
        a.oldval = buf.as_mut_ptr();
        a.oldlenp = &raw mut buf_len;
        errno::set_errno(errno::EBADF);
        let r = sysctl(&mut a);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_legacy_bsd_hostname_setter_workflow() {
        // Old BSD-derived tools (sendmail's hostname configuration,
        // pre-systemd /etc/rc scripts on some embedded distros) call
        // sysctl(CTL_KERN, KERN_HOSTNAME) with a write buffer. We
        // return ENOSYS so they fall back to gethostname/sethostname.
        let mut name = [CTL_KERN, KERN_HOSTNAME];
        let new_hostname = b"example.local";
        let mut a = make_args();
        a.name = name.as_mut_ptr();
        a.nlen = 2;
        a.newval = new_hostname.as_ptr() as *mut u8;
        a.newlen = new_hostname.len();
        errno::set_errno(errno::EBADF);
        let r = sysctl(&mut a);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_systemtap_kernel_version_probe_workflow() {
        // SystemTap and some kernel-version probes (Valgrind's
        // VKI_KERN_VERSION read on Linux, older perf-tools-unstable
        // builds) call sysctl(CTL_KERN, KERN_VERSION) with NULL oldval
        // and *oldlenp==0 to size-query the result first. We return
        // ENOSYS and they fall back to /proc/version.
        let mut name = [CTL_KERN, KERN_VERSION];
        let mut buf_len: usize = 0;
        let mut a = make_args();
        a.name = name.as_mut_ptr();
        a.nlen = 2;
        a.oldlenp = &raw mut buf_len;
        errno::set_errno(errno::EBADF);
        let r = sysctl(&mut a);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_malformed_args_detected_before_enosys() {
        // A caller passing junk (huge nlen) must see ENOTDIR, not
        // ENOSYS. The validator runs before the kernel-not-implemented
        // path so that genuinely malformed inputs surface clearly.
        let mut name = [0i32; 32];
        let mut a = make_args();
        a.name = name.as_mut_ptr();
        a.nlen = 32;
        errno::set_errno(errno::EBADF);
        let r = sysctl(&mut a);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOTDIR);
    }

    // ---- POSIX errno-preserved-on-validation-success regression ----

    #[test]
    fn test_errno_set_to_enosys_on_validation_success() {
        let mut name = [CTL_KERN];
        let mut a = make_args();
        a.name = name.as_mut_ptr();
        a.nlen = 1;
        errno::set_errno(errno::EBADF);
        let r = sysctl(&mut a);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    // ---- Existing tests preserved ----

    #[test]
    fn test_ctl_constants_distinct() {
        let ctls = [CTL_KERN, CTL_VM, CTL_NET, CTL_FS, CTL_DEBUG, CTL_DEV];
        for i in 0..ctls.len() {
            for j in (i + 1)..ctls.len() {
                assert_ne!(ctls[i], ctls[j]);
            }
        }
    }

    #[test]
    fn test_kern_constants_distinct() {
        let kerns = [
            KERN_OSTYPE, KERN_OSRELEASE, KERN_OSREV,
            KERN_VERSION, KERN_MAXVNODES, KERN_MAXPROC,
            KERN_MAXFILES, KERN_HOSTNAME, KERN_DOMAINNAME,
        ];
        for i in 0..kerns.len() {
            for j in (i + 1)..kerns.len() {
                assert_ne!(kerns[i], kerns[j]);
            }
        }
    }

    #[test]
    fn test_sysctl_args_size() {
        assert!(core::mem::size_of::<SysctlArgs>() > 0);
    }

    #[test]
    fn test_ctl_maxname_value() {
        assert_eq!(CTL_MAXNAME, 10);
    }
}
