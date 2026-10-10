//! The system calls `patch` makes that `std` does not wrap: the `*at` family
//! `safe.c` walks paths with, `struct stat` as the kernel fills it, and a few
//! calls on descriptors -- each through the linked C library
//! (design-decisions §768), declared here beside their one caller as
//! `coreutils::dirfd` declares its own.
//!
//! On a host that is not unix every call fails with `ENOSYS`: the tests that
//! run there exercise the parsing and the hunk logic, which need none of this.

use std::io;

/// `AT_FDCWD`: resolve against the working directory.
pub const AT_FDCWD: i32 = -100;
/// `AT_SYMLINK_NOFOLLOW`.
pub const AT_SYMLINK_NOFOLLOW: i32 = 0x100;
/// `AT_REMOVEDIR`, for `unlinkat`.
pub const AT_REMOVEDIR: i32 = 0x200;
/// `SEEK_SET` and `SEEK_CUR`.
pub const SEEK_SET: i32 = 0;
pub const SEEK_CUR: i32 = 1;

/// `open` flags, as Linux numbers them.
pub mod oflag {
    pub const RDONLY: i32 = 0;
    pub const WRONLY: i32 = 1;
    pub const RDWR: i32 = 2;
    pub const CREAT: i32 = 0o100;
    pub const EXCL: i32 = 0o200;
    pub const TRUNC: i32 = 0o1000;
    pub const APPEND: i32 = 0o2000;
    pub const DIRECTORY: i32 = 0o200_000;
    pub const NOFOLLOW: i32 = 0o400_000;
    pub const CLOEXEC: i32 = 0o2_000_000;
}

/// `access` modes.
pub const W_OK: i32 = 2;

/// The file-type bits of a mode, and their values.
pub const S_IFMT: u32 = 0o170_000;
pub const S_IFREG: u32 = 0o100_000;
pub const S_IFLNK: u32 = 0o120_000;
pub const S_IFDIR: u32 = 0o040_000;
/// The permission bits: `S_IRWXU | S_IRWXG | S_IRWXO`.
pub const S_IRWXUGO: u32 = 0o777;

/// `errno` values `patch` tests for by number.
pub mod errno {
    pub const EPERM: i32 = 1;
    pub const ENOENT: i32 = 2;
    pub const EIO: i32 = 5;
    pub const EEXIST: i32 = 17;
    pub const EXDEV: i32 = 18;
    pub const ENOTDIR: i32 = 20;
    pub const EINVAL: i32 = 22;
    pub const EMLINK: i32 = 31;
    #[cfg(not(unix))]
    pub const ENOSYS: i32 = 38;
    pub const ELOOP: i32 = 40;
}

/// A point in time, as `struct timespec` holds it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Timespec {
    pub sec: i64,
    pub nsec: i64,
}

/// What `patch` reads of a `struct stat`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stat {
    pub dev: u64,
    pub ino: u64,
    pub nlink: u64,
    pub mode: u32,
    pub uid: u32,
    pub gid: u32,
    pub size: i64,
    pub atime: Timespec,
    pub mtime: Timespec,
}

impl Stat {
    /// `S_ISREG`.
    pub fn is_reg(&self) -> bool {
        self.mode & S_IFMT == S_IFREG
    }

    /// `S_ISLNK`.
    pub fn is_lnk(&self) -> bool {
        self.mode & S_IFMT == S_IFLNK
    }

    /// `S_ISDIR`.
    pub fn is_dir(&self) -> bool {
        self.mode & S_IFMT == S_IFDIR
    }
}

/// The `errno` of the call that just failed.
pub fn last_errno() -> i32 {
    io::Error::last_os_error()
        .raw_os_error()
        .unwrap_or(errno::EIO)
}

/// A path as the C library takes it. A path with a NUL in it names no file:
/// `ENOENT`, which is what a C caller handing it over would have reached
/// first, at the NUL.
#[cfg(unix)]
pub fn cpath(path: &[u8]) -> Result<std::ffi::CString, i32> {
    std::ffi::CString::new(path).map_err(|_| errno::ENOENT)
}

#[cfg(unix)]
mod imp {
    use super::{Stat, Timespec, cpath, last_errno};

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct CTimespec {
        tv_sec: i64,
        tv_nsec: i64,
    }

    /// `struct stat`, Linux x86-64's layout, as `posix/src/stat.rs` declares it.
    #[repr(C)]
    #[derive(Default)]
    struct CStat {
        st_dev: u64,
        st_ino: u64,
        st_nlink: u64,
        st_mode: u32,
        st_uid: u32,
        st_gid: u32,
        _pad0: i32,
        st_rdev: u64,
        st_size: i64,
        st_blksize: i64,
        st_blocks: i64,
        st_atim: CTimespec,
        st_mtim: CTimespec,
        st_ctim: CTimespec,
        _reserved: [i64; 3],
    }

    const _: () = assert!(core::mem::size_of::<CStat>() == 144);

