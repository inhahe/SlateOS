//! The F2FS superblock: geometry, area layout, and the feature gate.
//!
//! The superblock is the only structure in F2FS at a fixed byte offset, and
//! everything else is found through the block addresses it carries. Unlike
//! Btrfs, there is no bootstrap circularity to unpick — the checkpoint, SIT,
//! NAT, SSA and main areas are all named here by absolute block address, so a
//! mount is a straight line rather than a fixed point.
//!
//! # Two copies, and why both are tried
//!
//! F2FS keeps the superblock twice, in blocks 0 and 1 of the volume (byte
//! offsets 1024 and 5120). They are not a checksum pair — either one alone is
//! authoritative — so a reader tries the first and falls back to the second.
//! That is worth doing rather than skipping: the first 4 KiB is exactly the
//! region a stray `dd`, a partition-table rewrite or a mis-aimed bootloader
//! install lands on, which is why the format put a spare one block further in.

use alloc::string::String;
use alloc::vec::Vec;

use crate::error::{KernelError, KernelResult};
use crate::fs::blocksrc::{SectorSource, read_bytes};

use super::raw::{BLOCK_SIZE, LOG_BLOCK_SIZE, MAGIC, SUPER_OFFSET, read_u16, read_u32, read_u64};

// ---------------------------------------------------------------------------
// Feature flags
// ---------------------------------------------------------------------------

/// Per-file encryption (fscrypt). File *contents* are unreadable without keys.
pub const FEATURE_ENCRYPT: u32 = 0x0000_0001;
/// The volume spans host-managed zoned block devices.
pub const FEATURE_BLKZONED: u32 = 0x0000_0002;
/// Atomic write support.
pub const FEATURE_ATOMIC_WRITE: u32 = 0x0000_0004;
/// Inodes carry `i_extra_isize` and the fields after it.
pub const FEATURE_EXTRA_ATTR: u32 = 0x0000_0008;
/// Project quotas.
pub const FEATURE_PRJQUOTA: u32 = 0x0000_0010;
/// Inodes carry their own checksum.
pub const FEATURE_INODE_CHKSUM: u32 = 0x0000_0020;
/// The inline-xattr area is variable-sized (`i_inline_xattr_size` is honoured).
pub const FEATURE_FLEXIBLE_INLINE_XATTR: u32 = 0x0000_0040;
/// Quota data lives in hidden inodes named by `qf_ino`.
pub const FEATURE_QUOTA_INO: u32 = 0x0000_0080;
/// Inodes carry a creation time.
pub const FEATURE_INODE_CRTIME: u32 = 0x0000_0100;
/// A `lost+found` directory is reserved.
pub const FEATURE_LOST_FOUND: u32 = 0x0000_0200;
/// fs-verity.
pub const FEATURE_VERITY: u32 = 0x0000_0400;
/// The superblock itself carries a CRC at `checksum_offset`.
pub const FEATURE_SB_CHKSUM: u32 = 0x0000_0800;
/// Case-insensitive lookup via a Unicode casefold table.
pub const FEATURE_CASEFOLD: u32 = 0x0000_1000;
/// Transparent per-file compression.
pub const FEATURE_COMPRESSION: u32 = 0x0000_2000;
/// The volume was made read-only at mkfs time.
pub const FEATURE_RO: u32 = 0x0000_4000;

/// Byte offset of the superblock's own CRC, when [`FEATURE_SB_CHKSUM`] is set.
pub const SB_CHKSUM_OFFSET: u32 = 3068;

/// Features whose presence changes how *metadata* is laid out or interpreted.
///
/// A feature outside this set may still make some files unreadable — a
/// `COMPRESSION` volume has compressed extents in it, an `ENCRYPT` volume has
/// encrypted ones — but that is a per-file failure this driver reports when it
/// reaches the file, which is strictly better than refusing the whole mount and
/// telling the user nothing. The features listed here are different in kind:
/// they mean the structures this driver walks are not the structures it knows,
/// so continuing would produce confident nonsense rather than an error.
const UNSUPPORTED_FEATURES: u32 = FEATURE_BLKZONED;

// ---------------------------------------------------------------------------
// The superblock
// ---------------------------------------------------------------------------

