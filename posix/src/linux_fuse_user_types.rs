//! `<linux/fuse.h>` — Filesystem in Userspace protocol constants.
//!
//! FUSE is how SSHFS, NTFS-3G, GlusterFS, libvirt's virtiofs guest,
//! and most desktop cloud-sync clients (Google Drive, OneDrive) ship
//! a filesystem driver as a userspace daemon. The opcodes and init
//! flags below are the part libfuse and async-fuse libraries pin to.

// ---------------------------------------------------------------------------
// Protocol versions
// ---------------------------------------------------------------------------

/// Major protocol version this header describes.
pub const FUSE_KERNEL_VERSION: u32 = 7;
/// Minor version (libfuse must support at least this much).
pub const FUSE_KERNEL_MINOR_VERSION: u32 = 38;
/// Magic byte sequence at the start of every fuse_in_header.
pub const FUSE_ROOT_ID: u64 = 1;

// ---------------------------------------------------------------------------
// Opcodes (struct fuse_in_header.opcode)
// ---------------------------------------------------------------------------

/// `FUSE_LOOKUP`.
pub const FUSE_LOOKUP: u32 = 1;
/// `FUSE_FORGET` — drop ref to an inode.
pub const FUSE_FORGET: u32 = 2;
/// `FUSE_GETATTR`.
pub const FUSE_GETATTR: u32 = 3;
/// `FUSE_SETATTR`.
pub const FUSE_SETATTR: u32 = 4;
/// `FUSE_READLINK`.
pub const FUSE_READLINK: u32 = 5;
/// `FUSE_SYMLINK`.
pub const FUSE_SYMLINK: u32 = 6;
/// `FUSE_MKNOD`.
pub const FUSE_MKNOD: u32 = 8;
/// `FUSE_MKDIR`.
pub const FUSE_MKDIR: u32 = 9;
/// `FUSE_UNLINK`.
pub const FUSE_UNLINK: u32 = 10;
/// `FUSE_RMDIR`.
pub const FUSE_RMDIR: u32 = 11;
/// `FUSE_RENAME`.
pub const FUSE_RENAME: u32 = 12;
/// `FUSE_LINK`.
pub const FUSE_LINK: u32 = 13;
/// `FUSE_OPEN`.
pub const FUSE_OPEN: u32 = 14;
/// `FUSE_READ`.
pub const FUSE_READ: u32 = 15;
/// `FUSE_WRITE`.
pub const FUSE_WRITE: u32 = 16;
/// `FUSE_STATFS`.
pub const FUSE_STATFS: u32 = 17;
/// `FUSE_RELEASE`.
pub const FUSE_RELEASE: u32 = 18;
/// `FUSE_FSYNC`.
pub const FUSE_FSYNC: u32 = 20;
/// `FUSE_SETXATTR`.
pub const FUSE_SETXATTR: u32 = 21;
/// `FUSE_GETXATTR`.
pub const FUSE_GETXATTR: u32 = 22;
/// `FUSE_LISTXATTR`.
pub const FUSE_LISTXATTR: u32 = 23;
/// `FUSE_REMOVEXATTR`.
pub const FUSE_REMOVEXATTR: u32 = 24;
/// `FUSE_FLUSH`.
pub const FUSE_FLUSH: u32 = 25;
/// `FUSE_INIT`.
pub const FUSE_INIT: u32 = 26;
/// `FUSE_OPENDIR`.
pub const FUSE_OPENDIR: u32 = 27;
/// `FUSE_READDIR`.
pub const FUSE_READDIR: u32 = 28;
/// `FUSE_RELEASEDIR`.
pub const FUSE_RELEASEDIR: u32 = 29;
/// `FUSE_FSYNCDIR`.
pub const FUSE_FSYNCDIR: u32 = 30;
/// `FUSE_GETLK`.
pub const FUSE_GETLK: u32 = 31;
/// `FUSE_SETLK`.
pub const FUSE_SETLK: u32 = 32;
/// `FUSE_SETLKW`.
pub const FUSE_SETLKW: u32 = 33;
/// `FUSE_ACCESS`.
pub const FUSE_ACCESS: u32 = 34;
/// `FUSE_CREATE` — open with O_CREAT.
pub const FUSE_CREATE: u32 = 35;
/// `FUSE_INTERRUPT` — cancel a pending op.
pub const FUSE_INTERRUPT: u32 = 36;
/// `FUSE_DESTROY`.
pub const FUSE_DESTROY: u32 = 38;
/// `FUSE_IOCTL`.
pub const FUSE_IOCTL: u32 = 39;

// ---------------------------------------------------------------------------
// INIT flags (struct fuse_init_in.flags)
// ---------------------------------------------------------------------------

