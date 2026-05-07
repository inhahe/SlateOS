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
}
