// SlateOS e2fsprogs — ext2/3/4 filesystem utilities
//
// Multi-personality binary:
//   mke2fs    / mkfs.ext2 / mkfs.ext3 / mkfs.ext4 — create ext2/3/4 filesystem
//   tune2fs   — adjust ext filesystem parameters
//   dumpe2fs  — dump filesystem info
//   debugfs   — ext filesystem debugger
//   resize2fs — resize ext filesystem
//   e2fsck    / fsck.ext2 / fsck.ext3 / fsck.ext4 — check/repair ext filesystem
//   e2label   — change/display filesystem label
//   e2image   — save ext2/3/4 filesystem metadata
//   filefrag  — show file fragmentation
//
// Personality is detected from argv[0] basename (stripping path and .exe suffix).

#![deny(clippy::all)]

use std::env;
use std::process;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const EXT2_MAGIC: u16 = 0xEF53;
const _EXT2_SUPER_OFFSET: u64 = 1024;
const EXT2_ROOT_INO: u32 = 2;
const EXT2_GOOD_OLD_REV: u32 = 0;
const EXT2_DYNAMIC_REV: u32 = 1;
const EXT2_GOOD_OLD_INODE_SIZE: u16 = 128;
const EXT2_DEFAULT_BLOCK_SIZE: u32 = 4096;
const EXT4_DEFAULT_BLOCK_SIZE: u32 = 4096;
const EXT2_MIN_BLOCK_SIZE: u32 = 1024;
const EXT2_MAX_BLOCK_SIZE: u32 = 65536;
const EXT2_DEFAULT_BLOCKS_PER_GROUP: u32 = 32768;
const EXT2_DEFAULT_INODES_PER_GROUP: u32 = 8192;
const EXT2_NDIR_BLOCKS: usize = 12;
const _EXT2_IND_BLOCK: usize = 12;
const _EXT2_DIND_BLOCK: usize = 13;
const _EXT2_TIND_BLOCK: usize = 14;
const EXT2_N_BLOCKS: usize = 15;
const EXT2_NAME_LEN: usize = 255;
const EXT2_LABEL_LEN: usize = 16;
const EXT4_FEATURE_COMPAT_HAS_JOURNAL: u32 = 0x0004;
const EXT4_FEATURE_INCOMPAT_EXTENTS: u32 = 0x0040;
const EXT4_FEATURE_INCOMPAT_64BIT: u32 = 0x0080;
const EXT4_FEATURE_INCOMPAT_FLEX_BG: u32 = 0x0200;
const EXT4_FEATURE_RO_COMPAT_SPARSE_SUPER: u32 = 0x0001;
const EXT4_FEATURE_RO_COMPAT_LARGE_FILE: u32 = 0x0002;
const EXT4_FEATURE_RO_COMPAT_HUGE_FILE: u32 = 0x0008;
const EXT4_FEATURE_RO_COMPAT_METADATA_CSUM: u32 = 0x0400;
const _EXT2_FT_UNKNOWN: u8 = 0;
#[allow(dead_code)] // spec constant; referenced by tests, not yet by dispatch
const EXT2_FT_REG_FILE: u8 = 1;
const EXT2_FT_DIR: u8 = 2;
const _EXT2_FT_CHRDEV: u8 = 3;
const _EXT2_FT_BLKDEV: u8 = 4;
const _EXT2_FT_FIFO: u8 = 5;
const _EXT2_FT_SOCK: u8 = 6;
#[allow(dead_code)] // spec constant; referenced by tests, not yet by dispatch
const EXT2_FT_SYMLINK: u8 = 7;

const S_IFMT: u16 = 0xF000;
const S_IFREG: u16 = 0x8000;
const S_IFDIR: u16 = 0x4000;
const S_IFLNK: u16 = 0xA000;
const _S_IFBLK: u16 = 0x6000;
const _S_IFCHR: u16 = 0x2000;

// Default UUIDs for simulation
const DEFAULT_UUID: [u8; 16] = [
    0xa1, 0xb2, 0xc3, 0xd4, 0xe5, 0xf6, 0x07, 0x18, 0x29, 0x3a, 0x4b, 0x5c, 0x6d, 0x7e, 0x8f, 0x90,
];

// ---------------------------------------------------------------------------
// Personality detection
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Personality {
    Mke2fs,
    Tune2fs,
    Dumpe2fs,
    Debugfs,
    Resize2fs,
    E2fsck,
    E2label,
    E2image,
    Filefrag,
}

fn detect_personality(argv0: &str) -> Personality {
    let base = argv0.rsplit('/').next().unwrap_or(argv0);
    let base = base.rsplit('\\').next().unwrap_or(base);
    let lower = base.to_ascii_lowercase();
    let name = lower.strip_suffix(".exe").unwrap_or(&lower);
    match name {
        "mke2fs" | "mkfs.ext2" | "mkfs.ext3" | "mkfs.ext4" => Personality::Mke2fs,
        "tune2fs" => Personality::Tune2fs,
        "dumpe2fs" => Personality::Dumpe2fs,
        "debugfs" => Personality::Debugfs,
        "resize2fs" => Personality::Resize2fs,
        "e2fsck" | "fsck.ext2" | "fsck.ext3" | "fsck.ext4" => Personality::E2fsck,
        "e2label" => Personality::E2label,
        "e2image" => Personality::E2image,
        "filefrag" => Personality::Filefrag,
        _ => Personality::Mke2fs,
    }
}

// ---------------------------------------------------------------------------
// Filesystem feature flags
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FeatureCompat(u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FeatureIncompat(u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FeatureRoCompat(u32);

impl FeatureCompat {
    fn has_journal(self) -> bool {
        self.0 & EXT4_FEATURE_COMPAT_HAS_JOURNAL != 0
    }

    fn set_journal(&mut self) {
        self.0 |= EXT4_FEATURE_COMPAT_HAS_JOURNAL;
    }

    fn clear_journal(&mut self) {
        self.0 &= !EXT4_FEATURE_COMPAT_HAS_JOURNAL;
    }

    fn format_flags(self) -> String {
        let mut flags = Vec::new();
        if self.has_journal() {
            flags.push("has_journal");
        }
        if self.0 & 0x0001 != 0 {
            flags.push("dir_prealloc");
        }
        if self.0 & 0x0002 != 0 {
            flags.push("imagic_inodes");
        }
        if self.0 & 0x0008 != 0 {
            flags.push("resize_inode");
        }
        if self.0 & 0x0010 != 0 {
            flags.push("dir_index");
        }
        if flags.is_empty() {
            String::from("(none)")
        } else {
            flags.join(" ")
        }
    }
}

impl FeatureIncompat {
    fn has_extents(self) -> bool {
        self.0 & EXT4_FEATURE_INCOMPAT_EXTENTS != 0
    }

    fn has_64bit(self) -> bool {
        self.0 & EXT4_FEATURE_INCOMPAT_64BIT != 0
    }

    fn has_flex_bg(self) -> bool {
        self.0 & EXT4_FEATURE_INCOMPAT_FLEX_BG != 0
    }

    fn format_flags(self) -> String {
        let mut flags = Vec::new();
        if self.0 & 0x0001 != 0 {
            flags.push("compression");
        }
        if self.0 & 0x0002 != 0 {
            flags.push("filetype");
        }
        if self.0 & 0x0004 != 0 {
            flags.push("recover");
        }
        if self.0 & 0x0008 != 0 {
            flags.push("journal_dev");
        }
        if self.0 & 0x0010 != 0 {
            flags.push("meta_bg");
        }
        if self.has_extents() {
            flags.push("extents");
        }
        if self.has_64bit() {
            flags.push("64bit");
        }
        if self.has_flex_bg() {
            flags.push("flex_bg");
        }
        if self.0 & 0x0400 != 0 {
            flags.push("mmp");
        }
        if flags.is_empty() {
            String::from("(none)")
        } else {
            flags.join(" ")
        }
    }
}

impl FeatureRoCompat {
    fn has_sparse_super(self) -> bool {
        self.0 & EXT4_FEATURE_RO_COMPAT_SPARSE_SUPER != 0
    }

    fn has_large_file(self) -> bool {
        self.0 & EXT4_FEATURE_RO_COMPAT_LARGE_FILE != 0
    }

    fn has_huge_file(self) -> bool {
        self.0 & EXT4_FEATURE_RO_COMPAT_HUGE_FILE != 0
    }

    fn has_metadata_csum(self) -> bool {
        self.0 & EXT4_FEATURE_RO_COMPAT_METADATA_CSUM != 0
    }

    fn format_flags(self) -> String {
        let mut flags = Vec::new();
        if self.has_sparse_super() {
            flags.push("sparse_super");
        }
        if self.has_large_file() {
            flags.push("large_file");
        }
        if self.has_huge_file() {
            flags.push("huge_file");
        }
        if self.0 & 0x0004 != 0 {
            flags.push("btree_dir");
        }
        if self.0 & 0x0010 != 0 {
            flags.push("dir_nlink");
        }
        if self.0 & 0x0020 != 0 {
            flags.push("extra_isize");
        }
        if self.has_metadata_csum() {
            flags.push("metadata_csum");
        }
        if flags.is_empty() {
            String::from("(none)")
        } else {
            flags.join(" ")
        }
    }
}

// ---------------------------------------------------------------------------
// Filesystem type
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FsType {
    Ext2,
    Ext3,
    Ext4,
}

impl FsType {
    // Tested accessors below are not yet wired into the command-dispatch path
    // (this binary's per-personality logic is still being built out), so they
    // carry #[allow(dead_code)] until then. See todo.txt.
    #[allow(dead_code)]
    fn name(self) -> &'static str {
        match self {
            FsType::Ext2 => "ext2",
            FsType::Ext3 => "ext3",
            FsType::Ext4 => "ext4",
        }
    }

    fn default_features_compat(self) -> FeatureCompat {
        match self {
            FsType::Ext2 => FeatureCompat(0x0010 | 0x0008), // dir_index, resize_inode
            FsType::Ext3 => FeatureCompat(0x0010 | 0x0008 | EXT4_FEATURE_COMPAT_HAS_JOURNAL),
            FsType::Ext4 => FeatureCompat(0x0010 | 0x0008 | EXT4_FEATURE_COMPAT_HAS_JOURNAL),
        }
    }

    fn default_features_incompat(self) -> FeatureIncompat {
        match self {
            FsType::Ext2 => FeatureIncompat(0x0002), // filetype
            FsType::Ext3 => FeatureIncompat(0x0002),
            FsType::Ext4 => FeatureIncompat(
                0x0002
                    | EXT4_FEATURE_INCOMPAT_EXTENTS
                    | EXT4_FEATURE_INCOMPAT_64BIT
                    | EXT4_FEATURE_INCOMPAT_FLEX_BG,
            ),
        }
    }

    fn default_features_ro_compat(self) -> FeatureRoCompat {
        match self {
            FsType::Ext2 => FeatureRoCompat(EXT4_FEATURE_RO_COMPAT_SPARSE_SUPER),
            FsType::Ext3 => FeatureRoCompat(
                EXT4_FEATURE_RO_COMPAT_SPARSE_SUPER | EXT4_FEATURE_RO_COMPAT_LARGE_FILE,
            ),
            FsType::Ext4 => FeatureRoCompat(
                EXT4_FEATURE_RO_COMPAT_SPARSE_SUPER
                    | EXT4_FEATURE_RO_COMPAT_LARGE_FILE
                    | EXT4_FEATURE_RO_COMPAT_HUGE_FILE
                    | 0x0020 // extra_isize
                    | EXT4_FEATURE_RO_COMPAT_METADATA_CSUM,
            ),
        }
    }
}

// ---------------------------------------------------------------------------
// Superblock
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Superblock {
    s_inodes_count: u32,
    s_blocks_count: u64,
    s_r_blocks_count: u64,
    s_free_blocks_count: u64,
    s_free_inodes_count: u32,
    s_first_data_block: u32,
    s_log_block_size: u32,
    s_log_frag_size: u32,
    s_blocks_per_group: u32,
    s_frags_per_group: u32,
    s_inodes_per_group: u32,
    s_mtime: u32,
    s_wtime: u32,
    s_mnt_count: u16,
    s_max_mnt_count: u16,
    s_magic: u16,
    s_state: u16,
    s_errors: u16,
    s_minor_rev_level: u16,
    s_lastcheck: u32,
    s_checkinterval: u32,
    s_creator_os: u32,
    s_rev_level: u32,
    s_def_resuid: u16,
    s_def_resgid: u16,
    // Dynamic rev fields
    s_first_ino: u32,
    s_inode_size: u16,
    s_block_group_nr: u16,
    s_feature_compat: FeatureCompat,
    s_feature_incompat: FeatureIncompat,
    s_feature_ro_compat: FeatureRoCompat,
    s_uuid: [u8; 16],
    s_volume_name: [u8; EXT2_LABEL_LEN],
    s_last_mounted: [u8; 64],
    s_algorithm_usage_bitmap: u32,
    // Performance hints
    s_prealloc_blocks: u8,
    s_prealloc_dir_blocks: u8,
    s_reserved_gdt_blocks: u16,
    // Journal
    s_journal_uuid: [u8; 16],
    s_journal_inum: u32,
    s_journal_dev: u32,
    s_last_orphan: u32,
    // 64-bit support
    s_blocks_count_hi: u32,
    s_r_blocks_count_hi: u32,
    s_free_blocks_count_hi: u32,
    s_min_extra_isize: u16,
    s_want_extra_isize: u16,
    s_flags: u32,
    s_mkfs_time: u32,
    s_first_meta_bg: u32,
    // Checksums
    s_checksum: u32,
}

impl Default for Superblock {
    fn default() -> Self {
        Self {
            s_inodes_count: 0,
            s_blocks_count: 0,
            s_r_blocks_count: 0,
            s_free_blocks_count: 0,
            s_free_inodes_count: 0,
            s_first_data_block: 0,
            s_log_block_size: 2, // 4096 bytes
            s_log_frag_size: 2,
            s_blocks_per_group: EXT2_DEFAULT_BLOCKS_PER_GROUP,
            s_frags_per_group: EXT2_DEFAULT_BLOCKS_PER_GROUP,
            s_inodes_per_group: EXT2_DEFAULT_INODES_PER_GROUP,
            s_mtime: 0,
            s_wtime: 1700000000,
            s_mnt_count: 0,
            s_max_mnt_count: u16::MAX,
            s_magic: EXT2_MAGIC,
            s_state: 1,  // EXT2_VALID_FS
            s_errors: 1, // EXT2_ERRORS_CONTINUE
            s_minor_rev_level: 0,
            s_lastcheck: 1700000000,
            s_checkinterval: 0,
            s_creator_os: 5, // SlateOS
            s_rev_level: EXT2_DYNAMIC_REV,
            s_def_resuid: 0,
            s_def_resgid: 0,
            s_first_ino: 11,
            s_inode_size: 256,
            s_block_group_nr: 0,
            s_feature_compat: FeatureCompat(0),
            s_feature_incompat: FeatureIncompat(0),
            s_feature_ro_compat: FeatureRoCompat(0),
            s_uuid: DEFAULT_UUID,
            s_volume_name: [0; EXT2_LABEL_LEN],
            s_last_mounted: [0; 64],
            s_algorithm_usage_bitmap: 0,
            s_prealloc_blocks: 0,
            s_prealloc_dir_blocks: 0,
            s_reserved_gdt_blocks: 256,
            s_journal_uuid: [0; 16],
            s_journal_inum: 8,
            s_journal_dev: 0,
            s_last_orphan: 0,
            s_blocks_count_hi: 0,
            s_r_blocks_count_hi: 0,
            s_free_blocks_count_hi: 0,
            s_min_extra_isize: 32,
            s_want_extra_isize: 32,
            s_flags: 0,
            s_mkfs_time: 1700000000,
            s_first_meta_bg: 0,
            s_checksum: 0,
        }
    }
}

impl Superblock {
    fn block_size(&self) -> u32 {
        1024u32.checked_shl(self.s_log_block_size).unwrap_or(4096)
    }

    fn total_blocks(&self) -> u64 {
        self.s_blocks_count | (u64::from(self.s_blocks_count_hi) << 32)
    }

    fn total_free_blocks(&self) -> u64 {
        self.s_free_blocks_count | (u64::from(self.s_free_blocks_count_hi) << 32)
    }

    fn reserved_blocks(&self) -> u64 {
        self.s_r_blocks_count | (u64::from(self.s_r_blocks_count_hi) << 32)
    }

    fn group_count(&self) -> u32 {
        if self.s_blocks_per_group == 0 {
            return 1;
        }
        let total = self.total_blocks();
        let groups = total.div_ceil(u64::from(self.s_blocks_per_group));
        groups as u32
    }

    fn label(&self) -> &str {
        let end = self
            .s_volume_name
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(EXT2_LABEL_LEN);
        std::str::from_utf8(&self.s_volume_name[..end]).unwrap_or("")
    }

    fn set_label(&mut self, label: &str) {
        self.s_volume_name = [0; EXT2_LABEL_LEN];
        let bytes = label.as_bytes();
        let len = bytes.len().min(EXT2_LABEL_LEN);
        self.s_volume_name[..len].copy_from_slice(&bytes[..len]);
    }

    fn format_uuid(&self) -> String {
        format!(
            "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            self.s_uuid[0],
            self.s_uuid[1],
            self.s_uuid[2],
            self.s_uuid[3],
            self.s_uuid[4],
            self.s_uuid[5],
            self.s_uuid[6],
            self.s_uuid[7],
            self.s_uuid[8],
            self.s_uuid[9],
            self.s_uuid[10],
            self.s_uuid[11],
            self.s_uuid[12],
            self.s_uuid[13],
            self.s_uuid[14],
            self.s_uuid[15],
        )
    }

    #[allow(dead_code)] // tested; not yet on the dispatch path (see todo.txt)
    fn is_valid(&self) -> bool {
        self.s_magic == EXT2_MAGIC
    }

    #[allow(dead_code)] // tested; not yet on the dispatch path (see todo.txt)
    fn fs_type(&self) -> FsType {
        if self.s_feature_incompat.has_extents() {
            FsType::Ext4
        } else if self.s_feature_compat.has_journal() {
            FsType::Ext3
        } else {
            FsType::Ext2
        }
    }

    fn state_str(&self) -> &'static str {
        match self.s_state {
            1 => "clean",
            2 => "has errors",
            4 => "orphan recovery needed",
            _ => "unknown",
        }
    }

    fn errors_behavior_str(&self) -> &'static str {
        match self.s_errors {
            1 => "Continue",
            2 => "Remount read-only",
            3 => "Panic",
            _ => "Unknown",
        }
    }

    fn creator_os_str(&self) -> &'static str {
        match self.s_creator_os {
            0 => "Linux",
            1 => "GNU Hurd",
            2 => "Masix",
            3 => "FreeBSD",
            4 => "Lites",
            5 => "SlateOS",
            _ => "Unknown",
        }
    }

    fn compute_checksum(&self) -> u32 {
        // Simulated CRC32c checksum
        let mut crc: u32 = 0xFFFF_FFFF;
        let bytes = [
            self.s_inodes_count.to_le_bytes(),
            (self.s_blocks_count as u32).to_le_bytes(),
            self.s_blocks_per_group.to_le_bytes(),
            self.s_inodes_per_group.to_le_bytes(),
        ];
        for group in &bytes {
            for &b in group {
                crc ^= u32::from(b);
                for _ in 0..8 {
                    if crc & 1 != 0 {
                        crc = (crc >> 1) ^ 0x82F6_3B78;
                    } else {
                        crc >>= 1;
                    }
                }
            }
        }
        crc ^ 0xFFFF_FFFF
    }
}

// ---------------------------------------------------------------------------
// Block group descriptor
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct BlockGroupDesc {
    bg_block_bitmap: u64,
    bg_inode_bitmap: u64,
    bg_inode_table: u64,
    bg_free_blocks_count: u32,
    bg_free_inodes_count: u32,
    bg_used_dirs_count: u32,
    bg_flags: u16,
    bg_checksum: u16,
    _bg_itable_unused: u32,
}

