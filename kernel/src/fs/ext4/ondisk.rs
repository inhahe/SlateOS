//! ext4 on-disk structure definitions.
//!
//! These structures match the exact byte layout of ext4 on disk.
//! All fields are little-endian (ext4 is LE-only).  We use `#[repr(C)]`
//! to ensure the compiler doesn't reorder or pad fields.
//!
//! References:
//! - Linux kernel source: `fs/ext4/ext4.h`
//! - ext4 wiki: <https://ext4.wiki.kernel.org/index.php/Ext4_Disk_Layout>
//! - `e2fsprogs` source (mke2fs, dumpe2fs)

// ---------------------------------------------------------------------------
// Superblock
// ---------------------------------------------------------------------------

/// ext4 superblock — the filesystem's master metadata structure.
///
/// Located at byte offset 1024 from the start of the partition.
/// Size: 1024 bytes (though only the first ~400 bytes are commonly used;
/// the rest is reserved or for newer features).
///
/// All multi-byte fields are little-endian.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct Ext4Superblock {
    // --- Original ext2 fields (offsets 0x00 - 0x5B) ---

    /// Total number of inodes in the filesystem.
    pub s_inodes_count: u32,              // 0x00
    /// Total number of blocks (low 32 bits).
    pub s_blocks_count_lo: u32,           // 0x04
    /// Number of blocks reserved for the superuser.
    pub s_r_blocks_count_lo: u32,         // 0x08
    /// Number of unallocated blocks (low 32 bits).
    pub s_free_blocks_count_lo: u32,      // 0x0C
    /// Number of unallocated inodes.
    pub s_free_inodes_count: u32,         // 0x10
    /// Block number of the first data block (block containing the superblock).
    /// For 1 KiB blocks this is 1; for 2 KiB+ blocks this is 0.
    pub s_first_data_block: u32,          // 0x14
    /// Block size = 1024 << s_log_block_size.
    pub s_log_block_size: u32,            // 0x18
    /// Cluster size = 1024 << s_log_cluster_size (usually same as block).
    pub s_log_cluster_size: u32,          // 0x1C
    /// Number of blocks per block group.
    pub s_blocks_per_group: u32,          // 0x20
    /// Number of clusters per block group.
    pub s_clusters_per_group: u32,        // 0x24
    /// Number of inodes per block group.
    pub s_inodes_per_group: u32,          // 0x28
    /// Last mount time (Unix timestamp).
    pub s_mtime: u32,                     // 0x2C
    /// Last write time (Unix timestamp).
    pub s_wtime: u32,                     // 0x30
    /// Mount count since last fsck.
    pub s_mnt_count: u16,                 // 0x34
    /// Maximum mount count before fsck is forced.
    pub s_max_mnt_count: u16,             // 0x36
    /// Magic number: 0xEF53.
    pub s_magic: u16,                     // 0x38
    /// Filesystem state (1=clean, 2=has errors, 4=orphan recovery).
    pub s_state: u16,                     // 0x3A
    /// Error handling behavior (1=continue, 2=remount-ro, 3=panic).
    pub s_errors: u16,                    // 0x3C
    /// Minor revision level.
    pub s_minor_rev_level: u16,           // 0x3E
    /// Last fsck time (Unix timestamp).
    pub s_lastcheck: u32,                 // 0x40
    /// Interval between forced fscks (seconds).
    pub s_checkinterval: u32,             // 0x44
    /// OS that created the filesystem (0=Linux, 1=Hurd, 2=Masix, 3=FreeBSD, 4=Lites).
    pub s_creator_os: u32,                // 0x48
    /// Revision level (0=original, 1=dynamic inode sizes).
    pub s_rev_level: u32,                 // 0x4C
    /// Default UID for reserved blocks.
    pub s_def_resuid: u16,                // 0x50
    /// Default GID for reserved blocks.
    pub s_def_resgid: u16,                // 0x52

    // --- ext4 dynamic revision fields (rev >= 1) ---

    /// First non-reserved inode (usually 11 in ext4).
    pub s_first_ino: u32,                 // 0x54
    /// Inode size in bytes (typically 128 for ext2, 256 for ext4).
    pub s_inode_size: u16,                // 0x58
    /// Block group number containing this superblock (for backups).
    pub s_block_group_nr: u16,            // 0x5A
    /// Compatible feature flags.
    pub s_feature_compat: u32,            // 0x5C
    /// Incompatible feature flags (must understand all set bits to mount).
    pub s_feature_incompat: u32,          // 0x60
    /// Read-only compatible feature flags.
    pub s_feature_ro_compat: u32,         // 0x64
    /// 128-bit filesystem UUID.
    pub s_uuid: [u8; 16],                 // 0x68
    /// Volume name (null-terminated).
    pub s_volume_name: [u8; 16],          // 0x78
    /// Directory where last mounted (null-terminated).
    pub s_last_mounted: [u8; 64],         // 0x88
    /// Compression algorithm bitmap.
    pub s_algorithm_usage_bitmap: u32,    // 0xC8

    // --- Performance hints ---

    /// Number of blocks to pre-allocate for files.
    pub s_prealloc_blocks: u8,            // 0xCC
    /// Number of blocks to pre-allocate for directories.
    pub s_prealloc_dir_blocks: u8,        // 0xCD
    /// Number of reserved GDT entries for future expansion.
    pub s_reserved_gdt_blocks: u16,       // 0xCE

    // --- Journaling (ext3/ext4) ---

    /// Journal superblock UUID.
    pub s_journal_uuid: [u8; 16],         // 0xD0
    /// Journal inode number.
    pub s_journal_inum: u32,              // 0xE0
    /// Journal device number (0 = internal journal).
    pub s_journal_dev: u32,               // 0xE4
    /// Head of orphan inode list.
    pub s_last_orphan: u32,               // 0xE8
    /// HTREE hash seed.
    pub s_hash_seed: [u32; 4],            // 0xEC
    /// Default hash algorithm for directories (0=legacy, 1=half_md4, 2=tea, 3=legacy_unsigned, 4=half_md4_unsigned, 5=tea_unsigned).
    pub s_def_hash_version: u8,           // 0xFC
    /// Journal backup type.
    pub s_jnl_backup_type: u8,            // 0xFD
    /// Group descriptor size (32 or 64 bytes).
    pub s_desc_size: u16,                 // 0xFE
    /// Default mount options.
    pub s_default_mount_opts: u32,        // 0x100
    /// First metablock block group.
    pub s_first_meta_bg: u32,             // 0x104
    /// Filesystem creation time (Unix timestamp).
    pub s_mkfs_time: u32,                 // 0x108
    /// Journal inode backup (17 u32s).
    pub s_jnl_blocks: [u32; 17],          // 0x10C

    // --- 64-bit support (if INCOMPAT_64BIT is set) ---

    /// Total number of blocks (high 32 bits).
    pub s_blocks_count_hi: u32,           // 0x150
    /// Reserved block count (high 32 bits).
    pub s_r_blocks_count_hi: u32,         // 0x154
    /// Free block count (high 32 bits).
    pub s_free_blocks_count_hi: u32,      // 0x158
    /// Minimum inode size for new files.
    pub s_min_extra_isize: u16,           // 0x15C
    /// Desired inode size (for new inodes).
    pub s_want_extra_isize: u16,          // 0x15E
    /// Miscellaneous flags.
    pub s_flags: u32,                     // 0x160
    /// RAID stride (blocks).
    pub s_raid_stride: u16,               // 0x164
    /// MMP check interval (seconds).
    pub s_mmp_interval: u16,              // 0x166
    /// MMP block number.
    pub s_mmp_block: u64,                 // 0x168
    /// RAID stripe width (blocks).
    pub s_raid_stripe_width: u32,         // 0x170
    /// log2(groups_per_flex) for flex block groups.
    pub s_log_groups_per_flex: u8,        // 0x174
    /// Metadata checksum algorithm type (1=crc32c).
    pub s_checksum_type: u8,              // 0x175
    /// Padding.
    pub s_reserved_pad: u16,              // 0x176
    /// Total KiB written to the filesystem.
    pub s_kbytes_written: u64,            // 0x178
    /// Inode number of active snapshot.
    pub s_snapshot_inum: u32,             // 0x180
    /// Sequential ID of active snapshot.
    pub s_snapshot_id: u32,               // 0x184
    /// Reserved blocks for active snapshot's future use.
    pub s_snapshot_r_blocks_count: u64,   // 0x188
    /// Inode number of head of snapshot list.
    pub s_snapshot_list: u32,             // 0x190
    /// Total number of filesystem errors.
    pub s_error_count: u32,               // 0x194
    /// Time of first error (Unix timestamp).
    pub s_first_error_time: u32,          // 0x198
    /// Inode involved in first error.
    pub s_first_error_ino: u32,           // 0x19C
    /// Block involved in first error.
    pub s_first_error_block: u64,         // 0x1A0
    /// Function name where first error happened (32 bytes).
    pub s_first_error_func: [u8; 32],     // 0x1A8
    /// Line number where first error happened.
    pub s_first_error_line: u32,          // 0x1C8
    /// Time of most recent error.
    pub s_last_error_time: u32,           // 0x1CC
    /// Inode involved in most recent error.
    pub s_last_error_ino: u32,            // 0x1D0
    /// Line number where most recent error happened.
    pub s_last_error_line: u32,           // 0x1D4
    /// Block involved in most recent error.
    pub s_last_error_block: u64,          // 0x1D8
    /// Function name where most recent error happened (32 bytes).
    pub s_last_error_func: [u8; 32],      // 0x1E0
    /// Mount options string (64 bytes, null-terminated).
    pub s_mount_opts: [u8; 64],           // 0x200
    /// Inode number of user quota file.
    pub s_usr_quota_inum: u32,            // 0x240
    /// Inode number of group quota file.
    pub s_grp_quota_inum: u32,            // 0x244
    /// Overhead blocks/clusters (not in any block group).
    pub s_overhead_blocks: u32,           // 0x248
    /// Block groups with SPARSE_SUPER2 superblock backups.
    pub s_backup_bgs: [u32; 2],           // 0x24C
    /// Encryption algorithms in use.
    pub s_encrypt_algos: [u8; 4],         // 0x254
    /// Salt for string2key algorithm for encryption.
    pub s_encrypt_pw_salt: [u8; 16],      // 0x258
    /// Inode of lost+found directory.
    pub s_lpf_ino: u32,                   // 0x268
    /// Inode for tracking project quotas.
    pub s_prj_quota_inum: u32,            // 0x26C
    /// Checksum seed (crc32c(~0, s_uuid) if INCOMPAT_CSUM_SEED).
    pub s_checksum_seed: u32,             // 0x270
    /// Upper 8 bits of s_wtime.
    pub s_wtime_hi: u8,                   // 0x274
    /// Upper 8 bits of s_mtime.
    pub s_mtime_hi: u8,                   // 0x275
    /// Upper 8 bits of s_mkfs_time.
    pub s_mkfs_time_hi: u8,              // 0x276
    /// Upper 8 bits of s_lastcheck.
    pub s_lastcheck_hi: u8,              // 0x277
    /// Upper 8 bits of s_first_error_time.
    pub s_first_error_time_hi: u8,       // 0x278
    /// Upper 8 bits of s_last_error_time.
    pub s_last_error_time_hi: u8,        // 0x279
    /// Padding.
    pub s_pad: [u8; 2],                  // 0x27A
    /// Filename charset encoding (e.g., UTF-8).
    pub s_encoding: u16,                 // 0x27C
    /// Filename charset encoding flags.
    pub s_encoding_flags: u16,           // 0x27E
    /// Orphan file inode number.
    pub s_orphan_file_inum: u32,         // 0x280
    /// Reserved for future expansion.
    pub s_reserved: [u32; 94],           // 0x284
    /// Superblock checksum (crc32c).
    pub s_checksum: u32,                 // 0x3FC
}

