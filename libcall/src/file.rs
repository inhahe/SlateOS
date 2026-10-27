//! Files named relative to an open directory, and the calls a program makes
//! on a file it creates in place of another -- through the linked C library.
//!
//! `std` resolves every name from the working directory, and has no word for
//! "open this name in the directory I already hold" (`openat`), for removing
//! one there (`unlinkat`), for reading a directory it did not open itself
//! (`fdopendir`), for setting a file's times to given values by descriptor
//! (`futimens`), or for a descriptor's status flags (`F_GETFL`/`F_SETFL`).
//! gzip does all of these, as does any program that replaces `name` with a
//! transformed `name.gz` beside it.
//!
//! On a host that is not unix every call answers [`ENOSYS`](crate::ENOSYS).

#[cfg(not(unix))]
use crate::ENOSYS;
#[cfg(unix)]
use crate::{last_errno, set_errno};
use core::ffi::CStr;

/// `openat`'s and `unlinkat`'s "the working directory" in place of a
/// directory descriptor.
pub const AT_FDCWD: i32 = -100;
/// `fstatat`'s flag: a symbolic link's own status, not its target's.
pub const AT_SYMLINK_NOFOLLOW: i32 = 0x100;

/// `struct stat` as glibc and SlateOS's C library lay it out on x86-64:
/// 144 bytes.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stat {
    /// `st_dev`: the device the file is on.
    pub st_dev: u64,
    /// `st_ino`: its inode.
    pub st_ino: u64,
    /// `st_nlink`: how many names it has.
    pub st_nlink: u64,
    /// `st_mode`: its type (`S_IFMT`) and permissions.
    pub st_mode: u32,
    /// `st_uid`.
    pub st_uid: u32,
    /// `st_gid`.
    pub st_gid: u32,
    pad0: i32,
    /// `st_rdev`: the device a device file is.
    pub st_rdev: u64,
    /// `st_size`: its size in bytes.
    pub st_size: i64,
    pub st_blksize: i64,
    pub st_blocks: i64,
    /// `st_atim`: seconds and nanoseconds.
    pub st_atime: i64,
    pub st_atime_nsec: i64,
    /// `st_mtim`: seconds and nanoseconds.
    pub st_mtime: i64,
    pub st_mtime_nsec: i64,
    /// `st_ctim`: seconds and nanoseconds.
    pub st_ctime: i64,
    pub st_ctime_nsec: i64,
    reserved: [i64; 3],
}

// `fstatat` writes a whole 144-byte `struct stat` through a pointer to
// this one: a smaller struct would be overrun.
const _: () = assert!(size_of::<Stat>() == 144);

/// `S_IFMT` and the types it masks.
pub const S_IFMT: u32 = 0o170_000;
/// A directory.
pub const S_IFDIR: u32 = 0o040_000;
/// A regular file.
pub const S_IFREG: u32 = 0o100_000;
/// A symbolic link.
pub const S_IFLNK: u32 = 0o120_000;

/// `open`'s access modes and flags, as Linux and SlateOS's C library number
/// them.
pub const O_RDONLY: i32 = 0;
/// Open for writing only.
pub const O_WRONLY: i32 = 0o1;
/// Create the file if it does not exist.
pub const O_CREAT: i32 = 0o100;
/// With [`O_CREAT`]: fail with `EEXIST` if it does.
pub const O_EXCL: i32 = 0o200;
/// Do not make a terminal the caller's controlling terminal.
pub const O_NOCTTY: i32 = 0o400;
/// Do not wait: an open of a FIFO with no writer, and reads after it.
pub const O_NONBLOCK: i32 = 0o4000;
/// Fail unless the name is a directory.
pub const O_DIRECTORY: i32 = 0o200_000;
/// Fail with `ELOOP` if the last component is a symbolic link.
pub const O_NOFOLLOW: i32 = 0o400_000;
/// Close the descriptor across an `exec`.
pub const O_CLOEXEC: i32 = 0o2_000_000;

#[cfg(unix)]
mod sys {
    /// The C library's `DIR`, never looked inside.
    #[repr(C)]
    pub struct Dir {
        _private: [u8; 0],
    }