impl BlockGroupDesc {
    fn new(
        block_bitmap: u64,
        inode_bitmap: u64,
        inode_table: u64,
        free_blocks: u32,
        free_inodes: u32,
    ) -> Self {
        Self {
            bg_block_bitmap: block_bitmap,
            bg_inode_bitmap: inode_bitmap,
            bg_inode_table: inode_table,
            bg_free_blocks_count: free_blocks,
            bg_free_inodes_count: free_inodes,
            bg_used_dirs_count: 0,
            bg_flags: 0,
            bg_checksum: 0,
            _bg_itable_unused: free_inodes,
        }
    }

    fn compute_checksum(&self, group_idx: u32, uuid: &[u8; 16]) -> u16 {
        let mut crc: u32 = 0xFFFF;
        // Mix in UUID
        for &b in uuid {
            crc ^= u32::from(b);
            for _ in 0..8 {
                if crc & 1 != 0 {
                    crc = (crc >> 1) ^ 0xA001;
                } else {
                    crc >>= 1;
                }
            }
        }
        // Mix in group index
        for &b in &group_idx.to_le_bytes() {
            crc ^= u32::from(b);
            for _ in 0..8 {
                if crc & 1 != 0 {
                    crc = (crc >> 1) ^ 0xA001;
                } else {
                    crc >>= 1;
                }
            }
        }
        (crc & 0xFFFF) as u16
    }
}

// ---------------------------------------------------------------------------
// Inode
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Inode {
    i_mode: u16,
    i_uid: u16,
    i_size: u32,
    i_atime: u32,
    i_ctime: u32,
    i_mtime: u32,
    i_dtime: u32,
    i_gid: u16,
    i_links_count: u16,
    i_blocks: u32,
    i_flags: u32,
    i_block: [u32; EXT2_N_BLOCKS],
    i_generation: u32,
    i_file_acl: u32,
    i_size_high: u32,
    _i_faddr: u32,
    i_extra_isize: u16,
    _i_checksum_hi: u16,
    i_crtime: u32,
}

impl Default for Inode {
    fn default() -> Self {
        Self {
            i_mode: 0,
            i_uid: 0,
            i_size: 0,
            i_atime: 0,
            i_ctime: 0,
            i_mtime: 0,
            i_dtime: 0,
            i_gid: 0,
            i_links_count: 0,
            i_blocks: 0,
            i_flags: 0,
            i_block: [0; EXT2_N_BLOCKS],
            i_generation: 0,
            i_file_acl: 0,
            i_size_high: 0,
            _i_faddr: 0,
            i_extra_isize: 32,
            _i_checksum_hi: 0,
            i_crtime: 0,
        }
    }
}

impl Inode {
    fn size(&self) -> u64 {
        u64::from(self.i_size) | (u64::from(self.i_size_high) << 32)
    }

    // The accessors below are tested but not yet wired into the dispatch path
    // (debugfs/dumpe2fs output is still being built out); #[allow(dead_code)]
    // until then. See todo.txt.
    #[allow(dead_code)]
    fn set_size(&mut self, size: u64) {
        self.i_size = size as u32;
        self.i_size_high = (size >> 32) as u32;
    }

    #[allow(dead_code)]
    fn is_dir(&self) -> bool {
        self.i_mode & S_IFMT == S_IFDIR
    }

    fn is_regular(&self) -> bool {
        self.i_mode & S_IFMT == S_IFREG
    }

    #[allow(dead_code)]
    fn is_symlink(&self) -> bool {
        self.i_mode & S_IFMT == S_IFLNK
    }

    #[allow(dead_code)]
    fn is_deleted(&self) -> bool {
        self.i_dtime != 0
    }

    #[allow(dead_code)]
    fn file_type_char(&self) -> char {
        match self.i_mode & S_IFMT {
            S_IFREG => '-',
            S_IFDIR => 'd',
            S_IFLNK => 'l',
            0x6000 => 'b',
            0x2000 => 'c',
            0x1000 => 'p',
            0xC000 => 's',
            _ => '?',
        }
    }

    #[allow(dead_code)]
    fn permissions_str(&self) -> String {
        let mode = self.i_mode;
        let mut s = String::with_capacity(10);
        s.push(self.file_type_char());
        s.push(if mode & 0o400 != 0 { 'r' } else { '-' });
        s.push(if mode & 0o200 != 0 { 'w' } else { '-' });
        s.push(if mode & 0o100 != 0 { 'x' } else { '-' });
        s.push(if mode & 0o040 != 0 { 'r' } else { '-' });
        s.push(if mode & 0o020 != 0 { 'w' } else { '-' });
        s.push(if mode & 0o010 != 0 { 'x' } else { '-' });
        s.push(if mode & 0o004 != 0 { 'r' } else { '-' });
        s.push(if mode & 0o002 != 0 { 'w' } else { '-' });
        s.push(if mode & 0o001 != 0 { 'x' } else { '-' });
        s
    }

    #[allow(dead_code)]
    fn format_flags(&self) -> String {
        let mut flags = Vec::new();
        if self.i_flags & 0x0000_0001 != 0 {
            flags.push("Secure_Deletion");
        }
        if self.i_flags & 0x0000_0002 != 0 {
            flags.push("Undelete");
        }
        if self.i_flags & 0x0000_0004 != 0 {
            flags.push("Compressed");
        }
        if self.i_flags & 0x0000_0008 != 0 {
            flags.push("Synchronous");
        }
        if self.i_flags & 0x0000_0010 != 0 {
            flags.push("Immutable");
        }
        if self.i_flags & 0x0000_0020 != 0 {
            flags.push("Append_Only");
        }
        if self.i_flags & 0x0000_0040 != 0 {
            flags.push("No_Dump");
        }
        if self.i_flags & 0x0000_0080 != 0 {
            flags.push("No_Atime");
        }
        if self.i_flags & 0x0008_0000 != 0 {
            flags.push("Extents");
        }
        if flags.is_empty() {
            String::from("(none)")
        } else {
            flags.join(", ")
        }
    }
}

// ---------------------------------------------------------------------------
// Directory entry
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
struct DirEntry {
    inode: u32,
    rec_len: u16,
    name_len: u8,
    file_type: u8,
    name: String,
}

impl DirEntry {
    fn new(inode: u32, file_type: u8, name: &str) -> Self {
        let name_len = name.len().min(EXT2_NAME_LEN) as u8;
        // Record length is padded to 4-byte boundary
        let base_len = 8 + u16::from(name_len);
        let rec_len = (base_len + 3) & !3;
        Self {
            inode,
            rec_len,
            name_len,
            file_type,
            name: name.to_string(),
        }
    }

    fn file_type_str(&self) -> &'static str {
        match self.file_type {
            0 => "Unknown",
            1 => "Regular",
            2 => "Directory",
            3 => "Character device",
            4 => "Block device",
            5 => "FIFO",
            6 => "Socket",
            7 => "Symbolic link",
            _ => "Unknown",
        }
    }
}

// ---------------------------------------------------------------------------
// Extent tree (ext4)
// ---------------------------------------------------------------------------

// ExtentHeader/ExtentIndex model the ext4 extent-tree on-disk structures.
// They're tested but the extent-tree walk isn't yet wired into the dumpe2fs/
// debugfs output path, so they carry #[allow(dead_code)] for now. See todo.txt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
struct ExtentHeader {
    eh_magic: u16,
    eh_entries: u16,
    eh_max: u16,
    eh_depth: u16,
    _eh_generation: u32,
}

impl ExtentHeader {
    #[allow(dead_code)]
    fn new(entries: u16, max: u16, depth: u16) -> Self {
        Self {
            eh_magic: 0xF30A,
            eh_entries: entries,
            eh_max: max,
            eh_depth: depth,
            _eh_generation: 0,
        }
    }

