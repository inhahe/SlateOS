//! `<mntent.h>` — mount table entry functions.
//!
//! Re-exports mount-related types and functions from the `unistd`
//! module and adds mount option string constants.

// ---------------------------------------------------------------------------
// Re-exports
// ---------------------------------------------------------------------------

pub use crate::unistd::Mntent;
pub use crate::unistd::setmntent;
pub use crate::unistd::getmntent;
pub use crate::unistd::getmntent_r;
pub use crate::unistd::endmntent;
pub use crate::unistd::hasmntopt;

// ---------------------------------------------------------------------------
// Standard mount table paths
// ---------------------------------------------------------------------------

/// Path to the mounted filesystem table.
pub const MOUNTED: &[u8] = b"/etc/mtab\0";

/// Path to the filesystem description table.
pub const MNTTAB: &[u8] = b"/etc/fstab\0";

// ---------------------------------------------------------------------------
// Standard mount option strings
// ---------------------------------------------------------------------------

/// Read-only filesystem.
pub const MNTOPT_RO: &[u8] = b"ro\0";

/// Read-write filesystem.
pub const MNTOPT_RW: &[u8] = b"rw\0";

/// Set-UID allowed.
pub const MNTOPT_SUID: &[u8] = b"suid\0";

/// No set-UID allowed.
pub const MNTOPT_NOSUID: &[u8] = b"nosuid\0";

/// No device files allowed.
pub const MNTOPT_NODEV: &[u8] = b"nodev\0";

/// No program execution allowed.
pub const MNTOPT_NOEXEC: &[u8] = b"noexec\0";

/// Synchronous I/O.
pub const MNTOPT_SYNC: &[u8] = b"sync\0";

/// Do not auto-mount.
pub const MNTOPT_NOAUTO: &[u8] = b"noauto\0";

/// All users may mount.
pub const MNTOPT_USER: &[u8] = b"user\0";

/// Filesystem defaults.
pub const MNTOPT_DEFAULTS: &[u8] = b"defaults\0";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mounted_path() {
        assert_eq!(MOUNTED, b"/etc/mtab\0");
    }

    #[test]
    fn test_mnttab_path() {
        assert_eq!(MNTTAB, b"/etc/fstab\0");
    }

    #[test]
    fn test_mount_options_null_terminated() {
        let opts: &[&[u8]] = &[
            MNTOPT_RO, MNTOPT_RW, MNTOPT_SUID, MNTOPT_NOSUID,
            MNTOPT_NODEV, MNTOPT_NOEXEC, MNTOPT_SYNC,
            MNTOPT_NOAUTO, MNTOPT_USER, MNTOPT_DEFAULTS,
        ];
        for opt in opts {
            assert!(
                opt.last() == Some(&0),
                "mount option must be null-terminated"
            );
        }
    }

    #[test]
    fn test_setmntent_accessible() {
        let ret = setmntent(MOUNTED.as_ptr(), b"r\0".as_ptr());
        assert!(ret.is_null());
    }

    #[test]
    fn test_getmntent_accessible() {
        let ret = getmntent(core::ptr::null_mut());
        assert!(ret.is_null());
    }

    #[test]
    fn test_endmntent_accessible() {
        let ret = endmntent(core::ptr::null_mut());
        assert_eq!(ret, 1);
    }

    #[test]
    fn test_hasmntopt_accessible() {
        let ret = hasmntopt(core::ptr::null(), MNTOPT_RW.as_ptr());
        assert!(ret.is_null());
    }

    #[test]
    fn test_mntent_struct_size() {
        assert!(core::mem::size_of::<Mntent>() > 0);
    }
}
