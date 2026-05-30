//! POSIX and Linux filesystem statistics.
//!
//! Implements `statvfs`, `fstatvfs` (POSIX) and `statfs`, `fstatfs`
//! (Linux) stubs.
//!
//! ## Implementation
//!
//! On bare metal these query the kernel `SYS_FS_STATVFS` syscall (which
//! returns a 64-byte block: block size, total/free blocks, total/free
//! inodes, max name length, and a read-only flag) and translate the
//! result into the POSIX/Linux structures.  The fd-based variants look up
//! the path stored for the descriptor and delegate to the path-based
//! query; descriptors with no stored path (pipes, sockets) fall back to
//! defaults.
//!
//! In host unit tests (where no kernel is present) the functions return
//! reasonable defaults — large free space, 16 KiB block size matching the
//! OS page size — so the structure-filling logic stays exercisable.  The
//! kernel-result translation itself is tested directly via
//! [`fill_statvfs_from_raw`] / [`fill_statfs_from_raw`].

use crate::errno;

// ---------------------------------------------------------------------------
// Default filesystem statistics constants
// ---------------------------------------------------------------------------
//
// These values are returned by statvfs/statfs when the kernel doesn't
// have real filesystem statistics syscalls.  Centralized here so they
// are easy to update when the kernel gains support.

/// Default filesystem block size (matches OS 16 KiB page size).
const DEFAULT_BLOCK_SIZE: u64 = 16384;

/// Default total filesystem size in bytes (10 GiB).
const DEFAULT_FS_TOTAL_BYTES: u64 = 10 * 1024 * 1024 * 1024;

/// Default free space in bytes (1 GiB).
const DEFAULT_FS_FREE_BYTES: u64 = 1024 * 1024 * 1024;

/// Default total inode count.
const DEFAULT_INODE_TOTAL: u64 = 1_000_000;

/// Default free inode count.
const DEFAULT_INODE_FREE: u64 = 500_000;

/// Maximum filename length (matches limits::NAME_MAX / Linux ext4).
const DEFAULT_NAMEMAX: u64 = crate::limits::NAME_MAX as u64;

// ---------------------------------------------------------------------------
// Structures
// ---------------------------------------------------------------------------

/// Filesystem statistics (struct statvfs).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Statvfs {
    /// Filesystem block size.
    pub f_bsize: u64,
    /// Fragment size.
    pub f_frsize: u64,
    /// Total number of blocks.
    pub f_blocks: u64,
    /// Free blocks.
    pub f_bfree: u64,
    /// Free blocks available to non-root.
    pub f_bavail: u64,
    /// Total inodes.
    pub f_files: u64,
    /// Free inodes.
    pub f_ffree: u64,
    /// Free inodes available to non-root.
    pub f_favail: u64,
    /// Filesystem ID.
    pub f_fsid: u64,
    /// Mount flags.
    pub f_flag: u64,
    /// Maximum filename length.
    pub f_namemax: u64,
}

// ---------------------------------------------------------------------------
// Mount flags
// ---------------------------------------------------------------------------

/// Read-only filesystem.
pub const ST_RDONLY: u64 = 1;
/// Don't allow setuid/setgid.
pub const ST_NOSUID: u64 = 2;

// ---------------------------------------------------------------------------
// Kernel `SYS_FS_STATVFS` result translation
// ---------------------------------------------------------------------------
//
// The kernel writes a fixed 64-byte block (little-endian):
//   [0..8]   block_size    u64
//   [8..16]  total_blocks  u64
//   [16..24] free_blocks   u64
//   [24..32] total_inodes  u64
//   [32..40] free_inodes   u64
//   [40..48] max_name_len  u64
//   [48]     read_only     u8 (0/1)
//   [49..64] reserved

/// Number of bytes the kernel writes for a `SYS_FS_STATVFS` result.
///
/// Only referenced by the syscall-decoding helpers below, which are
/// compiled on bare metal (and under `test`); a plain host build does
/// not use it.
#[cfg(any(target_os = "none", test))]
const KERNEL_STATVFS_LEN: usize = 64;

/// Read a little-endian `u64` at `off` from a kernel statvfs block.
#[cfg(any(target_os = "none", test))]
fn rd_u64(raw: &[u8; KERNEL_STATVFS_LEN], off: usize) -> u64 {
    let mut b = [0u8; 8];
    b.copy_from_slice(&raw[off..off + 8]);
    u64::from_le_bytes(b)
}

/// Translate a kernel statvfs block into a POSIX `struct statvfs`.
#[cfg(any(target_os = "none", test))]
fn fill_statvfs_from_raw(buf: &mut Statvfs, raw: &[u8; KERNEL_STATVFS_LEN]) {
    let block_size = rd_u64(raw, 0);
    let read_only = raw[48] != 0;
    let max_name_len = rd_u64(raw, 40);

    // Guard against a zero block size from virtual filesystems: callers
    // such as df divide by it.
    buf.f_bsize = if block_size == 0 { DEFAULT_BLOCK_SIZE } else { block_size };
    buf.f_frsize = buf.f_bsize;
    buf.f_blocks = rd_u64(raw, 8);
    buf.f_bfree = rd_u64(raw, 16);
    buf.f_bavail = buf.f_bfree;
    buf.f_files = rd_u64(raw, 24);
    buf.f_ffree = rd_u64(raw, 32);
    buf.f_favail = buf.f_ffree;
    buf.f_fsid = 1;
    buf.f_flag = if read_only { ST_RDONLY } else { 0 };
    buf.f_namemax = if max_name_len == 0 { DEFAULT_NAMEMAX } else { max_name_len };
}

