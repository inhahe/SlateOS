//! `<linux/nfs.h>` / `<linux/nfs4.h>` — Additional NFS constants.
//!
//! Supplementary NFS constants covering NFSv4 operations,
//! access bits, delegation types, and lock types.

// ---------------------------------------------------------------------------
// NFS version numbers
// ---------------------------------------------------------------------------

/// NFSv2.
pub const NFS2_VERSION: u32 = 2;
/// NFSv3.
pub const NFS3_VERSION: u32 = 3;
/// NFSv4.
pub const NFS4_VERSION: u32 = 4;

// ---------------------------------------------------------------------------
// NFSv3 procedure numbers
// ---------------------------------------------------------------------------

/// Null.
pub const NFS3_NULL: u32 = 0;
/// Getattr.
pub const NFS3_GETATTR: u32 = 1;
/// Setattr.
pub const NFS3_SETATTR: u32 = 2;
/// Lookup.
pub const NFS3_LOOKUP: u32 = 3;
/// Access.
pub const NFS3_ACCESS: u32 = 4;
/// Readlink.
pub const NFS3_READLINK: u32 = 5;
/// Read.
pub const NFS3_READ: u32 = 6;
/// Write.
pub const NFS3_WRITE: u32 = 7;
/// Create.
pub const NFS3_CREATE: u32 = 8;
/// Mkdir.
pub const NFS3_MKDIR: u32 = 9;
/// Symlink.
pub const NFS3_SYMLINK: u32 = 10;
/// Mknod.
pub const NFS3_MKNOD: u32 = 11;
/// Remove.
pub const NFS3_REMOVE: u32 = 12;
/// Rmdir.
pub const NFS3_RMDIR: u32 = 13;
/// Rename.
pub const NFS3_RENAME: u32 = 14;
/// Link.
pub const NFS3_LINK: u32 = 15;
/// Readdir.
pub const NFS3_READDIR: u32 = 16;
/// Readdirplus.
pub const NFS3_READDIRPLUS: u32 = 17;
/// Fsstat.
pub const NFS3_FSSTAT: u32 = 18;
/// Fsinfo.
pub const NFS3_FSINFO: u32 = 19;
/// Pathconf.
pub const NFS3_PATHCONF: u32 = 20;
/// Commit.
pub const NFS3_COMMIT: u32 = 21;

// ---------------------------------------------------------------------------
// NFS3 access bits
// ---------------------------------------------------------------------------

/// Read.
pub const NFS3_ACCESS_READ: u32 = 0x0001;
/// Lookup.
pub const NFS3_ACCESS_LOOKUP: u32 = 0x0002;
/// Modify.
pub const NFS3_ACCESS_MODIFY: u32 = 0x0004;
/// Extend.
pub const NFS3_ACCESS_EXTEND: u32 = 0x0008;
/// Delete.
pub const NFS3_ACCESS_DELETE: u32 = 0x0010;
/// Execute.
pub const NFS3_ACCESS_EXECUTE: u32 = 0x0020;

// ---------------------------------------------------------------------------
// NFSv4 delegation types
// ---------------------------------------------------------------------------

/// No delegation.
pub const NFS4_OPEN_DELEGATE_NONE: u32 = 0;
/// Read delegation.
pub const NFS4_OPEN_DELEGATE_READ: u32 = 1;
/// Write delegation.
pub const NFS4_OPEN_DELEGATE_WRITE: u32 = 2;
/// None ext.
pub const NFS4_OPEN_DELEGATE_NONE_EXT: u32 = 3;

// ---------------------------------------------------------------------------
// NFSv4 lock types
// ---------------------------------------------------------------------------

/// Read lock.
pub const NFS4_READ_LT: u32 = 1;
/// Write lock.
pub const NFS4_WRITE_LT: u32 = 2;
/// Read lock (wait).
pub const NFS4_READW_LT: u32 = 3;
/// Write lock (wait).
pub const NFS4_WRITEW_LT: u32 = 4;

// ---------------------------------------------------------------------------
// File handle sizes
// ---------------------------------------------------------------------------

/// NFS2 file handle size.
pub const NFS2_FHSIZE: u32 = 32;
/// NFS3 max file handle size.
pub const NFS3_FHSIZE: u32 = 64;
/// NFS4 max file handle size.
pub const NFS4_FHSIZE: u32 = 128;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_versions() {
        assert_eq!(NFS2_VERSION, 2);
        assert_eq!(NFS3_VERSION, 3);
        assert_eq!(NFS4_VERSION, 4);
    }

    #[test]
    fn test_nfs3_procs_sequential() {
        assert_eq!(NFS3_NULL, 0);
        assert_eq!(NFS3_GETATTR, 1);
        assert_eq!(NFS3_COMMIT, 21);
    }

    #[test]
    fn test_nfs3_procs_distinct() {
        let procs = [
            NFS3_NULL, NFS3_GETATTR, NFS3_SETATTR, NFS3_LOOKUP,
            NFS3_ACCESS, NFS3_READLINK, NFS3_READ, NFS3_WRITE,
            NFS3_CREATE, NFS3_MKDIR, NFS3_SYMLINK, NFS3_MKNOD,
            NFS3_REMOVE, NFS3_RMDIR, NFS3_RENAME, NFS3_LINK,
            NFS3_READDIR, NFS3_READDIRPLUS, NFS3_FSSTAT,
            NFS3_FSINFO, NFS3_PATHCONF, NFS3_COMMIT,
        ];
        for i in 0..procs.len() {
            for j in (i + 1)..procs.len() {
                assert_ne!(procs[i], procs[j]);
            }
        }
    }

    #[test]
    fn test_access_bits_power_of_two() {
        let bits = [
            NFS3_ACCESS_READ, NFS3_ACCESS_LOOKUP,
            NFS3_ACCESS_MODIFY, NFS3_ACCESS_EXTEND,
            NFS3_ACCESS_DELETE, NFS3_ACCESS_EXECUTE,
        ];
        for b in &bits {
            assert!(b.is_power_of_two(), "0x{:04x} not power of two", b);
        }
    }

    #[test]
    fn test_delegation_types() {
        assert_eq!(NFS4_OPEN_DELEGATE_NONE, 0);
        assert_eq!(NFS4_OPEN_DELEGATE_READ, 1);
        assert_eq!(NFS4_OPEN_DELEGATE_WRITE, 2);
    }

    #[test]
    fn test_lock_types_distinct() {
        let types = [NFS4_READ_LT, NFS4_WRITE_LT, NFS4_READW_LT, NFS4_WRITEW_LT];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_fh_sizes() {
        assert!(NFS2_FHSIZE < NFS3_FHSIZE);
        assert!(NFS3_FHSIZE < NFS4_FHSIZE);
    }
}
