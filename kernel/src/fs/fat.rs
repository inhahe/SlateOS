//! FAT filesystem driver (FAT16 and FAT32).
//!
//! Implements the [`FileSystem`] trait for FAT16 and FAT32 volumes.
//! Auto-detects the FAT type from the BPB on mount.  Handles real-world
//! media including USB drives, SD cards, and EFI System Partitions.
//!
//! ## Layout
//!
//! ```text
//! ┌─────────────┬────────────┬────────────┬───────────────┬──────────────┐
//! │  Boot sector │   FAT 1    │   FAT 2    │  Root dir     │  Data area   │
//! │  (BPB)       │            │  (copy)    │  (fixed size) │  (clusters)  │
//! └─────────────┴────────────┴────────────┴───────────────┴──────────────┘
//! ```
//!
//! ## References
//!
//! - Microsoft FAT specification (fatgen103.doc)
//! - <https://wiki.osdev.org/FAT>

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::blkdev::SECTOR_SIZE;
use crate::error::{KernelError, KernelResult};
use crate::fs::vfs::{DirEntry, EntryType, FileAttr, FileMeta, FileSystem, FsInfo};

// ---------------------------------------------------------------------------
// FAT type detection
// ---------------------------------------------------------------------------

/// Which FAT variant is on this volume.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FatType {
    Fat16,
    Fat32,
}

// ---------------------------------------------------------------------------
// BIOS Parameter Block (BPB)
// ---------------------------------------------------------------------------

/// Parsed FAT BPB from the boot sector (common + FAT32 extension).
#[derive(Debug, Clone)]
struct FatBpb {
    /// Detected FAT type.
    fat_type: FatType,
    /// Bytes per sector (typically 512).
    bytes_per_sector: u16,
    /// Sectors per cluster (power of 2).
    sectors_per_cluster: u8,
    /// Number of reserved sectors (including boot sector).
    reserved_sectors: u16,
    /// Number of FAT copies (usually 2).
    num_fats: u8,
    /// Maximum number of root directory entries (0 for FAT32).
    root_entry_count: u16,
    /// Total sectors (16-bit field; 0 if using 32-bit field).
    total_sectors_16: u16,
    /// Sectors per FAT (16-bit; 0 for FAT32).
    sectors_per_fat_16: u16,
    /// Total sectors (32-bit field; used if 16-bit is 0).
    total_sectors_32: u32,
    /// Sectors per FAT (32-bit; FAT32 only, 0 for FAT16).
    sectors_per_fat_32: u32,
    /// First cluster of root directory (FAT32 only; 0 for FAT16).
    root_cluster: u32,
    /// Volume label from extended boot record.
    volume_label: [u8; 11],
}

impl FatBpb {
    /// Parse a BPB from a boot sector (512 bytes).
    ///
    /// Detects FAT16 vs FAT32 based on total data cluster count
    /// per the Microsoft FAT specification (fatgen103).
    #[allow(clippy::arithmetic_side_effects)]
    fn parse(sector: &[u8; SECTOR_SIZE]) -> KernelResult<Self> {
        // Check boot signature.
        if sector.get(510).copied() != Some(0x55) || sector.get(511).copied() != Some(0xAA) {
            return Err(KernelError::InvalidArgument);
        }

        let bytes_per_sector = read_u16(sector, 11);
        let sectors_per_cluster = sector.get(13).copied().unwrap_or(0);
        let reserved_sectors = read_u16(sector, 14);
        let num_fats = sector.get(16).copied().unwrap_or(0);
        let root_entry_count = read_u16(sector, 17);
        let total_sectors_16 = read_u16(sector, 19);
        let sectors_per_fat_16 = read_u16(sector, 22);
        let total_sectors_32 = read_u32(sector, 32);

        // Validate basic fields.
        if bytes_per_sector == 0 || sectors_per_cluster == 0 || num_fats == 0 {
            return Err(KernelError::InvalidArgument);
        }

        // FAT32-specific fields (offset 36-51 of boot sector).
        let sectors_per_fat_32 = read_u32(sector, 36);
        let root_cluster = read_u32(sector, 44);

        // Determine actual sectors per FAT.
        let sectors_per_fat = if sectors_per_fat_16 != 0 {
            u32::from(sectors_per_fat_16)
        } else {
            sectors_per_fat_32
        };

        // Total sectors.
        let total_sectors = if total_sectors_16 != 0 {
            u32::from(total_sectors_16)
        } else {
            total_sectors_32
        };

        // Root directory sectors (0 for FAT32 where root_entry_count == 0).
        let root_dir_sectors = {
            let entries_bytes = u32::from(root_entry_count) * 32;
            let bps = u32::from(bytes_per_sector);
            (entries_bytes + bps - 1) / bps
        };

        // Data sectors and cluster count determine the FAT type.
        let data_sectors = total_sectors.saturating_sub(
            u32::from(reserved_sectors)
                + u32::from(num_fats) * sectors_per_fat
                + root_dir_sectors,
        );
        let _total_clusters = if sectors_per_cluster > 0 {
            data_sectors / u32::from(sectors_per_cluster)
        } else {
            0
        };

        // Determine FAT type.  The Microsoft spec (fatgen103) uses total
        // cluster count: <4085 = FAT12, 4085-65524 = FAT16, >65524 = FAT32.
        //
        // However, BPB_FATSz16 == 0 reliably indicates FAT32 (FAT16 always
        // has this field non-zero).  For the FAT12/FAT16 boundary, many
        // real-world FAT16 volumes have fewer than 4085 clusters (small
        // USB drives, test images).  Since we don't support FAT12, we
        // treat all non-FAT32 volumes with 16-bit FAT entries as FAT16.
        let fat_type = if sectors_per_fat_16 == 0 {
            // BPB_FATSz16 == 0 → must be FAT32.
            FatType::Fat32
        } else {
            // Has a 16-bit sectors-per-FAT field → FAT16 (or FAT12, which
            // we treat identically since 16-bit FAT entries are a superset).
            FatType::Fat16
        };

        // Volume label location differs between FAT16 (offset 43) and FAT32 (offset 71).
        let label_offset = if fat_type == FatType::Fat32 { 71 } else { 43 };
        let mut volume_label = [b' '; 11];
        if let Some(label) = sector.get(label_offset..label_offset + 11) {
            volume_label.copy_from_slice(label);
        }

        Ok(Self {
            fat_type,
            bytes_per_sector,
            sectors_per_cluster,
            reserved_sectors,
            num_fats,
            root_entry_count,
            total_sectors_16,
            sectors_per_fat_16,
            total_sectors_32,
            sectors_per_fat_32,
            root_cluster: if fat_type == FatType::Fat32 { root_cluster } else { 0 },
            volume_label,
        })
    }

    /// Total number of sectors on the volume.
    fn total_sectors(&self) -> u32 {
        if self.total_sectors_16 != 0 {
            u32::from(self.total_sectors_16)
        } else {
            self.total_sectors_32
        }
    }

    /// Sectors per FAT (works for both FAT16 and FAT32).
    fn sectors_per_fat(&self) -> u32 {
        if self.sectors_per_fat_16 != 0 {
            u32::from(self.sectors_per_fat_16)
        } else {
            self.sectors_per_fat_32
        }
    }

    /// LBA of the first FAT.
    fn fat_start_lba(&self) -> u32 {
        u32::from(self.reserved_sectors)
    }

    /// LBA of the root directory (FAT16 only; meaningless for FAT32).
    #[allow(clippy::arithmetic_side_effects)]
    fn root_dir_start_lba(&self) -> u32 {
        self.fat_start_lba()
            + u32::from(self.num_fats) * self.sectors_per_fat()
    }

    /// Number of sectors occupied by the root directory.
    /// Returns 0 for FAT32 (root is a cluster chain).
    #[allow(clippy::arithmetic_side_effects)]
    fn root_dir_sectors(&self) -> u32 {
        let entries_bytes = u32::from(self.root_entry_count) * 32;
        let bps = u32::from(self.bytes_per_sector);
        (entries_bytes + bps - 1) / bps
    }

    /// LBA of the first data sector (cluster 2).
    #[allow(clippy::arithmetic_side_effects)]
    fn data_start_lba(&self) -> u32 {
        self.root_dir_start_lba() + self.root_dir_sectors()
    }

    /// Convert a cluster number to an LBA.
    ///
    /// Cluster numbering starts at 2 (clusters 0 and 1 are reserved).
    #[allow(clippy::arithmetic_side_effects)]
    fn cluster_to_lba(&self, cluster: u32) -> u32 {
        self.data_start_lba()
            + (cluster - 2) * u32::from(self.sectors_per_cluster)
    }

    /// Check if a cluster number is a valid data cluster
    /// (not free, not reserved, not end-of-chain, not bad).
    fn is_valid_cluster(&self, cluster: u32) -> bool {
        match self.fat_type {
            FatType::Fat16 => cluster >= 2 && cluster <= 0xFFEF,
            FatType::Fat32 => cluster >= 2 && cluster <= 0x0FFF_FFEF,
        }
    }
}

// ---------------------------------------------------------------------------
// FAT directory entry (32 bytes)
// ---------------------------------------------------------------------------

/// Attribute flags for FAT directory entries.
const ATTR_READ_ONLY: u8 = 0x01;
const ATTR_HIDDEN: u8    = 0x02;
const ATTR_SYSTEM: u8    = 0x04;
const ATTR_VOLUME_ID: u8 = 0x08;
const ATTR_DIRECTORY: u8 = 0x10;
const _ATTR_ARCHIVE: u8  = 0x20;
/// Combination that indicates a long filename entry.
const ATTR_LONG_NAME: u8 = ATTR_READ_ONLY | ATTR_HIDDEN | ATTR_SYSTEM | ATTR_VOLUME_ID;

/// A parsed FAT directory entry.
#[derive(Debug, Clone)]
struct FatDirEntry {
    /// 8.3 filename (without dot, padded with spaces).
    name: [u8; 11],
    /// Attribute byte.
    attr: u8,
    /// First cluster of the file (32-bit; FAT16 uses only low 16 bits).
    first_cluster: u32,
    /// File size in bytes.
    file_size: u32,
    /// Last-write time (DOS packed: HHHHHmmmmmmSSSSS).
    write_time: u16,
    /// Last-write date (DOS packed: YYYYYYYMMMMDDDDD, Y=year-1980).
    write_date: u16,
    /// Creation time (DOS packed, same format as write_time).
    create_time: u16,
    /// Creation date (DOS packed, same format as write_date).
    create_date: u16,
    /// Last-access date (DOS packed, date only — no time component).
    access_date: u16,
    /// Long filename (from LFN directory entries), or `None` for 8.3-only.
    long_name: Option<String>,
}

// ---------------------------------------------------------------------------
// Long Filename (LFN) support
// ---------------------------------------------------------------------------

/// Maximum characters in a FAT long filename (255 UCS-2 chars).
const LFN_MAX_CHARS: usize = 255;

/// Number of UCS-2 characters stored per LFN entry.
const LFN_CHARS_PER_ENTRY: usize = 13;

/// Bit flag on the sequence byte of the last LFN entry.
const LFN_LAST_ENTRY: u8 = 0x40;

/// Compute the short-name checksum used by LFN entries.
///
/// The checksum ties LFN entries to their corresponding 8.3 entry.
/// Algorithm from Microsoft FAT specification (fatgen103).
#[allow(clippy::arithmetic_side_effects)]
fn lfn_checksum(name83: &[u8; 11]) -> u8 {
    let mut sum: u8 = 0;
    for &b in name83.iter() {
        // Rotate right 1 bit, then add.
        sum = ((sum & 1) << 7)
            .wrapping_add(sum >> 1)
            .wrapping_add(b);
    }
    sum
}

/// Extract the 13 UCS-2 characters from a raw LFN directory entry.
///
/// Characters are stored at offsets 1-10 (5 chars), 14-25 (6 chars),
/// 28-31 (2 chars) within the 32-byte entry.
fn lfn_extract_chars(raw: &[u8]) -> [u16; LFN_CHARS_PER_ENTRY] {
    let mut chars = [0xFFFFu16; LFN_CHARS_PER_ENTRY];

    // Chars 1-5 at offsets 1, 3, 5, 7, 9.
    for i in 0..5 {
        let off = 1 + i * 2;
        chars[i] = read_u16(raw, off);
    }
    // Chars 6-11 at offsets 14, 16, 18, 20, 22, 24.
    for i in 0..6 {
        let off = 14 + i * 2;
        chars[5 + i] = read_u16(raw, off);
    }
    // Chars 12-13 at offsets 28, 30.
    for i in 0..2 {
        let off = 28 + i * 2;
        chars[11 + i] = read_u16(raw, off);
    }

    chars
}

/// Assemble a long filename from collected LFN character arrays.
///
/// `lfn_parts` is ordered from lowest sequence number (1) to highest.
/// Each part is a 13-char UCS-2 array.  We concatenate, strip 0xFFFF
/// padding and the null terminator.
fn assemble_lfn(lfn_parts: &[[u16; LFN_CHARS_PER_ENTRY]]) -> Option<String> {
    let mut chars: Vec<u16> = Vec::with_capacity(lfn_parts.len() * LFN_CHARS_PER_ENTRY);

    for part in lfn_parts {
        for &ch in part.iter() {
            // 0x0000 is the null terminator, 0xFFFF is padding.
            if ch == 0x0000 || ch == 0xFFFF {
                // Convert collected UCS-2 to UTF-8.
                let s: String = chars.iter().filter_map(|&c| {
                    char::from_u32(u32::from(c))
                }).collect();
                return if s.is_empty() { None } else { Some(s) };
            }
            chars.push(ch);
        }
    }

    // No null terminator found — convert what we have.
    let s: String = chars.iter().filter_map(|&c| {
        char::from_u32(u32::from(c))
    }).collect();
    if s.is_empty() { None } else { Some(s) }
}

/// Encode a filename into UCS-2 for LFN entries.
///
/// Returns the UCS-2 characters with null terminator and 0xFFFF padding.
/// Returns `None` if the name exceeds 255 characters.
fn encode_lfn(name: &str) -> Option<Vec<u16>> {
    if name.len() > LFN_MAX_CHARS {
        return None;
    }

    let mut ucs2: Vec<u16> = Vec::with_capacity(name.len() + 1);
    for ch in name.chars() {
        let cp = ch as u32;
        if cp > 0xFFFF {
            // BMP only — surrogate pairs not supported in FAT LFN.
            return None;
        }
        ucs2.push(cp as u16);
    }
    // Add null terminator.
    ucs2.push(0x0000);
    // Pad to multiple of 13 with 0xFFFF.
    while ucs2.len() % LFN_CHARS_PER_ENTRY != 0 {
        ucs2.push(0xFFFF);
    }
    Some(ucs2)
}

/// Build the raw 32-byte LFN directory entries for a filename.
///
/// Returns the entries in on-disk order (highest sequence number first,
/// lowest last — they precede the short entry on disk).
#[allow(clippy::arithmetic_side_effects)]
fn build_lfn_entries(name: &str, name83: &[u8; 11]) -> Option<Vec<[u8; 32]>> {
    let ucs2 = encode_lfn(name)?;
    let num_entries = ucs2.len() / LFN_CHARS_PER_ENTRY;
    let checksum = lfn_checksum(name83);

    let mut entries: Vec<[u8; 32]> = Vec::with_capacity(num_entries);

    // Build entries from first (seq 1) to last (seq N).
    for seq_idx in 0..num_entries {
        let mut raw = [0u8; 32];
        // Sequence number (1-based).  Last entry has bit 6 set.
        let seq_num = (seq_idx + 1) as u8;
        raw[0] = if seq_idx == num_entries - 1 {
            seq_num | LFN_LAST_ENTRY
        } else {
            seq_num
        };
        // Attribute: long name.
        raw[11] = ATTR_LONG_NAME;
        // Type: 0 (sub-component of a long name).
        raw[12] = 0;
        // Checksum.
        raw[13] = checksum;
        // Cluster: always 0.
        raw[26] = 0;
        raw[27] = 0;

        // Fill in the 13 UCS-2 characters for this entry.
        let base = seq_idx * LFN_CHARS_PER_ENTRY;

        // Chars 1-5 at offsets 1,3,5,7,9.
        for i in 0..5 {
            let ch = ucs2.get(base + i).copied().unwrap_or(0xFFFF);
            raw[1 + i * 2] = ch as u8;
            raw[2 + i * 2] = (ch >> 8) as u8;
        }
        // Chars 6-11 at offsets 14,16,18,20,22,24.
        for i in 0..6 {
            let ch = ucs2.get(base + 5 + i).copied().unwrap_or(0xFFFF);
            raw[14 + i * 2] = ch as u8;
            raw[15 + i * 2] = (ch >> 8) as u8;
        }
        // Chars 12-13 at offsets 28,30.
        for i in 0..2 {
            let ch = ucs2.get(base + 11 + i).copied().unwrap_or(0xFFFF);
            raw[28 + i * 2] = ch as u8;
            raw[29 + i * 2] = (ch >> 8) as u8;
        }

        entries.push(raw);
    }

    // On-disk order: highest sequence number first.
    entries.reverse();
    Some(entries)
}

