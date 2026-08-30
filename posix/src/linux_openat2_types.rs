//! `<linux/openat2.h>` — openat2() resolve and how flags.
//!
//! The openat2() system call extends open/openat with structured
//! flags controlling path resolution behavior. The `resolve` field
//! prevents certain symlink and mount traversals, enabling safe
//! path operations inside untrusted directory hierarchies.

// ---------------------------------------------------------------------------
// Resolve flags (open_how.resolve)
// ---------------------------------------------------------------------------

/// Block resolving through absolute symlinks.
pub const RESOLVE_NO_XDEV: u64 = 0x01;
/// Block all magic-link (e.g., /proc/self/fd/N) resolution.
pub const RESOLVE_NO_MAGICLINKS: u64 = 0x02;
/// Block following symlinks entirely.
pub const RESOLVE_NO_SYMLINKS: u64 = 0x04;
/// Path must stay beneath the starting directory.
pub const RESOLVE_BENEATH: u64 = 0x08;
/// Treat the starting directory as the filesystem root.
pub const RESOLVE_IN_ROOT: u64 = 0x10;
/// Only create cached lookups (no revalidation).
pub const RESOLVE_CACHED: u64 = 0x20;

// ---------------------------------------------------------------------------
// open_how.flags (same as open() O_* flags, commonly used subset)
// ---------------------------------------------------------------------------

/// Open read-only.
pub const OPENAT2_O_RDONLY: u64 = 0;
/// Open write-only.
pub const OPENAT2_O_WRONLY: u64 = 1;
/// Open read-write.
pub const OPENAT2_O_RDWR: u64 = 2;
/// Create file if it doesn't exist.
pub const OPENAT2_O_CREAT: u64 = 0o100;
/// Fail if file exists (with O_CREAT).
pub const OPENAT2_O_EXCL: u64 = 0o200;
/// Truncate file to zero length.
pub const OPENAT2_O_TRUNC: u64 = 0o1000;
/// Append mode.
pub const OPENAT2_O_APPEND: u64 = 0o2000;
/// Non-blocking I/O.
pub const OPENAT2_O_NONBLOCK: u64 = 0o4000;
/// Close-on-exec.
pub const OPENAT2_O_CLOEXEC: u64 = 0o2000000;
/// Open path-only FD (no I/O allowed).
pub const OPENAT2_O_PATH: u64 = 0o10000000;
/// Open directory (fail on non-directory).
pub const OPENAT2_O_DIRECTORY: u64 = 0o200000;
/// Do not follow final symlink.
pub const OPENAT2_O_NOFOLLOW: u64 = 0o400000;

// ---------------------------------------------------------------------------
// open_how structure size
// ---------------------------------------------------------------------------

/// Size of struct open_how (for the size parameter of openat2).
pub const OPEN_HOW_SIZE_VER0: u32 = 24;
/// Latest size (currently same as VER0, may grow).
pub const OPEN_HOW_SIZE_LATEST: u32 = 24;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_flags_no_overlap() {
        let flags = [
            RESOLVE_NO_XDEV,
            RESOLVE_NO_MAGICLINKS,
            RESOLVE_NO_SYMLINKS,
            RESOLVE_BENEATH,
            RESOLVE_IN_ROOT,
            RESOLVE_CACHED,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_access_modes_distinct() {
        assert_ne!(OPENAT2_O_RDONLY, OPENAT2_O_WRONLY);
        assert_ne!(OPENAT2_O_WRONLY, OPENAT2_O_RDWR);
    }

    #[test]
    fn test_flags_distinct() {
        let flags = [
            OPENAT2_O_CREAT,
            OPENAT2_O_EXCL,
            OPENAT2_O_TRUNC,
            OPENAT2_O_APPEND,
            OPENAT2_O_NONBLOCK,
            OPENAT2_O_CLOEXEC,
            OPENAT2_O_PATH,
            OPENAT2_O_DIRECTORY,
            OPENAT2_O_NOFOLLOW,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_struct_size() {
        assert_eq!(OPEN_HOW_SIZE_VER0, 24);
        assert_eq!(OPEN_HOW_SIZE_LATEST, OPEN_HOW_SIZE_VER0);
    }
}
