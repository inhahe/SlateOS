//! `<sys/sysctl.h>` — system control (deprecated Linux interface).
//!
//! This interface is deprecated in Linux; modern code should use
//! `/proc/sys` or `sysctl(8)` instead.  Provided for compatibility
//! with older programs.

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
// sysctl struct
// ---------------------------------------------------------------------------

/// Arguments for the `sysctl()` system call.
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
/// Stub — always returns -1 with `ENOSYS`.  Use `/proc/sys` instead.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn sysctl(_args: *mut SysctlArgs) -> i32 {
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
    fn test_sysctl_stub() {
        let ret = sysctl(core::ptr::null_mut());
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_sysctl_args_size() {
        assert!(core::mem::size_of::<SysctlArgs>() > 0);
    }
}
