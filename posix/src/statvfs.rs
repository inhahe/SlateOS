//! POSIX and Linux filesystem statistics.
//!
//! Implements `statvfs`, `fstatvfs` (POSIX) and `statfs`, `fstatfs`
//! (Linux).
//!
//! ## Implementation
//!
//! On bare metal these query the kernel `SYS_FS_STATVFS` syscall (which
//! returns a 64-byte block: block size, total/free blocks, total/free
//! inodes, max name length, and a read-only flag) and translate the
//! result into the POSIX/Linux structures.  The fd-based variants look up
//! the path stored for the descriptor and delegate to the path-based
//! query; descriptors with no stored path (pipes, sockets) report
//! defaults.
//!
//! Every call finds its answer before it writes it, as Linux does
//! (`user_statfs` or `fd_statfs`, then the copy-out): a bad path or
//! descriptor is reported ahead of a NULL buffer.
//!
//! In host unit tests (where no kernel is present) the path is still
//! resolved -- that part is pure -- but the kernel's answer is replaced by
//! defaults (large free space, a 16 KiB block matching the OS page size),
//! which go through the same translation as a real answer. The translation
//! itself is tested directly via [`statvfs_from_raw`] / [`statfs_from_raw`].

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
    /// musl's trailing `__f_spare[6]`, which takes the struct from 88 bytes to
    /// 112. Every named field was already at the right offset; only the tail
    /// was short. Found by `scripts/check-libc-abi.py`.
    pub __f_spare: [u32; 6],
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
const KERNEL_STATVFS_LEN: usize = 64;

/// [`DEFAULT_FS_TOTAL_BYTES`] in [`DEFAULT_BLOCK_SIZE`] blocks.
const DEFAULT_BLOCKS: u64 = DEFAULT_FS_TOTAL_BYTES / DEFAULT_BLOCK_SIZE;

/// [`DEFAULT_FS_FREE_BYTES`] in [`DEFAULT_BLOCK_SIZE`] blocks.
const DEFAULT_FREE_BLOCKS: u64 = DEFAULT_FS_FREE_BYTES / DEFAULT_BLOCK_SIZE;

/// Read a little-endian `u64` at `off` from a kernel statvfs block.
///
/// Returns 0 if `off` would push the 8-byte read past the end of the
/// fixed-size block. All in-tree callers pass static offsets that fit
/// (0, 8, 16, 24, 32, 40 — see [`statvfs_from_raw`]), so this is a
/// defence-in-depth check that also satisfies clippy without panicking.
fn rd_u64(raw: &[u8; KERNEL_STATVFS_LEN], off: usize) -> u64 {
    raw.get(off..)
        .and_then(|s| s.get(..8))
        .and_then(|s| <[u8; 8]>::try_from(s).ok())
        .map_or(0, u64::from_le_bytes)
}

/// The block for a filesystem the kernel gives no figures for: the defaults
/// above, laid out as the kernel lays out a real answer.
///
/// A descriptor with no stored path (a pipe, a socket) reports it, and so
/// does every query in a host test, where there is no kernel to ask. Either
/// way it goes through the same translation as a real answer, so the two
/// cannot drift apart.
fn default_raw() -> [u8; KERNEL_STATVFS_LEN] {
    let mut raw = [0u8; KERNEL_STATVFS_LEN];
    let words = [
        DEFAULT_BLOCK_SIZE,
        DEFAULT_BLOCKS,
        DEFAULT_FREE_BLOCKS,
        DEFAULT_INODE_TOTAL,
        DEFAULT_INODE_FREE,
        DEFAULT_NAMEMAX,
    ];
    // The six words from offset 0; the read-only byte stays 0.  `as_chunks_mut`
    // rather than `chunks_exact_mut(8)`: clippy 1.98's
    // `chunks_exact_to_as_chunks` refuses a constant width the other way.
    let (slots, _) = raw.as_chunks_mut::<8>();
    for (slot, word) in slots.iter_mut().zip(words) {
        *slot = word.to_le_bytes();
    }
    raw
}

