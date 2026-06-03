//! `<linux/nfs.h>` — NFS (Network File System) constants.
//!
//! NFS is a distributed filesystem protocol allowing remote file
//! access over a network. NFSv4 added stateful operations, compound
//! RPCs, and strong security via RPCSEC_GSS.

// ---------------------------------------------------------------------------
// NFS versions
// ---------------------------------------------------------------------------

/// NFSv2.
pub const NFS_VERSION_2: u32 = 2;
/// NFSv3.
pub const NFS_VERSION_3: u32 = 3;
/// NFSv4.0.
pub const NFS_VERSION_4_0: u32 = 4;
/// NFSv4.1 (sessions, pNFS).
pub const NFS_VERSION_4_1: u32 = 41;
/// NFSv4.2 (server-side copy, space reservations).
pub const NFS_VERSION_4_2: u32 = 42;

// ---------------------------------------------------------------------------
// NFSv3 procedure numbers
// ---------------------------------------------------------------------------

/// Null (ping).
pub const NFS3_PROC_NULL: u32 = 0;
/// Get file attributes.
pub const NFS3_PROC_GETATTR: u32 = 1;
/// Set file attributes.
pub const NFS3_PROC_SETATTR: u32 = 2;
/// Lookup filename.
pub const NFS3_PROC_LOOKUP: u32 = 3;
/// Check access permissions.
pub const NFS3_PROC_ACCESS: u32 = 4;
/// Read symbolic link.
pub const NFS3_PROC_READLINK: u32 = 5;
/// Read from file.
pub const NFS3_PROC_READ: u32 = 6;
/// Write to file.
pub const NFS3_PROC_WRITE: u32 = 7;
/// Create file.
pub const NFS3_PROC_CREATE: u32 = 8;
/// Make directory.
pub const NFS3_PROC_MKDIR: u32 = 9;
/// Create symbolic link.
pub const NFS3_PROC_SYMLINK: u32 = 10;
/// Make device node.
pub const NFS3_PROC_MKNOD: u32 = 11;
/// Remove file.
pub const NFS3_PROC_REMOVE: u32 = 12;
/// Remove directory.
pub const NFS3_PROC_RMDIR: u32 = 13;
/// Rename.
pub const NFS3_PROC_RENAME: u32 = 14;
/// Create hard link.
pub const NFS3_PROC_LINK: u32 = 15;
/// Read directory.
pub const NFS3_PROC_READDIR: u32 = 16;
/// Read directory plus attributes.
pub const NFS3_PROC_READDIRPLUS: u32 = 17;
/// Get filesystem stats.
pub const NFS3_PROC_FSSTAT: u32 = 18;
/// Get filesystem info.
pub const NFS3_PROC_FSINFO: u32 = 19;
/// Get POSIX pathconf info.
pub const NFS3_PROC_PATHCONF: u32 = 20;
/// Commit writes to stable storage.
pub const NFS3_PROC_COMMIT: u32 = 21;

// ---------------------------------------------------------------------------
// NFS file types
// ---------------------------------------------------------------------------

/// Regular file.
pub const NFS_FTYPE_REG: u32 = 1;
/// Directory.
pub const NFS_FTYPE_DIR: u32 = 2;
/// Block device.
pub const NFS_FTYPE_BLK: u32 = 3;
/// Character device.
pub const NFS_FTYPE_CHR: u32 = 4;
/// Symbolic link.
pub const NFS_FTYPE_LNK: u32 = 5;
/// Socket.
pub const NFS_FTYPE_SOCK: u32 = 6;
/// Named pipe (FIFO).
pub const NFS_FTYPE_FIFO: u32 = 7;

// ---------------------------------------------------------------------------
// NFS status codes (NFSv3)
// ---------------------------------------------------------------------------

