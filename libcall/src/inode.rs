//! An inode's attributes as `chattr` and `lsattr` reach them: its flags
//! (`FS_IOC_GETFLAGS`/`FS_IOC_SETFLAGS`), its generation
//! (`FS_IOC_GETVERSION`/`FS_IOC_SETVERSION`) and its project
//! (`FS_IOC_FSGETXATTR`/`FS_IOC_FSSETXATTR`) -- each an `ioctl` on a
//! descriptor of the file, through the linked C library.
//!
//! The numbers are Linux's, which SlateOS's kernel answers for the flags it
//! keeps (`requests/b-a-chattr-needs-fs-ioc-getflags-or-it-should-be-deleted.md`:
//! immutable and append-only; any other bit is `EOPNOTSUPP`). Off Unix every
//! call answers [`ENOSYS`](crate::ENOSYS).

#[cfg(not(unix))]
use crate::ENOSYS;
#[cfg(unix)]
use crate::last_errno;

/// `FS_IOC_GETFLAGS`: `_IOR('f', 1, long)`.
pub const FS_IOC_GETFLAGS: u64 = 0x8008_6601;
/// `FS_IOC_SETFLAGS`: `_IOW('f', 2, long)`.
pub const FS_IOC_SETFLAGS: u64 = 0x4008_6602;
/// `FS_IOC_GETVERSION`: `_IOR('v', 1, long)`.
pub const FS_IOC_GETVERSION: u64 = 0x8008_7601;
/// `FS_IOC_SETVERSION`: `_IOW('v', 2, long)`.
pub const FS_IOC_SETVERSION: u64 = 0x4008_7602;
/// `FS_IOC_FSGETXATTR`: `_IOR('X', 31, struct fsxattr)`.
pub const FS_IOC_FSGETXATTR: u64 = 0x801c_581f;
/// `FS_IOC_FSSETXATTR`: `_IOW('X', 32, struct fsxattr)`.
pub const FS_IOC_FSSETXATTR: u64 = 0x401c_5820;

/// `struct fsxattr`: 28 bytes, as `<linux/fs.h>` lays it out.
#[cfg(any(unix, test))]
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
struct FsXattr {
    xflags: u32,
    extsize: u32,
    nextents: u32,
    projid: u32,
    cowextsize: u32,
    pad: [u8; 8],
}

#[cfg(unix)]
mod sys {
    unsafe extern "C" {
        // Variadic, as C declares it; see `pty.rs` on why one declaration
        // serves both our library and glibc.
        pub fn ioctl(fd: i32, request: u64, ...) -> i32;
    }
}

/// `ioctl (fd, request, arg)`: 0, or the `errno`.
#[cfg(unix)]
fn ioctl_ptr<T>(fd: i32, request: u64, arg: *mut T) -> Result<(), i32> {
    // SAFETY: each caller passes a request whose argument is the `T` that
    // `arg` points at, alive and writable for the call.
    let rc = unsafe { sys::ioctl(fd, request, arg) };
    if rc == -1 { Err(last_errno()) } else { Ok(()) }
}

/// The file's flags, as `FS_IOC_GETFLAGS` gives them (C's `int`).
///
/// # Errors
///
/// What the `ioctl` reports: `ENOTTY` for a file that has none, `EOPNOTSUPP`;
/// [`ENOSYS`](crate::ENOSYS) off Unix.
pub fn get_flags(fd: i32) -> Result<i32, i32> {
    #[cfg(unix)]
    {
        let mut flags: i32 = 0;
        ioctl_ptr(fd, FS_IOC_GETFLAGS, &raw mut flags).map(|()| flags)
    }
    #[cfg(not(unix))]
    {
        let _ = fd;
        Err(ENOSYS)
    }
}

/// Set the file's flags (`FS_IOC_SETFLAGS`).
///
/// # Errors
///
/// `EPERM` for a flag the caller may not change, `EOPNOTSUPP` for one the
/// file system does not keep, `ENOTTY`; [`ENOSYS`](crate::ENOSYS) off Unix.
pub fn set_flags(fd: i32, flags: i32) -> Result<(), i32> {
    #[cfg(unix)]
    {
        let mut flags = flags;
        ioctl_ptr(fd, FS_IOC_SETFLAGS, &raw mut flags)
    }
    #[cfg(not(unix))]
    {
        let _ = (fd, flags);
        Err(ENOSYS)
    }
}

