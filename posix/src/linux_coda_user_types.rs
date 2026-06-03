//! `<linux/coda.h>` — Coda distributed-fs userspace protocol.
//!
//! Coda is a CMU-derived distributed filesystem. Its kernel
//! driver reads/writes a single upcall device (`/dev/cfs0`),
//! exchanging request/reply records with the userspace Venus
//! daemon. Constants below cover the upcall opcodes and limits.

// ---------------------------------------------------------------------------
// Protocol version
// ---------------------------------------------------------------------------

/// Coda kernel/Venus protocol version.
pub const CODA_KERNEL_VERSION: u32 = 3;

// ---------------------------------------------------------------------------
// Maximum sizes
// ---------------------------------------------------------------------------

/// Maximum upcall message size in bytes.
pub const CODA_MAXMSG: u32 = 8192;
/// Maximum filename length.
pub const CODA_MAXNAMLEN: u32 = 255;
/// Maximum path length.
pub const CODA_MAXPATHLEN: u32 = 1024;

// ---------------------------------------------------------------------------
// Upcall opcodes (struct coda_in_hdr.opcode)
// ---------------------------------------------------------------------------

/// Get root file handle.
pub const CODA_ROOT: u32 = 2;
/// Open an existing file.
pub const CODA_OPEN: u32 = 3;
/// Close a file.
pub const CODA_CLOSE: u32 = 4;
/// Issue an ioctl on a Coda file.
pub const CODA_IOCTL: u32 = 5;
/// Get attributes.
pub const CODA_GETATTR: u32 = 6;
/// Set attributes.
pub const CODA_SETATTR: u32 = 7;
/// Access check.
pub const CODA_ACCESS: u32 = 8;
/// Lookup a name in a directory.
pub const CODA_LOOKUP: u32 = 9;
/// Create a file.
pub const CODA_CREATE: u32 = 10;
/// Remove a file.
pub const CODA_REMOVE: u32 = 11;
/// Link a file.
pub const CODA_LINK: u32 = 12;
/// Rename a file.
pub const CODA_RENAME: u32 = 13;
/// Make a directory.
pub const CODA_MKDIR: u32 = 14;
/// Remove a directory.
pub const CODA_RMDIR: u32 = 15;
/// Symbolic link.
pub const CODA_SYMLINK: u32 = 17;
/// Read a symbolic link.
pub const CODA_READLINK: u32 = 18;
/// Filesystem sync.
pub const CODA_FSYNC: u32 = 19;
/// Get vfsstat.
pub const CODA_VGET: u32 = 20;
/// Signal Venus daemon to flush state.
pub const CODA_SIGNAL: u32 = 21;
/// Replace fid (downcall).
pub const CODA_REPLACE: u32 = 22;
/// Flush file (downcall).
pub const CODA_FLUSH: u32 = 23;
/// Purge user (downcall).
pub const CODA_PURGEUSER: u32 = 24;
/// Zap dir (downcall).
pub const CODA_ZAPDIR: u32 = 26;
/// Zap file (downcall).
pub const CODA_ZAPFILE: u32 = 27;
/// Purge fid (downcall).
pub const CODA_PURGEFID: u32 = 28;
/// Open by-cnode (modern).
pub const CODA_OPEN_BY_PATH: u32 = 32;
/// Resolve path.
pub const CODA_RESOLVE: u32 = 33;
/// Reintegrate (after disconnection).
pub const CODA_REINTEGRATE: u32 = 34;
/// Statfs.
pub const CODA_STATFS: u32 = 35;
/// Store (downcall).
pub const CODA_STORE: u32 = 36;
/// Release.
pub const CODA_RELEASE: u32 = 37;

// ---------------------------------------------------------------------------
// CodaFid magic and tags
// ---------------------------------------------------------------------------

/// Length of a Coda file id (CodaFid) in u32 words.
pub const CODA_FID_LEN: u32 = 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protocol_version() {
        // v3 has been stable since the late-90s rewrite.
        assert_eq!(CODA_KERNEL_VERSION, 3);
    }

    #[test]
    fn test_max_sizes() {
        assert!(CODA_MAXMSG.is_power_of_two());
        assert!(CODA_MAXPATHLEN.is_power_of_two());
        // 255 — fits POSIX NAME_MAX.
        assert_eq!(CODA_MAXNAMLEN, 255);
    }

    #[test]
    fn test_opcodes_distinct() {
        let o = [
            CODA_ROOT, CODA_OPEN, CODA_CLOSE, CODA_IOCTL, CODA_GETATTR,
            CODA_SETATTR, CODA_ACCESS, CODA_LOOKUP, CODA_CREATE, CODA_REMOVE,
            CODA_LINK, CODA_RENAME, CODA_MKDIR, CODA_RMDIR, CODA_SYMLINK,
            CODA_READLINK, CODA_FSYNC, CODA_VGET, CODA_SIGNAL, CODA_REPLACE,
            CODA_FLUSH, CODA_PURGEUSER, CODA_ZAPDIR, CODA_ZAPFILE,
            CODA_PURGEFID, CODA_OPEN_BY_PATH, CODA_RESOLVE, CODA_REINTEGRATE,
            CODA_STATFS, CODA_STORE, CODA_RELEASE,
        ];
        for i in 0..o.len() {
            for j in (i + 1)..o.len() {
                assert_ne!(o[i], o[j]);
            }
        }
    }

    #[test]
    fn test_fid_length() {
        // CodaFid is 4 x u32 = 16 bytes.
        assert_eq!(CODA_FID_LEN, 4);
    }
}