/// Translate a kernel statvfs block into a Linux `struct statfs`.
#[cfg(any(target_os = "none", test))]
fn fill_statfs_from_raw(buf: &mut Statfs, raw: &[u8; KERNEL_STATVFS_LEN]) {
    let block_size = rd_u64(raw, 0);
    let read_only = raw[48] != 0;
    let max_name_len = rd_u64(raw, 40);
    let bsize = if block_size == 0 { DEFAULT_BLOCK_SIZE } else { block_size };
    let namelen = if max_name_len == 0 { DEFAULT_NAMEMAX } else { max_name_len };

    // The kernel ABI doesn't convey the filesystem type magic; report
    // ext4 (the primary on-disk filesystem) as before.
    buf.f_type = EXT4_SUPER_MAGIC;
    buf.f_bsize = i64::try_from(bsize).unwrap_or(i64::MAX);
    buf.f_blocks = rd_u64(raw, 8);
    buf.f_bfree = rd_u64(raw, 16);
    buf.f_bavail = buf.f_bfree;
    buf.f_files = rd_u64(raw, 24);
    buf.f_ffree = rd_u64(raw, 32);
    buf.f_fsid = [1, 0];
    buf.f_namelen = i64::try_from(namelen).unwrap_or(i64::MAX);
    buf.f_frsize = buf.f_bsize;
    buf.f_flags = if read_only { ST_RDONLY as i64 } else { 0 };
    buf.f_spare = [0; 4];
}

/// Resolve a path and query the kernel for `struct statvfs` (bare metal).
///
/// `path` must be non-null (checked by the caller).
#[cfg(target_os = "none")]
fn query_statvfs(path: *const u8, buf: *mut Statvfs) -> i32 {
    let mut resolved = [0u8; crate::unistd::PATH_MAX];
    // SAFETY: caller guarantees `path` is a non-null valid C string.
    let Some(len) = (unsafe { crate::unistd::resolve_path(path, &mut resolved) }) else {
        // SAFETY: caller guarantees `path` is a valid C string.
        if unsafe { *path } == 0 {
            errno::set_errno(errno::ENOENT);
        } else {
            errno::set_errno(errno::ENAMETOOLONG);
        }
        return -1;
    };
    let mut raw = [0u8; KERNEL_STATVFS_LEN];
    let ret = crate::syscall::syscall3(
        crate::syscall::SYS_FS_STATVFS,
        resolved.as_ptr() as u64,
        len as u64,
        raw.as_mut_ptr() as u64,
    );
    if ret < 0 {
        return errno::translate(ret) as i32;
    }
    // SAFETY: caller guarantees `buf` is non-null and writable.
    fill_statvfs_from_raw(unsafe { &mut *buf }, &raw);
    0
}

/// Resolve a path and query the kernel for `struct statfs` (bare metal).
#[cfg(target_os = "none")]
fn query_statfs(path: *const u8, buf: *mut Statfs) -> i32 {
    let mut resolved = [0u8; crate::unistd::PATH_MAX];
    // SAFETY: caller guarantees `path` is a non-null valid C string.
    let Some(len) = (unsafe { crate::unistd::resolve_path(path, &mut resolved) }) else {
        // SAFETY: caller guarantees `path` is a valid C string.
        if unsafe { *path } == 0 {
            errno::set_errno(errno::ENOENT);
        } else {
            errno::set_errno(errno::ENAMETOOLONG);
        }
        return -1;
    };
    let mut raw = [0u8; KERNEL_STATVFS_LEN];
    let ret = crate::syscall::syscall3(
        crate::syscall::SYS_FS_STATVFS,
        resolved.as_ptr() as u64,
        len as u64,
        raw.as_mut_ptr() as u64,
    );
    if ret < 0 {
        return errno::translate(ret) as i32;
    }
    // SAFETY: caller guarantees `buf` is non-null and writable.
    fill_statfs_from_raw(unsafe { &mut *buf }, &raw);
    0
}

// ---------------------------------------------------------------------------
// Functions
// ---------------------------------------------------------------------------

/// Get filesystem statistics for a path.
///
/// On bare metal this queries the kernel and reports the real filesystem
/// capacity/usage.  In host tests it reports defaults (1 GiB free on a
/// 10 GiB filesystem).  Returns 0 on success, -1 on error.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn statvfs(path: *const u8, buf: *mut Statvfs) -> i32 {
    if path.is_null() || buf.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    #[cfg(target_os = "none")]
    {
        query_statvfs(path, buf)
    }
    #[cfg(not(target_os = "none"))]
    {
        fill_defaults(buf);
        0
    }
}

/// Get filesystem statistics for an open file descriptor.
///
/// Validates `fd` (must be non-negative and open) and `buf` (must be
/// non-NULL).  On bare metal the path stored for the descriptor is
/// resolved and queried; descriptors with no stored path (pipes,
/// sockets) report defaults.
///
/// Errors:
///   * `EBADF` — `fd` is negative or not open.
///   * `EFAULT` — `buf` is NULL.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fstatvfs(fd: i32, buf: *mut Statvfs) -> i32 {
    if fd < 0 {
        errno::set_errno(errno::EBADF);
        return -1;
    }
    if crate::fdtable::get_fd(fd).is_none() {
        errno::set_errno(errno::EBADF);
        return -1;
    }
    if buf.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    #[cfg(target_os = "none")]
    {
        let mut path = [0u8; crate::unistd::PATH_MAX];
        let len = crate::fdtable::get_fd_path(fd, &mut path);
        if len == 0 {
            // No stored path (pipe/socket/etc.) — report defaults.
            fill_defaults(buf);
            return 0;
        }
        query_statvfs(path.as_ptr(), buf)
    }
    #[cfg(not(target_os = "none"))]
    {
        fill_defaults(buf);
        0
    }
}