    /// The head of C's `struct dirent` on x86-64 Linux, as far as the name:
    /// `d_ino`, `d_off`, `d_reclen`, `d_type`, then `d_name`.
    #[repr(C)]
    pub struct Dirent {
        pub d_ino: u64,
        pub d_off: i64,
        pub d_reclen: u16,
        pub d_type: u8,
        pub d_name: [u8; 256],
    }

    unsafe extern "C" {
        pub fn openat(dir: i32, path: *const core::ffi::c_char, flags: i32, ...) -> i32;
        pub fn unlinkat(dir: i32, path: *const core::ffi::c_char, flags: i32) -> i32;
        pub fn unlink(path: *const core::ffi::c_char) -> i32;
        pub fn access(path: *const core::ffi::c_char, mode: i32) -> i32;
        pub fn fstatat(
            dir: i32,
            path: *const core::ffi::c_char,
            buf: *mut super::Stat,
            flags: i32,
        ) -> i32;
        // Two `struct timespec`s as four 64-bit words, as `fd.rs` declares
        // it: one symbol may have only one declaration in a crate.
        pub fn futimens(fd: i32, times: *const [i64; 4]) -> i32;
        pub fn fcntl(fd: i32, cmd: i32, ...) -> i32;
        pub fn fdopendir(fd: i32) -> *mut Dir;
        pub fn readdir(dir: *mut Dir) -> *mut Dirent;
        pub fn closedir(dir: *mut Dir) -> i32;
    }

    /// `fcntl`'s commands for the file status flags.
    pub const F_GETFL: i32 = 3;
    pub const F_SETFL: i32 = 4;
}

/// `openat (dir, name, flags, mode)`: open `name` relative to the directory
/// on `dir` (or the working directory, for [`AT_FDCWD`]). The descriptor,
/// the caller's to close.
///
/// # Errors
///
/// What `openat` reports: `ENOENT`, `EEXIST` under [`O_EXCL`], `ELOOP` under
/// [`O_NOFOLLOW`], `EACCES` and the rest; [`ENOSYS`](crate::ENOSYS) off Unix.
pub fn openat(dir: i32, name: &CStr, flags: i32, mode: u32) -> Result<i32, i32> {
    #[cfg(unix)]
    {
        // SAFETY: `name` is NUL-terminated and only read during the call;
        // the mode travels as the variadic `unsigned`, as C passes it.
        let fd = unsafe { sys::openat(dir, name.as_ptr(), flags, mode) };
        if fd < 0 { Err(last_errno()) } else { Ok(fd) }
    }
    #[cfg(not(unix))]
    {
        let _ = (dir, name, flags, mode);
        Err(ENOSYS)
    }
}

