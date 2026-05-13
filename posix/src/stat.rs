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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn S_ISREG(mode: u32) -> i32 {
    i32::from((mode & crate::fcntl::S_IFMT) == crate::fcntl::S_IFREG)
}

/// `S_ISDIR(mode)` — test for directory.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn S_ISDIR(mode: u32) -> i32 {
    i32::from((mode & crate::fcntl::S_IFMT) == crate::fcntl::S_IFDIR)
}

/// `S_ISLNK(mode)` — test for symbolic link.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn S_ISLNK(mode: u32) -> i32 {
    i32::from((mode & crate::fcntl::S_IFMT) == crate::fcntl::S_IFLNK)
}

/// `S_ISCHR(mode)` — test for character device.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn S_ISCHR(mode: u32) -> i32 {
    i32::from((mode & crate::fcntl::S_IFMT) == crate::fcntl::S_IFCHR)
}

/// `S_ISBLK(mode)` — test for block device.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn S_ISBLK(mode: u32) -> i32 {
    i32::from((mode & crate::fcntl::S_IFMT) == crate::fcntl::S_IFBLK)
}

/// `S_ISFIFO(mode)` — test for FIFO.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn S_ISFIFO(mode: u32) -> i32 {
    i32::from((mode & crate::fcntl::S_IFMT) == crate::fcntl::S_IFIFO)
}

/// `S_ISSOCK(mode)` — test for socket.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn mknod(_pathname: *const u8, _mode: u32, _dev: u64) -> i32 {
    crate::errno::set_errno(crate::errno::ENOSYS);
    -1
}

/// Create a special file relative to a directory fd.
///
/// Stub: returns -1 with ENOSYS.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn mknodat(_dirfd: i32, _pathname: *const u8, _mode: u32, _dev: u64) -> i32 {
    crate::errno::set_errno(crate::errno::ENOSYS);
    -1
}

/// Create a FIFO (named pipe).
///
/// Stub: returns -1 with ENOSYS.  Named pipes require kernel support
/// for special file types in the filesystem.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn mkfifo(_pathname: *const u8, _mode: u32) -> i32 {
    crate::errno::set_errno(crate::errno::ENOSYS);
    -1
}

