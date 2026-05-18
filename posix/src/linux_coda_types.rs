//! `<linux/coda.h>` — Coda distributed filesystem constants.
//!
//! Coda is a distributed filesystem based on AFS with
//! disconnected operation support. These constants define
//! opcodes, vnode types, and permission flags.

// ---------------------------------------------------------------------------
// Magic
// ---------------------------------------------------------------------------

/// Coda super magic.
pub const CODA_SUPER_MAGIC: u32 = 0x73757245;

// ---------------------------------------------------------------------------
// Upcall opcodes (kernel → venus)
// ---------------------------------------------------------------------------

/// Lookup.
pub const CODA_ROOT: u32 = 2;
/// Open file.
pub const CODA_OPEN: u32 = 3;
/// Close file.
pub const CODA_CLOSE: u32 = 4;
/// IO control.
pub const CODA_IOCTL: u32 = 5;
/// Get attributes.
pub const CODA_GETATTR: u32 = 6;
/// Set attributes.
pub const CODA_SETATTR: u32 = 7;
/// Access check.
pub const CODA_ACCESS: u32 = 8;
/// Lookup name.
pub const CODA_LOOKUP: u32 = 9;
/// Create file.
pub const CODA_CREATE: u32 = 10;
/// Remove file.
pub const CODA_REMOVE: u32 = 11;
/// Create link.
pub const CODA_LINK: u32 = 12;
/// Rename.
pub const CODA_RENAME: u32 = 13;
/// Create directory.
pub const CODA_MKDIR: u32 = 14;
/// Remove directory.
pub const CODA_RMDIR: u32 = 15;
/// Read directory.
pub const CODA_READDIR: u32 = 16;
/// Create symlink.
pub const CODA_SYMLINK: u32 = 17;
/// Read symlink.
pub const CODA_READLINK: u32 = 18;
/// Fsync.
pub const CODA_FSYNC: u32 = 19;
/// Statfs.
pub const CODA_STATFS: u32 = 25;
/// Store.
pub const CODA_STORE: u32 = 26;
/// Release.
pub const CODA_RELEASE: u32 = 27;

// ---------------------------------------------------------------------------
// Downcall opcodes (venus → kernel)
// ---------------------------------------------------------------------------

/// Purge user credentials.
pub const CODA_PURGEUSER: u32 = 30;
/// Zapfile (invalidate cached file).
pub const CODA_ZAPFILE: u32 = 31;
/// Zapdir (invalidate cached dir).
pub const CODA_ZAPDIR: u32 = 32;
/// Purgefid.
pub const CODA_PURGEFID: u32 = 34;
/// Replace.
pub const CODA_REPLACE: u32 = 35;

// ---------------------------------------------------------------------------
// Vnode types
// ---------------------------------------------------------------------------

/// Regular file.
pub const C_VREG: u32 = 1;
/// Directory.
pub const C_VDIR: u32 = 2;
/// Block device.
pub const C_VBLK: u32 = 3;
/// Character device.
pub const C_VCHR: u32 = 4;
/// Symbolic link.
pub const C_VLNK: u32 = 5;
/// Socket.
pub const C_VSOCK: u32 = 6;
/// FIFO.
pub const C_VFIFO: u32 = 7;
/// Bad vnode.
pub const C_VBAD: u32 = 8;

// ---------------------------------------------------------------------------
// Access permission bits
// ---------------------------------------------------------------------------

/// Read permission.
pub const C_A_R_OK: u32 = 4;
/// Write permission.
pub const C_A_W_OK: u32 = 2;
/// Execute permission.
pub const C_A_X_OK: u32 = 1;
/// File existence.
pub const C_A_F_OK: u32 = 0;

// ---------------------------------------------------------------------------
// Open flags
// ---------------------------------------------------------------------------

/// Open for read.
pub const C_O_READ: u32 = 0x001;
/// Open for write.
pub const C_O_WRITE: u32 = 0x002;
/// Open for truncate.
pub const C_O_TRUNC: u32 = 0x010;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_super_magic() {
        assert_eq!(CODA_SUPER_MAGIC, 0x73757245);
    }

    #[test]
    fn test_upcall_ops_distinct() {
        let ops = [
            CODA_ROOT, CODA_OPEN, CODA_CLOSE, CODA_IOCTL,
            CODA_GETATTR, CODA_SETATTR, CODA_ACCESS, CODA_LOOKUP,
            CODA_CREATE, CODA_REMOVE, CODA_LINK, CODA_RENAME,
            CODA_MKDIR, CODA_RMDIR, CODA_READDIR, CODA_SYMLINK,
            CODA_READLINK, CODA_FSYNC, CODA_STATFS, CODA_STORE,
            CODA_RELEASE,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
    }

    #[test]
    fn test_downcall_ops_distinct() {
        let ops = [
            CODA_PURGEUSER, CODA_ZAPFILE, CODA_ZAPDIR,
            CODA_PURGEFID, CODA_REPLACE,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
    }

    #[test]
    fn test_vnode_types_sequential() {
        assert_eq!(C_VREG, 1);
        assert_eq!(C_VDIR, 2);
        assert_eq!(C_VBAD, 8);
    }

    #[test]
    fn test_access_bits() {
        assert_eq!(C_A_F_OK, 0);
        assert_eq!(C_A_X_OK, 1);
        assert_eq!(C_A_W_OK, 2);
        assert_eq!(C_A_R_OK, 4);
    }

    #[test]
    fn test_open_flags_distinct() {
        let flags = [C_O_READ, C_O_WRITE, C_O_TRUNC];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }
}