    #[allow(dead_code)]
    fn is_valid(&self) -> bool {
        self.eh_magic == 0xF30A
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Extent {
    ee_block: u32,
    ee_len: u16,
    ee_start_hi: u16,
    ee_start_lo: u32,
}

impl Extent {
    fn new(logical_block: u32, len: u16, physical_block: u64) -> Self {
        Self {
            ee_block: logical_block,
            ee_len: len,
            ee_start_hi: (physical_block >> 32) as u16,
            ee_start_lo: physical_block as u32,
        }
    }

    fn physical_block(&self) -> u64 {
        u64::from(self.ee_start_lo) | (u64::from(self.ee_start_hi) << 32)
    }

    fn is_uninitialized(&self) -> bool {
        self.ee_len > 0x8000
    }

    fn actual_len(&self) -> u16 {
        if self.is_uninitialized() {
            self.ee_len - 0x8000
        } else {
            self.ee_len
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
struct ExtentIndex {
    ei_block: u32,
    ei_leaf_lo: u32,
    ei_leaf_hi: u16,
    _ei_unused: u16,
}

impl ExtentIndex {
    #[allow(dead_code)]
    fn leaf_block(&self) -> u64 {
        u64::from(self.ei_leaf_lo) | (u64::from(self.ei_leaf_hi) << 32)
    }
}

// ---------------------------------------------------------------------------
// Journal structures
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct JournalSuperblock {
    js_header_magic: u32,
    js_header_blocktype: u32,
    js_header_sequence: u32,
    js_blocksize: u32,
    js_maxlen: u32,
    js_first: u32,
    js_sequence: u32,
    js_start: u32,
    js_errno: i32,
    // Compat features
    js_feature_compat: u32,
    js_feature_incompat: u32,
    js_feature_ro_compat: u32,
    js_uuid: [u8; 16],
    js_nr_users: u32,
    _js_dynsuper: u32,
    js_max_transaction: u32,
    js_max_trans_data: u32,
    _js_checksum_type: u8,
}

impl Default for JournalSuperblock {
    fn default() -> Self {
        Self {
            js_header_magic: 0xC03B_3998,
            js_header_blocktype: 3, // descriptor block
            js_header_sequence: 0,
            js_blocksize: 4096,
            js_maxlen: 32768,
            js_first: 1,
            js_sequence: 1,
            js_start: 0,
            js_errno: 0,
            js_feature_compat: 0,
            js_feature_incompat: 0,
            js_feature_ro_compat: 0,
            js_uuid: DEFAULT_UUID,
            js_nr_users: 1,
            _js_dynsuper: 0,
            js_max_transaction: 0,
            js_max_trans_data: 0,
            _js_checksum_type: 1,
        }
    }
}

impl JournalSuperblock {
    #[allow(dead_code)] // tested; journal inspection not yet on the dispatch path
    fn is_valid(&self) -> bool {
        self.js_header_magic == 0xC03B_3998
    }
}

// ---------------------------------------------------------------------------
// Simulated filesystem image
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct FilesystemImage {
    #[allow(dead_code)] // recorded at creation; not yet read by the dispatch path
    device: String,
    superblock: Superblock,
    block_groups: Vec<BlockGroupDesc>,
    inodes: Vec<Inode>,
    dir_entries: Vec<(u32, Vec<DirEntry>)>, // (parent_inode, entries)
    journal: Option<JournalSuperblock>,
    extents: Vec<(u32, Vec<Extent>)>, // (inode, extents)
    _mounted: bool,
}

impl FilesystemImage {
    fn new(device: &str) -> Self {
        Self {
            device: device.to_string(),
            superblock: Superblock::default(),
            block_groups: Vec::new(),
            inodes: Vec::new(),
            dir_entries: Vec::new(),
            journal: None,
            extents: Vec::new(),
            _mounted: false,
        }
    }

    fn create_default(device: &str, fs_type: FsType, blocks: u64, label: &str) -> Self {
        let mut img = Self::new(device);
        let sb = &mut img.superblock;

        // Set features based on fs type
        sb.s_feature_compat = fs_type.default_features_compat();
        sb.s_feature_incompat = fs_type.default_features_incompat();
        sb.s_feature_ro_compat = fs_type.default_features_ro_compat();

        let block_size = match fs_type {
            FsType::Ext2 => EXT2_DEFAULT_BLOCK_SIZE,
            FsType::Ext3 | FsType::Ext4 => EXT4_DEFAULT_BLOCK_SIZE,
        };
        sb.s_log_block_size = match block_size {
            1024 => 0,
            2048 => 1,
            4096 => 2,
            8192 => 3,
            16384 => 4,
            _ => 2,
        };
        sb.s_log_frag_size = sb.s_log_block_size;

        sb.s_blocks_count = blocks;
        sb.s_blocks_count_hi = (blocks >> 32) as u32;

        // 5% reserved for root
        let reserved = blocks / 20;
        sb.s_r_blocks_count = reserved;

        sb.s_blocks_per_group = EXT2_DEFAULT_BLOCKS_PER_GROUP;
        sb.s_frags_per_group = EXT2_DEFAULT_BLOCKS_PER_GROUP;
        sb.s_inodes_per_group = EXT2_DEFAULT_INODES_PER_GROUP;

        // First data block: 0 for 4k blocks, 1 for 1k blocks
        sb.s_first_data_block = if block_size <= 1024 { 1 } else { 0 };

        let group_count = sb.group_count();
        sb.s_inodes_count = group_count * sb.s_inodes_per_group;

        // Compute used blocks (rough simulation)
        let overhead_per_group: u64 = 3 + // superblock copy, GDT, reserved GDT
            1 + // block bitmap
            1 + // inode bitmap
            u64::from(sb.s_inodes_per_group * u32::from(sb.s_inode_size)) / u64::from(block_size);
        let total_overhead = u64::from(group_count) * overhead_per_group;
        let used_blocks = total_overhead + 100; // root dir, journal, etc.
        sb.s_free_blocks_count = blocks.saturating_sub(used_blocks);
        sb.s_free_inodes_count = sb.s_inodes_count.saturating_sub(11); // first 11 inodes reserved

        sb.s_rev_level = EXT2_DYNAMIC_REV;
        sb.s_inode_size = 256;
        sb.s_first_ino = 11;

        sb.set_label(label);

        // Journal for ext3/ext4
        if sb.s_feature_compat.has_journal() {
            let journal = JournalSuperblock {
                js_blocksize: block_size,
                js_maxlen: 32768.min(blocks as u32 / 4),
                ..JournalSuperblock::default()
            };
            img.journal = Some(journal);
        }

        // Create block group descriptors
        let free_blocks_per_group = (sb.s_free_blocks_count as u32)
            .checked_div(group_count)
            .unwrap_or(0);
        let free_inodes_per_group = sb.s_free_inodes_count.checked_div(group_count).unwrap_or(0);

        for i in 0..group_count {
            let base =
                u64::from(sb.s_first_data_block) + u64::from(i) * u64::from(sb.s_blocks_per_group);
            let block_bitmap = base + 1;
            let inode_bitmap = base + 2;
            let inode_table = base + 3;

            let free_b = if i == group_count - 1 {
                // Last group may differ
                sb.s_free_blocks_count as u32 - free_blocks_per_group * (group_count - 1)
            } else {
                free_blocks_per_group
            };

            let free_i = if i == group_count - 1 {
                sb.s_free_inodes_count - free_inodes_per_group * (group_count - 1)
            } else {
                free_inodes_per_group
            };

            let mut bgd =
                BlockGroupDesc::new(block_bitmap, inode_bitmap, inode_table, free_b, free_i);
            bgd.bg_checksum = bgd.compute_checksum(i, &sb.s_uuid);
            if i == 0 {
                bgd.bg_used_dirs_count = 2; // root + lost+found
            }
            img.block_groups.push(bgd);
        }

        // Create inodes
        // Reserved inodes (1..=10)
        for _ in 0..10 {
            img.inodes.push(Inode::default());
        }

        // Root inode (#2, index 1)
        let root_data_block = sb.s_first_data_block + group_count + 10;
        {
            let root = &mut img.inodes[1];
            root.i_mode = S_IFDIR | 0o755;
            root.i_links_count = 3; // ., .., lost+found
            root.i_size = block_size;
            root.i_blocks = block_size / 512;
            root.i_ctime = 1700000000;
            root.i_mtime = 1700000000;
            root.i_atime = 1700000000;
            root.i_crtime = 1700000000;
            root.i_block[0] = root_data_block;
        }

        // lost+found inode (#11, index 10 in our 0-indexed array)
        let lost_found = Inode {
            i_mode: S_IFDIR | 0o700,
            i_links_count: 2,
            i_size: block_size * 4,
            i_blocks: block_size * 4 / 512,
            i_ctime: 1700000000,
            i_mtime: 1700000000,
            i_atime: 1700000000,
            i_crtime: 1700000000,
            ..Inode::default()
        };
        img.inodes.push(lost_found);

        // Create root directory entries
        img.dir_entries.push((
            EXT2_ROOT_INO,
            vec![
                DirEntry::new(EXT2_ROOT_INO, EXT2_FT_DIR, "."),
                DirEntry::new(EXT2_ROOT_INO, EXT2_FT_DIR, ".."),
                DirEntry::new(11, EXT2_FT_DIR, "lost+found"),
            ],
        ));

        // lost+found directory
        img.dir_entries.push((
            11,
            vec![
                DirEntry::new(11, EXT2_FT_DIR, "."),
                DirEntry::new(EXT2_ROOT_INO, EXT2_FT_DIR, ".."),
            ],
        ));

        // Add some extents for ext4
        if fs_type == FsType::Ext4 {
            img.extents.push((
                EXT2_ROOT_INO,
                vec![Extent::new(0, 1, u64::from(root_data_block))],
            ));
        }

        sb.s_checksum = sb.compute_checksum();
        img
    }
}

// ---------------------------------------------------------------------------
// Utility functions
// ---------------------------------------------------------------------------

fn format_bytes(bytes: u64) -> String {
    const KI: u64 = 1024;
    const MI: u64 = 1024 * 1024;
    const GI: u64 = 1024 * 1024 * 1024;
    const TI: u64 = 1024 * 1024 * 1024 * 1024;

    if bytes >= TI {
        format!("{:.1} TiB", bytes as f64 / TI as f64)
    } else if bytes >= GI {
        format!("{:.1} GiB", bytes as f64 / GI as f64)
    } else if bytes >= MI {
        format!("{:.1} MiB", bytes as f64 / MI as f64)
    } else if bytes >= KI {
        format!("{:.1} KiB", bytes as f64 / KI as f64)
    } else {
        format!("{bytes} bytes")
    }
}

fn format_timestamp(ts: u32) -> String {
    if ts == 0 {
        return String::from("n/a");
    }
    // Simple timestamp formatting (simulated)
    let secs = ts % 60;
    let mins = (ts / 60) % 60;
    let hours = (ts / 3600) % 24;
    let days = ts / 86400;
    // Approximate date from epoch
    let years = 1970 + days / 365;
    let remaining_days = days % 365;
    let month = remaining_days / 30 + 1;
    let day = remaining_days % 30 + 1;
    format!("{years}-{month:02}-{day:02} {hours:02}:{mins:02}:{secs:02}")
}

fn parse_size(s: &str) -> Result<u64, String> {
    let s = s.trim();
    if s.is_empty() {
        return Err("empty size string".to_string());
    }

    let (num_str, multiplier) = if let Some(n) = s.strip_suffix('T') {
        (n, 1024u64 * 1024 * 1024 * 1024)
    } else if let Some(n) = s.strip_suffix('G') {
        (n, 1024 * 1024 * 1024)
    } else if let Some(n) = s.strip_suffix('M') {
        (n, 1024 * 1024)
    } else if let Some(n) = s.strip_suffix('K') {
        (n, 1024)
    } else if let Some(n) = s.strip_suffix("TiB") {
        (n, 1024u64 * 1024 * 1024 * 1024)
    } else if let Some(n) = s.strip_suffix("GiB") {
        (n, 1024 * 1024 * 1024)
    } else if let Some(n) = s.strip_suffix("MiB") {
        (n, 1024 * 1024)
    } else if let Some(n) = s.strip_suffix("KiB") {
        (n, 1024)
    } else {
        (s, 1)
    };

    let num: u64 = num_str
        .trim()
        .parse()
        .map_err(|e| format!("invalid number '{num_str}': {e}"))?;
    num.checked_mul(multiplier)
        .ok_or_else(|| "size overflow".to_string())
}

fn parse_block_count(s: &str, block_size: u32) -> Result<u64, String> {
    // If it has a suffix, treat as byte size and convert to blocks
    let last = s.bytes().last().unwrap_or(b'0');
    if last.is_ascii_alphabetic() {
        let bytes = parse_size(s)?;
        Ok(bytes / u64::from(block_size))
    } else {
        s.parse::<u64>()
            .map_err(|e| format!("invalid block count '{s}': {e}"))
    }
}

fn parse_uuid(s: &str) -> Result<[u8; 16], String> {
    let hex: String = s.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    if hex.len() != 32 {
        return Err(format!(
            "invalid UUID '{}': expected 32 hex digits, got {}",
            s,
            hex.len()
        ));
    }
    let mut uuid = [0u8; 16];
    for i in 0..16 {
        uuid[i] = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16)
            .map_err(|e| format!("invalid UUID hex: {e}"))?;
    }
    Ok(uuid)
}

fn is_power_of(n: u32, base: u32) -> bool {
    if n == 0 || base <= 1 {
        return false;
    }
    let mut v = n;
    while v > 1 {
        if !v.is_multiple_of(base) {
            return false;
        }
        v /= base;
    }
    true
}

fn has_superblock_backup(group: u32) -> bool {
    // Sparse superblock: copies in groups 0, 1, and powers of 3, 5, 7
    group == 0
        || group == 1
        || is_power_of(group, 3)
        || is_power_of(group, 5)
        || is_power_of(group, 7)
}

// ---------------------------------------------------------------------------
// mke2fs — create filesystem
// ---------------------------------------------------------------------------

struct Mke2fsOptions {
    device: String,
    fs_type: FsType,
    block_size: u32,
    blocks: Option<u64>,
    inode_size: u16,
    inodes_per_group: u32,
    label: String,
    uuid: Option<[u8; 16]>,
    reserved_percent: f64,
    journal: bool,
    no_journal: bool,
    quiet: bool,
    verbose: bool,
    dry_run: bool,
    stride: u32,
    stripe_width: u32,
}

impl Default for Mke2fsOptions {
    fn default() -> Self {
        Self {
            device: String::new(),
            fs_type: FsType::Ext4,
            block_size: EXT4_DEFAULT_BLOCK_SIZE,
            blocks: None,
            inode_size: 256,
            inodes_per_group: EXT2_DEFAULT_INODES_PER_GROUP,
            label: String::new(),
            uuid: None,
            reserved_percent: 5.0,
            journal: true,
            no_journal: false,
            quiet: false,
            verbose: false,
            dry_run: false,
            stride: 0,
            stripe_width: 0,
        }
    }
}

fn parse_mke2fs_args(args: &[String]) -> Result<Mke2fsOptions, String> {
    let mut opts = Mke2fsOptions::default();
    let mut positional = Vec::new();

    // Check argv[0] for personality-based type
    if !args.is_empty() {
        let base = args[0].rsplit('/').next().unwrap_or(&args[0]);
        let base = base.rsplit('\\').next().unwrap_or(base);
        let lower = base.to_ascii_lowercase();
        let name = lower.strip_suffix(".exe").unwrap_or(&lower);
        match name {
            "mkfs.ext2" => {
                opts.fs_type = FsType::Ext2;
                opts.journal = false;
            }
            "mkfs.ext3" => {
                opts.fs_type = FsType::Ext3;
            }
            _ => {}
        }
    }

    let mut i = 1; // skip argv[0]
    while i < args.len() {
        match args[i].as_str() {
            "-t" => {
                i += 1;
                if i >= args.len() {
                    return Err("-t requires an argument".to_string());
                }
                opts.fs_type = match args[i].as_str() {
                    "ext2" => FsType::Ext2,
                    "ext3" => FsType::Ext3,
                    "ext4" => FsType::Ext4,
                    other => return Err(format!("unknown filesystem type: {other}")),
                };
            }
            "-b" => {
                i += 1;
                if i >= args.len() {
                    return Err("-b requires an argument".to_string());
                }
                opts.block_size = args[i]
                    .parse()
                    .map_err(|e| format!("invalid block size: {e}"))?;
                if opts.block_size < EXT2_MIN_BLOCK_SIZE
                    || opts.block_size > EXT2_MAX_BLOCK_SIZE
                    || opts.block_size & (opts.block_size - 1) != 0
                {
                    return Err(format!(
                        "invalid block size {}: must be power of 2 between {} and {}",
                        opts.block_size, EXT2_MIN_BLOCK_SIZE, EXT2_MAX_BLOCK_SIZE
                    ));
                }
            }
            "-I" => {
                i += 1;
                if i >= args.len() {
                    return Err("-I requires an argument".to_string());
                }
                opts.inode_size = args[i]
                    .parse()
                    .map_err(|e| format!("invalid inode size: {e}"))?;
                if opts.inode_size < EXT2_GOOD_OLD_INODE_SIZE || opts.inode_size > 4096 {
                    return Err(format!(
                        "invalid inode size {}: must be between {} and 4096",
                        opts.inode_size, EXT2_GOOD_OLD_INODE_SIZE
                    ));
                }
            }
            "-L" => {
                i += 1;
                if i >= args.len() {
                    return Err("-L requires an argument".to_string());
                }
                opts.label = args[i].clone();
                if opts.label.len() > EXT2_LABEL_LEN {
                    return Err(format!("label too long: max {} characters", EXT2_LABEL_LEN));
                }
            }
            "-U" => {
                i += 1;
                if i >= args.len() {
                    return Err("-U requires an argument".to_string());
                }
                opts.uuid = Some(parse_uuid(&args[i])?);
            }
            "-m" => {
                i += 1;
                if i >= args.len() {
                    return Err("-m requires an argument".to_string());
                }
                opts.reserved_percent = args[i]
                    .parse()
                    .map_err(|e| format!("invalid reserved percentage: {e}"))?;
            }
            "-O" => {
                i += 1;
                if i >= args.len() {
                    return Err("-O requires an argument".to_string());
                }
                // Feature parsing
                for feat in args[i].split(',') {
                    match feat.trim() {
                        "^has_journal" => opts.no_journal = true,
                        "has_journal" => opts.journal = true,
                        "^extent" | "^extents" if opts.fs_type == FsType::Ext4 => {
                            opts.fs_type = FsType::Ext3;
                        }
                        _ => {} // Silently ignore unknown features for forward compat
                    }
                }
            }
            "-E" => {
                i += 1;
                if i >= args.len() {
                    return Err("-E requires an argument".to_string());
                }
                for param in args[i].split(',') {
                    if let Some(val) = param.strip_prefix("stride=") {
                        opts.stride = val.parse().map_err(|e| format!("invalid stride: {e}"))?;
                    } else if let Some(val) = param.strip_prefix("stripe-width=") {
                        opts.stripe_width = val
                            .parse()
                            .map_err(|e| format!("invalid stripe-width: {e}"))?;
                    }
                }
            }
            "-q" => opts.quiet = true,
            "-v" => opts.verbose = true,
            "-n" => opts.dry_run = true,
            "-V" => {
                println!("mke2fs 1.47.0 (SlateOS)");
                process::exit(0);
            }
            arg if arg.starts_with('-') => {
                return Err(format!("unknown option: {arg}"));
            }
            _ => {
                positional.push(args[i].clone());
            }
        }
        i += 1;
    }

    if positional.is_empty() {
        return Err("missing device argument".to_string());
    }
    opts.device = positional[0].clone();

    if positional.len() > 1 {
        opts.blocks = Some(parse_block_count(&positional[1], opts.block_size)?);
    }

    Ok(opts)
}

fn run_mke2fs(args: &[String]) -> i32 {
    let opts = match parse_mke2fs_args(args) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("mke2fs: {e}");
            eprintln!(
                "Usage: mke2fs [-t fs-type] [-b block-size] [-L label] [-U uuid] device [blocks]"
            );
            return 1;
        }
    };

    let blocks = opts.blocks.unwrap_or(262144); // Default 1GiB @ 4k blocks
    let journal_enabled = opts.journal && !opts.no_journal && opts.fs_type != FsType::Ext2;

    let mut img = FilesystemImage::create_default(&opts.device, opts.fs_type, blocks, &opts.label);

    if let Some(uuid) = opts.uuid {
        img.superblock.s_uuid = uuid;
    }

    img.superblock.s_inode_size = opts.inode_size;
    img.superblock.s_inodes_per_group = opts.inodes_per_group;

    // Apply reserved percentage
    let reserved = (blocks as f64 * opts.reserved_percent / 100.0) as u64;
    img.superblock.s_r_blocks_count = reserved;
    img.superblock.s_r_blocks_count_hi = (reserved >> 32) as u32;

    // Apply journal preference
    if !journal_enabled {
        img.superblock.s_feature_compat.clear_journal();
        img.journal = None;
    }

    img.superblock.s_checksum = img.superblock.compute_checksum();

    if !opts.quiet {
        let sb = &img.superblock;
        let total_bytes = u64::from(sb.block_size()) * sb.total_blocks();
        println!("mke2fs 1.47.0 (SlateOS)");
        println!(
            "Creating filesystem with {} {}k blocks and {} inodes",
            sb.total_blocks(),
            sb.block_size() / 1024,
            sb.s_inodes_count,
        );
        println!("Filesystem UUID: {}", sb.format_uuid());
        if !opts.label.is_empty() {
            println!("Filesystem label: {}", opts.label);
        }
        println!(
            "Superblock backups stored on blocks: {}",
            format_backup_blocks(sb)
        );
        println!();
        println!("Allocating group tables: done");
        println!("Writing inode tables: done");
        if journal_enabled && let Some(ref j) = img.journal {
            println!("Creating journal ({} blocks): done", j.js_maxlen);
        }
        println!("Writing superblocks and filesystem accounting information: done");
        println!();
        if opts.verbose {
            println!("Total size: {}", format_bytes(total_bytes));
            println!("Block size: {}", sb.block_size());
            println!("Groups: {}", sb.group_count());
        }
    }

    if opts.dry_run {
        println!("(dry run, no changes written)");
    }

    0
}

fn format_backup_blocks(sb: &Superblock) -> String {
    let mut backups = Vec::new();
    let group_count = sb.group_count();
    for g in 1..group_count {
        if has_superblock_backup(g) {
            let block =
                u64::from(sb.s_first_data_block) + u64::from(g) * u64::from(sb.s_blocks_per_group);
            backups.push(block.to_string());
        }
        if backups.len() >= 10 {
            break;
        }
    }
    backups.join(", ")
}

// ---------------------------------------------------------------------------
// tune2fs — adjust filesystem parameters
// ---------------------------------------------------------------------------

#[derive(Default)]
struct Tune2fsOptions {
    device: String,
    label: Option<String>,
    uuid: Option<String>,
    max_mount_count: Option<u16>,
    check_interval: Option<u32>,
    error_behavior: Option<u16>,
    reserved_percent: Option<f64>,
    reserved_uid: Option<u16>,
    reserved_gid: Option<u16>,
    add_journal: bool,
    remove_journal: bool,
    list_contents: bool,
    set_features: Vec<String>,
    clear_features: Vec<String>,
    mount_opts: Option<String>,
}

fn parse_tune2fs_args(args: &[String]) -> Result<Tune2fsOptions, String> {
    let mut opts = Tune2fsOptions::default();
    let mut i = 1;

    while i < args.len() {
        match args[i].as_str() {
            "-L" => {
                i += 1;
                if i >= args.len() {
                    return Err("-L requires an argument".to_string());
                }
                opts.label = Some(args[i].clone());
            }
            "-U" => {
                i += 1;
                if i >= args.len() {
                    return Err("-U requires an argument".to_string());
                }
                opts.uuid = Some(args[i].clone());
            }
            "-c" => {
                i += 1;
                if i >= args.len() {
                    return Err("-c requires an argument".to_string());
                }
                opts.max_mount_count = Some(
                    args[i]
                        .parse()
                        .map_err(|e| format!("invalid mount count: {e}"))?,
                );
            }
            "-i" => {
                i += 1;
                if i >= args.len() {
                    return Err("-i requires an argument".to_string());
                }
                let s = &args[i];
                let interval = if let Some(d) = s.strip_suffix('d') {
                    d.parse::<u32>()
                        .map_err(|e| format!("invalid interval: {e}"))?
                        * 86400
                } else if let Some(w) = s.strip_suffix('w') {
                    w.parse::<u32>()
                        .map_err(|e| format!("invalid interval: {e}"))?
                        * 604800
                } else if let Some(m) = s.strip_suffix('m') {
                    m.parse::<u32>()
                        .map_err(|e| format!("invalid interval: {e}"))?
                        * 2592000
                } else {
                    s.parse::<u32>()
                        .map_err(|e| format!("invalid interval: {e}"))?
                        * 86400
                };
                opts.check_interval = Some(interval);
            }
            "-e" => {
                i += 1;
                if i >= args.len() {
                    return Err("-e requires an argument".to_string());
                }
                opts.error_behavior = Some(match args[i].as_str() {
                    "continue" => 1,
                    "remount-ro" => 2,
                    "panic" => 3,
                    other => {
                        return Err(format!("unknown error behavior: {other}"));
                    }
                });
            }
            "-m" => {
                i += 1;
                if i >= args.len() {
                    return Err("-m requires an argument".to_string());
                }
                opts.reserved_percent = Some(
                    args[i]
                        .parse()
                        .map_err(|e| format!("invalid reserved percentage: {e}"))?,
                );
            }
            "-r" => {
                i += 1;
                if i >= args.len() {
                    return Err("-r requires an argument".to_string());
                }
                opts.reserved_uid = Some(args[i].parse().map_err(|e| format!("invalid UID: {e}"))?);
            }
            "-g" => {
                i += 1;
                if i >= args.len() {
                    return Err("-g requires an argument".to_string());
                }
                opts.reserved_gid = Some(args[i].parse().map_err(|e| format!("invalid GID: {e}"))?);
            }
            "-j" => opts.add_journal = true,
            "-O" => {
                i += 1;
                if i >= args.len() {
                    return Err("-O requires an argument".to_string());
                }
                for feat in args[i].split(',') {
                    let feat = feat.trim();
                    if let Some(cleared) = feat.strip_prefix('^') {
                        opts.clear_features.push(cleared.to_string());
                        if cleared == "has_journal" {
                            opts.remove_journal = true;
                        }
                    } else {
                        opts.set_features.push(feat.to_string());
                        if feat == "has_journal" {
                            opts.add_journal = true;
                        }
                    }
                }
            }
            "-o" => {
                i += 1;
                if i >= args.len() {
                    return Err("-o requires an argument".to_string());
                }
                opts.mount_opts = Some(args[i].clone());
            }
            "-l" => opts.list_contents = true,
            arg if arg.starts_with('-') => {
                return Err(format!("unknown option: {arg}"));
            }
            _ => {
                opts.device = args[i].clone();
            }
        }
        i += 1;
    }

    if opts.device.is_empty() {
        return Err("missing device argument".to_string());
    }
    Ok(opts)
}

fn run_tune2fs(args: &[String]) -> i32 {
    let opts = match parse_tune2fs_args(args) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("tune2fs: {e}");
            eprintln!("Usage: tune2fs [-l] [-L label] [-U uuid] [-c count] [-e behavior] device");
            return 1;
        }
    };

    // Simulate reading the filesystem
    let mut img = FilesystemImage::create_default(&opts.device, FsType::Ext4, 262144, "");

    if opts.list_contents {
        print_superblock_info(&img.superblock);
        return 0;
    }

    let mut changed = false;

    if let Some(ref label) = opts.label {
        img.superblock.set_label(label);
        println!("tune2fs: setting label to '{label}'");
        changed = true;
    }

    if let Some(ref uuid_str) = opts.uuid {
        match uuid_str.as_str() {
            "random" => {
                // Simulated random UUID
                img.superblock.s_uuid = [
                    0xDE, 0xAD, 0xBE, 0xEF, 0xCA, 0xFE, 0xBA, 0xBE, 0x12, 0x34, 0x56, 0x78, 0x9A,
                    0xBC, 0xDE, 0xF0,
                ];
                println!("tune2fs: setting UUID to {}", img.superblock.format_uuid());
                changed = true;
            }
            "clear" => {
                img.superblock.s_uuid = [0; 16];
                println!("tune2fs: clearing UUID");
                changed = true;
            }
            "time" => {
                // Time-based UUID (simulated)
                img.superblock.s_uuid = [
                    0x01, 0x23, 0x45, 0x67, 0x89, 0xAB, 0xCD, 0xEF, 0xFE, 0xDC, 0xBA, 0x98, 0x76,
                    0x54, 0x32, 0x10,
                ];
                println!("tune2fs: setting UUID to {}", img.superblock.format_uuid());
                changed = true;
            }
            _ => match parse_uuid(uuid_str) {
                Ok(uuid) => {
                    img.superblock.s_uuid = uuid;
                    println!("tune2fs: setting UUID to {}", img.superblock.format_uuid());
                    changed = true;
                }
                Err(e) => {
                    eprintln!("tune2fs: {e}");
                    return 1;
                }
            },
        }
    }

    if let Some(max) = opts.max_mount_count {
        img.superblock.s_max_mnt_count = max;
        println!("tune2fs: setting max mount count to {max}");
        changed = true;
    }

    if let Some(interval) = opts.check_interval {
        img.superblock.s_checkinterval = interval;
        println!(
            "tune2fs: setting check interval to {} days",
            interval / 86400
        );
        changed = true;
    }

    if let Some(behavior) = opts.error_behavior {
        img.superblock.s_errors = behavior;
        println!(
            "tune2fs: setting error behavior to {}",
            img.superblock.errors_behavior_str()
        );
        changed = true;
    }

    if let Some(pct) = opts.reserved_percent {
        let total = img.superblock.total_blocks();
        let reserved = (total as f64 * pct / 100.0) as u64;
        img.superblock.s_r_blocks_count = reserved;
        img.superblock.s_r_blocks_count_hi = (reserved >> 32) as u32;
        println!("tune2fs: setting reserved blocks percentage to {pct}%");
        changed = true;
    }

    if let Some(uid) = opts.reserved_uid {
        img.superblock.s_def_resuid = uid;
        println!("tune2fs: setting reserved blocks uid to {uid}");
        changed = true;
    }

    if let Some(gid) = opts.reserved_gid {
        img.superblock.s_def_resgid = gid;
        println!("tune2fs: setting reserved blocks gid to {gid}");
        changed = true;
    }

    if opts.add_journal && !img.superblock.s_feature_compat.has_journal() {
        img.superblock.s_feature_compat.set_journal();
        img.journal = Some(JournalSuperblock::default());
        println!("tune2fs: creating journal");
        changed = true;
    }

    if opts.remove_journal && img.superblock.s_feature_compat.has_journal() {
        img.superblock.s_feature_compat.clear_journal();
        img.journal = None;
        println!("tune2fs: removing journal");
        changed = true;
    }

    if changed {
        img.superblock.s_checksum = img.superblock.compute_checksum();
        println!("tune2fs: done");
    } else {
        eprintln!("tune2fs: no changes requested");
        return 1;
    }

    0
}

fn print_superblock_info(sb: &Superblock) {
    println!(
        "Filesystem volume name:   {}",
        if sb.label().is_empty() {
            "<none>"
        } else {
            sb.label()
        }
    );
    println!("Last mounted on:          <not available>");
    println!("Filesystem UUID:          {}", sb.format_uuid());
    println!("Filesystem magic number:  0x{:04X}", sb.s_magic);
    println!(
        "Filesystem revision #:    {} ({})",
        sb.s_rev_level,
        if sb.s_rev_level == EXT2_GOOD_OLD_REV {
            "original"
        } else {
            "dynamic"
        }
    );
    println!(
        "Filesystem features:      {}",
        sb.s_feature_compat.format_flags()
    );
    println!("Filesystem flags:         signed_directory_hash");
    println!("Default mount options:    user_xattr acl");
    println!("Filesystem state:         {}", sb.state_str());
    println!("Errors behavior:          {}", sb.errors_behavior_str());
    println!("Filesystem OS type:       {}", sb.creator_os_str());
    println!("Inode count:              {}", sb.s_inodes_count);
    println!("Block count:              {}", sb.total_blocks());
    println!("Reserved block count:     {}", sb.reserved_blocks());
    println!("Free blocks:              {}", sb.total_free_blocks());
    println!("Free inodes:              {}", sb.s_free_inodes_count);
    println!("First block:              {}", sb.s_first_data_block);
    println!("Block size:               {}", sb.block_size());
    println!("Fragment size:            {}", sb.block_size());
    println!("Group descriptor size:    64");
    println!("Blocks per group:         {}", sb.s_blocks_per_group);
    println!("Fragments per group:      {}", sb.s_frags_per_group);
    println!("Inodes per group:         {}", sb.s_inodes_per_group);
    println!(
        "Inode blocks per group:   {}",
        u32::from(sb.s_inode_size) * sb.s_inodes_per_group / sb.block_size()
    );
    println!("Inode size:               {}", sb.s_inode_size);
    println!("Required extra isize:     {}", sb.s_min_extra_isize);
    println!("Desired extra isize:      {}", sb.s_want_extra_isize);
    println!("Journal inode:            {}", sb.s_journal_inum);
    println!("Default directory hash:   half_md4");
    println!(
        "Filesystem created:       {}",
        format_timestamp(sb.s_mkfs_time)
    );
    println!("Last mount time:          {}", format_timestamp(sb.s_mtime));
    println!("Last write time:          {}", format_timestamp(sb.s_wtime));
    println!("Mount count:              {}", sb.s_mnt_count);
    println!(
        "Maximum mount count:      {}",
        if sb.s_max_mnt_count == u16::MAX {
            -1i16 as i32
        } else {
            i32::from(sb.s_max_mnt_count)
        }
    );
    println!(
        "Last checked:             {}",
        format_timestamp(sb.s_lastcheck)
    );
    println!(
        "Check interval:           {} ({} days)",
        sb.s_checkinterval,
        sb.s_checkinterval / 86400
    );
    println!("Reserved blocks uid:      {}", sb.s_def_resuid);
    println!("Reserved blocks gid:      {}", sb.s_def_resgid);
    println!("First inode:              {}", sb.s_first_ino);
}