/// Translate a kernel statvfs block into a POSIX `struct statvfs`, as
/// glibc's `__internal_statvfs64` translates a `statfs` -- over the fields
/// the kernel reports, with the spare words zero.
fn statvfs_from_raw(raw: &[u8; KERNEL_STATVFS_LEN]) -> Statvfs {
    let block_size = rd_u64(raw, 0);
    let max_name_len = rd_u64(raw, 40);
    // Guard against a zero block size from virtual filesystems: callers
    // such as df divide by it.
    let bsize = if block_size == 0 {
        DEFAULT_BLOCK_SIZE
    } else {
        block_size
    };
    let bfree = rd_u64(raw, 16);
    let ffree = rd_u64(raw, 32);
    Statvfs {
        f_bsize: bsize,
        f_frsize: bsize,
        f_blocks: rd_u64(raw, 8),
        f_bfree: bfree,
        f_bavail: bfree,
        f_files: rd_u64(raw, 24),
        f_ffree: ffree,
        f_favail: ffree,
        f_fsid: 1,
        f_flag: if raw[48] != 0 { ST_RDONLY } else { 0 },
        f_namemax: if max_name_len == 0 {
            DEFAULT_NAMEMAX
        } else {
            max_name_len
        },
        __f_spare: [0; 6],
    }
}

/// Translate a kernel statvfs block into a Linux `struct statfs`.
fn statfs_from_raw(raw: &[u8; KERNEL_STATVFS_LEN]) -> Statfs {
    let block_size = rd_u64(raw, 0);
    let max_name_len = rd_u64(raw, 40);
    let bsize = if block_size == 0 {
        DEFAULT_BLOCK_SIZE
    } else {
        block_size
    };
    let namelen = if max_name_len == 0 {
        DEFAULT_NAMEMAX
    } else {
        max_name_len
    };
    let bsize = i64::try_from(bsize).unwrap_or(i64::MAX);
    let bfree = rd_u64(raw, 16);
    let read_only = if raw[48] != 0 { ST_RDONLY as i64 } else { 0 };
    Statfs {
        // The kernel ABI doesn't convey the filesystem type magic; report
        // ext4 (the primary on-disk filesystem).
        f_type: EXT4_SUPER_MAGIC,
        f_bsize: bsize,
        f_blocks: rd_u64(raw, 8),
        f_bfree: bfree,
        f_bavail: bfree,
        f_files: rd_u64(raw, 24),
        f_ffree: rd_u64(raw, 32),
        f_fsid: [1, 0],
        f_namelen: i64::try_from(namelen).unwrap_or(i64::MAX),
        f_frsize: bsize,
        // Linux sets ST_VALID on every answer (`calculate_f_flags`); it is
        // how a reader knows the flags mean anything.
        f_flags: ST_VALID | read_only,
        f_spare: [0; 4],
    }
}

// ---------------------------------------------------------------------------
// Queries
// ---------------------------------------------------------------------------

/// The kernel's block for the filesystem holding `path`, or `Err` with
/// `errno` set: `EFAULT` for a NULL path, `ENOENT` for an empty one,
/// `ENAMETOOLONG`, or whatever the kernel reports.
///
/// The path is resolved against the working directory on the host too --
/// resolving is pure -- so a path that cannot be resolved fails in a test as
/// it does on the target. Only the kernel's answer is replaced there, by
/// [`default_raw`].
fn query_path(path: *const u8) -> Result<[u8; KERNEL_STATVFS_LEN], ()> {
    if path.is_null() {
        // The kernel's `getname` faults reading it.
        errno::set_errno(errno::EFAULT);
        return Err(());
    }
    let mut resolved = [0u8; crate::unistd::PATH_MAX];
    let len = crate::file::resolve_or_err(path, &mut resolved).ok_or(())?;
    #[cfg(target_os = "none")]
    {
        let mut raw = [0u8; KERNEL_STATVFS_LEN];
        let ret = crate::syscall::syscall3(
            crate::syscall::SYS_FS_STATVFS,
            resolved.as_ptr() as u64,
            len as u64,
            raw.as_mut_ptr() as u64,
        );
        if ret < 0 {
            errno::set_errno(errno::errno_for(ret));
            return Err(());
        }
        Ok(raw)
    }
    #[cfg(not(target_os = "none"))]
    {
        // No kernel to ask: the path resolved, so the defaults answer.
        let _ = len;
        Ok(default_raw())
    }
}

