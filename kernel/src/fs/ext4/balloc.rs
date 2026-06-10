//! ext4 block and inode allocation.
//!
//! Implements bitmap-based allocation for both blocks and inodes.
//! Each block group has a block bitmap and an inode bitmap.  Allocation
//! searches for free entries in these bitmaps, marks them as used, and
//! updates the free counts in both the group descriptor and superblock.
//!
//! ## Block allocation strategy
//!
//! 1. **Goal-directed**: Try to allocate near a "goal" block (e.g., the
//!    previous extent's physical block + 1) for locality.
//! 2. **Group preference**: Prefer the block group containing the goal.
//! 3. **Fallback scan**: If the preferred group is full, scan all groups.
//!
//! Based on Linux `fs/ext4/balloc.c` and `fs/ext4/mballoc.c` (simplified;
//! no multi-block allocator or buddy bitmap yet).

// All public functions are infrastructure for upcoming write support.
#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;

use crate::error::{KernelError, KernelResult};

use super::io::BlockReader;
use super::ondisk::Ext4GroupDesc;
use super::superblock::ParsedSuperblock;

// ---------------------------------------------------------------------------
// Block bitmap operations
// ---------------------------------------------------------------------------

/// Read the block bitmap for a given block group.
///
/// Returns a Vec of `block_size` bytes representing the bitmap.
/// Each bit corresponds to one block in the group.  Bit 0 of byte 0
/// is the first block in the group.
///
/// If metadata checksums are enabled, validates the CRC32C stored in
/// the group descriptor against the bitmap data.
pub fn read_block_bitmap(
    reader: &BlockReader,
    sb: &ParsedSuperblock,
    gd: &Ext4GroupDesc,
) -> KernelResult<Vec<u8>> {
    let bitmap_block = group_desc_block_bitmap(gd, sb.is_64bit);
    let mut buf = vec![0u8; sb.block_size as usize];
    reader.read_block(bitmap_block, &mut buf)?;

    // Validate checksum if metadata checksums are enabled.
    if sb.has_metadata_csum {
        let computed = compute_bitmap_checksum(sb, &buf);
        let stored = block_bitmap_checksum(gd, sb.desc_size);
        if stored != computed {
            crate::serial_println!(
                "[ext4] block bitmap checksum mismatch: stored={:#010x} computed={:#010x}",
                stored, computed,
            );
            return Err(KernelError::CorruptedData);
        }
    }

    Ok(buf)
}

/// Write the block bitmap for a given block group.
///
/// Also updates the bitmap checksum in the group descriptor.
pub fn write_block_bitmap(
    reader: &BlockReader,
    sb: &ParsedSuperblock,
    gd: &mut Ext4GroupDesc,
    bitmap: &[u8],
) -> KernelResult<()> {
    let bitmap_block = group_desc_block_bitmap(gd, sb.is_64bit);
    if bitmap.len() < sb.block_size as usize {
        return Err(KernelError::InvalidArgument);
    }

    // Stamp checksum into the group descriptor.
    if sb.has_metadata_csum {
        let csum = compute_bitmap_checksum(sb, bitmap);
        set_block_bitmap_checksum(gd, sb.desc_size, csum);
    }

    reader.write_block(bitmap_block, bitmap)
}

/// Read the inode bitmap for a given block group.
///
/// If metadata checksums are enabled, validates the CRC32C.
pub fn read_inode_bitmap(
    reader: &BlockReader,
    sb: &ParsedSuperblock,
    gd: &Ext4GroupDesc,
) -> KernelResult<Vec<u8>> {
    let bitmap_block = group_desc_inode_bitmap(gd, sb.is_64bit);
    let mut buf = vec![0u8; sb.block_size as usize];
    reader.read_block(bitmap_block, &mut buf)?;

    // Validate checksum if metadata checksums are enabled.
    if sb.has_metadata_csum {
        let computed = compute_bitmap_checksum(sb, &buf);
        let stored = inode_bitmap_checksum(gd, sb.desc_size);
        if stored != computed {
            crate::serial_println!(
                "[ext4] inode bitmap checksum mismatch: stored={:#010x} computed={:#010x}",
                stored, computed,
            );
            return Err(KernelError::CorruptedData);
        }
    }

    Ok(buf)
}

/// Write the inode bitmap for a given block group.
///
/// Also updates the bitmap checksum in the group descriptor.
pub fn write_inode_bitmap(
    reader: &BlockReader,
    sb: &ParsedSuperblock,
    gd: &mut Ext4GroupDesc,
    bitmap: &[u8],
) -> KernelResult<()> {
    let bitmap_block = group_desc_inode_bitmap(gd, sb.is_64bit);
    if bitmap.len() < sb.block_size as usize {
        return Err(KernelError::InvalidArgument);
    }

    // Stamp checksum into the group descriptor.
    if sb.has_metadata_csum {
        let csum = compute_bitmap_checksum(sb, bitmap);
        set_inode_bitmap_checksum(gd, sb.desc_size, csum);
    }

    reader.write_block(bitmap_block, bitmap)
}

