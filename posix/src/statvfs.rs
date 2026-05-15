//! POSIX and Linux filesystem statistics.
//!
//! Implements `statvfs`, `fstatvfs` (POSIX) and `statfs`, `fstatfs`
//! (Linux) stubs.
//!
//! ## Implementation
//!
//! Our kernel doesn't have filesystem statistics syscalls yet.
//! These stubs return reasonable defaults (large free space, 16 KiB
//! block size matching the OS page size) so programs that check disk
//! space don't fail.

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
// Functions
// ---------------------------------------------------------------------------

/// Get filesystem statistics for a path.
///
/// Returns 0 on success, -1 on error.
/// Reports 1 GiB free space on a 10 GiB filesystem as defaults.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn statvfs(path: *const u8, buf: *mut Statvfs) -> i32 {
    if path.is_null() || buf.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    fill_defaults(buf);
    0
}

/// Get filesystem statistics for an open file descriptor.
///
/// Returns 0 on success, -1 on error.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fstatvfs(_fd: i32, buf: *mut Statvfs) -> i32 {
    if buf.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    fill_defaults(buf);
    0
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
/// Returns 0 on success, -1 on error.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn statfs(path: *const u8, buf: *mut Statfs) -> i32 {
    if path.is_null() || buf.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    fill_statfs_defaults(buf);
    0
}

/// Get filesystem statistics for an fd (Linux).
///
/// Returns 0 on success, -1 on error.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fstatfs(_fd: i32, buf: *mut Statfs) -> i32 {
    if buf.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    fill_statfs_defaults(buf);
    0
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
        let mut buf = unsafe { mem::zeroed::<Statvfs>() };
        let ret = fstatvfs(3, &mut buf as *mut Statvfs);
        assert_eq!(ret, 0);
    }

    #[test]
    fn fstatvfs_fills_same_defaults_as_statvfs() {
        let mut buf1 = unsafe { mem::zeroed::<Statvfs>() };
        let mut buf2 = unsafe { mem::zeroed::<Statvfs>() };
        let path = b"/\0";
        statvfs(path.as_ptr(), &mut buf1 as *mut Statvfs);
        fstatvfs(3, &mut buf2 as *mut Statvfs);

        assert_eq!(buf1.f_bsize, buf2.f_bsize);
        assert_eq!(buf1.f_frsize, buf2.f_frsize);
        assert_eq!(buf1.f_blocks, buf2.f_blocks);
        assert_eq!(buf1.f_bfree, buf2.f_bfree);
        assert_eq!(buf1.f_bavail, buf2.f_bavail);
        assert_eq!(buf1.f_files, buf2.f_files);
        assert_eq!(buf1.f_ffree, buf2.f_ffree);
        assert_eq!(buf1.f_favail, buf2.f_favail);
        assert_eq!(buf1.f_namemax, buf2.f_namemax);
    }

    // -----------------------------------------------------------------------
    // fstatvfs — null buf
    // -----------------------------------------------------------------------

    #[test]
    fn fstatvfs_null_buf_returns_negative_one() {
        let ret = fstatvfs(3, core::ptr::null_mut());
        assert_eq!(ret, -1);
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
        let mut buf = unsafe { mem::zeroed::<Statfs>() };
        let ret = fstatfs(3, &mut buf as *mut Statfs);
        assert_eq!(ret, 0);
    }

    #[test]
    fn fstatfs_fills_same_defaults_as_statfs() {
        let mut buf1 = unsafe { mem::zeroed::<Statfs>() };
        let mut buf2 = unsafe { mem::zeroed::<Statfs>() };
        let path = b"/\0";
        statfs(path.as_ptr(), &mut buf1 as *mut Statfs);
        fstatfs(3, &mut buf2 as *mut Statfs);

        assert_eq!(buf1.f_type, buf2.f_type);
        assert_eq!(buf1.f_bsize, buf2.f_bsize);
        assert_eq!(buf1.f_blocks, buf2.f_blocks);
        assert_eq!(buf1.f_bfree, buf2.f_bfree);
        assert_eq!(buf1.f_bavail, buf2.f_bavail);
        assert_eq!(buf1.f_files, buf2.f_files);
        assert_eq!(buf1.f_ffree, buf2.f_ffree);
        assert_eq!(buf1.f_namelen, buf2.f_namelen);
        assert_eq!(buf1.f_frsize, buf2.f_frsize);
    }

    // -----------------------------------------------------------------------
    // fstatfs — null buf
    // -----------------------------------------------------------------------

    #[test]
    fn fstatfs_null_buf_returns_negative_one() {
        let ret = fstatfs(3, core::ptr::null_mut());
        assert_eq!(ret, -1);
    }

    // -----------------------------------------------------------------------
    // Default value verification (cross-cutting)
    // -----------------------------------------------------------------------

    #[test]
    fn default_block_size_is_16kib() {
        let mut buf = unsafe { mem::zeroed::<Statvfs>() };
        fstatvfs(0, &mut buf as *mut Statvfs);
        assert_eq!(buf.f_bsize, 16384);
        assert_eq!(buf.f_bsize, 16 * 1024);
    }

    #[test]
    fn default_total_is_10gib() {
        let mut buf = unsafe { mem::zeroed::<Statvfs>() };
        fstatvfs(0, &mut buf as *mut Statvfs);
        let total_bytes = buf.f_blocks * buf.f_bsize;
        assert_eq!(total_bytes, 10 * 1024 * 1024 * 1024);
    }

    #[test]
    fn default_free_is_1gib() {
        let mut buf = unsafe { mem::zeroed::<Statvfs>() };
        fstatvfs(0, &mut buf as *mut Statvfs);
        let free_bytes = buf.f_bfree * buf.f_bsize;
        assert_eq!(free_bytes, 1024 * 1024 * 1024);
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
        let mut buf1 = unsafe { mem::zeroed::<Statfs>() };
        let mut buf2 = unsafe { mem::zeroed::<Statfs>() };
        let ret1 = fstatfs(3, &mut buf1 as *mut Statfs);
        let ret2 = fstatfs64(3, &mut buf2 as *mut Statfs);
        assert_eq!(ret1, ret2);
        assert_eq!(buf1.f_type, buf2.f_type);
        assert_eq!(buf1.f_bsize, buf2.f_bsize);
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
}
