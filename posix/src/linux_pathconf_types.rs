//! `<unistd.h>` — pathconf()/fpathconf() name constants.
//!
//! `pathconf()` and `fpathconf()` query filesystem-specific
//! configuration limits at runtime.  These constants are the
//! `name` parameter identifying which value to query.

// ---------------------------------------------------------------------------
// POSIX pathconf names (_PC_*)
// ---------------------------------------------------------------------------

/// Maximum number of links to a file.
pub const PC_LINK_MAX: u32 = 0;
/// Maximum length of a canonical pathname component.
pub const PC_MAX_CANON: u32 = 1;
/// Maximum length of a raw pathname component.
pub const PC_MAX_INPUT: u32 = 2;
/// Maximum filename length in the directory.
pub const PC_NAME_MAX: u32 = 3;
/// Maximum relative pathname length.
pub const PC_PATH_MAX: u32 = 4;
/// Size of the pipe buffer.
pub const PC_PIPE_BUF: u32 = 5;
/// Terminal special characters are disabled.
pub const PC_CHOWN_RESTRICTED: u32 = 6;
/// Filenames are not truncated beyond NAME_MAX.
pub const PC_NO_TRUNC: u32 = 7;
/// Terminal special character processing.
pub const PC_VDISABLE: u32 = 8;
/// Filesystem supports synchronized I/O.
pub const PC_SYNC_IO: u32 = 9;
/// Filesystem supports asynchronous I/O.
pub const PC_ASYNC_IO: u32 = 10;
/// Filesystem supports prioritized I/O.
pub const PC_PRIO_IO: u32 = 11;
/// Maximum socket buffer size.
pub const PC_SOCK_MAXBUF: u32 = 12;
/// Filesystem supports file locking.
pub const PC_FILESIZEBITS: u32 = 13;
/// Recommended I/O block size.
pub const PC_REC_INCR_XFER_SIZE: u32 = 14;
/// Maximum recommended I/O transfer size.
pub const PC_REC_MAX_XFER_SIZE: u32 = 15;
/// Minimum recommended I/O transfer size.
pub const PC_REC_MIN_XFER_SIZE: u32 = 16;
/// Recommended transfer alignment.
pub const PC_REC_XFER_ALIGN: u32 = 17;
/// Allocation size granularity.
pub const PC_ALLOC_SIZE_MIN: u32 = 18;
/// Maximum number of symbolic links in path traversal.
pub const PC_SYMLINK_MAX: u32 = 19;
/// Shell interprets 8-bit characters.
pub const PC_2_SYMLINKS: u32 = 20;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_names_distinct() {
        let names = [
            PC_LINK_MAX,
            PC_MAX_CANON,
            PC_MAX_INPUT,
            PC_NAME_MAX,
            PC_PATH_MAX,
            PC_PIPE_BUF,
            PC_CHOWN_RESTRICTED,
            PC_NO_TRUNC,
            PC_VDISABLE,
            PC_SYNC_IO,
            PC_ASYNC_IO,
            PC_PRIO_IO,
            PC_SOCK_MAXBUF,
            PC_FILESIZEBITS,
            PC_REC_INCR_XFER_SIZE,
            PC_REC_MAX_XFER_SIZE,
            PC_REC_MIN_XFER_SIZE,
            PC_REC_XFER_ALIGN,
            PC_ALLOC_SIZE_MIN,
            PC_SYMLINK_MAX,
            PC_2_SYMLINKS,
        ];
        for i in 0..names.len() {
            for j in (i + 1)..names.len() {
                assert_ne!(names[i], names[j]);
            }
        }
    }

    #[test]
    fn test_link_max_is_zero() {
        assert_eq!(PC_LINK_MAX, 0);
    }

    #[test]
    fn test_name_max() {
        assert_eq!(PC_NAME_MAX, 3);
    }

    #[test]
    fn test_path_max() {
        assert_eq!(PC_PATH_MAX, 4);
    }

    #[test]
    fn test_pipe_buf() {
        assert_eq!(PC_PIPE_BUF, 5);
    }

    #[test]
    fn test_sequential_range() {
        // First 9 values are sequential 0..=8
        assert_eq!(PC_LINK_MAX, 0);
        assert_eq!(PC_VDISABLE, 8);
    }
}
