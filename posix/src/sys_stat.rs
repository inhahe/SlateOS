//! `<sys/stat.h>` — file status and mode definitions.
//!
//! Re-exports the `Stat` structure, file-type constants, permission
//! constants, and stat/chmod/mkdir functions from `stat` and `file`
//! modules.

// ---------------------------------------------------------------------------
// Structures
// ---------------------------------------------------------------------------

pub use crate::stat::Stat;
pub use crate::stat::Timespec;

// ---------------------------------------------------------------------------
// File-type constants (S_IF*)
// ---------------------------------------------------------------------------

pub use crate::fcntl::S_IFBLK;
pub use crate::fcntl::S_IFCHR;
pub use crate::fcntl::S_IFDIR;
pub use crate::fcntl::S_IFIFO;
pub use crate::fcntl::S_IFLNK;
pub use crate::fcntl::S_IFMT;
pub use crate::fcntl::S_IFREG;
pub use crate::fcntl::S_IFSOCK;

// ---------------------------------------------------------------------------
// Permission bits
// ---------------------------------------------------------------------------

pub use crate::fcntl::S_IRGRP;
pub use crate::fcntl::S_IROTH;
pub use crate::fcntl::S_IRUSR;
pub use crate::fcntl::S_ISGID;
pub use crate::fcntl::S_ISUID;
pub use crate::fcntl::S_ISVTX;
pub use crate::fcntl::S_IWGRP;
pub use crate::fcntl::S_IWOTH;
pub use crate::fcntl::S_IWUSR;
pub use crate::fcntl::S_IXGRP;
pub use crate::fcntl::S_IXOTH;
pub use crate::fcntl::S_IXUSR;

/// Read, write, execute for owner.
pub const S_IRWXU: u32 = S_IRUSR | S_IWUSR | S_IXUSR;

/// Read, write, execute for group.
pub const S_IRWXG: u32 = S_IRGRP | S_IWGRP | S_IXGRP;

/// Read, write, execute for others.
pub const S_IRWXO: u32 = S_IROTH | S_IWOTH | S_IXOTH;

// ---------------------------------------------------------------------------
// stat functions
// ---------------------------------------------------------------------------

pub use crate::file::chmod;
pub use crate::file::fchmod;
pub use crate::file::fstat;
pub use crate::file::fstatat;
pub use crate::file::lstat;
pub use crate::file::mkdir;
pub use crate::file::stat;
pub use crate::file::umask;

// ---------------------------------------------------------------------------
// mknod / mkfifo
// ---------------------------------------------------------------------------

pub use crate::stat::mkfifo;
pub use crate::stat::mkfifoat;
pub use crate::stat::mknod;
pub use crate::stat::mknodat;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stat_struct_size() {
        assert!(core::mem::size_of::<Stat>() > 0);
    }

    #[test]
    fn test_file_type_constants() {
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

    #[test]
    fn test_rwx_combined() {
        assert_eq!(S_IRWXU, 0o700);
        assert_eq!(S_IRWXG, 0o070);
        assert_eq!(S_IRWXO, 0o007);
    }

    #[test]
    fn test_special_bits() {
        assert_eq!(S_ISUID, 0o4000);
        assert_eq!(S_ISGID, 0o2000);
        assert_eq!(S_ISVTX, 0o1000);
    }

    #[test]
    fn test_file_type_distinct() {
        let types = [
            S_IFDIR, S_IFCHR, S_IFBLK, S_IFREG, S_IFIFO, S_IFLNK, S_IFSOCK,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_umask_returns_previous() {
        // umask should return the previous mask.
        let old = umask(0o022);
        let _restore = umask(old);
    }

    #[test]
    fn test_mknod_stub() {
        let ret = mknod(b"/nonexistent\0".as_ptr(), S_IFREG | 0o644, 0);
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_mkfifo_stub() {
        let ret = mkfifo(b"/nonexistent\0".as_ptr(), 0o644);
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_cross_module() {
        assert_eq!(S_IFREG, crate::fcntl::S_IFREG);
        assert_eq!(S_IFDIR, crate::fcntl::S_IFDIR);
        assert_eq!(S_IRUSR, crate::fcntl::S_IRUSR);
    }
}
