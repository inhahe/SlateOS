//! `<linux/gpt.h>` — GUID Partition Table (GPT) constants.
//!
//! GPT is the standard partition table format for UEFI systems.
//! These constants define the protective MBR marker, GPT header
//! signature, partition entry sizes, and well-known partition
//! type GUIDs encoded as `(u32, u16, u16, [u8; 8])` tuples
//! matching the mixed-endian UUID on-disk layout.

// ---------------------------------------------------------------------------
// GPT header constants
// ---------------------------------------------------------------------------

/// GPT header signature: "EFI PART" as little-endian u64.
pub const GPT_HEADER_SIGNATURE: u64 = 0x5452_4150_2049_4645;
/// GPT header revision 1.0.
pub const GPT_HEADER_REVISION_V1: u32 = 0x0001_0000;
/// Minimum GPT header size in bytes.
pub const GPT_HEADER_SIZE_MIN: u32 = 92;
/// Standard number of partition entries.
pub const GPT_ENTRY_COUNT_DEFAULT: u32 = 128;
/// Standard partition entry size in bytes.
pub const GPT_ENTRY_SIZE: u32 = 128;

// ---------------------------------------------------------------------------
// Protective MBR constants
// ---------------------------------------------------------------------------

/// MBR partition type for GPT protective partition.
pub const GPT_PROTECTIVE_MBR_TYPE: u8 = 0xEE;
/// MBR boot signature (little-endian).
pub const MBR_SIGNATURE: u16 = 0xAA55;

// ---------------------------------------------------------------------------
// GPT partition attribute flags (bit positions)
// ---------------------------------------------------------------------------

/// Required for platform to function.
pub const GPT_ATTR_PLATFORM_REQUIRED: u64 = 1 << 0;
/// EFI firmware should ignore this partition.
pub const GPT_ATTR_EFI_IGNORE: u64 = 1 << 1;
/// Legacy BIOS bootable flag.
pub const GPT_ATTR_LEGACY_BIOS_BOOTABLE: u64 = 1 << 2;

// ---------------------------------------------------------------------------
// Well-known partition type GUIDs — (data1, data2, data3, data4[8])
//
// Each GUID is stored in mixed-endian format as defined by RFC 4122:
// data1 (u32 LE), data2 (u16 LE), data3 (u16 LE), data4 ([u8] BE).
// ---------------------------------------------------------------------------