// Compile-time size check: superblock must be exactly 1024 bytes.
const _: () = assert!(
    core::mem::size_of::<Ext4Superblock>() == 1024,
    "Ext4Superblock size must be exactly 1024 bytes"
);

/// ext4 magic number (0xEF53, little-endian).
pub const EXT4_MAGIC: u16 = 0xEF53;

/// Byte offset of the superblock from the start of the partition.
pub const SUPERBLOCK_OFFSET: u64 = 1024;

// ---------------------------------------------------------------------------
// Feature flags
// ---------------------------------------------------------------------------

/// Compatible feature flags (`s_feature_compat`).
///
/// The filesystem can be mounted read-write even if unknown compat bits
/// are set — they're informational.
#[allow(dead_code)]
pub mod compat {
    /// Directory preallocation.
    pub const DIR_PREALLOC: u32     = 0x0001;
    /// "imagic" inodes (AFS server).
    pub const IMAGIC_INODES: u32    = 0x0002;
    /// Has a journal (ext3/ext4).
    pub const HAS_JOURNAL: u32      = 0x0004;
    /// Extended attributes.
    pub const EXT_ATTR: u32         = 0x0008;
    /// Filesystem can resize itself.
    pub const RESIZE_INODE: u32     = 0x0010;
    /// Directory indexing (htree).
    pub const DIR_INDEX: u32        = 0x0020;
    /// Lazy block group init.
    pub const LAZY_BG: u32          = 0x0040;
    /// Exclude inode for snapshots.
    pub const EXCLUDE_INODE: u32    = 0x0080;
    /// Exclude bitmap for snapshots.
    pub const EXCLUDE_BITMAP: u32   = 0x0100;
    /// Sparse super2.
    pub const SPARSE_SUPER2: u32    = 0x0200;
    /// Fast commits.
    pub const FAST_COMMIT: u32      = 0x0400;
    /// Stable inodes (inode numbers don't change with defrag).
    pub const STABLE_INODES: u32    = 0x0800;
    /// Orphan file.
    pub const ORPHAN_FILE: u32      = 0x1000;
}

