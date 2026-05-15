//! POSIX file control flags and constants.
//!
//! Flags for `open()`, `fcntl()`, and file descriptor operations.
//! Values match Linux x86_64 for binary compatibility.

// ---------------------------------------------------------------------------
// open() flags
// ---------------------------------------------------------------------------

/// Open for reading only.
pub const O_RDONLY: i32 = 0;
/// Open for writing only.
pub const O_WRONLY: i32 = 1;
/// Open for reading and writing.
pub const O_RDWR: i32 = 2;
/// Access mode mask (low 2 bits).
pub const O_ACCMODE: i32 = 3;

/// Create file if it does not exist.
pub const O_CREAT: i32 = 0o100;
/// Error if O_CREAT and file already exists.
pub const O_EXCL: i32 = 0o200;
/// No controlling terminal.
pub const O_NOCTTY: i32 = 0o400;
/// Truncate file to zero length.
pub const O_TRUNC: i32 = 0o1000;
/// Set append mode.
pub const O_APPEND: i32 = 0o2000;
/// Non-blocking mode.
pub const O_NONBLOCK: i32 = 0o4000;
/// Synchronous writes.
pub const O_SYNC: i32 = 0o4_010_000;
/// Don't follow symlinks (on final component).
pub const O_NOFOLLOW: i32 = 0o400_000;
/// Close on exec.
pub const O_CLOEXEC: i32 = 0o2_000_000;
/// Open directory only.
pub const O_DIRECTORY: i32 = 0o200_000;
/// Don't update access time.
pub const O_NOATIME: i32 = 0o1_000_000;
/// Resolve pathname, don't open (Linux 2.6.39+).
pub const O_PATH: i32 = 0o10_000_000;
/// Create unnamed temporary file (Linux 3.11+).
pub const O_TMPFILE: i32 = 0o20_200_000;
/// Data integrity sync (write data + necessary metadata).
pub const O_DSYNC: i32 = 0o10_000;

// ---------------------------------------------------------------------------
// lseek() whence
// ---------------------------------------------------------------------------

/// Seek relative to beginning of file.
pub const SEEK_SET: i32 = 0;
/// Seek relative to current position.
pub const SEEK_CUR: i32 = 1;
/// Seek relative to end of file.
pub const SEEK_END: i32 = 2;
/// Seek to next data (Linux 3.1+, `lseek` extension).
pub const SEEK_DATA: i32 = 3;
/// Seek to next hole (Linux 3.1+, `lseek` extension).
pub const SEEK_HOLE: i32 = 4;

// ---------------------------------------------------------------------------
// access() mode flags
// ---------------------------------------------------------------------------

/// Test for existence.
pub const F_OK: i32 = 0;
/// Test for execute/search permission.
pub const X_OK: i32 = 1;
/// Test for write permission.
pub const W_OK: i32 = 2;
/// Test for read permission.
pub const R_OK: i32 = 4;

// ---------------------------------------------------------------------------
// File type mode bits (for stat)
// ---------------------------------------------------------------------------

/// Bit mask for file type.
pub const S_IFMT: u32 = 0o170_000;
/// Directory.
pub const S_IFDIR: u32 = 0o040_000;
/// Character device.
pub const S_IFCHR: u32 = 0o020_000;
/// Block device.
pub const S_IFBLK: u32 = 0o060_000;
/// Regular file.
pub const S_IFREG: u32 = 0o100_000;
/// FIFO (named pipe).
pub const S_IFIFO: u32 = 0o010_000;
/// Symbolic link.
pub const S_IFLNK: u32 = 0o120_000;
/// Socket.
pub const S_IFSOCK: u32 = 0o140_000;

/// Set user ID on execution.
pub const S_ISUID: u32 = 0o4000;
/// Set group ID on execution.
pub const S_ISGID: u32 = 0o2000;
/// Sticky bit.
pub const S_ISVTX: u32 = 0o1000;

