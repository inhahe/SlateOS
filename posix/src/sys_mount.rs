//! `<sys/mount.h>` — mount/unmount filesystem.
//!
//! Re-exports `mount()`, `umount()`, and `umount2()` from the
//! `process` module and defines `MS_*` mount flags; and has glibc's new
//! mount API -- `fsopen` ... `mount_setattr` -- each answering `ENOSYS`, as
//! glibc's does on a kernel without it (below).

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

/// A filesystem that may not be mounted from userspace.  It is the one flag
/// `mount(2)` refuses outright: `path_mount` returns `EINVAL` for it
/// (fs/namespace.c:3601) before anything but the target has been examined.
pub const MS_NOUSER: u64 = 1 << 31;

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

// ---------------------------------------------------------------------------
// The new mount API (glibc 2.36): answered ENOSYS
// ---------------------------------------------------------------------------
//
// `fsopen`, `fsconfig`, `fsmount`, `move_mount`, `fspick`, `open_tree` and
// `mount_setattr` are glibc's wrappers for Linux 5.2's and 5.12's system
// calls, which mount by building a filesystem context on a descriptor rather
// than in one call. SlateOS's kernel has no such calls, so each answers
// ENOSYS -- what glibc's wrapper answers on a Linux without them -- before
// looking at its arguments, as a missing system call does; and a program
// falls back to `mount(2)`, which this library has, as util-linux's libmount
// and systemd do on such a kernel. The flags and `struct mount_attr` are
// glibc's, so that a program that names them compiles.

/// `fsopen`: the context's descriptor is close-on-exec.
pub const FSOPEN_CLOEXEC: u32 = 0x0000_0001;
/// `fsmount`: the mount's descriptor is close-on-exec.
pub const FSMOUNT_CLOEXEC: u32 = 0x0000_0001;
/// `fspick`: close-on-exec.
pub const FSPICK_CLOEXEC: u32 = 0x0000_0001;
/// `fspick`: do not follow a final symbolic link.
pub const FSPICK_SYMLINK_NOFOLLOW: u32 = 0x0000_0002;
/// `fspick`: do not trigger an automount.
pub const FSPICK_NO_AUTOMOUNT: u32 = 0x0000_0004;
/// `fspick`: an empty path names the descriptor itself.
pub const FSPICK_EMPTY_PATH: u32 = 0x0000_0008;
/// `move_mount`: follow symbolic links on the source path.
pub const MOVE_MOUNT_F_SYMLINKS: u32 = 0x0000_0001;
/// `move_mount`: follow automounts on the source path.
pub const MOVE_MOUNT_F_AUTOMOUNTS: u32 = 0x0000_0002;
/// `move_mount`: an empty source path names the descriptor.
pub const MOVE_MOUNT_F_EMPTY_PATH: u32 = 0x0000_0004;
/// `move_mount`: follow symbolic links on the target path.
pub const MOVE_MOUNT_T_SYMLINKS: u32 = 0x0000_0010;
/// `move_mount`: follow automounts on the target path.
pub const MOVE_MOUNT_T_AUTOMOUNTS: u32 = 0x0000_0020;
/// `move_mount`: an empty target path names the descriptor.
pub const MOVE_MOUNT_T_EMPTY_PATH: u32 = 0x0000_0040;
/// `move_mount`: set the sharing group instead of moving.
pub const MOVE_MOUNT_SET_GROUP: u32 = 0x0000_0100;
/// `move_mount`: mount beneath the top mount.
pub const MOVE_MOUNT_BENEATH: u32 = 0x0000_0200;
/// `open_tree`: clone the tree and attach the clone.
pub const OPEN_TREE_CLONE: u32 = 1;
/// `open_tree`: close-on-exec -- `O_CLOEXEC`.
pub const OPEN_TREE_CLOEXEC: u32 = 0o2_000_000;
/// Mount read-only.
pub const MOUNT_ATTR_RDONLY: u32 = 0x0000_0001;
/// Ignore set-user-ID and set-group-ID bits.
pub const MOUNT_ATTR_NOSUID: u32 = 0x0000_0002;
/// No device files.
pub const MOUNT_ATTR_NODEV: u32 = 0x0000_0004;
/// No program execution.
pub const MOUNT_ATTR_NOEXEC: u32 = 0x0000_0008;
/// The bits that say how access times are kept.
pub const MOUNT_ATTR__ATIME: u32 = 0x0000_0070;
/// Access times relative to the modification and change times.
pub const MOUNT_ATTR_RELATIME: u32 = 0x0000_0000;
/// No access times.
pub const MOUNT_ATTR_NOATIME: u32 = 0x0000_0010;
/// Every access time.
pub const MOUNT_ATTR_STRICTATIME: u32 = 0x0000_0020;
/// No access times on directories.
pub const MOUNT_ATTR_NODIRATIME: u32 = 0x0000_0080;
/// An ID-mapped mount, by `struct mount_attr`'s `userns_fd`.
pub const MOUNT_ATTR_IDMAP: u32 = 0x0010_0000;
/// No symbolic links followed.
pub const MOUNT_ATTR_NOSYMFOLLOW: u32 = 0x0020_0000;
/// `sizeof (struct mount_attr)` as first published.
pub const MOUNT_ATTR_SIZE_VER0: u32 = 32;
/// `fsconfig` commands: set a flag; a string; a blob; a path; an empty path;
/// a descriptor; create the superblock; reconfigure it; create, refusing to
/// reuse one.
pub const FSCONFIG_SET_FLAG: u32 = 0;
pub const FSCONFIG_SET_STRING: u32 = 1;
pub const FSCONFIG_SET_BINARY: u32 = 2;
pub const FSCONFIG_SET_PATH: u32 = 3;
pub const FSCONFIG_SET_PATH_EMPTY: u32 = 4;
pub const FSCONFIG_SET_FD: u32 = 5;
pub const FSCONFIG_CMD_CREATE: u32 = 6;
pub const FSCONFIG_CMD_RECONFIGURE: u32 = 7;
pub const FSCONFIG_CMD_CREATE_EXCL: u32 = 8;