    unsafe extern "C" {
        fn openat(dirfd: i32, path: *const u8, flags: i32, mode: u32) -> i32;
        fn close(fd: i32) -> i32;
        fn dup(fd: i32) -> i32;
        fn fstat(fd: i32, buf: *mut CStat) -> i32;
        fn fstatat(dirfd: i32, path: *const u8, buf: *mut CStat, flags: i32) -> i32;
        fn renameat(olddirfd: i32, oldpath: *const u8, newdirfd: i32, newpath: *const u8) -> i32;
        fn mkdirat(dirfd: i32, path: *const u8, mode: u32) -> i32;
        fn unlinkat(dirfd: i32, path: *const u8, flags: i32) -> i32;
        fn symlinkat(target: *const u8, dirfd: i32, path: *const u8) -> i32;
        fn fchmodat(dirfd: i32, path: *const u8, mode: u32, flags: i32) -> i32;
        fn fchownat(dirfd: i32, path: *const u8, owner: u32, group: u32, flags: i32) -> i32;
        fn utimensat(dirfd: i32, path: *const u8, times: *const CTimespec, flags: i32) -> i32;
        fn readlinkat(dirfd: i32, path: *const u8, buf: *mut u8, len: usize) -> isize;
        fn faccessat(dirfd: i32, path: *const u8, mode: i32, flags: i32) -> i32;
        fn lseek(fd: i32, offset: i64, whence: i32) -> i64;
        fn geteuid() -> u32;
        fn getegid() -> u32;
    }

    fn stat_of(c: &CStat) -> Stat {
        Stat {
            dev: c.st_dev,
            ino: c.st_ino,
            nlink: c.st_nlink,
            mode: c.st_mode,
            uid: c.st_uid,
            gid: c.st_gid,
            size: c.st_size,
            atime: Timespec {
                sec: c.st_atim.tv_sec,
                nsec: c.st_atim.tv_nsec,
            },
            mtime: Timespec {
                sec: c.st_mtim.tv_sec,
                nsec: c.st_mtim.tv_nsec,
            },
        }
    }

    fn rc(r: i32) -> Result<(), i32> {
        if r == 0 { Ok(()) } else { Err(last_errno()) }
    }

    pub fn open_at(dirfd: i32, path: &[u8], flags: i32, mode: u32) -> Result<i32, i32> {
        let p = cpath(path)?;
        // SAFETY: `p` is a live NUL-terminated string for the call; the
        // library reads it and keeps no pointer.
        let fd = unsafe { openat(dirfd, p.as_ptr().cast(), flags, mode) };
        if fd < 0 { Err(last_errno()) } else { Ok(fd) }
    }

    pub fn close_fd(fd: i32) -> Result<(), i32> {
        // SAFETY: a number; the caller owns it and closes it once.
        rc(unsafe { close(fd) })
    }

    pub fn dup_fd(fd: i32) -> Result<i32, i32> {
        // SAFETY: a number in, a number out.
        let d = unsafe { dup(fd) };
        if d < 0 { Err(last_errno()) } else { Ok(d) }
    }

    pub fn fstat_fd(fd: i32) -> Result<Stat, i32> {
        let mut c = CStat::default();
        // SAFETY: `c` is a live `struct stat` of the right size (asserted
        // above), written once by the call.
        rc(unsafe { fstat(fd, &raw mut c) })?;
        Ok(stat_of(&c))
    }

    pub fn stat_at(dirfd: i32, path: &[u8], flags: i32) -> Result<Stat, i32> {
        let p = cpath(path)?;
        let mut c = CStat::default();
        // SAFETY: as for `fstat_fd`, and `p` lives for the call.
        rc(unsafe { fstatat(dirfd, p.as_ptr().cast(), &raw mut c, flags) })?;
        Ok(stat_of(&c))
    }

    pub fn rename_at(olddirfd: i32, old: &[u8], newdirfd: i32, new: &[u8]) -> Result<(), i32> {
        let o = cpath(old)?;
        let n = cpath(new)?;
        // SAFETY: two live NUL-terminated strings for the call.
        rc(unsafe { renameat(olddirfd, o.as_ptr().cast(), newdirfd, n.as_ptr().cast()) })
    }

    pub fn mkdir_at(dirfd: i32, path: &[u8], mode: u32) -> Result<(), i32> {
        let p = cpath(path)?;
        // SAFETY: a live string for the call.
        rc(unsafe { mkdirat(dirfd, p.as_ptr().cast(), mode) })
    }

    pub fn unlink_at(dirfd: i32, path: &[u8], flags: i32) -> Result<(), i32> {
        let p = cpath(path)?;
        // SAFETY: a live string for the call.
        rc(unsafe { unlinkat(dirfd, p.as_ptr().cast(), flags) })
    }

    pub fn symlink_at(target: &[u8], dirfd: i32, path: &[u8]) -> Result<(), i32> {
        let t = cpath(target)?;
        let p = cpath(path)?;
        // SAFETY: two live strings for the call.
        rc(unsafe { symlinkat(t.as_ptr().cast(), dirfd, p.as_ptr().cast()) })
    }