/// Incompatible feature flags (`s_feature_incompat`).
///
/// If any unknown incompat bit is set, the filesystem MUST NOT be mounted.
/// We need to understand all set bits.
#[allow(dead_code)]
pub mod incompat {
    /// Uses compression.
    pub const COMPRESSION: u32      = 0x0001;
    /// Directory entries contain file type.
    pub const FILETYPE: u32         = 0x0002;
    /// Filesystem needs recovery (journal replay).
    pub const RECOVER: u32          = 0x0004;
    /// Separate journal device.
    pub const JOURNAL_DEV: u32      = 0x0008;
    /// Meta block groups.
    pub const META_BG: u32          = 0x0010;
    /// Extents (ext4 extent tree instead of indirect blocks).
    pub const EXTENTS: u32          = 0x0040;
    /// 64-bit block numbers.
    pub const BIT64: u32            = 0x0080;
    /// Multiple mount protection.
    pub const MMP: u32              = 0x0100;
    /// Flexible block groups.
    pub const FLEX_BG: u32          = 0x0200;
    /// Extended attribute inodes (large xattrs).
    pub const EA_INODE: u32         = 0x0400;
    /// Data in directory entries.
    pub const DIRDATA: u32          = 0x1000;
    /// Metadata checksum seed in superblock.
    pub const CSUM_SEED: u32        = 0x2000;
    /// Large directory (>2GB, 3-level htree).
    pub const LARGEDIR: u32         = 0x4000;
    /// Data in inode.
    pub const INLINE_DATA: u32      = 0x8000;
    /// Encrypted inodes.
    pub const ENCRYPT: u32          = 0x10000;
    /// Casefolded directories.
    pub const CASEFOLD: u32         = 0x20000;
}

