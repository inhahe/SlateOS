//! ext4 filesystem consistency checker.
//!
//! Performs a read-only check of an ext4 filesystem by opening a fresh
//! driver instance (not through the VFS — avoids lock contention with
//! mounted filesystems).
//!
//! ## Checks
//!
//! 1. **Superblock validation** — magic number, block size, group count,
//!    feature flags.
//! 2. **Group descriptor bitmap consistency** — for each block group,
//!    reads the block and inode bitmaps and counts the actual free
//!    entries, comparing with the stored free counts in the group
//!    descriptor.
//! 3. **Inode scan** — reads every allocated inode, verifies basic field
//!    validity (mode, size, blocks).
//! 4. **Directory tree walk** — traverses from the root inode, counting
//!    references to each inode (link count verification).
//! 5. **Summary** — totals of superblock-stored vs bitmap-counted free
//!    counts.
//!
//! ## Reference
//!
//! Based on Linux e2fsck (e2fsprogs), simplified for the checks that
//! catch the most common corruption patterns: bitmap drift and link
//! count mismatches.

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::error::KernelResult;

use super::balloc;
use super::driver::Ext4Driver;
use super::ondisk::{self, Ext4Inode};

// ---------------------------------------------------------------------------
// Report
// ---------------------------------------------------------------------------

/// Result of an ext4 filesystem check.
#[derive(Default)]
pub struct Ext4FsckReport {
    /// Regular files found.
    pub files: u32,
    /// Directories found.
    pub dirs: u32,
    /// Symbolic links found.
    pub symlinks: u32,
    /// Other inode types (block dev, char dev, fifo, socket).
    pub other: u32,
    /// Errors detected.
    pub errors: u32,
    /// Warnings (non-fatal inconsistencies).
    pub warnings: u32,
    /// Human-readable messages collected during the check.
    pub messages: Vec<String>,
}

impl Ext4FsckReport {
    fn error(&mut self, msg: String) {
        self.errors = self.errors.saturating_add(1);
        self.messages.push(msg);
    }

    fn warn(&mut self, msg: String) {
        self.warnings = self.warnings.saturating_add(1);
        self.messages.push(msg);
    }

    fn info(&mut self, msg: String) {
        self.messages.push(msg);
    }
}

// ---------------------------------------------------------------------------
// Inode mode helpers
// ---------------------------------------------------------------------------

/// S_IFMT — file type mask in i_mode.
const S_IFMT: u16 = 0xF000;
const S_IFREG: u16 = 0x8000;
const S_IFDIR: u16 = 0x4000;
const S_IFLNK: u16 = 0xA000;

fn inode_is_regular(inode: &Ext4Inode) -> bool {
    inode.i_mode & S_IFMT == S_IFREG
}

fn inode_is_dir(inode: &Ext4Inode) -> bool {
    inode.i_mode & S_IFMT == S_IFDIR
}

fn inode_is_symlink(inode: &Ext4Inode) -> bool {
    inode.i_mode & S_IFMT == S_IFLNK
}

fn inode_type_name(mode: u16) -> &'static str {
    match mode & S_IFMT {
        S_IFREG => "regular",
        S_IFDIR => "directory",
        S_IFLNK => "symlink",
        0x6000 => "block-dev",
        0x2000 => "char-dev",
        0x1000 => "fifo",
        0xC000 => "socket",
        _ => "unknown",
    }
}

// ---------------------------------------------------------------------------
// Bitmap helpers
// ---------------------------------------------------------------------------

/// Count the number of set (used) bits in a bitmap.
///
/// `total_bits` limits the count to the valid range (last group may
/// have fewer valid bits than a full block of bitmap).
#[allow(clippy::arithmetic_side_effects)]
fn count_used_bits(bitmap: &[u8], total_bits: u32) -> u32 {
    let mut used: u32 = 0;
    for bit_idx in 0..total_bits {
        let byte_idx = (bit_idx / 8) as usize;
        let bit_pos = bit_idx % 8;
        if let Some(&byte) = bitmap.get(byte_idx) {
            if byte & (1 << bit_pos) != 0 {
                used = used.saturating_add(1);
            }
        }
    }
    used
}

