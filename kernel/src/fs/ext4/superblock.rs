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
    /// Whether the filesystem supports huge files (RO_COMPAT_HUGE_FILE).
    ///
    /// When set, inodes may use 48-bit block counts (lo+hi).  If a
    /// specific inode also has `inode_flags::HUGE_FILE`, the block count
    /// is in filesystem block units rather than 512-byte sectors.
    #[allow(dead_code)]
    pub has_huge_file: bool,
    /// Whether we can mount read-write (false if unsupported ro_compat).
    #[allow(dead_code)]
    pub can_write: bool,
    /// Volume label (trimmed, UTF-8 best-effort).
    pub volume_name: String,
    /// Whether the filesystem has metadata checksums (RO_COMPAT_METADATA_CSUM).
    pub has_metadata_csum: bool,
    /// CRC32C seed for metadata checksum computation.
    ///
    /// If INCOMPAT_CSUM_SEED is set, this is `s_checksum_seed` from the
    /// superblock.  Otherwise, it is `crc32c(~0, s_uuid)`.  This seed is
    /// used as the initial value when computing checksums for group
    /// descriptors, inodes, and other metadata.
    pub csum_seed: u32,
    /// Preferred i_extra_isize for new inodes.
    ///
    /// Taken from `s_want_extra_isize` (or `s_min_extra_isize` as fallback).
    /// Common value on modern Linux: 32 (allows crtime, version, etc.).
    /// Set to 0 for filesystems with 128-byte inodes.
    pub want_extra_isize: u16,
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
    let has_huge_file =
        (raw.s_feature_ro_compat & ondisk::ro_compat::HUGE_FILE) != 0;

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

    // Metadata checksums.
    let has_metadata_csum =
        (raw.s_feature_ro_compat & ondisk::ro_compat::METADATA_CSUM) != 0;

    // Compute the checksum seed.  ext4 uses this as the initial CRC
    // accumulator for all per-object checksums (group descriptors, inodes,
    // extents, etc.).
    let csum_seed = if has_metadata_csum {
        if (raw.s_feature_incompat & ondisk::incompat::CSUM_SEED) != 0 {
            // The filesystem stores a pre-computed seed.
            raw.s_checksum_seed
        } else {
            // Compute seed from the filesystem UUID:
            //   crc32c(~0, s_uuid)  (raw, without final inversion)
            crate::crypto::crc32c_raw(!0u32, &raw.s_uuid)
        }
    } else {
        0
    };

    // Validate superblock checksum if metadata checksumming is enabled.
    if has_metadata_csum {
        validate_superblock_checksum(data)?;
    }

    // Preferred i_extra_isize for new inodes.  Typical Linux default: 32.
    // Only meaningful if inode_size > 128.
    let want_extra_isize = if inode_size > 128 {
        let want = raw.s_want_extra_isize;
        let min = raw.s_min_extra_isize;
        if want > 0 { want } else if min > 0 { min } else { 32 }
    } else {
        0
    };

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
        has_huge_file,
        can_write,
        volume_name,
        has_metadata_csum,
        csum_seed,
        want_extra_isize,
    })
}