/// A parsed and validated `f2fs_super_block`.
#[derive(Debug, Clone)]
pub struct SuperBlock {
    /// Blocks per segment (always 512 in practice; read, not assumed).
    pub blocks_per_seg: u32,
    /// Segments per section.
    pub segs_per_sec: u32,
    /// Sections per zone.
    pub secs_per_zone: u32,
    /// Total user blocks.
    pub block_count: u64,
    /// Total sections in the volume.
    pub section_count: u32,
    /// Total segments in the volume.
    pub segment_count: u32,
    /// Segments belonging to the main (data + node) area.
    pub segment_count_main: u32,
    /// First block of the checkpoint area.
    pub cp_blkaddr: u32,
    /// First block of the SIT area.
    pub sit_blkaddr: u32,
    /// First block of the NAT area.
    pub nat_blkaddr: u32,
    /// First block of the SSA (segment summary) area.
    pub ssa_blkaddr: u32,
    /// First block of the main area.
    pub main_blkaddr: u32,
    /// Node id of the root directory.
    pub root_ino: u32,
    /// Node id of the hidden node-address inode.
    pub node_ino: u32,
    /// Node id of the hidden meta inode.
    pub meta_ino: u32,
    /// Volume UUID.
    pub uuid: [u8; 16],
    /// Volume label, decoded from its UTF-16 on-disk form.
    pub label: String,
    /// Feature bitmap.
    pub feature: u32,
    /// Extra checkpoint-pack blocks holding overflow bitmaps.
    pub cp_payload: u32,
}

impl SuperBlock {
    /// Is `feature` set?
    pub const fn has_feature(&self, feature: u32) -> bool {
        self.feature & feature != 0
    }

    /// Blocks in one segment, as a `u64` for offset arithmetic.
    pub const fn blocks_per_seg_u64(&self) -> u64 {
        self.blocks_per_seg as u64
    }

    /// Is `block` a plausible address for data this driver will read?
    ///
    /// The main area is where every node and data block lives; a pointer into
    /// the metadata areas, or past the end of the volume, is corruption. This
    /// is the single check that stops a garbled block pointer from being
    /// turned into a read at an arbitrary device offset — worth having in one
    /// place, because every path that dereferences an address goes through it.
    pub fn is_valid_data_block(&self, block: u32) -> bool {
        block >= self.main_blkaddr && u64::from(block) < self.total_blocks()
    }

    /// Does `block` lie in the checkpoint area?
    ///
    /// Used to bound the addresses derived from checkpoint header fields
    /// before they are turned into reads. The checkpoint area runs from
    /// `cp_blkaddr` up to the start of the SIT area.
    pub const fn is_in_checkpoint_area(&self, block: u32) -> bool {
        block >= self.cp_blkaddr && block < self.sit_blkaddr
    }

    /// Does `block` lie in the NAT area?
    pub const fn is_in_nat_area(&self, block: u32) -> bool {
        block >= self.nat_blkaddr && block < self.ssa_blkaddr
    }

    /// Total blocks addressed by the volume, metadata areas included.
    ///
    /// `block_count` in the superblock counts only *user* blocks, so it is not
    /// the device's size and cannot be used as an upper bound on a block
    /// address. The real ceiling is the end of the main area.
    pub fn total_blocks(&self) -> u64 {
        u64::from(self.main_blkaddr).saturating_add(
            u64::from(self.segment_count_main).saturating_mul(u64::from(self.blocks_per_seg)),
        )
    }

    /// Parse a superblock out of the 4 KiB block that contains it.
    ///
    /// `buf` starts at the superblock structure itself (byte 1024 of its
    /// block), not at the block boundary — the CRC is defined over the
    /// structure, so making the caller do the slicing would put the one
    /// offset that must not drift in two places.
    pub fn parse(buf: &[u8]) -> KernelResult<Self> {
        if read_u32(buf, 0)? != MAGIC {
            return Err(KernelError::InvalidArgument);
        }

        let log_blocksize = read_u32(buf, 16)?;
        if log_blocksize != LOG_BLOCK_SIZE {
            // F2FS has only ever shipped a 4 KiB block size, and every offset
            // in this driver is derived from that constant. Refuse rather than
            // silently misparse a volume from some future variant.
            return Err(KernelError::NotSupported);
        }

        let log_blocks_per_seg = read_u32(buf, 20)?;
        let blocks_per_seg = 1u32
            .checked_shl(log_blocks_per_seg)
            .ok_or(KernelError::CorruptedData)?;
        if blocks_per_seg == 0 || !blocks_per_seg.is_power_of_two() {
            return Err(KernelError::CorruptedData);
        }

        let feature = read_u32(buf, 2180).unwrap_or(0);
        if feature & UNSUPPORTED_FEATURES != 0 {
            return Err(KernelError::NotSupported);
        }

        let mut uuid = [0u8; 16];
        let uuid_src = buf.get(108..124).ok_or(KernelError::InvalidArgument)?;
        uuid.copy_from_slice(uuid_src);

        let sb = Self {
            blocks_per_seg,
            segs_per_sec: read_u32(buf, 24)?,
            secs_per_zone: read_u32(buf, 28)?,
            block_count: read_u64(buf, 36)?,
            section_count: read_u32(buf, 44)?,
            segment_count: read_u32(buf, 48)?,
            segment_count_main: read_u32(buf, 68)?,
            cp_blkaddr: read_u32(buf, 76)?,
            sit_blkaddr: read_u32(buf, 80)?,
            nat_blkaddr: read_u32(buf, 84)?,
            ssa_blkaddr: read_u32(buf, 88)?,
            main_blkaddr: read_u32(buf, 92)?,
            root_ino: read_u32(buf, 96)?,
            node_ino: read_u32(buf, 100)?,
            meta_ino: read_u32(buf, 104)?,
            uuid,
            label: decode_label(buf),
            feature,
            cp_payload: read_u32(buf, 1664).unwrap_or(0),
        };

        sb.sanity_check()?;

        if sb.has_feature(FEATURE_SB_CHKSUM) {
            let crc_offset = read_u32(buf, 32)?;
            if crc_offset != SB_CHKSUM_OFFSET {
                return Err(KernelError::CorruptedData);
            }
            let covered = buf
                .get(..crc_offset as usize)
                .ok_or(KernelError::InvalidArgument)?;
            let want = read_u32(buf, crc_offset as usize)?;
            // Seeded with the magic and *not* inverted at either end: F2FS
            // uses Linux's bare `crc32_le`, so the conventional framing would
            // be wrong by `!0` on both sides.
            if crate::crypto::crc32_raw(MAGIC, covered) != want {
                return Err(KernelError::CorruptedData);
            }
        }

        Ok(sb)
    }