/// Create a FIFO relative to a directory fd.
///
/// Stub: returns -1 with ENOSYS.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn mkfifoat(_dirfd: i32, _pathname: *const u8, _mode: u32) -> i32 {
    crate::errno::set_errno(crate::errno::ENOSYS);
    -1
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fcntl::*;

    // -- S_IS* C-callable functions --

    #[test]
    fn test_s_isreg() {
        assert_eq!(S_ISREG(S_IFREG | 0o644), 1);
        assert_eq!(S_ISREG(S_IFDIR | 0o755), 0);
        assert_eq!(S_ISREG(0), 0);
    }

    #[test]
    fn test_s_isdir() {
        assert_eq!(S_ISDIR(S_IFDIR | 0o755), 1);
        assert_eq!(S_ISDIR(S_IFREG | 0o644), 0);
    }

    #[test]
    fn test_s_islnk() {
        assert_eq!(S_ISLNK(S_IFLNK | 0o777), 1);
        assert_eq!(S_ISLNK(S_IFREG | 0o644), 0);
    }

    #[test]
    fn test_s_ischr() {
        assert_eq!(S_ISCHR(S_IFCHR | 0o666), 1);
        assert_eq!(S_ISCHR(S_IFBLK | 0o660), 0);
    }

    #[test]
    fn test_s_isblk() {
        assert_eq!(S_ISBLK(S_IFBLK | 0o660), 1);
        assert_eq!(S_ISBLK(S_IFCHR | 0o666), 0);
    }

    #[test]
    fn test_s_isfifo() {
        assert_eq!(S_ISFIFO(S_IFIFO | 0o644), 1);
        assert_eq!(S_ISFIFO(S_IFREG | 0o644), 0);
    }

    #[test]
    fn test_s_issock() {
        assert_eq!(S_ISSOCK(S_IFSOCK | 0o755), 1);
        assert_eq!(S_ISSOCK(S_IFREG | 0o644), 0);
    }

    // -- Stat struct methods --

    #[test]
    fn test_stat_is_file() {
        let mut st = Stat::zeroed();
        st.st_mode = S_IFREG | 0o644;
        assert!(st.is_file());
        assert!(!st.is_dir());
        assert!(!st.is_link());
    }

    #[test]
    fn test_stat_is_dir() {
        let mut st = Stat::zeroed();
        st.st_mode = S_IFDIR | 0o755;
        assert!(st.is_dir());
        assert!(!st.is_file());
    }

    #[test]
    fn test_stat_is_link() {
        let mut st = Stat::zeroed();
        st.st_mode = S_IFLNK | 0o777;
        assert!(st.is_link());
        assert!(!st.is_file());
        assert!(!st.is_dir());
    }

    #[test]
    fn test_stat_is_chr() {
        let mut st = Stat::zeroed();
        st.st_mode = S_IFCHR | 0o666;
        assert!(st.is_chr());
    }

    #[test]
    fn test_stat_is_blk() {
        let mut st = Stat::zeroed();
        st.st_mode = S_IFBLK | 0o660;
        assert!(st.is_blk());
    }

    #[test]
    fn test_stat_is_fifo() {
        let mut st = Stat::zeroed();
        st.st_mode = S_IFIFO | 0o644;
        assert!(st.is_fifo());
    }

    #[test]
    fn test_stat_is_sock() {
        let mut st = Stat::zeroed();
        st.st_mode = S_IFSOCK;
        assert!(st.is_sock());
    }

    // -- Stat struct layout --

    #[test]
    fn test_stat_size() {
        // Linux x86_64 struct stat is 144 bytes.
        assert_eq!(core::mem::size_of::<Stat>(), 144);
    }

    #[test]
    fn test_stat_zeroed() {
        let st = Stat::zeroed();
        assert_eq!(st.st_dev, 0);
        assert_eq!(st.st_ino, 0);
        assert_eq!(st.st_mode, 0);
        assert_eq!(st.st_size, 0);
    }

    // -- Timespec layout --

    #[test]
    fn test_timespec_size() {
        assert_eq!(core::mem::size_of::<Timespec>(), 16);
    }

    #[test]
    fn test_timespec_default() {
        let ts = Timespec::default();
        assert_eq!(ts.tv_sec, 0);
        assert_eq!(ts.tv_nsec, 0);
    }

    // -- File mode constants match Linux --

    #[test]
    fn test_mode_constants() {
        assert_eq!(S_IFMT, 0o170_000);
        assert_eq!(S_IFREG, 0o100_000);
        assert_eq!(S_IFDIR, 0o040_000);
        assert_eq!(S_IFLNK, 0o120_000);
        assert_eq!(S_IFCHR, 0o020_000);
        assert_eq!(S_IFBLK, 0o060_000);
        assert_eq!(S_IFIFO, 0o010_000);
        assert_eq!(S_IFSOCK, 0o140_000);
    }

    #[test]
    fn test_permission_constants() {
        assert_eq!(S_IRUSR, 0o400);
        assert_eq!(S_IWUSR, 0o200);
        assert_eq!(S_IXUSR, 0o100);
        assert_eq!(S_IRGRP, 0o040);
        assert_eq!(S_IWGRP, 0o020);
        assert_eq!(S_IXGRP, 0o010);
        assert_eq!(S_IROTH, 0o004);
        assert_eq!(S_IWOTH, 0o002);
        assert_eq!(S_IXOTH, 0o001);
    }

    #[test]
    fn test_special_bits() {
        assert_eq!(S_ISUID, 0o4000);
        assert_eq!(S_ISGID, 0o2000);
        assert_eq!(S_ISVTX, 0o1000);
    }

    // -- All types are disjoint --

    #[test]
    fn test_file_types_disjoint() {
        let types = [S_IFREG, S_IFDIR, S_IFLNK, S_IFCHR, S_IFBLK, S_IFIFO, S_IFSOCK];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(
                    types[i], types[j],
                    "file types must be disjoint"
                );
            }
        }
    }

    // -- mknod/mkfifo stubs return ENOSYS --

    #[test]
    fn test_mknod_returns_enosys() {
        assert_eq!(mknod(b"/dev/null\0".as_ptr(), S_IFCHR | 0o666, 0), -1);
    }

    #[test]
    fn test_mknodat_returns_enosys() {
        assert_eq!(mknodat(-1, b"node\0".as_ptr(), S_IFCHR | 0o666, 0), -1);
    }

    #[test]
    fn test_mkfifo_returns_enosys() {
        assert_eq!(mkfifo(b"/tmp/fifo\0".as_ptr(), 0o644), -1);
    }

    #[test]
    fn test_mkfifoat_returns_enosys() {
        assert_eq!(mkfifoat(-1, b"fifo\0".as_ptr(), 0o644), -1);
    }

    // -- S_IS* functions edge cases --

    #[test]
    fn test_s_isreg_with_permissions() {
        // Regular file with setuid bit — still a regular file.
        assert_eq!(S_ISREG(S_IFREG | S_ISUID | 0o755), 1);
    }

    #[test]
    fn test_s_isdir_with_sticky() {
        // Directory with sticky bit — still a directory.
        assert_eq!(S_ISDIR(S_IFDIR | S_ISVTX | 0o755), 1);
    }

    #[test]
    fn test_stat_methods_consistent_with_c_functions() {
        let mut st = Stat::zeroed();
        st.st_mode = S_IFREG | 0o644;
        assert_eq!(S_ISREG(st.st_mode) != 0, st.is_file());
        assert_eq!(S_ISDIR(st.st_mode) != 0, st.is_dir());
        assert_eq!(S_ISLNK(st.st_mode) != 0, st.is_link());
    }
}