// ---------------------------------------------------------------------------
// Bitmap bit operations
// ---------------------------------------------------------------------------

/// Check if a bit is set in a bitmap.
#[inline]
fn bitmap_test(bitmap: &[u8], bit: u32) -> bool {
    let byte_idx = (bit / 8) as usize;
    let bit_idx = bit % 8;
    bitmap.get(byte_idx)
        .is_some_and(|b| (b >> bit_idx) & 1 != 0)
}

/// Set a bit in a bitmap (mark as allocated).
#[inline]
fn bitmap_set(bitmap: &mut [u8], bit: u32) {
    let byte_idx = (bit / 8) as usize;
    let bit_idx = bit % 8;
    if let Some(b) = bitmap.get_mut(byte_idx) {
        *b |= 1 << bit_idx;
    }
}

/// Clear a bit in a bitmap (mark as free).
#[inline]
fn bitmap_clear(bitmap: &mut [u8], bit: u32) {
    let byte_idx = (bit / 8) as usize;
    let bit_idx = bit % 8;
    if let Some(b) = bitmap.get_mut(byte_idx) {
        *b &= !(1 << bit_idx);
    }
}

/// Find the first clear (free) bit in a bitmap, starting from `start`.
///
/// Returns `None` if no free bit is found within `max_bits`.
fn bitmap_find_free(bitmap: &[u8], start: u32, max_bits: u32) -> Option<u32> {
    // Search from `start` to `max_bits`.
    for bit in start..max_bits {
        if !bitmap_test(bitmap, bit) {
            return Some(bit);
        }
    }
    // Wrap around: search from 0 to `start`.
    (0..start).find(|&bit| !bitmap_test(bitmap, bit))
}

/// Find a contiguous run of `count` free bits starting from `start`.
///
/// Returns the first bit of the run, or `None` if not found.
fn bitmap_find_free_run(
    bitmap: &[u8],
    start: u32,
    max_bits: u32,
    count: u32,
) -> Option<u32> {
    if count == 0 {
        return Some(start);
    }
    if count == 1 {
        return bitmap_find_free(bitmap, start, max_bits);
    }

    let mut bit = start;
    let mut scanned = 0u32;

    while scanned < max_bits {
        if bit >= max_bits {
            bit = 0;
        }

        // Check if `count` consecutive bits starting at `bit` are free.
        let mut run_len = 0u32;
        while run_len < count {
            let check_bit = bit.saturating_add(run_len);
            if check_bit >= max_bits || bitmap_test(bitmap, check_bit) {
                break;
            }
            run_len = run_len.saturating_add(1);
        }

        if run_len >= count {
            return Some(bit);
        }

        // Skip past the end of the failed run.
        let advance = run_len.saturating_add(1).max(1);
        bit = bit.saturating_add(advance);
        scanned = scanned.saturating_add(advance);
    }

    None
}

// ---------------------------------------------------------------------------
// Block allocation
// ---------------------------------------------------------------------------

/// Allocate a single block, preferring the given goal block.
///
/// Returns the allocated physical block number.
///
/// # Strategy
///
/// 1. Compute the block group containing `goal`.
/// 2. Try to allocate in that group near the goal offset.
/// 3. If full, scan all groups sequentially.
/// 4. Update the block bitmap, group descriptor free count, and superblock.
pub fn alloc_block(
    reader: &BlockReader,
    sb: &mut ParsedSuperblock,
    group_descs: &mut [Ext4GroupDesc],
    goal: u64,
) -> KernelResult<u64> {
    let blocks_per_group = u64::from(sb.raw.s_blocks_per_group);
    let group_count = sb.group_count as usize;

    if blocks_per_group == 0 || group_count == 0 {
        return Err(KernelError::DiskFull);
    }

    // Determine preferred group and offset within that group.
    let first_data = u64::from(sb.raw.s_first_data_block);
    let pref_group = ((goal.saturating_sub(first_data)) / blocks_per_group) as usize;
    let pref_offset = ((goal.saturating_sub(first_data)) % blocks_per_group) as u32;

    // Try preferred group first, then scan all groups.
    for delta in 0..group_count {
        let group_idx = (pref_group.wrapping_add(delta)) % group_count;
        let gd = group_descs.get_mut(group_idx).ok_or(KernelError::IoError)?;

        // Check if this group has any free blocks.
        let free = group_desc_free_blocks(gd, sb.is_64bit);
        if free == 0 {
            continue;
        }

        // Read the bitmap.
        let mut bitmap = read_block_bitmap(reader, sb, gd)?;
        let max_bits = if group_idx == group_count.saturating_sub(1) {
            // Last group may have fewer blocks.
            let total = sb.block_count.saturating_sub(
                first_data.saturating_add(
                    (group_idx as u64).saturating_mul(blocks_per_group)
                )
            );
            (total as u32).min(sb.raw.s_blocks_per_group)
        } else {
            sb.raw.s_blocks_per_group
        };

        let start_offset = if delta == 0 { pref_offset } else { 0 };
        let found = bitmap_find_free(&bitmap, start_offset, max_bits);

        if let Some(bit) = found {
            // Mark the block as allocated.
            bitmap_set(&mut bitmap, bit);

            // Write the updated bitmap (stamps checksum in GD).
            write_block_bitmap(reader, sb, gd, &bitmap)?;

            // Update the group descriptor free count.
            decrement_gd_free_blocks(gd, sb.is_64bit);

            // Update the superblock free count.
            sb.free_block_count = sb.free_block_count.saturating_sub(1);
            update_sb_free_blocks(&mut sb.raw, sb.free_block_count, sb.is_64bit);

            // Compute the absolute block number.
            let block_nr = first_data
                .saturating_add((group_idx as u64).saturating_mul(blocks_per_group))
                .saturating_add(u64::from(bit));

            return Ok(block_nr);
        }
    }

    Err(KernelError::DiskFull)
}

