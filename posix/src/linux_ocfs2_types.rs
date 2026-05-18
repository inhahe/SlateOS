//! `<linux/ocfs2_fs.h>` — OCFS2 (Oracle Cluster File System 2) constants.
//!
//! OCFS2 is a shared-disk cluster filesystem.
//! These constants define superblock flags, inode flags,
//! journal parameters, and DLM lock types.

// ---------------------------------------------------------------------------
// Magic numbers
// ---------------------------------------------------------------------------

/// OCFS2 superblock magic.
pub const OCFS2_SUPER_MAGIC: u32 = 0x7461636F;

// ---------------------------------------------------------------------------
// Feature flags — compatible
// ---------------------------------------------------------------------------

/// Has backup super.
pub const OCFS2_FEATURE_COMPAT_BACKUP_SB: u32 = 0x0001;
/// Has journal checksum.
pub const OCFS2_FEATURE_COMPAT_JBD2_SB: u32 = 0x0002;

// ---------------------------------------------------------------------------
// Feature flags — incompatible
// ---------------------------------------------------------------------------

/// Inline data.
pub const OCFS2_FEATURE_INCOMPAT_INLINE_DATA: u32 = 0x0040;
/// Extended attributes.
pub const OCFS2_FEATURE_INCOMPAT_EXTENDED_SLOT_MAP: u32 = 0x0100;
/// Indexed directories.
pub const OCFS2_FEATURE_INCOMPAT_INDEXED_DIRS: u32 = 0x0200;
/// Metadata checksum.
pub const OCFS2_FEATURE_INCOMPAT_META_ECC: u32 = 0x0800;
/// Refcount trees.
pub const OCFS2_FEATURE_INCOMPAT_REFCOUNT_TREE: u32 = 0x1000;
/// Discontig block groups.
pub const OCFS2_FEATURE_INCOMPAT_DISCONTIG_BG: u32 = 0x2000;
/// Clusterinfo uses stack glue.
pub const OCFS2_FEATURE_INCOMPAT_CLUSTERINFO: u32 = 0x4000;

// ---------------------------------------------------------------------------
// Inode flags
// ---------------------------------------------------------------------------

/// Valid inode.
pub const OCFS2_VALID_FL: u32 = 0x00000001;
/// System file.
pub const OCFS2_SYSTEM_FL: u32 = 0x00000002;
/// Super block.
pub const OCFS2_SUPER_BLOCK_FL: u32 = 0x00000004;
/// Local alloc.
pub const OCFS2_LOCAL_ALLOC_FL: u32 = 0x00000008;
/// Bitmap inode.
pub const OCFS2_BITMAP_FL: u32 = 0x00000010;
/// Journal inode.
pub const OCFS2_JOURNAL_FL: u32 = 0x00000020;
/// Heartbeat inode.
pub const OCFS2_HEARTBEAT_FL: u32 = 0x00000040;
/// Orphan dir.
pub const OCFS2_ORPHAN_FL: u32 = 0x00000080;

// ---------------------------------------------------------------------------
// DLM lock types
// ---------------------------------------------------------------------------

/// No lock.
pub const OCFS2_LOCK_TYPE_NONE: u32 = 0;
/// Metadata lock.
pub const OCFS2_LOCK_TYPE_META: u32 = 1;
/// Data lock.
pub const OCFS2_LOCK_TYPE_DATA: u32 = 2;
/// Super lock.
pub const OCFS2_LOCK_TYPE_SUPER: u32 = 3;
/// Rename lock.
pub const OCFS2_LOCK_TYPE_RENAME: u32 = 4;
/// RW lock.
pub const OCFS2_LOCK_TYPE_RW: u32 = 5;
/// Dentry lock.
pub const OCFS2_LOCK_TYPE_DENTRY: u32 = 6;
/// Open lock.
pub const OCFS2_LOCK_TYPE_OPEN: u32 = 7;
/// Flock lock.
pub const OCFS2_LOCK_TYPE_FLOCK: u32 = 8;
/// Quota lock.
pub const OCFS2_LOCK_TYPE_QINFO: u32 = 9;

// ---------------------------------------------------------------------------
// Cluster / slot limits
// ---------------------------------------------------------------------------

/// Maximum cluster name length.
pub const OCFS2_CLUSTER_NAME_LEN: u32 = 16;
/// Maximum slot count.
pub const OCFS2_MAX_SLOTS: u32 = 255;
/// Max filename length.
pub const OCFS2_MAX_FILENAME_LEN: u32 = 255;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_super_magic() {
        assert_eq!(OCFS2_SUPER_MAGIC, 0x7461636F);
    }

    #[test]
    fn test_incompat_features_distinct() {
        let feats = [
            OCFS2_FEATURE_INCOMPAT_INLINE_DATA,
            OCFS2_FEATURE_INCOMPAT_EXTENDED_SLOT_MAP,
            OCFS2_FEATURE_INCOMPAT_INDEXED_DIRS,
            OCFS2_FEATURE_INCOMPAT_META_ECC,
            OCFS2_FEATURE_INCOMPAT_REFCOUNT_TREE,
            OCFS2_FEATURE_INCOMPAT_DISCONTIG_BG,
            OCFS2_FEATURE_INCOMPAT_CLUSTERINFO,
        ];
        for i in 0..feats.len() {
            for j in (i + 1)..feats.len() {
                assert_ne!(feats[i], feats[j]);
            }
        }
    }

    #[test]
    fn test_inode_flags_power_of_two() {
        let flags = [
            OCFS2_VALID_FL, OCFS2_SYSTEM_FL, OCFS2_SUPER_BLOCK_FL,
            OCFS2_LOCAL_ALLOC_FL, OCFS2_BITMAP_FL, OCFS2_JOURNAL_FL,
            OCFS2_HEARTBEAT_FL, OCFS2_ORPHAN_FL,
        ];
        for f in &flags {
            assert!(f.is_power_of_two(), "0x{:08x} not power of two", f);
        }
    }

    #[test]
    fn test_lock_types_sequential() {
        assert_eq!(OCFS2_LOCK_TYPE_NONE, 0);
        assert_eq!(OCFS2_LOCK_TYPE_META, 1);
        assert_eq!(OCFS2_LOCK_TYPE_QINFO, 9);
    }

    #[test]
    fn test_lock_types_distinct() {
        let types = [
            OCFS2_LOCK_TYPE_NONE, OCFS2_LOCK_TYPE_META,
            OCFS2_LOCK_TYPE_DATA, OCFS2_LOCK_TYPE_SUPER,
            OCFS2_LOCK_TYPE_RENAME, OCFS2_LOCK_TYPE_RW,
            OCFS2_LOCK_TYPE_DENTRY, OCFS2_LOCK_TYPE_OPEN,
            OCFS2_LOCK_TYPE_FLOCK, OCFS2_LOCK_TYPE_QINFO,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_max_slots() {
        assert_eq!(OCFS2_MAX_SLOTS, 255);
    }

    #[test]
    fn test_cluster_name_len() {
        assert_eq!(OCFS2_CLUSTER_NAME_LEN, 16);
    }
}
