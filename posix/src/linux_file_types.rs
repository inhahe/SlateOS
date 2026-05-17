//! `<linux/fs.h>` (file subset) — Open file description constants.
//!
//! A `struct file` represents an open file description — the state
//! created by open()/openat(). Multiple file descriptors (via dup/fork)
//! can share the same open file description, which holds the file
//! position, access mode, and status flags. The file's f_op table
//! dispatches read/write/ioctl/mmap to the filesystem or device driver.

// ---------------------------------------------------------------------------
// File access modes (O_ACCMODE bits in f_flags)
// ---------------------------------------------------------------------------

/// Read-only access.
pub const O_RDONLY: u32 = 0o0;
/// Write-only access.
pub const O_WRONLY: u32 = 0o1;
/// Read-write access.
pub const O_RDWR: u32 = 0o2;
/// Access mode mask.
pub const O_ACCMODE: u32 = 0o3;

// ---------------------------------------------------------------------------
// File creation flags (used at open time only)
// ---------------------------------------------------------------------------

/// Create file if it doesn't exist.
pub const O_CREAT: u32 = 0o100;
/// Fail if file exists (with O_CREAT).
pub const O_EXCL: u32 = 0o200;
/// Don't assign controlling terminal.
pub const O_NOCTTY: u32 = 0o400;
/// Truncate file to zero length.
pub const O_TRUNC: u32 = 0o1000;
/// Append mode (writes always at end).
pub const O_APPEND: u32 = 0o2000;
/// Non-blocking I/O.
pub const O_NONBLOCK: u32 = 0o4000;
/// Synchronous writes.
pub const O_DSYNC: u32 = 0o10000;
/// Signal-driven I/O.
pub const O_ASYNC: u32 = 0o20000;
/// Direct I/O (bypass page cache).
pub const O_DIRECT: u32 = 0o40000;
/// Allow opening large files on 32-bit.
pub const O_LARGEFILE: u32 = 0o100000;
/// Must be a directory.
pub const O_DIRECTORY: u32 = 0o200000;
/// Don't follow symlinks.
pub const O_NOFOLLOW: u32 = 0o400000;
/// Don't update atime.
pub const O_NOATIME: u32 = 0o1000000;
/// Set close-on-exec.
pub const O_CLOEXEC: u32 = 0o2000000;
/// Synchronous I/O (data + metadata).
pub const O_SYNC: u32 = 0o4010000;
/// Path-only open (no actual file access).
pub const O_PATH: u32 = 0o10000000;
/// Create unnamed temporary file.
pub const O_TMPFILE: u32 = 0o20200000;

// ---------------------------------------------------------------------------
// File lock types (flock)
// ---------------------------------------------------------------------------

/// Shared (read) lock.
pub const LOCK_SH: u32 = 1;
/// Exclusive (write) lock.
pub const LOCK_EX: u32 = 2;
/// Non-blocking lock request.
pub const LOCK_NB: u32 = 4;
/// Unlock.
pub const LOCK_UN: u32 = 8;

// ---------------------------------------------------------------------------
// Seek whence values
// ---------------------------------------------------------------------------

/// Seek from beginning of file.
pub const SEEK_SET: u32 = 0;
/// Seek from current position.
pub const SEEK_CUR: u32 = 1;
/// Seek from end of file.
pub const SEEK_END: u32 = 2;
/// Seek to next data region.
pub const SEEK_DATA: u32 = 3;
/// Seek to next hole.
pub const SEEK_HOLE: u32 = 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_access_modes() {
        assert_eq!(O_RDONLY & O_ACCMODE, O_RDONLY);
        assert_eq!(O_WRONLY & O_ACCMODE, O_WRONLY);
        assert_eq!(O_RDWR & O_ACCMODE, O_RDWR);
    }

    #[test]
    fn test_lock_types_distinct() {
        let locks = [LOCK_SH, LOCK_EX, LOCK_NB, LOCK_UN];
        for i in 0..locks.len() {
            for j in (i + 1)..locks.len() {
                assert_ne!(locks[i], locks[j]);
            }
        }
    }

    #[test]
    fn test_seek_values_distinct() {
        let seeks = [SEEK_SET, SEEK_CUR, SEEK_END, SEEK_DATA, SEEK_HOLE];
        for i in 0..seeks.len() {
            for j in (i + 1)..seeks.len() {
                assert_ne!(seeks[i], seeks[j]);
            }
        }
    }

    #[test]
    fn test_open_flags_nonzero() {
        // O_RDONLY is 0, so test the creation flags
        assert_ne!(O_CREAT, 0);
        assert_ne!(O_EXCL, 0);
        assert_ne!(O_TRUNC, 0);
        assert_ne!(O_APPEND, 0);
        assert_ne!(O_NONBLOCK, 0);
    }
}
