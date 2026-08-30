//! `<linux/genhd.h>` — Generic hard disk partition constants.
//!
//! The genhd subsystem manages disk geometry, partition tables,
//! and partition scanning. This module defines partition types
//! (MBR and GPT), geometry limits, and partitioning flags.

// ---------------------------------------------------------------------------
// MBR partition types
// ---------------------------------------------------------------------------

/// Empty partition.
pub const MBR_EMPTY: u8 = 0x00;
/// FAT12.
pub const MBR_FAT12: u8 = 0x01;
/// FAT16 (< 32 MB).
pub const MBR_FAT16_SMALL: u8 = 0x04;
/// Extended partition (CHS).
pub const MBR_EXTENDED: u8 = 0x05;
/// FAT16 (>= 32 MB).
pub const MBR_FAT16: u8 = 0x06;
/// NTFS / exFAT / HPFS.
pub const MBR_NTFS: u8 = 0x07;
/// FAT32 (CHS).
pub const MBR_FAT32: u8 = 0x0B;
/// FAT32 (LBA).
pub const MBR_FAT32_LBA: u8 = 0x0C;
/// FAT16 (LBA).
pub const MBR_FAT16_LBA: u8 = 0x0E;
/// Extended partition (LBA).
pub const MBR_EXTENDED_LBA: u8 = 0x0F;
/// Linux swap.
pub const MBR_LINUX_SWAP: u8 = 0x82;
/// Linux native.
pub const MBR_LINUX: u8 = 0x83;
/// Linux LVM.
pub const MBR_LINUX_LVM: u8 = 0x8E;
/// GPT protective MBR.
pub const MBR_GPT_PROTECTIVE: u8 = 0xEE;
/// EFI System Partition.
pub const MBR_EFI_SYSTEM: u8 = 0xEF;

// ---------------------------------------------------------------------------
// MBR constants
// ---------------------------------------------------------------------------

/// MBR signature offset.
pub const MBR_SIGNATURE_OFFSET: usize = 510;
/// MBR signature value.
pub const MBR_SIGNATURE: u16 = 0xAA55;
/// Maximum MBR partitions.
pub const MBR_MAX_PARTITIONS: u32 = 4;
/// Partition table offset.
pub const MBR_PARTITION_TABLE_OFFSET: usize = 446;
/// Partition entry size.
pub const MBR_PARTITION_ENTRY_SIZE: usize = 16;

// ---------------------------------------------------------------------------
// GPT constants
// ---------------------------------------------------------------------------

/// GPT signature ("EFI PART").
pub const GPT_SIGNATURE: u64 = 0x5452_4150_2049_4645;
/// GPT header size (minimum).
pub const GPT_HEADER_SIZE: u32 = 92;
/// GPT partition entry size (minimum).
pub const GPT_ENTRY_SIZE: u32 = 128;
/// Maximum GPT partitions (typical).
pub const GPT_MAX_PARTITIONS: u32 = 128;

// ---------------------------------------------------------------------------
// Disk geometry limits
// ---------------------------------------------------------------------------

/// Maximum sectors per track (CHS).
pub const MAX_SECTORS_PER_TRACK: u32 = 63;
/// Maximum heads (CHS).
pub const MAX_HEADS: u32 = 255;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mbr_types_distinct() {
        let types = [
            MBR_EMPTY,
            MBR_FAT12,
            MBR_FAT16_SMALL,
            MBR_EXTENDED,
            MBR_FAT16,
            MBR_NTFS,
            MBR_FAT32,
            MBR_FAT32_LBA,
            MBR_FAT16_LBA,
            MBR_EXTENDED_LBA,
            MBR_LINUX_SWAP,
            MBR_LINUX,
            MBR_LINUX_LVM,
            MBR_GPT_PROTECTIVE,
            MBR_EFI_SYSTEM,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_mbr_signature() {
        assert_eq!(MBR_SIGNATURE, 0xAA55);
        assert_eq!(MBR_SIGNATURE_OFFSET, 510);
    }

    #[test]
    fn test_mbr_partition_table() {
        assert_eq!(MBR_MAX_PARTITIONS, 4);
        assert_eq!(MBR_PARTITION_TABLE_OFFSET, 446);
        assert_eq!(MBR_PARTITION_ENTRY_SIZE, 16);
        // Table fits between offset 446 and signature at 510.
        assert_eq!(
            MBR_PARTITION_TABLE_OFFSET + MBR_MAX_PARTITIONS as usize * MBR_PARTITION_ENTRY_SIZE,
            MBR_SIGNATURE_OFFSET
        );
    }

    #[test]
    fn test_gpt_signature() {
        assert_eq!(GPT_SIGNATURE, 0x5452_4150_2049_4645);
    }

    #[test]
    fn test_gpt_sizes() {
        assert_eq!(GPT_HEADER_SIZE, 92);
        assert_eq!(GPT_ENTRY_SIZE, 128);
        assert_eq!(GPT_MAX_PARTITIONS, 128);
    }

    #[test]
    fn test_geometry_limits() {
        assert_eq!(MAX_SECTORS_PER_TRACK, 63);
        assert_eq!(MAX_HEADS, 255);
    }
}
