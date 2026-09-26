//! `<sys/sysctl.h>` — system parameters by numeric name: gone from Linux.
//!
//! Linux removed the `sysctl(2)` system call in 5.5 (April 2020), and glibc
//! removed the `sysctl` function and `<sys/sysctl.h>` in 2.32, keeping the
//! function only for old binaries -- as a stub that answers `ENOSYS`,
//! whatever it is given (glibc 2.39, `sysdeps/unix/sysv/linux/sysctl.c`).
//! [`sysctl`] is that stub, with glibc's six arguments. A program built
//! against an old `<sys/sysctl.h>` finds it, gets `ENOSYS`, and falls back to
//! `/proc/sys` -- as it must on any current Linux. The constants stay for
//! programs that still name them.
//!
//! ## What changed on 2026-09-26
//!
//! `sysctl` took one argument, a `struct __sysctl_args *` -- the removed
//! system call's, not the function's -- so a C caller's first argument, its
//! name array, was read as that structure. It then "validated" it with
//! checks no kernel made in that combination (`EFAULT` for NULL pointers,
//! `EINVAL` for an empty name where the old kernel said `ENOTDIR`, `E2BIG`
//! past a limit of its own) before answering `ENOSYS`.

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

/// The most name components a `sysctl` path had (`<linux/sysctl.h>`'s
/// `CTL_MAXNAME`).
pub const CTL_MAXNAME: i32 = 10;

// ---------------------------------------------------------------------------
// sysctl struct
// ---------------------------------------------------------------------------

/// The removed `sysctl(2)` system call's argument: Linux's
/// `struct __sysctl_args` (`<linux/sysctl.h>`), `__unused` included.
///
/// Nothing here takes one any more -- the [`sysctl`] function has glibc's
/// six arguments -- but a program that declares the structure itself finds
/// it the kernel's size.
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
    /// Reserved (`unsigned long __unused[4]`).
    pub __unused: [u64; 4],
}

// ---------------------------------------------------------------------------
// sysctl()
// ---------------------------------------------------------------------------

/// `sysctl` — read or write a system parameter by numeric name: removed.
///
/// glibc 2.39's compat stub: `-1` with `ENOSYS`, reading none of its
/// arguments -- Linux has had no `sysctl` system call since 5.5, so there is
/// nothing a checked argument could be handed to, and no fault for a NULL
/// one to be substituted for. Callers are expected to fall back to
/// `/proc/sys`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn sysctl(
    _name: *mut i32,
    _nlen: i32,
    _oldval: *mut u8,
    _oldlenp: *mut usize,
    _newval: *mut u8,
    _newlen: usize,
) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use core::ptr::null_mut;

    #[test]
    fn sysctl_is_enosys_whatever_it_is_given() {
        let mut name = [CTL_KERN, KERN_OSRELEASE];
        let mut deep = [0i32; 32];
        let mut buf = [0xAAu8; 64];
        let mut len: usize = 64;
        let n = name.as_mut_ptr();
        let old = buf.as_mut_ptr();
        let lenp = &raw mut len;
        // What the old kernel and this libc's validator refused, and what
        // they accepted: all the same now.
        let shapes: [(*mut i32, i32, *mut u8, *mut usize, *mut u8, usize); 8] = [
            (null_mut(), 0, null_mut(), null_mut(), null_mut(), 0),
            (null_mut(), 2, null_mut(), null_mut(), null_mut(), 0),
            (n, 0, null_mut(), null_mut(), null_mut(), 0),
            (n, -1, null_mut(), null_mut(), null_mut(), 0),
            (deep.as_mut_ptr(), 32, null_mut(), null_mut(), null_mut(), 0),
            (n, 2, old, null_mut(), null_mut(), 0),
            (n, 2, null_mut(), null_mut(), null_mut(), 1 << 20),
            (n, 2, old, lenp, null_mut(), 0),
        ];
        for (i, (nm, nlen, oldval, oldlenp, newval, newlen)) in shapes.into_iter().enumerate() {
            errno::set_errno(errno::EBADF);
            assert_eq!(
                sysctl(nm, nlen, oldval, oldlenp, newval, newlen),
                -1,
                "shape {i}"
            );
            assert_eq!(errno::get_errno(), errno::ENOSYS, "shape {i}");
        }
        assert_eq!(len, 64, "*oldlenp is not written");
        assert!(buf.iter().all(|&b| b == 0xAA), "nor is the buffer");
    }

    #[test]
    fn a_kernel_release_read_falls_back() {
        // An old program's read of the kernel release,
        // sysctl(CTL_KERN, KERN_OSRELEASE): ENOSYS sends it to
        // /proc/sys/kernel/osrelease, as on any current Linux.
        let mut name = [CTL_KERN, KERN_OSRELEASE];
        let mut buf = [0u8; 64];
        let mut len = buf.len();
        errno::set_errno(0);
        let r = sysctl(
            name.as_mut_ptr(),
            2,
            buf.as_mut_ptr(),
            &raw mut len,
            null_mut(),
            0,
        );
        assert_eq!((r, errno::get_errno()), (-1, errno::ENOSYS));
    }

    #[test]
    fn a_hostname_write_falls_back() {
        // Old BSD-derived tools write KERN_HOSTNAME; ENOSYS sends them to
        // sethostname.
        let mut name = [CTL_KERN, KERN_HOSTNAME];
        let mut host = *b"example.local";
        errno::set_errno(0);
        let r = sysctl(
            name.as_mut_ptr(),
            2,
            null_mut(),
            null_mut(),
            host.as_mut_ptr(),
            host.len(),
        );
        assert_eq!((r, errno::get_errno()), (-1, errno::ENOSYS));
        assert_eq!(&host, b"example.local");
    }

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
            KERN_OSTYPE,
            KERN_OSRELEASE,
            KERN_OSREV,
            KERN_VERSION,
            KERN_MAXVNODES,
            KERN_MAXPROC,
            KERN_MAXFILES,
            KERN_HOSTNAME,
            KERN_DOMAINNAME,
        ];
        for i in 0..kerns.len() {
            for j in (i + 1)..kerns.len() {
                assert_ne!(kerns[i], kerns[j]);
            }
        }
    }

    #[test]
    fn sysctl_args_is_the_kernels_structure() {
        // Six fields and `unsigned long __unused[4]`: 80 bytes.
        assert_eq!(size_of::<SysctlArgs>(), 80);
        assert_eq!(core::mem::offset_of!(SysctlArgs, newlen), 40);
    }

    #[test]
    fn test_ctl_maxname_value() {
        assert_eq!(CTL_MAXNAME, 10);
    }
}