/// Owner read.
pub const S_IRUSR: u32 = 0o400;
/// Owner write.
pub const S_IWUSR: u32 = 0o200;
/// Owner execute.
pub const S_IXUSR: u32 = 0o100;
/// Group read.
pub const S_IRGRP: u32 = 0o040;
/// Group write.
pub const S_IWGRP: u32 = 0o020;
/// Group execute.
pub const S_IXGRP: u32 = 0o010;
/// Others read.
pub const S_IROTH: u32 = 0o004;
/// Others write.
pub const S_IWOTH: u32 = 0o002;
/// Others execute.
pub const S_IXOTH: u32 = 0o001;

/// Default file permissions (owner rw, group/other r).
pub const DEFAULT_FILE_MODE: u32 = S_IRUSR | S_IWUSR | S_IRGRP | S_IROTH;
/// Default directory permissions (owner rwx, group/other rx).
pub const DEFAULT_DIR_MODE: u32 = S_IRUSR | S_IWUSR | S_IXUSR | S_IRGRP | S_IXGRP | S_IROTH | S_IXOTH;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- open() flags match Linux x86_64 --

    #[test]
    fn test_open_flags() {
        assert_eq!(O_RDONLY, 0);
        assert_eq!(O_WRONLY, 1);
        assert_eq!(O_RDWR, 2);
        assert_eq!(O_ACCMODE, 3);
        assert_eq!(O_CREAT, 64);       // 0o100
        assert_eq!(O_EXCL, 128);       // 0o200
        assert_eq!(O_NOCTTY, 256);     // 0o400
        assert_eq!(O_TRUNC, 512);      // 0o1000
        assert_eq!(O_APPEND, 1024);    // 0o2000
        assert_eq!(O_NONBLOCK, 2048);  // 0o4000
        assert_eq!(O_CLOEXEC, 524288); // 0o2_000_000
    }

    #[test]
    fn test_o_sync_value() {
        // O_SYNC on Linux x86_64 = 0o4_010_000 = 0x101000 = 1052672
        assert_eq!(O_SYNC, 0o4_010_000);
        assert_eq!(O_SYNC, 1_052_672);
    }

    #[test]
    fn test_o_directory_nofollow() {
        assert_eq!(O_DIRECTORY, 0o200_000);
        assert_eq!(O_NOFOLLOW, 0o400_000);
        assert_eq!(O_DIRECTORY, 65536);
        assert_eq!(O_NOFOLLOW, 131072);
    }

    #[test]
    fn test_extended_open_flags() {
        assert_eq!(O_NOATIME, 0o1_000_000);
        assert_eq!(O_PATH, 0o10_000_000);
        assert_eq!(O_TMPFILE, 0o20_200_000);
        assert_eq!(O_DSYNC, 0o10_000);
    }

    #[test]
    fn test_open_flags_no_collisions() {
        // Each flag should be a distinct bit or combination.
        // O_TMPFILE includes O_DIRECTORY as a sub-flag (Linux design).
        assert_ne!(O_NOATIME, O_PATH);
        assert_ne!(O_NOATIME, O_DSYNC);
        assert_ne!(O_PATH, O_DSYNC);
    }

    #[test]
    fn test_accmode_mask() {
        // O_ACCMODE should extract access mode from flags
        assert_eq!(O_RDONLY & O_ACCMODE, O_RDONLY);
        assert_eq!(O_WRONLY & O_ACCMODE, O_WRONLY);
        assert_eq!(O_RDWR & O_ACCMODE, O_RDWR);
        // Flags with access mode should be extractable
        assert_eq!((O_RDWR | O_CREAT | O_TRUNC) & O_ACCMODE, O_RDWR);
    }

    // -- lseek whence --

    #[test]
    fn test_seek_constants() {
        assert_eq!(SEEK_SET, 0);
        assert_eq!(SEEK_CUR, 1);
        assert_eq!(SEEK_END, 2);
        assert_eq!(SEEK_DATA, 3);
        assert_eq!(SEEK_HOLE, 4);
    }

    #[test]
    fn test_seek_constants_distinct() {
        let vals = [SEEK_SET, SEEK_CUR, SEEK_END, SEEK_DATA, SEEK_HOLE];
        for i in 0..vals.len() {
            for j in (i + 1)..vals.len() {
                assert_ne!(vals[i], vals[j], "seek constants must be distinct");
            }
        }
    }

    // -- access() mode flags --

    #[test]
    fn test_access_flags() {
        assert_eq!(F_OK, 0);
        assert_eq!(X_OK, 1);
        assert_eq!(W_OK, 2);
        assert_eq!(R_OK, 4);
    }

    #[test]
    fn test_access_flags_combinable() {
        // R_OK | W_OK | X_OK should be 7 (read+write+execute)
        assert_eq!(R_OK | W_OK | X_OK, 7);
    }

    // -- File type mode bits (S_IF*) match Linux --

    #[test]
    fn test_file_type_bits() {
        assert_eq!(S_IFMT, 0o170_000);
        assert_eq!(S_IFDIR, 0o040_000);
        assert_eq!(S_IFCHR, 0o020_000);
        assert_eq!(S_IFBLK, 0o060_000);
        assert_eq!(S_IFREG, 0o100_000);
        assert_eq!(S_IFIFO, 0o010_000);
        assert_eq!(S_IFLNK, 0o120_000);
        assert_eq!(S_IFSOCK, 0o140_000);
    }

    #[test]
    fn test_file_types_extracted_by_mask() {
        // S_IFMT should correctly extract file type
        assert_eq!(S_IFREG & S_IFMT, S_IFREG);
        assert_eq!(S_IFDIR & S_IFMT, S_IFDIR);
        assert_eq!(S_IFLNK & S_IFMT, S_IFLNK);
        // Permissions should be masked out
        assert_eq!((S_IFREG | 0o644) & S_IFMT, S_IFREG);
    }

    // -- Permission bits --

    #[test]
    fn test_permission_bits() {
        assert_eq!(S_IRUSR, 0o400);
        assert_eq!(S_IWUSR, 0o200);
        assert_eq!(S_IXUSR, 0o100);
        assert_eq!(S_IRGRP, 0o040);
        assert_eq!(S_IWGRP, 0o020);
        assert_eq!(S_IXGRP, 0o010);
        assert_eq!(S_IROTH, 0o004);
        assert_eq!(S_IWOTH, 0o002);
        assert_eq!(S_IXOTH, 0o001);
    }

    // -- Special bits --

    #[test]
    fn test_special_bits() {
        assert_eq!(S_ISUID, 0o4000);
        assert_eq!(S_ISGID, 0o2000);
        assert_eq!(S_ISVTX, 0o1000);
    }

    // -- Derived modes --

    #[test]
    fn test_default_file_mode() {
        // 0644 = owner rw, group r, others r
        assert_eq!(DEFAULT_FILE_MODE, 0o644);
    }

    #[test]
    fn test_default_dir_mode() {
        // 0755 = owner rwx, group rx, others rx
        assert_eq!(DEFAULT_DIR_MODE, 0o755);
    }

    // -- No overlap between file types --

    #[test]
    fn test_file_types_distinct() {
        let types = [S_IFREG, S_IFDIR, S_IFLNK, S_IFCHR, S_IFBLK, S_IFIFO, S_IFSOCK];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j], "file types must not overlap");
            }
        }
    }

    // -- Permission bits are disjoint --

    #[test]
    fn test_permission_bits_disjoint() {
        let perms = [
            S_IRUSR, S_IWUSR, S_IXUSR,
            S_IRGRP, S_IWGRP, S_IXGRP,
            S_IROTH, S_IWOTH, S_IXOTH,
        ];
        // All bits should be unique (no overlap)
        let mut combined: u32 = 0;
        for p in perms {
            assert_eq!(combined & p, 0, "permission bit 0o{p:o} overlaps");
            combined |= p;
        }
        // Full permission = 0o777
        assert_eq!(combined, 0o777);
    }
}
