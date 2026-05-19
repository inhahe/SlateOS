//! `<linux/nfs.h>` — Additional NFS constants (part 4).
//!
//! Supplementary NFS constants covering NFSv4 operation types,
//! access flags, and delegation types.

// ---------------------------------------------------------------------------
// NFSv4 operations
// ---------------------------------------------------------------------------

/// Access check.
pub const OP_ACCESS: u32 = 3;
/// Close.
pub const OP_CLOSE: u32 = 4;
/// Commit.
pub const OP_COMMIT: u32 = 5;
/// Create.
pub const OP_CREATE: u32 = 6;
/// Delegpurge.
pub const OP_DELEGPURGE: u32 = 7;
/// Delegreturn.
pub const OP_DELEGRETURN: u32 = 8;
/// Getattr.
pub const OP_GETATTR: u32 = 9;
/// Getfh.
pub const OP_GETFH: u32 = 10;
/// Link.
pub const OP_LINK: u32 = 11;
/// Lock.
pub const OP_LOCK: u32 = 12;
/// Lockt.
pub const OP_LOCKT: u32 = 13;
/// Locku.
pub const OP_LOCKU: u32 = 14;
/// Lookup.
pub const OP_LOOKUP: u32 = 15;
/// Lookupp.
pub const OP_LOOKUPP: u32 = 16;
/// Nverify.
pub const OP_NVERIFY: u32 = 17;
/// Open.
pub const OP_OPEN: u32 = 18;
/// Openattr.
pub const OP_OPENATTR: u32 = 19;
/// Open confirm.
pub const OP_OPEN_CONFIRM: u32 = 20;
/// Open downgrade.
pub const OP_OPEN_DOWNGRADE: u32 = 21;
/// Putfh.
pub const OP_PUTFH: u32 = 22;
/// Putpubfh.
pub const OP_PUTPUBFH: u32 = 23;
/// Putrootfh.
pub const OP_PUTROOTFH: u32 = 24;
/// Read.
pub const OP_READ: u32 = 25;
/// Readdir.
pub const OP_READDIR: u32 = 26;
/// Readlink.
pub const OP_READLINK: u32 = 27;
/// Remove.
pub const OP_REMOVE: u32 = 28;
/// Rename.
pub const OP_RENAME: u32 = 29;

// ---------------------------------------------------------------------------
// NFSv4 access bits
// ---------------------------------------------------------------------------

/// Read data.
pub const NFS4_ACCESS_READ: u32 = 0x0001;
/// Lookup.
pub const NFS4_ACCESS_LOOKUP: u32 = 0x0002;
/// Modify.
pub const NFS4_ACCESS_MODIFY: u32 = 0x0004;
/// Extend.
pub const NFS4_ACCESS_EXTEND: u32 = 0x0008;
/// Delete.
pub const NFS4_ACCESS_DELETE: u32 = 0x0010;
/// Execute.
pub const NFS4_ACCESS_EXECUTE: u32 = 0x0020;
/// XAttr read.
pub const NFS4_ACCESS_XAREAD: u32 = 0x0040;
/// XAttr write.
pub const NFS4_ACCESS_XAWRITE: u32 = 0x0080;
/// XAttr list.
pub const NFS4_ACCESS_XALIST: u32 = 0x0100;

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
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ops_distinct() {
        let ops = [
            OP_ACCESS, OP_CLOSE, OP_COMMIT, OP_CREATE,
            OP_DELEGPURGE, OP_DELEGRETURN, OP_GETATTR,
            OP_GETFH, OP_LINK, OP_LOCK, OP_LOCKT,
            OP_LOCKU, OP_LOOKUP, OP_LOOKUPP, OP_NVERIFY,
            OP_OPEN, OP_OPENATTR, OP_OPEN_CONFIRM,
            OP_OPEN_DOWNGRADE, OP_PUTFH, OP_PUTPUBFH,
            OP_PUTROOTFH, OP_READ, OP_READDIR,
            OP_READLINK, OP_REMOVE, OP_RENAME,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
    }

    #[test]
    fn test_access_bits_no_overlap() {
        let bits = [
            NFS4_ACCESS_READ, NFS4_ACCESS_LOOKUP,
            NFS4_ACCESS_MODIFY, NFS4_ACCESS_EXTEND,
            NFS4_ACCESS_DELETE, NFS4_ACCESS_EXECUTE,
            NFS4_ACCESS_XAREAD, NFS4_ACCESS_XAWRITE,
            NFS4_ACCESS_XALIST,
        ];
        for i in 0..bits.len() {
            for j in (i + 1)..bits.len() {
                assert_eq!(bits[i] & bits[j], 0);
            }
        }
    }

    #[test]
    fn test_delegation_types_distinct() {
        let types = [
            NFS4_OPEN_DELEGATE_NONE, NFS4_OPEN_DELEGATE_READ,
            NFS4_OPEN_DELEGATE_WRITE, NFS4_OPEN_DELEGATE_NONE_EXT,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }
}