    pub fn chmod_at(dirfd: i32, path: &[u8], mode: u32) -> Result<(), i32> {
        let p = cpath(path)?;
        // SAFETY: a live string for the call.
        rc(unsafe { fchmodat(dirfd, p.as_ptr().cast(), mode, 0) })
    }

    pub fn lchown_at(dirfd: i32, path: &[u8], uid: u32, gid: u32) -> Result<(), i32> {
        let p = cpath(path)?;
        // SAFETY: a live string for the call.
        rc(unsafe {
            fchownat(
                dirfd,
                p.as_ptr().cast(),
                uid,
                gid,
                super::AT_SYMLINK_NOFOLLOW,
            )
        })
    }

    pub fn lutimens_at(dirfd: i32, path: &[u8], times: [Timespec; 2]) -> Result<(), i32> {
        let p = cpath(path)?;
        let t = [
            CTimespec {
                tv_sec: times[0].sec,
                tv_nsec: times[0].nsec,
            },
            CTimespec {
                tv_sec: times[1].sec,
                tv_nsec: times[1].nsec,
            },
        ];
        // SAFETY: `t` is two live `struct timespec`s and `p` a live string,
        // both read by the call and not kept.
        rc(unsafe {
            utimensat(
                dirfd,
                p.as_ptr().cast(),
                t.as_ptr(),
                super::AT_SYMLINK_NOFOLLOW,
            )
        })
    }

    pub fn readlink_at(dirfd: i32, path: &[u8], size: usize) -> Result<Vec<u8>, i32> {
        let p = cpath(path)?;
        let mut buf = vec![0u8; size.max(1)];
        // SAFETY: `buf` is `size` writable bytes (at least one), filled at
        // most that far by the call.
        let n = unsafe { readlinkat(dirfd, p.as_ptr().cast(), buf.as_mut_ptr(), size) };
        let n = usize::try_from(n).map_err(|_| last_errno())?;
        buf.truncate(n);
        Ok(buf)
    }

    pub fn access_at(dirfd: i32, path: &[u8], mode: i32) -> Result<(), i32> {
        let p = cpath(path)?;
        // SAFETY: a live string for the call.
        rc(unsafe { faccessat(dirfd, p.as_ptr().cast(), mode, 0) })
    }

    pub fn lseek_fd(fd: i32, offset: i64, whence: i32) -> Result<i64, i32> {
        // SAFETY: numbers in, a number out.
        let r = unsafe { lseek(fd, offset, whence) };
        if r < 0 { Err(last_errno()) } else { Ok(r) }
    }

    pub fn effective_ids() -> (u32, u32) {
        // SAFETY: no arguments; each returns a number.
        unsafe { (geteuid(), getegid()) }
    }
}

#[cfg(not(unix))]
mod imp {
    use super::{Stat, Timespec, errno};

    pub fn open_at(_: i32, _: &[u8], _: i32, _: u32) -> Result<i32, i32> {
        Err(errno::ENOSYS)
    }
    pub fn close_fd(_: i32) -> Result<(), i32> {
        Err(errno::ENOSYS)
    }
    pub fn dup_fd(_: i32) -> Result<i32, i32> {
        Err(errno::ENOSYS)
    }
    pub fn fstat_fd(_: i32) -> Result<Stat, i32> {
        Err(errno::ENOSYS)
    }
    pub fn stat_at(_: i32, _: &[u8], _: i32) -> Result<Stat, i32> {
        Err(errno::ENOSYS)
    }
    pub fn rename_at(_: i32, _: &[u8], _: i32, _: &[u8]) -> Result<(), i32> {
        Err(errno::ENOSYS)
    }
    pub fn mkdir_at(_: i32, _: &[u8], _: u32) -> Result<(), i32> {
        Err(errno::ENOSYS)
    }
    pub fn unlink_at(_: i32, _: &[u8], _: i32) -> Result<(), i32> {
        Err(errno::ENOSYS)
    }
    pub fn symlink_at(_: &[u8], _: i32, _: &[u8]) -> Result<(), i32> {
        Err(errno::ENOSYS)
    }
    pub fn chmod_at(_: i32, _: &[u8], _: u32) -> Result<(), i32> {
        Err(errno::ENOSYS)
    }
    pub fn lchown_at(_: i32, _: &[u8], _: u32, _: u32) -> Result<(), i32> {
        Err(errno::ENOSYS)
    }
    pub fn lutimens_at(_: i32, _: &[u8], _: [Timespec; 2]) -> Result<(), i32> {
        Err(errno::ENOSYS)
    }
    pub fn readlink_at(_: i32, _: &[u8], _: usize) -> Result<Vec<u8>, i32> {
        Err(errno::ENOSYS)
    }
    pub fn access_at(_: i32, _: &[u8], _: i32) -> Result<(), i32> {
        Err(errno::ENOSYS)
    }
    pub fn lseek_fd(_: i32, _: i64, _: i32) -> Result<i64, i32> {
        Err(errno::ENOSYS)
    }
    pub fn effective_ids() -> (u32, u32) {
        (0, 0)
    }
}

pub use imp::*;