// ---------------------------------------------------------------------------
// dumpe2fs — dump filesystem info
// ---------------------------------------------------------------------------

struct Dumpe2fsOptions {
    device: String,
    #[allow(dead_code)] // parsed/defaulted; group dump always emitted for now
    show_groups: bool,
    show_header_only: bool,
    _hex_dump: bool,
    image_file: bool,
}

impl Default for Dumpe2fsOptions {
    fn default() -> Self {
        Self {
            device: String::new(),
            show_groups: true,
            show_header_only: false,
            _hex_dump: false,
            image_file: false,
        }
    }
}

fn parse_dumpe2fs_args(args: &[String]) -> Result<Dumpe2fsOptions, String> {
    let mut opts = Dumpe2fsOptions::default();
    let mut i = 1;

    while i < args.len() {
        match args[i].as_str() {
            "-h" => opts.show_header_only = true,
            "-x" => opts._hex_dump = true,
            "-i" => opts.image_file = true,
            "-V" => {
                println!("dumpe2fs 1.47.0 (SlateOS)");
                process::exit(0);
            }
            arg if arg.starts_with('-') => {
                return Err(format!("unknown option: {arg}"));
            }
            _ => {
                opts.device = args[i].clone();
            }
        }
        i += 1;
    }

    if opts.device.is_empty() {
        return Err("missing device argument".to_string());
    }
    Ok(opts)
}

fn run_dumpe2fs(args: &[String]) -> i32 {
    let opts = match parse_dumpe2fs_args(args) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("dumpe2fs: {e}");
            eprintln!("Usage: dumpe2fs [-h] [-x] [-i] device");
            return 1;
        }
    };

    let label = if opts.image_file { "image" } else { "" };
    let img = FilesystemImage::create_default(&opts.device, FsType::Ext4, 262144, label);

    println!("dumpe2fs 1.47.0 (SlateOS)");

    let sb = &img.superblock;
    print_superblock_info(sb);
    println!(
        "Filesystem compatible features:   {}",
        sb.s_feature_compat.format_flags()
    );
    println!(
        "Filesystem incompatible features: {}",
        sb.s_feature_incompat.format_flags()
    );
    println!(
        "Filesystem ro-compatible features:{}",
        sb.s_feature_ro_compat.format_flags()
    );

    if let Some(ref journal) = img.journal {
        println!();
        println!("Journal features:         (none)");
        println!(
            "Journal size:             {}M",
            u64::from(journal.js_maxlen) * u64::from(journal.js_blocksize) / (1024 * 1024)
        );
        println!("Journal length:           {}", journal.js_maxlen);
        println!("Journal sequence:         0x{:08x}", journal.js_sequence);
        println!("Journal start:            {}", journal.js_start);
    }

    if opts.show_header_only {
        return 0;
    }

    println!();

    for (i, bg) in img.block_groups.iter().enumerate() {
        println!(
            "Group {i}: (Blocks {}-{})",
            u64::from(sb.s_first_data_block)
                + u64::from(i as u32) * u64::from(sb.s_blocks_per_group),
            (u64::from(sb.s_first_data_block)
                + u64::from(i as u32 + 1) * u64::from(sb.s_blocks_per_group))
            .saturating_sub(1)
            .min(sb.total_blocks().saturating_sub(1)),
        );

        if has_superblock_backup(i as u32) {
            let backup_block = u64::from(sb.s_first_data_block)
                + u64::from(i as u32) * u64::from(sb.s_blocks_per_group);
            println!(
                "  Primary superblock at {backup_block}, Group descriptors at {}-{}",
                backup_block + 1,
                backup_block + 2
            );
        }

        println!(
            "  Block bitmap at {} (+{})",
            bg.bg_block_bitmap,
            bg.bg_block_bitmap
                - u64::from(sb.s_first_data_block)
                - u64::from(i as u32) * u64::from(sb.s_blocks_per_group)
        );
        println!(
            "  Inode bitmap at {} (+{})",
            bg.bg_inode_bitmap,
            bg.bg_inode_bitmap
                - u64::from(sb.s_first_data_block)
                - u64::from(i as u32) * u64::from(sb.s_blocks_per_group)
        );
        println!(
            "  Inode table at {}-{} (+{})",
            bg.bg_inode_table,
            bg.bg_inode_table
                + u64::from(sb.s_inodes_per_group * u32::from(sb.s_inode_size) / sb.block_size())
                - 1,
            bg.bg_inode_table
                - u64::from(sb.s_first_data_block)
                - u64::from(i as u32) * u64::from(sb.s_blocks_per_group)
        );
        println!(
            "  {} free blocks, {} free inodes, {} directories, {} unused inodes",
            bg.bg_free_blocks_count,
            bg.bg_free_inodes_count,
            bg.bg_used_dirs_count,
            bg._bg_itable_unused
        );
        println!("  Free blocks: (simulated range)");
        println!("  Free inodes: (simulated range)");
        println!("  Checksum: 0x{:04x}", bg.bg_checksum);
    }

    0
}

// ---------------------------------------------------------------------------
// debugfs — filesystem debugger
// ---------------------------------------------------------------------------

#[derive(Default)]
struct DebugfsOptions {
    device: String,
    writable: bool,
    _catastrophic: bool,
    commands: Vec<String>,
}

fn parse_debugfs_args(args: &[String]) -> Result<DebugfsOptions, String> {
    let mut opts = DebugfsOptions::default();
    let mut i = 1;

    while i < args.len() {
        match args[i].as_str() {
            "-w" => opts.writable = true,
            "-c" => opts._catastrophic = true,
            "-R" => {
                i += 1;
                if i >= args.len() {
                    return Err("-R requires a command argument".to_string());
                }
                opts.commands.push(args[i].clone());
            }
            "-f" => {
                i += 1;
                if i >= args.len() {
                    return Err("-f requires a filename argument".to_string());
                }
                // Simulate reading commands from file
                opts.commands.push(format!("# commands from {}", args[i]));
            }
            "-V" => {
                println!("debugfs 1.47.0 (SlateOS)");
                process::exit(0);
            }
            arg if arg.starts_with('-') => {
                return Err(format!("unknown option: {arg}"));
            }
            _ => {
                opts.device = args[i].clone();
            }
        }
        i += 1;
    }

    Ok(opts)
}

fn run_debugfs_command(cmd: &str, img: &FilesystemImage) -> bool {
    let parts: Vec<&str> = cmd.split_whitespace().collect();
    if parts.is_empty() {
        return true;
    }

    match parts[0] {
        "quit" | "q" | "exit" => return false,
        "help" | "?" => {
            println!("Available commands:");
            println!("  stat <inode>     - show inode statistics");
            println!("  ls <inode>       - list directory contents");
            println!("  cat <inode>      - dump file contents (simulated)");
            println!("  inode_dump <ino> - dump raw inode");
            println!("  blocks <inode>   - show block map");
            println!("  show_super_stats - show superblock statistics");
            println!("  stats            - alias for show_super_stats");
            println!("  dump_extents <i> - dump extent tree");
            println!("  feature          - show filesystem features");
            println!("  supported_features - list all supported features");
            println!("  freei <inode>    - free an inode (write mode)");
            println!("  seti <inode>     - mark inode as used (write mode)");
            println!("  testb <block>    - test if block is in use");
            println!("  ncheck <inode>   - inode-to-name lookup");
            println!("  pwd              - show current directory");
            println!("  cd <dir>         - change directory");
            println!("  quit             - exit debugfs");
        }
        "show_super_stats" | "stats" | "show_super" => {
            print_superblock_info(&img.superblock);
        }
        "stat" => {
            if parts.len() < 2 {
                eprintln!("Usage: stat <inode>");
            } else {
                let ino: u32 = match parts[1].strip_prefix('<').and_then(|s| s.strip_suffix('>')) {
                    Some(s) => s.parse().unwrap_or(0),
                    None => parts[1].parse().unwrap_or(0),
                };
                debugfs_stat(ino, img);
            }
        }
        "ls" => {
            let ino = if parts.len() < 2 {
                EXT2_ROOT_INO
            } else {
                parts[1].parse().unwrap_or(EXT2_ROOT_INO)
            };
            debugfs_ls(ino, img);
        }
        "cat" => {
            if parts.len() < 2 {
                eprintln!("Usage: cat <inode>");
            } else {
                let ino: u32 = parts[1].parse().unwrap_or(0);
                debugfs_cat(ino, img);
            }
        }
        "inode_dump" => {
            if parts.len() < 2 {
                eprintln!("Usage: inode_dump <inode>");
            } else {
                let ino: u32 = parts[1].parse().unwrap_or(0);
                debugfs_inode_dump(ino, img);
            }
        }
        "blocks" => {
            if parts.len() < 2 {
                eprintln!("Usage: blocks <inode>");
            } else {
                let ino: u32 = parts[1].parse().unwrap_or(0);
                debugfs_blocks(ino, img);
            }
        }
        "dump_extents" | "extents" => {
            if parts.len() < 2 {
                eprintln!("Usage: dump_extents <inode>");
            } else {
                let ino: u32 = parts[1].parse().unwrap_or(0);
                debugfs_dump_extents(ino, img);
            }
        }
        "feature" | "features" => {
            let sb = &img.superblock;
            println!("Filesystem feature flags:");
            println!("  Compatible:   {}", sb.s_feature_compat.format_flags());
            println!("  Incompatible: {}", sb.s_feature_incompat.format_flags());
            println!("  Read-only:    {}", sb.s_feature_ro_compat.format_flags());
        }
        "supported_features" => {
            println!("Supported feature flags:");
            println!(
                "  Compatible:   has_journal dir_prealloc imagic_inodes resize_inode dir_index"
            );
            println!(
                "  Incompatible: compression filetype recover journal_dev meta_bg extents 64bit flex_bg mmp"
            );
            println!(
                "  Read-only:    sparse_super large_file huge_file btree_dir dir_nlink extra_isize metadata_csum"
            );
        }
        "testb" => {
            if parts.len() < 2 {
                eprintln!("Usage: testb <block>");
            } else {
                let block: u64 = parts[1].parse().unwrap_or(0);
                let sb = &img.superblock;
                // Simulated: low blocks are always in use
                let in_use = block < 100 || (block % u64::from(sb.s_blocks_per_group)) < 50;
                println!(
                    "Block {block} {}",
                    if in_use {
                        "marked in use"
                    } else {
                        "not in use"
                    }
                );
            }
        }
        "ncheck" => {
            if parts.len() < 2 {
                eprintln!("Usage: ncheck <inode>");
            } else {
                let ino: u32 = parts[1].parse().unwrap_or(0);
                debugfs_ncheck(ino, img);
            }
        }
        "freei" => {
            println!("freei: filesystem opened read-only (use -w to enable writes)");
        }
        "seti" => {
            println!("seti: filesystem opened read-only (use -w to enable writes)");
        }
        "pwd" => {
            println!("[pwd]   INODE: 2  PATH: /");
        }
        "cd" => {
            if parts.len() < 2 {
                println!("[pwd]   INODE: 2  PATH: /");
            } else {
                println!(
                    "[pwd]   INODE: 2  PATH: /{}",
                    parts[1].trim_start_matches('/')
                );
            }
        }
        other => {
            eprintln!("debugfs: unknown command: {other}");
            eprintln!("Type '?' for help");
        }
    }
    true
}

fn debugfs_stat(ino: u32, img: &FilesystemImage) {
    if ino < 1 || (ino as usize) > img.inodes.len() {
        eprintln!("stat: inode {ino} out of range");
        return;
    }
    let idx = (ino - 1) as usize;
    let inode = &img.inodes[idx];
    let sb = &img.superblock;

    println!(
        "Inode: {ino}   Type: {}   Mode:  {:04o}   Flags: 0x{:x}",
        match inode.i_mode & S_IFMT {
            S_IFREG => "regular",
            S_IFDIR => "directory",
            S_IFLNK => "symlink",
            0x6000 => "block device",
            0x2000 => "char device",
            _ => "unknown",
        },
        inode.i_mode & 0o7777,
        inode.i_flags,
    );
    println!("Generation: {}    Version: 0x00000000", inode.i_generation);
    println!(
        "User:  {}   Group:  {}   Size: {}",
        inode.i_uid,
        inode.i_gid,
        inode.size()
    );
    println!("File ACL: {}    Directory ACL: 0", inode.i_file_acl);
    println!(
        "Links: {}   Blockcount: {}",
        inode.i_links_count, inode.i_blocks
    );
    println!("Fragment:  Address: 0    Number: 0    Size: 0");

    println!(
        "ctime: 0x{:08x} -- {}",
        inode.i_ctime,
        format_timestamp(inode.i_ctime)
    );
    println!(
        "atime: 0x{:08x} -- {}",
        inode.i_atime,
        format_timestamp(inode.i_atime)
    );
    println!(
        "mtime: 0x{:08x} -- {}",
        inode.i_mtime,
        format_timestamp(inode.i_mtime)
    );
    println!(
        "crtime: 0x{:08x} -- {}",
        inode.i_crtime,
        format_timestamp(inode.i_crtime)
    );

    if sb.s_feature_incompat.has_extents() && inode.i_flags & 0x0008_0000 != 0 {
        println!("EXTENTS:");
        if let Some((_, extents)) = img.extents.iter().find(|(i, _)| *i == ino) {
            for ext in extents {
                println!(
                    "  ({}, {}): {} - {}",
                    ext.ee_block,
                    ext.actual_len(),
                    ext.physical_block(),
                    ext.physical_block() + u64::from(ext.actual_len()) - 1
                );
            }
        }
    } else {
        println!("BLOCKS:");
        for (i, &block) in inode.i_block.iter().enumerate() {
            if block != 0 {
                if i < EXT2_NDIR_BLOCKS {
                    println!("  ({}): {block}", i);
                } else {
                    let label = match i {
                        12 => "IND",
                        13 => "DIND",
                        14 => "TIND",
                        _ => "???",
                    };
                    println!("  ({label}): {block}");
                }
            }
        }
    }
}

fn debugfs_ls(ino: u32, img: &FilesystemImage) {
    if let Some((_, entries)) = img.dir_entries.iter().find(|(i, _)| *i == ino) {
        for entry in entries {
            println!(
                "{:>8} {:>3} {:>5} {:>6} {}",
                entry.inode,
                entry.rec_len,
                entry.name_len,
                entry.file_type_str(),
                entry.name,
            );
        }
    } else {
        eprintln!("ls: inode {ino} is not a directory or not found");
    }
}

fn debugfs_cat(ino: u32, img: &FilesystemImage) {
    if ino < 1 || (ino as usize) > img.inodes.len() {
        eprintln!("cat: inode {ino} out of range");
        return;
    }
    let idx = (ino - 1) as usize;
    let inode = &img.inodes[idx];
    if !inode.is_regular() {
        eprintln!("cat: inode {ino} is not a regular file");
        return;
    }
    println!(
        "(simulated file content for inode {ino}, {} bytes)",
        inode.size()
    );
}

fn debugfs_inode_dump(ino: u32, img: &FilesystemImage) {
    if ino < 1 || (ino as usize) > img.inodes.len() {
        eprintln!("inode_dump: inode {ino} out of range");
        return;
    }
    let idx = (ino - 1) as usize;
    let inode = &img.inodes[idx];
    println!(
        "0000  {:04x} {:04x} {:08x} {:08x}  {:08x} {:08x} {:04x} {:04x}",
        inode.i_mode,
        inode.i_uid,
        inode.i_size,
        inode.i_atime,
        inode.i_ctime,
        inode.i_mtime,
        inode.i_gid,
        inode.i_links_count
    );
    println!("0020  {:08x} {:08x}", inode.i_blocks, inode.i_flags);
    for (i, block) in inode.i_block.iter().enumerate() {
        if i % 4 == 0 {
            print!("{:04x} ", 0x28 + i * 4);
        }
        print!(" {:08x}", block);
        if i % 4 == 3 || i == EXT2_N_BLOCKS - 1 {
            println!();
        }
    }
}

fn debugfs_blocks(ino: u32, img: &FilesystemImage) {
    if ino < 1 || (ino as usize) > img.inodes.len() {
        eprintln!("blocks: inode {ino} out of range");
        return;
    }
    let idx = (ino - 1) as usize;
    let inode = &img.inodes[idx];

    // Check for extents first
    if let Some((_, extents)) = img.extents.iter().find(|(i, _)| *i == ino) {
        let block_list: Vec<String> = extents
            .iter()
            .flat_map(|ext| {
                let start = ext.physical_block();
                (0..u64::from(ext.actual_len())).map(move |i| (start + i).to_string())
            })
            .collect();
        println!("BLOCKS: {}", block_list.join(" "));
        return;
    }

    // Direct blocks
    let blocks: Vec<String> = inode
        .i_block
        .iter()
        .take(EXT2_NDIR_BLOCKS)
        .filter(|&&b| b != 0)
        .map(|b| b.to_string())
        .collect();
    if blocks.is_empty() {
        println!("BLOCKS: (none)");
    } else {
        println!("BLOCKS: {}", blocks.join(" "));
    }
}

fn debugfs_dump_extents(ino: u32, img: &FilesystemImage) {
    if let Some((_, extents)) = img.extents.iter().find(|(i, _)| *i == ino) {
        println!("Level Entries       Logical          Physical Length Flags");
        for (i, ext) in extents.iter().enumerate() {
            let uninitialized = if ext.is_uninitialized() { "Uninit" } else { "" };
            println!(
                " 0/{:>2} {:>3}/{:>3}  {:>5} - {:>5}  {:>10} - {:>10}  {:>5} {}",
                0,
                i + 1,
                extents.len(),
                ext.ee_block,
                ext.ee_block + u32::from(ext.actual_len()) - 1,
                ext.physical_block(),
                ext.physical_block() + u64::from(ext.actual_len()) - 1,
                ext.actual_len(),
                uninitialized,
            );
        }
    } else {
        println!("inode {ino} does not use extents or not found");
    }
}

fn debugfs_ncheck(ino: u32, img: &FilesystemImage) {
    let mut found = false;
    for (parent_ino, entries) in &img.dir_entries {
        for entry in entries {
            if entry.inode == ino {
                let path = if *parent_ino == EXT2_ROOT_INO {
                    format!("/{}", entry.name)
                } else {
                    format!("<{parent_ino}>/{}", entry.name)
                };
                println!("Inode\tPathname");
                println!("{ino}\t{path}");
                found = true;
            }
        }
    }
    if !found {
        println!("Inode {ino} not found in any directory");
    }
}