    /// Reject a superblock whose areas do not form an ascending, non-empty
    /// layout.
    ///
    /// Every one of these is a bound that later arithmetic assumes. Checking
    /// them once here is what lets `is_valid_data_block` be a comparison
    /// rather than a proof.
    fn sanity_check(&self) -> KernelResult<()> {
        let ordered = self.cp_blkaddr < self.sit_blkaddr
            && self.sit_blkaddr < self.nat_blkaddr
            && self.nat_blkaddr < self.ssa_blkaddr
            && self.ssa_blkaddr < self.main_blkaddr;
        if !ordered {
            return Err(KernelError::CorruptedData);
        }
        if self.segment_count_main == 0 || self.segment_count == 0 {
            return Err(KernelError::CorruptedData);
        }
        if self.root_ino == 0 {
            return Err(KernelError::CorruptedData);
        }
        // A main area that claims more blocks than the volume has is the shape
        // that turns a plausible block pointer into a read past the device.
        if self.total_blocks()
            > u64::from(self.segment_count).saturating_mul(u64::from(self.blocks_per_seg))
        {
            return Err(KernelError::CorruptedData);
        }
        Ok(())
    }
}

/// Decode the UTF-16LE volume label at offset 124.
///
/// Truncated at the first NUL, as the format intends, and lossy only in the
/// narrow sense that an unpaired surrogate becomes U+FFFD — the label is a
/// display string, not a path, so it is the one place in this driver where
/// producing *some* text beats producing an error.
fn decode_label(buf: &[u8]) -> String {
    let mut units: Vec<u16> = Vec::new();
    for i in 0..512usize {
        let off = 124usize.saturating_add(i.saturating_mul(2));
        let Ok(u) = read_u16(buf, off) else { break };
        if u == 0 {
            break;
        }
        units.push(u);
    }
    char::decode_utf16(units)
        .map(|r| r.unwrap_or(char::REPLACEMENT_CHARACTER))
        .collect()
}

/// Read and validate the superblock, trying the primary then the backup.
///
/// # Errors
///
/// The *primary's* error is the one returned when both fail, because it is
/// what a user asking "why will this not mount?" needs: reporting the backup's
/// error instead would describe a copy they did not know existed.
pub fn read_superblock(src: &dyn SectorSource) -> KernelResult<SuperBlock> {
    let primary = read_bytes(src, SUPER_OFFSET, BLOCK_SIZE).and_then(|b| SuperBlock::parse(&b));
    if primary.is_ok() {
        return primary;
    }

    let backup_off = SUPER_OFFSET.saturating_add(BLOCK_SIZE as u64);
    if let Ok(sb) = read_bytes(src, backup_off, BLOCK_SIZE).and_then(|b| SuperBlock::parse(&b)) {
        crate::serial_println!("[f2fs] Primary superblock rejected; using the backup.");
        return Ok(sb);
    }

    primary
}

/// Does this volume look like F2FS?
///
/// Checks the magic in both superblock copies and nothing else. A probe must
/// be cheap and must not reject a volume this driver could go on to mount, so
/// the real validation is left to [`read_superblock`].
pub fn probe(src: &dyn SectorSource) -> bool {
    for off in [SUPER_OFFSET, SUPER_OFFSET.saturating_add(BLOCK_SIZE as u64)] {
        if let Ok(buf) = read_bytes(src, off, 4) {
            if read_u32(&buf, 0) == Ok(MAGIC) {
                return true;
            }
        }
    }
    false
}
