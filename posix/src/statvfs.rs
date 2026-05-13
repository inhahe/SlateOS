//! POSIX filesystem statistics.
//!
//! Implements `statvfs` and `fstatvfs` stubs.
//!
//! ## Implementation
//!
//! Our kernel doesn't have filesystem statistics syscalls yet.
//! These stubs return reasonable defaults (large free space, 4K
//! block size) so programs that check disk space don't fail.

use crate::errno;

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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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
    // Our OS uses 16 KiB pages; filesystem block size matches.
    s.f_bsize = 16384;
    s.f_frsize = 16384;
    // 10 GiB filesystem with 16 KiB blocks.
    #[allow(clippy::arithmetic_side_effects)]
    {
        s.f_blocks = 10 * 1024 * 1024 * 1024 / 16384; // 10 GiB
        s.f_bfree = 1024 * 1024 * 1024 / 16384;       // 1 GiB
    }
    s.f_bavail = s.f_bfree;
    s.f_files = 1_000_000;
    s.f_ffree = 500_000;
    s.f_favail = s.f_ffree;
    s.f_fsid = 1;
    s.f_flag = 0;
    s.f_namemax = 255;
}