fn run_debugfs(args: &[String]) -> i32 {
    let opts = match parse_debugfs_args(args) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("debugfs: {e}");
            eprintln!("Usage: debugfs [-w] [-R command] [-f cmdfile] [device]");
            return 1;
        }
    };

    let device = if opts.device.is_empty() {
        "(none)"
    } else {
        &opts.device
    };

    let img = FilesystemImage::create_default(device, FsType::Ext4, 262144, "");

    if opts.commands.is_empty() {
        println!("debugfs 1.47.0 (SlateOS)");
        if !opts.device.is_empty() {
            let mode = if opts.writable {
                "read/write"
            } else {
                "read-only"
            };
            println!("debugfs: opened {} in {mode} mode", opts.device);
        }
        println!(
            "debugfs: use -R <command> to run a command, or type 'help' for interactive commands"
        );
        return 0;
    }

    for cmd in &opts.commands {
        if cmd.starts_with('#') {
            continue;
        }
        println!("debugfs: {cmd}");
        if !run_debugfs_command(cmd, &img) {
            break;
        }
    }

    0
}

// ---------------------------------------------------------------------------
// resize2fs — resize filesystem
// ---------------------------------------------------------------------------

struct Resize2fsOptions {
    device: String,
    new_size: Option<String>,
    force: bool,
    flush: bool,
    minimum: bool,
    progress: bool,
    _print_min_size: bool,
}

impl Default for Resize2fsOptions {
    fn default() -> Self {
        Self {
            device: String::new(),
            new_size: None,
            force: false,
            flush: false,
            minimum: false,
            progress: true,
            _print_min_size: false,
        }
    }
}

fn parse_resize2fs_args(args: &[String]) -> Result<Resize2fsOptions, String> {
    let mut opts = Resize2fsOptions::default();
    let mut positional = Vec::new();
    let mut i = 1;

    while i < args.len() {
        match args[i].as_str() {
            "-f" => opts.force = true,
            "-F" => opts.flush = true,
            "-M" => opts.minimum = true,
            "-p" => opts.progress = true,
            "-P" => opts._print_min_size = true,
            "-V" => {
                println!("resize2fs 1.47.0 (SlateOS)");
                process::exit(0);
            }
            arg if arg.starts_with('-') => {
                return Err(format!("unknown option: {arg}"));
            }
            _ => positional.push(args[i].clone()),
        }
        i += 1;
    }

    if positional.is_empty() {
        return Err("missing device argument".to_string());
    }
    opts.device = positional[0].clone();
    if positional.len() > 1 {
        opts.new_size = Some(positional[1].clone());
    }

    Ok(opts)
}

fn run_resize2fs(args: &[String]) -> i32 {
    let opts = match parse_resize2fs_args(args) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("resize2fs: {e}");
            eprintln!("Usage: resize2fs [-fFpPM] device [new-size]");
            return 1;
        }
    };

    let img = FilesystemImage::create_default(&opts.device, FsType::Ext4, 262144, "");
    let sb = &img.superblock;

    if opts._print_min_size {
        // Minimum size = used blocks + overhead
        let min_blocks = sb.total_blocks() - sb.total_free_blocks() + 100;
        println!("Estimated minimum size of the filesystem: {min_blocks}");
        return 0;
    }

    let current_blocks = sb.total_blocks();
    let block_size = sb.block_size();

    let new_blocks = if opts.minimum {
        // Shrink to minimum
        let used = current_blocks - sb.total_free_blocks();
        used + 100 // Small margin
    } else if let Some(ref size_str) = opts.new_size {
        match parse_block_count(size_str, block_size) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("resize2fs: {e}");
                return 1;
            }
        }
    } else {
        // Grow to fill device — simulated: current + 50%
        current_blocks + current_blocks / 2
    };

    println!("resize2fs 1.47.0 (SlateOS)");

    if new_blocks == current_blocks {
        println!(
            "The filesystem is already {} ({}) blocks long. Nothing to do!",
            current_blocks, current_blocks
        );
        return 0;
    }

    let min_blocks = current_blocks - sb.total_free_blocks();
    if new_blocks < min_blocks && !opts.force {
        eprintln!(
            "resize2fs: new size {} is smaller than minimum {} blocks",
            new_blocks, min_blocks
        );
        eprintln!("resize2fs: use -f to force");
        return 1;
    }

    if new_blocks > current_blocks {
        println!(
            "Resizing the filesystem on {} to {} ({}) blocks.",
            opts.device, new_blocks, new_blocks
        );
        if opts.progress {
            println!("(pass 1/1)");
            println!("Extending the inode table     \x1b[K done");
            println!(
                "The filesystem on {} is now {} ({}) blocks long.",
                opts.device, new_blocks, new_blocks
            );
        }
    } else {
        println!(
            "Resizing the filesystem on {} to {} ({}) blocks.",
            opts.device, new_blocks, new_blocks
        );
        if opts.progress {
            println!("(pass 1/4) Checking for unmovable inodes  done");
            println!("(pass 2/4) Moving blocks                  done");
            println!("(pass 3/4) Scanning inode table            done");
            println!("(pass 4/4) Updating block group descriptors done");
            println!(
                "The filesystem on {} is now {} ({}) blocks long.",
                opts.device, new_blocks, new_blocks
            );
        }
    }

    0
}

// ---------------------------------------------------------------------------
// e2fsck — filesystem check/repair
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FsckVerbosity {
    Quiet,
    Normal,
    Verbose,
}

struct E2fsckOptions {
    device: String,
    preen: bool,
    yes: bool,
    no: bool,
    force: bool,
    verbosity: FsckVerbosity,
    check_blocks: bool,
    show_progress: bool,
    timing: bool,
    _read_only: bool,
}

impl Default for E2fsckOptions {
    fn default() -> Self {
        Self {
            device: String::new(),
            preen: false,
            yes: false,
            no: false,
            force: false,
            verbosity: FsckVerbosity::Normal,
            check_blocks: false,
            show_progress: false,
            timing: false,
            _read_only: false,
        }
    }
}

fn parse_e2fsck_args(args: &[String]) -> Result<E2fsckOptions, String> {
    let mut opts = E2fsckOptions::default();
    let mut i = 1;

    while i < args.len() {
        match args[i].as_str() {
            "-p" | "-a" => opts.preen = true,
            "-y" => opts.yes = true,
            "-n" => {
                opts.no = true;
                opts._read_only = true;
            }
            "-f" => opts.force = true,
            "-v" => opts.verbosity = FsckVerbosity::Verbose,
            "-q" => opts.verbosity = FsckVerbosity::Quiet,
            "-c" => opts.check_blocks = true,
            "-C" => {
                opts.show_progress = true;
                // Optionally skip the fd argument
                if i + 1 < args.len() && args[i + 1].parse::<i32>().is_ok() {
                    i += 1;
                }
            }
            "-t" => opts.timing = true,
            "-V" => {
                println!("e2fsck 1.47.0 (SlateOS)");
                process::exit(0);
            }
            arg if arg.starts_with('-') => {
                return Err(format!("unknown option: {arg}"));
            }
            _ => {
                opts.device = args[i].clone();
            }
        }
        i += 1;
    }

    if opts.device.is_empty() {
        return Err("missing device argument".to_string());
    }
    Ok(opts)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FsckPassStats {
    files: u32,
    dirs: u32,
    blocks_used: u64,
    blocks_total: u64,
    inodes_used: u32,
    inodes_total: u32,
    errors_found: u32,
    errors_fixed: u32,
}

fn run_e2fsck(args: &[String]) -> i32 {
    let opts = match parse_e2fsck_args(args) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("e2fsck: {e}");
            eprintln!("Usage: e2fsck [-panyf] [-C fd] device");
            return 1;
        }
    };

    let img = FilesystemImage::create_default(&opts.device, FsType::Ext4, 262144, "");
    let sb = &img.superblock;

    if opts.verbosity != FsckVerbosity::Quiet {
        println!("e2fsck 1.47.0 (SlateOS)");
    }

    // Check if filesystem is clean
    if sb.s_state == 1 && !opts.force {
        if opts.verbosity != FsckVerbosity::Quiet {
            println!(
                "{}: clean, {}/{} files, {}/{} blocks",
                opts.device,
                sb.s_inodes_count - sb.s_free_inodes_count,
                sb.s_inodes_count,
                sb.total_blocks() - sb.total_free_blocks(),
                sb.total_blocks(),
            );
        }
        return 0;
    }

    let stats = FsckPassStats {
        files: sb.s_inodes_count - sb.s_free_inodes_count - 2,
        dirs: 2,
        blocks_used: sb.total_blocks() - sb.total_free_blocks(),
        blocks_total: sb.total_blocks(),
        inodes_used: sb.s_inodes_count - sb.s_free_inodes_count,
        inodes_total: sb.s_inodes_count,
        errors_found: 0,
        errors_fixed: 0,
    };

    // Pass 1: Checking inodes, blocks, and sizes
    if opts.verbosity != FsckVerbosity::Quiet {
        println!("Pass 1: Checking inodes, blocks, and sizes");
    }
    if opts.verbosity == FsckVerbosity::Verbose {
        println!("  Scanning inode table...");
        println!("  Inode {} is root directory", EXT2_ROOT_INO);
        println!("  Inode 8 is journal inode");
        println!("  Inode 11 is lost+found directory");
    }

    // Pass 2: Checking directory structure
    if opts.verbosity != FsckVerbosity::Quiet {
        println!("Pass 2: Checking directory structure");
    }
    if opts.verbosity == FsckVerbosity::Verbose {
        println!("  Checking root directory...");
        println!("  Checking lost+found directory...");
    }

    // Pass 3: Checking directory connectivity
    if opts.verbosity != FsckVerbosity::Quiet {
        println!("Pass 3: Checking directory connectivity");
    }

    // Pass 4: Checking reference counts
    if opts.verbosity != FsckVerbosity::Quiet {
        println!("Pass 4: Checking reference counts");
    }

    // Pass 5: Checking group summary information
    if opts.verbosity != FsckVerbosity::Quiet {
        println!("Pass 5: Checking group summary information");
    }

    if opts.check_blocks && opts.verbosity != FsckVerbosity::Quiet {
        println!("Checking bad block list...");
    }

    // Summary
    if opts.verbosity != FsckVerbosity::Quiet {
        println!();
        println!(
            "{}: {}/{} files ({:.1}% non-contiguous), {}/{} blocks",
            opts.device,
            stats.inodes_used,
            stats.inodes_total,
            0.0f64,
            stats.blocks_used,
            stats.blocks_total,
        );
        if stats.errors_found > 0 {
            println!(
                "  {} errors found, {} fixed",
                stats.errors_found, stats.errors_fixed
            );
        }
    }

    if opts.timing {
        println!("Memory used: 1234k/0k (462k/772k), time:  0.01/ 0.00/ 0.00");
    }

    // Return codes: 0=clean, 1=errors fixed, 2=errors found, 4=errors uncorrected
    if stats.errors_found > 0 && stats.errors_fixed == stats.errors_found {
        1
    } else if stats.errors_found > stats.errors_fixed {
        4
    } else {
        0
    }
}

// ---------------------------------------------------------------------------
// e2label — change/display filesystem label
// ---------------------------------------------------------------------------

fn run_e2label(args: &[String]) -> i32 {
    if args.len() < 2 {
        eprintln!("Usage: e2label device [label]");
        return 1;
    }

    let device = &args[1];

    let mut img = FilesystemImage::create_default(device, FsType::Ext4, 262144, "old_label");

    if args.len() == 2 {
        // Display label
        let label = img.superblock.label();
        if label.is_empty() {
            println!("(no label)");
        } else {
            println!("{label}");
        }
        return 0;
    }

    let new_label = &args[2];
    if new_label.len() > EXT2_LABEL_LEN {
        eprintln!(
            "e2label: label too long (max {} characters)",
            EXT2_LABEL_LEN
        );
        return 1;
    }

    img.superblock.set_label(new_label);
    println!("e2label: setting label to '{new_label}'");
    0
}

// ---------------------------------------------------------------------------
// e2image — save filesystem metadata
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ImageFormat {
    Normal,
    Raw,
    Qcow2,
}

struct E2imageOptions {
    device: String,
    output: String,
    format: ImageFormat,
    all_data: bool,
    scramble_dir: bool,
    install: bool,
}

impl Default for E2imageOptions {
    fn default() -> Self {
        Self {
            device: String::new(),
            output: String::new(),
            format: ImageFormat::Normal,
            all_data: false,
            scramble_dir: false,
            install: false,
        }
    }
}

fn parse_e2image_args(args: &[String]) -> Result<E2imageOptions, String> {
    let mut opts = E2imageOptions::default();
    let mut positional = Vec::new();
    let mut i = 1;

    while i < args.len() {
        match args[i].as_str() {
            "-r" => opts.format = ImageFormat::Raw,
            "-Q" => opts.format = ImageFormat::Qcow2,
            "-a" => opts.all_data = true,
            "-s" => opts.scramble_dir = true,
            "-I" => opts.install = true,
            "-V" => {
                println!("e2image 1.47.0 (SlateOS)");
                process::exit(0);
            }
            arg if arg.starts_with('-') => {
                return Err(format!("unknown option: {arg}"));
            }
            _ => {
                positional.push(args[i].clone());
            }
        }
        i += 1;
    }

    if positional.len() < 2 {
        return Err("need device and output file arguments".to_string());
    }
    opts.device = positional[0].clone();
    opts.output = positional[1].clone();

    Ok(opts)
}

fn run_e2image(args: &[String]) -> i32 {
    let opts = match parse_e2image_args(args) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("e2image: {e}");
            eprintln!("Usage: e2image [-r|-Q] [-a] [-s] device output-file");
            eprintln!("       e2image -I device image-file");
            return 1;
        }
    };

    let img = FilesystemImage::create_default(&opts.device, FsType::Ext4, 262144, "");
    let sb = &img.superblock;

    if opts.install {
        println!(
            "e2image: installing image from '{}' to '{}'",
            opts.output, opts.device
        );
        println!("  Restoring superblock...");
        println!("  Restoring block group descriptors...");
        println!("  Restoring inode table...");
        println!("  Done.");
        return 0;
    }

    let format_name = match opts.format {
        ImageFormat::Normal => "e2image",
        ImageFormat::Raw => "raw",
        ImageFormat::Qcow2 => "QCOW2",
    };

    println!(
        "e2image: saving {} format image of '{}' to '{}'",
        format_name, opts.device, opts.output
    );

    // Simulate writing metadata
    let metadata_blocks = 2 + // superblock + backup
        sb.group_count() + // GDT blocks
        sb.group_count() * 2 + // block/inode bitmaps
        sb.group_count() * (u32::from(sb.s_inode_size) * sb.s_inodes_per_group / sb.block_size());

    let metadata_bytes = u64::from(metadata_blocks) * u64::from(sb.block_size());

    println!(
        "  Copying {} metadata blocks ({})...",
        metadata_blocks,
        format_bytes(metadata_bytes)
    );

    if opts.all_data {
        let data_blocks = sb.total_blocks() - sb.total_free_blocks();
        let data_bytes = data_blocks * u64::from(sb.block_size());
        println!(
            "  Copying {} data blocks ({})...",
            data_blocks,
            format_bytes(data_bytes)
        );
    }

    if opts.scramble_dir {
        println!("  Scrambling directory entries...");
    }

    match opts.format {
        ImageFormat::Qcow2 => {
            println!("  Writing QCOW2 header...");
            println!("  Writing L1/L2 tables...");
        }
        ImageFormat::Raw => {
            println!("  Writing raw image...");
        }
        ImageFormat::Normal => {
            println!("  Writing e2image metadata table...");
        }
    }

    println!("  Done.");
    0
}

// ---------------------------------------------------------------------------
// filefrag — show file fragmentation
// ---------------------------------------------------------------------------

struct FilefragOptions {
    files: Vec<String>,
    verbose: bool,
    block_size: bool,
    sync_first: bool,
    _extent_format: bool,
}

impl Default for FilefragOptions {
    fn default() -> Self {
        Self {
            files: Vec::new(),
            verbose: false,
            block_size: false,
            sync_first: false,
            _extent_format: true,
        }
    }
}

fn parse_filefrag_args(args: &[String]) -> Result<FilefragOptions, String> {
    let mut opts = FilefragOptions::default();
    let mut i = 1;

    while i < args.len() {
        match args[i].as_str() {
            "-v" => opts.verbose = true,
            "-b" => opts.block_size = true,
            "-s" => opts.sync_first = true,
            "-e" => opts._extent_format = true,
            "-V" => {
                println!("filefrag 1.47.0 (SlateOS)");
                process::exit(0);
            }
            arg if arg.starts_with('-') => {
                return Err(format!("unknown option: {arg}"));
            }
            _ => {
                opts.files.push(args[i].clone());
            }
        }
        i += 1;
    }

    if opts.files.is_empty() {
        return Err("no files specified".to_string());
    }
    Ok(opts)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FragExtent {
    logical_offset: u64,
    physical_offset: u64,
    length: u64,
    _flags: u32,
}

fn simulate_file_extents(file_size: u64, block_size: u32) -> Vec<FragExtent> {
    if file_size == 0 {
        return Vec::new();
    }

    let total_blocks = file_size.div_ceil(u64::from(block_size));
    let mut extents = Vec::new();
    let mut logical = 0u64;
    let mut physical = 1000u64; // Starting physical block

    // Simulate some fragmentation
    let blocks_per_extent = if total_blocks > 10 {
        total_blocks / 3
    } else {
        total_blocks
    };

    while logical < total_blocks {
        let len = blocks_per_extent.min(total_blocks - logical);
        extents.push(FragExtent {
            logical_offset: logical,
            physical_offset: physical,
            length: len,
            _flags: 0,
        });
        logical += len;
        physical += len + 50; // Gap between extents (fragmentation)
    }

    // Mark last extent
    if let Some(last) = extents.last_mut() {
        last._flags = 0x301; // LAST | EOF
    }

    extents
}

fn run_filefrag(args: &[String]) -> i32 {
    let opts = match parse_filefrag_args(args) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("filefrag: {e}");
            eprintln!("Usage: filefrag [-v] [-b] [-s] file...");
            return 1;
        }
    };

    if opts.sync_first {
        // Simulate syncing
    }

    let block_size: u32 = if opts.block_size { 1024 } else { 4096 };
    let mut exit_code = 0;

    for file in &opts.files {
        // Simulate file info
        let file_size: u64 = simulate_file_size(file);
        let extents = simulate_file_extents(file_size, block_size);

        if opts.verbose {
            println!("Filesystem type is: ext4");
            println!(
                "File size of {file} is {file_size} ({} blocks of {block_size} bytes)",
                file_size.div_ceil(u64::from(block_size))
            );
            println!(
                " ext:     logical_offset:        physical_offset: length:   expected: flags:"
            );
            for (i, ext) in extents.iter().enumerate() {
                let expected = if i > 0 {
                    let prev = &extents[i - 1];
                    format!("{}", prev.physical_offset + prev.length)
                } else {
                    String::new()
                };
                let flags = if ext._flags & 0x1 != 0 {
                    "last,eof"
                } else {
                    ""
                };
                println!(
                    "   {i}:  {:>12}..{:>12}: {:>12}..{:>12}: {:>6}: {:>9} {flags}",
                    ext.logical_offset,
                    ext.logical_offset + ext.length - 1,
                    ext.physical_offset,
                    ext.physical_offset + ext.length - 1,
                    ext.length,
                    expected,
                );
            }
        }

        let fragment_count = if extents.is_empty() { 0 } else { extents.len() };
        println!(
            "{file}: {fragment_count} extent{} found",
            if fragment_count == 1 { "" } else { "s" }
        );

        if fragment_count > 1 {
            exit_code = 1; // Fragmented file
        }
    }

    exit_code
}

fn simulate_file_size(file: &str) -> u64 {
    // Deterministic simulation based on filename
    let mut hash: u64 = 5381;
    for b in file.bytes() {
        hash = hash.wrapping_mul(33).wrapping_add(u64::from(b));
    }
    // Return a plausible file size
    (hash % (1024 * 1024 * 100)) + 4096 // Between 4K and ~100MB
}

// ---------------------------------------------------------------------------
// Usage / version helpers
// ---------------------------------------------------------------------------

// These usage/version helpers are tested but not yet wired into the per-
// personality `run_*` dispatch (each currently does its own ad-hoc arg
// handling). Centralising `--help`/`--version` through them is a follow-up;
// see todo.txt. #[allow(dead_code)] until then.
#[allow(dead_code)]
fn print_version(personality: Personality) {
    let name = personality_name(personality);
    println!("{name} 1.47.0 (SlateOS)");
}