/// The file's generation (`FS_IOC_GETVERSION`; an `int` comes back).
///
/// # Errors
///
/// What the `ioctl` reports; [`ENOSYS`](crate::ENOSYS) off Unix.
pub fn get_version(fd: i32) -> Result<u32, i32> {
    #[cfg(unix)]
    {
        let mut version: u32 = u32::MAX;
        ioctl_ptr(fd, FS_IOC_GETVERSION, &raw mut version).map(|()| version)
    }
    #[cfg(not(unix))]
    {
        let _ = fd;
        Err(ENOSYS)
    }
}

/// Set the file's generation (`FS_IOC_SETVERSION`).
///
/// # Errors
///
/// `EPERM` without the right to; [`ENOSYS`](crate::ENOSYS) off Unix.
pub fn set_version(fd: i32, version: i32) -> Result<(), i32> {
    #[cfg(unix)]
    {
        let mut version = version;
        ioctl_ptr(fd, FS_IOC_SETVERSION, &raw mut version)
    }
    #[cfg(not(unix))]
    {
        let _ = (fd, version);
        Err(ENOSYS)
    }
}

/// The file's project id (`FS_IOC_FSGETXATTR`'s `fsx_projid`).
///
/// # Errors
///
/// What the `ioctl` reports; [`ENOSYS`](crate::ENOSYS) off Unix.
pub fn get_project(fd: i32) -> Result<u32, i32> {
    #[cfg(unix)]
    {
        let mut attr = FsXattr::default();
        ioctl_ptr(fd, FS_IOC_FSGETXATTR, &raw mut attr).map(|()| attr.projid)
    }
    #[cfg(not(unix))]
    {
        let _ = fd;
        Err(ENOSYS)
    }
}

/// Set the file's project id: its extended attributes read
/// (`FS_IOC_FSGETXATTR`), the project changed, and written back
/// (`FS_IOC_FSSETXATTR`), as e2fsprogs' `fsetproject` does.
///
/// # Errors
///
/// The first `ioctl`'s failure; [`ENOSYS`](crate::ENOSYS) off Unix.
pub fn set_project(fd: i32, project: u32) -> Result<(), i32> {
    #[cfg(unix)]
    {
        let mut attr = FsXattr::default();
        ioctl_ptr(fd, FS_IOC_FSGETXATTR, &raw mut attr)?;
        attr.projid = project;
        ioctl_ptr(fd, FS_IOC_FSSETXATTR, &raw mut attr)
    }
    #[cfg(not(unix))]
    {
        let _ = (fd, project);
        Err(ENOSYS)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_extended_attributes_are_linux_s_28_bytes() {
        assert_eq!(core::mem::size_of::<FsXattr>(), 28);
        // `_IOR`/`_IOW`: direction, size, type, number.
        let ior = |ty: u8, nr: u8, size: u64| {
            (2u64 << 30) | (size << 16) | (u64::from(ty) << 8) | u64::from(nr)
        };
        let iow = |ty: u8, nr: u8, size: u64| {
            (1u64 << 30) | (size << 16) | (u64::from(ty) << 8) | u64::from(nr)
        };
        assert_eq!(FS_IOC_GETFLAGS, ior(b'f', 1, 8));
        assert_eq!(FS_IOC_SETFLAGS, iow(b'f', 2, 8));
        assert_eq!(FS_IOC_GETVERSION, ior(b'v', 1, 8));
        assert_eq!(FS_IOC_SETVERSION, iow(b'v', 2, 8));
        assert_eq!(FS_IOC_FSGETXATTR, ior(b'X', 31, 28));
        assert_eq!(FS_IOC_FSSETXATTR, iow(b'X', 32, 28));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_pipe_has_no_flags() {
        let mut fds = [0i32; 2];
        // SAFETY: two ints for the two ends.
        assert_eq!(unsafe { libc_pipe(fds.as_mut_ptr()) }, 0);
        assert_eq!(get_flags(fds[0]), Err(25), "ENOTTY");
        for fd in fds {
            let _ = crate::fd::close(fd);
        }
    }

    #[cfg(target_os = "linux")]
    unsafe extern "C" {
        #[link_name = "pipe"]
        fn libc_pipe(fds: *mut i32) -> i32;
    }
}