/// Read-only compatible feature flags (`s_feature_ro_compat`).
///
/// If unknown ro_compat bits are set, the filesystem can be mounted
/// read-only but NOT read-write.
#[allow(dead_code)]
pub mod ro_compat {
    /// Sparse superblocks.
    pub const SPARSE_SUPER: u32     = 0x0001;
    /// Filesystem contains large files (>2 GiB).
    pub const LARGE_FILE: u32       = 0x0002;
    /// Btree directories (never used in ext4).
    pub const BTREE_DIR: u32        = 0x0004;
    /// Huge files (uses units of logical blocks, not 512-byte sectors).
    pub const HUGE_FILE: u32        = 0x0008;
    /// Group descriptor checksum.
    pub const GDT_CSUM: u32         = 0x0010;
    /// Large subdirectory count (>65000).
    pub const DIR_NLINK: u32        = 0x0020;
    /// Large inodes (>128 bytes extra).
    pub const EXTRA_ISIZE: u32      = 0x0040;
    /// Snapshots.
    pub const HAS_SNAPSHOT: u32     = 0x0080;
    /// Quota.
    pub const QUOTA: u32            = 0x0100;
    /// Big alloc (clusters instead of blocks).
    pub const BIGALLOC: u32         = 0x0200;
    /// Metadata checksumming (crc32c).
    pub const METADATA_CSUM: u32    = 0x0400;
    /// Read-only replicas.
    pub const REPLICA: u32          = 0x0800;
    /// Read-only filesystem image.
    pub const READONLY: u32         = 0x1000;
    /// Track project quotas.
    pub const PROJECT: u32          = 0x2000;
    /// Verity inodes.
    pub const VERITY: u32           = 0x8000;
    /// Orphan file present.
    pub const ORPHAN_PRESENT: u32   = 0x10000;
}