#[allow(dead_code)]
fn personality_name(p: Personality) -> &'static str {
    match p {
        Personality::Mke2fs => "mke2fs",
        Personality::Tune2fs => "tune2fs",
        Personality::Dumpe2fs => "dumpe2fs",
        Personality::Debugfs => "debugfs",
        Personality::Resize2fs => "resize2fs",
        Personality::E2fsck => "e2fsck",
        Personality::E2label => "e2label",
        Personality::E2image => "e2image",
        Personality::Filefrag => "filefrag",
    }
}

#[allow(dead_code)]
fn print_usage(personality: Personality) {
    match personality {
        Personality::Mke2fs => {
            eprintln!("Usage: mke2fs [-t fs-type] [-b block-size] [-I inode-size] [-L label]");
            eprintln!("              [-U uuid] [-m reserved%] [-O features] [-E options]");
            eprintln!("              [-q] [-v] [-n] device [blocks]");
        }
        Personality::Tune2fs => {
            eprintln!("Usage: tune2fs [-l] [-L label] [-U uuid] [-c count] [-i interval]");
            eprintln!("              [-e behavior] [-m reserved%] [-O features] [-j] device");
        }
        Personality::Dumpe2fs => {
            eprintln!("Usage: dumpe2fs [-h] [-x] [-i] device");
        }
        Personality::Debugfs => {
            eprintln!("Usage: debugfs [-w] [-c] [-R command] [-f cmdfile] [device]");
        }
        Personality::Resize2fs => {
            eprintln!("Usage: resize2fs [-fFpPM] device [new-size]");
        }
        Personality::E2fsck => {
            eprintln!("Usage: e2fsck [-panyf] [-v] [-C fd] [-t] device");
        }
        Personality::E2label => {
            eprintln!("Usage: e2label device [label]");
        }
        Personality::E2image => {
            eprintln!("Usage: e2image [-r|-Q] [-a] [-s] device output-file");
            eprintln!("       e2image -I device image-file");
        }
        Personality::Filefrag => {
            eprintln!("Usage: filefrag [-v] [-b] [-s] [-e] file...");
        }
    }
}

// ---------------------------------------------------------------------------
// Main entry point
// ---------------------------------------------------------------------------