/// Fill a Statvfs with reasonable defaults.
///
/// Uses 16 KiB block size to match the OS page size.
fn fill_defaults(buf: *mut Statvfs) {
    // SAFETY: Caller verified buf is non-null.
    let s = unsafe { &mut *buf };
    s.f_bsize = DEFAULT_BLOCK_SIZE;
    s.f_frsize = DEFAULT_BLOCK_SIZE;
    #[allow(clippy::arithmetic_side_effects)]
    {
        s.f_blocks = DEFAULT_FS_TOTAL_BYTES / DEFAULT_BLOCK_SIZE;
        s.f_bfree = DEFAULT_FS_FREE_BYTES / DEFAULT_BLOCK_SIZE;
    }
    s.f_bavail = s.f_bfree;
    s.f_files = DEFAULT_INODE_TOTAL;
    s.f_ffree = DEFAULT_INODE_FREE;
    s.f_favail = s.f_ffree;
    s.f_fsid = 1;
    s.f_flag = 0;
    s.f_namemax = DEFAULT_NAMEMAX;
}

// ===========================================================================
// Linux `statfs` / `fstatfs`
// ===========================================================================

/// Linux filesystem statistics (struct statfs).
///
/// Different from `statvfs` — has different field names and includes
/// filesystem type.  Many Linux programs use `statfs` directly instead
/// of the POSIX `statvfs`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Statfs {
    /// Filesystem type magic number.
    pub f_type: i64,
    /// Optimal transfer block size.
    pub f_bsize: i64,
    /// Total data blocks.
    pub f_blocks: u64,
    /// Free blocks.
    pub f_bfree: u64,
    /// Free blocks available to unprivileged user.
    pub f_bavail: u64,
    /// Total file nodes.
    pub f_files: u64,
    /// Free file nodes.
    pub f_ffree: u64,
    /// Filesystem ID.
    pub f_fsid: [i32; 2],
    /// Maximum filename length.
    pub f_namelen: i64,
    /// Fragment size.
    pub f_frsize: i64,
    /// Mount flags.
    pub f_flags: i64,
    /// Padding.
    f_spare: [i64; 4],
}

/// ext4 filesystem magic number.
const EXT4_SUPER_MAGIC: i64 = 0xEF53;

/// Fill a Statfs with reasonable defaults.
fn fill_statfs_defaults(buf: *mut Statfs) {
    // SAFETY: Caller verified buf is non-null.
    let s = unsafe { &mut *buf };
    s.f_type = EXT4_SUPER_MAGIC;
    s.f_bsize = DEFAULT_BLOCK_SIZE as i64;
    #[allow(clippy::arithmetic_side_effects)]
    {
        s.f_blocks = DEFAULT_FS_TOTAL_BYTES / DEFAULT_BLOCK_SIZE;
        s.f_bfree = DEFAULT_FS_FREE_BYTES / DEFAULT_BLOCK_SIZE;
    }
    s.f_bavail = s.f_bfree;
    s.f_files = DEFAULT_INODE_TOTAL;
    s.f_ffree = DEFAULT_INODE_FREE;
    s.f_fsid = [1, 0];
    s.f_namelen = DEFAULT_NAMEMAX as i64;
    s.f_frsize = DEFAULT_BLOCK_SIZE as i64;
    s.f_flags = 0;
    s.f_spare = [0; 4];
}

/// Get filesystem statistics (Linux).
///
/// On bare metal this queries the kernel; in host tests it reports
/// defaults.  Returns 0 on success, -1 on error.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn statfs(path: *const u8, buf: *mut Statfs) -> i32 {
    if path.is_null() || buf.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    #[cfg(target_os = "none")]
    {
        query_statfs(path, buf)
    }
    #[cfg(not(target_os = "none"))]
    {
        fill_statfs_defaults(buf);
        0
    }
}

/// Get filesystem statistics for an fd (Linux).
///
/// Validates `fd` (must be non-negative and open) and `buf` (must be
/// non-NULL).  On bare metal the path stored for the descriptor is
/// resolved and queried; descriptors with no stored path report
/// defaults.
///
/// Errors:
///   * `EBADF` — `fd` is negative or not open.
///   * `EFAULT` — `buf` is NULL.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fstatfs(fd: i32, buf: *mut Statfs) -> i32 {
    if fd < 0 {
        errno::set_errno(errno::EBADF);
        return -1;
    }
    if crate::fdtable::get_fd(fd).is_none() {
        errno::set_errno(errno::EBADF);
        return -1;
    }
    if buf.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    #[cfg(target_os = "none")]
    {
        let mut path = [0u8; crate::unistd::PATH_MAX];
        let len = crate::fdtable::get_fd_path(fd, &mut path);
        if len == 0 {
            fill_statfs_defaults(buf);
            return 0;
        }
        query_statfs(path.as_ptr(), buf)
    }
    #[cfg(not(target_os = "none"))]
    {
        fill_statfs_defaults(buf);
        0
    }
}

/// `statfs64` — LP64 alias (off_t already 64-bit).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn statfs64(path: *const u8, buf: *mut Statfs) -> i32 {
    statfs(path, buf)
}