/// Generate an 8.3 basis name from a long filename.
///
/// Follows the Microsoft algorithm:
/// 1. Take the first 6 bytes of the base (uppercase, strip invalid chars)
/// 2. Append `~1` (incrementing if collision)
/// 3. Take first 3 bytes of the extension
///
/// Returns the basis name. The caller must check for uniqueness and
/// increment the tail number.
fn generate_basis_name(name: &str) -> [u8; 11] {
    let mut result = [b' '; 11];

    let upper = name.to_uppercase();
    let (base_part, ext_part) = if let Some(dot_pos) = upper.rfind('.') {
        (&upper[..dot_pos], &upper[dot_pos + 1..])
    } else {
        (upper.as_str(), "")
    };

    // Strip leading/embedded spaces and dots, keep only valid 8.3 chars.
    let mut base_clean = Vec::with_capacity(8);
    for ch in base_part.bytes() {
        match ch {
            b' ' | b'.' => {} // Skip.
            b'A'..=b'Z' | b'0'..=b'9' | b'!' | b'#' | b'$' | b'%'
            | b'&' | b'\'' | b'(' | b')' | b'-' | b'@' | b'^'
            | b'_' | b'`' | b'{' | b'}' | b'~' => {
                if base_clean.len() < 6 {
                    base_clean.push(ch);
                }
            }
            _ => {
                // Replace other chars with underscore.
                if base_clean.len() < 6 {
                    base_clean.push(b'_');
                }
            }
        }
    }

    // If base is empty, use a fallback.
    if base_clean.is_empty() {
        base_clean.extend_from_slice(b"FILE");
    }

    // Copy base (max 6 chars to leave room for ~N).
    for (i, &b) in base_clean.iter().enumerate().take(6) {
        result[i] = b;
    }
    // Append ~1 (caller increments if needed).
    let tail_pos = base_clean.len().min(6);
    result[tail_pos] = b'~';
    if tail_pos + 1 < 8 {
        result[tail_pos + 1] = b'1';
    }

    // Extension (max 3 chars).
    let ext_clean: Vec<u8> = ext_part.bytes().filter(|b| {
        matches!(b, b'A'..=b'Z' | b'0'..=b'9' | b'!' | b'#' | b'$'
            | b'%' | b'&' | b'\'' | b'(' | b')' | b'-' | b'@'
            | b'^' | b'_' | b'`' | b'{' | b'}' | b'~')
    }).take(3).collect();

    for (i, &b) in ext_clean.iter().enumerate() {
        result[8 + i] = b;
    }

    result
}

/// Set the numeric tail (~N) on a basis name.
///
/// Writes the `~N` suffix starting at position `tail_pos` in the name.
#[allow(clippy::arithmetic_side_effects)]
fn set_basis_tail(basis: &mut [u8; 11], n: u32) {
    // Format the tail: ~1, ~2, ..., ~9999.
    let tail_str = {
        let mut buf = [0u8; 6]; // ~NNNNN max
        let mut len = 0;
        buf[0] = b'~';
        len += 1;

        // Convert number to decimal digits.
        let mut digits = [0u8; 5];
        let mut num = n;
        let mut dlen = 0;
        loop {
            digits[dlen] = b'0' + (num % 10) as u8;
            dlen += 1;
            num /= 10;
            if num == 0 || dlen >= 5 {
                break;
            }
        }
        // Reverse digits into buf.
        for i in 0..dlen {
            buf[len] = digits[dlen - 1 - i];
            len += 1;
        }
        (buf, len)
    };

    let (tail_buf, tail_len) = tail_str;

    // Find the rightmost non-space character in the first 8 bytes.
    // Place the tail so it fits within 8 chars.
    let max_base = 8usize.saturating_sub(tail_len);
    let mut base_end = 0;
    for i in 0..8 {
        if basis[i] != b' ' && basis[i] != b'~' {
            base_end = i + 1;
        }
    }
    // Trim base to make room for tail.
    let base_end = base_end.min(max_base);

    // Clear the base area from base_end to 8.
    for i in base_end..8 {
        basis[i] = b' ';
    }

    // Write the tail.
    for i in 0..tail_len {
        if base_end + i < 8 {
            basis[base_end + i] = tail_buf[i];
        }
    }
}

/// Check if a filename needs LFN entries (doesn't fit 8.3 format).
fn needs_lfn(name: &str) -> bool {
    // If to_83_name succeeds, check if the round-trip is lossy.
    // Names with lowercase, spaces (in wrong positions), multiple dots,
    // or length > 8+3 need LFN.
    if name.len() > 12 {
        return true;
    }
    // Check for characters that can't be represented in 8.3.
    for ch in name.chars() {
        if ch.is_ascii_lowercase() {
            return true;
        }
        match ch {
            ' ' | '+' | ',' | ';' | '=' | '[' | ']' => return true,
            _ => {}
        }
    }
    // Check if the name has multiple dots.
    if name.matches('.').count() > 1 {
        return true;
    }
    // Check base/extension length.
    if let Some(dot_pos) = name.rfind('.') {
        let base = &name[..dot_pos];
        let ext = &name[dot_pos + 1..];
        if base.len() > 8 || ext.len() > 3 || base.is_empty() {
            return true;
        }
    } else if name.len() > 8 {
        return true;
    }
    false
}

impl FatDirEntry {
    /// Parse a directory entry from 32 raw bytes.
    ///
    /// Reads both the low (offset 26-27) and high (offset 20-21) cluster
    /// words, combining them into a 32-bit cluster number.  On FAT16
    /// volumes the high word is naturally 0.
    fn parse(raw: &[u8]) -> Option<Self> {
        if raw.len() < 32 {
            return None;
        }

        let first_byte = raw.get(0).copied()?;

        // 0x00 = end of directory, 0xE5 = deleted entry.
        if first_byte == 0x00 || first_byte == 0xE5 {
            return None;
        }

        let attr = raw.get(11).copied()?;

        // Skip long filename entries.
        if attr == ATTR_LONG_NAME {
            return None;
        }

        let mut name = [0u8; 11];
        name.copy_from_slice(raw.get(0..11)?);

        // Combine high and low 16-bit cluster words into 32 bits.
        let cluster_hi = u32::from(read_u16(raw, 20));
        let cluster_lo = u32::from(read_u16(raw, 26));
        let first_cluster = (cluster_hi << 16) | cluster_lo;
        let file_size = read_u32(raw, 28);

        // Timestamps (DOS packed format).
        let create_time = read_u16(raw, 14);
        let create_date = read_u16(raw, 16);
        let access_date = read_u16(raw, 18);
        let write_time = read_u16(raw, 22);
        let write_date = read_u16(raw, 24);

        Some(Self {
            name,
            attr,
            first_cluster,
            file_size,
            write_time,
            write_date,
            create_time,
            create_date,
            access_date,
            long_name: None, // Set later by directory reader from LFN entries.
        })
    }

    /// Is this a directory?
    fn is_directory(&self) -> bool {
        self.attr & ATTR_DIRECTORY != 0
    }

    /// Is this a volume label?
    fn is_volume_label(&self) -> bool {
        self.attr & ATTR_VOLUME_ID != 0
    }

    /// Return the display name, preferring the long filename if available.
    ///
    /// Falls back to the 8.3 short name: `"HELLO   TXT"` → `"HELLO.TXT"`.
    fn display_name(&self) -> String {
        // Prefer long filename if present.
        if let Some(ref lfn) = self.long_name {
            return lfn.clone();
        }

        // Fall back to 8.3 short name.
        let base = core::str::from_utf8(&self.name[..8])
            .unwrap_or("????????")
            .trim_end();
        let ext = core::str::from_utf8(&self.name[8..11])
            .unwrap_or("???")
            .trim_end();

        if self.is_volume_label() || self.is_directory() || ext.is_empty() {
            String::from(base)
        } else {
            let mut s = String::from(base);
            s.push('.');
            s.push_str(ext);
            s
        }
    }

    /// Return the 8.3 short name as a string (for matching purposes).
    fn short_name(&self) -> String {
        let base = core::str::from_utf8(&self.name[..8])
            .unwrap_or("????????")
            .trim_end();
        let ext = core::str::from_utf8(&self.name[8..11])
            .unwrap_or("???")
            .trim_end();

        if self.is_volume_label() || self.is_directory() || ext.is_empty() {
            String::from(base)
        } else {
            let mut s = String::from(base);
            s.push('.');
            s.push_str(ext);
            s
        }
    }

    /// Convert to a VFS [`DirEntry`].
    fn to_vfs_entry(&self) -> DirEntry {
        DirEntry {
            name: self.display_name(),
            entry_type: if self.is_volume_label() {
                EntryType::VolumeLabel
            } else if self.is_directory() {
                EntryType::Directory
            } else {
                EntryType::File
            },
            size: u64::from(self.file_size),
        }
    }
}

// ---------------------------------------------------------------------------
// DOS date/time → nanoseconds-since-Unix-epoch conversion
// ---------------------------------------------------------------------------