/// The kernel's block for the filesystem holding the open descriptor `fd`,
/// or `Err` with `errno` set (`EBADF`, or the query's own error).
///
/// The query goes through the path stored for the descriptor; one with no
/// stored path (a pipe, a socket) reports the defaults.
fn query_fd(fd: i32) -> Result<[u8; KERNEL_STATVFS_LEN], ()> {
    if fd < 0 || crate::fdtable::get_fd(fd).is_none() {
        errno::set_errno(errno::EBADF);
        return Err(());
    }
    let mut path = [0u8; crate::unistd::PATH_MAX];
    if crate::fdtable::get_fd_path(fd, &mut path) == 0 {
        return Ok(default_raw());
    }
    // `get_fd_path` NUL-terminated what it stored.
    query_path(path.as_ptr())
}

/// Write a translated answer to the caller's buffer, or fail with `EFAULT`
/// for a NULL one.
///
/// This is the last step of every call, as it is on Linux: `statfs` and
/// `fstatfs` find their answer (`user_statfs`, `fd_statfs`) before
/// `do_statfs_native` copies it out, and glibc's `statvfs` and `fstatvfs`
/// make that call into a local before converting it into the caller's
/// buffer. So a bad path or descriptor is reported ahead of a NULL buffer --
/// which the kernel refuses with `EFAULT`, and glibc's conversion would
/// fault writing.
fn copy_out<T>(buf: *mut T, answer: T) -> i32 {
    if buf.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    // SAFETY: `buf` is non-null, and the caller's contract is that it points
    // to a writable, aligned `T`.
    unsafe { buf.write(answer) };
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
///
/// Errors, in Linux's order: `EFAULT` for a NULL `path`; `ENOENT` for an
/// empty one; `ENAMETOOLONG`; the kernel's lookup errors; and only then
/// `EFAULT` for a NULL `buf`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn statvfs(path: *const u8, buf: *mut Statvfs) -> i32 {
    match query_path(path) {
        Ok(raw) => copy_out(buf, statvfs_from_raw(&raw)),
        Err(()) => -1,
    }
}

/// Get filesystem statistics for an open file descriptor.
///
/// On bare metal the path stored for the descriptor is resolved and
/// queried; descriptors with no stored path (pipes, sockets) report
/// defaults.
///
/// Errors, in Linux's order:
///   * `EBADF` — `fd` is negative or not open.
///   * the query's own error, for a descriptor with a stored path.
///   * `EFAULT` — `buf` is NULL.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fstatvfs(fd: i32, buf: *mut Statvfs) -> i32 {
    match query_fd(fd) {
        Ok(raw) => copy_out(buf, statvfs_from_raw(&raw)),
        Err(()) => -1,
    }
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

/// `statfs`'s `f_flags` are meaningful (Linux's `include/linux/statfs.h`).
///
/// Linux sets it on every answer; glibc's `statvfs` removes it on the way to
/// `f_flag`, which is why `statvfs` does not report it and has no constant
/// for it.
const ST_VALID: i64 = 0x0020;

/// Get filesystem statistics (Linux).
///
/// On bare metal this queries the kernel; in host tests it reports
/// defaults.  Returns 0 on success, -1 on error, in [`statvfs`]'s order.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn statfs(path: *const u8, buf: *mut Statfs) -> i32 {
    match query_path(path) {
        Ok(raw) => copy_out(buf, statfs_from_raw(&raw)),
        Err(()) => -1,
    }
}

