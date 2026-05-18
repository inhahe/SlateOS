//! `<errno.h>` — POSIX error number constants (1-34).
//!
//! These are the standard error codes returned by Linux syscalls
//! and C library functions. The kernel returns negative values;
//! libc converts them to positive errno values. This module covers
//! the first 34 "classic" POSIX error numbers.

// ---------------------------------------------------------------------------
// Standard POSIX errno values (1-34)
// ---------------------------------------------------------------------------

/// Operation not permitted.
pub const EPERM: u32 = 1;
/// No such file or directory.
pub const ENOENT: u32 = 2;
/// No such process.
pub const ESRCH: u32 = 3;
/// Interrupted system call.
pub const EINTR: u32 = 4;
/// I/O error.
pub const EIO: u32 = 5;
/// No such device or address.
pub const ENXIO: u32 = 6;
/// Argument list too long.
pub const E2BIG: u32 = 7;
/// Exec format error.
pub const ENOEXEC: u32 = 8;
/// Bad file descriptor.
pub const EBADF: u32 = 9;
/// No child processes.
pub const ECHILD: u32 = 10;
/// Try again (resource temporarily unavailable).
pub const EAGAIN: u32 = 11;
/// Out of memory.
pub const ENOMEM: u32 = 12;
/// Permission denied.
pub const EACCES: u32 = 13;
/// Bad address.
pub const EFAULT: u32 = 14;
/// Block device required.
pub const ENOTBLK: u32 = 15;
/// Device or resource busy.
pub const EBUSY: u32 = 16;
/// File exists.
pub const EEXIST: u32 = 17;
/// Cross-device link.
pub const EXDEV: u32 = 18;
/// No such device.
pub const ENODEV: u32 = 19;
/// Not a directory.
pub const ENOTDIR: u32 = 20;
/// Is a directory.
pub const EISDIR: u32 = 21;
/// Invalid argument.
pub const EINVAL: u32 = 22;
/// File table overflow.
pub const ENFILE: u32 = 23;
/// Too many open files.
pub const EMFILE: u32 = 24;
/// Not a typewriter (inappropriate ioctl).
pub const ENOTTY: u32 = 25;
/// Text file busy.
pub const ETXTBSY: u32 = 26;
/// File too large.
pub const EFBIG: u32 = 27;
/// No space left on device.
pub const ENOSPC: u32 = 28;
/// Illegal seek.
pub const ESPIPE: u32 = 29;
/// Read-only file system.
pub const EROFS: u32 = 30;
/// Too many links.
pub const EMLINK: u32 = 31;
/// Broken pipe.
pub const EPIPE: u32 = 32;
/// Math argument out of domain.
pub const EDOM: u32 = 33;
/// Math result not representable.
pub const ERANGE: u32 = 34;

// ---------------------------------------------------------------------------
// Alias
// ---------------------------------------------------------------------------

/// EWOULDBLOCK is the same as EAGAIN on Linux.
pub const EWOULDBLOCK: u32 = EAGAIN;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_errno_values_distinct() {
        let errs = [
            EPERM, ENOENT, ESRCH, EINTR, EIO, ENXIO, E2BIG,
            ENOEXEC, EBADF, ECHILD, EAGAIN, ENOMEM, EACCES,
            EFAULT, ENOTBLK, EBUSY, EEXIST, EXDEV, ENODEV,
            ENOTDIR, EISDIR, EINVAL, ENFILE, EMFILE, ENOTTY,
            ETXTBSY, EFBIG, ENOSPC, ESPIPE, EROFS, EMLINK,
            EPIPE, EDOM, ERANGE,
        ];
        for i in 0..errs.len() {
            for j in (i + 1)..errs.len() {
                assert_ne!(errs[i], errs[j]);
            }
        }
    }

    #[test]
    fn test_eperm_is_one() {
        assert_eq!(EPERM, 1);
    }

    #[test]
    fn test_common_values() {
        assert_eq!(ENOENT, 2);
        assert_eq!(EINVAL, 22);
        assert_eq!(ENOMEM, 12);
        assert_eq!(EACCES, 13);
    }

    #[test]
    fn test_ewouldblock_alias() {
        assert_eq!(EWOULDBLOCK, EAGAIN);
    }

    #[test]
    fn test_sequential() {
        // First 34 errnos are sequential 1..=34
        assert_eq!(ERANGE, 34);
    }
}
