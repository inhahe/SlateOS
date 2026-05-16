//! `<linux/fuse.h>` — Filesystem in Userspace protocol.
//!
//! FUSE allows implementing filesystems in userspace. The kernel
//! module communicates with the userspace daemon via `/dev/fuse`
//! using a request/response protocol defined here.

// ---------------------------------------------------------------------------
// FUSE opcodes
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
/// Create file node.
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
/// Read data.
pub const FUSE_READ: u32 = 15;
/// Write data.
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
/// Flush (before close).
pub const FUSE_FLUSH: u32 = 25;
/// Initialize connection.
pub const FUSE_INIT: u32 = 26;
/// Open directory.
pub const FUSE_OPENDIR: u32 = 27;
/// Read directory.
pub const FUSE_READDIR: u32 = 28;
/// Release directory.
pub const FUSE_RELEASEDIR: u32 = 29;
/// Fsync directory.
pub const FUSE_FSYNCDIR: u32 = 30;
/// Get file lock.
pub const FUSE_GETLK: u32 = 31;
/// Set file lock.
pub const FUSE_SETLK: u32 = 32;
/// Set file lock wait.
pub const FUSE_SETLKW: u32 = 33;
/// Access check.
pub const FUSE_ACCESS: u32 = 34;
/// Create and open.
pub const FUSE_CREATE: u32 = 35;
/// Interrupt.
pub const FUSE_INTERRUPT: u32 = 36;
/// Memory-map.
pub const FUSE_BMAP: u32 = 37;
/// Destroy.
pub const FUSE_DESTROY: u32 = 38;
/// Ioctl.
pub const FUSE_IOCTL: u32 = 39;
/// Poll.
pub const FUSE_POLL: u32 = 40;
/// Notify reply.
pub const FUSE_NOTIFY_REPLY: u32 = 41;
/// Batch forget.
pub const FUSE_BATCH_FORGET: u32 = 42;
/// Fallocate.
pub const FUSE_FALLOCATE: u32 = 43;
/// Read directory plus (with attributes).
pub const FUSE_READDIRPLUS: u32 = 44;
/// Rename2 (with flags).
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

/// Asynchronous read.
pub const FUSE_ASYNC_READ: u32 = 1 << 0;
/// POSIX file locks.
pub const FUSE_POSIX_LOCKS: u32 = 1 << 1;
/// Atomic O_TRUNC.
pub const FUSE_ATOMIC_O_TRUNC: u32 = 1 << 3;
/// Export support (NFS).
pub const FUSE_EXPORT_SUPPORT: u32 = 1 << 4;
/// Handle killpriv.
pub const FUSE_HANDLE_KILLPRIV: u32 = 1 << 6;
/// POSIX ACL.
pub const FUSE_POSIX_ACL: u32 = 1 << 17;
/// Writeback cache.
pub const FUSE_WRITEBACK_CACHE: u32 = 1 << 16;
/// Parallel directory operations.
pub const FUSE_PARALLEL_DIROPS: u32 = 1 << 18;
/// No open support.
pub const FUSE_NO_OPEN_SUPPORT: u32 = 1 << 23;
/// No opendir support.
pub const FUSE_NO_OPENDIR_SUPPORT: u32 = 1 << 24;

// ---------------------------------------------------------------------------
// FUSE header structs
// ---------------------------------------------------------------------------

/// FUSE request header (40 bytes).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct FuseInHeader {
    /// Total message length.
    pub len: u32,
    /// Opcode (FUSE_*).
    pub opcode: u32,
    /// Unique request ID.
    pub unique: u64,
    /// Inode number.
    pub nodeid: u64,
    /// Calling user ID.
    pub uid: u32,
    /// Calling group ID.
    pub gid: u32,
    /// Calling process ID.
    pub pid: u32,
    /// Padding.
    _padding: u32,
}

impl FuseInHeader {
    /// Create a zeroed request header.
    pub fn zeroed() -> Self {
        // SAFETY: All-zero is valid for this repr(C) struct.
        unsafe { core::mem::zeroed() }
    }
}

/// FUSE response header (16 bytes).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct FuseOutHeader {
    /// Total message length.
    pub len: u32,
    /// Error (negative errno, or 0 for success).
    pub error: i32,
    /// Unique request ID (from FuseInHeader).
    pub unique: u64,
}

impl FuseOutHeader {
    /// Create a zeroed response header.
    pub fn zeroed() -> Self {
        // SAFETY: All-zero is valid for this repr(C) struct.
        unsafe { core::mem::zeroed() }
    }
}

// ---------------------------------------------------------------------------
// FUSE protocol version
// ---------------------------------------------------------------------------

/// Major protocol version.
pub const FUSE_KERNEL_VERSION: u32 = 7;
/// Minor protocol version (7.38 as of Linux 6.x).
pub const FUSE_KERNEL_MINOR_VERSION: u32 = 38;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fuse_in_header_size() {
        assert_eq!(core::mem::size_of::<FuseInHeader>(), 40);
    }

    #[test]
    fn test_fuse_out_header_size() {
        assert_eq!(core::mem::size_of::<FuseOutHeader>(), 16);
    }

    #[test]
    fn test_opcodes_distinct() {
        let ops = [
            FUSE_LOOKUP, FUSE_FORGET, FUSE_GETATTR, FUSE_SETATTR,
            FUSE_READLINK, FUSE_SYMLINK, FUSE_MKNOD, FUSE_MKDIR,
            FUSE_UNLINK, FUSE_RMDIR, FUSE_RENAME, FUSE_LINK,
            FUSE_OPEN, FUSE_READ, FUSE_WRITE, FUSE_STATFS,
            FUSE_RELEASE, FUSE_FSYNC, FUSE_INIT, FUSE_OPENDIR,
            FUSE_READDIR, FUSE_RELEASEDIR, FUSE_CREATE,
            FUSE_READDIRPLUS, FUSE_RENAME2, FUSE_LSEEK,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
    }

    #[test]
    fn test_fuse_version() {
        assert_eq!(FUSE_KERNEL_VERSION, 7);
        assert!(FUSE_KERNEL_MINOR_VERSION > 0);
    }

    #[test]
    fn test_init_flags_are_powers_of_two() {
        let flags = [
            FUSE_ASYNC_READ, FUSE_POSIX_LOCKS, FUSE_ATOMIC_O_TRUNC,
            FUSE_EXPORT_SUPPORT, FUSE_HANDLE_KILLPRIV,
            FUSE_WRITEBACK_CACHE, FUSE_POSIX_ACL,
            FUSE_PARALLEL_DIROPS, FUSE_NO_OPEN_SUPPORT,
            FUSE_NO_OPENDIR_SUPPORT,
        ];
        for f in &flags {
            assert!(f.is_power_of_two(), "flag {f:#x} not a power of 2");
        }
    }

    #[test]
    fn test_headers_zeroed() {
        let inh = FuseInHeader::zeroed();
        assert_eq!(inh.len, 0);
        assert_eq!(inh.opcode, 0);
        assert_eq!(inh.unique, 0);
        let outh = FuseOutHeader::zeroed();
        assert_eq!(outh.len, 0);
        assert_eq!(outh.error, 0);
    }
}