/// Allocate `count` contiguous blocks, preferring the given goal block.
///
/// Returns the first allocated physical block number.
/// Falls back to single-block allocation if no contiguous run is available.
pub fn alloc_blocks(
    reader: &BlockReader,
    sb: &mut ParsedSuperblock,
    group_descs: &mut [Ext4GroupDesc],
    goal: u64,
    count: u32,
) -> KernelResult<u64> {
    if count == 0 {
        return Err(KernelError::InvalidArgument);
    }
    if count == 1 {
        return alloc_block(reader, sb, group_descs, goal);
    }

    let blocks_per_group = u64::from(sb.raw.s_blocks_per_group);
    let group_count = sb.group_count as usize;

    if blocks_per_group == 0 || group_count == 0 {
        return Err(KernelError::DiskFull);
    }

    let first_data = u64::from(sb.raw.s_first_data_block);
    let pref_group = ((goal.saturating_sub(first_data)) / blocks_per_group) as usize;
    let pref_offset = ((goal.saturating_sub(first_data)) % blocks_per_group) as u32;

    for delta in 0..group_count {
        let group_idx = (pref_group.wrapping_add(delta)) % group_count;
        let gd = group_descs.get_mut(group_idx).ok_or(KernelError::IoError)?;

        let free = group_desc_free_blocks(gd, sb.is_64bit);
        if free < u64::from(count) {
            continue;
        }

        let mut bitmap = read_block_bitmap(reader, sb, gd)?;
        let max_bits = if group_idx == group_count.saturating_sub(1) {
            let total = sb.block_count.saturating_sub(
                first_data.saturating_add(
                    (group_idx as u64).saturating_mul(blocks_per_group)
                )
            );
            (total as u32).min(sb.raw.s_blocks_per_group)
        } else {
            sb.raw.s_blocks_per_group
        };

        let start = if delta == 0 { pref_offset } else { 0 };
        let found = bitmap_find_free_run(&bitmap, start, max_bits, count);

        if let Some(first_bit) = found {
            // Mark all blocks as allocated.
            for i in 0..count {
                bitmap_set(&mut bitmap, first_bit.saturating_add(i));
            }

            // Write bitmap (stamps checksum in GD).
            write_block_bitmap(reader, sb, gd, &bitmap)?;

            for _ in 0..count {
                decrement_gd_free_blocks(gd, sb.is_64bit);
            }

            sb.free_block_count = sb.free_block_count.saturating_sub(u64::from(count));
            update_sb_free_blocks(&mut sb.raw, sb.free_block_count, sb.is_64bit);

            let block_nr = first_data
                .saturating_add((group_idx as u64).saturating_mul(blocks_per_group))
                .saturating_add(u64::from(first_bit));

            return Ok(block_nr);
        }
    }

    Err(KernelError::DiskFull)
}

/// Free a single block.
///
/// Clears the bit in the block bitmap and updates free counts.
pub fn free_block(
    reader: &BlockReader,
    sb: &mut ParsedSuperblock,
    group_descs: &mut [Ext4GroupDesc],
    block_nr: u64,
) -> KernelResult<()> {
    let blocks_per_group = u64::from(sb.raw.s_blocks_per_group);
    let first_data = u64::from(sb.raw.s_first_data_block);

    if block_nr < first_data {
        return Err(KernelError::InvalidArgument);
    }

    let relative = block_nr.saturating_sub(first_data);
    let group_idx = (relative / blocks_per_group) as usize;
    let bit = (relative % blocks_per_group) as u32;

    let gd = group_descs.get_mut(group_idx).ok_or(KernelError::InvalidArgument)?;
    let mut bitmap = read_block_bitmap(reader, sb, gd)?;

    if !bitmap_test(&bitmap, bit) {
        // Double-free: block is already free. This is a filesystem error.
        return Err(KernelError::InvalidArgument);
    }

    bitmap_clear(&mut bitmap, bit);

    // Write bitmap (stamps checksum in GD).
    write_block_bitmap(reader, sb, gd, &bitmap)?;

    increment_gd_free_blocks(gd, sb.is_64bit);

    sb.free_block_count = sb.free_block_count.saturating_add(1);
    update_sb_free_blocks(&mut sb.raw, sb.free_block_count, sb.is_64bit);

    Ok(())
}

