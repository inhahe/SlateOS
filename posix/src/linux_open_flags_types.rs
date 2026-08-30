//! `<fcntl.h>` — Open flag constants for open/openat/openat2.
//!
//! These flags control how files are opened: access mode,
//! creation behavior, synchronization, and other modifiers.

// ---------------------------------------------------------------------------
// Access mode flags (mutually exclusive lowest 2 bits)
// ---------------------------------------------------------------------------

/// Open for reading only.
pub const O_RDONLY: u32 = 0o0;
/// Open for writing only.
pub const O_WRONLY: u32 = 0o1;
/// Open for reading and writing.
pub const O_RDWR: u32 = 0o2;

// ---------------------------------------------------------------------------
// Access mode mask
// ---------------------------------------------------------------------------

/// Mask for access mode bits.
pub const O_ACCMODE: u32 = 0o3;

// ---------------------------------------------------------------------------
// Creation and status flags
// ---------------------------------------------------------------------------

/// Create file if it doesn't exist.
pub const O_CREAT: u32 = 0o100;
/// Fail if file exists (with O_CREAT).
pub const O_EXCL: u32 = 0o200;
/// Don't assign controlling terminal.
pub const O_NOCTTY: u32 = 0o400;
/// Truncate file to zero length.
pub const O_TRUNC: u32 = 0o1000;
/// Append on each write.
pub const O_APPEND: u32 = 0o2000;
/// Non-blocking mode.
pub const O_NONBLOCK_OPEN: u32 = 0o4000;
/// Synchronous writes.
pub const O_DSYNC: u32 = 0o10000;
/// Async I/O signal-driven.
pub const O_ASYNC: u32 = 0o20000;
/// Direct I/O (bypass page cache).
pub const O_DIRECT_OPEN: u32 = 0o40000;
/// Allow opening large files on 32-bit.
pub const O_LARGEFILE: u32 = 0o100000;
/// Must be a directory.
pub const O_DIRECTORY: u32 = 0o200000;
/// Don't follow symbolic links.
pub const O_NOFOLLOW: u32 = 0o400000;
/// Don't update access time.
pub const O_NOATIME: u32 = 0o1000000;
/// Set close-on-exec.
pub const O_CLOEXEC_OPEN: u32 = 0o2000000;
/// Synchronous I/O.
pub const O_SYNC: u32 = 0o4010000;
/// Open with path fd only (no I/O).
pub const O_PATH: u32 = 0o10000000;
/// Create unnamed temporary file.
pub const O_TMPFILE: u32 = 0o20200000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_access_modes() {
        assert_eq!(O_RDONLY, 0);
        assert_eq!(O_WRONLY, 1);
        assert_eq!(O_RDWR, 2);
    }

    #[test]
    fn test_accmode_mask() {
        assert_eq!(O_RDONLY & O_ACCMODE, O_RDONLY);
        assert_eq!(O_WRONLY & O_ACCMODE, O_WRONLY);
        assert_eq!(O_RDWR & O_ACCMODE, O_RDWR);
    }

    #[test]
    fn test_creation_flags_values() {
        assert_eq!(O_CREAT, 0o100);
        assert_eq!(O_EXCL, 0o200);
        assert_eq!(O_TRUNC, 0o1000);
    }

    #[test]
    fn test_cloexec() {
        assert_eq!(O_CLOEXEC_OPEN, 0o2000000);
    }

    #[test]
    fn test_path_flag() {
        assert_eq!(O_PATH, 0o10000000);
    }

    #[test]
    fn test_key_flags_nonzero() {
        assert_ne!(O_CREAT, 0);
        assert_ne!(O_EXCL, 0);
        assert_ne!(O_TRUNC, 0);
        assert_ne!(O_APPEND, 0);
        assert_ne!(O_DIRECTORY, 0);
    }
}