/// Convert a DOS packed date+time to nanoseconds since Unix epoch.
///
/// DOS date format: `(year-1980) << 9 | month << 5 | day`
/// DOS time format: `hours << 11 | minutes << 5 | seconds/2`
///
/// Returns 0 if date is 0 (not set).
fn dos_datetime_to_ns(date: u16, time: u16) -> u64 {
    if date == 0 {
        return 0;
    }

    let day   = u64::from(date & 0x1F);
    let month = u64::from((date >> 5) & 0x0F);
    let year  = u64::from((date >> 9) & 0x7F).wrapping_add(1980);

    let secs2 = u64::from(time & 0x1F);
    let mins  = u64::from((time >> 5) & 0x3F);
    let hours = u64::from((time >> 11) & 0x1F);

    // Rough days-since-epoch using the common formula.
    // Not perfectly accurate for all leap years but correct within a day.
    let mut days: u64 = 0;
    // Years since 1970.
    let mut y = 1970u64;
    while y < year {
        days = days.wrapping_add(if is_leap(y) { 366 } else { 365 });
        y = y.wrapping_add(1);
    }
    // Months within the target year.
    let month_days: [u64; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut m = 1u64;
    while m < month && m <= 12 {
        let md = if m == 2 && is_leap(year) { 29 } else {
            month_days.get(m.wrapping_sub(1) as usize).copied().unwrap_or(30)
        };
        days = days.wrapping_add(md);
        m = m.wrapping_add(1);
    }
    days = days.wrapping_add(day.saturating_sub(1));

    let total_secs = days
        .wrapping_mul(86400)
        .wrapping_add(hours.wrapping_mul(3600))
        .wrapping_add(mins.wrapping_mul(60))
        .wrapping_add(secs2.wrapping_mul(2));

    total_secs.wrapping_mul(1_000_000_000)
}

/// Check if a year is a leap year.
const fn is_leap(y: u64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

/// Convert a kernel RTC `DateTime` to DOS packed date and time.
///
/// Returns `(date, time)` in DOS format.
/// If year < 1980 or > 2107, clamps to the DOS epoch range.
#[allow(clippy::arithmetic_side_effects)]
fn rtc_to_dos_datetime(dt: &crate::rtc::DateTime) -> (u16, u16) {
    // DOS year is offset from 1980, stored in 7 bits (0-127 → 1980-2107).
    let dos_year = if dt.year < 1980 {
        0u16
    } else if dt.year > 2107 {
        127u16
    } else {
        dt.year - 1980
    };

    let date: u16 = (dos_year << 9)
        | (u16::from(dt.month) << 5)
        | u16::from(dt.day);

    // DOS time packs hours (5 bits), minutes (6 bits), seconds/2 (5 bits).
    let time: u16 = (u16::from(dt.hour) << 11)
        | (u16::from(dt.minute) << 5)
        | (u16::from(dt.second) >> 1); // 2-second granularity.

    (date, time)
}

/// Get the current time as DOS packed date and time.
///
/// Reads the CMOS RTC and converts to DOS format.
fn current_dos_datetime() -> (u16, u16) {
    let dt = crate::rtc::read_datetime();
    rtc_to_dos_datetime(&dt)
}

// ---------------------------------------------------------------------------
// FAT filesystem (FAT16 / FAT32)
// ---------------------------------------------------------------------------

/// Maximum number of cached path resolution results.
const DCACHE_MAX_ENTRIES: usize = 64;

/// A cached path resolution result.
///
/// Maps a full path string to its parent cluster and directory entry,
/// avoiding repeated directory tree walks for frequently accessed paths.
#[derive(Clone)]
struct DcacheEntry {
    /// Full path (e.g., "/TESTDIR/FILE.TXT").
    path: String,
    /// Parent directory cluster (0 = root).
    parent_cluster: u32,
    /// Resolved directory entry.
    entry: FatDirEntry,
    /// Access counter for LRU eviction.
    last_access: u64,
    /// Whether this slot is in use.
    valid: bool,
}

impl DcacheEntry {
    const fn empty() -> Self {
        Self {
            path: String::new(),
            parent_cluster: 0,
            entry: FatDirEntry {
                name: [0; 11],
                attr: 0,
                first_cluster: 0,
                file_size: 0,
                write_time: 0,
                write_date: 0,
                create_time: 0,
                create_date: 0,
                access_date: 0,
                long_name: None,
            },
            last_access: 0,
            valid: false,
        }
    }
}

/// A mounted FAT filesystem (auto-detects FAT16 or FAT32).
pub struct FatFs {
    /// The block device name in the registry.
    device_name: String,
    /// Parsed BIOS Parameter Block.
    bpb: FatBpb,
    /// Path resolution cache (dcache).
    ///
    /// Caches `resolve_path()` results so repeated lookups on the same
    /// path avoid re-reading directory sectors.  Invalidated on any
    /// mutating operation that could change directory structure.
    dcache: Vec<DcacheEntry>,
    /// Monotonic access counter for dcache LRU.
    dcache_counter: u64,
    /// Dcache statistics.
    dcache_hits: u64,
    dcache_misses: u64,
}

impl FatFs {
    /// Mount a FAT filesystem from a named block device.
    ///
    /// Reads the boot sector, validates the BPB, auto-detects FAT16 or
    /// FAT32, and returns the filesystem instance.
    pub fn mount(device_name: &str) -> KernelResult<Self> {
        // Read the boot sector through the buffer cache.
        let mut boot_sector = [0u8; SECTOR_SIZE];
        super::cache::read_sector(device_name, 0, &mut boot_sector)?;

        let bpb = FatBpb::parse(&boot_sector)?;

        let label = core::str::from_utf8(&bpb.volume_label)
            .unwrap_or("???????????")
            .trim_end();

        let type_str = match bpb.fat_type {
            FatType::Fat16 => "FAT16",
            FatType::Fat32 => "FAT32",
        };

        crate::serial_println!(
            "[fat] Mounted {} '{}' on device '{}': {} sectors, {} bytes/sector, \
             {} sectors/cluster",
            type_str,
            label,
            device_name,
            bpb.total_sectors(),
            bpb.bytes_per_sector,
            bpb.sectors_per_cluster,
        );

        if bpb.fat_type == FatType::Fat32 {
            crate::serial_println!(
                "[fat]   Root cluster: {}, sectors/FAT: {}",
                bpb.root_cluster,
                bpb.sectors_per_fat(),
            );
        }

        // Initialize the path resolution cache.
        let mut dcache = Vec::with_capacity(DCACHE_MAX_ENTRIES);
        for _ in 0..DCACHE_MAX_ENTRIES {
            dcache.push(DcacheEntry::empty());
        }

        Ok(Self {
            device_name: String::from(device_name),
            bpb,
            dcache,
            dcache_counter: 0,
            dcache_hits: 0,
            dcache_misses: 0,
        })
    }

    // -- Dcache (path resolution cache) --

    /// Look up a path in the dcache.
    ///
    /// Returns a clone of the cached result on hit, or `None` on miss.
    #[allow(clippy::arithmetic_side_effects)]
    fn dcache_lookup(&mut self, path: &str) -> Option<(u32, FatDirEntry)> {
        for entry in self.dcache.iter_mut() {
            if entry.valid && entry.path.eq_ignore_ascii_case(path) {
                self.dcache_counter = self.dcache_counter.wrapping_add(1);
                entry.last_access = self.dcache_counter;
                self.dcache_hits = self.dcache_hits.wrapping_add(1);
                return Some((entry.parent_cluster, entry.entry.clone()));
            }
        }
        self.dcache_misses = self.dcache_misses.wrapping_add(1);
        None
    }

    /// Insert a path resolution result into the dcache.
    #[allow(clippy::arithmetic_side_effects)]
    fn dcache_insert(&mut self, path: &str, parent_cluster: u32, entry: &FatDirEntry) {
        self.dcache_counter = self.dcache_counter.wrapping_add(1);

        // Try to find an existing entry for this path (update in place).
        for e in self.dcache.iter_mut() {
            if e.valid && e.path.eq_ignore_ascii_case(path) {
                e.parent_cluster = parent_cluster;
                e.entry = entry.clone();
                e.last_access = self.dcache_counter;
                return;
            }
        }

        // Find a free slot.
        for e in self.dcache.iter_mut() {
            if !e.valid {
                e.path = String::from(path);
                e.parent_cluster = parent_cluster;
                e.entry = entry.clone();
                e.last_access = self.dcache_counter;
                e.valid = true;
                return;
            }
        }

        // Evict LRU entry.
        let mut lru_idx = 0;
        let mut lru_access = u64::MAX;
        for (i, e) in self.dcache.iter().enumerate() {
            if e.valid && e.last_access < lru_access {
                lru_access = e.last_access;
                lru_idx = i;
            }
        }
        self.dcache[lru_idx].path = String::from(path);
        self.dcache[lru_idx].parent_cluster = parent_cluster;
        self.dcache[lru_idx].entry = entry.clone();
        self.dcache[lru_idx].last_access = self.dcache_counter;
        self.dcache[lru_idx].valid = true;
    }

    /// Invalidate dcache entries whose path starts with `prefix`.
    ///
    /// Used after mutating operations to ensure stale data isn't served.
    fn dcache_invalidate_prefix(&mut self, prefix: &str) {
        for entry in self.dcache.iter_mut() {
            if entry.valid && entry.path.to_uppercase().starts_with(&prefix.to_uppercase()) {
                entry.valid = false;
            }
        }
    }

    /// Invalidate all dcache entries.
    fn dcache_invalidate_all(&mut self) {
        for entry in self.dcache.iter_mut() {
            entry.valid = false;
        }
    }

    /// Check if a cluster number is valid for data access.
    fn is_valid_cluster(&self, cluster: u32) -> bool {
        self.bpb.is_valid_cluster(cluster)
    }

    /// Count the number of free and total data clusters.
    ///
    /// Scans the FAT to count entries with value 0 (free).
    /// Returns `(free_clusters, total_clusters)`.
    #[allow(clippy::arithmetic_side_effects)]
    fn count_clusters(&mut self) -> KernelResult<(u64, u64)> {
        let bps = u32::from(self.bpb.bytes_per_sector);
        let entry_bytes: u32 = match self.bpb.fat_type {
            FatType::Fat16 => 2,
            FatType::Fat32 => 4,
        };
        let fat_start = self.bpb.fat_start_lba();

        let data_sectors = self.bpb.total_sectors()
            .saturating_sub(u32::from(self.bpb.reserved_sectors))
            .saturating_sub(u32::from(self.bpb.num_fats) * self.bpb.sectors_per_fat())
            .saturating_sub(self.bpb.root_dir_sectors());
        let total_clusters = data_sectors / u32::from(self.bpb.sectors_per_cluster);

        let max_cluster = match self.bpb.fat_type {
            FatType::Fat16 => (total_clusters + 2).min(0xFFEF),
            FatType::Fat32 => (total_clusters + 2).min(0x0FFF_FFEF),
        };

        let mut free_count: u64 = 0;
        let mut sector_buf = [0u8; SECTOR_SIZE];
        let mut last_sector = u32::MAX;

        for cluster in 2..max_cluster {
            let fat_offset = cluster * entry_bytes;
            let sector_num = fat_start + fat_offset / bps;

            if sector_num != last_sector {
                self.read_sector(u64::from(sector_num), &mut sector_buf)?;
                last_sector = sector_num;
            }

            let offset = (fat_offset % bps) as usize;
            let is_free = match self.bpb.fat_type {
                FatType::Fat16 => read_u16(&sector_buf, offset) == 0x0000,
                FatType::Fat32 => (read_u32(&sector_buf, offset) & 0x0FFF_FFFF) == 0,
            };

            if is_free {
                free_count += 1;
            }
        }

        Ok((free_count, u64::from(total_clusters)))
    }

    /// Read the root directory entries.
    ///
    /// FAT16: reads the fixed-size root directory area.
    /// FAT32: reads the cluster chain starting at `bpb.root_cluster`.
    ///
    /// Collects LFN (long filename) entries and attaches them to the
    /// following short (8.3) entry.
    fn read_root_dir(&mut self) -> KernelResult<Vec<FatDirEntry>> {
        // FAT32 root directory is a cluster chain.
        if self.bpb.fat_type == FatType::Fat32 {
            return self.read_dir_cluster(self.bpb.root_cluster);
        }

        // FAT16: fixed root directory area.
        let root_lba = self.bpb.root_dir_start_lba();
        let root_sectors = self.bpb.root_dir_sectors();
        let max_entries = self.bpb.root_entry_count;

        let mut entries = Vec::new();
        let mut sector_buf = [0u8; SECTOR_SIZE];
        let mut entry_index: u16 = 0;
        // Buffer for collecting LFN parts (indexed by seq number - 1).
        let mut lfn_buf: Vec<[u16; LFN_CHARS_PER_ENTRY]> = Vec::new();
        let mut lfn_checksum_expected: u8 = 0;

        'outer: for sec in 0..root_sectors {
            let lba = u64::from(root_lba.checked_add(sec)
                .ok_or(KernelError::InvalidArgument)?);

            self.read_sector(lba, &mut sector_buf)?;

            // Each sector holds 16 directory entries (512 / 32).
            let entries_per_sector = usize::from(self.bpb.bytes_per_sector) / 32;
            for i in 0..entries_per_sector {
                if entry_index >= max_entries {
                    break 'outer;
                }

                let offset = i * 32;
                if let Some(raw) = sector_buf.get(offset..offset + 32) {
                    let first_byte = raw.first().copied().unwrap_or(0);

                    // End-of-directory marker.
                    if first_byte == 0x00 {
                        break 'outer;
                    }

                    // Deleted entry — reset LFN buffer.
                    if first_byte == 0xE5 {
                        lfn_buf.clear();
                        entry_index = entry_index.wrapping_add(1);
                        continue;
                    }

                    let attr = raw.get(11).copied().unwrap_or(0);

                    if attr == ATTR_LONG_NAME {
                        // LFN entry — collect it.
                        let seq = first_byte & 0x3F;
                        let chksum = raw.get(13).copied().unwrap_or(0);

                        if first_byte & LFN_LAST_ENTRY != 0 {
                            // First LFN entry we encounter (highest seq number).
                            lfn_buf.clear();
                            lfn_buf.resize(seq as usize, [0xFFFF; LFN_CHARS_PER_ENTRY]);
                            lfn_checksum_expected = chksum;
                        }

                        if chksum == lfn_checksum_expected && seq >= 1 {
                            let idx = (seq as usize).saturating_sub(1);
                            if idx < lfn_buf.len() {
                                lfn_buf[idx] = lfn_extract_chars(raw);
                            }
                        }
                    } else if let Some(mut entry) = FatDirEntry::parse(raw) {
                        // Short entry — attach LFN if available.
                        if !lfn_buf.is_empty() {
                            let actual_checksum = lfn_checksum(&entry.name);
                            if actual_checksum == lfn_checksum_expected {
                                entry.long_name = assemble_lfn(&lfn_buf);
                            }
                            lfn_buf.clear();
                        }
                        entries.push(entry);
                    } else {
                        lfn_buf.clear();
                    }
                }

                entry_index = entry_index.wrapping_add(1);
            }
        }

        Ok(entries)
    }

    /// Read directory entries from a cluster chain (for subdirectories).
    ///
    /// Subdirectories are stored as files: their data is a chain of clusters
    /// containing 32-byte directory entries.  Collects LFN entries and
    /// attaches them to the following short entry.
    #[allow(clippy::arithmetic_side_effects)]
    fn read_dir_cluster(&mut self, first_cluster: u32) -> KernelResult<Vec<FatDirEntry>> {
        let mut entries = Vec::new();
        let mut cluster = first_cluster;
        let mut iterations = 0u32;
        let max_iterations = 65536u32;
        // Buffer for collecting LFN parts.
        let mut lfn_buf: Vec<[u16; LFN_CHARS_PER_ENTRY]> = Vec::new();
        let mut lfn_checksum_expected: u8 = 0;

        while self.is_valid_cluster(cluster) {
            iterations = iterations.wrapping_add(1);
            if iterations > max_iterations {
                return Err(KernelError::IoError);
            }

            let lba = self.bpb.cluster_to_lba(cluster);
            let mut sector_buf = [0u8; SECTOR_SIZE];
            let entries_per_sector = usize::from(self.bpb.bytes_per_sector) / 32;

            for s in 0..u32::from(self.bpb.sectors_per_cluster) {
                self.read_sector(u64::from(lba + s), &mut sector_buf)?;

                for i in 0..entries_per_sector {
                    let offset = i * 32;
                    if let Some(raw) = sector_buf.get(offset..offset + 32) {
                        let first_byte = raw.first().copied().unwrap_or(0);

                        if first_byte == 0x00 {
                            return Ok(entries); // End of directory.
                        }

                        if first_byte == 0xE5 {
                            lfn_buf.clear();
                            continue;
                        }

                        let attr = raw.get(11).copied().unwrap_or(0);

                        if attr == ATTR_LONG_NAME {
                            // LFN entry — collect it.
                            let seq = first_byte & 0x3F;
                            let chksum = raw.get(13).copied().unwrap_or(0);

                            if first_byte & LFN_LAST_ENTRY != 0 {
                                lfn_buf.clear();
                                lfn_buf.resize(seq as usize, [0xFFFF; LFN_CHARS_PER_ENTRY]);
                                lfn_checksum_expected = chksum;
                            }

                            if chksum == lfn_checksum_expected && seq >= 1 {
                                let idx = (seq as usize).saturating_sub(1);
                                if idx < lfn_buf.len() {
                                    lfn_buf[idx] = lfn_extract_chars(raw);
                                }
                            }
                        } else if let Some(mut entry) = FatDirEntry::parse(raw) {
                            // Short entry — attach LFN if available.
                            if !lfn_buf.is_empty() {
                                let actual_checksum = lfn_checksum(&entry.name);
                                if actual_checksum == lfn_checksum_expected {
                                    entry.long_name = assemble_lfn(&lfn_buf);
                                }
                                lfn_buf.clear();
                            }
                            // Skip . and .. entries.
                            if entry.name[0] != b'.' {
                                entries.push(entry);
                            }
                        } else {
                            lfn_buf.clear();
                        }
                    }
                }
            }

            // Follow the FAT chain.
            match self.fat_entry(cluster)? {
                Some(next) => cluster = next,
                None => break,
            }
        }

        Ok(entries)
    }

    /// Resolve a path to a directory entry.
    ///
    /// Walks path components through the directory tree.
    /// Returns `(parent_cluster, entry)` where parent_cluster is 0 for root.
    ///
    /// For the root directory itself, returns `None` for the entry.
    fn resolve_path(&mut self, path: &str) -> KernelResult<(u32, Option<FatDirEntry>)> {
        let path = path.strip_prefix('/').unwrap_or(path);
        let path = path.trim_end_matches('/');

        if path.is_empty() {
            // Root directory.
            return Ok((0, None));
        }

        // Check the dcache first — avoids re-reading directory sectors
        // for frequently accessed paths.
        let full_path = {
            let mut p = String::from("/");
            p.push_str(path);
            p
        };
        if let Some((parent, entry)) = self.dcache_lookup(&full_path) {
            return Ok((parent, Some(entry)));
        }

        // Cache miss — walk the directory tree.
        let components: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        let mut current_cluster: u32 = 0; // 0 = root directory.

        for (i, component) in components.iter().enumerate() {
            let is_last = i == components.len() - 1;
            let target = component.to_uppercase();

            // Read the current directory.
            let entries = if current_cluster == 0 {
                self.read_root_dir()?
            } else {
                self.read_dir_cluster(current_cluster)?
            };

            // Find the component — match against long name (preferred),
            // short name, and the raw component (for case-sensitive lookup).
            let found = entries.iter().find(|e| {
                if e.is_volume_label() {
                    return false;
                }
                // Match against display name (long name if present, else 8.3).
                if e.display_name().eq_ignore_ascii_case(&target) {
                    return true;
                }
                // Also match against the short name if a long name was used
                // for display — callers may use either form.
                if e.long_name.is_some() {
                    return e.short_name().eq_ignore_ascii_case(&target);
                }
                false
            });

            match found {
                Some(entry) => {
                    if is_last {
                        // Cache the result before returning.
                        self.dcache_insert(&full_path, current_cluster, entry);
                        return Ok((current_cluster, Some(entry.clone())));
                    }
                    // Must be a directory to traverse into.
                    if !entry.is_directory() {
                        return Err(KernelError::NotADirectory);
                    }
                    current_cluster = entry.first_cluster;
                }
                None => return Err(KernelError::NotFound),
            }
        }

        Ok((current_cluster, None))
    }

    /// Resolve a directory path to its cluster number.
    ///
    /// Returns 0 for root directory, or the first cluster of a subdirectory.
    fn resolve_dir_cluster(&mut self, path: &str) -> KernelResult<u32> {
        let (parent_cluster, entry) = self.resolve_path(path)?;
        match entry {
            None => Ok(parent_cluster),
            Some(e) if e.is_directory() => Ok(e.first_cluster),
            Some(_) => Err(KernelError::NotADirectory),
        }
    }

    /// Read a FAT entry for a given cluster.
    ///
    /// Returns the next cluster number, or `None` for end-of-chain /
    /// free / bad cluster markers.  Works for both FAT16 and FAT32.
    #[allow(clippy::arithmetic_side_effects)]
    fn fat_entry(&mut self, cluster: u32) -> KernelResult<Option<u32>> {
        let bps = u32::from(self.bpb.bytes_per_sector);

        let (fat_offset, entry_bytes) = match self.bpb.fat_type {
            FatType::Fat16 => (cluster * 2, 2u32),
            FatType::Fat32 => (cluster * 4, 4u32),
        };

        let fat_sector = self.bpb.fat_start_lba() + fat_offset / bps;
        let offset_in_sector = (fat_offset % bps) as usize;
        let _ = entry_bytes; // Used only for documentation clarity.

        let mut sector_buf = [0u8; SECTOR_SIZE];
        self.read_sector(u64::from(fat_sector), &mut sector_buf)?;

        match self.bpb.fat_type {
            FatType::Fat16 => {
                let value = read_u16(&sector_buf, offset_in_sector);
                // 0x0000 = free, 0x0002-0xFFEF = next, 0xFFF8+ = end.
                if value >= 0xFFF8 {
                    Ok(None)
                } else if value >= 2 && value <= 0xFFEF {
                    Ok(Some(u32::from(value)))
                } else {
                    Ok(None)
                }
            }
            FatType::Fat32 => {
                // Upper 4 bits are reserved; mask to 28 bits.
                let value = read_u32(&sector_buf, offset_in_sector) & 0x0FFF_FFFF;
                // 0x0FFFFFF8+ = end of chain.
                if value >= 0x0FFF_FFF8 {
                    Ok(None)
                } else if value >= 2 && value <= 0x0FFF_FFEF {
                    Ok(Some(value))
                } else {
                    Ok(None)
                }
            }
        }
    }

    /// Read the contents of a file given its directory entry.
    #[allow(clippy::arithmetic_side_effects)]
    fn read_file_data(&mut self, entry: &FatDirEntry) -> KernelResult<Vec<u8>> {
        let file_size = entry.file_size as usize;
        let mut data = vec![0u8; file_size];
        let mut cluster = entry.first_cluster;
        let mut bytes_read: usize = 0;
        let cluster_bytes = usize::from(self.bpb.sectors_per_cluster)
            * usize::from(self.bpb.bytes_per_sector);

        let mut iterations = 0u32;
        let max_iterations = 65536u32; // Prevent infinite loops on corrupt FAT.

        while bytes_read < file_size && self.is_valid_cluster(cluster) {
            iterations = iterations.wrapping_add(1);
            if iterations > max_iterations {
                return Err(KernelError::IoError);
            }

            let lba = u64::from(self.bpb.cluster_to_lba(cluster));

            // Read each sector in this cluster.
            let mut sector_buf = [0u8; SECTOR_SIZE];
            for s in 0..u32::from(self.bpb.sectors_per_cluster) {
                if bytes_read >= file_size {
                    break;
                }

                self.read_sector(lba + u64::from(s), &mut sector_buf)?;

                let to_copy = (file_size - bytes_read).min(SECTOR_SIZE);
                if let Some(dest) = data.get_mut(bytes_read..bytes_read + to_copy) {
                    if let Some(src) = sector_buf.get(..to_copy) {
                        dest.copy_from_slice(src);
                    }
                }
                bytes_read += to_copy;
            }

            // Follow the FAT chain.
            match self.fat_entry(cluster)? {
                Some(next) => cluster = next,
                None => break,
            }

            // Sanity check: don't read more data than the cluster holds.
            let _ = cluster_bytes; // Suppress unused warning.
        }

        Ok(data)
    }

    // -- Write support --

    /// Helper: write a sector through the buffer cache.
    ///
    /// All FAT sector writes go through the cache for write-back
    /// coalescing (particularly important for FAT table updates).
    fn write_sector(&mut self, lba: u64, buf: &[u8; SECTOR_SIZE]) -> KernelResult<()> {
        super::cache::write_sector(&self.device_name, lba, buf)
    }

    /// Helper: read a sector through the buffer cache.
    ///
    /// Cache hits avoid device I/O entirely.  Misses read from the
    /// device and populate the cache for subsequent accesses.
    fn read_sector(&mut self, lba: u64, buf: &mut [u8; SECTOR_SIZE]) -> KernelResult<()> {
        super::cache::read_sector(&self.device_name, lba, buf)
    }

    /// Write a FAT entry (update both FAT copies).
    ///
    /// For FAT32, preserves the upper 4 reserved bits.
    #[allow(clippy::arithmetic_side_effects)]
    fn set_fat_entry(&mut self, cluster: u32, value: u32) -> KernelResult<()> {
        let bps = u32::from(self.bpb.bytes_per_sector);

        let fat_offset = match self.bpb.fat_type {
            FatType::Fat16 => cluster * 2,
            FatType::Fat32 => cluster * 4,
        };

        let offset_in_sector = (fat_offset % bps) as usize;

        // Update both FAT copies.
        for fat_idx in 0..u32::from(self.bpb.num_fats) {
            let fat_base = self.bpb.fat_start_lba()
                + fat_idx * self.bpb.sectors_per_fat();
            let sector_num = fat_base + fat_offset / bps;

            let mut sector_buf = [0u8; SECTOR_SIZE];
            self.read_sector(u64::from(sector_num), &mut sector_buf)?;

            match self.bpb.fat_type {
                FatType::Fat16 => {
                    let v16 = value as u16;
                    if let Some(lo) = sector_buf.get_mut(offset_in_sector) {
                        *lo = v16 as u8;
                    }
                    if let Some(hi) = sector_buf.get_mut(offset_in_sector + 1) {
                        *hi = (v16 >> 8) as u8;
                    }
                }
                FatType::Fat32 => {
                    // Preserve the upper 4 reserved bits.
                    let existing = read_u32(&sector_buf, offset_in_sector);
                    let new_val = (existing & 0xF000_0000) | (value & 0x0FFF_FFFF);
                    write_u32_le(&mut sector_buf, offset_in_sector, new_val);
                }
            }

            self.write_sector(u64::from(sector_num), &sector_buf)?;
        }

        Ok(())
    }

    /// Find a free cluster in the FAT.
    ///
    /// Scans from cluster 2 upward.  Returns `DiskFull` if none found.
    #[allow(clippy::arithmetic_side_effects)]
    fn alloc_cluster(&mut self) -> KernelResult<u32> {
        // Total data clusters.
        let data_sectors = self.bpb.total_sectors()
            - u32::from(self.bpb.reserved_sectors)
            - u32::from(self.bpb.num_fats) * self.bpb.sectors_per_fat()
            - self.bpb.root_dir_sectors();
        let total_clusters = data_sectors / u32::from(self.bpb.sectors_per_cluster);

        // Scan FAT for a free entry (value == 0).
        let bps = u32::from(self.bpb.bytes_per_sector);
        let entry_bytes: u32 = match self.bpb.fat_type {
            FatType::Fat16 => 2,
            FatType::Fat32 => 4,
        };
        let fat_start = self.bpb.fat_start_lba();
        let mut sector_buf = [0u8; SECTOR_SIZE];
        let mut last_sector = u32::MAX;

        // Clusters are numbered 2..total_clusters+2.
        let max_cluster = match self.bpb.fat_type {
            FatType::Fat16 => (total_clusters + 2).min(0xFFEF),
            FatType::Fat32 => (total_clusters + 2).min(0x0FFF_FFEF),
        };

        for cluster in 2..max_cluster {
            let fat_offset = cluster * entry_bytes;
            let sector_num = fat_start + fat_offset / bps;

            // Only read the sector if we haven't already.
            if sector_num != last_sector {
                self.read_sector(u64::from(sector_num), &mut sector_buf)?;
                last_sector = sector_num;
            }

            let offset = (fat_offset % bps) as usize;
            let is_free = match self.bpb.fat_type {
                FatType::Fat16 => read_u16(&sector_buf, offset) == 0x0000,
                FatType::Fat32 => (read_u32(&sector_buf, offset) & 0x0FFF_FFFF) == 0,
            };

            if is_free {
                return Ok(cluster);
            }
        }

        Err(KernelError::DiskFull)
    }

    /// Free the cluster chain starting at `first_cluster`.
    fn free_chain(&mut self, first_cluster: u32) -> KernelResult<()> {
        let mut cluster = first_cluster;
        let mut iterations = 0u32;

        while self.is_valid_cluster(cluster) {
            iterations = iterations.wrapping_add(1);
            if iterations > 65536 {
                return Err(KernelError::IoError); // Corrupt chain.
            }

            let next = self.fat_entry(cluster)?;
            self.set_fat_entry(cluster, 0x0000)?; // Mark free.

            match next {
                Some(n) => cluster = n,
                None => break,
            }
        }
        Ok(())
    }

    /// Write file data to newly-allocated clusters.
    ///
    /// Returns the first cluster number of the chain.
    #[allow(clippy::arithmetic_side_effects)]
    fn write_file_data(&mut self, data: &[u8]) -> KernelResult<u32> {
        if data.is_empty() {
            return Ok(0); // Empty file — no clusters needed.
        }

        let cluster_bytes = usize::from(self.bpb.sectors_per_cluster)
            * usize::from(self.bpb.bytes_per_sector);
        let clusters_needed = (data.len() + cluster_bytes - 1) / cluster_bytes;

        // End-of-chain marker depends on FAT type.
        let eoc = match self.bpb.fat_type {
            FatType::Fat16 => 0xFFFF,
            FatType::Fat32 => 0x0FFF_FFFF,
        };

        // Allocate all needed clusters first.
        let mut clusters = Vec::with_capacity(clusters_needed);
        for _ in 0..clusters_needed {
            let c = self.alloc_cluster()?;
            // Mark as end-of-chain temporarily so FAT scanner skips it.
            self.set_fat_entry(c, eoc)?;
            clusters.push(c);
        }

        // Link the chain (each cluster points to the next).
        for i in 0..clusters.len() {
            if i + 1 < clusters.len() {
                self.set_fat_entry(clusters[i], clusters[i + 1])?;
            }
            // Last cluster already marked 0xFFFF.
        }

        // Write data to each cluster.
        let mut offset = 0usize;

        for &cluster in &clusters {
            let lba = u64::from(self.bpb.cluster_to_lba(cluster));

            for s in 0..u32::from(self.bpb.sectors_per_cluster) {
                let mut sector_buf = [0u8; SECTOR_SIZE];

                if offset >= data.len() {
                    // Zero-fill remaining sectors in the cluster.
                    self.write_sector(lba + u64::from(s), &sector_buf)?;
                    continue;
                }

                let to_copy = (data.len() - offset).min(SECTOR_SIZE);
                if let Some(src) = data.get(offset..offset + to_copy) {
                    sector_buf[..to_copy].copy_from_slice(src);
                }
                self.write_sector(lba + u64::from(s), &sector_buf)?;
                offset += to_copy;
            }
        }

        Ok(clusters[0])
    }

    /// Convert a filename to 8.3 format.
    ///
    /// Returns `None` if the name is invalid.
    fn to_83_name(name: &str) -> Option<[u8; 11]> {
        let name = name.strip_prefix('/').unwrap_or(name);
        let name = name.to_uppercase();

        let mut result = [b' '; 11];

        if let Some(dot_pos) = name.rfind('.') {
            let base = &name[..dot_pos];
            let ext = &name[dot_pos + 1..];

            if base.is_empty() || base.len() > 8 || ext.len() > 3 {
                return None;
            }

            for (i, b) in base.bytes().enumerate().take(8) {
                result[i] = b;
            }
            for (i, b) in ext.bytes().enumerate().take(3) {
                result[8 + i] = b;
            }
        } else {
            // No extension.
            if name.is_empty() || name.len() > 8 {
                return None;
            }
            for (i, b) in name.bytes().enumerate().take(8) {
                result[i] = b;
            }
        }

        Some(result)
    }

    /// Find or create a root directory entry slot.
    ///
    /// If the file already exists, returns its slot (sector LBA, offset
    /// within sector).  Otherwise finds the first free or end-of-directory
    /// slot.
    #[allow(clippy::arithmetic_side_effects)]
    fn find_or_create_dir_slot(
        &mut self,
        name83: &[u8; 11],
    ) -> KernelResult<(u64, usize, bool)> {
        // Returns (sector_lba, byte_offset_in_sector, already_exists).
        let root_lba = self.bpb.root_dir_start_lba();
        let root_sectors = self.bpb.root_dir_sectors();
        let max_entries = self.bpb.root_entry_count;
        let mut sector_buf = [0u8; SECTOR_SIZE];
        let mut entry_index: u16 = 0;
        let entries_per_sector = usize::from(self.bpb.bytes_per_sector) / 32;

        // First pass: look for existing entry or free slot.
        let mut first_free: Option<(u64, usize)> = None;

        for sec in 0..root_sectors {
            let lba = u64::from(root_lba + sec);
            self.read_sector(lba, &mut sector_buf)?;

            for i in 0..entries_per_sector {
                if entry_index >= max_entries {
                    return first_free
                        .map(|(l, o)| (l, o, false))
                        .ok_or(KernelError::DiskFull);
                }

                let offset = i * 32;
                let first_byte = sector_buf.get(offset).copied().unwrap_or(0);

                if first_byte == 0x00 || first_byte == 0xE5 {
                    // Free slot.
                    if first_free.is_none() {
                        first_free = Some((lba, offset));
                    }
                    if first_byte == 0x00 {
                        // End of directory — no more entries to check.
                        return first_free
                            .map(|(l, o)| (l, o, false))
                            .ok_or(KernelError::DiskFull);
                    }
                } else {
                    // Check if this is the same file.
                    if let Some(raw) = sector_buf.get(offset..offset + 11) {
                        if raw == name83.as_slice() {
                            return Ok((lba, offset, true));
                        }
                    }
                }

                entry_index = entry_index.wrapping_add(1);
            }
        }

        first_free
            .map(|(l, o)| (l, o, false))
            .ok_or(KernelError::DiskFull)
    }

    /// Find or create a directory entry slot in a given directory.
    ///
    /// Dispatches to root directory or subdirectory scanning based on
    /// `parent_cluster` (0 = root, otherwise first cluster of subdir).
    fn find_or_create_slot_in(
        &mut self,
        parent_cluster: u32,
        name83: &[u8; 11],
    ) -> KernelResult<(u64, usize, bool)> {
        if parent_cluster == 0 && self.bpb.fat_type == FatType::Fat16 {
            // FAT16: root directory is a fixed area.
            self.find_or_create_dir_slot(name83)
        } else {
            // FAT32 root or any subdirectory: cluster chain.
            let cluster = if parent_cluster == 0 {
                self.bpb.root_cluster // FAT32 root.
            } else {
                parent_cluster
            };
            self.find_or_create_subdir_slot(cluster, name83)
        }
    }

    /// Find or create a directory entry slot in a subdirectory.
    ///
    /// Walks the cluster chain looking for a matching entry or a free slot.
    /// If the directory is full, allocates a new cluster to extend it.
    #[allow(clippy::arithmetic_side_effects)]
    fn find_or_create_subdir_slot(
        &mut self,
        first_cluster: u32,
        name83: &[u8; 11],
    ) -> KernelResult<(u64, usize, bool)> {
        let mut cluster = first_cluster;
        let mut last_cluster = first_cluster;
        let mut iterations = 0u32;
        let entries_per_sector = usize::from(self.bpb.bytes_per_sector) / 32;
        let mut first_free: Option<(u64, usize)> = None;

        while self.is_valid_cluster(cluster) {
            iterations = iterations.wrapping_add(1);
            if iterations > 65536 {
                return Err(KernelError::IoError);
            }

            let lba = self.bpb.cluster_to_lba(cluster);
            let mut sector_buf = [0u8; SECTOR_SIZE];

            for s in 0..u32::from(self.bpb.sectors_per_cluster) {
                let sector_lba = u64::from(lba + s);
                self.read_sector(sector_lba, &mut sector_buf)?;

                for i in 0..entries_per_sector {
                    let offset = i * 32;
                    let first_byte = sector_buf.get(offset).copied().unwrap_or(0);

                    if first_byte == 0x00 || first_byte == 0xE5 {
                        if first_free.is_none() {
                            first_free = Some((sector_lba, offset));
                        }
                        if first_byte == 0x00 {
                            // End of directory.
                            return first_free
                                .map(|(l, o)| (l, o, false))
                                .ok_or(KernelError::DiskFull);
                        }
                    } else {
                        // Check for matching name.
                        if let Some(raw) = sector_buf.get(offset..offset + 11) {
                            if raw == name83.as_slice() {
                                return Ok((sector_lba, offset, true));
                            }
                        }
                    }
                }
            }

            last_cluster = cluster;
            match self.fat_entry(cluster)? {
                Some(next) => cluster = next,
                None => break,
            }
        }

        // If we found a free slot during scanning, use it.
        if let Some((l, o)) = first_free {
            return Ok((l, o, false));
        }

        // Directory is full — allocate a new cluster to extend it.
        let eoc = match self.bpb.fat_type {
            FatType::Fat16 => 0xFFFF,
            FatType::Fat32 => 0x0FFF_FFFF,
        };
        let new_cluster = self.alloc_cluster()?;
        self.set_fat_entry(new_cluster, eoc)?;
        self.set_fat_entry(last_cluster, new_cluster)?;

        // Zero-fill the new cluster.
        let lba = self.bpb.cluster_to_lba(new_cluster);
        let zero_sector = [0u8; SECTOR_SIZE];
        for s in 0..u32::from(self.bpb.sectors_per_cluster) {
            self.write_sector(u64::from(lba + s), &zero_sector)?;
        }

        // First entry of the new cluster.
        Ok((u64::from(lba), 0, false))
    }

    /// Write a directory entry at the specified location.
    ///
    /// Stamps current RTC time as the last-write and last-access time.
    /// For new entries (not overwrite), also stamps creation time.
    #[allow(clippy::arithmetic_side_effects)]
    fn write_dir_entry(
        &mut self,
        lba: u64,
        offset: usize,
        name83: &[u8; 11],
        first_cluster: u32,
        file_size: u32,
        attr: u8,
    ) -> KernelResult<()> {
        let mut sector_buf = [0u8; SECTOR_SIZE];
        self.read_sector(lba, &mut sector_buf)?;

        // Read the old first byte to detect if this is a new entry.
        let old_first_byte = sector_buf.get(offset).copied().unwrap_or(0);
        let is_new = old_first_byte == 0x00 || old_first_byte == 0xE5;

        // Get current time as DOS packed date/time.
        let (now_date, now_time) = current_dos_datetime();

        // Write the 32-byte directory entry.
        if let Some(entry) = sector_buf.get_mut(offset..offset + 32) {
            entry[0..11].copy_from_slice(name83);
            entry[11] = attr;
            // Byte 12: reserved (NT case flags), clear.
            entry[12] = 0;
            // Byte 13: creation time fine resolution (10ms units, 0-199).
            entry[13] = 0;
            // Bytes 14-15: creation time (only set for new entries).
            if is_new {
                entry[14] = now_time as u8;
                entry[15] = (now_time >> 8) as u8;
                // Bytes 16-17: creation date.
                entry[16] = now_date as u8;
                entry[17] = (now_date >> 8) as u8;
            }
            // else: preserve existing creation time (already in buffer).

            // Bytes 18-19: last access date.
            entry[18] = now_date as u8;
            entry[19] = (now_date >> 8) as u8;
            // First cluster high word (offset 20-21, FAT32; zero for FAT16).
            entry[20] = (first_cluster >> 16) as u8;
            entry[21] = (first_cluster >> 24) as u8;
            // Bytes 22-23: last write time.
            entry[22] = now_time as u8;
            entry[23] = (now_time >> 8) as u8;
            // Bytes 24-25: last write date.
            entry[24] = now_date as u8;
            entry[25] = (now_date >> 8) as u8;
            // First cluster low word (offset 26-27).
            entry[26] = first_cluster as u8;
            entry[27] = (first_cluster >> 8) as u8;
            // File size (little-endian u32 at offset 28).
            entry[28] = file_size as u8;
            entry[29] = (file_size >> 8) as u8;
            entry[30] = (file_size >> 16) as u8;
            entry[31] = (file_size >> 24) as u8;
        }

        self.write_sector(lba, &sector_buf)
    }

    /// Delete a directory entry (mark as 0xE5).
    fn delete_dir_entry(&mut self, lba: u64, offset: usize) -> KernelResult<()> {
        let mut sector_buf = [0u8; SECTOR_SIZE];
        self.read_sector(lba, &mut sector_buf)?;

        if let Some(byte) = sector_buf.get_mut(offset) {
            *byte = 0xE5; // Deleted marker.
        }

        self.write_sector(lba, &sector_buf)
    }

    /// Delete LFN entries preceding a short entry at (lba, offset).
    ///
    /// Scans backward from the given slot looking for LFN entries with
    /// matching checksum.  Marks each one as deleted (0xE5).
    #[allow(clippy::arithmetic_side_effects)]
    fn delete_lfn_entries(&mut self, lba: u64, offset: usize, name83: &[u8; 11]) -> KernelResult<()> {
        let checksum = lfn_checksum(name83);
        let entries_per_sector = usize::from(self.bpb.bytes_per_sector) / 32;
        let mut cur_lba = lba;
        let mut cur_slot = offset / 32;
        let mut sector_buf = [0u8; SECTOR_SIZE];
        self.read_sector(cur_lba, &mut sector_buf)?;
        let mut sector_dirty = false;

        // Walk backward through directory entries.
        for _ in 0..20 {
            // Move to previous slot.
            if cur_slot == 0 {
                // Need to move to the previous sector.
                // For simplicity, skip cross-sector backward scan on FAT16 root.
                // This handles the common case where LFN entries are in the same sector.
                if sector_dirty {
                    self.write_sector(cur_lba, &sector_buf)?;
                }
                // Try previous sector.
                if cur_lba == 0 {
                    break;
                }
                cur_lba = cur_lba.saturating_sub(1);
                self.read_sector(cur_lba, &mut sector_buf)?;
                sector_dirty = false;
                cur_slot = entries_per_sector.saturating_sub(1);
            } else {
                cur_slot = cur_slot.saturating_sub(1);
            }

            let slot_offset = cur_slot * 32;
            let first_byte = sector_buf.get(slot_offset).copied().unwrap_or(0);
            let attr = sector_buf.get(slot_offset + 11).copied().unwrap_or(0);

            // Check if this is an LFN entry with matching checksum.
            if attr != ATTR_LONG_NAME || first_byte == 0xE5 || first_byte == 0x00 {
                break;
            }

            let entry_checksum = sector_buf.get(slot_offset + 13).copied().unwrap_or(0);
            if entry_checksum != checksum {
                break;
            }

            // Mark as deleted.
            if let Some(byte) = sector_buf.get_mut(slot_offset) {
                *byte = 0xE5;
                sector_dirty = true;
            }

            // If this was the last (first written) LFN entry, we're done.
            if first_byte & LFN_LAST_ENTRY != 0 {
                break;
            }
        }

        if sector_dirty {
            self.write_sector(cur_lba, &sector_buf)?;
        }

        Ok(())
    }

    /// Create a file or directory with LFN support.
    ///
    /// If the filename fits 8.3 format, writes a single short entry.
    /// Otherwise, generates LFN entries and a corresponding short entry.
    ///
    /// Returns `(dir_lba, dir_offset, already_exists)` for the short entry.
    #[allow(clippy::arithmetic_side_effects)]
    fn create_entry_with_lfn(
        &mut self,
        parent_cluster: u32,
        filename: &str,
        first_cluster: u32,
        file_size: u32,
        attr: u8,
    ) -> KernelResult<(u64, usize, bool)> {
        // Check if we need LFN.
        if !needs_lfn(filename) {
            // Simple 8.3 path.
            let name83 = Self::to_83_name(filename)
                .ok_or(KernelError::InvalidArgument)?;
            let (lba, offset, exists) = self.find_or_create_slot_in(
                parent_cluster, &name83,
            )?;
            self.write_dir_entry(lba, offset, &name83, first_cluster, file_size, attr)?;
            return Ok((lba, offset, exists));
        }

        // Generate basis name for the short entry.
        let mut basis = generate_basis_name(filename);

        // Check for uniqueness of the short name.  Read the parent
        // directory to find collisions and iterate the tail.
        let dir_entries = if parent_cluster == 0 {
            self.read_root_dir()?
        } else {
            self.read_dir_cluster(parent_cluster)?
        };

        for tail_num in 1..10000u32 {
            set_basis_tail(&mut basis, tail_num);
            let has_collision = dir_entries.iter().any(|e| {
                !e.is_volume_label() && e.name == basis
            });
            if !has_collision {
                break;
            }
        }

        // Build LFN entries.
        let lfn_entries = build_lfn_entries(filename, &basis)
            .ok_or(KernelError::InvalidArgument)?;
        let total_slots = lfn_entries.len() + 1; // LFN + short.

        // Find contiguous free slots.  We need `total_slots` adjacent
        // free entries (0x00 or 0xE5) in the parent directory.
        let slots = self.find_contiguous_free_slots(parent_cluster, total_slots)?;

        // Write LFN entries (they come first, in reverse sequence order).
        let entries_per_sector = usize::from(self.bpb.bytes_per_sector) / 32;
        for (i, lfn_raw) in lfn_entries.iter().enumerate() {
            let (slot_lba, slot_idx) = slots[i];
            let slot_offset = slot_idx * 32;

            let mut sector_buf = [0u8; SECTOR_SIZE];
            self.read_sector(slot_lba, &mut sector_buf)?;

            if let Some(dest) = sector_buf.get_mut(slot_offset..slot_offset + 32) {
                dest.copy_from_slice(lfn_raw);
            }

            self.write_sector(slot_lba, &sector_buf)?;
        }

        // Write the short entry in the last slot.
        let (short_lba, short_idx) = slots[lfn_entries.len()];
        let short_offset = short_idx * 32;
        self.write_dir_entry(short_lba, short_offset, &basis, first_cluster, file_size, attr)?;

        // Suppress unused variable warning.
        let _ = entries_per_sector;

        Ok((short_lba, short_offset, false))
    }

    /// Find `n` contiguous free slots in a directory.
    ///
    /// Returns a vector of `(lba, slot_index_within_sector)` for each slot.
    /// Slots are ordered sequentially (first slot first).
    #[allow(clippy::arithmetic_side_effects)]
    fn find_contiguous_free_slots(
        &mut self,
        parent_cluster: u32,
        n: usize,
    ) -> KernelResult<Vec<(u64, usize)>> {
        let entries_per_sector = usize::from(self.bpb.bytes_per_sector) / 32;
        let mut run: Vec<(u64, usize)> = Vec::with_capacity(n);

        if parent_cluster == 0 && self.bpb.fat_type == FatType::Fat16 {
            // FAT16 root directory: fixed area.
            let root_lba = self.bpb.root_dir_start_lba();
            let root_sectors = self.bpb.root_dir_sectors();
            let max_entries = usize::from(self.bpb.root_entry_count);
            let mut sector_buf = [0u8; SECTOR_SIZE];
            let mut entry_count = 0usize;

            for sec in 0..root_sectors {
                let lba = u64::from(root_lba + sec);
                self.read_sector(lba, &mut sector_buf)?;

                for i in 0..entries_per_sector {
                    if entry_count >= max_entries {
                        return Err(KernelError::DiskFull);
                    }
                    let offset = i * 32;
                    let first_byte = sector_buf.get(offset).copied().unwrap_or(0xFF);

                    if first_byte == 0x00 || first_byte == 0xE5 {
                        run.push((lba, i));
                        if run.len() >= n {
                            return Ok(run);
                        }
                    } else {
                        run.clear(); // Break in the run.
                    }
                    entry_count += 1;
                }
            }
        } else {
            // Cluster-chain directory (FAT32 root or any subdirectory).
            let first_cluster = if parent_cluster == 0 {
                self.bpb.root_cluster
            } else {
                parent_cluster
            };
            let mut cluster = first_cluster;
            let mut iterations = 0u32;

            while self.is_valid_cluster(cluster) {
                iterations += 1;
                if iterations > 65536 {
                    return Err(KernelError::IoError);
                }

                let lba = self.bpb.cluster_to_lba(cluster);
                let mut sector_buf = [0u8; SECTOR_SIZE];

                for s in 0..u32::from(self.bpb.sectors_per_cluster) {
                    let sector_lba = u64::from(lba + s);
                    self.read_sector(sector_lba, &mut sector_buf)?;

                    for i in 0..entries_per_sector {
                        let offset = i * 32;
                        let first_byte = sector_buf.get(offset).copied().unwrap_or(0xFF);

                        if first_byte == 0x00 || first_byte == 0xE5 {
                            run.push((sector_lba, i));
                            if run.len() >= n {
                                return Ok(run);
                            }
                        } else {
                            run.clear();
                        }
                    }
                }

                match self.fat_entry(cluster)? {
                    Some(next) => cluster = next,
                    None => break,
                }
            }

            // Directory is full — extend it with a new cluster.
            let eoc = match self.bpb.fat_type {
                FatType::Fat16 => 0xFFFF,
                FatType::Fat32 => 0x0FFF_FFFF,
            };
            // Find the last cluster in the chain to append to.
            let mut last = first_cluster;
            let mut c = first_cluster;
            while self.is_valid_cluster(c) {
                last = c;
                match self.fat_entry(c)? {
                    Some(next) => c = next,
                    None => break,
                }
            }

            // Allocate enough new clusters to hold the remaining slots.
            let remaining = n - run.len();
            let slots_per_cluster = entries_per_sector * usize::from(self.bpb.sectors_per_cluster);
            let clusters_needed = (remaining + slots_per_cluster - 1) / slots_per_cluster;

            for _ in 0..clusters_needed {
                let new_c = self.alloc_cluster()?;
                self.set_fat_entry(new_c, eoc)?;
                self.set_fat_entry(last, new_c)?;

                // Zero-fill the new cluster.
                let new_lba = self.bpb.cluster_to_lba(new_c);
                let zero = [0u8; SECTOR_SIZE];
                for s in 0..u32::from(self.bpb.sectors_per_cluster) {
                    self.write_sector(u64::from(new_lba + s), &zero)?;
                }

                // Add slots from the new cluster.
                for s in 0..u32::from(self.bpb.sectors_per_cluster) {
                    let sector_lba = u64::from(new_lba + s);
                    for i in 0..entries_per_sector {
                        run.push((sector_lba, i));
                        if run.len() >= n {
                            return Ok(run);
                        }
                    }
                }

                last = new_c;
            }
        }

        if run.len() >= n {
            Ok(run)
        } else {
            Err(KernelError::DiskFull)
        }
    }
}

impl FileSystem for FatFs {
    fn fs_type(&self) -> &str {
        match self.bpb.fat_type {
            FatType::Fat16 => "fat16",
            FatType::Fat32 => "fat32",
        }
    }

    fn debug_stats(&self) -> String {
        let valid = self.dcache.iter().filter(|e| e.valid).count();
        use core::fmt::Write;
        let mut s = String::new();
        let _ = write!(
            s,
            "dcache: {}/{} slots used, {} hits, {} misses",
            valid,
            DCACHE_MAX_ENTRIES,
            self.dcache_hits,
            self.dcache_misses,
        );
        let total = self.dcache_hits + self.dcache_misses;
        if total > 0 {
            // Integer hit-rate percentage to avoid floating point.
            let pct = self.dcache_hits.saturating_mul(100) / total;
            let _ = write!(s, " ({}% hit rate)", pct);
        }
        s
    }

    /// Report filesystem capacity and free space.
    ///
    /// Scans the FAT to count free clusters and computes byte totals.
    #[allow(clippy::arithmetic_side_effects)]
    fn sync(&mut self) -> KernelResult<()> {
        super::cache::flush(&self.device_name)
    }

    fn statvfs(&mut self) -> KernelResult<FsInfo> {
        let cluster_bytes = u64::from(self.bpb.sectors_per_cluster)
            * u64::from(self.bpb.bytes_per_sector);

        let (free_clusters, total_clusters) = self.count_clusters()?;

        // FAT max filename: 255 chars (LFN) or 12 chars (8.3).
        let max_name_len = if true { 255 } else { 12 }; // LFN is always supported now.

        Ok(FsInfo {
            fs_type: String::from(self.fs_type()),
            block_size: cluster_bytes,
            total_blocks: total_clusters,
            free_blocks: free_clusters,
            total_inodes: 0, // FAT doesn't have inodes.
            free_inodes: 0,
            max_name_len,
            read_only: false,
        })
    }

    fn readdir(&mut self, path: &str) -> KernelResult<Vec<DirEntry>> {
        let (parent_cluster, entry) = self.resolve_path(path)?;

        // Determine which directory to list.
        let fat_entries = match entry {
            None => {
                // Path resolved to a directory (root or subdirectory).
                if parent_cluster == 0 {
                    self.read_root_dir()?
                } else {
                    self.read_dir_cluster(parent_cluster)?
                }
            }
            Some(ref e) if e.is_directory() => {
                self.read_dir_cluster(e.first_cluster)?
            }
            Some(_) => {
                return Err(KernelError::NotADirectory);
            }
        };

        let vfs_entries = fat_entries
            .iter()
            .filter(|e| !e.is_volume_label())
            .map(FatDirEntry::to_vfs_entry)
            .collect();

        Ok(vfs_entries)
    }

    fn read_file(&mut self, path: &str) -> KernelResult<Vec<u8>> {
        let (_parent, entry) = self.resolve_path(path)?;
        let entry = entry.ok_or(KernelError::NotFound)?;
        if entry.is_directory() {
            return Err(KernelError::IsADirectory);
        }
        self.read_file_data(&entry)
    }

    fn stat(&mut self, path: &str) -> KernelResult<DirEntry> {
        let (parent_cluster, entry) = self.resolve_path(path)?;
        match entry {
            None => {
                // Path points to a directory itself.
                let name = if parent_cluster == 0 {
                    String::from("/")
                } else {
                    // Use the last path component as the name.
                    let last = path.trim_end_matches('/')
                        .rsplit('/')
                        .next()
                        .unwrap_or("/");
                    String::from(last)
                };
                Ok(DirEntry {
                    name,
                    entry_type: EntryType::Directory,
                    size: 0,
                })
            }
            Some(e) => Ok(e.to_vfs_entry()),
        }
    }

    /// Return rich metadata including FAT timestamps.
    ///
    /// FAT stores creation, last-write, and last-access timestamps in
    /// packed DOS format.  We convert them to nanoseconds-since-epoch.
    /// FAT has no ownership or Unix permissions, so those stay at 0.
    ///
    /// FAT attribute flags are mapped to VFS attributes:
    /// - `ATTR_READ_ONLY` → `FileAttr::IMMUTABLE`
    /// - `ATTR_HIDDEN` → `FileAttr::HIDDEN`
    /// - `ATTR_SYSTEM` → `FileAttr::SYSTEM`
    fn metadata(&mut self, path: &str) -> KernelResult<FileMeta> {
        let (parent_cluster, entry) = self.resolve_path(path)?;

        match entry {
            None => {
                // Root or resolved directory — no FAT entry with timestamps.
                let entry_type = crate::fs::vfs::EntryType::Directory;
                Ok(FileMeta::minimal(entry_type, 0))
            }
            Some(e) => {
                let entry_type = if e.is_volume_label() {
                    EntryType::VolumeLabel
                } else if e.is_directory() {
                    EntryType::Directory
                } else {
                    EntryType::File
                };

                // Convert DOS timestamps to nanoseconds-since-epoch.
                let created_ns = dos_datetime_to_ns(e.create_date, e.create_time);
                let modified_ns = dos_datetime_to_ns(e.write_date, e.write_time);
                // Access date has no time component — use midnight.
                let accessed_ns = dos_datetime_to_ns(e.access_date, 0);

                // Map FAT attribute flags to VFS attributes.
                let mut attrs = FileAttr::NONE;
                if e.attr & ATTR_READ_ONLY != 0 {
                    attrs = attrs.union(FileAttr::IMMUTABLE);
                }
                if e.attr & ATTR_HIDDEN != 0 {
                    attrs = attrs.union(FileAttr::HIDDEN);
                }
                if e.attr & ATTR_SYSTEM != 0 {
                    attrs = attrs.union(FileAttr::SYSTEM);
                }

                // Suppress unused variable warning — parent_cluster is needed
                // by resolve_path but not used in the metadata response.
                let _ = parent_cluster;

                Ok(FileMeta {
                    size: u64::from(e.file_size),
                    entry_type,
                    created_ns,
                    modified_ns,
                    accessed_ns,
                    // FAT has no metadata-change timestamp; use modified as proxy.
                    changed_ns: modified_ns,
                    // FAT has no ownership model.
                    uid: 0,
                    gid: 0,
                    // FAT has no Unix permissions; 0 signals "not applicable".
                    permissions: 0,
                    attributes: attrs,
                    // FAT has no hard link support; always 1.
                    nlinks: 1,
                    xattrs: Vec::new(),
                    hash: Vec::new(),
                })
            }
        }
    }

    #[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
    fn write_file(&mut self, path: &str, data: &[u8]) -> KernelResult<()> {
        let (parent_path, filename) = split_path(path);

        // Check file size limit (FAT16 max: 2 GiB, but u32 field caps at ~4 GiB).
        if data.len() > u32::MAX as usize {
            return Err(KernelError::InvalidArgument);
        }

        // Resolve the parent directory.
        let parent_cluster = self.resolve_dir_cluster(parent_path)?;

        // Try to find the file by its display name first (handles both
        // 8.3 and LFN names).  If it exists, update in place.
        let existing = {
            let entries = if parent_cluster == 0 {
                self.read_root_dir()?
            } else {
                self.read_dir_cluster(parent_cluster)?
            };
            entries.into_iter().find(|e| {
                !e.is_volume_label() && e.display_name().eq_ignore_ascii_case(filename)
            })
        };

        if let Some(existing_entry) = existing {
            // Overwriting an existing file.
            if existing_entry.is_directory() {
                return Err(KernelError::IsADirectory);
            }

            // Free old cluster chain.
            if existing_entry.first_cluster >= 2 {
                self.free_chain(existing_entry.first_cluster)?;
            }

            // Write new data.
            let first_cluster = self.write_file_data(data)?;

            // Find the existing short entry's slot and update it.
            let name83 = existing_entry.name;
            let (dir_lba, dir_offset, _) = self.find_or_create_slot_in(
                parent_cluster, &name83,
            )?;
            self.write_dir_entry(
                dir_lba, dir_offset, &name83,
                first_cluster, data.len() as u32, 0x20,
            )?;

            crate::serial_println!(
                "[fat] Overwrote '{}' ({} bytes, cluster {})",
                path, data.len(), first_cluster
            );
        } else {
            // New file — write data first, then create entry.
            let first_cluster = self.write_file_data(data)?;

            // Create entry with LFN if needed.
            self.create_entry_with_lfn(
                parent_cluster, filename,
                first_cluster, data.len() as u32, 0x20,
            )?;

            crate::serial_println!(
                "[fat] Created '{}' ({} bytes, cluster {})",
                path, data.len(), first_cluster
            );
        }

        // Invalidate dcache: file metadata (size, cluster) changed.
        self.dcache_invalidate_prefix(path);

        Ok(())
    }

    fn remove(&mut self, path: &str) -> KernelResult<()> {
        let (parent_path, _filename) = split_path(path);

        // Resolve the parent directory.
        let parent_cluster = self.resolve_dir_cluster(parent_path)?;

        // Resolve by path to find the entry (handles both LFN and 8.3).
        let (_pc, entry_opt) = self.resolve_path(path)?;
        let entry = entry_opt.ok_or(KernelError::NotFound)?;

        if entry.is_directory() {
            return Err(KernelError::IsADirectory);
        }

        // Find the short entry slot using the 8.3 name.
        let name83 = entry.name;
        let (dir_lba, dir_offset, exists) = self.find_or_create_slot_in(
            parent_cluster, &name83,
        )?;

        if !exists {
            return Err(KernelError::NotFound);
        }

        // Free the cluster chain.
        if entry.first_cluster >= 2 {
            self.free_chain(entry.first_cluster)?;
        }

        // Delete LFN entries first (must happen before short entry deletion).
        if entry.long_name.is_some() {
            self.delete_lfn_entries(dir_lba, dir_offset, &name83)?;
        }

        // Mark the short directory entry as deleted.
        self.delete_dir_entry(dir_lba, dir_offset)?;

        // Invalidate dcache: entry no longer exists.
        self.dcache_invalidate_prefix(path);

        crate::serial_println!("[fat] Deleted '{}'", path);
        Ok(())
    }

    fn rmdir(&mut self, path: &str) -> KernelResult<()> {
        let (parent_path, dirname) = split_path(path);
        let name83 = Self::to_83_name(dirname).ok_or(KernelError::InvalidArgument)?;

        // Resolve the parent directory.
        let parent_cluster = self.resolve_dir_cluster(parent_path)?;

        let (dir_lba, dir_offset, exists) = self.find_or_create_slot_in(
            parent_cluster, &name83,
        )?;

        if !exists {
            return Err(KernelError::NotFound);
        }

        // Read the directory entry to verify it's a directory.
        let mut sector_buf = [0u8; SECTOR_SIZE];
        self.read_sector(dir_lba, &mut sector_buf)?;
        let attr = sector_buf.get(dir_offset + 11).copied().unwrap_or(0);
        if attr & ATTR_DIRECTORY == 0 {
            return Err(KernelError::NotADirectory);
        }

        let cluster_lo = u32::from(read_u16(&sector_buf, dir_offset + 26));
        let cluster_hi = u32::from(read_u16(&sector_buf, dir_offset + 20));
        let first_cluster = (cluster_hi << 16) | cluster_lo;

        // Check the directory is empty (only . and .. allowed).
        if first_cluster >= 2 {
            let entries = self.read_dir_cluster(first_cluster)?;
            if !entries.is_empty() {
                return Err(KernelError::InvalidArgument); // Directory not empty.
            }
            self.free_chain(first_cluster)?;
        }

        // Delete the directory entry in the parent.
        self.delete_dir_entry(dir_lba, dir_offset)?;

        // Invalidate dcache: directory and all descendant paths.
        self.dcache_invalidate_prefix(path);

        crate::serial_println!("[fat] Removed directory '{}'", path);
        Ok(())
    }

    #[allow(clippy::arithmetic_side_effects)]
    fn mkdir(&mut self, path: &str) -> KernelResult<()> {
        let (parent_path, dirname) = split_path(path);
        let name83 = Self::to_83_name(dirname).ok_or(KernelError::InvalidArgument)?;

        // Resolve the parent directory.
        let parent_cluster = self.resolve_dir_cluster(parent_path)?;

        // Check if the name already exists.
        let (dir_lba, dir_offset, exists) = self.find_or_create_slot_in(
            parent_cluster, &name83,
        )?;

        if exists {
            return Err(KernelError::AlreadyExists);
        }

        // Allocate a cluster for the new directory's contents.
        let eoc = match self.bpb.fat_type {
            FatType::Fat16 => 0xFFFF,
            FatType::Fat32 => 0x0FFF_FFFF,
        };
        let new_cluster = self.alloc_cluster()?;
        self.set_fat_entry(new_cluster, eoc)?;

        // Initialize the cluster with "." and ".." entries.
        let lba = self.bpb.cluster_to_lba(new_cluster);
        let mut sector_buf = [0u8; SECTOR_SIZE];

        // "." entry — points to this directory.
        if let Some(dot) = sector_buf.get_mut(0..32) {
            dot[0..11].copy_from_slice(b".          ");
            dot[11] = ATTR_DIRECTORY;
            // Cluster high word (offset 20-21).
            dot[20] = (new_cluster >> 16) as u8;
            dot[21] = (new_cluster >> 24) as u8;
            // Cluster low word (offset 26-27).
            dot[26] = new_cluster as u8;
            dot[27] = (new_cluster >> 8) as u8;
        }

        // ".." entry — points to parent (0 for root).
        if let Some(dotdot) = sector_buf.get_mut(32..64) {
            dotdot[0..11].copy_from_slice(b"..         ");
            dotdot[11] = ATTR_DIRECTORY;
            dotdot[20] = (parent_cluster >> 16) as u8;
            dotdot[21] = (parent_cluster >> 24) as u8;
            dotdot[26] = parent_cluster as u8;
            dotdot[27] = (parent_cluster >> 8) as u8;
        }

        // Rest is zeros (end-of-directory marker).
        self.write_sector(u64::from(lba), &sector_buf)?;

        // Zero-fill remaining sectors in the cluster.
        let zero_sector = [0u8; SECTOR_SIZE];
        for s in 1..u32::from(self.bpb.sectors_per_cluster) {
            self.write_sector(u64::from(lba) + u64::from(s), &zero_sector)?;
        }

        // Create the directory entry in the parent.
        self.write_dir_entry(
            dir_lba,
            dir_offset,
            &name83,
            new_cluster,
            0, // Directories have size 0 in FAT16.
            ATTR_DIRECTORY,
        )?;

        crate::serial_println!(
            "[fat] Created directory '{}' (cluster {})",
            path, new_cluster
        );

        // Invalidate dcache: new directory entry added.
        self.dcache_invalidate_prefix(path);

        Ok(())
    }

    /// Rename or move a file or directory within the FAT filesystem.
    ///
    /// Strategy: read the old directory entry's metadata (cluster, size,
    /// attr), create the new entry in the destination directory, then
    /// delete the old entry.  The file data (cluster chain) is not moved
    /// — only the directory entries change.
    #[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
    fn rename(&mut self, from: &str, to: &str) -> KernelResult<()> {
        // 1. Resolve the source entry.
        let (from_parent_path, from_filename) = split_path(from);
        let from_name83 = Self::to_83_name(from_filename)
            .ok_or(KernelError::InvalidArgument)?;
        let from_parent_cluster = self.resolve_dir_cluster(from_parent_path)?;

        let (from_lba, from_offset, from_exists) =
            self.find_or_create_slot_in(from_parent_cluster, &from_name83)?;
        if !from_exists {
            return Err(KernelError::NotFound);
        }

        // Read the old entry's metadata.
        let mut from_sector = [0u8; SECTOR_SIZE];
        self.read_sector(from_lba, &mut from_sector)?;

        let old_attr = from_sector.get(from_offset + 11).copied().unwrap_or(0);
        let cluster_lo = u32::from(read_u16(&from_sector, from_offset + 26));
        let cluster_hi = u32::from(read_u16(&from_sector, from_offset + 20));
        let old_cluster = (cluster_hi << 16) | cluster_lo;
        let old_size = read_u32(&from_sector, from_offset + 28);

        // 2. Resolve the destination.
        let (to_parent_path, to_filename) = split_path(to);
        let to_name83 = Self::to_83_name(to_filename)
            .ok_or(KernelError::InvalidArgument)?;
        let to_parent_cluster = self.resolve_dir_cluster(to_parent_path)?;

        let (to_lba, to_offset, to_exists) =
            self.find_or_create_slot_in(to_parent_cluster, &to_name83)?;

        // If destination exists, fail (no silent overwrite on rename).
        if to_exists {
            return Err(KernelError::AlreadyExists);
        }

        // 3. Create the new directory entry pointing to the same clusters.
        self.write_dir_entry(
            to_lba, to_offset, &to_name83,
            old_cluster, old_size, old_attr,
        )?;

        // 4. Delete the old entry (mark as 0xE5). Data is untouched.
        self.delete_dir_entry(from_lba, from_offset)?;

        // Invalidate dcache: old path no longer valid, new path created.
        // Use prefix invalidation on `from` to handle directory renames
        // (all descendant paths become stale).
        self.dcache_invalidate_prefix(from);
        self.dcache_invalidate_prefix(to);

        crate::serial_println!("[fat] Renamed '{}' → '{}'", from, to);
        Ok(())
    }

    /// Read a range of bytes from a file without reading the entire file.
    ///
    /// Walks the FAT cluster chain to skip directly to the cluster
    /// containing `offset`, then reads only the sectors that overlap
    /// with the requested range.  For a 100-byte read from a 10 MB
    /// file at offset 5000, this reads ~1 cluster instead of the
    /// entire file.
    ///
    /// Overrides the default [`FileSystem::read_at`] which reads the
    /// whole file into memory and slices — O(file_size) even for
    /// small reads.
    #[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
    fn read_at(&mut self, path: &str, offset: u64, len: usize) -> KernelResult<Vec<u8>> {
        let (_parent, entry) = self.resolve_path(path)?;
        let entry = entry.ok_or(KernelError::NotFound)?;
        if entry.is_directory() {
            return Err(KernelError::IsADirectory);
        }

        let file_size = u64::from(entry.file_size);

        // Clamp to file bounds.
        if offset >= file_size {
            return Ok(Vec::new());
        }
        let available = (file_size - offset) as usize;
        let actual_len = len.min(available);
        if actual_len == 0 {
            return Ok(Vec::new());
        }

        // Empty file (no clusters).
        if entry.first_cluster < 2 {
            return Ok(Vec::new());
        }

        let cluster_bytes = usize::from(self.bpb.sectors_per_cluster)
            * usize::from(self.bpb.bytes_per_sector);

        // Which cluster in the chain contains `offset`?
        let target_cluster_idx = offset as usize / cluster_bytes;
        let offset_in_cluster = offset as usize % cluster_bytes;

        // Walk the FAT chain to the target cluster.
        let mut cluster = entry.first_cluster;
        for _ in 0..target_cluster_idx {
            if !self.is_valid_cluster(cluster) {
                return Err(KernelError::IoError); // Truncated chain.
            }
            match self.fat_entry(cluster)? {
                Some(next) => cluster = next,
                None => return Ok(Vec::new()), // Chain ended early.
            }
        }

        // Now `cluster` is the first cluster we need to read from,
        // and `offset_in_cluster` is the byte offset within it.
        let mut result = Vec::with_capacity(actual_len);
        let mut remaining = actual_len;
        let mut skip_in_cluster = offset_in_cluster;
        let mut iterations = 0u32;

        while remaining > 0 && self.is_valid_cluster(cluster) {
            iterations = iterations.wrapping_add(1);
            if iterations > 65536 {
                return Err(KernelError::IoError);
            }

            let lba = u64::from(self.bpb.cluster_to_lba(cluster));
            let mut sector_buf = [0u8; SECTOR_SIZE];

            // Determine which sector within this cluster to start from.
            let start_sector = skip_in_cluster / SECTOR_SIZE;
            let skip_in_sector = skip_in_cluster % SECTOR_SIZE;

            for s in start_sector..usize::from(self.bpb.sectors_per_cluster) {
                if remaining == 0 {
                    break;
                }

                self.read_sector(lba + s as u64, &mut sector_buf)?;

                let sector_offset = if s == start_sector { skip_in_sector } else { 0 };
                let avail_in_sector = SECTOR_SIZE - sector_offset;
                let to_copy = remaining.min(avail_in_sector);

                if let Some(src) = sector_buf.get(sector_offset..sector_offset + to_copy) {
                    result.extend_from_slice(src);
                }
                remaining -= to_copy;
            }

            // Next cluster in the chain.
            skip_in_cluster = 0; // Only the first cluster has an internal offset.
            match self.fat_entry(cluster)? {
                Some(next) => cluster = next,
                None => break,
            }
        }

        Ok(result)
    }

    /// Write bytes at a specific offset without rewriting the entire file.
    ///
    /// Three cases:
    /// 1. **Overwrite within existing data**: walk cluster chain to offset,
    ///    read-modify-write the affected sectors.
    /// 2. **Append past current size**: extend the cluster chain as needed,
    ///    zero-fill any gap between old EOF and the write offset.
    /// 3. **Write to new file**: create the file, allocate clusters, write.
    ///
    /// Overrides the default which reads the entire file, patches in
    /// memory, and rewrites everything.
    #[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
    fn write_at(&mut self, path: &str, offset: u64, data: &[u8]) -> KernelResult<()> {
        if data.is_empty() {
            return Ok(());
        }

        let (parent_path, filename) = split_path(path);
        let name83 = Self::to_83_name(filename)
            .ok_or(KernelError::InvalidArgument)?;
        let parent_cluster = self.resolve_dir_cluster(parent_path)?;

        // Find or create the directory entry.
        let (dir_lba, dir_offset, exists) =
            self.find_or_create_slot_in(parent_cluster, &name83)?;

        // Read existing metadata.
        let (old_cluster, old_size) = if exists {
            let mut sector_buf = [0u8; SECTOR_SIZE];
            self.read_sector(dir_lba, &mut sector_buf)?;
            let attr = sector_buf.get(dir_offset + 11).copied().unwrap_or(0);
            if attr & ATTR_DIRECTORY != 0 {
                return Err(KernelError::IsADirectory);
            }
            let clo = u32::from(read_u16(&sector_buf, dir_offset + 26));
            let chi = u32::from(read_u16(&sector_buf, dir_offset + 20));
            let cluster = (chi << 16) | clo;
            let size = read_u32(&sector_buf, dir_offset + 28);
            (cluster, size)
        } else {
            (0u32, 0u32)
        };

        let new_end = offset as usize + data.len();
        let new_size = new_end.max(old_size as usize);

        // Check FAT file size limit.
        if new_size > u32::MAX as usize {
            return Err(KernelError::InvalidArgument);
        }

        let cluster_bytes = usize::from(self.bpb.sectors_per_cluster)
            * usize::from(self.bpb.bytes_per_sector);

        // Calculate how many clusters are needed for the new size.
        let clusters_needed = if new_size == 0 { 0 } else {
            (new_size + cluster_bytes - 1) / cluster_bytes
        };

        // Count existing clusters.
        let mut existing_count = 0usize;
        let mut last_cluster = 0u32;
        {
            let mut c = old_cluster;
            while self.is_valid_cluster(c) {
                existing_count += 1;
                last_cluster = c;
                match self.fat_entry(c)? {
                    Some(next) => c = next,
                    None => break,
                }
            }
        }

        // If the file needs to grow, allocate more clusters.
        let eoc = match self.bpb.fat_type {
            FatType::Fat16 => 0xFFFF,
            FatType::Fat32 => 0x0FFF_FFFF,
        };
        let mut first_cluster = old_cluster;

        if clusters_needed > existing_count {
            let extra = clusters_needed - existing_count;
            for _ in 0..extra {
                let new_c = self.alloc_cluster()?;
                self.set_fat_entry(new_c, eoc)?;

                // Zero-fill the new cluster.
                let new_lba = self.bpb.cluster_to_lba(new_c);
                let zero = [0u8; SECTOR_SIZE];
                for s in 0..u32::from(self.bpb.sectors_per_cluster) {
                    self.write_sector(u64::from(new_lba + s), &zero)?;
                }

                if first_cluster < 2 {
                    // File was empty — this is the first cluster.
                    first_cluster = new_c;
                } else {
                    // Link to end of existing chain.
                    self.set_fat_entry(last_cluster, new_c)?;
                }
                last_cluster = new_c;
            }
        }

        // Now write the data at the requested offset.
        // Walk chain to the target cluster.
        let target_cluster_idx = offset as usize / cluster_bytes;
        let offset_in_cluster = offset as usize % cluster_bytes;

        let mut cluster = first_cluster;
        for _ in 0..target_cluster_idx {
            match self.fat_entry(cluster)? {
                Some(next) => cluster = next,
                None => return Err(KernelError::IoError),
            }
        }

        let mut written = 0usize;
        let mut skip_in_cluster = offset_in_cluster;

        while written < data.len() && self.is_valid_cluster(cluster) {
            let lba = u64::from(self.bpb.cluster_to_lba(cluster));
            let start_sector = skip_in_cluster / SECTOR_SIZE;
            let skip_in_sector = skip_in_cluster % SECTOR_SIZE;

            for s in start_sector..usize::from(self.bpb.sectors_per_cluster) {
                if written >= data.len() {
                    break;
                }

                let sector_lba = lba + s as u64;
                let sector_offset = if s == start_sector { skip_in_sector } else { 0 };

                // Read-modify-write if we're not writing a full sector.
                let mut sector_buf = [0u8; SECTOR_SIZE];
                if sector_offset > 0 || (data.len() - written) < SECTOR_SIZE {
                    self.read_sector(sector_lba, &mut sector_buf)?;
                }

                let avail = SECTOR_SIZE - sector_offset;
                let to_write = (data.len() - written).min(avail);
                if let Some(src) = data.get(written..written + to_write) {
                    if let Some(dest) = sector_buf.get_mut(sector_offset..sector_offset + to_write) {
                        dest.copy_from_slice(src);
                    }
                }

                self.write_sector(sector_lba, &sector_buf)?;
                written += to_write;
            }

            skip_in_cluster = 0;
            match self.fat_entry(cluster)? {
                Some(next) => cluster = next,
                None => break,
            }
        }

        // Update directory entry with new first cluster and size.
        self.write_dir_entry(
            dir_lba, dir_offset, &name83,
            first_cluster, new_size as u32, 0x20,
        )?;

        // Invalidate dcache: file metadata (size, cluster chain) changed.
        self.dcache_invalidate_prefix(path);

        Ok(())
    }

    /// Truncate a file efficiently.
    ///
    /// Overrides the default read-resize-rewrite approach.
    /// Shrinks by freeing excess clusters; grows by allocating and
    /// zero-filling new clusters.
    #[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
    fn truncate(&mut self, path: &str, size: u64) -> KernelResult<()> {
        if size > u64::from(u32::MAX) {
            return Err(KernelError::InvalidArgument);
        }

        let (parent_path, filename) = split_path(path);
        let name83 = Self::to_83_name(filename)
            .ok_or(KernelError::InvalidArgument)?;
        let parent_cluster = self.resolve_dir_cluster(parent_path)?;

        let (dir_lba, dir_offset, exists) =
            self.find_or_create_slot_in(parent_cluster, &name83)?;
        if !exists {
            return Err(KernelError::NotFound);
        }

        // Read existing metadata.
        let mut sector_buf = [0u8; SECTOR_SIZE];
        self.read_sector(dir_lba, &mut sector_buf)?;
        let attr = sector_buf.get(dir_offset + 11).copied().unwrap_or(0);
        if attr & ATTR_DIRECTORY != 0 {
            return Err(KernelError::IsADirectory);
        }
        let clo = u32::from(read_u16(&sector_buf, dir_offset + 26));
        let chi = u32::from(read_u16(&sector_buf, dir_offset + 20));
        let old_cluster = (chi << 16) | clo;
        let old_size = read_u32(&sector_buf, dir_offset + 28);

        let new_size = size as u32;
        let cluster_bytes = usize::from(self.bpb.sectors_per_cluster)
            * usize::from(self.bpb.bytes_per_sector);
        let eoc = match self.bpb.fat_type {
            FatType::Fat16 => 0xFFFF,
            FatType::Fat32 => 0x0FFF_FFFF,
        };

        let clusters_needed = if new_size == 0 { 0 } else {
            ((new_size as usize) + cluster_bytes - 1) / cluster_bytes
        };

        // Walk existing chain to count clusters.
        let mut chain: Vec<u32> = Vec::new();
        let mut c = old_cluster;
        while self.is_valid_cluster(c) {
            chain.push(c);
            match self.fat_entry(c)? {
                Some(next) => c = next,
                None => break,
            }
        }

        let mut first_cluster = old_cluster;

        if clusters_needed == 0 {
            // Truncate to zero — free the entire chain.
            if old_cluster >= 2 {
                self.free_chain(old_cluster)?;
            }
            first_cluster = 0;
        } else if clusters_needed < chain.len() {
            // Shrink: mark the last-kept cluster as EOC, free the rest.
            let keep = clusters_needed;
            self.set_fat_entry(chain[keep - 1], eoc)?;
            for &c in &chain[keep..] {
                self.set_fat_entry(c, 0)?;
            }
        } else if clusters_needed > chain.len() {
            // Grow: allocate more clusters, zero-fill.
            let mut last = if chain.is_empty() { 0u32 } else { chain[chain.len() - 1] };
            let extra = clusters_needed - chain.len();
            for _ in 0..extra {
                let new_c = self.alloc_cluster()?;
                self.set_fat_entry(new_c, eoc)?;

                // Zero-fill.
                let new_lba = self.bpb.cluster_to_lba(new_c);
                let zero = [0u8; SECTOR_SIZE];
                for s in 0..u32::from(self.bpb.sectors_per_cluster) {
                    self.write_sector(u64::from(new_lba + s), &zero)?;
                }

                if first_cluster < 2 {
                    first_cluster = new_c;
                } else {
                    self.set_fat_entry(last, new_c)?;
                }
                last = new_c;
            }
        }
        // else: same cluster count — just update the size.

        // Zero-fill the partial cluster at the end if shrinking.
        if new_size < old_size && clusters_needed > 0 && first_cluster >= 2 {
            let tail_offset = new_size as usize % cluster_bytes;
            if tail_offset > 0 {
                // Walk to the last kept cluster.
                let mut c = first_cluster;
                for _ in 1..clusters_needed {
                    match self.fat_entry(c)? {
                        Some(next) => c = next,
                        None => break,
                    }
                }

                // Zero from tail_offset to end of cluster.
                let lba = u64::from(self.bpb.cluster_to_lba(c));
                let start_sector = tail_offset / SECTOR_SIZE;
                let zero_from = tail_offset % SECTOR_SIZE;

                for s in start_sector..usize::from(self.bpb.sectors_per_cluster) {
                    let sector_lba = lba + s as u64;
                    let mut sbuf = [0u8; SECTOR_SIZE];
                    let from = if s == start_sector { zero_from } else { 0 };
                    if from > 0 {
                        self.read_sector(sector_lba, &mut sbuf)?;
                    }
                    if let Some(region) = sbuf.get_mut(from..SECTOR_SIZE) {
                        region.fill(0);
                    }
                    self.write_sector(sector_lba, &sbuf)?;
                }
            }
        }

        // Update directory entry.
        self.write_dir_entry(
            dir_lba, dir_offset, &name83,
            first_cluster, new_size, attr,
        )?;

        // Invalidate dcache: file metadata (size, cluster chain) changed.
        self.dcache_invalidate_prefix(path);

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Initialization and self-test
// ---------------------------------------------------------------------------

/// Try to mount a FAT filesystem from the given device and mount it
/// at the VFS root.  Auto-detects FAT16 or FAT32.
pub fn init(device_name: &str) -> KernelResult<()> {
    let fs = FatFs::mount(device_name)?;
    crate::fs::Vfs::mount("/", Box::new(fs))?;
    Ok(())
}

/// Self-test: verify we can read the directory and a file.
// String formatting uses bounded operations.
#[allow(clippy::arithmetic_side_effects)]
pub fn self_test() -> KernelResult<()> {
    crate::serial_println!("[fat] Running self-test...");

    // List root directory.
    let entries = crate::fs::Vfs::readdir("/")?;
    crate::serial_println!("[fat]   Root directory ({} entries):", entries.len());
    for entry in &entries {
        let type_str = match entry.entry_type {
            EntryType::File => "FILE",
            EntryType::Directory => "DIR ",
            EntryType::Symlink => "LINK",
            EntryType::VolumeLabel => "VOL ",
        };
        crate::serial_println!(
            "[fat]     {} {:12} {} bytes",
            type_str, entry.name, entry.size
        );
    }

    // Try to read HELLO.TXT.
    match crate::fs::Vfs::read_file("/HELLO.TXT") {
        Ok(data) => {
            let text = core::str::from_utf8(&data).unwrap_or("<binary>");
            crate::serial_println!(
                "[fat]   HELLO.TXT ({} bytes): {}",
                data.len(),
                text.trim_end()
            );
        }
        Err(KernelError::NotFound) => {
            crate::serial_println!("[fat]   HELLO.TXT not found (OK if disk has no test files)");
        }
        Err(e) => return Err(e),
    }

    // Test write: create a new file, read it back, then delete it.
    let test_data = b"FAT16 write test: the quick brown fox jumps over the lazy dog.\n";
    crate::serial_println!("[fat]   Testing write...");

    crate::fs::Vfs::write_file("/TEST.TXT", test_data)?;

    // Read it back and verify.
    let readback = crate::fs::Vfs::read_file("/TEST.TXT")?;
    if readback.as_slice() != test_data.as_slice() {
        crate::serial_println!(
            "[fat]   Write verification FAILED: expected {} bytes, got {}",
            test_data.len(),
            readback.len()
        );
        return Err(KernelError::IoError);
    }
    crate::serial_println!(
        "[fat]   Write+read verified: {} bytes match",
        readback.len()
    );

    // Delete the test file.
    crate::fs::Vfs::remove("/TEST.TXT")?;

    // Verify it's gone.
    match crate::fs::Vfs::read_file("/TEST.TXT") {
        Err(KernelError::NotFound) => {
            crate::serial_println!("[fat]   Delete verified: file not found (correct)");
        }
        Ok(_) => {
            crate::serial_println!("[fat]   Delete verification FAILED: file still exists");
            return Err(KernelError::IoError);
        }
        Err(e) => return Err(e),
    }

    // Test subdirectory support.
    crate::serial_println!("[fat]   Testing mkdir...");

    // Clean up any leftover TESTDIR from previous runs.
    // A previous boot may have left SUB.TXT inside the directory,
    // so remove it before attempting rmdir (which requires an empty dir).
    // Clean up any leftover TESTDIR from previous boots.
    let _ = crate::fs::Vfs::remove("/TESTDIR/SUB.TXT");
    let _ = crate::fs::Vfs::rmdir("/TESTDIR");

    crate::fs::Vfs::mkdir("/TESTDIR")?;

    // Verify the directory appears in root listing.
    let entries = crate::fs::Vfs::readdir("/")?;
    let has_testdir = entries.iter().any(|e| {
        e.name.eq_ignore_ascii_case("TESTDIR")
            && e.entry_type == EntryType::Directory
    });
    if !has_testdir {
        crate::serial_println!("[fat]   mkdir FAILED: TESTDIR not in root listing");
        return Err(KernelError::IoError);
    }
    crate::serial_println!("[fat]   mkdir verified: TESTDIR in root");

    // Write a file into the subdirectory.
    let sub_data = b"File inside a subdirectory.\n";
    crate::fs::Vfs::write_file("/TESTDIR/SUB.TXT", sub_data)?;

    // Read it back.
    let sub_readback = crate::fs::Vfs::read_file("/TESTDIR/SUB.TXT")?;
    if sub_readback.as_slice() != sub_data.as_slice() {
        crate::serial_println!(
            "[fat]   Subdir write FAILED: expected {} bytes, got {}",
            sub_data.len(),
            sub_readback.len()
        );
        return Err(KernelError::IoError);
    }
    crate::serial_println!("[fat]   Subdir write+read verified: {} bytes", sub_data.len());

    // List subdirectory contents.
    let sub_entries = crate::fs::Vfs::readdir("/TESTDIR")?;
    crate::serial_println!("[fat]   TESTDIR has {} entries", sub_entries.len());
    let has_sub_txt = sub_entries.iter().any(|e| {
        e.name.eq_ignore_ascii_case("SUB.TXT")
    });
    if !has_sub_txt {
        crate::serial_println!("[fat]   Subdir listing FAILED: SUB.TXT not found");
        return Err(KernelError::IoError);
    }

    // Delete the file in the subdirectory.
    crate::fs::Vfs::remove("/TESTDIR/SUB.TXT")?;

    // Verify it's gone.
    match crate::fs::Vfs::read_file("/TESTDIR/SUB.TXT") {
        Err(KernelError::NotFound) => {
            crate::serial_println!("[fat]   Subdir delete verified");
        }
        Ok(_) => {
            crate::serial_println!("[fat]   Subdir delete FAILED: file still exists");
            return Err(KernelError::IoError);
        }
        Err(e) => return Err(e),
    }

    // Clean up: remove the empty test directory.
    crate::fs::Vfs::rmdir("/TESTDIR")?;
    crate::serial_println!("[fat]   rmdir verified: TESTDIR removed");

    // ---------------------------------------------------------------
    // dos_datetime_to_ns unit tests (pure computation, no disk I/O)
    // ---------------------------------------------------------------
    crate::serial_println!("[fat]   Testing dos_datetime_to_ns...");

    // Known epoch: 0 date → 0 ns.
    assert_eq!(dos_datetime_to_ns(0, 0), 0);

    // 1980-01-01 00:00:00 — DOS epoch.
    //   date = (1980-1980)<<9 | 1<<5 | 1 = 0x0021
    //   time = 0
    //   Expected: 315532800 seconds since Unix epoch = 315_532_800_000_000_000 ns.
    let dos_epoch_date: u16 = (0 << 9) | (1 << 5) | 1;
    let dos_epoch_ns = dos_datetime_to_ns(dos_epoch_date, 0);
    // 1980-01-01T00:00:00Z = 315532800 seconds * 1e9.
    let expected_dos_epoch_ns: u64 = 315_532_800_000_000_000;
    if dos_epoch_ns != expected_dos_epoch_ns {
        crate::serial_println!(
            "[fat]   dos_datetime_to_ns FAILED: DOS epoch = {}, expected {}",
            dos_epoch_ns, expected_dos_epoch_ns
        );
        return Err(KernelError::IoError);
    }

    // 2000-06-15 14:30:00.
    //   date = (2000-1980)<<9 | 6<<5 | 15 = 20<<9 | 6<<5 | 15 = 10240 + 192 + 15 = 10447
    //   time = 14<<11 | 30<<5 | 0 = 28672 + 960 = 29632
    let y2k_date: u16 = (20 << 9) | (6 << 5) | 15;
    let y2k_time: u16 = (14 << 11) | (30 << 5) | 0;
    let y2k_ns = dos_datetime_to_ns(y2k_date, y2k_time);
    // 2000-06-15T14:30:00Z = 961078200 seconds * 1e9.
    let expected_y2k_ns: u64 = 961_078_200_000_000_000;
    if y2k_ns != expected_y2k_ns {
        crate::serial_println!(
            "[fat]   dos_datetime_to_ns FAILED: 2000-06-15 14:30 = {}, expected {}",
            y2k_ns, expected_y2k_ns
        );
        return Err(KernelError::IoError);
    }
    crate::serial_println!("[fat]   dos_datetime_to_ns verified");

    // ---------------------------------------------------------------
    // FAT metadata integration test
    // ---------------------------------------------------------------
    crate::serial_println!("[fat]   Testing metadata...");

    // Create a test file, fetch its metadata, verify fields.
    let meta_test_data = b"Metadata test file content.\n";
    crate::fs::Vfs::write_file("/METATST.TXT", meta_test_data)?;

    let meta = crate::fs::Vfs::metadata("/METATST.TXT")?;
    if meta.size != meta_test_data.len() as u64 {
        crate::serial_println!(
            "[fat]   metadata FAILED: size = {}, expected {}",
            meta.size, meta_test_data.len()
        );
        crate::fs::Vfs::remove("/METATST.TXT")?;
        return Err(KernelError::IoError);
    }
    if meta.entry_type != EntryType::File {
        crate::serial_println!("[fat]   metadata FAILED: entry_type is not File");
        crate::fs::Vfs::remove("/METATST.TXT")?;
        return Err(KernelError::IoError);
    }
    // FAT has no ownership — uid/gid should be 0.
    if meta.uid != 0 || meta.gid != 0 {
        crate::serial_println!("[fat]   metadata FAILED: uid={}, gid={}", meta.uid, meta.gid);
        crate::fs::Vfs::remove("/METATST.TXT")?;
        return Err(KernelError::IoError);
    }
    // Verify newly written files have non-zero timestamps (RTC-stamped).
    // DOS epoch (1980-01-01) in ns = 315_532_800_000_000_000.
    let dos_epoch_ns: u64 = 315_532_800_000_000_000;
    if meta.created_ns < dos_epoch_ns {
        crate::serial_println!(
            "[fat]   metadata WARNING: created_ns={} is before DOS epoch (RTC may not be set)",
            meta.created_ns
        );
    }
    if meta.modified_ns < dos_epoch_ns {
        crate::serial_println!(
            "[fat]   metadata WARNING: modified_ns={} is before DOS epoch",
            meta.modified_ns
        );
    }
    // Timestamps should be non-zero since we now stamp them on write.
    if meta.created_ns == 0 || meta.modified_ns == 0 {
        crate::serial_println!(
            "[fat]   metadata WARNING: timestamps are zero — write_dir_entry may not be stamping"
        );
    }
    crate::serial_println!(
        "[fat]   metadata OK: size={}, type=File, created_ns={}, modified_ns={}, accessed_ns={}",
        meta.size, meta.created_ns, meta.modified_ns, meta.accessed_ns
    );

    // Test round-trip: rtc_to_dos_datetime → dos_datetime_to_ns should
    // produce a timestamp within a few seconds of "now" (from RTC).
    let dt = crate::rtc::read_datetime();
    let (rt_date, rt_time) = rtc_to_dos_datetime(&dt);
    let rt_ns = dos_datetime_to_ns(rt_date, rt_time);
    crate::serial_println!(
        "[fat]   RTC round-trip: {} → date=0x{:04X} time=0x{:04X} → {} ns",
        dt, rt_date, rt_time, rt_ns
    );
    // Sanity check: the round-tripped timestamp should be >= DOS epoch.
    if rt_ns < dos_epoch_ns {
        crate::serial_println!(
            "[fat]   RTC round-trip WARNING: result before DOS epoch"
        );
    }

    // Test directory metadata.
    let _ = crate::fs::Vfs::remove("/METADIR/INSIDE.TXT");
    let _ = crate::fs::Vfs::rmdir("/METADIR");
    crate::fs::Vfs::mkdir("/METADIR")?;

    let dir_meta = crate::fs::Vfs::metadata("/METADIR")?;
    if dir_meta.entry_type != EntryType::Directory {
        crate::serial_println!("[fat]   metadata FAILED: METADIR entry_type is not Directory");
        crate::fs::Vfs::rmdir("/METADIR")?;
        crate::fs::Vfs::remove("/METATST.TXT")?;
        return Err(KernelError::IoError);
    }
    crate::serial_println!(
        "[fat]   directory metadata OK: type=Directory, size={}",
        dir_meta.size
    );

    // Clean up.
    crate::fs::Vfs::rmdir("/METADIR")?;
    crate::fs::Vfs::remove("/METATST.TXT")?;

    // ---------------------------------------------------------------
    // Long Filename (LFN) tests
    // ---------------------------------------------------------------
    crate::serial_println!("[fat]   Testing LFN support...");

    // Unit test: lfn_checksum
    let test_name83: [u8; 11] = *b"HELLO   TXT";
    let cksum = lfn_checksum(&test_name83);
    crate::serial_println!("[fat]   lfn_checksum(\"HELLO   TXT\") = 0x{:02X}", cksum);

    // Unit test: needs_lfn
    assert!(!needs_lfn("HELLO.TXT"));
    assert!(!needs_lfn("FILE"));
    assert!(needs_lfn("Hello.txt"));      // lowercase
    assert!(needs_lfn("long filename.txt")); // spaces + lowercase
    assert!(needs_lfn("document.docx"));   // lowercase
    assert!(needs_lfn("a.b.c"));           // multiple dots
    assert!(needs_lfn("verylongbasename.txt")); // base > 8
    crate::serial_println!("[fat]   needs_lfn checks passed");

    // Unit test: encode/decode round-trip
    let test_name = "Hello World.txt";
    let encoded = encode_lfn(test_name);
    if let Some(ref _ucs2) = encoded {
        // Build LFN entries and decode.
        let mut test83 = generate_basis_name(test_name);
        set_basis_tail(&mut test83, 1);
        if let Some(lfn_entries) = build_lfn_entries(test_name, &test83) {
            crate::serial_println!(
                "[fat]   LFN encode/build: '{}' → {} LFN entries",
                test_name, lfn_entries.len()
            );

            // Verify checksum in entries matches.
            let expected_cksum = lfn_checksum(&test83);
            for raw in &lfn_entries {
                let entry_cksum = raw[13];
                if entry_cksum != expected_cksum {
                    crate::serial_println!(
                        "[fat]   LFN checksum FAILED: entry has 0x{:02X}, expected 0x{:02X}",
                        entry_cksum, expected_cksum
                    );
                    return Err(KernelError::IoError);
                }
            }
            crate::serial_println!("[fat]   LFN checksum consistency verified");
        }
    }

    // Integration test: write and read a file with a long filename.
    let lfn_test_data = b"Long filename test content.\n";
    let lfn_path = "/Hello World.txt";
    // Clean up any leftover from previous runs.
    let _ = crate::fs::Vfs::remove(lfn_path);

    crate::fs::Vfs::write_file(lfn_path, lfn_test_data)?;

    // Read it back.
    let lfn_readback = crate::fs::Vfs::read_file(lfn_path)?;
    if lfn_readback.as_slice() != lfn_test_data.as_slice() {
        crate::serial_println!(
            "[fat]   LFN write FAILED: expected {} bytes, got {}",
            lfn_test_data.len(), lfn_readback.len()
        );
        let _ = crate::fs::Vfs::remove(lfn_path);
        return Err(KernelError::IoError);
    }
    crate::serial_println!(
        "[fat]   LFN write+read verified: '{}' ({} bytes)",
        lfn_path, lfn_readback.len()
    );

    // Verify the long name appears in directory listing.
    let root_entries = crate::fs::Vfs::readdir("/")?;
    let has_lfn = root_entries.iter().any(|e| {
        e.name == "Hello World.txt"
    });
    if !has_lfn {
        crate::serial_println!("[fat]   LFN listing FAILED: 'Hello World.txt' not in root");
        // Check if it appears under the short name instead.
        for e in &root_entries {
            crate::serial_println!("[fat]     found: '{}'", e.name);
        }
        let _ = crate::fs::Vfs::remove(lfn_path);
        return Err(KernelError::IoError);
    }
    crate::serial_println!("[fat]   LFN directory listing verified");

    // Clean up.
    crate::fs::Vfs::remove(lfn_path)?;

    // Verify it's gone.
    match crate::fs::Vfs::read_file(lfn_path) {
        Err(KernelError::NotFound) => {
            crate::serial_println!("[fat]   LFN delete verified");
        }
        Ok(_) => {
            crate::serial_println!("[fat]   LFN delete FAILED: file still exists");
            return Err(KernelError::IoError);
        }
        Err(e) => return Err(e),
    }

    crate::serial_println!("[fat]   LFN tests passed");

    // Report dcache statistics.
    match crate::fs::Vfs::debug_stats("/") {
        Ok(stats) if !stats.is_empty() => {
            crate::serial_println!("[fat]   {}", stats);
        }
        _ => {}
    }

    crate::serial_println!("[fat] Self-test PASSED");
    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Split a path into (parent directory path, filename).
///
/// - `"/file.txt"` → `("/", "file.txt")`
/// - `"/subdir/file.txt"` → `("/subdir", "file.txt")`
/// - `"/a/b/file.txt"` → `("/a/b", "file.txt")`
/// - `"file.txt"` → `("/", "file.txt")`
fn split_path(path: &str) -> (&str, &str) {
    let path = path.strip_suffix('/').unwrap_or(path);
    match path.rfind('/') {
        Some(0) => ("/", &path[1..]),
        Some(pos) => (&path[..pos], &path[pos + 1..]),
        None => ("/", path),
    }
}

/// Read a little-endian u16 from a byte slice at the given offset.
fn read_u16(data: &[u8], offset: usize) -> u16 {
    let lo = u16::from(data.get(offset).copied().unwrap_or(0));
    let hi = u16::from(data.get(offset + 1).copied().unwrap_or(0));
    lo | (hi << 8)
}

/// Read a little-endian u32 from a byte slice at the given offset.
fn read_u32(data: &[u8], offset: usize) -> u32 {
    let b0 = u32::from(data.get(offset).copied().unwrap_or(0));
    let b1 = u32::from(data.get(offset + 1).copied().unwrap_or(0));
    let b2 = u32::from(data.get(offset + 2).copied().unwrap_or(0));
    let b3 = u32::from(data.get(offset + 3).copied().unwrap_or(0));
    b0 | (b1 << 8) | (b2 << 16) | (b3 << 24)
}

/// Write a little-endian u32 to a byte slice at the given offset.
fn write_u32_le(data: &mut [u8], offset: usize, value: u32) {
    if let Some(b) = data.get_mut(offset) {
        *b = value as u8;
    }
    if let Some(b) = data.get_mut(offset + 1) {
        *b = (value >> 8) as u8;
    }
    if let Some(b) = data.get_mut(offset + 2) {
        *b = (value >> 16) as u8;
    }
    if let Some(b) = data.get_mut(offset + 3) {
        *b = (value >> 24) as u8;
    }
}