/// Incompatible features that our implementation currently supports.
///
/// We MUST refuse to mount any filesystem with incompat bits set that
/// are not in this mask.  This prevents accidental data corruption from
/// not understanding a required feature.
pub const SUPPORTED_INCOMPAT: u32 =
    incompat::FILETYPE
    | incompat::EXTENTS
    | incompat::BIT64
    | incompat::FLEX_BG
    | incompat::RECOVER; // We handle RECOVER by requiring journal replay.

/// Read-only compat features we support for read-write mounting.
///
/// If unknown ro_compat bits are set, we can still mount read-only.
pub const SUPPORTED_RO_COMPAT: u32 =
    ro_compat::SPARSE_SUPER
    | ro_compat::LARGE_FILE
    | ro_compat::HUGE_FILE
    | ro_compat::GDT_CSUM
    | ro_compat::DIR_NLINK
    | ro_compat::EXTRA_ISIZE
    | ro_compat::METADATA_CSUM;

// ---------------------------------------------------------------------------
// Block Group Descriptor
// ---------------------------------------------------------------------------

/// Block Group Descriptor (32-byte version).
///
/// Every block group has one of these in the block group descriptor table
/// (located in the block immediately after the superblock).
///
/// If `INCOMPAT_64BIT` is set AND `s_desc_size >= 64`, the descriptor
/// is 64 bytes and uses the `_hi` fields.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct Ext4GroupDesc {
    /// Block bitmap block number (low 32 bits).
    pub bg_block_bitmap_lo: u32,          // 0x00
    /// Inode bitmap block number (low 32 bits).
    pub bg_inode_bitmap_lo: u32,          // 0x04
    /// Inode table start block (low 32 bits).
    pub bg_inode_table_lo: u32,           // 0x08
    /// Free block count (low 16 bits).
    pub bg_free_blocks_count_lo: u16,     // 0x0C
    /// Free inode count (low 16 bits).
    pub bg_free_inodes_count_lo: u16,     // 0x0E
    /// Directory count (low 16 bits).
    pub bg_used_dirs_count_lo: u16,       // 0x10
    /// Block group flags.
    pub bg_flags: u16,                    // 0x12
    /// Exclude bitmap block (low 32 bits) (snapshots).
    pub bg_exclude_bitmap_lo: u32,        // 0x14
    /// Block bitmap checksum (low 16 bits) (crc32c).
    pub bg_block_bitmap_csum_lo: u16,     // 0x18
    /// Inode bitmap checksum (low 16 bits) (crc32c).
    pub bg_inode_bitmap_csum_lo: u16,     // 0x1A
    /// Unused inode count (low 16 bits).
    pub bg_itable_unused_lo: u16,         // 0x1C
    /// Group descriptor checksum (crc16 or crc32c low16).
    pub bg_checksum: u16,                 // 0x1E

    // --- 64-bit extensions (only if INCOMPAT_64BIT && s_desc_size >= 64) ---

    /// Block bitmap block (high 32 bits).
    pub bg_block_bitmap_hi: u32,          // 0x20
    /// Inode bitmap block (high 32 bits).
    pub bg_inode_bitmap_hi: u32,          // 0x24
    /// Inode table start block (high 32 bits).
    pub bg_inode_table_hi: u32,           // 0x28
    /// Free block count (high 16 bits).
    pub bg_free_blocks_count_hi: u16,     // 0x2C
    /// Free inode count (high 16 bits).
    pub bg_free_inodes_count_hi: u16,     // 0x2E
    /// Directory count (high 16 bits).
    pub bg_used_dirs_count_hi: u16,       // 0x30
    /// Unused inode count (high 16 bits).
    pub bg_itable_unused_hi: u16,         // 0x32
    /// Exclude bitmap block (high 32 bits).
    pub bg_exclude_bitmap_hi: u32,        // 0x34
    /// Block bitmap checksum (high 16 bits).
    pub bg_block_bitmap_csum_hi: u16,     // 0x38
    /// Inode bitmap checksum (high 16 bits).
    pub bg_inode_bitmap_csum_hi: u16,     // 0x3A
    /// Reserved.
    pub bg_reserved: u32,                 // 0x3C
}