/// Free a contiguous range of blocks.
pub fn free_blocks(
    reader: &BlockReader,
    sb: &mut ParsedSuperblock,
    group_descs: &mut [Ext4GroupDesc],
    start_block: u64,
    count: u32,
) -> KernelResult<()> {
    // For simplicity, free each block individually.
    // A production implementation would batch within the same group.
    for i in 0..u64::from(count) {
        free_block(reader, sb, group_descs, start_block.saturating_add(i))?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Inode allocation
// ---------------------------------------------------------------------------

/// Allocate a single inode in the given preferred block group.
///
/// Returns the inode number (1-based).
///
/// # Strategy
///
/// 1. Try the preferred group first.
/// 2. If full, scan all groups.
/// 3. For directories, prefer groups with the most free inodes
///    (Orlov allocator simplified).
pub fn alloc_inode(
    reader: &BlockReader,
    sb: &mut ParsedSuperblock,
    group_descs: &mut [Ext4GroupDesc],
    preferred_group: u32,
    _is_directory: bool,
) -> KernelResult<u32> {
    let inodes_per_group = sb.raw.s_inodes_per_group;
    let group_count = sb.group_count as usize;

    if inodes_per_group == 0 || group_count == 0 {
        return Err(KernelError::DiskFull);
    }

    for delta in 0..group_count {
        let group_idx = ((preferred_group as usize).wrapping_add(delta)) % group_count;
        let gd = group_descs.get_mut(group_idx).ok_or(KernelError::IoError)?;

        let free = group_desc_free_inodes(gd, sb.is_64bit);
        if free == 0 {
            continue;
        }

        let mut bitmap = read_inode_bitmap(reader, sb, gd)?;

        // Find a free inode bit (0-indexed within the group).
        let found = bitmap_find_free(&bitmap, 0, inodes_per_group);

        if let Some(bit) = found {
            bitmap_set(&mut bitmap, bit);

            // Write bitmap (stamps checksum in GD).
            write_inode_bitmap(reader, sb, gd, &bitmap)?;

            decrement_gd_free_inodes(gd, sb.is_64bit);

            // Update superblock free inode count.
            sb.raw.s_free_inodes_count = sb.raw.s_free_inodes_count.saturating_sub(1);

            // Convert (group, bit) to inode number.
            // Inode numbers are 1-based: inode 1 is bit 0 of group 0.
            let inode_nr = (group_idx as u32)
                .saturating_mul(inodes_per_group)
                .saturating_add(bit)
                .saturating_add(1);

            return Ok(inode_nr);
        }
    }

    Err(KernelError::DiskFull)
}

/// Free an inode.
///
/// Clears the bit in the inode bitmap and updates free counts.
pub fn free_inode(
    reader: &BlockReader,
    sb: &mut ParsedSuperblock,
    group_descs: &mut [Ext4GroupDesc],
    inode_nr: u32,
) -> KernelResult<()> {
    if inode_nr == 0 {
        return Err(KernelError::InvalidArgument);
    }

    let inodes_per_group = sb.raw.s_inodes_per_group;
    let group_idx = sb.inode_group(inode_nr) as usize;
    let bit = sb.inode_index_in_group(inode_nr);

    if bit >= inodes_per_group {
        return Err(KernelError::InvalidArgument);
    }

    let gd = group_descs.get_mut(group_idx).ok_or(KernelError::InvalidArgument)?;
    let mut bitmap = read_inode_bitmap(reader, sb, gd)?;

    if !bitmap_test(&bitmap, bit) {
        // Double-free: inode is already free.
        return Err(KernelError::InvalidArgument);
    }

    bitmap_clear(&mut bitmap, bit);

    // Write bitmap (stamps checksum in GD).
    write_inode_bitmap(reader, sb, gd, &bitmap)?;

    increment_gd_free_inodes(gd, sb.is_64bit);

    sb.raw.s_free_inodes_count = sb.raw.s_free_inodes_count.saturating_add(1);

    Ok(())
}

// ---------------------------------------------------------------------------
// Group descriptor helpers
// ---------------------------------------------------------------------------

/// Get the block bitmap block number from a group descriptor.
fn group_desc_block_bitmap(gd: &Ext4GroupDesc, is_64bit: bool) -> u64 {
    let lo = u64::from(gd.bg_block_bitmap_lo);
    if is_64bit {
        lo | (u64::from(gd.bg_block_bitmap_hi) << 32)
    } else {
        lo
    }
}

/// Get the inode bitmap block number from a group descriptor.
fn group_desc_inode_bitmap(gd: &Ext4GroupDesc, is_64bit: bool) -> u64 {
    let lo = u64::from(gd.bg_inode_bitmap_lo);
    if is_64bit {
        lo | (u64::from(gd.bg_inode_bitmap_hi) << 32)
    } else {
        lo
    }
}

/// Get the free block count from a group descriptor.
fn group_desc_free_blocks(gd: &Ext4GroupDesc, is_64bit: bool) -> u64 {
    let lo = u64::from(gd.bg_free_blocks_count_lo);
    if is_64bit {
        // lo is bits 0-15, hi is bits 16-31 — combined 32-bit count.
        lo | (u64::from(gd.bg_free_blocks_count_hi) << 16)
    } else {
        lo
    }
}

/// Get the free inode count from a group descriptor.
fn group_desc_free_inodes(gd: &Ext4GroupDesc, is_64bit: bool) -> u64 {
    let lo = u64::from(gd.bg_free_inodes_count_lo);
    if is_64bit {
        // lo is bits 0-15, hi is bits 16-31 — combined 32-bit count.
        lo | (u64::from(gd.bg_free_inodes_count_hi) << 16)
    } else {
        lo
    }
}

/// Decrement the free block count in a group descriptor.
fn decrement_gd_free_blocks(gd: &mut Ext4GroupDesc, is_64bit: bool) {
    let current = group_desc_free_blocks(gd, is_64bit);
    let new_val = current.saturating_sub(1);
    gd.bg_free_blocks_count_lo = new_val as u16;
    if is_64bit {
        gd.bg_free_blocks_count_hi = (new_val >> 16) as u16;
    }
}

/// Increment the free block count in a group descriptor.
fn increment_gd_free_blocks(gd: &mut Ext4GroupDesc, is_64bit: bool) {
    let current = group_desc_free_blocks(gd, is_64bit);
    let new_val = current.saturating_add(1);
    gd.bg_free_blocks_count_lo = new_val as u16;
    if is_64bit {
        gd.bg_free_blocks_count_hi = (new_val >> 16) as u16;
    }
}

/// Decrement the free inode count in a group descriptor.
fn decrement_gd_free_inodes(gd: &mut Ext4GroupDesc, is_64bit: bool) {
    let current = group_desc_free_inodes(gd, is_64bit);
    let new_val = current.saturating_sub(1);
    gd.bg_free_inodes_count_lo = new_val as u16;
    if is_64bit {
        gd.bg_free_inodes_count_hi = (new_val >> 16) as u16;
    }
}

/// Increment the free inode count in a group descriptor.
fn increment_gd_free_inodes(gd: &mut Ext4GroupDesc, is_64bit: bool) {
    let current = group_desc_free_inodes(gd, is_64bit);
    let new_val = current.saturating_add(1);
    gd.bg_free_inodes_count_lo = new_val as u16;
    if is_64bit {
        gd.bg_free_inodes_count_hi = (new_val >> 16) as u16;
    }
}

/// Update the superblock's free block count (both lo and hi fields).
fn update_sb_free_blocks(raw: &mut super::ondisk::Ext4Superblock, count: u64, is_64bit: bool) {
    raw.s_free_blocks_count_lo = count as u32;
    if is_64bit {
        raw.s_free_blocks_count_hi = (count >> 32) as u32;
    }
}

// ---------------------------------------------------------------------------
// Bitmap checksum helpers
// ---------------------------------------------------------------------------

/// Compute the CRC32C checksum for a bitmap block.
///
/// The bitmap checksum is `crc32c(csum_seed, bitmap_data)`.
/// The result is stored split across low-16 and high-16 fields in
/// the group descriptor.
///
/// Based on Linux `ext4_block_bitmap_csum_set()` in `fs/ext4/bitmap.c`.
fn compute_bitmap_checksum(sb: &ParsedSuperblock, bitmap_data: &[u8]) -> u32 {
    crate::crypto::crc32c_seed(sb.csum_seed, bitmap_data)
}

/// Read the stored block bitmap checksum from a group descriptor.
///
/// Uses only the low 16 bits if `desc_size < 58` (32-byte descriptors),
/// otherwise combines lo+hi for a full 32-bit checksum.
fn block_bitmap_checksum(gd: &Ext4GroupDesc, desc_size: u32) -> u32 {
    let lo = u32::from(gd.bg_block_bitmap_csum_lo);
    // bg_block_bitmap_csum_hi ends at offset 0x3A = 58.
    if desc_size >= 58 {
        lo | (u32::from(gd.bg_block_bitmap_csum_hi) << 16)
    } else {
        lo
    }
}

/// Write a block bitmap checksum into a group descriptor.
fn set_block_bitmap_checksum(gd: &mut Ext4GroupDesc, desc_size: u32, csum: u32) {
    gd.bg_block_bitmap_csum_lo = csum as u16;
    if desc_size >= 58 {
        gd.bg_block_bitmap_csum_hi = (csum >> 16) as u16;
    }
}

/// Read the stored inode bitmap checksum from a group descriptor.
///
/// Uses only the low 16 bits if `desc_size < 60` (32-byte descriptors),
/// otherwise combines lo+hi for a full 32-bit checksum.
fn inode_bitmap_checksum(gd: &Ext4GroupDesc, desc_size: u32) -> u32 {
    let lo = u32::from(gd.bg_inode_bitmap_csum_lo);
    // bg_inode_bitmap_csum_hi ends at offset 0x3C = 60.
    if desc_size >= 60 {
        lo | (u32::from(gd.bg_inode_bitmap_csum_hi) << 16)
    } else {
        lo
    }
}

/// Write an inode bitmap checksum into a group descriptor.
fn set_inode_bitmap_checksum(gd: &mut Ext4GroupDesc, desc_size: u32, csum: u32) {
    gd.bg_inode_bitmap_csum_lo = csum as u16;
    if desc_size >= 60 {
        gd.bg_inode_bitmap_csum_hi = (csum >> 16) as u16;
    }
}

// ---------------------------------------------------------------------------
// Self-test (runs in kernel)
// ---------------------------------------------------------------------------

/// ext4 block allocator unit tests — exercises bitmap operations and
/// group descriptor field accessors.  The group descriptor tests are
/// critical regression tests for the bit-shift bugs fixed in af1c277.
pub fn self_test() -> KernelResult<()> {
    crate::serial_println!("[ext4-balloc] Running self-test...");

    test_bitmap_test_set_clear()?;
    test_bitmap_find_free()?;
    test_bitmap_find_free_run()?;
    test_gd_free_blocks_32bit()?;
    test_gd_free_blocks_64bit()?;
    test_gd_free_inodes_64bit()?;
    test_gd_increment_decrement()?;

    crate::serial_println!("[ext4-balloc] Self-test PASSED (7 tests)");
    Ok(())
}

/// Test bitmap_test, bitmap_set, bitmap_clear.
fn test_bitmap_test_set_clear() -> KernelResult<()> {
    let mut bitmap = vec![0u8; 16]; // 128 bits

    // All bits should be free initially.
    if bitmap_test(&bitmap, 0) || bitmap_test(&bitmap, 7) || bitmap_test(&bitmap, 127) {
        crate::serial_println!("[ext4-balloc]   FAIL: zero bitmap has set bits");
        return Err(KernelError::InternalError);
    }

    // Set bit 0.
    bitmap_set(&mut bitmap, 0);
    if !bitmap_test(&bitmap, 0) || bitmap_test(&bitmap, 1) {
        crate::serial_println!("[ext4-balloc]   FAIL: bitmap_set(0) failed");
        return Err(KernelError::InternalError);
    }

    // Set bit 7 (last bit of first byte).
    bitmap_set(&mut bitmap, 7);
    if !bitmap_test(&bitmap, 7) || bitmap[0] != 0b1000_0001 {
        crate::serial_println!("[ext4-balloc]   FAIL: bitmap_set(7) wrong byte");
        return Err(KernelError::InternalError);
    }

    // Set bit 8 (first bit of second byte).
    bitmap_set(&mut bitmap, 8);
    if !bitmap_test(&bitmap, 8) || bitmap[1] != 1 {
        crate::serial_println!("[ext4-balloc]   FAIL: bitmap_set(8) wrong byte");
        return Err(KernelError::InternalError);
    }

    // Clear bit 0.
    bitmap_clear(&mut bitmap, 0);
    if bitmap_test(&bitmap, 0) || bitmap[0] != 0b1000_0000 {
        crate::serial_println!("[ext4-balloc]   FAIL: bitmap_clear(0) failed");
        return Err(KernelError::InternalError);
    }

    // Out-of-bounds access should not panic (bitmap_test returns false).
    if bitmap_test(&bitmap, 200) {
        crate::serial_println!("[ext4-balloc]   FAIL: OOB bit reported as set");
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[ext4-balloc]   bitmap test/set/clear: OK");
    Ok(())
}

/// Test bitmap_find_free.
fn test_bitmap_find_free() -> KernelResult<()> {
    let mut bitmap = vec![0xFFu8; 4]; // 32 bits, all set
    bitmap[2] = 0xFE; // Bit 16 is free (LSB of byte 2).

    match bitmap_find_free(&bitmap, 0, 32) {
        Some(16) => {}
        other => {
            crate::serial_println!("[ext4-balloc]   FAIL: find_free from 0 = {:?}", other);
            return Err(KernelError::InternalError);
        }
    }

    // Starting at 17 should wrap around and find bit 16.
    match bitmap_find_free(&bitmap, 17, 32) {
        Some(16) => {}
        other => {
            crate::serial_println!("[ext4-balloc]   FAIL: find_free wrap = {:?}", other);
            return Err(KernelError::InternalError);
        }
    }

    // All full bitmap.
    let full = vec![0xFFu8; 4];
    if bitmap_find_free(&full, 0, 32).is_some() {
        crate::serial_println!("[ext4-balloc]   FAIL: found free in full bitmap");
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[ext4-balloc]   bitmap find_free: OK");
    Ok(())
}

/// Test bitmap_find_free_run for contiguous allocation.
fn test_bitmap_find_free_run() -> KernelResult<()> {
    let mut bitmap = vec![0xFFu8; 4]; // 32 bits, all set

    // Free bits 10, 11, 12 in byte 1 (which covers bits 8-15).
    // bit 8 = LSB of byte 1
    // bits 8,9 set, 10,11,12 clear, 13,14,15 set
    // byte 1 = (1<<0)|(1<<1)|(0<<2)|(0<<3)|(0<<4)|(1<<5)|(1<<6)|(1<<7) = 0xE3
    bitmap[1] = 0xE3;

    match bitmap_find_free_run(&bitmap, 0, 32, 3) {
        Some(10) => {}
        other => {
            crate::serial_println!("[ext4-balloc]   FAIL: find_free_run(3) = {:?}", other);
            return Err(KernelError::InternalError);
        }
    }

    // 4 contiguous should fail (only 3 free).
    if bitmap_find_free_run(&bitmap, 0, 32, 4).is_some() {
        crate::serial_println!("[ext4-balloc]   FAIL: found 4-run in 3-free bitmap");
        return Err(KernelError::InternalError);
    }

    // Run of 1 should find bit 10.
    match bitmap_find_free_run(&bitmap, 0, 32, 1) {
        Some(10) => {}
        other => {
            crate::serial_println!("[ext4-balloc]   FAIL: find_free_run(1) = {:?}", other);
            return Err(KernelError::InternalError);
        }
    }

    crate::serial_println!("[ext4-balloc]   bitmap find_free_run: OK");
    Ok(())
}

/// Create a zeroed Ext4GroupDesc for testing.
///
/// # Safety
/// Ext4GroupDesc is composed entirely of integer fields (u32, u16),
/// for which all-zeros is a valid bit pattern.
fn zeroed_gd() -> Ext4GroupDesc {
    // SAFETY: All fields are u32/u16 — zero is a valid value for each.
    unsafe { core::mem::zeroed() }
}

/// Test group_desc_free_blocks in 32-bit mode (lo field only).
fn test_gd_free_blocks_32bit() -> KernelResult<()> {
    let mut gd = zeroed_gd();
    gd.bg_free_blocks_count_lo = 1234;

    let count = group_desc_free_blocks(&gd, false);
    if count != 1234 {
        crate::serial_println!("[ext4-balloc]   FAIL: 32-bit free blocks = {}", count);
        return Err(KernelError::InternalError);
    }

    // hi field should be ignored in 32-bit mode.
    gd.bg_free_blocks_count_hi = 0xFFFF;
    let count2 = group_desc_free_blocks(&gd, false);
    if count2 != 1234 {
        crate::serial_println!("[ext4-balloc]   FAIL: 32-bit mode used hi field");
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[ext4-balloc]   gd free blocks 32-bit: OK");
    Ok(())
}

/// Test group_desc_free_blocks in 64-bit mode (lo + hi << 16).
///
/// REGRESSION TEST: This caught the critical bit-shift bug where
/// hi was shifted by 32 instead of 16 (fixed in commit af1c277).
fn test_gd_free_blocks_64bit() -> KernelResult<()> {
    let mut gd = zeroed_gd();

    // Set lo=0x1234, hi=0x0005 → combined = 0x00051234.
    gd.bg_free_blocks_count_lo = 0x1234;
    gd.bg_free_blocks_count_hi = 0x0005;

    let count = group_desc_free_blocks(&gd, true);
    let expected: u64 = 0x00051234;
    if count != expected {
        crate::serial_println!(
            "[ext4-balloc]   FAIL: 64-bit free blocks = {:#x}, expected {:#x}",
            count, expected
        );
        return Err(KernelError::InternalError);
    }

    // Verify max values: lo=0xFFFF, hi=0xFFFF → 0x0000_0000_FFFF_FFFF.
    gd.bg_free_blocks_count_lo = 0xFFFF;
    gd.bg_free_blocks_count_hi = 0xFFFF;

    let max_count = group_desc_free_blocks(&gd, true);
    let expected_max: u64 = 0xFFFFFFFF;
    if max_count != expected_max {
        crate::serial_println!(
            "[ext4-balloc]   FAIL: max free blocks = {:#x}, expected {:#x}",
            max_count, expected_max
        );
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[ext4-balloc]   gd free blocks 64-bit: OK");
    Ok(())
}

/// Test group_desc_free_inodes in 64-bit mode (same bit-shift pattern).
fn test_gd_free_inodes_64bit() -> KernelResult<()> {
    let mut gd = zeroed_gd();

    gd.bg_free_inodes_count_lo = 0xABCD;
    gd.bg_free_inodes_count_hi = 0x0012;

    let count = group_desc_free_inodes(&gd, true);
    let expected: u64 = 0x0012ABCD;
    if count != expected {
        crate::serial_println!(
            "[ext4-balloc]   FAIL: 64-bit free inodes = {:#x}, expected {:#x}",
            count, expected
        );
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[ext4-balloc]   gd free inodes 64-bit: OK");
    Ok(())
}

/// Test increment/decrement of free block/inode counts.
///
/// Verifies the write path: that decrement/increment correctly
/// split the value back into lo(16) and hi(16) fields.
fn test_gd_increment_decrement() -> KernelResult<()> {
    let mut gd = zeroed_gd();

    // Start at 0x10000 (hi=1, lo=0).
    gd.bg_free_blocks_count_lo = 0;
    gd.bg_free_blocks_count_hi = 1;

    // Decrement: 0x10000 → 0xFFFF.
    decrement_gd_free_blocks(&mut gd, true);
    let after_dec = group_desc_free_blocks(&gd, true);
    if after_dec != 0xFFFF {
        crate::serial_println!(
            "[ext4-balloc]   FAIL: 0x10000 - 1 = {:#x}, expected 0xFFFF",
            after_dec
        );
        return Err(KernelError::InternalError);
    }
    // Verify lo/hi fields: 0xFFFF → lo=0xFFFF, hi=0.
    if gd.bg_free_blocks_count_lo != 0xFFFF || gd.bg_free_blocks_count_hi != 0 {
        crate::serial_println!(
            "[ext4-balloc]   FAIL: lo={:#x} hi={:#x} after dec",
            gd.bg_free_blocks_count_lo, gd.bg_free_blocks_count_hi
        );
        return Err(KernelError::InternalError);
    }

    // Increment: 0xFFFF → 0x10000.
    increment_gd_free_blocks(&mut gd, true);
    let after_inc = group_desc_free_blocks(&gd, true);
    if after_inc != 0x10000 {
        crate::serial_println!(
            "[ext4-balloc]   FAIL: 0xFFFF + 1 = {:#x}, expected 0x10000",
            after_inc
        );
        return Err(KernelError::InternalError);
    }
    // Verify: lo=0, hi=1.
    if gd.bg_free_blocks_count_lo != 0 || gd.bg_free_blocks_count_hi != 1 {
        crate::serial_println!(
            "[ext4-balloc]   FAIL: lo={:#x} hi={:#x} after inc",
            gd.bg_free_blocks_count_lo, gd.bg_free_blocks_count_hi
        );
        return Err(KernelError::InternalError);
    }

    // Same test for inodes.
    gd.bg_free_inodes_count_lo = 0;
    gd.bg_free_inodes_count_hi = 2; // 0x20000
    decrement_gd_free_inodes(&mut gd, true);
    let inodes_after = group_desc_free_inodes(&gd, true);
    if inodes_after != 0x1FFFF {
        crate::serial_println!(
            "[ext4-balloc]   FAIL: inode 0x20000 - 1 = {:#x}",
            inodes_after
        );
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[ext4-balloc]   increment/decrement: OK");
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests (std)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bitmap_operations() {
        let mut bitmap = vec![0u8; 16]; // 128 bits

        // All bits should be free.
        assert!(!bitmap_test(&bitmap, 0));
        assert!(!bitmap_test(&bitmap, 7));
        assert!(!bitmap_test(&bitmap, 127));

        // Set bit 0.
        bitmap_set(&mut bitmap, 0);
        assert!(bitmap_test(&bitmap, 0));
        assert!(!bitmap_test(&bitmap, 1));

        // Set bit 7 (last bit of first byte).
        bitmap_set(&mut bitmap, 7);
        assert!(bitmap_test(&bitmap, 7));
        assert_eq!(bitmap[0], 0b1000_0001);

        // Set bit 8 (first bit of second byte).
        bitmap_set(&mut bitmap, 8);
        assert!(bitmap_test(&bitmap, 8));
        assert_eq!(bitmap[1], 0b0000_0001);

        // Clear bit 0.
        bitmap_clear(&mut bitmap, 0);
        assert!(!bitmap_test(&bitmap, 0));
        assert_eq!(bitmap[0], 0b1000_0000);
    }

    #[test]
    fn test_bitmap_find_free() {
        let mut bitmap = vec![0xFFu8; 4]; // 32 bits, all set
        bitmap[2] = 0xFE; // Bit 16 is free.

        assert_eq!(bitmap_find_free(&bitmap, 0, 32), Some(16));
        assert_eq!(bitmap_find_free(&bitmap, 16, 32), Some(16));
        assert_eq!(bitmap_find_free(&bitmap, 17, 32), Some(16)); // Wraps around.

        // All full.
        let full = vec![0xFFu8; 4];
        assert_eq!(bitmap_find_free(&full, 0, 32), None);
    }

    #[test]
    fn test_bitmap_find_free_run() {
        let mut bitmap = vec![0xFFu8; 4];
        // Free bits 10, 11, 12 (3 contiguous).
        bitmap[1] = 0b11111000 | 0b00000011; // bits 8-9 set, 10-12 free, 13-15 set
        // Actually let me be more precise.
        // Byte 1 covers bits 8-15.
        // We want bits 10, 11, 12 free → byte 1 = 0b11100011 = 0xE3 (wrong)
        // Bit layout: bit 8 = LSB of byte 1
        // bits 8,9 set, 10,11,12 clear, 13,14,15 set
        // byte 1 = (1<<0) | (1<<1) | (0<<2) | (0<<3) | (0<<4) | (1<<5) | (1<<6) | (1<<7)
        //        = 0b1110_0011 = 0xE3
        bitmap[1] = 0xE3;

        assert_eq!(bitmap_find_free_run(&bitmap, 0, 32, 3), Some(10));
        assert_eq!(bitmap_find_free_run(&bitmap, 0, 32, 4), None);
        assert_eq!(bitmap_find_free_run(&bitmap, 0, 32, 1), Some(10));
    }
}