/// `unlinkat (dir, name, flags)`: remove `name` relative to the directory on
/// `dir`.
///
/// # Errors
///
/// What `unlinkat` reports; [`ENOSYS`](crate::ENOSYS) off Unix.
pub fn unlinkat(dir: i32, name: &CStr, flags: i32) -> Result<(), i32> {
    #[cfg(unix)]
    {
        // SAFETY: `name` is NUL-terminated and only read during the call.
        if unsafe { sys::unlinkat(dir, name.as_ptr(), flags) } == 0 {
            Ok(())
        } else {
            Err(last_errno())
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (dir, name, flags);
        Err(ENOSYS)
    }
}

/// `unlink (path)`. One system call and nothing else, so a signal handler
/// may make it -- which is why it is here beside `std::fs::remove_file`.
///
/// # Errors
///
/// What `unlink` reports; [`ENOSYS`](crate::ENOSYS) off Unix.
pub fn unlink(path: &CStr) -> Result<(), i32> {
    #[cfg(unix)]
    {
        // SAFETY: `path` is NUL-terminated and only read during the call.
        if unsafe { sys::unlink(path.as_ptr()) } == 0 {
            Ok(())
        } else {
            Err(last_errno())
        }
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Err(ENOSYS)
    }
}

/// `fstatat (dir, name, &st, flags)`: the status of `name`, relative to the
/// directory open on `dir` ([`AT_FDCWD`] for the working directory) -- of a
/// symbolic link itself with [`AT_SYMLINK_NOFOLLOW`]. Relative to a
/// directory's descriptor a name is not limited by `PATH_MAX`, where a
/// whole path is.
///
/// # Errors
///
/// `ENOENT`, `EACCES`, `ENAMETOOLONG` and the rest;
/// [`ENOSYS`](crate::ENOSYS) off Unix.
pub fn fstatat(dir: i32, name: &CStr, flags: i32) -> Result<Stat, i32> {
    #[cfg(unix)]
    {
        let mut st = Stat::default();
        // SAFETY: `name` is NUL-terminated and only read; `st` is a whole
        // `struct stat`, which is all the call writes.
        if unsafe { sys::fstatat(dir, name.as_ptr(), &raw mut st, flags) } == 0 {
            Ok(st)
        } else {
            Err(last_errno())
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (dir, name, flags);
        Err(ENOSYS)
    }
}

/// `access`'s `F_OK`: whether the file is there at all.
pub const F_OK: i32 = 0;
/// `access`'s `W_OK`: whether this process may write it.
pub const W_OK: i32 = 2;

/// `access (path, mode)`: whether this process may use `path` as `mode`
/// asks ([`F_OK`], [`W_OK`], ...), judged by its real user and group.
///
/// # Errors
///
/// `EACCES` for a use that is not allowed, `ENOENT` and the rest;
/// [`ENOSYS`](crate::ENOSYS) off Unix.
pub fn access(path: &CStr, mode: i32) -> Result<(), i32> {
    #[cfg(unix)]
    {
        // SAFETY: `path` is NUL-terminated and only read during the call.
        if unsafe { sys::access(path.as_ptr(), mode) } == 0 {
            Ok(())
        } else {
            Err(last_errno())
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (path, mode);
        Err(ENOSYS)
    }
}

/// `futimens (fd, {atime, mtime})`: set the file's access and modification
/// times, each as seconds and nanoseconds since the epoch.
///
/// # Errors
///
/// `EINVAL` for nanoseconds outside a second, `EPERM`/`EACCES` for a file
/// that is someone else's; [`ENOSYS`](crate::ENOSYS) off Unix.
pub fn set_times(fd: i32, atime: (i64, i64), mtime: (i64, i64)) -> Result<(), i32> {
    #[cfg(unix)]
    {
        // Two `struct timespec`s: seconds then nanoseconds, each 64 bits.
        let times: [i64; 4] = [atime.0, atime.1, mtime.0, mtime.1];
        // SAFETY: two `struct timespec`s, alive across the call, which only
        // reads them.
        if unsafe { sys::futimens(fd, &raw const times) } == 0 {
            Ok(())
        } else {
            Err(last_errno())
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (fd, atime, mtime);
        Err(ENOSYS)
    }
}

/// `fcntl (fd, F_GETFL)`: the descriptor's access mode and status flags
/// ([`O_NONBLOCK`] among them).
///
/// # Errors
///
/// `EBADF`; [`ENOSYS`](crate::ENOSYS) off Unix.
pub fn status_flags(fd: i32) -> Result<i32, i32> {
    #[cfg(unix)]
    {
        // SAFETY: the command takes no argument.
        let flags = unsafe { sys::fcntl(fd, sys::F_GETFL) };
        if flags < 0 {
            Err(last_errno())
        } else {
            Ok(flags)
        }
    }
    #[cfg(not(unix))]
    {
        let _ = fd;
        Err(ENOSYS)
    }
}

/// `fcntl (fd, F_SETFL, flags)`: set the descriptor's status flags.
///
/// # Errors
///
/// `EBADF`; [`ENOSYS`](crate::ENOSYS) off Unix.
pub fn set_status_flags(fd: i32, flags: i32) -> Result<(), i32> {
    #[cfg(unix)]
    {
        // SAFETY: the command takes an integer argument, not a pointer.
        if unsafe { sys::fcntl(fd, sys::F_SETFL, flags) } == -1 {
            Err(last_errno())
        } else {
            Ok(())
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (fd, flags);
        Err(ENOSYS)
    }
}

/// A directory being read: C's `DIR`, from `fdopendir`. Each name is read
/// by [`Directory::next_name`], in the order the file system keeps them; the
/// stream is closed, and the descriptor with it, by [`Directory::close`] or
/// on drop.
#[derive(Debug)]
pub struct Directory {
    #[cfg(unix)]
    dir: *mut sys::Dir,
}

impl Directory {
    /// `fdopendir (fd)`: read the directory open on `fd`. On success the
    /// descriptor belongs to the stream; on failure it is still the
    /// caller's.
    ///
    /// # Errors
    ///
    /// `ENOTDIR`, `EBADF`, `ENOMEM`; [`ENOSYS`](crate::ENOSYS) off Unix.
    pub fn from_fd(fd: i32) -> Result<Self, i32> {
        #[cfg(unix)]
        {
            // SAFETY: one number in; the stream returned is ours to close.
            let dir = unsafe { sys::fdopendir(fd) };
            if dir.is_null() {
                Err(last_errno())
            } else {
                Ok(Directory { dir })
            }
        }
        #[cfg(not(unix))]
        {
            let _ = fd;
            Err(ENOSYS)
        }
    }

    /// `readdir`: the next name, `.` and `..` included; `None` at the end.
    /// The name is copied into `buf`, which a name never outgrows, and
    /// borrowed from there.
    ///
    /// # Errors
    ///
    /// What `readdir` reports, `EIO` among them.
    pub fn next_name<'b>(&mut self, buf: &'b mut [u8; 256]) -> Option<Result<&'b [u8], i32>> {
        #[cfg(unix)]
        {
            set_errno(0);
            // SAFETY: the stream is open and ours.
            let entry = unsafe { sys::readdir(self.dir) };
            if entry.is_null() {
                let e = last_errno();
                return if e == 0 { None } else { Some(Err(e)) };
            }
            // A pointer to the name, not a reference to `d_name`: glibc's
            // record is only `d_reclen` long, and may end -- near the end
            // of its buffer -- well short of the 256 bytes the field
            // declares, which a reference would claim.
            // SAFETY: a non-null result points at an entry that stays valid
            // until the next `readdir` on this stream; taking a field's
            // address reads nothing.
            let name = unsafe { &raw const (*entry).d_name }.cast::<u8>();
            let mut len = 0;
            while let Some(slot) = buf.get_mut(len) {
                // SAFETY: the name is NUL-terminated within the record, and
                // every byte up to its NUL is read here, none past it.
                let b = unsafe { name.add(len).read() };
                if b == 0 {
                    break;
                }
                *slot = b;
                len = len.saturating_add(1);
            }
            let (dst, _) = buf.split_at_mut(len);
            Some(Ok(dst))
        }
        #[cfg(not(unix))]
        {
            let _ = buf;
            None
        }
    }

    /// `closedir`: close the stream and its descriptor.
    ///
    /// # Errors
    ///
    /// What `closedir` reports; [`ENOSYS`](crate::ENOSYS) off Unix.
    pub fn close(self) -> Result<(), i32> {
        let this = core::mem::ManuallyDrop::new(self);
        #[cfg(unix)]
        {
            // SAFETY: the stream is open and ours, and is not used again --
            // `this` is not dropped, so it is not closed twice.
            if unsafe { sys::closedir(this.dir) } == 0 {
                Ok(())
            } else {
                Err(last_errno())
            }
        }
        #[cfg(not(unix))]
        {
            let _ = this;
            Err(ENOSYS)
        }
    }
}

impl Drop for Directory {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            // SAFETY: the stream is open and ours; `close` never reaches
            // here, so this is its only closing. A failure to close is
            // nobody's to report from a drop.
            let _ = unsafe { sys::closedir(self.dir) };
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    /// The flag values are Linux's, which SlateOS's library shares.
    #[test]
    fn the_flags_are_linux_s() {
        assert_eq!(O_CREAT, posix::fcntl::O_CREAT);
        assert_eq!(O_EXCL, posix::fcntl::O_EXCL);
        assert_eq!(O_NOCTTY, posix::fcntl::O_NOCTTY);
        assert_eq!(O_NONBLOCK, posix::fcntl::O_NONBLOCK);
        assert_eq!(O_DIRECTORY, posix::fcntl::O_DIRECTORY);
        assert_eq!(O_NOFOLLOW, posix::fcntl::O_NOFOLLOW);
        assert_eq!(O_CLOEXEC, posix::fcntl::O_CLOEXEC);
        assert_eq!(O_WRONLY, posix::fcntl::O_WRONLY);
        assert_eq!(AT_FDCWD, posix::file::AT_FDCWD);
    }

    /// A file's status read by name, its presence and absence told.
    #[cfg(unix)]
    #[test]
    fn a_file_s_status_and_presence_are_read_by_name() {
        extern crate std;
        let base = std::env::temp_dir().join(std::format!("libcall-stat-{}", std::process::id()));
        std::fs::create_dir_all(&base).expect("scratch directory");
        std::fs::write(base.join("five"), b"12345").expect("a file");
        let path = std::ffi::CString::new(base.join("five").to_str().expect("UTF-8 temp"))
            .expect("no NUL");
        let missing = std::ffi::CString::new(base.join("none").to_str().expect("UTF-8 temp"))
            .expect("no NUL");

        let st = fstatat(AT_FDCWD, &path, 0).expect("fstatat");
        assert_eq!((st.st_size, st.st_mode & S_IFMT), (5, S_IFREG));
        assert_eq!(fstatat(AT_FDCWD, &missing, 0), Err(crate::ENOENT));

        assert_eq!(access(&path, F_OK), Ok(()));
        assert_eq!(access(&missing, F_OK), Err(crate::ENOENT));
        std::fs::remove_dir_all(&base).expect("cleaned up");
    }

    /// A file made, read back, timed, flagged and removed through a
    /// directory descriptor.
    #[cfg(unix)]
    #[test]
    fn a_file_is_made_and_removed_beside_its_directory_descriptor() {
        extern crate std;
        let base = std::env::temp_dir().join(std::format!("libcall-file-{}", std::process::id()));
        std::fs::create_dir_all(&base).expect("scratch directory");
        let dir_c = std::ffi::CString::new(base.to_str().expect("UTF-8 temp")).expect("no NUL");
        let dfd = openat(AT_FDCWD, &dir_c, O_RDONLY | O_DIRECTORY, 0).expect("open directory");

        let fd = openat(dfd, c"made", O_WRONLY | O_CREAT | O_EXCL, 0o600).expect("create");
        assert_eq!(
            openat(dfd, c"made", O_WRONLY | O_CREAT | O_EXCL, 0o600),
            Err(17),
            "EEXIST"
        );
        set_times(fd, (1_000_000_000, 5), (1_200_000_000, 999_999_999)).expect("times");
        let md = std::fs::metadata(base.join("made")).expect("stat");
        {
            use std::os::unix::fs::MetadataExt;
            assert_eq!((md.mtime(), md.mtime_nsec()), (1_200_000_000, 999_999_999));
            assert_eq!(md.atime(), 1_000_000_000);
        }
        assert_eq!(
            set_times(fd, (0, 0), (0, 1_000_000_000)),
            Err(crate::EINVAL)
        );

        let flags = status_flags(fd).expect("F_GETFL");
        set_status_flags(fd, flags | O_NONBLOCK).expect("F_SETFL");
        assert_ne!(status_flags(fd).expect("F_GETFL") & O_NONBLOCK, 0);
        assert!(crate::fd::close(fd).is_ok());

        let listing = openat(dfd, c".", O_RDONLY | O_DIRECTORY, 0).expect("reopen");
        let mut dir = Directory::from_fd(listing).expect("fdopendir");
        let mut buf = [0u8; 256];
        let mut seen = std::vec::Vec::new();
        while let Some(name) = dir.next_name(&mut buf) {
            seen.push(name.expect("readdir").to_vec());
        }
        assert!(dir.close().is_ok());
        seen.sort();
        assert_eq!(seen, [b".".to_vec(), b"..".to_vec(), b"made".to_vec()]);

        unlinkat(dfd, c"made", 0).expect("unlinkat");
        assert_eq!(unlinkat(dfd, c"made", 0), Err(crate::ENOENT));
        assert!(crate::fd::close(dfd).is_ok());
        assert_eq!(unlink(&dir_c), Err(21), "EISDIR");
        std::fs::remove_dir(&base).expect("cleanup");
    }

    /// Off Unix every call declines.
    #[cfg(not(unix))]
    #[test]
    fn off_unix_every_call_declines() {
        assert_eq!(openat(AT_FDCWD, c"x", O_RDONLY, 0), Err(ENOSYS));
        assert_eq!(unlinkat(AT_FDCWD, c"x", 0), Err(ENOSYS));
        assert_eq!(unlink(c"x"), Err(ENOSYS));
        assert_eq!(set_times(0, (0, 0), (0, 0)), Err(ENOSYS));
        assert_eq!(status_flags(0), Err(ENOSYS));
        assert_eq!(set_status_flags(0, 0), Err(ENOSYS));
        assert_eq!(Directory::from_fd(0).err(), Some(ENOSYS));
    }
}
