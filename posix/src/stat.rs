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

/// Return `true` if the `S_IFMT` field of `mode` names a file type that
/// `mknod(2)` accepts.  Linux's `fs/namei.c::do_mknodat` rejects any
/// other value with `-EINVAL`, including `mode & S_IFMT == 0`.
///
/// Accepted types: regular (`S_IFREG`), character device (`S_IFCHR`),
/// block device (`S_IFBLK`), FIFO (`S_IFIFO`), socket (`S_IFSOCK`).
/// Directory and symlink are excluded because `mkdir`/`symlink` create
/// them.
#[must_use]
pub fn mknod_type_valid(mode: u32) -> bool {
    let t = mode & crate::fcntl::S_IFMT;
    t == crate::fcntl::S_IFREG
        || t == crate::fcntl::S_IFCHR
        || t == crate::fcntl::S_IFBLK
        || t == crate::fcntl::S_IFIFO
        || t == crate::fcntl::S_IFSOCK
}

/// Create a special or ordinary file.
///
/// Returns -1 with `ENOSYS` after argument-domain validation.  Our
/// filesystem doesn't support device nodes or special files yet, but
/// invalid callers must still see Linux-matching errno values so
/// portable code (udev, mdev, tmpfiles.d processors) reports failures
/// correctly.
///
/// Validation order matches `fs/namei.c::do_mknodat` in Linux:
/// 1. `pathname == NULL` → `EFAULT`.
/// 2. `pathname` is the empty string → `ENOENT`.
/// 3. `mode & S_IFMT` is not a valid file type → `EINVAL`.
///    Plain `0` (no type bits) is rejected — Linux treats that as
///    "create a regular file" in the BSD legacy interface but
///    `do_mknodat` is strict.  Our stub follows the strict path.
/// 4. All validated → `ENOSYS`.
///
/// Things we cannot validate yet:
/// - `EPERM`: CHR/BLK device creation requires `CAP_MKNOD`.
/// - `EEXIST`: pathname already exists.
/// - `ENOTDIR`/`ENOENT`: a path component is wrong.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn mknod(pathname: *const u8, mode: u32, _dev: u64) -> i32 {
    if pathname.is_null() {
        crate::errno::set_errno(crate::errno::EFAULT);
        return -1;
    }
    // SAFETY: pathname non-NULL; read one byte to detect empty string.
    if unsafe { *pathname } == 0 {
        crate::errno::set_errno(crate::errno::ENOENT);
        return -1;
    }
    if !mknod_type_valid(mode) {
        crate::errno::set_errno(crate::errno::EINVAL);
        return -1;
    }
    crate::errno::set_errno(crate::errno::ENOSYS);
    -1
}

/// Create a special file relative to a directory fd.
///
/// Returns -1 with `ENOSYS` after argument-domain validation, matching
/// `mknod` for path/mode and adding directory-fd checks.
///
/// Validation order:
/// 1. `pathname == NULL` → `EFAULT`.
/// 2. `pathname` empty → `ENOENT`.
/// 3. `mode & S_IFMT` invalid → `EINVAL`.
/// 4. `dirfd != AT_FDCWD` and `dirfd < 0` → `EBADF`.
/// 5. `dirfd != AT_FDCWD` and not an open fd → `EBADF`.
/// 6. All validated → `ENOSYS`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn mknodat(dirfd: i32, pathname: *const u8, mode: u32, _dev: u64) -> i32 {
    if pathname.is_null() {
        crate::errno::set_errno(crate::errno::EFAULT);
        return -1;
    }
    // SAFETY: pathname non-NULL.
    if unsafe { *pathname } == 0 {
        crate::errno::set_errno(crate::errno::ENOENT);
        return -1;
    }
    if !mknod_type_valid(mode) {
        crate::errno::set_errno(crate::errno::EINVAL);
        return -1;
    }
    if dirfd != crate::file::AT_FDCWD {
        if dirfd < 0 {
            crate::errno::set_errno(crate::errno::EBADF);
            return -1;
        }
        if crate::fdtable::get_fd(dirfd).is_none() {
            crate::errno::set_errno(crate::errno::EBADF);
            return -1;
        }
    }
    crate::errno::set_errno(crate::errno::ENOSYS);
    -1
}