/// Validate the superblock's CRC32C checksum.
///
/// The checksum covers bytes 0..0x3FC of the superblock (everything
/// except the checksum field itself, which occupies the last 4 bytes).
///
/// Reference: Linux `ext4_superblock_csum()` in `fs/ext4/super.c`.
fn validate_superblock_checksum(data: &[u8]) -> KernelResult<()> {
    // The checksum field is at offset 0x3FC within the 1024-byte superblock.
    const CSUM_OFFSET: usize = 0x3FC;
    const SB_SIZE: usize = core::mem::size_of::<Ext4Superblock>();

    if data.len() < SB_SIZE {
        return Err(KernelError::InvalidArgument);
    }

    // Read the stored checksum (little-endian u32 at offset 0x3FC).
    let stored = u32::from_le_bytes([
        data[CSUM_OFFSET],
        data[CSUM_OFFSET.saturating_add(1)],
        data[CSUM_OFFSET.saturating_add(2)],
        data[CSUM_OFFSET.saturating_add(3)],
    ]);

    // Compute CRC32C over everything before the checksum field,
    // then over everything after it (there's nothing after in a standard
    // 1024-byte superblock, but be correct anyway).
    let before = data.get(..CSUM_OFFSET).unwrap_or(&[]);
    let after = data.get(CSUM_OFFSET.saturating_add(4)..SB_SIZE).unwrap_or(&[]);

    let mut crc = crate::crypto::crc32c_raw(!0u32, before);
    // Zero-fill the checksum field position (4 bytes of 0x00).
    crc = crate::crypto::crc32c_raw(crc, &[0u8; 4]);
    // Continue with any bytes after the checksum field.
    let computed = crate::crypto::crc32c_seed(crc, after);

    if stored != computed {
        crate::serial_println!(
            "[ext4] superblock checksum mismatch: stored={:#010X}, computed={:#010X}",
            stored,
            computed,
        );
        return Err(KernelError::CorruptedData);
    }

    crate::serial_println!("[ext4] superblock checksum valid ({:#010X})", stored);
    Ok(())
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
///
/// Returns valid UTF-8 or falls back to a hex-escaped representation
/// for non-UTF-8 volume names (which are informational, not path data).
fn extract_name(buf: &[u8]) -> String {
    let len = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    let name_bytes = buf.get(..len).unwrap_or(&[]);
    match core::str::from_utf8(name_bytes) {
        Ok(s) => String::from(s),
        Err(_) => {
            // Volume name is informational — hex-escape non-UTF-8 bytes
            // rather than silently corrupting with replacement characters.
            let mut out = String::with_capacity(name_bytes.len() * 4);
            for &b in name_bytes {
                if b.is_ascii_graphic() || b == b' ' {
                    out.push(b as char);
                } else {
                    use core::fmt::Write;
                    let _ = write!(out, "\\x{:02x}", b);
                }
            }
            out
        }
    }
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

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Superblock parsing and accessor tests.
///
/// Constructs synthetic superblock byte buffers and verifies that
/// `parse()` correctly rejects invalid data and correctly computes
/// all derived fields.
pub fn self_test() -> KernelResult<()> {
    crate::serial_println!("[ext4-superblock] Running self-test...");

    test_parse_too_short()?;
    test_parse_bad_magic()?;
    test_parse_valid_4k()?;
    test_parse_64bit_mode()?;
    test_inode_group_index()?;
    test_group_desc_offset()?;
    test_size_accessors()?;

    crate::serial_println!("[ext4-superblock] Self-test PASSED (7 tests)");
    Ok(())
}

/// Helper: create a 1024-byte zeroed superblock buffer with defaults
/// that will pass basic validation.
fn make_valid_sb_buf() -> [u8; 1024] {
    let mut buf = [0u8; 1024];

    // s_magic at offset 0x38: 0xEF53 LE.
    buf[0x38] = 0x53;
    buf[0x39] = 0xEF;

    // s_log_block_size at offset 0x18: 2 → block_size = 1024 << 2 = 4096.
    buf[0x18] = 2;

    // s_blocks_per_group at offset 0x20: 32768 = 0x8000 LE.
    buf[0x20] = 0x00;
    buf[0x21] = 0x80;

    // s_blocks_count_lo at offset 0x04: 32768 (1 group's worth).
    buf[0x04] = 0x00;
    buf[0x05] = 0x80;

    // s_inodes_per_group at offset 0x28: 8192 = 0x2000.
    buf[0x28] = 0x00;
    buf[0x29] = 0x20;

    // s_inodes_count at offset 0x00: 8192.
    buf[0x00] = 0x00;
    buf[0x01] = 0x20;

    // s_rev_level at offset 0x4C: 1 (dynamic inodes).
    buf[0x4C] = 1;

    // s_inode_size at offset 0x58: 256 = 0x0100.
    buf[0x58] = 0x00;
    buf[0x59] = 0x01;

    // s_feature_incompat at offset 0x60: FILETYPE | EXTENTS = 0x0042.
    buf[0x60] = 0x42;

    // s_free_blocks_count_lo at offset 0x0C: 16384 = 0x4000.
    buf[0x0C] = 0x00;
    buf[0x0D] = 0x40;

    buf
}

/// Parse must reject data shorter than 1024 bytes.
fn test_parse_too_short() -> KernelResult<()> {
    let short_buf = [0u8; 512];
    match parse(&short_buf) {
        Err(KernelError::InvalidArgument) => {}
        other => {
            crate::serial_println!(
                "[ext4-superblock]   FAIL: parse(512 bytes) = {:?}",
                other.map(|_| ())
            );
            return Err(KernelError::InternalError);
        }
    }

    crate::serial_println!("[ext4-superblock]   parse too short: OK");
    Ok(())
}

/// Parse must reject bad magic number.
fn test_parse_bad_magic() -> KernelResult<()> {
    let buf = [0u8; 1024]; // All zeros → magic = 0.
    match parse(&buf) {
        Err(KernelError::InvalidExecutable) => {}
        other => {
            crate::serial_println!(
                "[ext4-superblock]   FAIL: parse(bad magic) = {:?}",
                other.map(|_| ())
            );
            return Err(KernelError::InternalError);
        }
    }

    crate::serial_println!("[ext4-superblock]   parse bad magic: OK");
    Ok(())
}

/// Parse a valid 4K-block, 32-bit-mode superblock.
fn test_parse_valid_4k() -> KernelResult<()> {
    let buf = make_valid_sb_buf();
    let sb = parse(&buf)?;

    if sb.block_size != 4096 {
        crate::serial_println!(
            "[ext4-superblock]   FAIL: block_size = {}", sb.block_size
        );
        return Err(KernelError::InternalError);
    }
    if sb.block_count != 32768 {
        crate::serial_println!(
            "[ext4-superblock]   FAIL: block_count = {}", sb.block_count
        );
        return Err(KernelError::InternalError);
    }
    if sb.free_block_count != 16384 {
        crate::serial_println!(
            "[ext4-superblock]   FAIL: free_block_count = {}", sb.free_block_count
        );
        return Err(KernelError::InternalError);
    }
    if sb.inode_size != 256 {
        crate::serial_println!(
            "[ext4-superblock]   FAIL: inode_size = {}", sb.inode_size
        );
        return Err(KernelError::InternalError);
    }
    if sb.group_count != 1 {
        crate::serial_println!(
            "[ext4-superblock]   FAIL: group_count = {}", sb.group_count
        );
        return Err(KernelError::InternalError);
    }
    // 32-bit mode: desc_size should be 32.
    if sb.desc_size != 32 || sb.is_64bit {
        crate::serial_println!(
            "[ext4-superblock]   FAIL: desc_size={}, is_64bit={}",
            sb.desc_size, sb.is_64bit
        );
        return Err(KernelError::InternalError);
    }
    if !sb.has_extents {
        crate::serial_println!("[ext4-superblock]   FAIL: has_extents should be true");
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[ext4-superblock]   parse valid 4K: OK");
    Ok(())
}

/// Parse a 64-bit mode superblock with larger group descriptors.
fn test_parse_64bit_mode() -> KernelResult<()> {
    let mut buf = make_valid_sb_buf();

    // Add BIT64 to incompat: 0x0042 | 0x0080 = 0x00C2.
    buf[0x60] = 0xC2;

    // s_desc_size at offset 0xFE: 64 = 0x0040.
    buf[0xFE] = 0x40;

    // s_blocks_count_hi at offset 0x150: 1 → total = 0x1_0000_8000.
    buf[0x150] = 1;

    let sb = parse(&buf)?;

    if !sb.is_64bit {
        crate::serial_println!("[ext4-superblock]   FAIL: not 64-bit");
        return Err(KernelError::InternalError);
    }
    if sb.desc_size != 64 {
        crate::serial_println!(
            "[ext4-superblock]   FAIL: desc_size = {}", sb.desc_size
        );
        return Err(KernelError::InternalError);
    }

    // Total blocks = lo(0x8000) | hi(1) << 32 = 0x100008000.
    let expected: u64 = 0x1_0000_8000;
    if sb.block_count != expected {
        crate::serial_println!(
            "[ext4-superblock]   FAIL: block_count = {:#x}, expected {:#x}",
            sb.block_count, expected
        );
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[ext4-superblock]   parse 64-bit mode: OK");
    Ok(())
}

/// Test inode_group() and inode_index_in_group().
fn test_inode_group_index() -> KernelResult<()> {
    let buf = make_valid_sb_buf();
    let sb = parse(&buf)?;
    // s_inodes_per_group = 8192.

    // Inode 1 → group 0, index 0.
    if sb.inode_group(1) != 0 || sb.inode_index_in_group(1) != 0 {
        crate::serial_println!("[ext4-superblock]   FAIL: inode 1 group/index");
        return Err(KernelError::InternalError);
    }

    // Inode 2 (root) → group 0, index 1.
    if sb.inode_group(2) != 0 || sb.inode_index_in_group(2) != 1 {
        crate::serial_println!("[ext4-superblock]   FAIL: inode 2 group/index");
        return Err(KernelError::InternalError);
    }

    // Inode 8192 → group 0, index 8191 (last in group 0).
    if sb.inode_group(8192) != 0 || sb.inode_index_in_group(8192) != 8191 {
        crate::serial_println!("[ext4-superblock]   FAIL: inode 8192 group/index");
        return Err(KernelError::InternalError);
    }

    // Inode 8193 → group 1, index 0.
    if sb.inode_group(8193) != 1 || sb.inode_index_in_group(8193) != 0 {
        crate::serial_println!("[ext4-superblock]   FAIL: inode 8193 group/index");
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[ext4-superblock]   inode group/index: OK");
    Ok(())
}

/// Test group_desc_offset() computation.
fn test_group_desc_offset() -> KernelResult<()> {
    let buf = make_valid_sb_buf();
    let sb = parse(&buf)?;
    // block_size=4096, s_first_data_block=0 (for 4K blocks).
    // GDT block = 0 + 1 = 1. GDT byte offset = 1 * 4096 = 4096.
    // desc_size = 32 (32-bit mode).

    let off0 = sb.group_desc_offset(0);
    if off0 != 4096 {
        crate::serial_println!(
            "[ext4-superblock]   FAIL: gd_offset(0) = {}", off0
        );
        return Err(KernelError::InternalError);
    }

    let off1 = sb.group_desc_offset(1);
    if off1 != 4096 + 32 {
        crate::serial_println!(
            "[ext4-superblock]   FAIL: gd_offset(1) = {}", off1
        );
        return Err(KernelError::InternalError);
    }

    let off10 = sb.group_desc_offset(10);
    if off10 != 4096 + 320 {
        crate::serial_println!(
            "[ext4-superblock]   FAIL: gd_offset(10) = {}", off10
        );
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[ext4-superblock]   group desc offset: OK");
    Ok(())
}

/// Test total_bytes() and free_bytes().
fn test_size_accessors() -> KernelResult<()> {
    let buf = make_valid_sb_buf();
    let sb = parse(&buf)?;

    // block_count = 32768, block_size = 4096.
    // total = 32768 * 4096 = 128 MiB = 134217728.
    let total = sb.total_bytes();
    if total != 134_217_728 {
        crate::serial_println!(
            "[ext4-superblock]   FAIL: total_bytes = {}", total
        );
        return Err(KernelError::InternalError);
    }

    // free_block_count = 16384.
    // free = 16384 * 4096 = 64 MiB = 67108864.
    let free = sb.free_bytes();
    if free != 67_108_864 {
        crate::serial_println!(
            "[ext4-superblock]   FAIL: free_bytes = {}", free
        );
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[ext4-superblock]   size accessors: OK");
    Ok(())
}
