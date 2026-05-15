//! `<sys/mount.h>` — mount/unmount filesystem.
//!
//! Re-exports `mount()`, `umount()`, and `umount2()` from the
//! `process` module and defines `MS_*` mount flags.

// ---------------------------------------------------------------------------
// Re-exports
// ---------------------------------------------------------------------------

pub use crate::process::mount;
pub use crate::process::umount;
pub use crate::process::umount2;

// ---------------------------------------------------------------------------
// Mount flags (MS_*)
// ---------------------------------------------------------------------------

/// Mount read-only.
pub const MS_RDONLY: u64 = 1;

/// Ignore suid and sgid bits.
pub const MS_NOSUID: u64 = 2;

/// Disallow access to device special files.
pub const MS_NODEV: u64 = 4;

/// Disallow program execution.
pub const MS_NOEXEC: u64 = 8;

/// Writes are synced immediately.
pub const MS_SYNCHRONOUS: u64 = 16;

/// Alter flags of a mounted filesystem.
pub const MS_REMOUNT: u64 = 32;

/// Allow mandatory locks.
pub const MS_MANDLOCK: u64 = 64;

/// Directory modifications are synchronous.
pub const MS_DIRSYNC: u64 = 128;

/// Do not follow symlinks.
pub const MS_NOSYMFOLLOW: u64 = 256;

/// Do not update access times.
pub const MS_NOATIME: u64 = 1024;

/// Do not update directory access times.
pub const MS_NODIRATIME: u64 = 2048;

/// Bind mount.
pub const MS_BIND: u64 = 4096;

/// Move a subtree.
pub const MS_MOVE: u64 = 8192;

/// Recursive mount.
pub const MS_REC: u64 = 16384;

/// Silent mount (suppress printk messages).
pub const MS_SILENT: u64 = 32768;

/// VFS does not apply umask.
pub const MS_POSIXACL: u64 = 1 << 16;

/// Unbindable mount.
pub const MS_UNBINDABLE: u64 = 1 << 17;

/// Private mount.
pub const MS_PRIVATE: u64 = 1 << 18;

/// Slave mount.
pub const MS_SLAVE: u64 = 1 << 19;

/// Shared mount.
pub const MS_SHARED: u64 = 1 << 20;

/// Update atime relative to mtime/ctime.
pub const MS_RELATIME: u64 = 1 << 21;

/// Kernel internal: this is a kern_mount call.
pub const MS_KERNMOUNT: u64 = 1 << 22;

/// Kernel internal: update inode I_version field.
pub const MS_I_VERSION: u64 = 1 << 23;

/// Update atime on access (strict atime).
pub const MS_STRICTATIME: u64 = 1 << 24;

/// Change to lazy time.
pub const MS_LAZYTIME: u64 = 1 << 25;

// ---------------------------------------------------------------------------
// Umount2 flags (MNT_*)
// ---------------------------------------------------------------------------

/// Force unmount (even if busy).
pub const MNT_FORCE: i32 = 1;

/// Just detach from the tree (lazy unmount).
pub const MNT_DETACH: i32 = 2;

/// Mark for expiry.
pub const MNT_EXPIRE: i32 = 4;

/// Don't follow symlinks on umount.
pub const UMOUNT_NOFOLLOW: i32 = 8;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Mount flags
    // -----------------------------------------------------------------------

    #[test]
    fn test_ms_rdonly() {
        assert_eq!(MS_RDONLY, 1);
    }

    #[test]
    fn test_ms_nosuid() {
        assert_eq!(MS_NOSUID, 2);
    }

    #[test]
    fn test_ms_flags_are_powers_of_two() {
        let flags = [
            MS_RDONLY, MS_NOSUID, MS_NODEV, MS_NOEXEC,
            MS_SYNCHRONOUS, MS_REMOUNT, MS_MANDLOCK, MS_DIRSYNC,
            MS_NOATIME, MS_NODIRATIME, MS_BIND, MS_MOVE,
            MS_REC, MS_SILENT,
        ];
        for &f in &flags {
            assert_ne!(f, 0);
            assert_eq!(f & (f - 1), 0, "MS flag 0x{f:X} not a power of two");
        }
    }

    #[test]
    fn test_ms_flags_distinct() {
        let flags = [
            MS_RDONLY, MS_NOSUID, MS_NODEV, MS_NOEXEC,
            MS_SYNCHRONOUS, MS_REMOUNT, MS_MANDLOCK, MS_DIRSYNC,
            MS_NOATIME, MS_NODIRATIME, MS_BIND, MS_MOVE,
            MS_REC, MS_SILENT, MS_POSIXACL, MS_UNBINDABLE,
            MS_PRIVATE, MS_SLAVE, MS_SHARED, MS_RELATIME,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j], "MS flags must be distinct");
            }
        }
    }

    // -----------------------------------------------------------------------
    // Umount flags
    // -----------------------------------------------------------------------

    #[test]
    fn test_mnt_force() {
        assert_eq!(MNT_FORCE, 1);
    }

    #[test]
    fn test_mnt_detach() {
        assert_eq!(MNT_DETACH, 2);
    }

    #[test]
    fn test_umount_flags_distinct() {
        let flags = [MNT_FORCE, MNT_DETACH, MNT_EXPIRE, UMOUNT_NOFOLLOW];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    // -----------------------------------------------------------------------
    // Function stubs
    // -----------------------------------------------------------------------

    #[test]
    fn test_mount_stub() {
        let ret = mount(
            b"none\0".as_ptr(),
            b"/mnt\0".as_ptr(),
            b"tmpfs\0".as_ptr(),
            0,
            core::ptr::null(),
        );
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_umount_stub() {
        let ret = umount(b"/mnt\0".as_ptr());
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_umount2_stub() {
        let ret = umount2(b"/mnt\0".as_ptr(), MNT_FORCE);
        assert_eq!(ret, -1);
    }
}
