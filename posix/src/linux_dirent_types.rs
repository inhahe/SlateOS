//! `<dirent.h>` — Directory entry constants.
//!
//! `readdir()`, `opendir()`, `closedir()`, and `scandir()`
//! iterate over directory entries.  These constants define
//! entry types, field offsets, and maximum name lengths.

// ---------------------------------------------------------------------------
// Directory entry types (d_type field)
// ---------------------------------------------------------------------------

/// Unknown type.
pub const DT_UNKNOWN: u8 = 0;
/// Named pipe (FIFO).
pub const DT_FIFO: u8 = 1;
/// Character device.
pub const DT_CHR: u8 = 2;
/// Directory.
pub const DT_DIR: u8 = 4;
/// Block device.
pub const DT_BLK: u8 = 6;
/// Regular file.
pub const DT_REG: u8 = 8;
/// Symbolic link.
pub const DT_LNK: u8 = 10;
/// Unix domain socket.
pub const DT_SOCK: u8 = 12;
/// Door (Solaris, not on Linux but reserved).
pub const DT_WHT: u8 = 14;

// ---------------------------------------------------------------------------
// struct dirent64 field offsets (Linux x86_64)
// ---------------------------------------------------------------------------

/// Offset of d_ino (inode number) in struct dirent64.
pub const DIRENT64_OFF_INO: u32 = 0;
/// Offset of d_off (offset to next entry) in struct dirent64.
pub const DIRENT64_OFF_OFF: u32 = 8;
/// Offset of d_reclen (record length) in struct dirent64.
pub const DIRENT64_OFF_RECLEN: u32 = 16;
/// Offset of d_type (file type) in struct dirent64.
pub const DIRENT64_OFF_TYPE: u32 = 18;
/// Offset of d_name (filename) in struct dirent64.
pub const DIRENT64_OFF_NAME: u32 = 19;

// ---------------------------------------------------------------------------
// Name length limits
// ---------------------------------------------------------------------------

/// Maximum filename length (not including NUL).
pub const NAME_MAX: u32 = 255;
/// Maximum filename length in a dirent (including NUL terminator).
pub const DIRENT_NAME_LEN: u32 = 256;

// ---------------------------------------------------------------------------
// scandir() filter/sort helpers
// ---------------------------------------------------------------------------

/// DT_* to mode_t conversion shift (DT_* << 12 = S_IF*).
pub const DT_TO_MODE_SHIFT: u32 = 12;

// ---------------------------------------------------------------------------
// getdents64 buffer size recommendations
// ---------------------------------------------------------------------------

/// Recommended buffer size for getdents64 (bytes).
pub const GETDENTS_BUF_SIZE: u32 = 32768;
/// Minimum buffer size for getdents64 (bytes).
pub const GETDENTS_BUF_MIN: u32 = 264;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_types_distinct() {
        let types = [
            DT_UNKNOWN, DT_FIFO, DT_CHR, DT_DIR,
            DT_BLK, DT_REG, DT_LNK, DT_SOCK, DT_WHT,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_unknown_is_zero() {
        assert_eq!(DT_UNKNOWN, 0);
    }

    #[test]
    fn test_reg_is_eight() {
        assert_eq!(DT_REG, 8);
    }

    #[test]
    fn test_dir_is_four() {
        assert_eq!(DT_DIR, 4);
    }

    #[test]
    fn test_offsets_ascending() {
        let offsets = [
            DIRENT64_OFF_INO, DIRENT64_OFF_OFF,
            DIRENT64_OFF_RECLEN, DIRENT64_OFF_TYPE,
            DIRENT64_OFF_NAME,
        ];
        for i in 1..offsets.len() {
            assert!(offsets[i] > offsets[i - 1]);
        }
    }

    #[test]
    fn test_name_max() {
        assert_eq!(NAME_MAX, 255);
    }

    #[test]
    fn test_dirent_name_len() {
        assert_eq!(DIRENT_NAME_LEN, NAME_MAX + 1);
    }

    #[test]
    fn test_dt_to_mode_shift() {
        assert_eq!(DT_TO_MODE_SHIFT, 12);
    }

    #[test]
    fn test_getdents_buf_size() {
        assert!(GETDENTS_BUF_SIZE > GETDENTS_BUF_MIN);
    }
}
