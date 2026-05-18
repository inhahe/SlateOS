//! `<unistd.h>` — lseek/llseek whence and related constants.
//!
//! These constants define the `whence` argument for `lseek()` and
//! `llseek()`, as well as Linux-specific extensions for seeking
//! to data or hole regions in sparse files.

// ---------------------------------------------------------------------------
// Standard whence values
// ---------------------------------------------------------------------------

/// Seek from beginning of file.
pub const SEEK_SET: u32 = 0;
/// Seek from current position.
pub const SEEK_CUR: u32 = 1;
/// Seek from end of file.
pub const SEEK_END: u32 = 2;

// ---------------------------------------------------------------------------
// Linux-specific whence values (sparse file support)
// ---------------------------------------------------------------------------

/// Seek to next data region (past hole).
pub const SEEK_DATA: u32 = 3;
/// Seek to next hole region (past data).
pub const SEEK_HOLE: u32 = 4;

// ---------------------------------------------------------------------------
// Maximum seek offset
// ---------------------------------------------------------------------------

/// Maximum valid offset for lseek (2^63 - 1).
pub const LSEEK_MAX_OFFSET: i64 = i64::MAX;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_whence_distinct() {
        let whence = [SEEK_SET, SEEK_CUR, SEEK_END, SEEK_DATA, SEEK_HOLE];
        for i in 0..whence.len() {
            for j in (i + 1)..whence.len() {
                assert_ne!(whence[i], whence[j]);
            }
        }
    }

    #[test]
    fn test_seek_set_is_zero() {
        assert_eq!(SEEK_SET, 0);
    }

    #[test]
    fn test_seek_data_hole() {
        assert_eq!(SEEK_DATA, 3);
        assert_eq!(SEEK_HOLE, 4);
    }

    #[test]
    fn test_max_offset() {
        assert_eq!(LSEEK_MAX_OFFSET, i64::MAX);
    }
}