/// glibc's `struct mount_attr`, `mount_setattr`'s argument: 32 bytes.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MountAttr {
    /// Attributes to set.
    pub attr_set: u64,
    /// Attributes to clear.
    pub attr_clr: u64,
    /// The mount's propagation type.
    pub propagation: u64,
    /// The user namespace of an ID-mapped mount.
    pub userns_fd: u64,
}

/// What every call of the new mount API answers here: -1, `ENOSYS`.
fn no_new_mount_api() -> i32 {
    crate::errno::set_errno(crate::errno::ENOSYS);
    -1
}

/// `fsopen(fs_name, flags)`: -1, `ENOSYS` (see above).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fsopen(_fs_name: *const u8, _flags: u32) -> i32 {
    no_new_mount_api()
}

/// `fsmount(fd, flags, attr_flags)`: -1, `ENOSYS`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fsmount(_fd: i32, _flags: u32, _attr_flags: u32) -> i32 {
    no_new_mount_api()
}

/// `move_mount(from_dfd, from_path, to_dfd, to_path, flags)`: -1, `ENOSYS`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn move_mount(
    _from_dfd: i32,
    _from_path: *const u8,
    _to_dfd: i32,
    _to_path: *const u8,
    _flags: u32,
) -> i32 {
    no_new_mount_api()
}

/// `fsconfig(fd, cmd, key, value, aux)`: -1, `ENOSYS`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fsconfig(
    _fd: i32,
    _cmd: u32,
    _key: *const u8,
    _value: *const core::ffi::c_void,
    _aux: i32,
) -> i32 {
    no_new_mount_api()
}

/// `fspick(dfd, path, flags)`: -1, `ENOSYS`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fspick(_dfd: i32, _path: *const u8, _flags: u32) -> i32 {
    no_new_mount_api()
}

/// `open_tree(dfd, filename, flags)`: -1, `ENOSYS`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn open_tree(_dfd: i32, _filename: *const u8, _flags: u32) -> i32 {
    no_new_mount_api()
}

