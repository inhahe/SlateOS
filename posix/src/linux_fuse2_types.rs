//! `<linux/fuse.h>` — FUSE (Filesystem in Userspace) constants (extended).
//!
//! Extended FUSE constants covering opcodes, init flags,
//! attribute flags, open flags, and write flags.

// ---------------------------------------------------------------------------
// FUSE opcodes (FUSE_*)
// ---------------------------------------------------------------------------

/// Lookup entry.
pub const FUSE_LOOKUP: u32 = 1;
/// Forget inode.
pub const FUSE_FORGET: u32 = 2;
/// Get attributes.
pub const FUSE_GETATTR: u32 = 3;
/// Set attributes.
pub const FUSE_SETATTR: u32 = 4;
/// Read symlink.
pub const FUSE_READLINK: u32 = 5;
/// Create symlink.
pub const FUSE_SYMLINK: u32 = 6;
/// Create node (mknod).
pub const FUSE_MKNOD: u32 = 8;
/// Create directory.
pub const FUSE_MKDIR: u32 = 9;
/// Remove file.
pub const FUSE_UNLINK: u32 = 10;
/// Remove directory.
pub const FUSE_RMDIR: u32 = 11;
/// Rename.
pub const FUSE_RENAME: u32 = 12;
/// Create hard link.
pub const FUSE_LINK: u32 = 13;
/// Open file.
pub const FUSE_OPEN: u32 = 14;
/// Read file.
pub const FUSE_READ: u32 = 15;
/// Write file.
pub const FUSE_WRITE: u32 = 16;
/// Get filesystem stats.
pub const FUSE_STATFS: u32 = 17;
/// Release (close) file.
pub const FUSE_RELEASE: u32 = 18;
/// Fsync.
pub const FUSE_FSYNC: u32 = 20;
/// Set extended attribute.
pub const FUSE_SETXATTR: u32 = 21;
/// Get extended attribute.
pub const FUSE_GETXATTR: u32 = 22;
/// List extended attributes.
pub const FUSE_LISTXATTR: u32 = 23;
/// Remove extended attribute.
pub const FUSE_REMOVEXATTR: u32 = 24;
/// Flush.
pub const FUSE_FLUSH: u32 = 25;
/// Init (handshake).
pub const FUSE_INIT: u32 = 26;
/// Open directory.
pub const FUSE_OPENDIR: u32 = 27;
/// Read directory.
pub const FUSE_READDIR: u32 = 28;
/// Release directory.
pub const FUSE_RELEASEDIR: u32 = 29;
/// Fsync directory.
pub const FUSE_FSYNCDIR: u32 = 30;
/// Get lock.
pub const FUSE_GETLK: u32 = 31;
/// Set lock.
pub const FUSE_SETLK: u32 = 32;
/// Set lock (wait).
pub const FUSE_SETLKW: u32 = 33;
/// Access check.
pub const FUSE_ACCESS: u32 = 34;
/// Create + open.
pub const FUSE_CREATE: u32 = 35;
/// Interrupt.
pub const FUSE_INTERRUPT: u32 = 36;
/// BMap (block map).
pub const FUSE_BMAP: u32 = 37;
/// Destroy.
pub const FUSE_DESTROY: u32 = 38;
/// IOctl.
pub const FUSE_IOCTL: u32 = 39;
/// Poll.
pub const FUSE_POLL: u32 = 40;
/// Notify reply.
pub const FUSE_NOTIFY_REPLY: u32 = 41;
/// Batch forget.
pub const FUSE_BATCH_FORGET: u32 = 42;
/// Fallocate.
pub const FUSE_FALLOCATE: u32 = 43;
/// Read directory plus (with attrs).
pub const FUSE_READDIRPLUS: u32 = 44;
/// Rename2.
pub const FUSE_RENAME2: u32 = 45;
/// Lseek.
pub const FUSE_LSEEK: u32 = 46;
/// Copy file range.
pub const FUSE_COPY_FILE_RANGE: u32 = 47;
/// Setup mapping.
pub const FUSE_SETUPMAPPING: u32 = 48;
/// Remove mapping.
pub const FUSE_REMOVEMAPPING: u32 = 49;

// ---------------------------------------------------------------------------
// FUSE init flags
// ---------------------------------------------------------------------------

