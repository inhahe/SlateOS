//! ext4 superblock parsing and validation.
//!
//! Reads the superblock from a block device, validates the magic number
//! and feature flags, and provides accessors for derived values (block
//! count, block size, group count, etc.).

use alloc::format;
use alloc::string::String;

use crate::error::{KernelError, KernelResult};

use super::ondisk::{
    self, Ext4Superblock, EXT4_MAGIC, SUPERBLOCK_OFFSET,
    SUPPORTED_INCOMPAT, SUPPORTED_RO_COMPAT,
};

// ---------------------------------------------------------------------------
// Parsed superblock
// ---------------------------------------------------------------------------

/// Validated and parsed ext4 superblock with derived values.
///
/// Created by [`parse`] after reading raw bytes from the device.
/// All derived values are computed once and cached.
#[derive(Debug)]
pub struct ParsedSuperblock {
    /// Raw superblock (for accessing fields directly).
    pub raw: Ext4Superblock,
    /// Block size in bytes (1024, 2048, 4096, ...).
    pub block_size: u32,
    /// Total block count (64-bit).
    pub block_count: u64,
    /// Free block count (64-bit).
    pub free_block_count: u64,
    /// Inode size in bytes.
    pub inode_size: u32,
    /// Number of block groups.
    pub group_count: u32,
    /// Block group descriptor size (32 or 64).
    pub desc_size: u32,
    /// Whether the filesystem uses 64-bit block numbers.
    pub is_64bit: bool,
    /// Whether the filesystem uses extents.
    pub has_extents: bool,
    /// Whether the filesystem has a journal.
    pub has_journal: bool,
    /// Whether we can mount read-write (false if unsupported ro_compat).
    #[allow(dead_code)]
    pub can_write: bool,
    /// Volume label (trimmed, UTF-8 best-effort).
    pub volume_name: String,
}

/// Read and parse an ext4 superblock from raw bytes.
///
/// `data` must be at least 1024 bytes, starting at the superblock
/// (byte offset 1024 from the partition start ��� the caller must skip
/// the first 1024 bytes).
///
/// # Errors
///
/// - `InvalidArgument` — data too short or invalid block size.
/// - `NotSupported` — unsupported incompatible feature flags.
/// - `InvalidExecutable` — bad magic number (reusing the error; the
///   filesystem image is "not a valid ext4 filesystem").
pub fn parse(data: &[u8]) -> KernelResult<ParsedSuperblock> {
    if data.len() < core::mem::size_of::<Ext4Superblock>() {
        return Err(KernelError::InvalidArgument);
    }

    // SAFETY: We've verified the data is large enough.  Ext4Superblock
    // is #[repr(C)] with no padding requirements beyond 4-byte alignment.
    // ext4 is always little-endian, and we're on x86_64 (also LE), so
    // no byte-swapping is needed.  The pointer alignment is satisfied
    // because we copy to an aligned local.
    let raw = read_superblock(data)?;

    // Validate magic number.
    if raw.s_magic != EXT4_MAGIC {
        return Err(KernelError::InvalidExecutable);
    }

    // Block size.
    let block_size_shift = raw.s_log_block_size;
    if block_size_shift > 6 {
        // Maximum block size is 64 KiB (1024 << 6).  Anything larger
        // is invalid.
        return Err(KernelError::InvalidArgument);
    }
    let block_size = 1024u32 << block_size_shift;

    // Inode size.
    let inode_size = if raw.s_rev_level >= 1 {
        u32::from(raw.s_inode_size)
    } else {
        128 // Original ext2 inode size.
    };
    if inode_size < 128 || !inode_size.is_power_of_two() {
        return Err(KernelError::InvalidArgument);
    }

    // Feature flags.
    let is_64bit = (raw.s_feature_incompat & ondisk::incompat::BIT64) != 0;
    let has_extents = (raw.s_feature_incompat & ondisk::incompat::EXTENTS) != 0;
    let has_journal = (raw.s_feature_compat & ondisk::compat::HAS_JOURNAL) != 0;

    // Check incompatible features — refuse to mount if we don't understand.
    let unsupported_incompat = raw.s_feature_incompat & !SUPPORTED_INCOMPAT;
    if unsupported_incompat != 0 {
        return Err(KernelError::NotSupported);
    }

    // Check read-only compat — we can mount read-only if unknown bits are set.
    let unsupported_ro_compat = raw.s_feature_ro_compat & !SUPPORTED_RO_COMPAT;
    let can_write = unsupported_ro_compat == 0;

    // Total block count (combine hi+lo for 64-bit).
    let block_count = if is_64bit {
        u64::from(raw.s_blocks_count_lo)
            | (u64::from(raw.s_blocks_count_hi) << 32)
    } else {
        u64::from(raw.s_blocks_count_lo)
    };

    // Free block count.
    let free_block_count = if is_64bit {
        u64::from(raw.s_free_blocks_count_lo)
            | (u64::from(raw.s_free_blocks_count_hi) << 32)
    } else {
        u64::from(raw.s_free_blocks_count_lo)
    };

    // Number of block groups.
    let blocks_per_group = u64::from(raw.s_blocks_per_group);
    if blocks_per_group == 0 {
        return Err(KernelError::InvalidArgument);
    }
    // Round up: (block_count + blocks_per_group - 1) / blocks_per_group.
    let group_count = block_count
        .saturating_add(blocks_per_group)
        .saturating_sub(1)
        / blocks_per_group;
    // Block group count must fit in u32.
    let group_count = u32::try_from(group_count)
        .map_err(|_| KernelError::InvalidArgument)?;

    // Group descriptor size.
    let desc_size = if is_64bit && raw.s_desc_size >= 64 {
        u32::from(raw.s_desc_size)
    } else {
        32
    };

    // Volume name.
    let volume_name = extract_name(&raw.s_volume_name);

    Ok(ParsedSuperblock {
        raw,
        block_size,
        block_count,
        free_block_count,
        inode_size,
        group_count,
        desc_size,
        is_64bit,
        has_extents,
        has_journal,
        can_write,
        volume_name,
    })
}