/// `fstatfs64` — LP64 alias.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fstatfs64(fd: i32, buf: *mut Statfs) -> i32 {
    fstatfs(fd, buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem;

    // -----------------------------------------------------------------------
    // Test helpers
    // -----------------------------------------------------------------------

    /// Allocate a real, open fd for tests that need a valid file descriptor.
    ///
    /// `fstatvfs`/`fstatfs` (and their 64-bit aliases) now validate that the
    /// fd is open before doing any work, so tests can no longer pick a fixed
    /// number and hope.  Allocate via `fdtable::alloc_fd` to guarantee the
    /// kernel sees an open File handle, then `close_test_fd` to release it.
    fn alloc_test_fd() -> i32 {
        crate::fdtable::alloc_fd(crate::fdtable::HandleKind::File, 0)
            .expect("alloc_fd File failed")
    }

    fn close_test_fd(fd: i32) {
        let _ = crate::fdtable::close_fd(fd);
    }

    // -----------------------------------------------------------------------
    // Constants
    // -----------------------------------------------------------------------

    #[test]
    fn mount_flag_constants() {
        assert_eq!(ST_RDONLY, 1);
        assert_eq!(ST_NOSUID, 2);
    }

    // -----------------------------------------------------------------------
    // Struct layout
    // -----------------------------------------------------------------------

    #[test]
    fn statvfs_struct_size() {
        // Statvfs has 11 u64 fields = 11 * 8 = 88 bytes.
        assert_eq!(mem::size_of::<Statvfs>(), 11 * 8);
    }

    #[test]
    fn statfs_struct_size() {
        // Statfs: f_type (i64) + f_bsize (i64) + f_blocks (u64) + f_bfree (u64)
        //       + f_bavail (u64) + f_files (u64) + f_ffree (u64)
        //       + f_fsid ([i32; 2] = 8) + f_namelen (i64) + f_frsize (i64)
        //       + f_flags (i64) + f_spare ([i64; 4] = 32)
        // = 7*8 + 5*8 + 8 + 32 = 56 + 40 + 8 + 32 = 136 -- let's just check
        // it is the expected value.
        let expected = 2 * 8   // f_type, f_bsize (i64)
            + 5 * 8            // f_blocks..f_ffree (u64)
            + 8                // f_fsid ([i32; 2])
            + 3 * 8            // f_namelen, f_frsize, f_flags (i64)
            + 4 * 8;           // f_spare ([i64; 4])
        assert_eq!(mem::size_of::<Statfs>(), expected);
    }

    // -----------------------------------------------------------------------
    // statvfs — success
    // -----------------------------------------------------------------------

    #[test]
    fn statvfs_returns_zero_for_valid_args() {
        let path = b"/\0";
        let mut buf = unsafe { mem::zeroed::<Statvfs>() };
        let ret = statvfs(path.as_ptr(), &mut buf as *mut Statvfs);
        assert_eq!(ret, 0);
    }

    #[test]
    fn statvfs_fills_block_size() {
        let path = b"/\0";
        let mut buf = unsafe { mem::zeroed::<Statvfs>() };
        statvfs(path.as_ptr(), &mut buf as *mut Statvfs);
        assert_eq!(buf.f_bsize, 16384, "block size should be 16 KiB");
        assert_eq!(buf.f_frsize, 16384, "fragment size should be 16 KiB");
    }

    #[test]
    fn statvfs_fills_namemax() {
        let path = b"/\0";
        let mut buf = unsafe { mem::zeroed::<Statvfs>() };
        statvfs(path.as_ptr(), &mut buf as *mut Statvfs);
        assert_eq!(buf.f_namemax, 255);
    }

    #[test]
    fn statvfs_fills_space_defaults() {
        let path = b"/\0";
        let mut buf = unsafe { mem::zeroed::<Statvfs>() };
        statvfs(path.as_ptr(), &mut buf as *mut Statvfs);

        // 10 GiB total in 16 KiB blocks.
        let expected_total = 10 * 1024 * 1024 * 1024_u64 / 16384;
        assert_eq!(buf.f_blocks, expected_total, "total blocks should represent 10 GiB");

        // 1 GiB free in 16 KiB blocks.
        let expected_free = 1024 * 1024 * 1024_u64 / 16384;
        assert_eq!(buf.f_bfree, expected_free, "free blocks should represent 1 GiB");
        assert_eq!(buf.f_bavail, expected_free, "available blocks should equal free blocks");
    }

    #[test]
    fn statvfs_fills_inode_defaults() {
        let path = b"/\0";
        let mut buf = unsafe { mem::zeroed::<Statvfs>() };
        statvfs(path.as_ptr(), &mut buf as *mut Statvfs);
        assert_eq!(buf.f_files, 1_000_000);
        assert_eq!(buf.f_ffree, 500_000);
        assert_eq!(buf.f_favail, 500_000);
    }

    // -----------------------------------------------------------------------
    // statvfs — null arguments
    // -----------------------------------------------------------------------

    #[test]
    fn statvfs_null_path_returns_negative_one() {
        let mut buf = unsafe { mem::zeroed::<Statvfs>() };
        let ret = statvfs(core::ptr::null(), &mut buf as *mut Statvfs);
        assert_eq!(ret, -1);
    }

    #[test]
    fn statvfs_null_buf_returns_negative_one() {
        let path = b"/\0";
        let ret = statvfs(path.as_ptr(), core::ptr::null_mut());
        assert_eq!(ret, -1);
    }

    #[test]
    fn statvfs_both_null_returns_negative_one() {
        let ret = statvfs(core::ptr::null(), core::ptr::null_mut());
        assert_eq!(ret, -1);
    }

    // -----------------------------------------------------------------------
    // fstatvfs — success
    // -----------------------------------------------------------------------

    #[test]
    fn fstatvfs_returns_zero_for_valid_fd() {
        let fd = alloc_test_fd();
        let mut buf = unsafe { mem::zeroed::<Statvfs>() };
        let ret = fstatvfs(fd, &mut buf as *mut Statvfs);
        assert_eq!(ret, 0);
        close_test_fd(fd);
    }

    #[test]
    fn fstatvfs_fills_same_defaults_as_statvfs() {
        let fd = alloc_test_fd();
        let mut buf1 = unsafe { mem::zeroed::<Statvfs>() };
        let mut buf2 = unsafe { mem::zeroed::<Statvfs>() };
        let path = b"/\0";
        statvfs(path.as_ptr(), &mut buf1 as *mut Statvfs);
        fstatvfs(fd, &mut buf2 as *mut Statvfs);

        assert_eq!(buf1.f_bsize, buf2.f_bsize);
        assert_eq!(buf1.f_frsize, buf2.f_frsize);
        assert_eq!(buf1.f_blocks, buf2.f_blocks);
        assert_eq!(buf1.f_bfree, buf2.f_bfree);
        assert_eq!(buf1.f_bavail, buf2.f_bavail);
        assert_eq!(buf1.f_files, buf2.f_files);
        assert_eq!(buf1.f_ffree, buf2.f_ffree);
        assert_eq!(buf1.f_favail, buf2.f_favail);
        assert_eq!(buf1.f_namemax, buf2.f_namemax);
        close_test_fd(fd);
    }

    // -----------------------------------------------------------------------
    // fstatvfs — null buf
    // -----------------------------------------------------------------------

    #[test]
    fn fstatvfs_null_buf_returns_negative_one() {
        let fd = alloc_test_fd();
        let ret = fstatvfs(fd, core::ptr::null_mut());
        assert_eq!(ret, -1);
        close_test_fd(fd);
    }

    // -----------------------------------------------------------------------
    // statfs — success
    // -----------------------------------------------------------------------

    #[test]
    fn statfs_returns_zero_for_valid_args() {
        let path = b"/\0";
        let mut buf = unsafe { mem::zeroed::<Statfs>() };
        let ret = statfs(path.as_ptr(), &mut buf as *mut Statfs);
        assert_eq!(ret, 0);
    }

    #[test]
    fn statfs_fills_ext4_magic() {
        let path = b"/\0";
        let mut buf = unsafe { mem::zeroed::<Statfs>() };
        statfs(path.as_ptr(), &mut buf as *mut Statfs);
        assert_eq!(buf.f_type, 0xEF53, "filesystem type should be ext4 magic");
    }

    #[test]
    fn statfs_fills_block_size() {
        let path = b"/\0";
        let mut buf = unsafe { mem::zeroed::<Statfs>() };
        statfs(path.as_ptr(), &mut buf as *mut Statfs);
        assert_eq!(buf.f_bsize, 16384, "block size should be 16 KiB");
        assert_eq!(buf.f_frsize, 16384, "fragment size should be 16 KiB");
    }

    #[test]
    fn statfs_fills_space_defaults() {
        let path = b"/\0";
        let mut buf = unsafe { mem::zeroed::<Statfs>() };
        statfs(path.as_ptr(), &mut buf as *mut Statfs);

        let expected_total = 10 * 1024 * 1024 * 1024_u64 / 16384;
        assert_eq!(buf.f_blocks, expected_total);

        let expected_free = 1024 * 1024 * 1024_u64 / 16384;
        assert_eq!(buf.f_bfree, expected_free);
        assert_eq!(buf.f_bavail, expected_free);
    }

    #[test]
    fn statfs_fills_namelen() {
        let path = b"/\0";
        let mut buf = unsafe { mem::zeroed::<Statfs>() };
        statfs(path.as_ptr(), &mut buf as *mut Statfs);
        assert_eq!(buf.f_namelen, 255);
    }

    #[test]
    fn statfs_fills_inode_defaults() {
        let path = b"/\0";
        let mut buf = unsafe { mem::zeroed::<Statfs>() };
        statfs(path.as_ptr(), &mut buf as *mut Statfs);
        assert_eq!(buf.f_files, 1_000_000);
        assert_eq!(buf.f_ffree, 500_000);
    }

    // -----------------------------------------------------------------------
    // statfs — null arguments
    // -----------------------------------------------------------------------

    #[test]
    fn statfs_null_path_returns_negative_one() {
        let mut buf = unsafe { mem::zeroed::<Statfs>() };
        let ret = statfs(core::ptr::null(), &mut buf as *mut Statfs);
        assert_eq!(ret, -1);
    }

    #[test]
    fn statfs_null_buf_returns_negative_one() {
        let path = b"/\0";
        let ret = statfs(path.as_ptr(), core::ptr::null_mut());
        assert_eq!(ret, -1);
    }

    // -----------------------------------------------------------------------
    // fstatfs — success
    // -----------------------------------------------------------------------

    #[test]
    fn fstatfs_returns_zero_for_valid_fd() {
        let fd = alloc_test_fd();
        let mut buf = unsafe { mem::zeroed::<Statfs>() };
        let ret = fstatfs(fd, &mut buf as *mut Statfs);
        assert_eq!(ret, 0);
        close_test_fd(fd);
    }

    #[test]
    fn fstatfs_fills_same_defaults_as_statfs() {
        let fd = alloc_test_fd();
        let mut buf1 = unsafe { mem::zeroed::<Statfs>() };
        let mut buf2 = unsafe { mem::zeroed::<Statfs>() };
        let path = b"/\0";
        statfs(path.as_ptr(), &mut buf1 as *mut Statfs);
        fstatfs(fd, &mut buf2 as *mut Statfs);

        assert_eq!(buf1.f_type, buf2.f_type);
        assert_eq!(buf1.f_bsize, buf2.f_bsize);
        assert_eq!(buf1.f_blocks, buf2.f_blocks);
        assert_eq!(buf1.f_bfree, buf2.f_bfree);
        assert_eq!(buf1.f_bavail, buf2.f_bavail);
        assert_eq!(buf1.f_files, buf2.f_files);
        assert_eq!(buf1.f_ffree, buf2.f_ffree);
        assert_eq!(buf1.f_namelen, buf2.f_namelen);
        assert_eq!(buf1.f_frsize, buf2.f_frsize);
        close_test_fd(fd);
    }

    // -----------------------------------------------------------------------
    // fstatfs — null buf
    // -----------------------------------------------------------------------

    #[test]
    fn fstatfs_null_buf_returns_negative_one() {
        let fd = alloc_test_fd();
        let ret = fstatfs(fd, core::ptr::null_mut());
        assert_eq!(ret, -1);
        close_test_fd(fd);
    }

    // -----------------------------------------------------------------------
    // Default value verification (cross-cutting)
    // -----------------------------------------------------------------------

    #[test]
    fn default_block_size_is_16kib() {
        let fd = alloc_test_fd();
        let mut buf = unsafe { mem::zeroed::<Statvfs>() };
        fstatvfs(fd, &mut buf as *mut Statvfs);
        assert_eq!(buf.f_bsize, 16384);
        assert_eq!(buf.f_bsize, 16 * 1024);
        close_test_fd(fd);
    }

    #[test]
    fn default_total_is_10gib() {
        let fd = alloc_test_fd();
        let mut buf = unsafe { mem::zeroed::<Statvfs>() };
        fstatvfs(fd, &mut buf as *mut Statvfs);
        let total_bytes = buf.f_blocks * buf.f_bsize;
        assert_eq!(total_bytes, 10 * 1024 * 1024 * 1024);
        close_test_fd(fd);
    }

    #[test]
    fn default_free_is_1gib() {
        let fd = alloc_test_fd();
        let mut buf = unsafe { mem::zeroed::<Statvfs>() };
        fstatvfs(fd, &mut buf as *mut Statvfs);
        let free_bytes = buf.f_bfree * buf.f_bsize;
        assert_eq!(free_bytes, 1024 * 1024 * 1024);
        close_test_fd(fd);
    }

    #[test]
    fn statfs64_aliases_statfs() {
        let path = b"/\0";
        let mut buf1 = unsafe { mem::zeroed::<Statfs>() };
        let mut buf2 = unsafe { mem::zeroed::<Statfs>() };
        let ret1 = statfs(path.as_ptr(), &mut buf1 as *mut Statfs);
        let ret2 = statfs64(path.as_ptr(), &mut buf2 as *mut Statfs);
        assert_eq!(ret1, ret2);
        assert_eq!(buf1.f_type, buf2.f_type);
        assert_eq!(buf1.f_bsize, buf2.f_bsize);
    }

    #[test]
    fn fstatfs64_aliases_fstatfs() {
        let fd = alloc_test_fd();
        let mut buf1 = unsafe { mem::zeroed::<Statfs>() };
        let mut buf2 = unsafe { mem::zeroed::<Statfs>() };
        let ret1 = fstatfs(fd, &mut buf1 as *mut Statfs);
        let ret2 = fstatfs64(fd, &mut buf2 as *mut Statfs);
        assert_eq!(ret1, ret2);
        assert_eq!(buf1.f_type, buf2.f_type);
        assert_eq!(buf1.f_bsize, buf2.f_bsize);
        close_test_fd(fd);
    }

    // -----------------------------------------------------------------------
    // Constants are consistent across statvfs and statfs
    // -----------------------------------------------------------------------

    #[test]
    fn statvfs_and_statfs_agree_on_block_size() {
        let path = b"/\0";
        let mut vbuf = unsafe { mem::zeroed::<Statvfs>() };
        let mut sbuf = unsafe { mem::zeroed::<Statfs>() };
        statvfs(path.as_ptr(), &mut vbuf);
        statfs(path.as_ptr(), &mut sbuf);
        assert_eq!(vbuf.f_bsize, sbuf.f_bsize as u64);
        assert_eq!(vbuf.f_frsize, sbuf.f_frsize as u64);
    }

    #[test]
    fn statvfs_and_statfs_agree_on_space() {
        let path = b"/\0";
        let mut vbuf = unsafe { mem::zeroed::<Statvfs>() };
        let mut sbuf = unsafe { mem::zeroed::<Statfs>() };
        statvfs(path.as_ptr(), &mut vbuf);
        statfs(path.as_ptr(), &mut sbuf);
        assert_eq!(vbuf.f_blocks, sbuf.f_blocks);
        assert_eq!(vbuf.f_bfree, sbuf.f_bfree);
        assert_eq!(vbuf.f_files, sbuf.f_files);
        assert_eq!(vbuf.f_ffree, sbuf.f_ffree);
    }

    #[test]
    fn statvfs_and_statfs_agree_on_namemax() {
        let path = b"/\0";
        let mut vbuf = unsafe { mem::zeroed::<Statvfs>() };
        let mut sbuf = unsafe { mem::zeroed::<Statfs>() };
        statvfs(path.as_ptr(), &mut vbuf);
        statfs(path.as_ptr(), &mut sbuf);
        assert_eq!(vbuf.f_namemax, sbuf.f_namelen as u64);
    }

    // -----------------------------------------------------------------------
    // Named constant verification
    // -----------------------------------------------------------------------

    #[test]
    fn default_constants_match_design() {
        // Block size matches OS 16 KiB page size.
        assert_eq!(DEFAULT_BLOCK_SIZE, 16384);
        // Total filesystem is 10 GiB.
        assert_eq!(DEFAULT_FS_TOTAL_BYTES, 10 * 1024 * 1024 * 1024);
        // Free space is 1 GiB.
        assert_eq!(DEFAULT_FS_FREE_BYTES, 1024 * 1024 * 1024);
        // Inodes match expected values.
        assert_eq!(DEFAULT_INODE_TOTAL, 1_000_000);
        assert_eq!(DEFAULT_INODE_FREE, 500_000);
        // Namemax matches ext4.
        assert_eq!(DEFAULT_NAMEMAX, 255);
    }

    #[test]
    fn free_does_not_exceed_total() {
        assert!(DEFAULT_FS_FREE_BYTES <= DEFAULT_FS_TOTAL_BYTES);
        assert!(DEFAULT_INODE_FREE <= DEFAULT_INODE_TOTAL);
    }

    // =====================================================================
    // Phase 73 — fstatvfs / fstatfs / fstatfs64 fd validation
    //
    // Linux's fstatvfs/fstatfs prologues validate the fd before touching
    // the user buffer: a negative or unopen fd yields -1/EBADF.  After
    // the fd passes, a NULL buf yields -1/EFAULT.  This matches our
    // implementation order: fd<0 → get_fd None → buf.is_null().
    // =====================================================================

    // ---- Per-error class: bad fd ----

    #[test]
    fn fstatvfs_negative_fd_returns_ebadf() {
        let mut buf = unsafe { mem::zeroed::<Statvfs>() };
        errno::set_errno(0);
        assert_eq!(fstatvfs(-1, &mut buf as *mut Statvfs), -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn fstatvfs_large_negative_fd_returns_ebadf() {
        let mut buf = unsafe { mem::zeroed::<Statvfs>() };
        errno::set_errno(0);
        assert_eq!(fstatvfs(i32::MIN, &mut buf as *mut Statvfs), -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn fstatvfs_unopen_fd_returns_ebadf() {
        let probe: i32 = 0x4000_0060;
        let _ = crate::fdtable::close_fd(probe);
        let mut buf = unsafe { mem::zeroed::<Statvfs>() };
        errno::set_errno(0);
        assert_eq!(fstatvfs(probe, &mut buf as *mut Statvfs), -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn fstatfs_negative_fd_returns_ebadf() {
        let mut buf = unsafe { mem::zeroed::<Statfs>() };
        errno::set_errno(0);
        assert_eq!(fstatfs(-1, &mut buf as *mut Statfs), -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn fstatfs_unopen_fd_returns_ebadf() {
        let probe: i32 = 0x4000_0061;
        let _ = crate::fdtable::close_fd(probe);
        let mut buf = unsafe { mem::zeroed::<Statfs>() };
        errno::set_errno(0);
        assert_eq!(fstatfs(probe, &mut buf as *mut Statfs), -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn fstatfs64_negative_fd_returns_ebadf() {
        // fstatfs64 is the LP64 alias — must inherit fd validation.
        let mut buf = unsafe { mem::zeroed::<Statfs>() };
        errno::set_errno(0);
        assert_eq!(fstatfs64(-1, &mut buf as *mut Statfs), -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn fstatfs64_unopen_fd_returns_ebadf() {
        let probe: i32 = 0x4000_0062;
        let _ = crate::fdtable::close_fd(probe);
        let mut buf = unsafe { mem::zeroed::<Statfs>() };
        errno::set_errno(0);
        assert_eq!(fstatfs64(probe, &mut buf as *mut Statfs), -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    // ---- Per-error class: NULL buf with open fd ----

    #[test]
    fn fstatvfs_open_fd_null_buf_returns_efault() {
        let fd = alloc_test_fd();
        errno::set_errno(0);
        assert_eq!(fstatvfs(fd, core::ptr::null_mut()), -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
        close_test_fd(fd);
    }

    #[test]
    fn fstatfs_open_fd_null_buf_returns_efault() {
        let fd = alloc_test_fd();
        errno::set_errno(0);
        assert_eq!(fstatfs(fd, core::ptr::null_mut()), -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
        close_test_fd(fd);
    }

    // ---- Validation ordering: bad fd beats NULL buf ----

    #[test]
    fn fstatvfs_bad_fd_beats_null_buf() {
        // Both fd<0 and buf=NULL.  Linux validates fd first → EBADF, not
        // EFAULT.
        errno::set_errno(0);
        assert_eq!(fstatvfs(-1, core::ptr::null_mut()), -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn fstatvfs_unopen_fd_beats_null_buf() {
        let probe: i32 = 0x4000_0063;
        let _ = crate::fdtable::close_fd(probe);
        errno::set_errno(0);
        assert_eq!(fstatvfs(probe, core::ptr::null_mut()), -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn fstatfs_bad_fd_beats_null_buf() {
        errno::set_errno(0);
        assert_eq!(fstatfs(-1, core::ptr::null_mut()), -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn fstatfs_unopen_fd_beats_null_buf() {
        let probe: i32 = 0x4000_0064;
        let _ = crate::fdtable::close_fd(probe);
        errno::set_errno(0);
        assert_eq!(fstatfs(probe, core::ptr::null_mut()), -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    // ---- Buggy-caller patterns ----

    #[test]
    fn fstatvfs_buggy_uninit_fd_returns_ebadf() {
        // Stack-uninitialised fd happens to be -1.
        let mut fd: i32 = -1;
        fd = fd.wrapping_add(0);
        let mut buf = unsafe { mem::zeroed::<Statvfs>() };
        errno::set_errno(0);
        assert_eq!(fstatvfs(fd, &mut buf as *mut Statvfs), -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn fstatvfs_buggy_double_use_after_close() {
        // Caller closes the fd, then queries fstatvfs on the stale handle.
        let fd = alloc_test_fd();
        close_test_fd(fd);
        let mut buf = unsafe { mem::zeroed::<Statvfs>() };
        errno::set_errno(0);
        assert_eq!(fstatvfs(fd, &mut buf as *mut Statvfs), -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn fstatfs_buggy_double_use_after_close() {
        let fd = alloc_test_fd();
        close_test_fd(fd);
        let mut buf = unsafe { mem::zeroed::<Statfs>() };
        errno::set_errno(0);
        assert_eq!(fstatfs(fd, &mut buf as *mut Statfs), -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    // ---- Workflow: validated success path fills buffer ----

    #[test]
    fn fstatvfs_workflow_validated_fd_fills_buffer() {
        // After fd validation passes, the buffer is filled with the
        // standard defaults.
        let fd = alloc_test_fd();
        let mut buf = unsafe { mem::zeroed::<Statvfs>() };
        assert_eq!(fstatvfs(fd, &mut buf as *mut Statvfs), 0);
        assert_eq!(buf.f_bsize, DEFAULT_BLOCK_SIZE);
        assert_eq!(buf.f_namemax, DEFAULT_NAMEMAX);
        close_test_fd(fd);
    }

    #[test]
    fn fstatfs_workflow_validated_fd_fills_buffer() {
        let fd = alloc_test_fd();
        let mut buf = unsafe { mem::zeroed::<Statfs>() };
        assert_eq!(fstatfs(fd, &mut buf as *mut Statfs), 0);
        assert_eq!(buf.f_type, EXT4_SUPER_MAGIC);
        assert_eq!(buf.f_bsize, DEFAULT_BLOCK_SIZE as i64);
        close_test_fd(fd);
    }

    // =====================================================================
    // Kernel SYS_FS_STATVFS result translation
    //
    // These exercise fill_statvfs_from_raw / fill_statfs_from_raw directly
    // — the logic the bare-metal statvfs/statfs paths use to interpret the
    // kernel's 64-byte block.  (The syscall itself is bare-metal-only and
    // can't run under the host harness.)
    // =====================================================================

    /// Build a 64-byte kernel statvfs block for tests.
    fn raw_statvfs(
        block_size: u64,
        total_blocks: u64,
        free_blocks: u64,
        total_inodes: u64,
        free_inodes: u64,
        max_name_len: u64,
        read_only: bool,
    ) -> [u8; KERNEL_STATVFS_LEN] {
        let mut raw = [0u8; KERNEL_STATVFS_LEN];
        raw[0..8].copy_from_slice(&block_size.to_le_bytes());
        raw[8..16].copy_from_slice(&total_blocks.to_le_bytes());
        raw[16..24].copy_from_slice(&free_blocks.to_le_bytes());
        raw[24..32].copy_from_slice(&total_inodes.to_le_bytes());
        raw[32..40].copy_from_slice(&free_inodes.to_le_bytes());
        raw[40..48].copy_from_slice(&max_name_len.to_le_bytes());
        raw[48] = u8::from(read_only);
        raw
    }

    #[test]
    fn fill_statvfs_from_raw_translates_all_fields() {
        let raw = raw_statvfs(4096, 1_000_000, 250_000, 64_000, 40_000, 255, false);
        let mut buf = unsafe { mem::zeroed::<Statvfs>() };
        fill_statvfs_from_raw(&mut buf, &raw);
        assert_eq!(buf.f_bsize, 4096);
        assert_eq!(buf.f_frsize, 4096);
        assert_eq!(buf.f_blocks, 1_000_000);
        assert_eq!(buf.f_bfree, 250_000);
        assert_eq!(buf.f_bavail, 250_000);
        assert_eq!(buf.f_files, 64_000);
        assert_eq!(buf.f_ffree, 40_000);
        assert_eq!(buf.f_favail, 40_000);
        assert_eq!(buf.f_namemax, 255);
        assert_eq!(buf.f_flag, 0);
    }

    #[test]
    fn fill_statvfs_from_raw_sets_rdonly_flag() {
        let raw = raw_statvfs(512, 10, 5, 4, 2, 255, true);
        let mut buf = unsafe { mem::zeroed::<Statvfs>() };
        fill_statvfs_from_raw(&mut buf, &raw);
        assert_eq!(buf.f_flag & ST_RDONLY, ST_RDONLY);
    }

    #[test]
    fn fill_statvfs_from_raw_guards_zero_block_size() {
        // A virtual filesystem reporting a zero block size must not leave a
        // zero in f_bsize (df divides by it).
        let raw = raw_statvfs(0, 0, 0, 0, 0, 0, false);
        let mut buf = unsafe { mem::zeroed::<Statvfs>() };
        fill_statvfs_from_raw(&mut buf, &raw);
        assert_eq!(buf.f_bsize, DEFAULT_BLOCK_SIZE);
        assert_eq!(buf.f_namemax, DEFAULT_NAMEMAX);
    }

    #[test]
    fn fill_statfs_from_raw_translates_all_fields() {
        let raw = raw_statvfs(8192, 500_000, 100_000, 32_000, 16_000, 255, true);
        let mut buf = unsafe { mem::zeroed::<Statfs>() };
        fill_statfs_from_raw(&mut buf, &raw);
        assert_eq!(buf.f_type, EXT4_SUPER_MAGIC);
        assert_eq!(buf.f_bsize, 8192);
        assert_eq!(buf.f_frsize, 8192);
        assert_eq!(buf.f_blocks, 500_000);
        assert_eq!(buf.f_bfree, 100_000);
        assert_eq!(buf.f_bavail, 100_000);
        assert_eq!(buf.f_files, 32_000);
        assert_eq!(buf.f_ffree, 16_000);
        assert_eq!(buf.f_namelen, 255);
        assert_eq!(buf.f_flags & ST_RDONLY as i64, ST_RDONLY as i64);
    }

    #[test]
    fn fill_statfs_from_raw_guards_zero_block_size() {
        let raw = raw_statvfs(0, 0, 0, 0, 0, 0, false);
        let mut buf = unsafe { mem::zeroed::<Statfs>() };
        fill_statfs_from_raw(&mut buf, &raw);
        assert_eq!(buf.f_bsize, DEFAULT_BLOCK_SIZE as i64);
        assert_eq!(buf.f_namelen, DEFAULT_NAMEMAX as i64);
    }
}