/// Unused / empty entry: 00000000-0000-0000-0000-000000000000
pub const GPT_TYPE_UNUSED: (u32, u16, u16, [u8; 8]) = (
    0x0000_0000,
    0x0000,
    0x0000,
    [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
);

/// EFI System Partition: C12A7328-F81F-11D2-BA4B-00A0C93EC93B
pub const GPT_TYPE_EFI_SYSTEM: (u32, u16, u16, [u8; 8]) = (
    0xC12A_7328,
    0xF81F,
    0x11D2,
    [0xBA, 0x4B, 0x00, 0xA0, 0xC9, 0x3E, 0xC9, 0x3B],
);

/// Microsoft Basic Data: EBD0A0A2-B9E5-4433-87C0-68B6B72699C7
pub const GPT_TYPE_MS_BASIC_DATA: (u32, u16, u16, [u8; 8]) = (
    0xEBD0_A0A2,
    0xB9E5,
    0x4433,
    [0x87, 0xC0, 0x68, 0xB6, 0xB7, 0x26, 0x99, 0xC7],
);

/// Linux Filesystem Data: 0FC63DAF-8483-4772-8E79-3D69D8477DE4
pub const GPT_TYPE_LINUX_FS: (u32, u16, u16, [u8; 8]) = (
    0x0FC6_3DAF,
    0x8483,
    0x4772,
    [0x8E, 0x79, 0x3D, 0x69, 0xD8, 0x47, 0x7D, 0xE4],
);

/// Linux Swap: 0657FD6D-A4AB-43C4-84E5-0933C84B4F4F
pub const GPT_TYPE_LINUX_SWAP: (u32, u16, u16, [u8; 8]) = (
    0x0657_FD6D,
    0xA4AB,
    0x43C4,
    [0x84, 0xE5, 0x09, 0x33, 0xC8, 0x4B, 0x4F, 0x4F],
);

/// Linux LVM: E6D6D379-F507-44C2-A23C-238F2A3DF928
pub const GPT_TYPE_LINUX_LVM: (u32, u16, u16, [u8; 8]) = (
    0xE6D6_D379,
    0xF507,
    0x44C2,
    [0xA2, 0x3C, 0x23, 0x8F, 0x2A, 0x3D, 0xF9, 0x28],
);

/// Linux RAID: A19D880F-05FC-4D3B-A006-743F0F84911E
pub const GPT_TYPE_LINUX_RAID: (u32, u16, u16, [u8; 8]) = (
    0xA19D_880F,
    0x05FC,
    0x4D3B,
    [0xA0, 0x06, 0x74, 0x3F, 0x0F, 0x84, 0x91, 0x1E],
);

/// Linux Home: 933AC7E1-2EB4-4F13-B844-0E14E2AEF915
pub const GPT_TYPE_LINUX_HOME: (u32, u16, u16, [u8; 8]) = (
    0x933A_C7E1,
    0x2EB4,
    0x4F13,
    [0xB8, 0x44, 0x0E, 0x14, 0xE2, 0xAE, 0xF9, 0x15],
);

/// Linux Root (x86-64): 4F68BCE3-E8CD-4DB1-96E7-FBCAF984B709
pub const GPT_TYPE_LINUX_ROOT_X86_64: (u32, u16, u16, [u8; 8]) = (
    0x4F68_BCE3,
    0xE8CD,
    0x4DB1,
    [0x96, 0xE7, 0xFB, 0xCA, 0xF9, 0x84, 0xB7, 0x09],
);

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_header_signature() {
        assert_eq!(GPT_HEADER_SIGNATURE, 0x5452_4150_2049_4645);
    }

    #[test]
    fn test_header_revision() {
        assert_eq!(GPT_HEADER_REVISION_V1, 0x0001_0000);
    }

    #[test]
    fn test_header_size() {
        assert_eq!(GPT_HEADER_SIZE_MIN, 92);
    }

    #[test]
    fn test_entry_defaults() {
        assert_eq!(GPT_ENTRY_COUNT_DEFAULT, 128);
        assert_eq!(GPT_ENTRY_SIZE, 128);
    }

    #[test]
    fn test_protective_mbr() {
        assert_eq!(GPT_PROTECTIVE_MBR_TYPE, 0xEE);
        assert_eq!(MBR_SIGNATURE, 0xAA55);
    }

    #[test]
    fn test_attr_flags_no_overlap() {
        let flags = [
            GPT_ATTR_PLATFORM_REQUIRED,
            GPT_ATTR_EFI_IGNORE,
            GPT_ATTR_LEGACY_BIOS_BOOTABLE,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_attr_flags_power_of_two() {
        assert!(GPT_ATTR_PLATFORM_REQUIRED.is_power_of_two());
        assert!(GPT_ATTR_EFI_IGNORE.is_power_of_two());
        assert!(GPT_ATTR_LEGACY_BIOS_BOOTABLE.is_power_of_two());
    }

    #[test]
    fn test_unused_guid_is_zero() {
        let (d1, d2, d3, d4) = GPT_TYPE_UNUSED;
        assert_eq!(d1, 0);
        assert_eq!(d2, 0);
        assert_eq!(d3, 0);
        assert_eq!(d4, [0u8; 8]);
    }

    #[test]
    fn test_efi_system_guid() {
        let (d1, _d2, _d3, _d4) = GPT_TYPE_EFI_SYSTEM;
        assert_eq!(d1, 0xC12A_7328);
    }

    #[test]
    fn test_linux_fs_guid() {
        let (d1, _d2, _d3, _d4) = GPT_TYPE_LINUX_FS;
        assert_eq!(d1, 0x0FC6_3DAF);
    }

    #[test]
    fn test_type_guids_distinct() {
        let guids = [
            GPT_TYPE_UNUSED,
            GPT_TYPE_EFI_SYSTEM,
            GPT_TYPE_MS_BASIC_DATA,
            GPT_TYPE_LINUX_FS,
            GPT_TYPE_LINUX_SWAP,
            GPT_TYPE_LINUX_LVM,
            GPT_TYPE_LINUX_RAID,
            GPT_TYPE_LINUX_HOME,
            GPT_TYPE_LINUX_ROOT_X86_64,
        ];
        for i in 0..guids.len() {
            for j in (i + 1)..guids.len() {
                assert_ne!(guids[i], guids[j]);
            }
        }
    }
}