/// `mount_setattr(dfd, path, flags, uattr, usize)`: -1, `ENOSYS`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn mount_setattr(
    _dfd: i32,
    _path: *const u8,
    _flags: u32,
    _uattr: *mut MountAttr,
    _usize: usize,
) -> i32 {
    no_new_mount_api()
}

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
            MS_RDONLY,
            MS_NOSUID,
            MS_NODEV,
            MS_NOEXEC,
            MS_SYNCHRONOUS,
            MS_REMOUNT,
            MS_MANDLOCK,
            MS_DIRSYNC,
            MS_NOATIME,
            MS_NODIRATIME,
            MS_BIND,
            MS_MOVE,
            MS_REC,
            MS_SILENT,
        ];
        for &f in &flags {
            assert_ne!(f, 0);
            assert_eq!(f & (f - 1), 0, "MS flag 0x{f:X} not a power of two");
        }
    }

    #[test]
    fn test_ms_flags_distinct() {
        let flags = [
            MS_RDONLY,
            MS_NOSUID,
            MS_NODEV,
            MS_NOEXEC,
            MS_SYNCHRONOUS,
            MS_REMOUNT,
            MS_MANDLOCK,
            MS_DIRSYNC,
            MS_NOATIME,
            MS_NODIRATIME,
            MS_BIND,
            MS_MOVE,
            MS_REC,
            MS_SILENT,
            MS_POSIXACL,
            MS_UNBINDABLE,
            MS_PRIVATE,
            MS_SLAVE,
            MS_SHARED,
            MS_RELATIME,
            MS_NOUSER,
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

    /// The new mount API answers -1, `ENOSYS`, whatever it is given: what
    /// glibc's wrapper answers on a kernel without the calls, before any
    /// argument is looked at.
    #[test]
    fn the_new_mount_api_is_enosys() {
        /// A call's name, and the call.
        type Call = (&'static str, fn() -> i32);
        let calls: [Call; 7] = [
            ("fsopen", || fsopen(b"ext4\0".as_ptr(), FSOPEN_CLOEXEC)),
            ("fsmount", || fsmount(3, FSMOUNT_CLOEXEC, MOUNT_ATTR_RDONLY)),
            ("move_mount", || {
                move_mount(
                    3,
                    b"\0".as_ptr(),
                    -100,
                    b"/mnt\0".as_ptr(),
                    MOVE_MOUNT_F_EMPTY_PATH,
                )
            }),
            ("fsconfig", || {
                fsconfig(
                    3,
                    FSCONFIG_CMD_CREATE,
                    core::ptr::null(),
                    core::ptr::null(),
                    0,
                )
            }),
            ("fspick", || {
                fspick(-100, b"/mnt\0".as_ptr(), FSPICK_CLOEXEC)
            }),
            ("open_tree", || {
                open_tree(-100, b"/mnt\0".as_ptr(), OPEN_TREE_CLONE)
            }),
            ("mount_setattr", || {
                let mut attr = MountAttr::default();
                mount_setattr(-100, b"/mnt\0".as_ptr(), 0, &raw mut attr, 32)
            }),
        ];
        for (name, call) in calls {
            crate::errno::set_errno(0);
            assert_eq!(call(), -1, "{name}");
            assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS, "{name}");
        }
        // NULL everywhere is still ENOSYS: nothing is read first.
        crate::errno::set_errno(0);
        assert_eq!(fsopen(core::ptr::null(), 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    /// glibc's `struct mount_attr`: four 64-bit words, 32 bytes --
    /// `MOUNT_ATTR_SIZE_VER0`.
    #[test]
    fn mount_attr_is_glibcs() {
        assert_eq!(
            core::mem::size_of::<MountAttr>(),
            MOUNT_ATTR_SIZE_VER0 as usize
        );
        assert_eq!(core::mem::offset_of!(MountAttr, attr_set), 0);
        assert_eq!(core::mem::offset_of!(MountAttr, attr_clr), 8);
        assert_eq!(core::mem::offset_of!(MountAttr, propagation), 16);
        assert_eq!(core::mem::offset_of!(MountAttr, userns_fd), 24);
    }
}
