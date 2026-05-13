//! POSIX stat structure.
//!
//! The stat struct returned by `stat()`, `fstat()`, and `lstat()`.
//! Layout matches Linux x86_64 `struct stat` for binary compatibility.

use crate::types::*;

/// Timespec — seconds + nanoseconds.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct Timespec {
    /// Seconds since epoch.
    pub tv_sec: TimeT,
    /// Nanoseconds (0..999_999_999).
    pub tv_nsec: i64,
}

/// File status structure.
///
/// Returned by `stat()`, `fstat()`, `lstat()`.
/// Layout matches Linux x86_64 `struct stat`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Stat {
    /// Device ID of device containing file.
    pub st_dev: DevT,
    /// Inode number.
    pub st_ino: InoT,
    /// Number of hard links.
    pub st_nlink: NlinkT,
    /// File mode (permissions + type).
    pub st_mode: ModeT,
    /// User ID of owner.
    pub st_uid: UidT,
    /// Group ID of owner.
    pub st_gid: GidT,
    /// Padding.
    _pad0: i32,
    /// Device ID (if special file).
    pub st_rdev: DevT,
    /// Total size in bytes.
    pub st_size: OffT,
    /// Block size for filesystem I/O.
    pub st_blksize: BlksizeT,
    /// Number of 512-byte blocks allocated.
    pub st_blocks: BlkcntT,
    /// Time of last access.
    pub st_atim: Timespec,
    /// Time of last modification.
    pub st_mtim: Timespec,
    /// Time of last status change.
    pub st_ctim: Timespec,
    /// Reserved.
    _reserved: [i64; 3],
}

impl Default for Stat {
    fn default() -> Self {
        // SAFETY: Stat is a C-compatible struct, zero-init is valid.
        unsafe { core::mem::zeroed() }
    }
}

impl Stat {
    /// Create a zeroed stat structure.
    #[must_use]
    pub fn zeroed() -> Self {
        Self::default()
    }

    /// Check if this is a regular file.
    #[must_use]
    pub fn is_file(&self) -> bool {
        (self.st_mode & crate::fcntl::S_IFMT) == crate::fcntl::S_IFREG
    }

    /// Check if this is a directory.
    #[must_use]
    pub fn is_dir(&self) -> bool {
        (self.st_mode & crate::fcntl::S_IFMT) == crate::fcntl::S_IFDIR
    }

    /// Check if this is a symbolic link.
    #[must_use]
    pub fn is_link(&self) -> bool {
        (self.st_mode & crate::fcntl::S_IFMT) == crate::fcntl::S_IFLNK
    }

    /// Check if this is a character device.
    #[must_use]
    pub fn is_chr(&self) -> bool {
        (self.st_mode & crate::fcntl::S_IFMT) == crate::fcntl::S_IFCHR
    }

    /// Check if this is a block device.
    #[must_use]
    pub fn is_blk(&self) -> bool {
        (self.st_mode & crate::fcntl::S_IFMT) == crate::fcntl::S_IFBLK
    }

    /// Check if this is a FIFO (named pipe).
    #[must_use]
    pub fn is_fifo(&self) -> bool {
        (self.st_mode & crate::fcntl::S_IFMT) == crate::fcntl::S_IFIFO
    }

    /// Check if this is a socket.
    #[must_use]
    pub fn is_sock(&self) -> bool {
        (self.st_mode & crate::fcntl::S_IFMT) == crate::fcntl::S_IFSOCK
    }
}

// ---------------------------------------------------------------------------
// S_IS* macros as C-callable functions
// ---------------------------------------------------------------------------
//
// C programs use S_ISREG(m), S_ISDIR(m), etc. as macros that expand to
// bitwise tests on the mode.  These are typically preprocessor macros, but
// some build systems or languages need them as linkable symbols.

/// `S_ISREG(mode)` — test for regular file.
#[unsafe(no_mangle)]
pub extern "C" fn S_ISREG(mode: u32) -> i32 {
    i32::from((mode & crate::fcntl::S_IFMT) == crate::fcntl::S_IFREG)
}

/// `S_ISDIR(mode)` — test for directory.
#[unsafe(no_mangle)]
pub extern "C" fn S_ISDIR(mode: u32) -> i32 {
    i32::from((mode & crate::fcntl::S_IFMT) == crate::fcntl::S_IFDIR)
}

/// `S_ISLNK(mode)` — test for symbolic link.
#[unsafe(no_mangle)]
pub extern "C" fn S_ISLNK(mode: u32) -> i32 {
    i32::from((mode & crate::fcntl::S_IFMT) == crate::fcntl::S_IFLNK)
}

/// `S_ISCHR(mode)` — test for character device.
#[unsafe(no_mangle)]
pub extern "C" fn S_ISCHR(mode: u32) -> i32 {
    i32::from((mode & crate::fcntl::S_IFMT) == crate::fcntl::S_IFCHR)
}

/// `S_ISBLK(mode)` — test for block device.
#[unsafe(no_mangle)]
pub extern "C" fn S_ISBLK(mode: u32) -> i32 {
    i32::from((mode & crate::fcntl::S_IFMT) == crate::fcntl::S_IFBLK)
}

/// `S_ISFIFO(mode)` — test for FIFO.
#[unsafe(no_mangle)]
pub extern "C" fn S_ISFIFO(mode: u32) -> i32 {
    i32::from((mode & crate::fcntl::S_IFMT) == crate::fcntl::S_IFIFO)
}

/// `S_ISSOCK(mode)` — test for socket.
#[unsafe(no_mangle)]
pub extern "C" fn S_ISSOCK(mode: u32) -> i32 {
    i32::from((mode & crate::fcntl::S_IFMT) == crate::fcntl::S_IFSOCK)
}

// ---------------------------------------------------------------------------
// mknod / mkfifo — create special files
// ---------------------------------------------------------------------------

/// Create a special or ordinary file.
///
/// Stub: returns -1 with ENOSYS.  Our filesystem doesn't support
/// device nodes or special files yet.
#[unsafe(no_mangle)]
pub extern "C" fn mknod(_pathname: *const u8, _mode: u32, _dev: u64) -> i32 {
    crate::errno::set_errno(crate::errno::ENOSYS);
    -1
}

/// Create a special file relative to a directory fd.
///
/// Stub: returns -1 with ENOSYS.
#[unsafe(no_mangle)]
pub extern "C" fn mknodat(_dirfd: i32, _pathname: *const u8, _mode: u32, _dev: u64) -> i32 {
    crate::errno::set_errno(crate::errno::ENOSYS);
    -1
}

/// Create a FIFO (named pipe).
///
/// Stub: returns -1 with ENOSYS.  Named pipes require kernel support
/// for special file types in the filesystem.
#[unsafe(no_mangle)]
pub extern "C" fn mkfifo(_pathname: *const u8, _mode: u32) -> i32 {
    crate::errno::set_errno(crate::errno::ENOSYS);
    -1
}

/// Create a FIFO relative to a directory fd.
///
/// Stub: returns -1 with ENOSYS.
#[unsafe(no_mangle)]
pub extern "C" fn mkfifoat(_dirfd: i32, _pathname: *const u8, _mode: u32) -> i32 {
    crate::errno::set_errno(crate::errno::ENOSYS);
    -1
}