/// Support async read.
pub const FUSE_ASYNC_READ: u32 = 1 << 0;
/// Support POSIX locks.
pub const FUSE_POSIX_LOCKS: u32 = 1 << 1;
/// Don't enforce atomic O_TRUNC.
pub const FUSE_FILE_OPS: u32 = 1 << 2;
/// Support atomic O_TRUNC.
pub const FUSE_ATOMIC_O_TRUNC: u32 = 1 << 3;
/// Export support for NFS.
pub const FUSE_EXPORT_SUPPORT: u32 = 1 << 4;
/// Big writes (>4 KiB).
pub const FUSE_BIG_WRITES: u32 = 1 << 5;
/// Don't apply umask.
pub const FUSE_DONT_MASK: u32 = 1 << 6;
/// Splice write.
pub const FUSE_SPLICE_WRITE: u32 = 1 << 7;
/// Splice move.
pub const FUSE_SPLICE_MOVE: u32 = 1 << 8;
/// Splice read.
pub const FUSE_SPLICE_READ: u32 = 1 << 9;
/// Flock support.
pub const FUSE_FLOCK_LOCKS: u32 = 1 << 10;
/// Open inode ioctls.
pub const FUSE_HAS_IOCTL_DIR: u32 = 1 << 11;
/// Auto-invalidate cached data.
pub const FUSE_AUTO_INVAL_DATA: u32 = 1 << 12;
/// `do_readdirplus`.
pub const FUSE_DO_READDIRPLUS: u32 = 1 << 13;
/// Server supplies a maximum-write hint.
pub const FUSE_MAX_PAGES: u32 = 1 << 22;

// ---------------------------------------------------------------------------
// Buffer sizes
// ---------------------------------------------------------------------------

/// Minimum read buffer size negotiated.
pub const FUSE_MIN_READ_BUFFER: u32 = 8192;
/// Compat data size (pre-7.5 INIT).
pub const FUSE_COMPAT_INIT_OUT_SIZE: u32 = 8;
/// Modern fuse_init_out byte size.
pub const FUSE_INIT_OUT_SIZE: u32 = 64;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version() {
        // Protocol major is 7; minor 38 matches recent libfuse releases.
        assert_eq!(FUSE_KERNEL_VERSION, 7);
        assert!(FUSE_KERNEL_MINOR_VERSION >= 30);
        // Root inode id is 1; the kernel uses 0 to mean "no inode".
        assert_eq!(FUSE_ROOT_ID, 1);
    }

    #[test]
    fn test_opcodes_distinct() {
        let o = [
            FUSE_LOOKUP, FUSE_FORGET, FUSE_GETATTR, FUSE_SETATTR,
            FUSE_READLINK, FUSE_SYMLINK, FUSE_MKNOD, FUSE_MKDIR,
            FUSE_UNLINK, FUSE_RMDIR, FUSE_RENAME, FUSE_LINK, FUSE_OPEN,
            FUSE_READ, FUSE_WRITE, FUSE_STATFS, FUSE_RELEASE, FUSE_FSYNC,
            FUSE_SETXATTR, FUSE_GETXATTR, FUSE_LISTXATTR, FUSE_REMOVEXATTR,
            FUSE_FLUSH, FUSE_INIT, FUSE_OPENDIR, FUSE_READDIR,
            FUSE_RELEASEDIR, FUSE_FSYNCDIR, FUSE_GETLK, FUSE_SETLK,
            FUSE_SETLKW, FUSE_ACCESS, FUSE_CREATE, FUSE_INTERRUPT,
            FUSE_DESTROY, FUSE_IOCTL,
        ];
        for i in 0..o.len() {
            for j in (i + 1)..o.len() {
                assert_ne!(o[i], o[j]);
            }
        }
        // FUSE_LOOKUP is opcode 1 (zero is reserved).
        assert_eq!(FUSE_LOOKUP, 1);
        // INIT must be 26 — anything else breaks daemon handshake.
        assert_eq!(FUSE_INIT, 26);
    }

    #[test]
    fn test_init_flags_pow2_distinct() {
        let f = [
            FUSE_ASYNC_READ, FUSE_POSIX_LOCKS, FUSE_FILE_OPS,
            FUSE_ATOMIC_O_TRUNC, FUSE_EXPORT_SUPPORT, FUSE_BIG_WRITES,
            FUSE_DONT_MASK, FUSE_SPLICE_WRITE, FUSE_SPLICE_MOVE,
            FUSE_SPLICE_READ, FUSE_FLOCK_LOCKS, FUSE_HAS_IOCTL_DIR,
            FUSE_AUTO_INVAL_DATA, FUSE_DO_READDIRPLUS, FUSE_MAX_PAGES,
        ];
        for &b in &f {
            assert!(b.is_power_of_two());
        }
        for i in 0..f.len() {
            for j in (i + 1)..f.len() {
                assert_ne!(f[i], f[j]);
            }
        }
    }

    #[test]
    fn test_buffer_sizes() {
        // The kernel never accepts a read buffer below 8 KiB.
        assert_eq!(FUSE_MIN_READ_BUFFER, 8192);
        assert!(FUSE_INIT_OUT_SIZE > FUSE_COMPAT_INIT_OUT_SIZE);
        assert_eq!(FUSE_INIT_OUT_SIZE, 64);
    }
}
