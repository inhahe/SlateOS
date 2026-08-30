//! `<sys/statvfs.h>` — filesystem statistics.
//!
//! Re-exports the `Statvfs` structure, mount flag constants, and
//! `statvfs`/`fstatvfs` functions from the `statvfs` module.

pub use crate::statvfs::Statvfs;
pub use crate::statvfs::fstatvfs;
pub use crate::statvfs::statvfs;

pub use crate::statvfs::ST_NOSUID;
pub use crate::statvfs::ST_RDONLY;

/// Disallow access to device special files.
pub const ST_NODEV: u64 = 4;

/// Disallow execution.
pub const ST_NOEXEC: u64 = 8;

/// Writes are synced at once.
pub const ST_SYNCHRONOUS: u64 = 16;

/// Allow mandatory locks.
pub const ST_MANDLOCK: u64 = 64;

/// Do not update access times.
pub const ST_NOATIME: u64 = 1024;

/// Do not update directory access times.
pub const ST_NODIRATIME: u64 = 2048;

/// Update atime relative to mtime/ctime.
pub const ST_RELATIME: u64 = 4096;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_statvfs_struct_size() {
        assert!(core::mem::size_of::<Statvfs>() > 0);
    }

    #[test]
    fn test_st_flags_distinct() {
        let flags = [
            ST_RDONLY,
            ST_NOSUID,
            ST_NODEV,
            ST_NOEXEC,
            ST_SYNCHRONOUS,
            ST_MANDLOCK,
            ST_NOATIME,
            ST_NODIRATIME,
            ST_RELATIME,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j], "ST flags must be distinct");
            }
        }
    }

    #[test]
    fn test_st_rdonly_value() {
        assert_eq!(ST_RDONLY, 1);
    }

    #[test]
    fn test_cross_module() {
        assert_eq!(ST_RDONLY, crate::statvfs::ST_RDONLY);
        assert_eq!(ST_NOSUID, crate::statvfs::ST_NOSUID);
    }
}