const _: () = assert!(
    core::mem::size_of::<Ext4GroupDesc>() == 64,
    "Ext4GroupDesc size must be exactly 64 bytes"
);

// ---------------------------------------------------------------------------
// Inode
// ---------------------------------------------------------------------------

/// ext4 inode — per-file metadata.
///
/// The core inode is 128 bytes (ext2 original).  ext4 uses 256 bytes
/// by default (128 core + 128 extra).  The extra space stores extended
/// timestamps, extra isize, and checksum.
///
/// This struct represents the core 128-byte inode.  The extra fields
/// are accessed by reading beyond this struct's size.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct Ext4Inode {
    /// File mode (permissions + type: S_IFREG, S_IFDIR, etc.).
    pub i_mode: u16,                      // 0x00
    /// Owner UID (low 16 bits).
    pub i_uid: u16,                       // 0x02
    /// File size in bytes (low 32 bits).
    pub i_size_lo: u32,                   // 0x04
    /// Last access time (Unix timestamp, low 32 bits).
    pub i_atime: u32,                     // 0x08
    /// Inode change time (Unix timestamp, low 32 bits).
    pub i_ctime: u32,                     // 0x0C
    /// Last modification time (Unix timestamp, low 32 bits).
    pub i_mtime: u32,                     // 0x10
    /// Deletion time (Unix timestamp).
    pub i_dtime: u32,                     // 0x14
    /// Group GID (low 16 bits).
    pub i_gid: u16,                       // 0x18
    /// Hard link count.
    pub i_links_count: u16,              // 0x1A
    /// Block count (in 512-byte sectors, low 32 bits).
    pub i_blocks_lo: u32,                // 0x1C
    /// Inode flags.
    pub i_flags: u32,                    // 0x20
    /// OS-dependent value 1 (Linux: version).
    pub i_osd1: u32,                     // 0x24
    /// Block map or extent tree (60 bytes).
    ///
    /// If `EXTENTS` flag is set on the inode, this contains an extent
    /// tree header followed by extent entries.  Otherwise, it contains
    /// 12 direct + 1 indirect + 1 double-indirect + 1 triple-indirect
    /// block pointers (15 * 4 = 60 bytes).
    pub i_block: [u32; 15],             // 0x28
    /// File version (for NFS).
    pub i_generation: u32,              // 0x64
    /// File ACL (extended attribute block, low 32 bits).
    pub i_file_acl_lo: u32,            // 0x68
    /// File size (high 32 bits) / directory ACL.
    pub i_size_high: u32,              // 0x6C
    /// Fragment address (obsolete).
    pub i_obso_faddr: u32,             // 0x70
    /// OS-dependent value 2 (12 bytes).
    /// Linux: i_blocks_high(u16), i_file_acl_high(u16),
    ///        i_uid_high(u16), i_gid_high(u16), i_checksum_lo(u16), reserved(u16).
    pub i_osd2: [u8; 12],              // 0x74
}