/// Create a FIFO (named pipe).
///
/// Returns -1 with `ENOSYS` after argument-domain validation.  Named
/// pipes require kernel support for special file types in the
/// filesystem, which we don't have yet.
///
/// Validation order (matches `fs/namei.c::do_mkfifoat` in Linux):
/// 1. `pathname == NULL` → `EFAULT`.
/// 2. `pathname` empty → `ENOENT`.
/// 3. All validated → `ENOSYS`.  Linux does not validate `mode` bits
///    here — the type field is implicit (S_IFIFO) and the permission
///    bits are silently masked against the umask.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn mkfifo(pathname: *const u8, _mode: u32) -> i32 {
    if pathname.is_null() {
        crate::errno::set_errno(crate::errno::EFAULT);
        return -1;
    }
    // SAFETY: pathname non-NULL.
    if unsafe { *pathname } == 0 {
        crate::errno::set_errno(crate::errno::ENOENT);
        return -1;
    }
    crate::errno::set_errno(crate::errno::ENOSYS);
    -1
}

/// Create a FIFO relative to a directory fd.
///
/// Returns -1 with `ENOSYS` after argument-domain validation, matching
/// `mkfifo` plus directory-fd checks.
///
/// Validation order:
/// 1. `pathname == NULL` → `EFAULT`.
/// 2. `pathname` empty → `ENOENT`.
/// 3. `dirfd != AT_FDCWD` and `dirfd < 0` → `EBADF`.
/// 4. `dirfd != AT_FDCWD` and not an open fd → `EBADF`.
/// 5. All validated → `ENOSYS`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn mkfifoat(dirfd: i32, pathname: *const u8, _mode: u32) -> i32 {
    if pathname.is_null() {
        crate::errno::set_errno(crate::errno::EFAULT);
        return -1;
    }
    // SAFETY: pathname non-NULL.
    if unsafe { *pathname } == 0 {
        crate::errno::set_errno(crate::errno::ENOENT);
        return -1;
    }
    if dirfd != crate::file::AT_FDCWD {
        if dirfd < 0 {
            crate::errno::set_errno(crate::errno::EBADF);
            return -1;
        }
        if crate::fdtable::get_fd(dirfd).is_none() {
            crate::errno::set_errno(crate::errno::EBADF);
            return -1;
        }
    }
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
        // Phase 66: mknodat now rejects dirfd<0 (other than AT_FDCWD) with
        // EBADF before reaching ENOSYS.  Use AT_FDCWD so the call resolves
        // to the cwd path and reaches the ENOSYS sentinel.
        assert_eq!(
            mknodat(crate::file::AT_FDCWD, b"node\0".as_ptr(), S_IFCHR | 0o666, 0),
            -1,
        );
    }

    #[test]
    fn test_mkfifo_returns_enosys() {
        assert_eq!(mkfifo(b"/tmp/fifo\0".as_ptr(), 0o644), -1);
    }

    #[test]
    fn test_mkfifoat_returns_enosys() {
        // Phase 66: mkfifoat now rejects dirfd<0 (other than AT_FDCWD) with
        // EBADF.  Use AT_FDCWD so the call reaches ENOSYS.
        assert_eq!(mkfifoat(crate::file::AT_FDCWD, b"fifo\0".as_ptr(), 0o644), -1);
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

    // -- S_IS* with mode=0 (no type set) --

    #[test]
    fn test_s_is_functions_mode_zero() {
        assert_eq!(S_ISREG(0), 0);
        assert_eq!(S_ISDIR(0), 0);
        assert_eq!(S_ISLNK(0), 0);
        assert_eq!(S_ISCHR(0), 0);
        assert_eq!(S_ISBLK(0), 0);
        assert_eq!(S_ISFIFO(0), 0);
        assert_eq!(S_ISSOCK(0), 0);
    }

    // -- S_IS* with only permission bits (no type) --

    #[test]
    fn test_s_isreg_only_perms() {
        assert_eq!(S_ISREG(0o777), 0);
    }

    // -- Stat::is_* all false for mode=0 --

    #[test]
    fn test_stat_is_methods_mode_zero() {
        let st = Stat::zeroed();
        assert!(!st.is_file());
        assert!(!st.is_dir());
        assert!(!st.is_link());
        assert!(!st.is_chr());
        assert!(!st.is_blk());
        assert!(!st.is_fifo());
        assert!(!st.is_sock());
    }

    // -- Each type is exclusively one type --

    #[test]
    fn test_s_is_exclusive_reg() {
        let mode = S_IFREG | 0o644;
        assert_eq!(S_ISREG(mode), 1);
        assert_eq!(S_ISDIR(mode), 0);
        assert_eq!(S_ISLNK(mode), 0);
        assert_eq!(S_ISCHR(mode), 0);
        assert_eq!(S_ISBLK(mode), 0);
        assert_eq!(S_ISFIFO(mode), 0);
        assert_eq!(S_ISSOCK(mode), 0);
    }

    #[test]
    fn test_s_is_exclusive_dir() {
        let mode = S_IFDIR | 0o755;
        assert_eq!(S_ISDIR(mode), 1);
        assert_eq!(S_ISREG(mode), 0);
        assert_eq!(S_ISLNK(mode), 0);
        assert_eq!(S_ISCHR(mode), 0);
    }

    #[test]
    fn test_s_is_exclusive_lnk() {
        let mode = S_IFLNK | 0o777;
        assert_eq!(S_ISLNK(mode), 1);
        assert_eq!(S_ISREG(mode), 0);
        assert_eq!(S_ISDIR(mode), 0);
    }

    #[test]
    fn test_s_is_exclusive_chr() {
        let mode = S_IFCHR | 0o666;
        assert_eq!(S_ISCHR(mode), 1);
        assert_eq!(S_ISREG(mode), 0);
        assert_eq!(S_ISBLK(mode), 0);
    }

    #[test]
    fn test_s_is_exclusive_sock() {
        let mode = S_IFSOCK | 0o755;
        assert_eq!(S_ISSOCK(mode), 1);
        assert_eq!(S_ISREG(mode), 0);
        assert_eq!(S_ISFIFO(mode), 0);
    }

    // -- S_IFMT mask extracts type correctly --

    #[test]
    fn test_s_ifmt_extraction() {
        let mode = S_IFREG | S_ISUID | S_ISGID | S_ISVTX | 0o777;
        assert_eq!(mode & S_IFMT, S_IFREG);
    }

    #[test]
    fn test_s_ifmt_strips_permissions() {
        let mode = S_IFDIR | 0o777;
        assert_eq!(mode & S_IFMT, S_IFDIR);
        assert_eq!(mode & !S_IFMT, 0o777);
    }

    // -- Timespec --

    #[test]
    fn test_timespec_alignment() {
        assert_eq!(core::mem::align_of::<Timespec>(), 8);
    }

    #[test]
    fn test_timespec_field_values() {
        let ts = Timespec { tv_sec: 1000, tv_nsec: 500_000_000 };
        assert_eq!(ts.tv_sec, 1000);
        assert_eq!(ts.tv_nsec, 500_000_000);
    }

    // -- Stat struct alignment and field offsets --

    #[test]
    fn test_stat_alignment() {
        assert_eq!(core::mem::align_of::<Stat>(), 8);
    }

    #[test]
    fn test_stat_field_values() {
        let mut st = Stat::zeroed();
        st.st_ino = 12345;
        st.st_mode = S_IFREG | 0o644;
        st.st_nlink = 1;
        st.st_uid = 1000;
        st.st_gid = 1000;
        st.st_size = 4096;
        assert_eq!(st.st_ino, 12345);
        assert_eq!(st.st_nlink, 1);
        assert_eq!(st.st_uid, 1000);
        assert_eq!(st.st_gid, 1000);
        assert_eq!(st.st_size, 4096);
    }

    // -- Permission bit combining --

    #[test]
    fn test_permission_bits_compose() {
        // Owner rwx = 0o700
        assert_eq!(S_IRUSR | S_IWUSR | S_IXUSR, 0o700);
        // Group rwx = 0o070
        assert_eq!(S_IRGRP | S_IWGRP | S_IXGRP, 0o070);
        // Other rwx = 0o007
        assert_eq!(S_IROTH | S_IWOTH | S_IXOTH, 0o007);
        // All rwx = 0o777
        assert_eq!(
            S_IRUSR | S_IWUSR | S_IXUSR |
            S_IRGRP | S_IWGRP | S_IXGRP |
            S_IROTH | S_IWOTH | S_IXOTH,
            0o777,
        );
    }

    // -- mknod/mkfifo set errno --

    #[test]
    fn test_mknod_sets_enosys() {
        // Phase 66: mode=0 (no type bits) is now rejected with EINVAL
        // before reaching ENOSYS.  Use a valid type to reach the sentinel.
        crate::errno::set_errno(0);
        mknod(b"/tmp/n\0".as_ptr(), S_IFREG | 0o644, 0);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_mkfifo_sets_enosys() {
        crate::errno::set_errno(0);
        mkfifo(b"/tmp/f\0".as_ptr(), 0o644);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_mknodat_sets_enosys() {
        // Phase 66: mode=0 → EINVAL, dirfd=0 (not AT_FDCWD, not open) → EBADF.
        // Use S_IFREG type and AT_FDCWD to reach ENOSYS.
        crate::errno::set_errno(0);
        mknodat(crate::file::AT_FDCWD, b"n\0".as_ptr(), S_IFREG | 0o644, 0);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_mkfifoat_sets_enosys() {
        // Phase 66: dirfd=0 (not AT_FDCWD, not open) → EBADF.  Use AT_FDCWD.
        crate::errno::set_errno(0);
        mkfifoat(crate::file::AT_FDCWD, b"f\0".as_ptr(), 0o644);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    // -- Null pointer args don't crash --

    #[test]
    fn test_mknod_null_path() {
        assert_eq!(mknod(core::ptr::null(), 0, 0), -1);
    }

    #[test]
    fn test_mkfifo_null_path() {
        assert_eq!(mkfifo(core::ptr::null(), 0), -1);
    }

    // -----------------------------------------------------------------
    // Phase 66 — mknod / mknodat / mkfifo / mkfifoat full validators
    // -----------------------------------------------------------------

    // --- mknod_type_valid helper ---

    #[test]
    fn test_mknod_type_valid_accepts_reg() {
        assert!(mknod_type_valid(S_IFREG));
        assert!(mknod_type_valid(S_IFREG | 0o644));
    }

    #[test]
    fn test_mknod_type_valid_accepts_chr() {
        assert!(mknod_type_valid(S_IFCHR));
        assert!(mknod_type_valid(S_IFCHR | 0o666));
    }

    #[test]
    fn test_mknod_type_valid_accepts_blk() {
        assert!(mknod_type_valid(S_IFBLK));
        assert!(mknod_type_valid(S_IFBLK | 0o660));
    }

    #[test]
    fn test_mknod_type_valid_accepts_fifo() {
        assert!(mknod_type_valid(S_IFIFO));
        assert!(mknod_type_valid(S_IFIFO | 0o644));
    }

    #[test]
    fn test_mknod_type_valid_accepts_sock() {
        assert!(mknod_type_valid(S_IFSOCK));
        assert!(mknod_type_valid(S_IFSOCK | 0o755));
    }

    #[test]
    fn test_mknod_type_valid_rejects_dir() {
        // Directories are created via mkdir(2), not mknod.
        assert!(!mknod_type_valid(S_IFDIR | 0o755));
    }

    #[test]
    fn test_mknod_type_valid_rejects_symlink() {
        // Symlinks are created via symlink(2), not mknod.
        assert!(!mknod_type_valid(S_IFLNK | 0o777));
    }

    #[test]
    fn test_mknod_type_valid_rejects_zero() {
        // mode=0 has no type bits — Linux's do_mknodat rejects this.
        assert!(!mknod_type_valid(0));
        assert!(!mknod_type_valid(0o644));
    }

    // --- mknod: per-error-class ---

    #[test]
    fn test_mknod_null_efault() {
        crate::errno::set_errno(0);
        assert_eq!(mknod(core::ptr::null(), S_IFREG | 0o644, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_mknod_empty_enoent() {
        crate::errno::set_errno(0);
        assert_eq!(mknod(b"\0".as_ptr(), S_IFREG | 0o644, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOENT);
    }

    #[test]
    fn test_mknod_bad_type_einval() {
        crate::errno::set_errno(0);
        assert_eq!(mknod(b"/tmp/x\0".as_ptr(), S_IFDIR | 0o755, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_mknod_no_type_bits_einval() {
        crate::errno::set_errno(0);
        assert_eq!(mknod(b"/tmp/x\0".as_ptr(), 0o644, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_mknod_symlink_type_einval() {
        crate::errno::set_errno(0);
        assert_eq!(mknod(b"/tmp/x\0".as_ptr(), S_IFLNK | 0o777, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_mknod_valid_reaches_enosys() {
        crate::errno::set_errno(0);
        assert_eq!(mknod(b"/tmp/x\0".as_ptr(), S_IFCHR | 0o666, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    // --- mknod: ordering ---

    #[test]
    fn test_mknod_null_beats_bad_type() {
        // NULL path checked before mode validation.
        crate::errno::set_errno(0);
        assert_eq!(mknod(core::ptr::null(), S_IFDIR | 0o755, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_mknod_empty_beats_bad_type() {
        // Empty path checked before mode validation.
        crate::errno::set_errno(0);
        assert_eq!(mknod(b"\0".as_ptr(), S_IFDIR | 0o755, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOENT);
    }

    // --- mknodat: per-error-class ---

    #[test]
    fn test_mknodat_null_efault() {
        crate::errno::set_errno(0);
        assert_eq!(
            mknodat(crate::file::AT_FDCWD, core::ptr::null(), S_IFREG | 0o644, 0),
            -1,
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_mknodat_empty_enoent() {
        crate::errno::set_errno(0);
        assert_eq!(
            mknodat(crate::file::AT_FDCWD, b"\0".as_ptr(), S_IFREG | 0o644, 0),
            -1,
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOENT);
    }

    #[test]
    fn test_mknodat_bad_type_einval() {
        crate::errno::set_errno(0);
        assert_eq!(
            mknodat(crate::file::AT_FDCWD, b"n\0".as_ptr(), S_IFDIR | 0o755, 0),
            -1,
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_mknodat_negative_dirfd_ebadf() {
        crate::errno::set_errno(0);
        assert_eq!(mknodat(-1, b"n\0".as_ptr(), S_IFREG | 0o644, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_mknodat_nonexistent_fd_ebadf() {
        crate::errno::set_errno(0);
        // fd 9999 is overwhelmingly unlikely to be open.
        assert_eq!(mknodat(9999, b"n\0".as_ptr(), S_IFREG | 0o644, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_mknodat_at_fdcwd_reaches_enosys() {
        crate::errno::set_errno(0);
        assert_eq!(
            mknodat(crate::file::AT_FDCWD, b"n\0".as_ptr(), S_IFCHR | 0o666, 0),
            -1,
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    // --- mknodat: ordering ---

    #[test]
    fn test_mknodat_null_beats_bad_type() {
        // NULL path is checked first, before mode and dirfd.
        crate::errno::set_errno(0);
        assert_eq!(mknodat(-1, core::ptr::null(), S_IFDIR | 0, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_mknodat_empty_beats_bad_type() {
        crate::errno::set_errno(0);
        assert_eq!(mknodat(-1, b"\0".as_ptr(), S_IFDIR | 0, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOENT);
    }

    #[test]
    fn test_mknodat_bad_type_beats_bad_dirfd() {
        // Mode validation comes before dirfd validation.
        crate::errno::set_errno(0);
        assert_eq!(mknodat(-1, b"n\0".as_ptr(), S_IFDIR | 0o755, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // --- mkfifo: per-error-class ---

    #[test]
    fn test_mkfifo_null_efault() {
        crate::errno::set_errno(0);
        assert_eq!(mkfifo(core::ptr::null(), 0o644), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_mkfifo_empty_enoent() {
        crate::errno::set_errno(0);
        assert_eq!(mkfifo(b"\0".as_ptr(), 0o644), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOENT);
    }

    #[test]
    fn test_mkfifo_valid_reaches_enosys() {
        crate::errno::set_errno(0);
        assert_eq!(mkfifo(b"/tmp/fifo\0".as_ptr(), 0o644), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_mkfifo_any_mode_ok() {
        // Linux does not validate mode bits — only the type field matters
        // (implicit S_IFIFO) and even garbage mode bits are accepted at
        // this layer (perms are masked by umask later).
        crate::errno::set_errno(0);
        assert_eq!(mkfifo(b"/tmp/fifo\0".as_ptr(), 0xFFFF_FFFF), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    // --- mkfifo: ordering ---

    #[test]
    fn test_mkfifo_null_beats_empty() {
        // Trivially: NULL path is checked before the empty-string check
        // because dereferencing a NULL would crash.
        crate::errno::set_errno(0);
        assert_eq!(mkfifo(core::ptr::null(), 0o644), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    // --- mkfifoat: per-error-class ---

    #[test]
    fn test_mkfifoat_null_efault() {
        crate::errno::set_errno(0);
        assert_eq!(
            mkfifoat(crate::file::AT_FDCWD, core::ptr::null(), 0o644),
            -1,
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_mkfifoat_empty_enoent() {
        crate::errno::set_errno(0);
        assert_eq!(mkfifoat(crate::file::AT_FDCWD, b"\0".as_ptr(), 0o644), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOENT);
    }

    #[test]
    fn test_mkfifoat_negative_dirfd_ebadf() {
        crate::errno::set_errno(0);
        assert_eq!(mkfifoat(-1, b"f\0".as_ptr(), 0o644), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_mkfifoat_nonexistent_fd_ebadf() {
        crate::errno::set_errno(0);
        assert_eq!(mkfifoat(9999, b"f\0".as_ptr(), 0o644), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_mkfifoat_at_fdcwd_reaches_enosys() {
        crate::errno::set_errno(0);
        assert_eq!(mkfifoat(crate::file::AT_FDCWD, b"f\0".as_ptr(), 0o644), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    // --- mkfifoat: ordering ---

    #[test]
    fn test_mkfifoat_null_beats_bad_dirfd() {
        crate::errno::set_errno(0);
        assert_eq!(mkfifoat(-1, core::ptr::null(), 0o644), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_mkfifoat_empty_beats_bad_dirfd() {
        crate::errno::set_errno(0);
        assert_eq!(mkfifoat(-1, b"\0".as_ptr(), 0o644), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOENT);
    }

    // --- Real-world workflows ---

    #[test]
    fn test_workflow_udev_creates_dev_null() {
        // udev creating /dev/null: major=1, minor=3, mode=S_IFCHR | 0o666.
        // Linux makes this dev with makedev(1,3) → ((1u64) << 8) | 3.
        crate::errno::set_errno(0);
        let dev = (1u64 << 8) | 3;
        assert_eq!(
            mknod(b"/dev/null\0".as_ptr(), S_IFCHR | 0o666, dev),
            -1,
        );
        // Properly-formed call reaches ENOSYS (we don't implement nodes).
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_workflow_systemd_tmpfiles_creates_pipe() {
        // tmpfiles.d entry like:  p /run/initctl 0644 root root - -
        // would be implemented as mkfifo("/run/initctl", 0644).
        crate::errno::set_errno(0);
        assert_eq!(mkfifo(b"/run/initctl\0".as_ptr(), 0o644), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_workflow_mdev_block_device() {
        // mdev creates block device for /dev/sda: S_IFBLK | 0o660.
        crate::errno::set_errno(0);
        assert_eq!(
            mknod(b"/dev/sda\0".as_ptr(), S_IFBLK | 0o660, (8u64 << 8) | 0),
            -1,
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_workflow_mkfifoat_relative_to_dir() {
        // A daemon mkfifo()s relative to its working dir.  We use
        // AT_FDCWD since we can't open arbitrary directories in tests.
        crate::errno::set_errno(0);
        assert_eq!(
            mkfifoat(crate::file::AT_FDCWD, b"control\0".as_ptr(), 0o600),
            -1,
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    // --- Real-world buggy callers ---

    #[test]
    fn test_buggy_mknod_perms_only() {
        // Common bug: passing 0o644 to mknod expecting it to create a
        // regular file.  POSIX permits this (mode==0 → regular file in
        // some implementations) but Linux's do_mknodat is strict and
        // returns EINVAL.  We match Linux.
        crate::errno::set_errno(0);
        assert_eq!(mknod(b"/tmp/x\0".as_ptr(), 0o644, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_buggy_mknod_directory_type() {
        // Buggy caller tries to create a directory via mknod.  Must use
        // mkdir(2).  Linux rejects with EINVAL.
        crate::errno::set_errno(0);
        assert_eq!(mknod(b"/tmp/d\0".as_ptr(), S_IFDIR | 0o755, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_buggy_mknod_symlink_type() {
        // Buggy caller tries to create a symlink via mknod.  Must use
        // symlink(2).  Linux rejects with EINVAL.
        crate::errno::set_errno(0);
        assert_eq!(mknod(b"/tmp/l\0".as_ptr(), S_IFLNK | 0o777, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_buggy_mknodat_unopened_dirfd() {
        // Caller uses an fd that was never opened (or was closed).
        // Should get EBADF, not silently succeed.
        crate::errno::set_errno(0);
        assert_eq!(mknodat(12345, b"n\0".as_ptr(), S_IFREG | 0o644, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_buggy_mkfifoat_unopened_dirfd() {
        crate::errno::set_errno(0);
        assert_eq!(mkfifoat(12345, b"f\0".as_ptr(), 0o644), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EBADF);
    }

    #[test]
    fn test_buggy_mknod_no_pathname() {
        // Caller passes NULL pathname after a failed string construction.
        crate::errno::set_errno(0);
        assert_eq!(mknod(core::ptr::null(), S_IFREG | 0o644, 0), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }
}
