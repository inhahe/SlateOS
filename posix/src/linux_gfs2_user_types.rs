//! `<linux/gfs2_ondisk.h>` — Global File System 2 cluster-FS ABI.
//!
//! GFS2 is the cluster filesystem behind Red Hat HA and GlusterFS
//! shared-volume mode. Userspace tools `mkfs.gfs2`, `gfs2_jadd`,
//! `gfs2_grow`, `gfs2_quota`, and `dlm_controld` consume the
//! on-disk magic and metatype constants below.

// ---------------------------------------------------------------------------
// On-disk magic
// ---------------------------------------------------------------------------

/// Superblock magic at start of every GFS2 volume.
pub const GFS2_MAGIC: u32 = 0x01161970;
/// Filesystem statfs magic returned by f_type.
pub const GFS2_STATFS_MAGIC: u32 = 0x1161970;

// ---------------------------------------------------------------------------
// Filesystem geometry
// ---------------------------------------------------------------------------

/// Default block size.
pub const GFS2_DEFAULT_BSIZE: u32 = 4096;
/// Maximum on-disk block size GFS2 supports.
pub const GFS2_MAX_BSIZE: u32 = 4096;
/// Maximum filename length.
pub const GFS2_FNAMESIZE: u32 = 255;
/// Maximum locktable name length.
pub const GFS2_LOCKNAME_LEN: u32 = 64;

// ---------------------------------------------------------------------------
// Metatypes (struct gfs2_meta_header.mh_type)
// ---------------------------------------------------------------------------

/// Unused / sentinel.
pub const GFS2_METATYPE_NONE: u32 = 0;
/// Superblock.
pub const GFS2_METATYPE_SB: u32 = 1;
/// Resource group header.
pub const GFS2_METATYPE_RG: u32 = 2;
/// Bitmap.
pub const GFS2_METATYPE_RB: u32 = 3;
/// Dinode.
pub const GFS2_METATYPE_DI: u32 = 4;
/// Indirect block.
pub const GFS2_METATYPE_IN: u32 = 5;
/// Leaf.
pub const GFS2_METATYPE_LF: u32 = 6;
/// Journal data.
pub const GFS2_METATYPE_JD: u32 = 7;
/// Log header.
pub const GFS2_METATYPE_LH: u32 = 8;
/// Log descriptor.
pub const GFS2_METATYPE_LD: u32 = 9;
/// Log block.
pub const GFS2_METATYPE_LB: u32 = 10;
/// Extended attribute.
pub const GFS2_METATYPE_EA: u32 = 11;
/// Extended attribute data block.
pub const GFS2_METATYPE_ED: u32 = 12;
/// Quota change.
pub const GFS2_METATYPE_QC: u32 = 13;

// ---------------------------------------------------------------------------
// Locking protocols
// ---------------------------------------------------------------------------

/// DLM-based clustered locking.
pub const GFS2_LOCK_DLM: &str = "lock_dlm";
/// Local-only locking (single-node).
pub const GFS2_LOCK_NOLOCK: &str = "lock_nolock";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_magic_unchanged() {
        // 0x01161970 is the "GFS2 birthday" magic in the upstream
        // header — must never change without a forced fsck rev.
        assert_eq!(GFS2_MAGIC, 0x01161970);
        // statfs magic is the lower 28 bits of the same constant.
        assert_eq!(GFS2_STATFS_MAGIC, GFS2_MAGIC & 0x0FFF_FFFF);
    }

    #[test]
    fn test_geometry_constants() {
        assert_eq!(GFS2_DEFAULT_BSIZE, 4096);
        assert!(GFS2_DEFAULT_BSIZE <= GFS2_MAX_BSIZE);
        // POSIX filename length cap.
        assert_eq!(GFS2_FNAMESIZE, 255);
        assert_eq!(GFS2_LOCKNAME_LEN, 64);
    }

    #[test]
    fn test_metatypes_dense_0_to_13() {
        let m = [
            GFS2_METATYPE_NONE,
            GFS2_METATYPE_SB,
            GFS2_METATYPE_RG,
            GFS2_METATYPE_RB,
            GFS2_METATYPE_DI,
            GFS2_METATYPE_IN,
            GFS2_METATYPE_LF,
            GFS2_METATYPE_JD,
            GFS2_METATYPE_LH,
            GFS2_METATYPE_LD,
            GFS2_METATYPE_LB,
            GFS2_METATYPE_EA,
            GFS2_METATYPE_ED,
            GFS2_METATYPE_QC,
        ];
        for (i, &v) in m.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_lock_protocols_distinct() {
        assert_ne!(GFS2_LOCK_DLM, GFS2_LOCK_NOLOCK);
        // Both names must fit in the locktable-name field with NUL.
        assert!(GFS2_LOCK_DLM.len() < GFS2_LOCKNAME_LEN as usize);
        assert!(GFS2_LOCK_NOLOCK.len() < GFS2_LOCKNAME_LEN as usize);
    }
}