const _: () = assert!(
    core::mem::size_of::<Ext4Inode>() == 128,
    "Ext4Inode core size must be exactly 128 bytes"
);

/// Extra inode fields (immediately after the 128-byte core).
///
/// These are present when `s_inode_size > 128`.  The `i_extra_isize`
/// field tells how many bytes of this extended area are valid.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct Ext4InodeExtra {
    /// Size of extra inode fields (bytes after the 128-byte core).
    pub i_extra_isize: u16,             // 0x80
    /// Inode checksum (high 16 bits).
    pub i_checksum_hi: u16,            // 0x82
    /// Extra change time (nanoseconds + epoch bits).
    pub i_ctime_extra: u32,            // 0x84
    /// Extra modification time (nanoseconds + epoch bits).
    pub i_mtime_extra: u32,            // 0x88
    /// Extra access time (nanoseconds + epoch bits).
    pub i_atime_extra: u32,            // 0x8C
    /// Creation time (Unix timestamp).
    pub i_crtime: u32,                 // 0x90
    /// Creation time (nanoseconds + epoch bits).
    pub i_crtime_extra: u32,           // 0x94
    /// Version (high 32 bits).
    pub i_version_hi: u32,            // 0x98
    /// Project ID.
    pub i_projid: u32,                // 0x9C
}

const _: () = assert!(
    core::mem::size_of::<Ext4InodeExtra>() == 32,
    "Ext4InodeExtra size must be exactly 32 bytes"
);

// ---------------------------------------------------------------------------
// Inode flags
// ---------------------------------------------------------------------------

/// Inode flags (`i_flags`).
#[allow(dead_code)]
pub mod inode_flags {
    /// Secure deletion (not implemented).
    pub const SECRM: u32        = 0x0000_0001;
    /// Undelete (not implemented).
    pub const UNRM: u32         = 0x0000_0002;
    /// Compressed file.
    pub const COMPR: u32        = 0x0000_0004;
    /// Synchronous updates.
    pub const SYNC: u32         = 0x0000_0008;
    /// Immutable file.
    pub const IMMUTABLE: u32    = 0x0000_0010;
    /// Append only.
    pub const APPEND: u32       = 0x0000_0020;
    /// Do not dump.
    pub const NODUMP: u32       = 0x0000_0040;
    /// Do not update atime.
    pub const NOATIME: u32      = 0x0000_0080;
    /// Uses extents (not indirect blocks).
    pub const EXTENTS: u32      = 0x0008_0000;
    /// Inode stores a large extended attribute.
    pub const EA_INODE: u32     = 0x0020_0000;
    /// Encrypted.
    pub const ENCRYPT: u32      = 0x0000_0800;
    /// Directory with hash-indexed entries.
    pub const INDEX: u32        = 0x0000_1000;
    /// Inode uses inline data.
    pub const INLINE_DATA: u32  = 0x1000_0000;
    /// Casefolded directory.
    pub const CASEFOLD: u32     = 0x4000_0000;
}

// ---------------------------------------------------------------------------
// Inode mode (file type portion)
// ---------------------------------------------------------------------------

/// File type constants from `i_mode`.
#[allow(dead_code)]
pub mod file_type {
    /// Socket.
    pub const S_IFSOCK: u16 = 0xC000;
    /// Symbolic link.
    pub const S_IFLNK: u16  = 0xA000;
    /// Regular file.
    pub const S_IFREG: u16  = 0x8000;
    /// Block device.
    pub const S_IFBLK: u16  = 0x6000;
    /// Directory.
    pub const S_IFDIR: u16  = 0x4000;
    /// Character device.
    pub const S_IFCHR: u16  = 0x2000;
    /// FIFO (named pipe).
    pub const S_IFIFO: u16  = 0x1000;
    /// Mask for file type bits.
    pub const S_IFMT: u16   = 0xF000;
}