/// Get filesystem statistics for an fd (Linux).
///
/// On bare metal the path stored for the descriptor is resolved and
/// queried; descriptors with no stored path report defaults.
///
/// Errors, in [`fstatvfs`]'s order: `EBADF`, the query's own error, then
/// `EFAULT` for a NULL `buf`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fstatfs(fd: i32, buf: *mut Statfs) -> i32 {
    match query_fd(fd) {
        Ok(raw) => copy_out(buf, statfs_from_raw(&raw)),
        Err(()) => -1,
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
        crate::fdtable::alloc_fd(crate::fdtable::HandleKind::File, 0).expect("alloc_fd File failed")
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

    /// 112, which is musl's, not the 88 our eleven named fields come to.
    ///
    /// This asserted `11 * 8` -- a restatement of our own declaration rather
    /// than a claim about the C library, so it could only ever pass. musl
    /// carries `__f_spare[6]` after `f_namemax`, so a caller's object is 112
    /// bytes and `statvfs()` filled 88 of it. Every *named* field was already
    /// at the right offset, which is why nothing looked wrong. Found by
    /// `scripts/check-libc-abi.py`, which now asserts this against musl's own
    /// header on every push rather than against arithmetic.
    #[test]
    fn statvfs_struct_size() {
        assert_eq!(mem::size_of::<Statvfs>(), 112);
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
            + 4 * 8; // f_spare ([i64; 4])
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
        assert_eq!(
            buf.f_blocks, expected_total,
            "total blocks should represent 10 GiB"
        );

        // 1 GiB free in 16 KiB blocks.
        let expected_free = 1024 * 1024 * 1024_u64 / 16384;
        assert_eq!(
            buf.f_bfree, expected_free,
            "free blocks should represent 1 GiB"
        );
        assert_eq!(
            buf.f_bavail, expected_free,
            "available blocks should equal free blocks"
        );
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
    // These exercise statvfs_from_raw / statfs_from_raw directly
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
    fn statvfs_from_raw_translates_all_fields() {
        let raw = raw_statvfs(4096, 1_000_000, 250_000, 64_000, 40_000, 255, false);
        let buf = statvfs_from_raw(&raw);
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
    fn statvfs_from_raw_sets_rdonly_flag() {
        let raw = raw_statvfs(512, 10, 5, 4, 2, 255, true);
        let buf = statvfs_from_raw(&raw);
        assert_eq!(buf.f_flag & ST_RDONLY, ST_RDONLY);
    }

    #[test]
    fn statvfs_from_raw_guards_zero_block_size() {
        // A virtual filesystem reporting a zero block size must not leave a
        // zero in f_bsize (df divides by it).
        let raw = raw_statvfs(0, 0, 0, 0, 0, 0, false);
        let buf = statvfs_from_raw(&raw);
        assert_eq!(buf.f_bsize, DEFAULT_BLOCK_SIZE);
        assert_eq!(buf.f_namemax, DEFAULT_NAMEMAX);
    }

    #[test]
    fn statfs_from_raw_translates_all_fields() {
        let raw = raw_statvfs(8192, 500_000, 100_000, 32_000, 16_000, 255, true);
        let buf = statfs_from_raw(&raw);
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
    fn statfs_from_raw_guards_zero_block_size() {
        let raw = raw_statvfs(0, 0, 0, 0, 0, 0, false);
        let buf = statfs_from_raw(&raw);
        assert_eq!(buf.f_bsize, DEFAULT_BLOCK_SIZE as i64);
        assert_eq!(buf.f_namelen, DEFAULT_NAMEMAX as i64);
    }

    // =====================================================================
    // Linux's order: the answer is found before it is written
    //
    // `statfs`/`fstatfs` resolve the path or descriptor first
    // (`user_statfs`, `fd_statfs`) and copy the answer out last
    // (`do_statfs_native`); glibc's `statvfs`/`fstatvfs` make that call into
    // a local and convert it afterwards. So the buffer is judged last.
    // =====================================================================

    /// A path of `PATH_MAX` + 7 `a`s and a NUL: too long to resolve.
    fn overlong_path() -> [u8; crate::unistd::PATH_MAX + 8] {
        let mut long = [b'a'; crate::unistd::PATH_MAX + 8];
        if let Some(last) = long.last_mut() {
            *last = 0;
        }
        long
    }

    #[test]
    fn statvfs_null_path_is_efault() {
        let mut buf = statvfs_from_raw(&default_raw());
        errno::set_errno(0);
        assert_eq!(statvfs(core::ptr::null(), &mut buf), -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn statfs_null_path_is_efault() {
        let mut buf = statfs_from_raw(&default_raw());
        errno::set_errno(0);
        assert_eq!(statfs(core::ptr::null(), &mut buf), -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn statvfs_empty_path_beats_null_buf() {
        // Linux's lookup of "" is ENOENT, before any copy-out; the old
        // order answered EFAULT.
        errno::set_errno(0);
        assert_eq!(statvfs(b"\0".as_ptr(), core::ptr::null_mut()), -1);
        assert_eq!(errno::get_errno(), errno::ENOENT);
    }

    #[test]
    fn statfs_empty_path_beats_null_buf() {
        errno::set_errno(0);
        assert_eq!(statfs(b"\0".as_ptr(), core::ptr::null_mut()), -1);
        assert_eq!(errno::get_errno(), errno::ENOENT);
    }

    #[test]
    fn statfs64_empty_path_beats_null_buf() {
        errno::set_errno(0);
        assert_eq!(statfs64(b"\0".as_ptr(), core::ptr::null_mut()), -1);
        assert_eq!(errno::get_errno(), errno::ENOENT);
    }

    #[test]
    fn statvfs_overlong_path_beats_null_buf() {
        let long = overlong_path();
        errno::set_errno(0);
        assert_eq!(statvfs(long.as_ptr(), core::ptr::null_mut()), -1);
        assert_eq!(errno::get_errno(), errno::ENAMETOOLONG);
    }

    #[test]
    fn statfs_overlong_path_beats_null_buf() {
        let long = overlong_path();
        errno::set_errno(0);
        assert_eq!(statfs(long.as_ptr(), core::ptr::null_mut()), -1);
        assert_eq!(errno::get_errno(), errno::ENAMETOOLONG);
    }

    #[test]
    fn statvfs_good_path_null_buf_is_efault() {
        errno::set_errno(0);
        assert_eq!(statvfs(b"/\0".as_ptr(), core::ptr::null_mut()), -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn statfs_good_path_null_buf_is_efault() {
        errno::set_errno(0);
        assert_eq!(statfs(b"/\0".as_ptr(), core::ptr::null_mut()), -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn statvfs_relative_path_is_resolved_not_refused() {
        let mut buf = statvfs_from_raw(&raw_statvfs(1, 0, 0, 0, 0, 1, true));
        assert_eq!(statvfs(b"some/relative/../dir\0".as_ptr(), &mut buf), 0);
        assert_eq!(buf.f_bsize, DEFAULT_BLOCK_SIZE);
    }

    #[test]
    fn a_failed_statvfs_writes_nothing() {
        let mut buf = statvfs_from_raw(&default_raw());
        buf.f_bsize = 7;
        assert_eq!(statvfs(b"\0".as_ptr(), &mut buf), -1);
        assert_eq!(buf.f_bsize, 7);
    }

    #[test]
    fn statvfs_zeroes_the_spare_words() {
        // glibc's conversion zeroes `__f_spare`; the answer is written whole.
        let mut buf = statvfs_from_raw(&default_raw());
        buf.__f_spare = [0xdead_beef; 6];
        assert_eq!(statvfs(b"/\0".as_ptr(), &mut buf), 0);
        assert_eq!(buf.__f_spare, [0; 6]);
    }

    #[test]
    fn statfs_zeroes_the_spare_words() {
        let mut buf = statfs_from_raw(&default_raw());
        buf.f_spare = [-1; 4];
        assert_eq!(statfs(b"/\0".as_ptr(), &mut buf), 0);
        assert_eq!(buf.f_spare, [0; 4]);
    }

    #[test]
    fn statfs_flags_carry_st_valid() {
        let mut buf = statfs_from_raw(&raw_statvfs(1, 0, 0, 0, 0, 1, false));
        buf.f_flags = 0;
        assert_eq!(statfs(b"/\0".as_ptr(), &mut buf), 0);
        assert_eq!(buf.f_flags, ST_VALID);
        let ro = statfs_from_raw(&raw_statvfs(4096, 1, 1, 1, 1, 255, true));
        assert_eq!(ro.f_flags, ST_VALID | ST_RDONLY as i64);
    }

    #[test]
    fn statvfs_flag_does_not_carry_st_valid() {
        // glibc removes it: `f_flag` is the mount flags alone.
        let rw = statvfs_from_raw(&raw_statvfs(4096, 1, 1, 1, 1, 255, false));
        assert_eq!(rw.f_flag, 0);
        let ro = statvfs_from_raw(&raw_statvfs(4096, 1, 1, 1, 1, 255, true));
        assert_eq!(ro.f_flag, ST_RDONLY);
    }

    #[test]
    fn default_raw_translates_to_the_defaults() {
        // What the host and path-less descriptors report: the defaults,
        // field for field as the old `fill_defaults` wrote them.
        let v = statvfs_from_raw(&default_raw());
        assert_eq!(v.f_bsize, DEFAULT_BLOCK_SIZE);
        assert_eq!(v.f_frsize, DEFAULT_BLOCK_SIZE);
        assert_eq!(v.f_blocks, DEFAULT_FS_TOTAL_BYTES / DEFAULT_BLOCK_SIZE);
        assert_eq!(v.f_bfree, DEFAULT_FS_FREE_BYTES / DEFAULT_BLOCK_SIZE);
        assert_eq!(v.f_bavail, v.f_bfree);
        assert_eq!(v.f_files, DEFAULT_INODE_TOTAL);
        assert_eq!(v.f_ffree, DEFAULT_INODE_FREE);
        assert_eq!(v.f_favail, DEFAULT_INODE_FREE);
        assert_eq!(v.f_fsid, 1);
        assert_eq!(v.f_flag, 0);
        assert_eq!(v.f_namemax, DEFAULT_NAMEMAX);
        let f = statfs_from_raw(&default_raw());
        assert_eq!(f.f_type, EXT4_SUPER_MAGIC);
        assert_eq!(f.f_bsize, DEFAULT_BLOCK_SIZE as i64);
        assert_eq!(f.f_frsize, DEFAULT_BLOCK_SIZE as i64);
        assert_eq!(f.f_blocks, v.f_blocks);
        assert_eq!(f.f_bfree, v.f_bfree);
        assert_eq!(f.f_bavail, v.f_bfree);
        assert_eq!(f.f_files, v.f_files);
        assert_eq!(f.f_ffree, v.f_ffree);
        assert_eq!(f.f_namelen, DEFAULT_NAMEMAX as i64);
        assert_eq!(f.f_flags, ST_VALID);
    }

    #[test]
    fn fstatvfs_queries_a_stored_path() {
        let fd = alloc_test_fd();
        crate::fdtable::store_fd_path(fd, b"/mnt/data".as_ptr(), 9);
        let mut buf = statvfs_from_raw(&raw_statvfs(1, 0, 0, 0, 0, 1, true));
        assert_eq!(fstatvfs(fd, &mut buf), 0);
        assert_eq!(buf.f_bsize, DEFAULT_BLOCK_SIZE);
        assert_eq!(buf.f_flag, 0);
        close_test_fd(fd);
    }

    #[test]
    fn fstatfs_stored_path_null_buf_is_efault() {
        let fd = alloc_test_fd();
        crate::fdtable::store_fd_path(fd, b"/mnt/data".as_ptr(), 9);
        errno::set_errno(0);
        assert_eq!(fstatfs(fd, core::ptr::null_mut()), -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
        close_test_fd(fd);
    }
}