/// Success.
pub const NFS3_OK: u32 = 0;
/// Not owner (permission denied).
pub const NFS3ERR_PERM: u32 = 1;
/// No such file.
pub const NFS3ERR_NOENT: u32 = 2;
/// I/O error.
pub const NFS3ERR_IO: u32 = 5;
/// No such device.
pub const NFS3ERR_NXIO: u32 = 6;
/// Permission denied.
pub const NFS3ERR_ACCES: u32 = 13;
/// File exists.
pub const NFS3ERR_EXIST: u32 = 17;
/// Not a directory.
pub const NFS3ERR_NOTDIR: u32 = 20;
/// Is a directory.
pub const NFS3ERR_ISDIR: u32 = 21;
/// No space left.
pub const NFS3ERR_NOSPC: u32 = 28;
/// Read-only filesystem.
pub const NFS3ERR_ROFS: u32 = 30;
/// Name too long.
pub const NFS3ERR_NAMETOOLONG: u32 = 63;
/// Directory not empty.
pub const NFS3ERR_NOTEMPTY: u32 = 66;
/// Stale file handle.
pub const NFS3ERR_STALE: u32 = 70;

// ---------------------------------------------------------------------------
// NFS default port
// ---------------------------------------------------------------------------

/// NFS default port (NFSv3).
pub const NFS_DEFAULT_PORT: u16 = 2049;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_versions_distinct() {
        let vers = [
            NFS_VERSION_2,
            NFS_VERSION_3,
            NFS_VERSION_4_0,
            NFS_VERSION_4_1,
            NFS_VERSION_4_2,
        ];
        for i in 0..vers.len() {
            for j in (i + 1)..vers.len() {
                assert_ne!(vers[i], vers[j]);
            }
        }
    }

    #[test]
    fn test_nfs3_procs_distinct() {
        let procs = [
            NFS3_PROC_NULL,
            NFS3_PROC_GETATTR,
            NFS3_PROC_SETATTR,
            NFS3_PROC_LOOKUP,
            NFS3_PROC_ACCESS,
            NFS3_PROC_READLINK,
            NFS3_PROC_READ,
            NFS3_PROC_WRITE,
            NFS3_PROC_CREATE,
            NFS3_PROC_MKDIR,
            NFS3_PROC_SYMLINK,
            NFS3_PROC_MKNOD,
            NFS3_PROC_REMOVE,
            NFS3_PROC_RMDIR,
            NFS3_PROC_RENAME,
            NFS3_PROC_LINK,
            NFS3_PROC_READDIR,
            NFS3_PROC_READDIRPLUS,
            NFS3_PROC_FSSTAT,
            NFS3_PROC_FSINFO,
            NFS3_PROC_PATHCONF,
            NFS3_PROC_COMMIT,
        ];
        for i in 0..procs.len() {
            for j in (i + 1)..procs.len() {
                assert_ne!(procs[i], procs[j]);
            }
        }
    }

    #[test]
    fn test_file_types_distinct() {
        let types = [
            NFS_FTYPE_REG,
            NFS_FTYPE_DIR,
            NFS_FTYPE_BLK,
            NFS_FTYPE_CHR,
            NFS_FTYPE_LNK,
            NFS_FTYPE_SOCK,
            NFS_FTYPE_FIFO,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_status_codes_distinct() {
        let codes = [
            NFS3_OK,
            NFS3ERR_PERM,
            NFS3ERR_NOENT,
            NFS3ERR_IO,
            NFS3ERR_NXIO,
            NFS3ERR_ACCES,
            NFS3ERR_EXIST,
            NFS3ERR_NOTDIR,
            NFS3ERR_ISDIR,
            NFS3ERR_NOSPC,
            NFS3ERR_ROFS,
            NFS3ERR_NAMETOOLONG,
            NFS3ERR_NOTEMPTY,
            NFS3ERR_STALE,
        ];
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                assert_ne!(codes[i], codes[j]);
            }
        }
    }

    #[test]
    fn test_default_port() {
        assert_eq!(NFS_DEFAULT_PORT, 2049);
    }
}