// ---------------------------------------------------------------------------
// Extent tree
// ---------------------------------------------------------------------------

/// Extent tree header.
///
/// The first 12 bytes of the `i_block` area when the inode uses extents.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct Ext4ExtentHeader {
    /// Magic number (0xF30A).
    pub eh_magic: u16,
    /// Number of valid entries following the header.
    pub eh_entries: u16,
    /// Maximum number of entries that could follow.
    pub eh_max: u16,
    /// Depth of this node (0 = leaf, >0 = internal).
    pub eh_depth: u16,
    /// Generation (for snapshots).
    pub eh_generation: u32,
}

const _: () = assert!(core::mem::size_of::<Ext4ExtentHeader>() == 12);

/// Extent tree magic number.
pub const EXT4_EXTENT_MAGIC: u16 = 0xF30A;

/// Extent tree leaf entry — maps a range of logical blocks to physical.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct Ext4Extent {
    /// First logical block number this extent covers.
    pub ee_block: u32,
    /// Number of blocks covered by this extent (max 32768).
    pub ee_len: u16,
    /// Physical block number (high 16 bits).
    pub ee_start_hi: u16,
    /// Physical block number (low 32 bits).
    pub ee_start_lo: u32,
}

const _: () = assert!(core::mem::size_of::<Ext4Extent>() == 12);

/// Extent tree internal node entry — points to a child node.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct Ext4ExtentIdx {
    /// Logical block number that this child node covers.
    pub ei_block: u32,
    /// Physical block of the child node (low 32 bits).
    pub ei_leaf_lo: u32,
    /// Physical block of the child node (high 16 bits).
    pub ei_leaf_hi: u16,
    /// Unused.
    pub ei_unused: u16,
}

const _: () = assert!(core::mem::size_of::<Ext4ExtentIdx>() == 12);

// ---------------------------------------------------------------------------
// Directory entry
// ---------------------------------------------------------------------------

/// ext4 directory entry (linear format).
///
/// Variable-length structure.  The `rec_len` field gives the total
/// size of this entry including padding (always a multiple of 4).
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct Ext4DirEntry2 {
    /// Inode number (0 = deleted entry).
    pub inode: u32,
    /// Total size of this entry (including name and padding).
    pub rec_len: u16,
    /// Name length (bytes).
    pub name_len: u8,
    /// File type (if INCOMPAT_FILETYPE is set).
    pub file_type: u8,
    // Followed by `name_len` bytes of filename (NOT null-terminated).
    // Padding follows to align to `rec_len`.
}

const _: () = assert!(core::mem::size_of::<Ext4DirEntry2>() == 8);

/// Directory entry file type codes (`file_type` field).
#[allow(dead_code)]
pub mod dir_type {
    /// Unknown.
    pub const UNKNOWN: u8  = 0;
    /// Regular file.
    pub const REG_FILE: u8 = 1;
    /// Directory.
    pub const DIR: u8      = 2;
    /// Character device.
    pub const CHRDEV: u8   = 3;
    /// Block device.
    pub const BLKDEV: u8   = 4;
    /// FIFO.
    pub const FIFO: u8     = 5;
    /// Socket.
    pub const SOCK: u8     = 6;
    /// Symbolic link.
    pub const SYMLINK: u8  = 7;
}

// ---------------------------------------------------------------------------
// Well-known inode numbers
// ---------------------------------------------------------------------------

/// Root directory inode (always 2 in ext2/3/4).
pub const EXT4_ROOT_INO: u32 = 2;

/// Journal inode (inode 8).
pub const EXT4_JOURNAL_INO: u32 = 8;

/// First non-reserved inode in standard ext4 (usually 11).
pub const EXT4_FIRST_INO: u32 = 11;

/// Lost+found directory inode (inode 11, typically).
pub const EXT4_LOST_FOUND_INO: u32 = 11;