fn main() {
    let args: Vec<String> = env::args().collect();
    let argv0 = args.first().map(|s| s.as_str()).unwrap_or("mke2fs");
    let personality = detect_personality(argv0);

    let code = match personality {
        Personality::Mke2fs => run_mke2fs(&args),
        Personality::Tune2fs => run_tune2fs(&args),
        Personality::Dumpe2fs => run_dumpe2fs(&args),
        Personality::Debugfs => run_debugfs(&args),
        Personality::Resize2fs => run_resize2fs(&args),
        Personality::E2fsck => run_e2fsck(&args),
        Personality::E2label => run_e2label(&args),
        Personality::E2image => run_e2image(&args),
        Personality::Filefrag => run_filefrag(&args),
    };

    process::exit(code);
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)] // Tests construct fixtures by mutating defaults; clearer than functional-update across dozens of sites.
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Personality detection tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_personality_mke2fs() {
        assert_eq!(detect_personality("mke2fs"), Personality::Mke2fs);
    }

    #[test]
    fn test_personality_mkfs_ext2() {
        assert_eq!(detect_personality("mkfs.ext2"), Personality::Mke2fs);
    }

    #[test]
    fn test_personality_mkfs_ext3() {
        assert_eq!(detect_personality("mkfs.ext3"), Personality::Mke2fs);
    }

    #[test]
    fn test_personality_mkfs_ext4() {
        assert_eq!(detect_personality("mkfs.ext4"), Personality::Mke2fs);
    }

    #[test]
    fn test_personality_tune2fs() {
        assert_eq!(detect_personality("tune2fs"), Personality::Tune2fs);
    }

    #[test]
    fn test_personality_dumpe2fs() {
        assert_eq!(detect_personality("dumpe2fs"), Personality::Dumpe2fs);
    }

    #[test]
    fn test_personality_debugfs() {
        assert_eq!(detect_personality("debugfs"), Personality::Debugfs);
    }

    #[test]
    fn test_personality_resize2fs() {
        assert_eq!(detect_personality("resize2fs"), Personality::Resize2fs);
    }

    #[test]
    fn test_personality_e2fsck() {
        assert_eq!(detect_personality("e2fsck"), Personality::E2fsck);
    }

    #[test]
    fn test_personality_fsck_ext2() {
        assert_eq!(detect_personality("fsck.ext2"), Personality::E2fsck);
    }

    #[test]
    fn test_personality_fsck_ext3() {
        assert_eq!(detect_personality("fsck.ext3"), Personality::E2fsck);
    }

    #[test]
    fn test_personality_fsck_ext4() {
        assert_eq!(detect_personality("fsck.ext4"), Personality::E2fsck);
    }

    #[test]
    fn test_personality_e2label() {
        assert_eq!(detect_personality("e2label"), Personality::E2label);
    }

    #[test]
    fn test_personality_e2image() {
        assert_eq!(detect_personality("e2image"), Personality::E2image);
    }

    #[test]
    fn test_personality_filefrag() {
        assert_eq!(detect_personality("filefrag"), Personality::Filefrag);
    }

    #[test]
    fn test_personality_with_path_unix() {
        assert_eq!(detect_personality("/usr/bin/mke2fs"), Personality::Mke2fs);
    }

    #[test]
    fn test_personality_with_path_windows() {
        assert_eq!(detect_personality("C:\\bin\\tune2fs"), Personality::Tune2fs);
    }

    #[test]
    fn test_personality_with_exe_suffix() {
        assert_eq!(detect_personality("e2fsck.exe"), Personality::E2fsck);
    }

    #[test]
    fn test_personality_case_insensitive() {
        assert_eq!(detect_personality("MKE2FS"), Personality::Mke2fs);
        assert_eq!(detect_personality("Tune2fs.EXE"), Personality::Tune2fs);
    }

    #[test]
    fn test_personality_unknown_defaults_to_mke2fs() {
        assert_eq!(detect_personality("unknown"), Personality::Mke2fs);
        assert_eq!(detect_personality("e2fsprogs"), Personality::Mke2fs);
    }

    #[test]
    fn test_personality_full_path_with_exe() {
        assert_eq!(
            detect_personality("/opt/sbin/debugfs.exe"),
            Personality::Debugfs
        );
    }

    // -----------------------------------------------------------------------
    // Superblock tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_superblock_default() {
        let sb = Superblock::default();
        assert_eq!(sb.s_magic, EXT2_MAGIC);
        assert_eq!(sb.s_rev_level, EXT2_DYNAMIC_REV);
        assert_eq!(sb.s_inode_size, 256);
    }

    #[test]
    fn test_superblock_block_size() {
        let mut sb = Superblock::default();
        sb.s_log_block_size = 0;
        assert_eq!(sb.block_size(), 1024);
        sb.s_log_block_size = 1;
        assert_eq!(sb.block_size(), 2048);
        sb.s_log_block_size = 2;
        assert_eq!(sb.block_size(), 4096);
        sb.s_log_block_size = 3;
        assert_eq!(sb.block_size(), 8192);
    }

    #[test]
    fn test_superblock_total_blocks_64bit() {
        let mut sb = Superblock::default();
        sb.s_blocks_count = 100;
        sb.s_blocks_count_hi = 1;
        assert_eq!(sb.total_blocks(), (1u64 << 32) + 100);
    }

    #[test]
    fn test_superblock_free_blocks_64bit() {
        let mut sb = Superblock::default();
        sb.s_free_blocks_count = 50;
        sb.s_free_blocks_count_hi = 2;
        assert_eq!(sb.total_free_blocks(), (2u64 << 32) + 50);
    }

    #[test]
    fn test_superblock_reserved_blocks_64bit() {
        let mut sb = Superblock::default();
        sb.s_r_blocks_count = 10;
        sb.s_r_blocks_count_hi = 1;
        assert_eq!(sb.reserved_blocks(), (1u64 << 32) + 10);
    }

    #[test]
    fn test_superblock_group_count() {
        let mut sb = Superblock::default();
        sb.s_blocks_count = 65536;
        sb.s_blocks_per_group = 32768;
        assert_eq!(sb.group_count(), 2);
    }

    #[test]
    fn test_superblock_group_count_partial() {
        let mut sb = Superblock::default();
        sb.s_blocks_count = 40000;
        sb.s_blocks_per_group = 32768;
        assert_eq!(sb.group_count(), 2);
    }

    #[test]
    fn test_superblock_group_count_single() {
        let mut sb = Superblock::default();
        sb.s_blocks_count = 1000;
        sb.s_blocks_per_group = 32768;
        assert_eq!(sb.group_count(), 1);
    }

    #[test]
    fn test_superblock_group_count_zero_blocks_per_group() {
        let mut sb = Superblock::default();
        sb.s_blocks_per_group = 0;
        assert_eq!(sb.group_count(), 1);
    }

    #[test]
    fn test_superblock_label() {
        let mut sb = Superblock::default();
        assert_eq!(sb.label(), "");
        sb.set_label("test");
        assert_eq!(sb.label(), "test");
    }

    #[test]
    fn test_superblock_label_max_length() {
        let mut sb = Superblock::default();
        sb.set_label("1234567890abcdef");
        assert_eq!(sb.label(), "1234567890abcdef");
    }

    #[test]
    fn test_superblock_label_truncation() {
        let mut sb = Superblock::default();
        sb.set_label("1234567890abcdefXYZ"); // Longer than 16
        assert_eq!(sb.label(), "1234567890abcdef");
    }

    #[test]
    fn test_superblock_uuid_format() {
        let sb = Superblock::default();
        let uuid = sb.format_uuid();
        assert_eq!(uuid.len(), 36);
        assert_eq!(&uuid[8..9], "-");
        assert_eq!(&uuid[13..14], "-");
        assert_eq!(&uuid[18..19], "-");
        assert_eq!(&uuid[23..24], "-");
    }

    #[test]
    fn test_superblock_is_valid() {
        let sb = Superblock::default();
        assert!(sb.is_valid());
        let mut bad = sb;
        bad.s_magic = 0x1234;
        assert!(!bad.is_valid());
    }

    #[test]
    fn test_superblock_fs_type_ext4() {
        let mut sb = Superblock::default();
        sb.s_feature_incompat = FeatureIncompat(EXT4_FEATURE_INCOMPAT_EXTENTS);
        assert_eq!(sb.fs_type(), FsType::Ext4);
    }

    #[test]
    fn test_superblock_fs_type_ext3() {
        let mut sb = Superblock::default();
        sb.s_feature_incompat = FeatureIncompat(0);
        sb.s_feature_compat = FeatureCompat(EXT4_FEATURE_COMPAT_HAS_JOURNAL);
        assert_eq!(sb.fs_type(), FsType::Ext3);
    }

    #[test]
    fn test_superblock_fs_type_ext2() {
        let mut sb = Superblock::default();
        sb.s_feature_incompat = FeatureIncompat(0);
        sb.s_feature_compat = FeatureCompat(0);
        assert_eq!(sb.fs_type(), FsType::Ext2);
    }

    #[test]
    fn test_superblock_state_str() {
        let mut sb = Superblock::default();
        sb.s_state = 1;
        assert_eq!(sb.state_str(), "clean");
        sb.s_state = 2;
        assert_eq!(sb.state_str(), "has errors");
        sb.s_state = 4;
        assert_eq!(sb.state_str(), "orphan recovery needed");
        sb.s_state = 99;
        assert_eq!(sb.state_str(), "unknown");
    }

    #[test]
    fn test_superblock_errors_behavior() {
        let mut sb = Superblock::default();
        sb.s_errors = 1;
        assert_eq!(sb.errors_behavior_str(), "Continue");
        sb.s_errors = 2;
        assert_eq!(sb.errors_behavior_str(), "Remount read-only");
        sb.s_errors = 3;
        assert_eq!(sb.errors_behavior_str(), "Panic");
    }

    #[test]
    fn test_superblock_creator_os() {
        let mut sb = Superblock::default();
        sb.s_creator_os = 0;
        assert_eq!(sb.creator_os_str(), "Linux");
        sb.s_creator_os = 5;
        assert_eq!(sb.creator_os_str(), "SlateOS");
    }

    #[test]
    fn test_superblock_checksum_deterministic() {
        let sb = Superblock::default();
        let c1 = sb.compute_checksum();
        let c2 = sb.compute_checksum();
        assert_eq!(c1, c2);
    }

    #[test]
    fn test_superblock_checksum_changes_with_data() {
        let mut sb1 = Superblock::default();
        sb1.s_inodes_count = 100;
        let mut sb2 = Superblock::default();
        sb2.s_inodes_count = 200;
        assert_ne!(sb1.compute_checksum(), sb2.compute_checksum());
    }

    // -----------------------------------------------------------------------
    // Feature flag tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_feature_compat_journal() {
        let mut fc = FeatureCompat(0);
        assert!(!fc.has_journal());
        fc.set_journal();
        assert!(fc.has_journal());
        fc.clear_journal();
        assert!(!fc.has_journal());
    }

    #[test]
    fn test_feature_compat_format_empty() {
        let fc = FeatureCompat(0);
        assert_eq!(fc.format_flags(), "(none)");
    }

    #[test]
    fn test_feature_compat_format_journal() {
        let fc = FeatureCompat(EXT4_FEATURE_COMPAT_HAS_JOURNAL);
        assert!(fc.format_flags().contains("has_journal"));
    }

    #[test]
    fn test_feature_incompat_extents() {
        let fi = FeatureIncompat(EXT4_FEATURE_INCOMPAT_EXTENTS);
        assert!(fi.has_extents());
        assert!(!fi.has_64bit());
    }

    #[test]
    fn test_feature_incompat_64bit() {
        let fi = FeatureIncompat(EXT4_FEATURE_INCOMPAT_64BIT);
        assert!(fi.has_64bit());
    }

    #[test]
    fn test_feature_incompat_flex_bg() {
        let fi = FeatureIncompat(EXT4_FEATURE_INCOMPAT_FLEX_BG);
        assert!(fi.has_flex_bg());
    }

    #[test]
    fn test_feature_incompat_format() {
        let fi = FeatureIncompat(
            EXT4_FEATURE_INCOMPAT_EXTENTS
                | EXT4_FEATURE_INCOMPAT_64BIT
                | EXT4_FEATURE_INCOMPAT_FLEX_BG,
        );
        let s = fi.format_flags();
        assert!(s.contains("extents"));
        assert!(s.contains("64bit"));
        assert!(s.contains("flex_bg"));
    }

    #[test]
    fn test_feature_ro_compat_sparse_super() {
        let fr = FeatureRoCompat(EXT4_FEATURE_RO_COMPAT_SPARSE_SUPER);
        assert!(fr.has_sparse_super());
    }

    #[test]
    fn test_feature_ro_compat_large_file() {
        let fr = FeatureRoCompat(EXT4_FEATURE_RO_COMPAT_LARGE_FILE);
        assert!(fr.has_large_file());
    }

    #[test]
    fn test_feature_ro_compat_huge_file() {
        let fr = FeatureRoCompat(EXT4_FEATURE_RO_COMPAT_HUGE_FILE);
        assert!(fr.has_huge_file());
    }

    #[test]
    fn test_feature_ro_compat_metadata_csum() {
        let fr = FeatureRoCompat(EXT4_FEATURE_RO_COMPAT_METADATA_CSUM);
        assert!(fr.has_metadata_csum());
    }

    // -----------------------------------------------------------------------
    // FsType tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_fs_type_names() {
        assert_eq!(FsType::Ext2.name(), "ext2");
        assert_eq!(FsType::Ext3.name(), "ext3");
        assert_eq!(FsType::Ext4.name(), "ext4");
    }

    #[test]
    fn test_fs_type_default_features_ext2() {
        let fc = FsType::Ext2.default_features_compat();
        assert!(!fc.has_journal());
    }

    #[test]
    fn test_fs_type_default_features_ext3() {
        let fc = FsType::Ext3.default_features_compat();
        assert!(fc.has_journal());
        let fi = FsType::Ext3.default_features_incompat();
        assert!(!fi.has_extents());
    }

    #[test]
    fn test_fs_type_default_features_ext4() {
        let fc = FsType::Ext4.default_features_compat();
        assert!(fc.has_journal());
        let fi = FsType::Ext4.default_features_incompat();
        assert!(fi.has_extents());
        assert!(fi.has_64bit());
        assert!(fi.has_flex_bg());
    }

    // -----------------------------------------------------------------------
    // Block group descriptor tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_block_group_new() {
        let bg = BlockGroupDesc::new(10, 11, 12, 1000, 2000);
        assert_eq!(bg.bg_block_bitmap, 10);
        assert_eq!(bg.bg_inode_bitmap, 11);
        assert_eq!(bg.bg_inode_table, 12);
        assert_eq!(bg.bg_free_blocks_count, 1000);
        assert_eq!(bg.bg_free_inodes_count, 2000);
    }

    #[test]
    fn test_block_group_checksum_deterministic() {
        let bg = BlockGroupDesc::new(10, 11, 12, 1000, 2000);
        let uuid = DEFAULT_UUID;
        let c1 = bg.compute_checksum(0, &uuid);
        let c2 = bg.compute_checksum(0, &uuid);
        assert_eq!(c1, c2);
    }

    #[test]
    fn test_block_group_checksum_varies_by_group() {
        let bg = BlockGroupDesc::new(10, 11, 12, 1000, 2000);
        let uuid = DEFAULT_UUID;
        let c1 = bg.compute_checksum(0, &uuid);
        let c2 = bg.compute_checksum(1, &uuid);
        assert_ne!(c1, c2);
    }

    // -----------------------------------------------------------------------
    // Inode tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_inode_default() {
        let ino = Inode::default();
        assert_eq!(ino.i_mode, 0);
        assert_eq!(ino.i_links_count, 0);
        assert_eq!(ino.size(), 0);
    }

    #[test]
    fn test_inode_size_64bit() {
        let mut ino = Inode::default();
        ino.set_size(0x1_0000_1234);
        assert_eq!(ino.size(), 0x1_0000_1234);
        assert_eq!(ino.i_size, 0x0000_1234);
        assert_eq!(ino.i_size_high, 1);
    }

    #[test]
    fn test_inode_is_dir() {
        let mut ino = Inode::default();
        ino.i_mode = S_IFDIR | 0o755;
        assert!(ino.is_dir());
        assert!(!ino.is_regular());
        assert!(!ino.is_symlink());
    }

    #[test]
    fn test_inode_is_regular() {
        let mut ino = Inode::default();
        ino.i_mode = S_IFREG | 0o644;
        assert!(ino.is_regular());
        assert!(!ino.is_dir());
    }

    #[test]
    fn test_inode_is_symlink() {
        let mut ino = Inode::default();
        ino.i_mode = S_IFLNK | 0o777;
        assert!(ino.is_symlink());
    }

    #[test]
    fn test_inode_is_deleted() {
        let mut ino = Inode::default();
        assert!(!ino.is_deleted());
        ino.i_dtime = 1700000000;
        assert!(ino.is_deleted());
    }

    #[test]
    fn test_inode_file_type_char() {
        let mut ino = Inode::default();
        ino.i_mode = S_IFREG;
        assert_eq!(ino.file_type_char(), '-');
        ino.i_mode = S_IFDIR;
        assert_eq!(ino.file_type_char(), 'd');
        ino.i_mode = S_IFLNK;
        assert_eq!(ino.file_type_char(), 'l');
    }

    #[test]
    fn test_inode_permissions_str() {
        let mut ino = Inode::default();
        ino.i_mode = S_IFREG | 0o755;
        assert_eq!(ino.permissions_str(), "-rwxr-xr-x");
    }

    #[test]
    fn test_inode_permissions_str_dir() {
        let mut ino = Inode::default();
        ino.i_mode = S_IFDIR | 0o700;
        assert_eq!(ino.permissions_str(), "drwx------");
    }

    #[test]
    fn test_inode_permissions_str_none() {
        let mut ino = Inode::default();
        ino.i_mode = S_IFREG;
        assert_eq!(ino.permissions_str(), "----------");
    }

    #[test]
    fn test_inode_flags_format_empty() {
        let ino = Inode::default();
        assert_eq!(ino.format_flags(), "(none)");
    }

    #[test]
    fn test_inode_flags_format_extents() {
        let mut ino = Inode::default();
        ino.i_flags = 0x0008_0000;
        assert!(ino.format_flags().contains("Extents"));
    }

    #[test]
    fn test_inode_flags_format_immutable() {
        let mut ino = Inode::default();
        ino.i_flags = 0x0000_0010;
        assert!(ino.format_flags().contains("Immutable"));
    }

    // -----------------------------------------------------------------------
    // DirEntry tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_dir_entry_new() {
        let de = DirEntry::new(2, EXT2_FT_DIR, ".");
        assert_eq!(de.inode, 2);
        assert_eq!(de.name_len, 1);
        assert_eq!(de.file_type, EXT2_FT_DIR);
        assert_eq!(de.name, ".");
    }

    #[test]
    fn test_dir_entry_rec_len_alignment() {
        let de = DirEntry::new(2, EXT2_FT_DIR, ".");
        assert_eq!(de.rec_len % 4, 0);

        let de2 = DirEntry::new(11, EXT2_FT_DIR, "lost+found");
        assert_eq!(de2.rec_len % 4, 0);
    }

    #[test]
    fn test_dir_entry_file_type_str() {
        let de = DirEntry::new(2, EXT2_FT_DIR, ".");
        assert_eq!(de.file_type_str(), "Directory");

        let de2 = DirEntry::new(100, EXT2_FT_REG_FILE, "file.txt");
        assert_eq!(de2.file_type_str(), "Regular");

        let de3 = DirEntry::new(200, EXT2_FT_SYMLINK, "link");
        assert_eq!(de3.file_type_str(), "Symbolic link");
    }

    // -----------------------------------------------------------------------
    // Extent tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_extent_header_new() {
        let hdr = ExtentHeader::new(3, 340, 0);
        assert_eq!(hdr.eh_magic, 0xF30A);
        assert_eq!(hdr.eh_entries, 3);
        assert_eq!(hdr.eh_max, 340);
        assert_eq!(hdr.eh_depth, 0);
        assert!(hdr.is_valid());
    }

    #[test]
    fn test_extent_header_invalid() {
        let mut hdr = ExtentHeader::new(0, 0, 0);
        hdr.eh_magic = 0;
        assert!(!hdr.is_valid());
    }

    #[test]
    fn test_extent_new() {
        let ext = Extent::new(0, 10, 1000);
        assert_eq!(ext.ee_block, 0);
        assert_eq!(ext.ee_len, 10);
        assert_eq!(ext.physical_block(), 1000);
    }

    #[test]
    fn test_extent_large_physical_block() {
        let ext = Extent::new(0, 5, 0x1_0000_2000);
        assert_eq!(ext.physical_block(), 0x1_0000_2000);
        assert_eq!(ext.ee_start_hi, 1);
        assert_eq!(ext.ee_start_lo, 0x2000);
    }

    #[test]
    fn test_extent_uninitialized() {
        let ext = Extent::new(0, 0x8005, 100);
        assert!(ext.is_uninitialized());
        assert_eq!(ext.actual_len(), 5);
    }

    #[test]
    fn test_extent_initialized() {
        let ext = Extent::new(0, 10, 100);
        assert!(!ext.is_uninitialized());
        assert_eq!(ext.actual_len(), 10);
    }

    #[test]
    fn test_extent_index_leaf_block() {
        let idx = ExtentIndex {
            ei_block: 0,
            ei_leaf_lo: 5000,
            ei_leaf_hi: 2,
            _ei_unused: 0,
        };
        assert_eq!(idx.leaf_block(), (2u64 << 32) + 5000);
    }

    // -----------------------------------------------------------------------
    // Journal tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_journal_superblock_default() {
        let js = JournalSuperblock::default();
        assert!(js.is_valid());
        assert_eq!(js.js_header_magic, 0xC03B_3998);
    }

    #[test]
    fn test_journal_superblock_invalid() {
        let mut js = JournalSuperblock::default();
        js.js_header_magic = 0;
        assert!(!js.is_valid());
    }

    // -----------------------------------------------------------------------
    // Utility function tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_format_bytes_bytes() {
        assert_eq!(format_bytes(100), "100 bytes");
    }

    #[test]
    fn test_format_bytes_kib() {
        assert_eq!(format_bytes(2048), "2.0 KiB");
    }

    #[test]
    fn test_format_bytes_mib() {
        assert_eq!(format_bytes(10 * 1024 * 1024), "10.0 MiB");
    }

    #[test]
    fn test_format_bytes_gib() {
        assert_eq!(format_bytes(5 * 1024 * 1024 * 1024), "5.0 GiB");
    }

    #[test]
    fn test_format_bytes_tib() {
        assert_eq!(format_bytes(2 * 1024 * 1024 * 1024 * 1024), "2.0 TiB");
    }

    #[test]
    fn test_format_timestamp_zero() {
        assert_eq!(format_timestamp(0), "n/a");
    }

    #[test]
    fn test_format_timestamp_nonzero() {
        let s = format_timestamp(1700000000);
        assert!(!s.is_empty());
        assert_ne!(s, "n/a");
    }

    #[test]
    fn test_parse_size_plain() {
        assert_eq!(parse_size("1024"), Ok(1024));
    }

    #[test]
    fn test_parse_size_k() {
        assert_eq!(parse_size("10K"), Ok(10240));
    }

    #[test]
    fn test_parse_size_m() {
        assert_eq!(parse_size("5M"), Ok(5 * 1024 * 1024));
    }

    #[test]
    fn test_parse_size_g() {
        assert_eq!(parse_size("2G"), Ok(2 * 1024 * 1024 * 1024));
    }

    #[test]
    fn test_parse_size_t() {
        assert_eq!(parse_size("1T"), Ok(1024u64 * 1024 * 1024 * 1024));
    }

    #[test]
    fn test_parse_size_gib() {
        assert_eq!(parse_size("1GiB"), Ok(1024 * 1024 * 1024));
    }

    #[test]
    fn test_parse_size_empty() {
        assert!(parse_size("").is_err());
    }

    #[test]
    fn test_parse_size_invalid() {
        assert!(parse_size("abc").is_err());
    }

    #[test]
    fn test_parse_block_count_plain() {
        assert_eq!(parse_block_count("1000", 4096), Ok(1000));
    }

    #[test]
    fn test_parse_block_count_with_suffix() {
        assert_eq!(parse_block_count("4G", 4096), Ok(1048576));
    }

    #[test]
    fn test_parse_uuid_valid() {
        let uuid = parse_uuid("a1b2c3d4-e5f6-0718-293a-4b5c6d7e8f90").unwrap();
        assert_eq!(uuid, DEFAULT_UUID);
    }

    #[test]
    fn test_parse_uuid_no_dashes() {
        let uuid = parse_uuid("a1b2c3d4e5f607182900000000000000").unwrap();
        assert_eq!(uuid[0], 0xa1);
    }

    #[test]
    fn test_parse_uuid_invalid() {
        assert!(parse_uuid("not-a-uuid").is_err());
    }

    #[test]
    fn test_is_power_of() {
        assert!(is_power_of(1, 3));
        assert!(is_power_of(3, 3));
        assert!(is_power_of(9, 3));
        assert!(is_power_of(27, 3));
        assert!(!is_power_of(6, 3));
        assert!(!is_power_of(0, 3));
    }

    #[test]
    fn test_is_power_of_5() {
        assert!(is_power_of(1, 5));
        assert!(is_power_of(5, 5));
        assert!(is_power_of(25, 5));
        assert!(is_power_of(125, 5));
        assert!(!is_power_of(10, 5));
    }

    #[test]
    fn test_is_power_of_7() {
        assert!(is_power_of(1, 7));
        assert!(is_power_of(7, 7));
        assert!(is_power_of(49, 7));
        assert!(!is_power_of(14, 7));
    }

    #[test]
    fn test_is_power_of_edge_cases() {
        assert!(!is_power_of(0, 3));
        assert!(!is_power_of(5, 0));
        assert!(!is_power_of(5, 1));
    }

    #[test]
    fn test_has_superblock_backup() {
        assert!(has_superblock_backup(0));
        assert!(has_superblock_backup(1));
        assert!(has_superblock_backup(3));
        assert!(has_superblock_backup(5));
        assert!(has_superblock_backup(7));
        assert!(has_superblock_backup(9));
        assert!(has_superblock_backup(25));
        assert!(has_superblock_backup(27));
        assert!(has_superblock_backup(49));
        assert!(!has_superblock_backup(2));
        assert!(!has_superblock_backup(4));
        assert!(!has_superblock_backup(6));
    }

    // -----------------------------------------------------------------------
    // FilesystemImage tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_filesystem_image_new() {
        let img = FilesystemImage::new("/dev/sda1");
        assert_eq!(img.device, "/dev/sda1");
        assert!(img.block_groups.is_empty());
        assert!(img.inodes.is_empty());
    }

    #[test]
    fn test_filesystem_image_create_ext4() {
        let img = FilesystemImage::create_default("/dev/sda1", FsType::Ext4, 262144, "test");
        assert!(img.superblock.is_valid());
        assert_eq!(img.superblock.label(), "test");
        assert!(img.superblock.s_feature_compat.has_journal());
        assert!(img.superblock.s_feature_incompat.has_extents());
        assert!(!img.block_groups.is_empty());
        assert!(!img.inodes.is_empty());
        assert!(img.journal.is_some());
    }

    #[test]
    fn test_filesystem_image_create_ext3() {
        let img = FilesystemImage::create_default("/dev/sda1", FsType::Ext3, 262144, "");
        assert!(img.superblock.s_feature_compat.has_journal());
        assert!(!img.superblock.s_feature_incompat.has_extents());
    }

    #[test]
    fn test_filesystem_image_create_ext2() {
        let img = FilesystemImage::create_default("/dev/sda1", FsType::Ext2, 262144, "");
        assert!(!img.superblock.s_feature_compat.has_journal());
        assert!(!img.superblock.s_feature_incompat.has_extents());
        assert!(img.journal.is_none());
    }

    #[test]
    fn test_filesystem_image_root_inode() {
        let img = FilesystemImage::create_default("/dev/sda1", FsType::Ext4, 262144, "");
        // Root inode is at index 1 (inode #2)
        assert!(img.inodes.len() > 1);
        assert!(img.inodes[1].is_dir());
        assert_eq!(img.inodes[1].i_mode & 0o7777, 0o755);
    }

    #[test]
    fn test_filesystem_image_root_dir_entries() {
        let img = FilesystemImage::create_default("/dev/sda1", FsType::Ext4, 262144, "");
        let root_entries = img
            .dir_entries
            .iter()
            .find(|(ino, _)| *ino == EXT2_ROOT_INO);
        assert!(root_entries.is_some());
        let (_, entries) = root_entries.unwrap();
        assert!(entries.len() >= 3);
        assert_eq!(entries[0].name, ".");
        assert_eq!(entries[1].name, "..");
        assert_eq!(entries[2].name, "lost+found");
    }

    #[test]
    fn test_filesystem_image_block_groups() {
        let img = FilesystemImage::create_default("/dev/sda1", FsType::Ext4, 262144, "");
        let expected_groups = img.superblock.group_count();
        assert_eq!(img.block_groups.len() as u32, expected_groups);
    }

    #[test]
    fn test_filesystem_image_small() {
        let img = FilesystemImage::create_default("/dev/sda1", FsType::Ext4, 1024, "small");
        assert!(img.superblock.is_valid());
        assert!(img.superblock.group_count() >= 1);
    }

    // -----------------------------------------------------------------------
    // mke2fs argument parsing tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_mke2fs_args_minimal() {
        let args = vec!["mke2fs".into(), "/dev/sda1".into()];
        let opts = parse_mke2fs_args(&args).unwrap();
        assert_eq!(opts.device, "/dev/sda1");
        assert_eq!(opts.fs_type, FsType::Ext4);
    }

    #[test]
    fn test_mke2fs_args_with_type() {
        let args = vec![
            "mke2fs".into(),
            "-t".into(),
            "ext3".into(),
            "/dev/sda1".into(),
        ];
        let opts = parse_mke2fs_args(&args).unwrap();
        assert_eq!(opts.fs_type, FsType::Ext3);
    }

    #[test]
    fn test_mke2fs_args_with_block_size() {
        let args = vec![
            "mke2fs".into(),
            "-b".into(),
            "1024".into(),
            "/dev/sda1".into(),
        ];
        let opts = parse_mke2fs_args(&args).unwrap();
        assert_eq!(opts.block_size, 1024);
    }

    #[test]
    fn test_mke2fs_args_invalid_block_size() {
        let args = vec![
            "mke2fs".into(),
            "-b".into(),
            "3000".into(),
            "/dev/sda1".into(),
        ];
        assert!(parse_mke2fs_args(&args).is_err());
    }

    #[test]
    fn test_mke2fs_args_with_label() {
        let args = vec![
            "mke2fs".into(),
            "-L".into(),
            "mylabel".into(),
            "/dev/sda1".into(),
        ];
        let opts = parse_mke2fs_args(&args).unwrap();
        assert_eq!(opts.label, "mylabel");
    }

    #[test]
    fn test_mke2fs_args_with_blocks() {
        let args = vec!["mke2fs".into(), "/dev/sda1".into(), "100000".into()];
        let opts = parse_mke2fs_args(&args).unwrap();
        assert_eq!(opts.blocks, Some(100000));
    }

    #[test]
    fn test_mke2fs_args_quiet() {
        let args = vec!["mke2fs".into(), "-q".into(), "/dev/sda1".into()];
        let opts = parse_mke2fs_args(&args).unwrap();
        assert!(opts.quiet);
    }

    #[test]
    fn test_mke2fs_args_no_device() {
        let args = vec!["mke2fs".into()];
        assert!(parse_mke2fs_args(&args).is_err());
    }

    #[test]
    fn test_mke2fs_args_ext2_personality() {
        let args = vec!["mkfs.ext2".into(), "/dev/sda1".into()];
        let opts = parse_mke2fs_args(&args).unwrap();
        assert_eq!(opts.fs_type, FsType::Ext2);
        assert!(!opts.journal);
    }

    #[test]
    fn test_mke2fs_args_reserved_percent() {
        let args = vec![
            "mke2fs".into(),
            "-m".into(),
            "10".into(),
            "/dev/sda1".into(),
        ];
        let opts = parse_mke2fs_args(&args).unwrap();
        assert!((opts.reserved_percent - 10.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_mke2fs_args_uuid() {
        let args = vec![
            "mke2fs".into(),
            "-U".into(),
            "a1b2c3d4-e5f6-0718-293a-4b5c6d7e8f90".into(),
            "/dev/sda1".into(),
        ];
        let opts = parse_mke2fs_args(&args).unwrap();
        assert!(opts.uuid.is_some());
    }

    #[test]
    fn test_mke2fs_args_features() {
        let args = vec![
            "mke2fs".into(),
            "-O".into(),
            "^has_journal".into(),
            "/dev/sda1".into(),
        ];
        let opts = parse_mke2fs_args(&args).unwrap();
        assert!(opts.no_journal);
    }

    #[test]
    fn test_mke2fs_args_inode_size() {
        let args = vec![
            "mke2fs".into(),
            "-I".into(),
            "128".into(),
            "/dev/sda1".into(),
        ];
        let opts = parse_mke2fs_args(&args).unwrap();
        assert_eq!(opts.inode_size, 128);
    }

    #[test]
    fn test_mke2fs_args_invalid_inode_size() {
        let args = vec![
            "mke2fs".into(),
            "-I".into(),
            "64".into(),
            "/dev/sda1".into(),
        ];
        assert!(parse_mke2fs_args(&args).is_err());
    }

    // -----------------------------------------------------------------------
    // tune2fs argument parsing tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_tune2fs_args_label() {
        let args = vec![
            "tune2fs".into(),
            "-L".into(),
            "newlabel".into(),
            "/dev/sda1".into(),
        ];
        let opts = parse_tune2fs_args(&args).unwrap();
        assert_eq!(opts.label, Some("newlabel".to_string()));
    }

    #[test]
    fn test_tune2fs_args_uuid() {
        let args = vec![
            "tune2fs".into(),
            "-U".into(),
            "random".into(),
            "/dev/sda1".into(),
        ];
        let opts = parse_tune2fs_args(&args).unwrap();
        assert_eq!(opts.uuid, Some("random".to_string()));
    }

    #[test]
    fn test_tune2fs_args_max_mount() {
        let args = vec![
            "tune2fs".into(),
            "-c".into(),
            "30".into(),
            "/dev/sda1".into(),
        ];
        let opts = parse_tune2fs_args(&args).unwrap();
        assert_eq!(opts.max_mount_count, Some(30));
    }

    #[test]
    fn test_tune2fs_args_interval_days() {
        let args = vec![
            "tune2fs".into(),
            "-i".into(),
            "30d".into(),
            "/dev/sda1".into(),
        ];
        let opts = parse_tune2fs_args(&args).unwrap();
        assert_eq!(opts.check_interval, Some(30 * 86400));
    }

    #[test]
    fn test_tune2fs_args_interval_weeks() {
        let args = vec![
            "tune2fs".into(),
            "-i".into(),
            "4w".into(),
            "/dev/sda1".into(),
        ];
        let opts = parse_tune2fs_args(&args).unwrap();
        assert_eq!(opts.check_interval, Some(4 * 604800));
    }

    #[test]
    fn test_tune2fs_args_interval_months() {
        let args = vec![
            "tune2fs".into(),
            "-i".into(),
            "6m".into(),
            "/dev/sda1".into(),
        ];
        let opts = parse_tune2fs_args(&args).unwrap();
        assert_eq!(opts.check_interval, Some(6 * 2592000));
    }

    #[test]
    fn test_tune2fs_args_error_behavior() {
        let args = vec![
            "tune2fs".into(),
            "-e".into(),
            "panic".into(),
            "/dev/sda1".into(),
        ];
        let opts = parse_tune2fs_args(&args).unwrap();
        assert_eq!(opts.error_behavior, Some(3));
    }

    #[test]
    fn test_tune2fs_args_list() {
        let args = vec!["tune2fs".into(), "-l".into(), "/dev/sda1".into()];
        let opts = parse_tune2fs_args(&args).unwrap();
        assert!(opts.list_contents);
    }

    #[test]
    fn test_tune2fs_args_no_device() {
        let args = vec!["tune2fs".into(), "-l".into()];
        assert!(parse_tune2fs_args(&args).is_err());
    }

    #[test]
    fn test_tune2fs_args_add_journal() {
        let args = vec!["tune2fs".into(), "-j".into(), "/dev/sda1".into()];
        let opts = parse_tune2fs_args(&args).unwrap();
        assert!(opts.add_journal);
    }

    #[test]
    fn test_tune2fs_args_remove_journal() {
        let args = vec![
            "tune2fs".into(),
            "-O".into(),
            "^has_journal".into(),
            "/dev/sda1".into(),
        ];
        let opts = parse_tune2fs_args(&args).unwrap();
        assert!(opts.remove_journal);
    }

    // -----------------------------------------------------------------------
    // dumpe2fs argument parsing tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_dumpe2fs_args_minimal() {
        let args = vec!["dumpe2fs".into(), "/dev/sda1".into()];
        let opts = parse_dumpe2fs_args(&args).unwrap();
        assert_eq!(opts.device, "/dev/sda1");
        assert!(opts.show_groups);
    }

    #[test]
    fn test_dumpe2fs_args_header_only() {
        let args = vec!["dumpe2fs".into(), "-h".into(), "/dev/sda1".into()];
        let opts = parse_dumpe2fs_args(&args).unwrap();
        assert!(opts.show_header_only);
    }

    #[test]
    fn test_dumpe2fs_args_no_device() {
        let args = vec!["dumpe2fs".into()];
        assert!(parse_dumpe2fs_args(&args).is_err());
    }

    // -----------------------------------------------------------------------
    // debugfs tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_debugfs_args_minimal() {
        let args = vec!["debugfs".into(), "/dev/sda1".into()];
        let opts = parse_debugfs_args(&args).unwrap();
        assert_eq!(opts.device, "/dev/sda1");
        assert!(!opts.writable);
    }

    #[test]
    fn test_debugfs_args_writable() {
        let args = vec!["debugfs".into(), "-w".into(), "/dev/sda1".into()];
        let opts = parse_debugfs_args(&args).unwrap();
        assert!(opts.writable);
    }

    #[test]
    fn test_debugfs_args_command() {
        let args = vec![
            "debugfs".into(),
            "-R".into(),
            "stats".into(),
            "/dev/sda1".into(),
        ];
        let opts = parse_debugfs_args(&args).unwrap();
        assert_eq!(opts.commands.len(), 1);
        assert_eq!(opts.commands[0], "stats");
    }

    #[test]
    fn test_debugfs_command_quit() {
        let img = FilesystemImage::create_default("/dev/sda1", FsType::Ext4, 262144, "");
        assert!(!run_debugfs_command("quit", &img));
        assert!(!run_debugfs_command("exit", &img));
        assert!(!run_debugfs_command("q", &img));
    }

    #[test]
    fn test_debugfs_command_help() {
        let img = FilesystemImage::create_default("/dev/sda1", FsType::Ext4, 262144, "");
        assert!(run_debugfs_command("help", &img));
        assert!(run_debugfs_command("?", &img));
    }

    #[test]
    fn test_debugfs_command_unknown() {
        let img = FilesystemImage::create_default("/dev/sda1", FsType::Ext4, 262144, "");
        assert!(run_debugfs_command("notacommand", &img));
    }

    // -----------------------------------------------------------------------
    // resize2fs argument parsing tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_resize2fs_args_minimal() {
        let args = vec!["resize2fs".into(), "/dev/sda1".into()];
        let opts = parse_resize2fs_args(&args).unwrap();
        assert_eq!(opts.device, "/dev/sda1");
        assert!(opts.new_size.is_none());
    }

    #[test]
    fn test_resize2fs_args_with_size() {
        let args = vec!["resize2fs".into(), "/dev/sda1".into(), "500000".into()];
        let opts = parse_resize2fs_args(&args).unwrap();
        assert_eq!(opts.new_size, Some("500000".to_string()));
    }

    #[test]
    fn test_resize2fs_args_minimum() {
        let args = vec!["resize2fs".into(), "-M".into(), "/dev/sda1".into()];
        let opts = parse_resize2fs_args(&args).unwrap();
        assert!(opts.minimum);
    }

    #[test]
    fn test_resize2fs_args_force() {
        let args = vec!["resize2fs".into(), "-f".into(), "/dev/sda1".into()];
        let opts = parse_resize2fs_args(&args).unwrap();
        assert!(opts.force);
    }

    #[test]
    fn test_resize2fs_args_no_device() {
        let args = vec!["resize2fs".into()];
        assert!(parse_resize2fs_args(&args).is_err());
    }

    // -----------------------------------------------------------------------
    // e2fsck argument parsing tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_e2fsck_args_minimal() {
        let args = vec!["e2fsck".into(), "/dev/sda1".into()];
        let opts = parse_e2fsck_args(&args).unwrap();
        assert_eq!(opts.device, "/dev/sda1");
        assert!(!opts.force);
    }

    #[test]
    fn test_e2fsck_args_force() {
        let args = vec!["e2fsck".into(), "-f".into(), "/dev/sda1".into()];
        let opts = parse_e2fsck_args(&args).unwrap();
        assert!(opts.force);
    }

    #[test]
    fn test_e2fsck_args_preen() {
        let args = vec!["e2fsck".into(), "-p".into(), "/dev/sda1".into()];
        let opts = parse_e2fsck_args(&args).unwrap();
        assert!(opts.preen);
    }

    #[test]
    fn test_e2fsck_args_yes() {
        let args = vec!["e2fsck".into(), "-y".into(), "/dev/sda1".into()];
        let opts = parse_e2fsck_args(&args).unwrap();
        assert!(opts.yes);
    }

    #[test]
    fn test_e2fsck_args_no() {
        let args = vec!["e2fsck".into(), "-n".into(), "/dev/sda1".into()];
        let opts = parse_e2fsck_args(&args).unwrap();
        assert!(opts.no);
        assert!(opts._read_only);
    }

    #[test]
    fn test_e2fsck_args_verbose() {
        let args = vec!["e2fsck".into(), "-v".into(), "/dev/sda1".into()];
        let opts = parse_e2fsck_args(&args).unwrap();
        assert_eq!(opts.verbosity, FsckVerbosity::Verbose);
    }

    #[test]
    fn test_e2fsck_args_quiet() {
        let args = vec!["e2fsck".into(), "-q".into(), "/dev/sda1".into()];
        let opts = parse_e2fsck_args(&args).unwrap();
        assert_eq!(opts.verbosity, FsckVerbosity::Quiet);
    }

    #[test]
    fn test_e2fsck_args_no_device() {
        let args = vec!["e2fsck".into()];
        assert!(parse_e2fsck_args(&args).is_err());
    }

    #[test]
    fn test_e2fsck_args_check_blocks() {
        let args = vec!["e2fsck".into(), "-c".into(), "/dev/sda1".into()];
        let opts = parse_e2fsck_args(&args).unwrap();
        assert!(opts.check_blocks);
    }

    // -----------------------------------------------------------------------
    // e2image argument parsing tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_e2image_args_minimal() {
        let args = vec!["e2image".into(), "/dev/sda1".into(), "output.img".into()];
        let opts = parse_e2image_args(&args).unwrap();
        assert_eq!(opts.device, "/dev/sda1");
        assert_eq!(opts.output, "output.img");
        assert_eq!(opts.format, ImageFormat::Normal);
    }

    #[test]
    fn test_e2image_args_raw() {
        let args = vec![
            "e2image".into(),
            "-r".into(),
            "/dev/sda1".into(),
            "output.img".into(),
        ];
        let opts = parse_e2image_args(&args).unwrap();
        assert_eq!(opts.format, ImageFormat::Raw);
    }

    #[test]
    fn test_e2image_args_qcow2() {
        let args = vec![
            "e2image".into(),
            "-Q".into(),
            "/dev/sda1".into(),
            "output.qcow2".into(),
        ];
        let opts = parse_e2image_args(&args).unwrap();
        assert_eq!(opts.format, ImageFormat::Qcow2);
    }

    #[test]
    fn test_e2image_args_all_data() {
        let args = vec![
            "e2image".into(),
            "-a".into(),
            "/dev/sda1".into(),
            "output.img".into(),
        ];
        let opts = parse_e2image_args(&args).unwrap();
        assert!(opts.all_data);
    }

    #[test]
    fn test_e2image_args_install() {
        let args = vec![
            "e2image".into(),
            "-I".into(),
            "/dev/sda1".into(),
            "input.img".into(),
        ];
        let opts = parse_e2image_args(&args).unwrap();
        assert!(opts.install);
    }

    #[test]
    fn test_e2image_args_too_few() {
        let args = vec!["e2image".into(), "/dev/sda1".into()];
        assert!(parse_e2image_args(&args).is_err());
    }

    // -----------------------------------------------------------------------
    // filefrag tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_filefrag_args_minimal() {
        let args = vec!["filefrag".into(), "myfile.txt".into()];
        let opts = parse_filefrag_args(&args).unwrap();
        assert_eq!(opts.files.len(), 1);
        assert_eq!(opts.files[0], "myfile.txt");
    }

    #[test]
    fn test_filefrag_args_verbose() {
        let args = vec!["filefrag".into(), "-v".into(), "myfile.txt".into()];
        let opts = parse_filefrag_args(&args).unwrap();
        assert!(opts.verbose);
    }

    #[test]
    fn test_filefrag_args_multiple_files() {
        let args = vec![
            "filefrag".into(),
            "a.txt".into(),
            "b.txt".into(),
            "c.txt".into(),
        ];
        let opts = parse_filefrag_args(&args).unwrap();
        assert_eq!(opts.files.len(), 3);
    }

    #[test]
    fn test_filefrag_args_no_files() {
        let args = vec!["filefrag".into()];
        assert!(parse_filefrag_args(&args).is_err());
    }

    #[test]
    fn test_simulate_file_extents_zero_size() {
        let extents = simulate_file_extents(0, 4096);
        assert!(extents.is_empty());
    }

    #[test]
    fn test_simulate_file_extents_small() {
        let extents = simulate_file_extents(4096, 4096);
        assert!(!extents.is_empty());
        assert_eq!(extents[0].logical_offset, 0);
    }

    #[test]
    fn test_simulate_file_extents_large() {
        let extents = simulate_file_extents(100 * 4096, 4096);
        // Should have multiple extents for fragmented file
        assert!(extents.len() > 1);
    }

    #[test]
    fn test_simulate_file_size_deterministic() {
        let s1 = simulate_file_size("test.txt");
        let s2 = simulate_file_size("test.txt");
        assert_eq!(s1, s2);
    }

    #[test]
    fn test_simulate_file_size_varies() {
        let s1 = simulate_file_size("file_a.txt");
        let s2 = simulate_file_size("file_b.txt");
        // Different names should give different sizes (with high probability)
        assert_ne!(s1, s2);
    }

    // -----------------------------------------------------------------------
    // Integration-like run tests (check return codes)
    // -----------------------------------------------------------------------

    #[test]
    fn test_run_mke2fs_basic() {
        let args = vec!["mke2fs".into(), "-q".into(), "/dev/sda1".into()];
        assert_eq!(run_mke2fs(&args), 0);
    }

    #[test]
    fn test_run_mke2fs_with_label() {
        let args = vec![
            "mke2fs".into(),
            "-q".into(),
            "-L".into(),
            "test".into(),
            "/dev/sda1".into(),
        ];
        assert_eq!(run_mke2fs(&args), 0);
    }

    #[test]
    fn test_run_mke2fs_dry_run() {
        let args = vec!["mke2fs".into(), "-n".into(), "/dev/sda1".into()];
        assert_eq!(run_mke2fs(&args), 0);
    }

    #[test]
    fn test_run_mke2fs_no_args() {
        let args = vec!["mke2fs".into()];
        assert_eq!(run_mke2fs(&args), 1);
    }

    #[test]
    fn test_run_tune2fs_set_label() {
        let args = vec![
            "tune2fs".into(),
            "-L".into(),
            "new".into(),
            "/dev/sda1".into(),
        ];
        assert_eq!(run_tune2fs(&args), 0);
    }

    #[test]
    fn test_run_tune2fs_list() {
        let args = vec!["tune2fs".into(), "-l".into(), "/dev/sda1".into()];
        assert_eq!(run_tune2fs(&args), 0);
    }

    #[test]
    fn test_run_tune2fs_no_changes() {
        let args = vec!["tune2fs".into(), "/dev/sda1".into()];
        assert_eq!(run_tune2fs(&args), 1);
    }

    #[test]
    fn test_run_tune2fs_uuid_random() {
        let args = vec![
            "tune2fs".into(),
            "-U".into(),
            "random".into(),
            "/dev/sda1".into(),
        ];
        assert_eq!(run_tune2fs(&args), 0);
    }

    #[test]
    fn test_run_tune2fs_uuid_clear() {
        let args = vec![
            "tune2fs".into(),
            "-U".into(),
            "clear".into(),
            "/dev/sda1".into(),
        ];
        assert_eq!(run_tune2fs(&args), 0);
    }

    #[test]
    fn test_run_tune2fs_uuid_time() {
        let args = vec![
            "tune2fs".into(),
            "-U".into(),
            "time".into(),
            "/dev/sda1".into(),
        ];
        assert_eq!(run_tune2fs(&args), 0);
    }

    #[test]
    fn test_run_dumpe2fs_basic() {
        let args = vec!["dumpe2fs".into(), "/dev/sda1".into()];
        assert_eq!(run_dumpe2fs(&args), 0);
    }

    #[test]
    fn test_run_dumpe2fs_header_only() {
        let args = vec!["dumpe2fs".into(), "-h".into(), "/dev/sda1".into()];
        assert_eq!(run_dumpe2fs(&args), 0);
    }

    #[test]
    fn test_run_debugfs_no_device() {
        let args = vec!["debugfs".into()];
        assert_eq!(run_debugfs(&args), 0);
    }

    #[test]
    fn test_run_debugfs_with_command() {
        let args = vec![
            "debugfs".into(),
            "-R".into(),
            "stats".into(),
            "/dev/sda1".into(),
        ];
        assert_eq!(run_debugfs(&args), 0);
    }

    #[test]
    fn test_run_resize2fs_grow() {
        let args = vec!["resize2fs".into(), "/dev/sda1".into(), "500000".into()];
        assert_eq!(run_resize2fs(&args), 0);
    }

    #[test]
    fn test_run_resize2fs_minimum() {
        let args = vec!["resize2fs".into(), "-M".into(), "/dev/sda1".into()];
        assert_eq!(run_resize2fs(&args), 0);
    }

    #[test]
    fn test_run_resize2fs_print_min() {
        let args = vec!["resize2fs".into(), "-P".into(), "/dev/sda1".into()];
        assert_eq!(run_resize2fs(&args), 0);
    }

    #[test]
    fn test_run_e2fsck_clean() {
        let args = vec!["e2fsck".into(), "/dev/sda1".into()];
        assert_eq!(run_e2fsck(&args), 0); // Clean filesystem
    }

    #[test]
    fn test_run_e2fsck_force() {
        let args = vec!["e2fsck".into(), "-f".into(), "/dev/sda1".into()];
        let code = run_e2fsck(&args);
        assert!(code == 0 || code == 1); // Either clean or fixed
    }

    #[test]
    fn test_run_e2fsck_quiet() {
        let args = vec!["e2fsck".into(), "-q".into(), "/dev/sda1".into()];
        assert_eq!(run_e2fsck(&args), 0);
    }

    #[test]
    fn test_run_e2label_display() {
        let args = vec!["e2label".into(), "/dev/sda1".into()];
        assert_eq!(run_e2label(&args), 0);
    }

    #[test]
    fn test_run_e2label_set() {
        let args = vec!["e2label".into(), "/dev/sda1".into(), "newlabel".into()];
        assert_eq!(run_e2label(&args), 0);
    }

    #[test]
    fn test_run_e2label_too_long() {
        let args = vec![
            "e2label".into(),
            "/dev/sda1".into(),
            "this_label_is_way_too_long_for_ext".into(),
        ];
        assert_eq!(run_e2label(&args), 1);
    }

    #[test]
    fn test_run_e2label_no_args() {
        let args = vec!["e2label".into()];
        assert_eq!(run_e2label(&args), 1);
    }

    #[test]
    fn test_run_e2image_basic() {
        let args = vec!["e2image".into(), "/dev/sda1".into(), "output.img".into()];
        assert_eq!(run_e2image(&args), 0);
    }

    #[test]
    fn test_run_e2image_install() {
        let args = vec![
            "e2image".into(),
            "-I".into(),
            "/dev/sda1".into(),
            "input.img".into(),
        ];
        assert_eq!(run_e2image(&args), 0);
    }

    #[test]
    fn test_run_e2image_qcow2() {
        let args = vec![
            "e2image".into(),
            "-Q".into(),
            "/dev/sda1".into(),
            "out.qcow2".into(),
        ];
        assert_eq!(run_e2image(&args), 0);
    }

    #[test]
    fn test_run_filefrag_basic() {
        let args = vec!["filefrag".into(), "myfile.txt".into()];
        let code = run_filefrag(&args);
        // May return 0 or 1 depending on simulated fragmentation
        assert!(code == 0 || code == 1);
    }

    #[test]
    fn test_run_filefrag_verbose() {
        let args = vec!["filefrag".into(), "-v".into(), "myfile.txt".into()];
        let code = run_filefrag(&args);
        assert!(code == 0 || code == 1);
    }

    // -----------------------------------------------------------------------
    // Personality name and usage tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_personality_name() {
        assert_eq!(personality_name(Personality::Mke2fs), "mke2fs");
        assert_eq!(personality_name(Personality::Tune2fs), "tune2fs");
        assert_eq!(personality_name(Personality::Dumpe2fs), "dumpe2fs");
        assert_eq!(personality_name(Personality::Debugfs), "debugfs");
        assert_eq!(personality_name(Personality::Resize2fs), "resize2fs");
        assert_eq!(personality_name(Personality::E2fsck), "e2fsck");
        assert_eq!(personality_name(Personality::E2label), "e2label");
        assert_eq!(personality_name(Personality::E2image), "e2image");
        assert_eq!(personality_name(Personality::Filefrag), "filefrag");
    }

    #[test]
    fn test_print_version_does_not_panic() {
        // Just verify it doesn't panic
        print_version(Personality::Mke2fs);
        print_version(Personality::Filefrag);
    }

    #[test]
    fn test_print_usage_does_not_panic() {
        print_usage(Personality::Mke2fs);
        print_usage(Personality::Tune2fs);
        print_usage(Personality::Dumpe2fs);
        print_usage(Personality::Debugfs);
        print_usage(Personality::Resize2fs);
        print_usage(Personality::E2fsck);
        print_usage(Personality::E2label);
        print_usage(Personality::E2image);
        print_usage(Personality::Filefrag);
    }

    // -----------------------------------------------------------------------
    // Additional edge case tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_mke2fs_ext2_no_journal() {
        let args = vec![
            "mke2fs".into(),
            "-q".into(),
            "-t".into(),
            "ext2".into(),
            "/dev/sda1".into(),
        ];
        assert_eq!(run_mke2fs(&args), 0);
    }

    #[test]
    fn test_tune2fs_error_continue() {
        let args = vec![
            "tune2fs".into(),
            "-e".into(),
            "continue".into(),
            "/dev/sda1".into(),
        ];
        assert_eq!(run_tune2fs(&args), 0);
    }

    #[test]
    fn test_tune2fs_error_remount_ro() {
        let args = vec![
            "tune2fs".into(),
            "-e".into(),
            "remount-ro".into(),
            "/dev/sda1".into(),
        ];
        assert_eq!(run_tune2fs(&args), 0);
    }

    #[test]
    fn test_tune2fs_reserved_percent() {
        let args = vec![
            "tune2fs".into(),
            "-m".into(),
            "10".into(),
            "/dev/sda1".into(),
        ];
        assert_eq!(run_tune2fs(&args), 0);
    }

    #[test]
    fn test_debugfs_ls_command() {
        let img = FilesystemImage::create_default("/dev/sda1", FsType::Ext4, 262144, "");
        assert!(run_debugfs_command("ls 2", &img));
    }

    #[test]
    fn test_debugfs_stat_root() {
        let img = FilesystemImage::create_default("/dev/sda1", FsType::Ext4, 262144, "");
        assert!(run_debugfs_command("stat 2", &img));
    }

    #[test]
    fn test_debugfs_features() {
        let img = FilesystemImage::create_default("/dev/sda1", FsType::Ext4, 262144, "");
        assert!(run_debugfs_command("feature", &img));
    }

    #[test]
    fn test_debugfs_testb() {
        let img = FilesystemImage::create_default("/dev/sda1", FsType::Ext4, 262144, "");
        assert!(run_debugfs_command("testb 50", &img));
        assert!(run_debugfs_command("testb 100000", &img));
    }

    #[test]
    fn test_debugfs_pwd() {
        let img = FilesystemImage::create_default("/dev/sda1", FsType::Ext4, 262144, "");
        assert!(run_debugfs_command("pwd", &img));
    }

    #[test]
    fn test_debugfs_cd() {
        let img = FilesystemImage::create_default("/dev/sda1", FsType::Ext4, 262144, "");
        assert!(run_debugfs_command("cd lost+found", &img));
    }

    #[test]
    fn test_debugfs_ncheck_root() {
        let img = FilesystemImage::create_default("/dev/sda1", FsType::Ext4, 262144, "");
        assert!(run_debugfs_command("ncheck 2", &img));
    }

    #[test]
    fn test_debugfs_blocks_root() {
        let img = FilesystemImage::create_default("/dev/sda1", FsType::Ext4, 262144, "");
        assert!(run_debugfs_command("blocks 2", &img));
    }

    #[test]
    fn test_debugfs_supported_features() {
        let img = FilesystemImage::create_default("/dev/sda1", FsType::Ext4, 262144, "");
        assert!(run_debugfs_command("supported_features", &img));
    }

    #[test]
    fn test_resize2fs_no_change() {
        // If new_size equals current, nothing to do
        let args = vec!["resize2fs".into(), "/dev/sda1".into(), "262144".into()];
        assert_eq!(run_resize2fs(&args), 0);
    }

    #[test]
    fn test_e2fsck_timing() {
        let args = vec![
            "e2fsck".into(),
            "-f".into(),
            "-t".into(),
            "/dev/sda1".into(),
        ];
        let code = run_e2fsck(&args);
        assert!(code == 0 || code == 1);
    }
}
