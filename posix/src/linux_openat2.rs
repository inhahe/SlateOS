//! `<linux/openat2.h>` — openat2() syscall constants.
//!
//! openat2() is the extended version of openat() that takes a
//! structured `open_how` argument, allowing new flags and resolve
//! restrictions without running out of flag bits. It provides
//! path resolution controls for security (preventing symlink attacks,
//! escaping directories, etc.).

// ---------------------------------------------------------------------------
// Resolve flags (restrict path resolution)
// ---------------------------------------------------------------------------

/// Don't follow trailing symlink.
pub const RESOLVE_NO_SYMLINKS: u64 = 0x0000_0004;
/// Don't cross mount boundaries.
pub const RESOLVE_NO_XDEV: u64 = 0x0000_0001;
/// Don't traverse upward (no "..").
pub const RESOLVE_BENEATH: u64 = 0x0000_0008;
/// Resolve entirely within starting dir.
pub const RESOLVE_IN_ROOT: u64 = 0x0000_0010;
/// Treat "." as "/" for resolution purposes.
pub const RESOLVE_CACHED: u64 = 0x0000_0020;
/// Don't follow magic links (/proc/self/fd/N).
pub const RESOLVE_NO_MAGICLINKS: u64 = 0x0000_0002;

// ---------------------------------------------------------------------------
// open_how flags (standard O_ flags used with openat2)
// ---------------------------------------------------------------------------

/// Open for reading only.
pub const O_RDONLY: u32 = 0o0000_0000;
/// Open for writing only.
pub const O_WRONLY: u32 = 0o0000_0001;
/// Open for reading and writing.
pub const O_RDWR: u32 = 0o0000_0002;
/// Create file if it doesn't exist.
pub const O_CREAT: u32 = 0o0000_0100;
/// Fail if file exists (with O_CREAT).
pub const O_EXCL: u32 = 0o0000_0200;
/// Don't assign controlling terminal.
pub const O_NOCTTY: u32 = 0o0000_0400;
/// Truncate to zero length.
pub const O_TRUNC: u32 = 0o0000_1000;
/// Append writes to end.
pub const O_APPEND: u32 = 0o0000_2000;
/// Non-blocking mode.
pub const O_NONBLOCK: u32 = 0o0000_4000;
/// Synchronous I/O.
pub const O_SYNC: u32 = 0o0400_0000;
/// No follow symlinks.
pub const O_NOFOLLOW: u32 = 0o0020_0000;
/// Must be a directory.
pub const O_DIRECTORY: u32 = 0o0010_0000;
/// Close-on-exec.
pub const O_CLOEXEC: u32 = 0o0200_0000;
/// Open with O_PATH (no I/O, metadata only).
pub const O_PATH: u32 = 0o1000_0000;
/// Open as tmpfile (unlinked).
pub const O_TMPFILE: u32 = 0o2000_0000;

// ---------------------------------------------------------------------------
// open_how struct version
// ---------------------------------------------------------------------------

/// Current size of struct open_how (for extensibility checking).
pub const OPEN_HOW_SIZE_VER0: u32 = 24;

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
        // O_RDONLY is 0, so just check WRONLY and RDWR are distinct non-zero
        assert_ne!(O_WRONLY, O_RDWR);
        assert_ne!(O_RDONLY, O_WRONLY);
        assert_ne!(O_RDONLY, O_RDWR);
    }

    #[test]
    fn test_open_flags_distinct() {
        let flags = [
            O_CREAT,
            O_EXCL,
            O_NOCTTY,
            O_TRUNC,
            O_APPEND,
            O_NONBLOCK,
            O_NOFOLLOW,
            O_DIRECTORY,
            O_CLOEXEC,
            O_PATH,
            O_TMPFILE,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_open_how_size() {
        assert_eq!(OPEN_HOW_SIZE_VER0, 24);
    }
}
