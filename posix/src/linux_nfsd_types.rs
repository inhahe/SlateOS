//! `<linux/nfsd/nfsd.h>` — NFS server (knfsd) constants.
//!
//! knfsd is the kernel NFS server implementation. It exports local
//! filesystems over the network using NFS protocol versions 2, 3,
//! and 4.x. The kernel handles the protocol and filesystem
//! operations; userspace (nfs-utils) handles mount, authentication,
//! and configuration. Configured via /proc/fs/nfsd/ and rpc.nfsd.
//! Used on every NFS file server in enterprises, HPC clusters,
//! and NAS appliances.

// ---------------------------------------------------------------------------
// NFS versions
// ---------------------------------------------------------------------------

/// NFS version 2.
pub const NFSD_VERS_2: u32 = 2;
/// NFS version 3.
pub const NFSD_VERS_3: u32 = 3;
/// NFS version 4.0.
pub const NFSD_VERS_4_0: u32 = 4;
/// NFS minor version 4.1.
pub const NFSD_MINORVERS_4_1: u32 = 1;
/// NFS minor version 4.2.
pub const NFSD_MINORVERS_4_2: u32 = 2;

// ---------------------------------------------------------------------------
// NFS file types
// ---------------------------------------------------------------------------

/// Regular file.
pub const NF4REG: u32 = 1;
/// Directory.
pub const NF4DIR: u32 = 2;
/// Block device.
pub const NF4BLK: u32 = 3;
/// Character device.
pub const NF4CHR: u32 = 4;
/// Symbolic link.
pub const NF4LNK: u32 = 5;
/// Socket.
pub const NF4SOCK: u32 = 6;
/// Named pipe (FIFO).
pub const NF4FIFO: u32 = 7;
/// Named attribute directory (NFSv4).
pub const NF4ATTRDIR: u32 = 8;
/// Named attribute (NFSv4).
pub const NF4NAMEDATTR: u32 = 9;

// ---------------------------------------------------------------------------
// NFSv4 open flags
// ---------------------------------------------------------------------------

/// Create file if it doesn't exist.
pub const NFS4_OPEN_CREATE: u32 = 1 << 0;
/// Open for reading.
pub const NFS4_OPEN_READ: u32 = 1 << 1;
/// Open for writing.
pub const NFS4_OPEN_WRITE: u32 = 1 << 2;
/// Open with delegation wanted.
pub const NFS4_OPEN_DELEGATE_CUR: u32 = 1 << 3;

// ---------------------------------------------------------------------------
// NFSv4 share access/deny modes
// ---------------------------------------------------------------------------

/// Share access: read.
pub const NFS4_SHARE_ACCESS_READ: u32 = 0x0001;
/// Share access: write.
pub const NFS4_SHARE_ACCESS_WRITE: u32 = 0x0002;
/// Share access: read + write.
pub const NFS4_SHARE_ACCESS_BOTH: u32 = 0x0003;
/// Share deny: none.
pub const NFS4_SHARE_DENY_NONE: u32 = 0x0000;
/// Share deny: read.
pub const NFS4_SHARE_DENY_READ: u32 = 0x0001;
/// Share deny: write.
pub const NFS4_SHARE_DENY_WRITE: u32 = 0x0002;
/// Share deny: both.
pub const NFS4_SHARE_DENY_BOTH: u32 = 0x0003;

// ---------------------------------------------------------------------------
// NFSv4 delegation types
// ---------------------------------------------------------------------------

/// No delegation.
pub const NFS4_OPEN_DELEGATE_NONE: u32 = 0;
/// Read delegation.
pub const NFS4_OPEN_DELEGATE_READ: u32 = 1;
/// Write delegation.
pub const NFS4_OPEN_DELEGATE_WRITE: u32 = 2;
/// No delegation, but reason returned.
pub const NFS4_OPEN_DELEGATE_NONE_EXT: u32 = 3;

// ---------------------------------------------------------------------------
// NFS export flags (from /etc/exports)
// ---------------------------------------------------------------------------

/// Export read-only.
pub const NFSEXP_READONLY: u32 = 0x0001;
/// Don't translate insecure file locks.
pub const NFSEXP_INSECURE_PORT: u32 = 0x0002;
/// Map root to nobody.
pub const NFSEXP_ROOTSQUASH: u32 = 0x0004;
/// Map all users to nobody.
pub const NFSEXP_ALLSQUASH: u32 = 0x0008;
/// Secure (require auth from reserved port).
pub const NFSEXP_ASYNC: u32 = 0x0010;
/// No subtree checking.
pub const NFSEXP_NOHIDE: u32 = 0x0200;
/// Crossmnt — auto-mount sub-exports.
pub const NFSEXP_CROSSMOUNT: u32 = 0x4000;

// ---------------------------------------------------------------------------
// NFSD thread limits
// ---------------------------------------------------------------------------

/// Maximum number of NFS server threads.
pub const NFSD_MAXSERVS: u32 = 8192;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_versions_distinct() {
        let vers = [NFSD_VERS_2, NFSD_VERS_3, NFSD_VERS_4_0];
        for i in 0..vers.len() {
            for j in (i + 1)..vers.len() {
                assert_ne!(vers[i], vers[j]);
            }
        }
    }

    #[test]
    fn test_file_types_distinct() {
        let types = [
            NF4REG,
            NF4DIR,
            NF4BLK,
            NF4CHR,
            NF4LNK,
            NF4SOCK,
            NF4FIFO,
            NF4ATTRDIR,
            NF4NAMEDATTR,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_delegation_types_distinct() {
        let deleg = [
            NFS4_OPEN_DELEGATE_NONE,
            NFS4_OPEN_DELEGATE_READ,
            NFS4_OPEN_DELEGATE_WRITE,
            NFS4_OPEN_DELEGATE_NONE_EXT,
        ];
        for i in 0..deleg.len() {
            for j in (i + 1)..deleg.len() {
                assert_ne!(deleg[i], deleg[j]);
            }
        }
    }

    #[test]
    fn test_share_access_composition() {
        assert_eq!(
            NFS4_SHARE_ACCESS_BOTH,
            NFS4_SHARE_ACCESS_READ | NFS4_SHARE_ACCESS_WRITE
        );
    }

    #[test]
    fn test_export_flags_distinct() {
        let flags = [
            NFSEXP_READONLY,
            NFSEXP_INSECURE_PORT,
            NFSEXP_ROOTSQUASH,
            NFSEXP_ALLSQUASH,
            NFSEXP_ASYNC,
            NFSEXP_NOHIDE,
            NFSEXP_CROSSMOUNT,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_maxservs() {
        assert!(NFSD_MAXSERVS > 0);
        assert!(NFSD_MAXSERVS.is_power_of_two());
    }

    #[test]
    fn test_minor_versions_distinct() {
        assert_ne!(NFSD_MINORVERS_4_1, NFSD_MINORVERS_4_2);
    }
}
