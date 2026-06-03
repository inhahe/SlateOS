//! GUID Partition Table — `<uapi/linux/genhd.h>` / UEFI 2.x §5.3.
//!
//! Bootloaders (GRUB, systemd-boot), partition editors (parted,
//! gdisk, KDE Partition Manager), and our installer all rely on the
//! GPT magic header layout to lay down disk images correctly.

// ---------------------------------------------------------------------------
// Magic and signatures
// ---------------------------------------------------------------------------

/// GPT header signature ("EFI PART", little-endian).
pub const GPT_HEADER_SIGNATURE: u64 = 0x5452_4150_2049_4645;
/// Revision 1.0 as encoded in `revision` field.
pub const GPT_REVISION_1_0: u32 = 0x0001_0000;

/// MBR partition type that signals "Protective MBR" (GPT present).
pub const GPT_PROTECTIVE_MBR_PARTITION_TYPE: u8 = 0xEE;

// ---------------------------------------------------------------------------
// Standard sector layout
// ---------------------------------------------------------------------------

/// LBA of the primary GPT header.
pub const GPT_PRIMARY_HEADER_LBA: u64 = 1;
/// LBA at which the primary partition entry array starts.
pub const GPT_PRIMARY_ENTRIES_LBA: u64 = 2;
/// Number of partition entries in the standard layout.
pub const GPT_NUMBER_OF_ENTRIES: u32 = 128;
/// Size of a single partition entry in bytes.
pub const GPT_PARTITION_ENTRY_SIZE: u32 = 128;
/// Size of the GPT header itself, in bytes.
pub const GPT_HEADER_SIZE: u32 = 92;

/// Maximum filename (UTF-16LE chars) for `partition_name`.
pub const GPT_PARTITION_NAME_MAX_CHARS: usize = 36;

// ---------------------------------------------------------------------------
// Partition attribute bits (struct gpt_partition_entry.attributes)
// ---------------------------------------------------------------------------

/// Required by system (do not delete).
pub const GPT_ATTR_REQUIRED_PARTITION: u64 = 1 << 0;
/// No block I/O protocol — firmware should ignore.
pub const GPT_ATTR_NO_BLOCK_IO_PROTOCOL: u64 = 1 << 1;
/// Legacy BIOS bootable.
pub const GPT_ATTR_LEGACY_BIOS_BOOTABLE: u64 = 1 << 2;
/// Microsoft: read-only.
pub const GPT_ATTR_MS_READ_ONLY: u64 = 1 << 60;
/// Microsoft: shadow copy.
pub const GPT_ATTR_MS_SHADOW: u64 = 1 << 61;
/// Microsoft: hidden.
pub const GPT_ATTR_MS_HIDDEN: u64 = 1 << 62;
/// Microsoft: no drive letter (no automount).
pub const GPT_ATTR_MS_NO_AUTOMOUNT: u64 = 1 << 63;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_header_signature_is_efi_part() {
        // The on-disk magic is "EFI PART" in ASCII, little-endian u64.
        assert_eq!(GPT_HEADER_SIGNATURE, u64::from_le_bytes(*b"EFI PART"));
    }

    #[test]
    fn test_revision_layout() {
        // Revision is BCD-style major:minor in the upper:lower 16 bits.
        assert_eq!(GPT_REVISION_1_0 >> 16, 1);
        assert_eq!(GPT_REVISION_1_0 & 0xFFFF, 0);
    }

    #[test]
    fn test_protective_mbr_type() {
        // 0xEE is the spec-mandated protective-MBR partition type.
        assert_eq!(GPT_PROTECTIVE_MBR_PARTITION_TYPE, 0xEE);
    }

    #[test]
    fn test_sector_layout() {
        // LBA 0 holds the protective MBR; LBA 1 the primary header.
        assert_eq!(GPT_PRIMARY_HEADER_LBA, 1);
        assert_eq!(GPT_PRIMARY_ENTRIES_LBA, 2);
        // 128 entries × 128 bytes = 16 KiB = 32 LBAs of 512 bytes.
        assert_eq!(GPT_NUMBER_OF_ENTRIES * GPT_PARTITION_ENTRY_SIZE, 16384);
        // Header size is fixed at 92 bytes for revision 1.0.
        assert_eq!(GPT_HEADER_SIZE, 92);
    }

    #[test]
    fn test_partition_name_size() {
        // 36 UTF-16LE characters → 72 bytes name field.
        assert_eq!(GPT_PARTITION_NAME_MAX_CHARS * 2, 72);
    }

    #[test]
    fn test_attribute_bits_distinct() {
        let a = [
            GPT_ATTR_REQUIRED_PARTITION,
            GPT_ATTR_NO_BLOCK_IO_PROTOCOL,
            GPT_ATTR_LEGACY_BIOS_BOOTABLE,
            GPT_ATTR_MS_READ_ONLY,
            GPT_ATTR_MS_SHADOW,
            GPT_ATTR_MS_HIDDEN,
            GPT_ATTR_MS_NO_AUTOMOUNT,
        ];
        for &b in &a {
            assert!(b.is_power_of_two());
        }
        for i in 0..a.len() {
            for j in (i + 1)..a.len() {
                assert_ne!(a[i], a[j]);
            }
        }
        // Microsoft bits live in the high nibble (bits 60..63).
        for &b in &[
            GPT_ATTR_MS_READ_ONLY,
            GPT_ATTR_MS_SHADOW,
            GPT_ATTR_MS_HIDDEN,
            GPT_ATTR_MS_NO_AUTOMOUNT,
        ] {
            assert!(b >= 1u64 << 60);
        }
    }
}