/// Async read support.
pub const FUSE_ASYNC_READ: u32 = 1 << 0;
/// POSIX locks.
pub const FUSE_POSIX_LOCKS: u32 = 1 << 1;
/// File ops on symlink.
pub const FUSE_FILE_OPS: u32 = 1 << 2;
/// Atomic O_TRUNC.
pub const FUSE_ATOMIC_O_TRUNC: u32 = 1 << 3;
/// Export support.
pub const FUSE_EXPORT_SUPPORT: u32 = 1 << 4;
/// Big writes.
pub const FUSE_BIG_WRITES: u32 = 1 << 5;
/// Don't apply umask.
pub const FUSE_DONT_MASK: u32 = 1 << 6;
/// Splice write.
pub const FUSE_SPLICE_WRITE: u32 = 1 << 7;
/// Splice move.
pub const FUSE_SPLICE_MOVE: u32 = 1 << 8;
/// Splice read.
pub const FUSE_SPLICE_READ: u32 = 1 << 9;
/// BSD file locks.
pub const FUSE_FLOCK_LOCKS: u32 = 1 << 10;
/// Auto invalidate.
pub const FUSE_HAS_IOCTL_DIR: u32 = 1 << 11;
/// Auto inval data.
pub const FUSE_AUTO_INVAL_DATA: u32 = 1 << 12;
/// Readdirplus.
pub const FUSE_DO_READDIRPLUS: u32 = 1 << 13;
/// Readdirplus auto.
pub const FUSE_READDIRPLUS_AUTO: u32 = 1 << 14;
/// Async DIO.
pub const FUSE_ASYNC_DIO: u32 = 1 << 15;
/// Writeback cache.
pub const FUSE_WRITEBACK_CACHE: u32 = 1 << 16;
/// No open support.
pub const FUSE_NO_OPEN_SUPPORT: u32 = 1 << 17;
/// Parallel direct writes.
pub const FUSE_PARALLEL_DIROPS: u32 = 1 << 18;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_opcodes_distinct() {
        let ops = [
            FUSE_LOOKUP,
            FUSE_FORGET,
            FUSE_GETATTR,
            FUSE_SETATTR,
            FUSE_READLINK,
            FUSE_SYMLINK,
            FUSE_MKNOD,
            FUSE_MKDIR,
            FUSE_UNLINK,
            FUSE_RMDIR,
            FUSE_RENAME,
            FUSE_LINK,
            FUSE_OPEN,
            FUSE_READ,
            FUSE_WRITE,
            FUSE_STATFS,
            FUSE_RELEASE,
            FUSE_FSYNC,
            FUSE_SETXATTR,
            FUSE_GETXATTR,
            FUSE_LISTXATTR,
            FUSE_REMOVEXATTR,
            FUSE_FLUSH,
            FUSE_INIT,
            FUSE_OPENDIR,
            FUSE_READDIR,
            FUSE_RELEASEDIR,
            FUSE_FSYNCDIR,
            FUSE_GETLK,
            FUSE_SETLK,
            FUSE_SETLKW,
            FUSE_ACCESS,
            FUSE_CREATE,
            FUSE_INTERRUPT,
            FUSE_BMAP,
            FUSE_DESTROY,
            FUSE_IOCTL,
            FUSE_POLL,
            FUSE_NOTIFY_REPLY,
            FUSE_BATCH_FORGET,
            FUSE_FALLOCATE,
            FUSE_READDIRPLUS,
            FUSE_RENAME2,
            FUSE_LSEEK,
            FUSE_COPY_FILE_RANGE,
            FUSE_SETUPMAPPING,
            FUSE_REMOVEMAPPING,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
    }

    #[test]
    fn test_lookup_is_one() {
        assert_eq!(FUSE_LOOKUP, 1);
    }

    #[test]
    fn test_init_flags_powers_of_two() {
        let flags = [
            FUSE_ASYNC_READ,
            FUSE_POSIX_LOCKS,
            FUSE_FILE_OPS,
            FUSE_ATOMIC_O_TRUNC,
            FUSE_EXPORT_SUPPORT,
            FUSE_BIG_WRITES,
            FUSE_DONT_MASK,
            FUSE_SPLICE_WRITE,
            FUSE_SPLICE_MOVE,
            FUSE_SPLICE_READ,
            FUSE_FLOCK_LOCKS,
            FUSE_HAS_IOCTL_DIR,
            FUSE_AUTO_INVAL_DATA,
            FUSE_DO_READDIRPLUS,
            FUSE_READDIRPLUS_AUTO,
            FUSE_ASYNC_DIO,
            FUSE_WRITEBACK_CACHE,
            FUSE_NO_OPEN_SUPPORT,
            FUSE_PARALLEL_DIROPS,
        ];
        for f in &flags {
            assert!(f.is_power_of_two(), "flag {f:#x} not power of two");
        }
    }

    #[test]
    fn test_init_flags_no_overlap() {
        let flags = [
            FUSE_ASYNC_READ,
            FUSE_POSIX_LOCKS,
            FUSE_FILE_OPS,
            FUSE_ATOMIC_O_TRUNC,
            FUSE_EXPORT_SUPPORT,
            FUSE_BIG_WRITES,
            FUSE_DONT_MASK,
            FUSE_SPLICE_WRITE,
            FUSE_SPLICE_MOVE,
            FUSE_SPLICE_READ,
            FUSE_FLOCK_LOCKS,
            FUSE_HAS_IOCTL_DIR,
            FUSE_AUTO_INVAL_DATA,
            FUSE_DO_READDIRPLUS,
            FUSE_READDIRPLUS_AUTO,
            FUSE_ASYNC_DIO,
            FUSE_WRITEBACK_CACHE,
            FUSE_NO_OPEN_SUPPORT,
            FUSE_PARALLEL_DIROPS,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_init_opcode() {
        assert_eq!(FUSE_INIT, 26);
    }

    #[test]
    fn test_create_opcode() {
        assert_eq!(FUSE_CREATE, 35);
    }
}