/// Read an `Ext4Superblock` from raw bytes, handling alignment.
fn read_superblock(data: &[u8]) -> KernelResult<Ext4Superblock> {
    // We can't just cast the pointer because `data` may not be aligned
    // to the struct's alignment.  Copy byte-by-byte into an aligned local.
    let mut sb = core::mem::MaybeUninit::<Ext4Superblock>::uninit();
    let sb_size = core::mem::size_of::<Ext4Superblock>();

    if data.len() < sb_size {
        return Err(KernelError::InvalidArgument);
    }

    // SAFETY: We're copying sb_size bytes from `data` into the
    // MaybeUninit, which is the exact size of Ext4Superblock.
    // The struct is #[repr(C)] and all fields are integer types
    // (no padding holes that need specific values).
    unsafe {
        core::ptr::copy_nonoverlapping(
            data.as_ptr(),
            sb.as_mut_ptr().cast::<u8>(),
            sb_size,
        );
        Ok(sb.assume_init())
    }
}

/// Extract a null-terminated name from a fixed-size byte array.
fn extract_name(buf: &[u8]) -> String {
    let len = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    let name_bytes = buf.get(..len).unwrap_or(&[]);
    String::from_utf8_lossy(name_bytes).into_owned()
}

// ---------------------------------------------------------------------------
// Accessors
// ---------------------------------------------------------------------------

impl ParsedSuperblock {
    /// Byte offset of a given block number on the device.
    #[must_use]
    #[allow(dead_code)]
    pub fn block_offset(&self, block_nr: u64) -> u64 {
        block_nr.saturating_mul(u64::from(self.block_size))
    }

    /// Which block group contains a given inode number?
    ///
    /// Inode numbers are 1-based.  Block group 0 contains inodes
    /// 1 through `s_inodes_per_group`.
    #[must_use]
    pub fn inode_group(&self, inode_nr: u32) -> u32 {
        inode_nr.wrapping_sub(1) / self.raw.s_inodes_per_group
    }

    /// Index of an inode within its block group (0-based).
    #[must_use]
    pub fn inode_index_in_group(&self, inode_nr: u32) -> u32 {
        inode_nr.wrapping_sub(1) % self.raw.s_inodes_per_group
    }

    /// Byte offset of a block group descriptor in the descriptor table.
    ///
    /// The descriptor table starts at the block after the superblock block.
    #[must_use]
    pub fn group_desc_offset(&self, group_nr: u32) -> u64 {
        // Block group descriptor table starts at block
        // (s_first_data_block + 1).
        let gdt_block = u64::from(self.raw.s_first_data_block) + 1;
        let gdt_byte_offset = gdt_block.saturating_mul(u64::from(self.block_size));
        gdt_byte_offset.saturating_add(
            u64::from(group_nr).saturating_mul(u64::from(self.desc_size))
        )
    }

    /// Total filesystem size in bytes.
    #[must_use]
    pub fn total_bytes(&self) -> u64 {
        self.block_count.saturating_mul(u64::from(self.block_size))
    }

    /// Free space in bytes.
    #[must_use]
    pub fn free_bytes(&self) -> u64 {
        self.free_block_count.saturating_mul(u64::from(self.block_size))
    }

    /// Human-readable summary for diagnostics.
    #[must_use]
    pub fn summary(&self) -> String {
        let total_mb = self.total_bytes() / (1024 * 1024);
        let free_mb = self.free_bytes() / (1024 * 1024);
        format!(
            "ext4: \"{}\" {}MB ({}MB free), {} blocks ({}B), {} groups, \
             {} inodes ({}B), 64bit={}, extents={}, journal={}",
            self.volume_name,
            total_mb,
            free_mb,
            self.block_count,
            self.block_size,
            self.group_count,
            self.raw.s_inodes_count,
            self.inode_size,
            self.is_64bit,
            self.has_extents,
            self.has_journal,
        )
    }
}

// ---------------------------------------------------------------------------
// Superblock byte offset helper
// ---------------------------------------------------------------------------

/// The byte offset on the block device where the superblock lives.
///
/// This is always 1024, regardless of block size.  For 1 KiB blocks,
/// the superblock is in block 1.  For 2 KiB+ blocks, it's in block 0
/// at byte offset 1024 within that block.
#[must_use]
pub const fn superblock_device_offset() -> u64 {
    SUPERBLOCK_OFFSET
}