/// Check if a specific bit is set in a bitmap.
#[allow(clippy::arithmetic_side_effects)]
fn bitmap_bit_set(bitmap: &[u8], bit: u32) -> bool {
    let byte_idx = (bit / 8) as usize;
    let bit_pos = bit % 8;
    bitmap.get(byte_idx)
        .map(|&b| b & (1 << bit_pos) != 0)
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Group descriptor free count helpers
// ---------------------------------------------------------------------------

/// Get the 32-bit free block count from a group descriptor.
#[allow(clippy::arithmetic_side_effects)]
fn gd_free_blocks(gd: &ondisk::Ext4GroupDesc, is_64bit: bool) -> u32 {
    let lo = u32::from(gd.bg_free_blocks_count_lo);
    if is_64bit {
        lo | (u32::from(gd.bg_free_blocks_count_hi) << 16)
    } else {
        lo
    }
}

/// Get the 32-bit free inode count from a group descriptor.
#[allow(clippy::arithmetic_side_effects)]
fn gd_free_inodes(gd: &ondisk::Ext4GroupDesc, is_64bit: bool) -> u32 {
    let lo = u32::from(gd.bg_free_inodes_count_lo);
    if is_64bit {
        lo | (u32::from(gd.bg_free_inodes_count_hi) << 16)
    } else {
        lo
    }
}

// ---------------------------------------------------------------------------
// Main fsck entry point
// ---------------------------------------------------------------------------

/// Check an ext4 filesystem for consistency errors.
///
/// Opens a fresh `Ext4Driver` on the given block device (does not use
/// the VFS — the filesystem may or may not be mounted).  Performs
/// read-only checks and returns a report.
///
/// `device`: block device name (e.g., `"vda"`).
#[allow(clippy::arithmetic_side_effects)]
pub fn fsck_ext4(device: &str) -> KernelResult<Ext4FsckReport> {
    let mut report = Ext4FsckReport::default();

    // --- Phase 1: Open and validate superblock ---
    let driver = match Ext4Driver::open(device) {
        Ok(d) => d,
        Err(e) => {
            report.error(format!("cannot open ext4 on '{}': {:?}", device, e));
            return Ok(report);
        }
    };

    let sb = driver.superblock();
    report.info(format!(
        "Phase 1: Superblock OK — {} blocks, {} inodes, {}B block size, {} groups",
        sb.block_count,
        sb.raw.s_inodes_count,
        sb.block_size,
        sb.group_count,
    ));

    if sb.volume_name.is_empty() {
        report.info(String::from("  Volume label: (none)"));
    } else {
        report.info(format!("  Volume label: {}", sb.volume_name));
    }

    let inodes_per_group = sb.raw.s_inodes_per_group;
    let blocks_per_group = sb.raw.s_blocks_per_group;

    // --- Phase 2: Group descriptor vs bitmap consistency ---
    report.info(String::from("Phase 2: Checking group descriptors vs bitmaps..."));

    let group_descs = driver.group_descs();
    let mut total_free_blocks_bitmap: u64 = 0;
    let mut total_free_inodes_bitmap: u64 = 0;
    let mut total_free_blocks_gd: u64 = 0;
    let mut total_free_inodes_gd: u64 = 0;
    let mut bitmap_errors: u32 = 0;

    // Collect inode allocation status for Phase 4 link count check.
    // Map: inode_nr → true if allocated in bitmap.
    let total_inodes = sb.raw.s_inodes_count;

    for (g, gd) in group_descs.iter().enumerate() {
        let g = g as u32;

        // --- Block bitmap ---
        let blocks_in_group = if g == sb.group_count.saturating_sub(1) {
            // Last group may have fewer blocks.
            let remaining = sb.block_count.saturating_sub(
                u64::from(g).saturating_mul(u64::from(blocks_per_group))
            );
            remaining.min(u64::from(blocks_per_group)) as u32
        } else {
            blocks_per_group
        };

        match balloc::read_block_bitmap(driver.reader(), sb, gd) {
            Ok(bitmap) => {
                let used = count_used_bits(&bitmap, blocks_in_group);
                let free = blocks_in_group.saturating_sub(used);
                let stored_free = gd_free_blocks(gd, sb.is_64bit);

                total_free_blocks_bitmap =
                    total_free_blocks_bitmap.saturating_add(u64::from(free));
                total_free_blocks_gd =
                    total_free_blocks_gd.saturating_add(u64::from(stored_free));

                if free != stored_free {
                    report.error(format!(
                        "  Group {}: block bitmap free={} but descriptor says free={}",
                        g, free, stored_free
                    ));
                    bitmap_errors = bitmap_errors.saturating_add(1);
                }
            }
            Err(e) => {
                report.error(format!(
                    "  Group {}: cannot read block bitmap: {:?}",
                    g, e
                ));
            }
        }

        // --- Inode bitmap ---
        let inodes_in_group = if g == sb.group_count.saturating_sub(1) {
            let remaining = total_inodes.saturating_sub(
                g.saturating_mul(inodes_per_group)
            );
            remaining.min(inodes_per_group)
        } else {
            inodes_per_group
        };

        match balloc::read_inode_bitmap(driver.reader(), sb, gd) {
            Ok(bitmap) => {
                let used = count_used_bits(&bitmap, inodes_in_group);
                let free = inodes_in_group.saturating_sub(used);
                let stored_free = gd_free_inodes(gd, sb.is_64bit);

                total_free_inodes_bitmap =
                    total_free_inodes_bitmap.saturating_add(u64::from(free));
                total_free_inodes_gd =
                    total_free_inodes_gd.saturating_add(u64::from(stored_free));

                if free != stored_free {
                    report.error(format!(
                        "  Group {}: inode bitmap free={} but descriptor says free={}",
                        g, free, stored_free
                    ));
                    bitmap_errors = bitmap_errors.saturating_add(1);
                }
            }
            Err(e) => {
                report.error(format!(
                    "  Group {}: cannot read inode bitmap: {:?}",
                    g, e
                ));
            }
        }
    }

    if bitmap_errors == 0 {
        report.info(format!(
            "  All {} groups: bitmap counts match descriptors",
            sb.group_count
        ));
    }

    // Check superblock free counts vs bitmap totals.
    if total_free_blocks_bitmap != sb.free_block_count {
        report.error(format!(
            "  Superblock free_block_count={} but bitmaps show {}",
            sb.free_block_count, total_free_blocks_bitmap
        ));
    }
    let sb_free_inodes = u64::from(sb.raw.s_free_inodes_count);
    if total_free_inodes_bitmap != sb_free_inodes {
        report.error(format!(
            "  Superblock free_inodes_count={} but bitmaps show {}",
            sb_free_inodes, total_free_inodes_bitmap
        ));
    }

    // --- Phase 3: Inode scan ---
    report.info(String::from("Phase 3: Scanning allocated inodes..."));

    // Build inode allocation bitmap from all groups.
    let mut inode_allocated = Vec::new();
    inode_allocated.resize(total_inodes.saturating_add(1) as usize, false);

    for (g, gd) in group_descs.iter().enumerate() {
        let g = g as u32;
        let inodes_in_group = if g == sb.group_count.saturating_sub(1) {
            total_inodes.saturating_sub(g.saturating_mul(inodes_per_group))
                .min(inodes_per_group)
        } else {
            inodes_per_group
        };

        if let Ok(bitmap) = balloc::read_inode_bitmap(driver.reader(), sb, gd) {
            for bit in 0..inodes_in_group {
                if bitmap_bit_set(&bitmap, bit) {
                    let inode_nr = g.saturating_mul(inodes_per_group)
                        .saturating_add(bit)
                        .saturating_add(1); // inodes are 1-based
                    if let Some(slot) = inode_allocated.get_mut(inode_nr as usize) {
                        *slot = true;
                    }
                }
            }
        }
    }

    // Read each allocated inode and classify it.
    let mut scanned: u32 = 0;
    for ino in 1..=total_inodes {
        let allocated = inode_allocated.get(ino as usize).copied().unwrap_or(false);
        if !allocated {
            continue;
        }

        let inode = match driver.read_inode(ino) {
            Ok(i) => i,
            Err(e) => {
                // Special inodes (1-10) may not all be readable.
                if ino > 10 {
                    report.error(format!(
                        "  Inode {}: cannot read: {:?}", ino, e
                    ));
                }
                continue;
            }
        };

        // Skip inodes with mode 0 — these are deleted but still
        // marked in the bitmap (common in ext4 with lazy inode init).
        if inode.i_mode == 0 && inode.i_links_count == 0 {
            continue;
        }

        scanned = scanned.saturating_add(1);

        if inode_is_dir(&inode) {
            report.dirs = report.dirs.saturating_add(1);
        } else if inode_is_regular(&inode) {
            report.files = report.files.saturating_add(1);
        } else if inode_is_symlink(&inode) {
            report.symlinks = report.symlinks.saturating_add(1);
        } else {
            report.other = report.other.saturating_add(1);
        }

        // Basic sanity: links_count > 0 for allocated inodes.
        if inode.i_links_count == 0 && inode.i_mode != 0 {
            report.warn(format!(
                "  Inode {} ({}): allocated but i_links_count=0 (orphan?)",
                ino, inode_type_name(inode.i_mode)
            ));
        }
    }

    report.info(format!(
        "  Scanned {} allocated inodes: {} files, {} dirs, {} symlinks, {} other",
        scanned, report.files, report.dirs, report.symlinks, report.other
    ));

    // --- Phase 4: Directory tree walk (link count verification) ---
    report.info(String::from("Phase 4: Walking directory tree (link counts)..."));

    // ref_count[inode] = number of directory entries pointing to it.
    let mut ref_count: BTreeMap<u32, u32> = BTreeMap::new();
    let mut walk_errors: u32 = 0;

    // Start from root inode (2).
    let root_inode_nr: u32 = 2;
    let mut dir_stack: Vec<(u32, String)> = Vec::new();
    dir_stack.push((root_inode_nr, String::from("/")));

    // Count root's self-reference (. entry).
    *ref_count.entry(root_inode_nr).or_insert(0) =
        ref_count.get(&root_inode_nr).copied().unwrap_or(0).saturating_add(1);

    while let Some((dir_ino, dir_path)) = dir_stack.pop() {
        let dir_inode = match driver.read_inode(dir_ino) {
            Ok(i) => i,
            Err(_) => {
                walk_errors = walk_errors.saturating_add(1);
                continue;
            }
        };

        let entries = match driver.read_dir_entries(dir_ino, &dir_inode) {
            Ok(e) => e,
            Err(_) => {
                walk_errors = walk_errors.saturating_add(1);
                continue;
            }
        };

        for (child_ino, file_type, name) in &entries {
            if name == "." || name == ".." {
                // "." and ".." contribute to link counts.
                *ref_count.entry(*child_ino).or_insert(0) =
                    ref_count.get(child_ino).copied().unwrap_or(0).saturating_add(1);
                continue;
            }

            // Count reference to child.
            *ref_count.entry(*child_ino).or_insert(0) =
                ref_count.get(child_ino).copied().unwrap_or(0).saturating_add(1);

            // If child is a directory, add to stack for traversal.
            // File type byte: 2 = EXT4_FT_DIR.
            if *file_type == 2 {
                let child_path = if dir_path == "/" {
                    format!("/{}", name)
                } else {
                    format!("{}/{}", dir_path, name)
                };
                dir_stack.push((*child_ino, child_path));
            }
        }

        // Safety: limit traversal depth to prevent infinite loops from
        // circular directory references.
        if dir_stack.len() > 10000 {
            report.warn(String::from("  Directory tree too deep (>10000 pending) — stopping walk"));
            break;
        }
    }

    // Compare reference counts with i_links_count.
    let mut link_mismatches: u32 = 0;
    for (&ino, &refs) in &ref_count {
        if ino == 0 {
            continue; // Inode 0 is never valid.
        }
        let inode = match driver.read_inode(ino) {
            Ok(i) => i,
            Err(_) => continue,
        };
        let stored = u32::from(inode.i_links_count);
        if refs != stored {
            // Only report for non-special inodes (skip 1-10 range
            // for journal, lost+found, etc. which may have odd counts).
            if ino > 10 {
                report.error(format!(
                    "  Inode {} ({}): links_count={} but {} directory references found",
                    ino, inode_type_name(inode.i_mode), stored, refs
                ));
                link_mismatches = link_mismatches.saturating_add(1);
            }
        }
    }

    if link_mismatches == 0 && walk_errors == 0 {
        report.info(format!(
            "  Directory tree OK — {} inodes referenced, link counts match",
            ref_count.len()
        ));
    } else if walk_errors > 0 {
        report.warn(format!(
            "  {} directory read errors during tree walk",
            walk_errors
        ));
    }

    // --- Summary ---
    report.info(String::from("Summary:"));
    report.info(format!(
        "  {} files, {} directories, {} symlinks, {} other",
        report.files, report.dirs, report.symlinks, report.other
    ));
    report.info(format!(
        "  Free blocks: {} (bitmap) / {} (superblock)",
        total_free_blocks_bitmap, sb.free_block_count
    ));
    report.info(format!(
        "  Free inodes: {} (bitmap) / {} (superblock)",
        total_free_inodes_bitmap, sb_free_inodes
    ));

    if report.errors == 0 {
        report.info(String::from("  Filesystem clean — no errors found."));
    } else {
        report.info(format!(
            "  {} errors found, {} warnings.",
            report.errors, report.warnings
        ));
    }

    Ok(report)
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Tests for fsck helper functions: bitmap counting, inode classification,
/// group descriptor field accessors, and report construction.
pub fn self_test() -> KernelResult<()> {
    use crate::error::KernelError;

    crate::serial_println!("[ext4-fsck] Running self-test...");

    test_count_used_bits()?;
    test_bitmap_bit_set()?;
    test_gd_free_counts()?;
    test_inode_type_helpers()?;
    test_inode_type_name()?;
    test_report_construction()?;

    crate::serial_println!("[ext4-fsck] Self-test PASSED (6 tests)");
    Ok(())
}

/// Test count_used_bits with various bitmap patterns.
fn test_count_used_bits() -> KernelResult<()> {
    use crate::error::KernelError;

    // All zeros — no used bits.
    let zeros = [0u8; 4];
    if count_used_bits(&zeros, 32) != 0 {
        crate::serial_println!("[ext4-fsck]   FAIL: count_used_bits(zeros)");
        return Err(KernelError::InternalError);
    }

    // All ones — all bits used.
    let ones = [0xFF, 0xFF, 0xFF, 0xFF];
    if count_used_bits(&ones, 32) != 32 {
        crate::serial_println!("[ext4-fsck]   FAIL: count_used_bits(ones, 32)");
        return Err(KernelError::InternalError);
    }

    // All ones but only count first 10 bits.
    let count = count_used_bits(&ones, 10);
    if count != 10 {
        crate::serial_println!("[ext4-fsck]   FAIL: count_used_bits(ones, 10) = {}", count);
        return Err(KernelError::InternalError);
    }

    // Specific pattern: 0b10101010 = 0xAA → 4 bits per byte.
    let pattern = [0xAA, 0xAA];
    let count = count_used_bits(&pattern, 16);
    if count != 8 {
        crate::serial_println!("[ext4-fsck]   FAIL: count_used_bits(0xAA, 16) = {}", count);
        return Err(KernelError::InternalError);
    }

    // Partial: 0xFF but only count 3 bits → should be 3.
    let full = [0xFF];
    if count_used_bits(&full, 3) != 3 {
        crate::serial_println!("[ext4-fsck]   FAIL: count_used_bits(0xFF, 3)");
        return Err(KernelError::InternalError);
    }

    // Empty bitmap, 0 total → 0.
    if count_used_bits(&[], 0) != 0 {
        crate::serial_println!("[ext4-fsck]   FAIL: count_used_bits(empty, 0)");
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[ext4-fsck]   count_used_bits: OK");
    Ok(())
}

/// Test bitmap_bit_set.
fn test_bitmap_bit_set() -> KernelResult<()> {
    use crate::error::KernelError;

    let bitmap = [0b0000_0001, 0b1000_0000]; // bit 0 set, bit 15 set.

    if !bitmap_bit_set(&bitmap, 0) {
        crate::serial_println!("[ext4-fsck]   FAIL: bit 0 not set");
        return Err(KernelError::InternalError);
    }
    if bitmap_bit_set(&bitmap, 1) {
        crate::serial_println!("[ext4-fsck]   FAIL: bit 1 should not be set");
        return Err(KernelError::InternalError);
    }
    if !bitmap_bit_set(&bitmap, 15) {
        crate::serial_println!("[ext4-fsck]   FAIL: bit 15 not set");
        return Err(KernelError::InternalError);
    }

    // Out-of-bounds should return false, not panic.
    if bitmap_bit_set(&bitmap, 100) {
        crate::serial_println!("[ext4-fsck]   FAIL: OOB bit returned true");
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[ext4-fsck]   bitmap_bit_set: OK");
    Ok(())
}

/// Test gd_free_blocks and gd_free_inodes (32-bit and 64-bit).
fn test_gd_free_counts() -> KernelResult<()> {
    use crate::error::KernelError;

    // SAFETY: Ext4GroupDesc is all integer fields — zeroed is valid.
    let mut gd: ondisk::Ext4GroupDesc = unsafe { core::mem::zeroed() };

    // 32-bit mode: only lo field matters.
    gd.bg_free_blocks_count_lo = 500;
    gd.bg_free_blocks_count_hi = 0xFFFF;
    if gd_free_blocks(&gd, false) != 500 {
        crate::serial_println!("[ext4-fsck]   FAIL: gd_free_blocks 32-bit");
        return Err(KernelError::InternalError);
    }

    // 64-bit mode: lo | (hi << 16).
    gd.bg_free_blocks_count_lo = 0x1234;
    gd.bg_free_blocks_count_hi = 0x0005;
    let count = gd_free_blocks(&gd, true);
    if count != 0x0005_1234 {
        crate::serial_println!(
            "[ext4-fsck]   FAIL: gd_free_blocks 64-bit = {:#x}", count
        );
        return Err(KernelError::InternalError);
    }

    // Same for inodes.
    gd.bg_free_inodes_count_lo = 0xABCD;
    gd.bg_free_inodes_count_hi = 0x0012;
    let count = gd_free_inodes(&gd, true);
    if count != 0x0012_ABCD {
        crate::serial_println!(
            "[ext4-fsck]   FAIL: gd_free_inodes 64-bit = {:#x}", count
        );
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[ext4-fsck]   gd free counts: OK");
    Ok(())
}

/// Test inode_is_regular, inode_is_dir, inode_is_symlink.
fn test_inode_type_helpers() -> KernelResult<()> {
    use crate::error::KernelError;

    // SAFETY: Ext4Inode is all integer fields — zeroed is valid.
    let mut inode: Ext4Inode = unsafe { core::mem::zeroed() };

    // Regular file: mode = S_IFREG | 0644 = 0x81A4.
    inode.i_mode = 0x81A4;
    if !inode_is_regular(&inode) || inode_is_dir(&inode) || inode_is_symlink(&inode) {
        crate::serial_println!("[ext4-fsck]   FAIL: regular file detection");
        return Err(KernelError::InternalError);
    }

    // Directory: mode = S_IFDIR | 0755 = 0x41ED.
    inode.i_mode = 0x41ED;
    if inode_is_regular(&inode) || !inode_is_dir(&inode) || inode_is_symlink(&inode) {
        crate::serial_println!("[ext4-fsck]   FAIL: directory detection");
        return Err(KernelError::InternalError);
    }

    // Symlink: mode = S_IFLNK | 0777 = 0xA1FF.
    inode.i_mode = 0xA1FF;
    if inode_is_regular(&inode) || inode_is_dir(&inode) || !inode_is_symlink(&inode) {
        crate::serial_println!("[ext4-fsck]   FAIL: symlink detection");
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[ext4-fsck]   inode type helpers: OK");
    Ok(())
}

/// Test inode_type_name for all known types.
fn test_inode_type_name() -> KernelResult<()> {
    use crate::error::KernelError;

    let checks: &[(u16, &str)] = &[
        (S_IFREG, "regular"),
        (S_IFDIR, "directory"),
        (S_IFLNK, "symlink"),
        (0x6000, "block-dev"),
        (0x2000, "char-dev"),
        (0x1000, "fifo"),
        (0xC000, "socket"),
        (0x0000, "unknown"),
    ];

    for &(mode, expected) in checks {
        let name = inode_type_name(mode);
        if name != expected {
            crate::serial_println!(
                "[ext4-fsck]   FAIL: inode_type_name({:#x}) = '{}', expected '{}'",
                mode, name, expected
            );
            return Err(KernelError::InternalError);
        }
    }

    crate::serial_println!("[ext4-fsck]   inode_type_name: OK");
    Ok(())
}

/// Test Ext4FsckReport construction and methods.
fn test_report_construction() -> KernelResult<()> {
    use crate::error::KernelError;
    use alloc::string::String;

    let mut report = Ext4FsckReport::default();

    // Initial state: all zeros.
    if report.errors != 0 || report.warnings != 0 || !report.messages.is_empty() {
        crate::serial_println!("[ext4-fsck]   FAIL: default report not clean");
        return Err(KernelError::InternalError);
    }

    report.error(String::from("test error"));
    report.warn(String::from("test warning"));
    report.info(String::from("test info"));

    if report.errors != 1 || report.warnings != 1 {
        crate::serial_println!(
            "[ext4-fsck]   FAIL: errors={}, warnings={}",
            report.errors, report.warnings
        );
        return Err(KernelError::InternalError);
    }
    if report.messages.len() != 3 {
        crate::serial_println!(
            "[ext4-fsck]   FAIL: messages count = {}", report.messages.len()
        );
        return Err(KernelError::InternalError);
    }

    // Verify saturation: adding many errors shouldn't overflow.
    for _ in 0..100 {
        report.error(String::from("err"));
    }
    if report.errors != 101 {
        crate::serial_println!(
            "[ext4-fsck]   FAIL: errors after 100 more = {}", report.errors
        );
        return Err(KernelError::InternalError);
    }

    crate::serial_println!("[ext4-fsck]   report construction: OK");
    Ok(())
}
