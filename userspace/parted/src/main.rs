//! OurOS Disk Partition Editor
//!
//! Multi-personality binary that acts as `parted`, `partprobe`, or `partx`
//! depending on the name used to invoke it (detected via `argv[0]`).
//!
//! # Personalities
//!
//! - **parted**: GNU parted-compatible partition editor
//! - **partprobe**: inform OS of partition table changes
//! - **partx**: tell kernel about disk partitions
//!
//! # Usage
//!
//! ```text
//! parted /dev/sda print          # show partition table
//! parted /dev/sda mklabel gpt    # create GPT partition table
//! parted /dev/sda mkpart primary ext4 1MiB 100%
//! partprobe /dev/sda             # inform OS of changes
//! partx --show /dev/sda          # show partitions
//! ```

#![deny(clippy::all)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_lossless)]
#![allow(clippy::cast_possible_wrap)]
#![allow(clippy::too_many_lines)]
#![allow(clippy::similar_names)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::needless_range_loop)]
#![allow(dead_code)]

use std::env;
use std::fmt;
use std::process;

// ============================================================================
// Constants
// ============================================================================

const SECTOR_SIZE: u64 = 512;
const GPT_SIGNATURE: u64 = 0x5452_4150_2049_4645; // "EFI PART"
const GPT_REVISION_1_0: u32 = 0x0001_0000;
const GPT_HEADER_SIZE: u32 = 92;
const GPT_ENTRY_SIZE: u32 = 128;
const GPT_MAX_ENTRIES: u32 = 128;
const MBR_SIGNATURE: u16 = 0xAA55;
const MBR_SIZE: usize = 512;
const MBR_PARTITION_OFFSET: usize = 446;
const MBR_PARTITION_ENTRY_SIZE: usize = 16;
const MBR_MAX_PRIMARY: usize = 4;

const KB: u64 = 1000;
const MB: u64 = 1_000_000;
const GB: u64 = 1_000_000_000;
const TB: u64 = 1_000_000_000_000;
const PB: u64 = 1_000_000_000_000_000;
const KIB: u64 = 1024;
const MIB: u64 = 1_048_576;
const GIB: u64 = 1_073_741_824;
const TIB: u64 = 1_099_511_627_776;

// Default alignment: 1 MiB (2048 sectors of 512 bytes)
const DEFAULT_ALIGNMENT_SECTORS: u64 = 2048;

// ============================================================================
// CRC32 (ISO 3309 / ITU-T V.42)
// ============================================================================

const CRC32_TABLE: [u32; 256] = generate_crc32_table();

const fn generate_crc32_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut i = 0u32;
    while i < 256 {
        let mut crc = i;
        let mut j = 0;
        while j < 8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xEDB8_8320;
            } else {
                crc >>= 1;
            }
            j += 1;
        }
        table[i as usize] = crc;
        i += 1;
    }
    table
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in data {
        let index = ((crc ^ byte as u32) & 0xFF) as usize;
        crc = (crc >> 8) ^ CRC32_TABLE[index];
    }
    !crc
}

// ============================================================================
// GUID (128-bit UUID)
// ============================================================================

#[derive(Clone, Copy, PartialEq, Eq, Default)]
struct Guid {
    data1: u32,
    data2: u16,
    data3: u16,
    data4: [u8; 8],
}

impl Guid {
    const ZERO: Guid = Guid {
        data1: 0,
        data2: 0,
        data3: 0,
        data4: [0; 8],
    };

    fn from_bytes_le(bytes: &[u8]) -> Option<Guid> {
        if bytes.len() < 16 {
            return None;
        }
        Some(Guid {
            data1: u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
            data2: u16::from_le_bytes([bytes[4], bytes[5]]),
            data3: u16::from_le_bytes([bytes[6], bytes[7]]),
            data4: [
                bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14],
                bytes[15],
            ],
        })
    }

    fn to_bytes_le(self) -> [u8; 16] {
        let mut buf = [0u8; 16];
        buf[0..4].copy_from_slice(&self.data1.to_le_bytes());
        buf[4..6].copy_from_slice(&self.data2.to_le_bytes());
        buf[6..8].copy_from_slice(&self.data3.to_le_bytes());
        buf[8..16].copy_from_slice(&self.data4);
        buf
    }

    fn is_zero(&self) -> bool {
        *self == Self::ZERO
    }

    /// Parse a GUID from string like "C12A7328-F81F-11D2-BA4B-00A0C93EC93B"
    fn from_str_hex(s: &str) -> Option<Guid> {
        let clean: String = s.chars().filter(|c| *c != '-').collect();
        if clean.len() != 32 {
            return None;
        }
        let bytes: Vec<u8> = (0..16)
            .filter_map(|i| u8::from_str_radix(&clean[i * 2..i * 2 + 2], 16).ok())
            .collect();
        if bytes.len() != 16 {
            return None;
        }
        // Mixed-endian: first 3 fields are LE, last 2 are BE
        Some(Guid {
            data1: u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
            data2: u16::from_be_bytes([bytes[4], bytes[5]]),
            data3: u16::from_be_bytes([bytes[6], bytes[7]]),
            data4: [
                bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14],
                bytes[15],
            ],
        })
    }

    /// Generate a pseudo-random GUID (v4-like) from a seed
    fn generate(seed: u64) -> Guid {
        // Simple PRNG for deterministic GUID generation
        let mut state = seed;
        let mut bytes = [0u8; 16];
        for b in &mut bytes {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            *b = (state >> 33) as u8;
        }
        // Set version 4 and variant bits
        bytes[6] = (bytes[6] & 0x0F) | 0x40;
        bytes[8] = (bytes[8] & 0x3F) | 0x80;
        Guid::from_bytes_le(&bytes).unwrap_or(Guid::ZERO)
    }
}

impl fmt::Display for Guid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:08X}-{:04X}-{:04X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}",
            self.data1,
            self.data2,
            self.data3,
            self.data4[0],
            self.data4[1],
            self.data4[2],
            self.data4[3],
            self.data4[4],
            self.data4[5],
            self.data4[6],
            self.data4[7],
        )
    }
}

impl fmt::Debug for Guid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Guid({})", self)
    }
}

// ============================================================================
// Well-known GUIDs
// ============================================================================

struct GuidEntry {
    guid: Guid,
    name: &'static str,
}

fn well_known_guids() -> Vec<GuidEntry> {
    let parse = |s: &str| -> Guid { Guid::from_str_hex(s).unwrap_or(Guid::ZERO) };
    vec![
        GuidEntry {
            guid: parse("C12A7328-F81F-11D2-BA4B-00A0C93EC93B"),
            name: "EFI System",
        },
        GuidEntry {
            guid: parse("21686148-6449-6E6F-744E-656564454649"),
            name: "BIOS boot",
        },
        GuidEntry {
            guid: parse("024DEE41-33E7-11D3-9D69-0008C781F39F"),
            name: "MBR partition scheme",
        },
        GuidEntry {
            guid: parse("0FC63DAF-8483-4772-8E79-3D69D8477DE4"),
            name: "Linux filesystem",
        },
        GuidEntry {
            guid: parse("0657FD6D-A4AB-43C4-84E5-0933C84B4F4F"),
            name: "Linux swap",
        },
        GuidEntry {
            guid: parse("E6D6D379-F507-44C2-A23C-238F2A3DF928"),
            name: "Linux LVM",
        },
        GuidEntry {
            guid: parse("A19D880F-05FC-4D3B-A006-743F0F84911E"),
            name: "Linux RAID",
        },
        GuidEntry {
            guid: parse("933AC7E1-2EB4-4F13-B844-0E14E2AEF915"),
            name: "Linux home",
        },
        GuidEntry {
            guid: parse("EBD0A0A2-B9E5-4433-87C0-68B6B72699C7"),
            name: "Windows basic data",
        },
        GuidEntry {
            guid: parse("E3C9E316-0B5C-4DB8-817D-F92DF00215AE"),
            name: "Microsoft reserved",
        },
        GuidEntry {
            guid: parse("DE94BBA4-06D1-4D40-A16A-BFD50179D6AC"),
            name: "Windows recovery",
        },
        GuidEntry {
            guid: parse("48465300-0000-11AA-AA11-00306543ECAC"),
            name: "Apple HFS+",
        },
        GuidEntry {
            guid: parse("7C3457EF-0000-11AA-AA11-00306543ECAC"),
            name: "Apple APFS",
        },
        GuidEntry {
            guid: parse("516E7CB4-6ECF-11D6-8FF8-00022D09712B"),
            name: "FreeBSD UFS",
        },
        GuidEntry {
            guid: parse("516E7CB5-6ECF-11D6-8FF8-00022D09712B"),
            name: "FreeBSD swap",
        },
        GuidEntry {
            guid: parse("516E7CB6-6ECF-11D6-8FF8-00022D09712B"),
            name: "FreeBSD ZFS",
        },
    ]
}

fn guid_to_name(guid: &Guid) -> &'static str {
    for entry in &*WELL_KNOWN_GUIDS {
        if entry.guid == *guid {
            return entry.name;
        }
    }
    "unknown"
}

fn name_to_guid(name: &str) -> Option<Guid> {
    let lower = name.to_lowercase();
    for entry in &*WELL_KNOWN_GUIDS {
        if entry.name.to_lowercase() == lower {
            return Some(entry.guid);
        }
    }
    // Common aliases
    match lower.as_str() {
        "fat32" | "ntfs" | "basic data" | "msftdata" => {
            Guid::from_str_hex("EBD0A0A2-B9E5-4433-87C0-68B6B72699C7")
        }
        "efi" | "esp" | "efi system" => {
            Guid::from_str_hex("C12A7328-F81F-11D2-BA4B-00A0C93EC93B")
        }
        "linux" | "ext4" | "ext3" | "ext2" | "xfs" | "btrfs" => {
            Guid::from_str_hex("0FC63DAF-8483-4772-8E79-3D69D8477DE4")
        }
        "swap" | "linux-swap" | "linux swap" => {
            Guid::from_str_hex("0657FD6D-A4AB-43C4-84E5-0933C84B4F4F")
        }
        "lvm" => Guid::from_str_hex("E6D6D379-F507-44C2-A23C-238F2A3DF928"),
        "raid" => Guid::from_str_hex("A19D880F-05FC-4D3B-A006-743F0F84911E"),
        "bios_grub" | "bios boot" => {
            Guid::from_str_hex("21686148-6449-6E6F-744E-656564454649")
        }
        "msftres" | "microsoft reserved" => {
            Guid::from_str_hex("E3C9E316-0B5C-4DB8-817D-F92DF00215AE")
        }
        _ => None,
    }
}

// Lazy-init well-known GUID table
struct LazyGuids {
    inner: std::sync::OnceLock<Vec<GuidEntry>>,
}
impl std::ops::Deref for LazyGuids {
    type Target = Vec<GuidEntry>;
    fn deref(&self) -> &Vec<GuidEntry> {
        self.inner.get_or_init(well_known_guids)
    }
}
static WELL_KNOWN_GUIDS: LazyGuids = LazyGuids {
    inner: std::sync::OnceLock::new(),
};

// ============================================================================
// MBR partition types
// ============================================================================

struct MbrTypeEntry {
    code: u8,
    name: &'static str,
}

const MBR_TYPES: &[MbrTypeEntry] = &[
    MbrTypeEntry { code: 0x00, name: "Empty" },
    MbrTypeEntry { code: 0x01, name: "FAT12" },
    MbrTypeEntry { code: 0x04, name: "FAT16 <32M" },
    MbrTypeEntry { code: 0x05, name: "Extended" },
    MbrTypeEntry { code: 0x06, name: "FAT16" },
    MbrTypeEntry { code: 0x07, name: "HPFS/NTFS" },
    MbrTypeEntry { code: 0x0B, name: "W95 FAT32" },
    MbrTypeEntry { code: 0x0C, name: "W95 FAT32 (LBA)" },
    MbrTypeEntry { code: 0x0E, name: "W95 FAT16 (LBA)" },
    MbrTypeEntry { code: 0x0F, name: "W95 Ext'd (LBA)" },
    MbrTypeEntry { code: 0x11, name: "Hidden FAT12" },
    MbrTypeEntry { code: 0x14, name: "Hidden FAT16 <32M" },
    MbrTypeEntry { code: 0x16, name: "Hidden FAT16" },
    MbrTypeEntry { code: 0x17, name: "Hidden HPFS/NTFS" },
    MbrTypeEntry { code: 0x1B, name: "Hidden W95 FAT32" },
    MbrTypeEntry { code: 0x1C, name: "Hidden W95 FAT32 (LBA)" },
    MbrTypeEntry { code: 0x1E, name: "Hidden W95 FAT16 (LBA)" },
    MbrTypeEntry { code: 0x27, name: "Hidden NTFS WinRE" },
    MbrTypeEntry { code: 0x39, name: "Plan 9" },
    MbrTypeEntry { code: 0x3C, name: "PartitionMagic" },
    MbrTypeEntry { code: 0x42, name: "SFS / LDM" },
    MbrTypeEntry { code: 0x7F, name: "Chromium OS kernel" },
    MbrTypeEntry { code: 0x82, name: "Linux swap" },
    MbrTypeEntry { code: 0x83, name: "Linux" },
    MbrTypeEntry { code: 0x85, name: "Linux extended" },
    MbrTypeEntry { code: 0x8E, name: "Linux LVM" },
    MbrTypeEntry { code: 0xA5, name: "FreeBSD" },
    MbrTypeEntry { code: 0xA6, name: "OpenBSD" },
    MbrTypeEntry { code: 0xA8, name: "Darwin UFS" },
    MbrTypeEntry { code: 0xAB, name: "Darwin boot" },
    MbrTypeEntry { code: 0xAF, name: "Apple HFS+" },
    MbrTypeEntry { code: 0xBE, name: "Solaris boot" },
    MbrTypeEntry { code: 0xBF, name: "Solaris" },
    MbrTypeEntry { code: 0xEE, name: "GPT protective" },
    MbrTypeEntry { code: 0xEF, name: "EFI System" },
    MbrTypeEntry { code: 0xFB, name: "VMware VMFS" },
    MbrTypeEntry { code: 0xFC, name: "VMware swap" },
    MbrTypeEntry { code: 0xFD, name: "Linux RAID" },
];

fn mbr_type_name(code: u8) -> &'static str {
    for entry in MBR_TYPES {
        if entry.code == code {
            return entry.name;
        }
    }
    "Unknown"
}

fn mbr_type_code(name: &str) -> Option<u8> {
    let lower = name.to_lowercase();
    // Try hex code first
    if let Some(hex) = lower.strip_prefix("0x") {
        return u8::from_str_radix(hex, 16).ok();
    }
    // Common aliases
    match lower.as_str() {
        "linux" | "ext4" | "ext3" | "ext2" | "xfs" | "btrfs" => Some(0x83),
        "swap" | "linux-swap" | "linux swap" => Some(0x82),
        "fat32" => Some(0x0C),
        "fat16" => Some(0x06),
        "ntfs" | "hpfs/ntfs" => Some(0x07),
        "extended" | "ext" => Some(0x05),
        "efi" | "efi system" | "esp" => Some(0xEF),
        "lvm" | "linux lvm" => Some(0x8E),
        "raid" | "linux raid" => Some(0xFD),
        _ => {
            for entry in MBR_TYPES {
                if entry.name.to_lowercase() == lower {
                    return Some(entry.code);
                }
            }
            None
        }
    }
}

// ============================================================================
// Display units
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DisplayUnit {
    Sectors,
    Bytes,
    KB,
    MB,
    GB,
    TB,
    Percent,
    Compact,
}

impl DisplayUnit {
    fn from_str(s: &str) -> Option<DisplayUnit> {
        match s.to_lowercase().as_str() {
            "s" | "sectors" => Some(DisplayUnit::Sectors),
            "b" | "bytes" => Some(DisplayUnit::Bytes),
            "kb" => Some(DisplayUnit::KB),
            "mb" => Some(DisplayUnit::MB),
            "gb" => Some(DisplayUnit::GB),
            "tb" => Some(DisplayUnit::TB),
            "%" | "percent" => Some(DisplayUnit::Percent),
            "compact" | "cmp" => Some(DisplayUnit::Compact),
            _ => None,
        }
    }

    fn suffix(&self) -> &'static str {
        match self {
            DisplayUnit::Sectors => "s",
            DisplayUnit::Bytes => "B",
            DisplayUnit::KB => "kB",
            DisplayUnit::MB => "MB",
            DisplayUnit::GB => "GB",
            DisplayUnit::TB => "TB",
            DisplayUnit::Percent => "%",
            DisplayUnit::Compact => "",
        }
    }
}

// ============================================================================
// Size parsing
// ============================================================================

/// Parse a size string into bytes. Supports suffixes: s, B, kB, KB, KiB, MB, MiB,
/// GB, GiB, TB, TiB, PB, PiB, and % (requires disk_bytes for percentage).
fn parse_size(s: &str, sector_size: u64, disk_bytes: u64) -> Option<u64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }

    // Handle percentage
    if s.ends_with('%') {
        let num_str = s.trim_end_matches('%').trim();
        let pct: f64 = num_str.parse().ok()?;
        if !(0.0..=100.0).contains(&pct) {
            return None;
        }
        return Some((disk_bytes as f64 * pct / 100.0) as u64);
    }

    // Find where the numeric part ends
    let mut num_end = s.len();
    for (i, c) in s.char_indices().rev() {
        if c.is_ascii_digit() || c == '.' {
            num_end = i + c.len_utf8();
            break;
        }
    }

    // Handle the case where it's all digits (no suffix)
    let has_suffix = num_end < s.len();
    let (num_str, suffix) = if has_suffix {
        // Find the actual boundary
        let mut boundary = 0;
        for (i, c) in s.char_indices() {
            if !c.is_ascii_digit() && c != '.' && c != '-' {
                boundary = i;
                break;
            }
            boundary = i + c.len_utf8();
        }
        (&s[..boundary], s[boundary..].trim())
    } else {
        (s, "")
    };

    let value: f64 = num_str.parse().ok()?;

    let multiplier: u64 = match suffix.to_lowercase().as_str() {
        "" | "b" => 1,
        "s" => sector_size,
        "kb" | "k" => KB,
        "kib" => KIB,
        "mb" | "m" => MB,
        "mib" => MIB,
        "gb" | "g" => GB,
        "gib" => GIB,
        "tb" | "t" => TB,
        "tib" => TIB,
        "pb" | "p" => PB,
        "pib" => PB * 1024 / 1000, // approximate
        _ => return None,
    };

    Some((value * multiplier as f64) as u64)
}

/// Format bytes in the specified unit
fn format_size(bytes: u64, unit: DisplayUnit, disk_bytes: u64, sector_size: u64) -> String {
    match unit {
        DisplayUnit::Sectors => format!("{}s", bytes / sector_size),
        DisplayUnit::Bytes => format!("{}B", bytes),
        DisplayUnit::KB => format!("{:.1}kB", bytes as f64 / KB as f64),
        DisplayUnit::MB => format!("{:.1}MB", bytes as f64 / MB as f64),
        DisplayUnit::GB => format!("{:.2}GB", bytes as f64 / GB as f64),
        DisplayUnit::TB => format!("{:.3}TB", bytes as f64 / TB as f64),
        DisplayUnit::Percent => {
            if disk_bytes == 0 {
                "0%".to_string()
            } else {
                format!("{:.1}%", bytes as f64 / disk_bytes as f64 * 100.0)
            }
        }
        DisplayUnit::Compact => format_compact(bytes),
    }
}

fn format_compact(bytes: u64) -> String {
    if bytes < KB {
        format!("{}B", bytes)
    } else if bytes < MB {
        format!("{:.1}kB", bytes as f64 / KB as f64)
    } else if bytes < GB {
        format!("{:.1}MB", bytes as f64 / MB as f64)
    } else if bytes < TB {
        format!("{:.2}GB", bytes as f64 / GB as f64)
    } else {
        format!("{:.3}TB", bytes as f64 / TB as f64)
    }
}

/// Convert bytes to sectors (round up)
fn bytes_to_sectors_ceil(bytes: u64, sector_size: u64) -> u64 {
    bytes.div_ceil(sector_size)
}

/// Convert bytes to sectors (round down)
fn bytes_to_sectors_floor(bytes: u64, sector_size: u64) -> u64 {
    bytes / sector_size
}

/// Align a sector number up to the given alignment
fn align_up(sector: u64, alignment: u64) -> u64 {
    if alignment == 0 {
        return sector;
    }
    let remainder = sector % alignment;
    if remainder == 0 {
        sector
    } else {
        sector + alignment - remainder
    }
}

/// Align a sector number down to the given alignment
fn align_down(sector: u64, alignment: u64) -> u64 {
    if alignment == 0 {
        return sector;
    }
    sector - (sector % alignment)
}

// ============================================================================
// Partition flags
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PartitionFlag {
    Boot,
    Esp,
    Swap,
    Raid,
    Lvm,
    Hidden,
    MsftData,
    MsftRes,
    BiosGrub,
    Diag,
    PrepBoot,
    Irst,
    ChromeosKernel,
}

impl PartitionFlag {
    fn from_str(s: &str) -> Option<PartitionFlag> {
        match s.to_lowercase().as_str() {
            "boot" => Some(PartitionFlag::Boot),
            "esp" => Some(PartitionFlag::Esp),
            "swap" => Some(PartitionFlag::Swap),
            "raid" => Some(PartitionFlag::Raid),
            "lvm" => Some(PartitionFlag::Lvm),
            "hidden" => Some(PartitionFlag::Hidden),
            "msftdata" => Some(PartitionFlag::MsftData),
            "msftres" => Some(PartitionFlag::MsftRes),
            "bios_grub" => Some(PartitionFlag::BiosGrub),
            "diag" => Some(PartitionFlag::Diag),
            "prep" | "prep_boot" => Some(PartitionFlag::PrepBoot),
            "irst" => Some(PartitionFlag::Irst),
            "chromeos_kernel" => Some(PartitionFlag::ChromeosKernel),
            _ => None,
        }
    }

    fn name(&self) -> &'static str {
        match self {
            PartitionFlag::Boot => "boot",
            PartitionFlag::Esp => "esp",
            PartitionFlag::Swap => "swap",
            PartitionFlag::Raid => "raid",
            PartitionFlag::Lvm => "lvm",
            PartitionFlag::Hidden => "hidden",
            PartitionFlag::MsftData => "msftdata",
            PartitionFlag::MsftRes => "msftres",
            PartitionFlag::BiosGrub => "bios_grub",
            PartitionFlag::Diag => "diag",
            PartitionFlag::PrepBoot => "prep",
            PartitionFlag::Irst => "irst",
            PartitionFlag::ChromeosKernel => "chromeos_kernel",
        }
    }

    fn all() -> &'static [PartitionFlag] {
        &[
            PartitionFlag::Boot,
            PartitionFlag::Esp,
            PartitionFlag::Swap,
            PartitionFlag::Raid,
            PartitionFlag::Lvm,
            PartitionFlag::Hidden,
            PartitionFlag::MsftData,
            PartitionFlag::MsftRes,
            PartitionFlag::BiosGrub,
            PartitionFlag::Diag,
            PartitionFlag::PrepBoot,
            PartitionFlag::Irst,
            PartitionFlag::ChromeosKernel,
        ]
    }

    /// GPT attribute bit for this flag
    fn gpt_attribute_bit(&self) -> Option<u64> {
        match self {
            PartitionFlag::Boot => Some(1 << 2),  // Legacy BIOS bootable
            PartitionFlag::Esp => None,            // Use type GUID instead
            PartitionFlag::Hidden => Some(1 << 62),
            PartitionFlag::MsftData => None,       // Type GUID
            PartitionFlag::MsftRes => None,        // Type GUID
            PartitionFlag::BiosGrub => None,       // Type GUID
            PartitionFlag::Swap => None,           // Type GUID
            PartitionFlag::Raid => None,           // Type GUID
            PartitionFlag::Lvm => None,            // Type GUID
            _ => Some(0),                          // Not applicable
        }
    }

    /// MBR status/type modification for this flag
    fn mbr_boot_flag(&self) -> bool {
        matches!(self, PartitionFlag::Boot)
    }
}

// ============================================================================
// GPT structures
// ============================================================================

#[derive(Clone, Debug)]
struct GptHeader {
    revision: u32,
    header_size: u32,
    header_crc32: u32,
    my_lba: u64,
    alternate_lba: u64,
    first_usable_lba: u64,
    last_usable_lba: u64,
    disk_guid: Guid,
    partition_entry_lba: u64,
    num_partition_entries: u32,
    partition_entry_size: u32,
    partition_entry_crc32: u32,
}

impl GptHeader {
    fn new(disk_sectors: u64) -> GptHeader {
        let entries_sectors =
            (GPT_MAX_ENTRIES as u64 * GPT_ENTRY_SIZE as u64).div_ceil(SECTOR_SIZE);
        let first_usable = 2 + entries_sectors; // LBA 0: PMBR, LBA 1: GPT header, then entries
        let last_usable = disk_sectors - 1 - 1 - entries_sectors; // backup header + backup entries
        GptHeader {
            revision: GPT_REVISION_1_0,
            header_size: GPT_HEADER_SIZE,
            header_crc32: 0,
            my_lba: 1,
            alternate_lba: disk_sectors - 1,
            first_usable_lba: first_usable,
            last_usable_lba: last_usable,
            disk_guid: Guid::generate(0x1234_5678),
            partition_entry_lba: 2,
            num_partition_entries: GPT_MAX_ENTRIES,
            partition_entry_size: GPT_ENTRY_SIZE,
            partition_entry_crc32: 0,
        }
    }

    fn serialize(&self) -> Vec<u8> {
        let mut buf = vec![0u8; self.header_size as usize];
        // Signature
        buf[0..8].copy_from_slice(&GPT_SIGNATURE.to_le_bytes());
        // Revision
        buf[8..12].copy_from_slice(&self.revision.to_le_bytes());
        // Header size
        buf[12..16].copy_from_slice(&self.header_size.to_le_bytes());
        // CRC32 (set to 0 during calculation)
        buf[16..20].copy_from_slice(&0u32.to_le_bytes());
        // Reserved
        buf[20..24].copy_from_slice(&0u32.to_le_bytes());
        // My LBA
        buf[24..32].copy_from_slice(&self.my_lba.to_le_bytes());
        // Alternate LBA
        buf[32..40].copy_from_slice(&self.alternate_lba.to_le_bytes());
        // First usable LBA
        buf[40..48].copy_from_slice(&self.first_usable_lba.to_le_bytes());
        // Last usable LBA
        buf[48..56].copy_from_slice(&self.last_usable_lba.to_le_bytes());
        // Disk GUID
        buf[56..72].copy_from_slice(&self.disk_guid.to_bytes_le());
        // Partition entry LBA
        buf[72..80].copy_from_slice(&self.partition_entry_lba.to_le_bytes());
        // Number of partition entries
        buf[80..84].copy_from_slice(&self.num_partition_entries.to_le_bytes());
        // Size of partition entry
        buf[84..88].copy_from_slice(&self.partition_entry_size.to_le_bytes());
        // Partition entry CRC32
        buf[88..92].copy_from_slice(&self.partition_entry_crc32.to_le_bytes());
        buf
    }

    fn serialize_with_crc(&self) -> Vec<u8> {
        let mut buf = self.serialize();
        let crc = crc32(&buf);
        buf[16..20].copy_from_slice(&crc.to_le_bytes());
        buf
    }

    fn parse(data: &[u8]) -> Option<GptHeader> {
        if data.len() < GPT_HEADER_SIZE as usize {
            return None;
        }
        let sig = u64::from_le_bytes([
            data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
        ]);
        if sig != GPT_SIGNATURE {
            return None;
        }

        let header_crc_stored = u32::from_le_bytes([data[16], data[17], data[18], data[19]]);
        // Verify CRC: zero out the CRC field and recalculate
        let header_size =
            u32::from_le_bytes([data[12], data[13], data[14], data[15]]) as usize;
        if header_size > data.len() || header_size < GPT_HEADER_SIZE as usize {
            return None;
        }
        let mut check_buf = data[..header_size].to_vec();
        check_buf[16..20].copy_from_slice(&0u32.to_le_bytes());
        let computed_crc = crc32(&check_buf);
        if computed_crc != header_crc_stored {
            return None;
        }

        Some(GptHeader {
            revision: u32::from_le_bytes([data[8], data[9], data[10], data[11]]),
            header_size: header_size as u32,
            header_crc32: header_crc_stored,
            my_lba: u64::from_le_bytes([
                data[24], data[25], data[26], data[27], data[28], data[29], data[30], data[31],
            ]),
            alternate_lba: u64::from_le_bytes([
                data[32], data[33], data[34], data[35], data[36], data[37], data[38], data[39],
            ]),
            first_usable_lba: u64::from_le_bytes([
                data[40], data[41], data[42], data[43], data[44], data[45], data[46], data[47],
            ]),
            last_usable_lba: u64::from_le_bytes([
                data[48], data[49], data[50], data[51], data[52], data[53], data[54], data[55],
            ]),
            disk_guid: Guid::from_bytes_le(&data[56..72]).unwrap_or(Guid::ZERO),
            partition_entry_lba: u64::from_le_bytes([
                data[72], data[73], data[74], data[75], data[76], data[77], data[78], data[79],
            ]),
            num_partition_entries: u32::from_le_bytes([data[80], data[81], data[82], data[83]]),
            partition_entry_size: u32::from_le_bytes([data[84], data[85], data[86], data[87]]),
            partition_entry_crc32: u32::from_le_bytes([data[88], data[89], data[90], data[91]]),
        })
    }
}

#[derive(Clone, Debug)]
struct GptEntry {
    type_guid: Guid,
    unique_guid: Guid,
    first_lba: u64,
    last_lba: u64,
    attributes: u64,
    name: String,
}

impl GptEntry {
    fn new() -> GptEntry {
        GptEntry {
            type_guid: Guid::ZERO,
            unique_guid: Guid::ZERO,
            first_lba: 0,
            last_lba: 0,
            attributes: 0,
            name: String::new(),
        }
    }

    fn is_empty(&self) -> bool {
        self.type_guid.is_zero()
    }

    fn size_sectors(&self) -> u64 {
        if self.is_empty() || self.last_lba < self.first_lba {
            0
        } else {
            self.last_lba - self.first_lba + 1
        }
    }

    fn serialize(&self) -> Vec<u8> {
        let mut buf = vec![0u8; GPT_ENTRY_SIZE as usize];
        buf[0..16].copy_from_slice(&self.type_guid.to_bytes_le());
        buf[16..32].copy_from_slice(&self.unique_guid.to_bytes_le());
        buf[32..40].copy_from_slice(&self.first_lba.to_le_bytes());
        buf[40..48].copy_from_slice(&self.last_lba.to_le_bytes());
        buf[48..56].copy_from_slice(&self.attributes.to_le_bytes());
        // Name: UTF-16LE, max 36 code units (72 bytes)
        let name_bytes: Vec<u16> = self.name.encode_utf16().collect();
        let max_chars = 36;
        let count = name_bytes.len().min(max_chars);
        for i in 0..count {
            let offset = 56 + i * 2;
            buf[offset..offset + 2].copy_from_slice(&name_bytes[i].to_le_bytes());
        }
        buf
    }

    fn parse(data: &[u8]) -> Option<GptEntry> {
        if data.len() < GPT_ENTRY_SIZE as usize {
            return None;
        }
        let type_guid = Guid::from_bytes_le(&data[0..16])?;
        let unique_guid = Guid::from_bytes_le(&data[16..32])?;
        let first_lba = u64::from_le_bytes([
            data[32], data[33], data[34], data[35], data[36], data[37], data[38], data[39],
        ]);
        let last_lba = u64::from_le_bytes([
            data[40], data[41], data[42], data[43], data[44], data[45], data[46], data[47],
        ]);
        let attributes = u64::from_le_bytes([
            data[48], data[49], data[50], data[51], data[52], data[53], data[54], data[55],
        ]);

        // Read UTF-16LE name (max 36 code units = 72 bytes)
        let mut name_u16 = Vec::new();
        for i in 0..36 {
            let offset = 56 + i * 2;
            if offset + 1 >= data.len() {
                break;
            }
            let ch = u16::from_le_bytes([data[offset], data[offset + 1]]);
            if ch == 0 {
                break;
            }
            name_u16.push(ch);
        }
        let name = String::from_utf16_lossy(&name_u16);

        Some(GptEntry {
            type_guid,
            unique_guid,
            first_lba,
            last_lba,
            attributes,
            name,
        })
    }

    fn has_flag(&self, flag: PartitionFlag) -> bool {
        match flag {
            PartitionFlag::Esp => {
                self.type_guid
                    == Guid::from_str_hex("C12A7328-F81F-11D2-BA4B-00A0C93EC93B")
                        .unwrap_or(Guid::ZERO)
            }
            PartitionFlag::BiosGrub => {
                self.type_guid
                    == Guid::from_str_hex("21686148-6449-6E6F-744E-656564454649")
                        .unwrap_or(Guid::ZERO)
            }
            PartitionFlag::Swap => {
                self.type_guid
                    == Guid::from_str_hex("0657FD6D-A4AB-43C4-84E5-0933C84B4F4F")
                        .unwrap_or(Guid::ZERO)
            }
            PartitionFlag::Raid => {
                self.type_guid
                    == Guid::from_str_hex("A19D880F-05FC-4D3B-A006-743F0F84911E")
                        .unwrap_or(Guid::ZERO)
            }
            PartitionFlag::Lvm => {
                self.type_guid
                    == Guid::from_str_hex("E6D6D379-F507-44C2-A23C-238F2A3DF928")
                        .unwrap_or(Guid::ZERO)
            }
            PartitionFlag::MsftData => {
                self.type_guid
                    == Guid::from_str_hex("EBD0A0A2-B9E5-4433-87C0-68B6B72699C7")
                        .unwrap_or(Guid::ZERO)
            }
            PartitionFlag::MsftRes => {
                self.type_guid
                    == Guid::from_str_hex("E3C9E316-0B5C-4DB8-817D-F92DF00215AE")
                        .unwrap_or(Guid::ZERO)
            }
            _ => {
                if let Some(bit) = flag.gpt_attribute_bit() {
                    if bit == 0 {
                        false
                    } else {
                        self.attributes & bit != 0
                    }
                } else {
                    false
                }
            }
        }
    }

    fn set_flag(&mut self, flag: PartitionFlag, on: bool) {
        match flag {
            PartitionFlag::Esp => {
                if on {
                    self.type_guid =
                        Guid::from_str_hex("C12A7328-F81F-11D2-BA4B-00A0C93EC93B")
                            .unwrap_or(Guid::ZERO);
                }
            }
            PartitionFlag::BiosGrub => {
                if on {
                    self.type_guid =
                        Guid::from_str_hex("21686148-6449-6E6F-744E-656564454649")
                            .unwrap_or(Guid::ZERO);
                }
            }
            PartitionFlag::Swap => {
                if on {
                    self.type_guid =
                        Guid::from_str_hex("0657FD6D-A4AB-43C4-84E5-0933C84B4F4F")
                            .unwrap_or(Guid::ZERO);
                }
            }
            PartitionFlag::Raid => {
                if on {
                    self.type_guid =
                        Guid::from_str_hex("A19D880F-05FC-4D3B-A006-743F0F84911E")
                            .unwrap_or(Guid::ZERO);
                }
            }
            PartitionFlag::Lvm => {
                if on {
                    self.type_guid =
                        Guid::from_str_hex("E6D6D379-F507-44C2-A23C-238F2A3DF928")
                            .unwrap_or(Guid::ZERO);
                }
            }
            PartitionFlag::MsftData => {
                if on {
                    self.type_guid =
                        Guid::from_str_hex("EBD0A0A2-B9E5-4433-87C0-68B6B72699C7")
                            .unwrap_or(Guid::ZERO);
                }
            }
            PartitionFlag::MsftRes => {
                if on {
                    self.type_guid =
                        Guid::from_str_hex("E3C9E316-0B5C-4DB8-817D-F92DF00215AE")
                            .unwrap_or(Guid::ZERO);
                }
            }
            _ => {
                if let Some(bit) = flag.gpt_attribute_bit()
                    && bit != 0 {
                        if on {
                            self.attributes |= bit;
                        } else {
                            self.attributes &= !bit;
                        }
                    }
            }
        }
    }

    fn flags_list(&self) -> Vec<&'static str> {
        let mut flags = Vec::new();
        for flag in PartitionFlag::all() {
            if self.has_flag(*flag) {
                flags.push(flag.name());
            }
        }
        flags
    }
}

// ============================================================================
// MBR structures
// ============================================================================

#[derive(Clone, Debug)]
struct MbrPartitionEntry {
    status: u8,           // 0x80 = bootable, 0x00 = inactive
    chs_start: [u8; 3],
    partition_type: u8,
    chs_end: [u8; 3],
    lba_start: u32,
    lba_size: u32,
}

impl MbrPartitionEntry {
    fn new() -> MbrPartitionEntry {
        MbrPartitionEntry {
            status: 0,
            chs_start: [0; 3],
            partition_type: 0,
            chs_end: [0; 3],
            lba_start: 0,
            lba_size: 0,
        }
    }

    fn is_empty(&self) -> bool {
        self.partition_type == 0
    }

    fn is_extended(&self) -> bool {
        matches!(self.partition_type, 0x05 | 0x0F | 0x85)
    }

    fn is_bootable(&self) -> bool {
        self.status == 0x80
    }

    fn serialize(&self) -> [u8; 16] {
        let mut buf = [0u8; 16];
        buf[0] = self.status;
        buf[1..4].copy_from_slice(&self.chs_start);
        buf[4] = self.partition_type;
        buf[5..8].copy_from_slice(&self.chs_end);
        buf[8..12].copy_from_slice(&self.lba_start.to_le_bytes());
        buf[12..16].copy_from_slice(&self.lba_size.to_le_bytes());
        buf
    }

    fn parse(data: &[u8]) -> Option<MbrPartitionEntry> {
        if data.len() < 16 {
            return None;
        }
        Some(MbrPartitionEntry {
            status: data[0],
            chs_start: [data[1], data[2], data[3]],
            partition_type: data[4],
            chs_end: [data[5], data[6], data[7]],
            lba_start: u32::from_le_bytes([data[8], data[9], data[10], data[11]]),
            lba_size: u32::from_le_bytes([data[12], data[13], data[14], data[15]]),
        })
    }

    /// Compute CHS values from LBA (for compatibility)
    fn lba_to_chs(lba: u32) -> [u8; 3] {
        // Use standard geometry: 255 heads, 63 sectors
        let spt = 63u32;
        let heads = 255u32;
        if lba >= heads * spt * 1024 {
            // CHS overflow: use max values
            return [0xFE, 0xFF, 0xFF];
        }
        let c = lba / (heads * spt);
        let h = (lba / spt) % heads;
        let s = (lba % spt) + 1;
        [
            h as u8,
            ((s as u8) & 0x3F) | (((c >> 8) as u8) & 0xC0),
            c as u8,
        ]
    }
}

#[derive(Clone, Debug)]
struct Mbr {
    bootstrap: [u8; 446],
    partitions: [MbrPartitionEntry; 4],
    signature: u16,
}

impl Mbr {
    fn new() -> Mbr {
        Mbr {
            bootstrap: [0u8; 446],
            partitions: [
                MbrPartitionEntry::new(),
                MbrPartitionEntry::new(),
                MbrPartitionEntry::new(),
                MbrPartitionEntry::new(),
            ],
            signature: MBR_SIGNATURE,
        }
    }

    fn new_protective(disk_sectors: u64) -> Mbr {
        let mut mbr = Mbr::new();
        let size = if disk_sectors > u32::MAX as u64 {
            u32::MAX
        } else {
            (disk_sectors - 1) as u32
        };
        mbr.partitions[0] = MbrPartitionEntry {
            status: 0x00,
            chs_start: [0x00, 0x02, 0x00],
            partition_type: 0xEE,
            chs_end: [0xFF, 0xFF, 0xFF],
            lba_start: 1,
            lba_size: size,
        };
        mbr
    }

    fn serialize(&self) -> [u8; 512] {
        let mut buf = [0u8; 512];
        buf[..446].copy_from_slice(&self.bootstrap);
        for i in 0..4 {
            let offset = MBR_PARTITION_OFFSET + i * MBR_PARTITION_ENTRY_SIZE;
            buf[offset..offset + 16].copy_from_slice(&self.partitions[i].serialize());
        }
        buf[510..512].copy_from_slice(&self.signature.to_le_bytes());
        buf
    }

    fn parse(data: &[u8]) -> Option<Mbr> {
        if data.len() < MBR_SIZE {
            return None;
        }
        let sig = u16::from_le_bytes([data[510], data[511]]);
        if sig != MBR_SIGNATURE {
            return None;
        }

        let mut bootstrap = [0u8; 446];
        bootstrap.copy_from_slice(&data[..446]);

        let mut partitions = [
            MbrPartitionEntry::new(),
            MbrPartitionEntry::new(),
            MbrPartitionEntry::new(),
            MbrPartitionEntry::new(),
        ];
        for i in 0..4 {
            let offset = MBR_PARTITION_OFFSET + i * MBR_PARTITION_ENTRY_SIZE;
            partitions[i] =
                MbrPartitionEntry::parse(&data[offset..offset + 16]).unwrap_or(MbrPartitionEntry::new());
        }

        Some(Mbr {
            bootstrap,
            partitions,
            signature: sig,
        })
    }

    fn is_protective_gpt(&self) -> bool {
        self.partitions[0].partition_type == 0xEE
    }
}

// ============================================================================
// Unified partition table
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TableType {
    Gpt,
    Mbr,
}

impl fmt::Display for TableType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TableType::Gpt => write!(f, "gpt"),
            TableType::Mbr => write!(f, "msdos"),
        }
    }
}

impl TableType {
    fn from_str(s: &str) -> Option<TableType> {
        match s.to_lowercase().as_str() {
            "gpt" => Some(TableType::Gpt),
            "msdos" | "mbr" | "dos" => Some(TableType::Mbr),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MbrPartRole {
    Primary,
    Extended,
    Logical,
}

impl fmt::Display for MbrPartRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MbrPartRole::Primary => write!(f, "primary"),
            MbrPartRole::Extended => write!(f, "extended"),
            MbrPartRole::Logical => write!(f, "logical"),
        }
    }
}

#[derive(Clone, Debug)]
struct Partition {
    number: u32,
    first_lba: u64,
    last_lba: u64,
    size_sectors: u64,
    name: String,
    type_name: String,
    flags: Vec<PartitionFlag>,
    // GPT-specific
    type_guid: Option<Guid>,
    unique_guid: Option<Guid>,
    attributes: u64,
    // MBR-specific
    mbr_type: Option<u8>,
    mbr_role: Option<MbrPartRole>,
    bootable: bool,
}

impl Partition {
    fn size_bytes(&self) -> u64 {
        self.size_sectors * SECTOR_SIZE
    }
}

#[derive(Clone, Debug)]
struct DiskInfo {
    path: String,
    size_bytes: u64,
    size_sectors: u64,
    sector_size: u64,
    table_type: Option<TableType>,
    model: String,
    // GPT-specific
    gpt_header: Option<GptHeader>,
    // MBR-specific
    mbr: Option<Mbr>,
    // Partitions
    partitions: Vec<Partition>,
}

impl DiskInfo {
    fn new(path: &str, size_bytes: u64) -> DiskInfo {
        let sector_size = SECTOR_SIZE;
        DiskInfo {
            path: path.to_string(),
            size_bytes,
            size_sectors: size_bytes / sector_size,
            sector_size,
            table_type: None,
            model: String::new(),
            gpt_header: None,
            mbr: None,
            partitions: Vec::new(),
        }
    }

    fn detect_table_type(data: &[u8]) -> Option<TableType> {
        // Check for GPT first (protective MBR + GPT header)
        if data.len() >= 1024 {
            let sig = u64::from_le_bytes([
                data[512], data[513], data[514], data[515], data[516], data[517], data[518],
                data[519],
            ]);
            if sig == GPT_SIGNATURE {
                return Some(TableType::Gpt);
            }
        }
        // Check for MBR
        if data.len() >= 512 {
            let sig = u16::from_le_bytes([data[510], data[511]]);
            if sig == MBR_SIGNATURE {
                return Some(TableType::Mbr);
            }
        }
        None
    }

    fn parse_gpt(data: &[u8], disk: &mut DiskInfo) -> bool {
        if data.len() < 1024 {
            return false;
        }
        // Parse protective MBR
        if let Some(mbr) = Mbr::parse(&data[..512]) {
            disk.mbr = Some(mbr);
        }
        // Parse GPT header at LBA 1
        let header = match GptHeader::parse(&data[512..]) {
            Some(h) => h,
            None => return false,
        };

        // Parse partition entries
        let entry_start = header.partition_entry_lba as usize * SECTOR_SIZE as usize;
        let entry_total =
            header.num_partition_entries as usize * header.partition_entry_size as usize;
        if data.len() < entry_start + entry_total {
            // Not enough data for all entries, parse what we can
            let available = if data.len() > entry_start {
                data.len() - entry_start
            } else {
                0
            };
            let _max_entries = available / header.partition_entry_size as usize;
        }

        let mut part_num = 1u32;
        for i in 0..header.num_partition_entries as usize {
            let offset = entry_start + i * header.partition_entry_size as usize;
            if offset + header.partition_entry_size as usize > data.len() {
                break;
            }
            if let Some(entry) = GptEntry::parse(&data[offset..])
                && !entry.is_empty() {
                    let type_name = guid_to_name(&entry.type_guid).to_string();
                    let flags: Vec<PartitionFlag> = PartitionFlag::all()
                        .iter()
                        .filter(|f| entry.has_flag(**f))
                        .copied()
                        .collect();
                    disk.partitions.push(Partition {
                        number: part_num,
                        first_lba: entry.first_lba,
                        last_lba: entry.last_lba,
                        size_sectors: entry.size_sectors(),
                        name: entry.name.clone(),
                        type_name,
                        flags,
                        type_guid: Some(entry.type_guid),
                        unique_guid: Some(entry.unique_guid),
                        attributes: entry.attributes,
                        mbr_type: None,
                        mbr_role: None,
                        bootable: false,
                    });
                    part_num += 1;
                }
        }

        disk.gpt_header = Some(header);
        disk.table_type = Some(TableType::Gpt);
        true
    }

    fn parse_mbr(data: &[u8], disk: &mut DiskInfo) -> bool {
        let mbr = match Mbr::parse(data) {
            Some(m) => m,
            None => return false,
        };

        if mbr.is_protective_gpt() {
            return false; // This is a GPT disk
        }

        let mut part_num = 1u32;
        let mut _extended_lba: u64 = 0;

        for i in 0..4 {
            let entry = &mbr.partitions[i];
            if entry.is_empty() {
                continue;
            }
            let role = if entry.is_extended() {
                _extended_lba = entry.lba_start as u64;
                MbrPartRole::Extended
            } else {
                MbrPartRole::Primary
            };
            let type_name = mbr_type_name(entry.partition_type).to_string();
            let mut flags = Vec::new();
            if entry.is_bootable() {
                flags.push(PartitionFlag::Boot);
            }
            disk.partitions.push(Partition {
                number: part_num,
                first_lba: entry.lba_start as u64,
                last_lba: entry.lba_start as u64 + entry.lba_size as u64 - 1,
                size_sectors: entry.lba_size as u64,
                name: String::new(),
                type_name,
                flags,
                type_guid: None,
                unique_guid: None,
                attributes: 0,
                mbr_type: Some(entry.partition_type),
                mbr_role: Some(role),
                bootable: entry.is_bootable(),
            });
            part_num += 1;
        }

        // Parse logical partitions from extended partitions
        // In a real implementation we would walk the extended partition chain;
        // here we parse the chain from data if available.
        if _extended_lba > 0 {
            parse_logical_partitions(data, _extended_lba, &mut disk.partitions, &mut part_num);
        }

        disk.mbr = Some(mbr);
        disk.table_type = Some(TableType::Mbr);
        true
    }
}

/// Walk the extended partition chain to find logical partitions
fn parse_logical_partitions(
    data: &[u8],
    extended_lba: u64,
    partitions: &mut Vec<Partition>,
    part_num: &mut u32,
) {
    let mut current_ebr_lba = extended_lba;
    let mut iterations = 0;
    let max_iterations = 256; // Safety limit

    while iterations < max_iterations {
        iterations += 1;
        let offset = current_ebr_lba as usize * SECTOR_SIZE as usize;
        if offset + 512 > data.len() {
            break;
        }

        let sig = u16::from_le_bytes([data[offset + 510], data[offset + 511]]);
        if sig != MBR_SIGNATURE {
            break;
        }

        // First entry in EBR: the logical partition (relative to this EBR)
        let entry_offset = offset + MBR_PARTITION_OFFSET;
        let entry = match MbrPartitionEntry::parse(&data[entry_offset..]) {
            Some(e) => e,
            None => break,
        };

        if entry.is_empty() {
            break;
        }

        let logical_lba = current_ebr_lba + entry.lba_start as u64;
        let type_name = mbr_type_name(entry.partition_type).to_string();
        let mut flags = Vec::new();
        if entry.is_bootable() {
            flags.push(PartitionFlag::Boot);
        }

        partitions.push(Partition {
            number: *part_num,
            first_lba: logical_lba,
            last_lba: logical_lba + entry.lba_size as u64 - 1,
            size_sectors: entry.lba_size as u64,
            name: String::new(),
            type_name,
            flags,
            type_guid: None,
            unique_guid: None,
            attributes: 0,
            mbr_type: Some(entry.partition_type),
            mbr_role: Some(MbrPartRole::Logical),
            bootable: entry.is_bootable(),
        });
        *part_num += 1;

        // Second entry in EBR: link to next EBR (relative to extended partition start)
        let next_offset = entry_offset + 16;
        let next_entry = match MbrPartitionEntry::parse(&data[next_offset..]) {
            Some(e) => e,
            None => break,
        };
        if next_entry.is_empty() {
            break;
        }
        current_ebr_lba = extended_lba + next_entry.lba_start as u64;
    }
}

// ============================================================================
// In-memory partition table editor
// ============================================================================

#[derive(Clone, Debug)]
struct DiskEditor {
    disk: DiskInfo,
    unit: DisplayUnit,
    alignment: u64, // in sectors
    gpt_entries: Vec<GptEntry>,
    modified: bool,
}

impl DiskEditor {
    fn new(disk: DiskInfo) -> DiskEditor {
        let mut gpt_entries = Vec::new();
        // If GPT, populate entries from partitions
        if disk.table_type == Some(TableType::Gpt) {
            for part in &disk.partitions {
                let mut entry = GptEntry::new();
                entry.type_guid = part.type_guid.unwrap_or(Guid::ZERO);
                entry.unique_guid = part.unique_guid.unwrap_or(Guid::ZERO);
                entry.first_lba = part.first_lba;
                entry.last_lba = part.last_lba;
                entry.attributes = part.attributes;
                entry.name = part.name.clone();
                gpt_entries.push(entry);
            }
        }
        DiskEditor {
            disk,
            unit: DisplayUnit::Compact,
            alignment: DEFAULT_ALIGNMENT_SECTORS,
            gpt_entries,
            modified: false,
        }
    }

    fn create_label(&mut self, table_type: TableType) {
        self.disk.partitions.clear();
        self.gpt_entries.clear();
        self.disk.table_type = Some(table_type);
        match table_type {
            TableType::Gpt => {
                let header = GptHeader::new(self.disk.size_sectors);
                self.disk.gpt_header = Some(header);
                self.disk.mbr = Some(Mbr::new_protective(self.disk.size_sectors));
            }
            TableType::Mbr => {
                self.disk.mbr = Some(Mbr::new());
                self.disk.gpt_header = None;
            }
        }
        self.modified = true;
    }

    fn find_free_regions(&self) -> Vec<(u64, u64)> {
        let (first_usable, last_usable) = match self.disk.table_type {
            Some(TableType::Gpt) => {
                if let Some(ref hdr) = self.disk.gpt_header {
                    (hdr.first_usable_lba, hdr.last_usable_lba)
                } else {
                    (34, self.disk.size_sectors.saturating_sub(34))
                }
            }
            Some(TableType::Mbr) => (1, self.disk.size_sectors.saturating_sub(1)),
            None => (0, self.disk.size_sectors.saturating_sub(1)),
        };

        let mut used: Vec<(u64, u64)> = self
            .disk
            .partitions
            .iter()
            .map(|p| (p.first_lba, p.last_lba))
            .collect();
        used.sort_by_key(|&(start, _)| start);

        let mut free = Vec::new();
        let mut cursor = first_usable;
        for (start, end) in &used {
            if cursor < *start {
                free.push((cursor, *start - 1));
            }
            if *end >= cursor {
                cursor = *end + 1;
            }
        }
        if cursor <= last_usable {
            free.push((cursor, last_usable));
        }
        free
    }

    fn mkpart_gpt(
        &mut self,
        name: &str,
        fs_type: &str,
        start_bytes: u64,
        end_bytes: u64,
    ) -> Result<u32, String> {
        let start_sector = align_up(
            bytes_to_sectors_ceil(start_bytes, self.disk.sector_size),
            self.alignment,
        );
        let end_sector = align_down(
            bytes_to_sectors_floor(end_bytes, self.disk.sector_size),
            self.alignment,
        )
        .saturating_sub(1);

        if start_sector >= end_sector {
            return Err("Start must be before end".to_string());
        }

        // Check overlap
        for p in &self.disk.partitions {
            if start_sector <= p.last_lba && end_sector >= p.first_lba {
                return Err(format!("Overlaps with partition {}", p.number));
            }
        }

        // Check bounds
        if let Some(ref hdr) = self.disk.gpt_header {
            if start_sector < hdr.first_usable_lba {
                return Err("Start is before first usable LBA".to_string());
            }
            if end_sector > hdr.last_usable_lba {
                return Err("End is past last usable LBA".to_string());
            }
        }

        let type_guid = name_to_guid(fs_type)
            .unwrap_or_else(|| {
                Guid::from_str_hex("0FC63DAF-8483-4772-8E79-3D69D8477DE4").unwrap_or(Guid::ZERO)
            });
        let part_num = self.disk.partitions.len() as u32 + 1;
        let unique_guid = Guid::generate(start_sector ^ end_sector ^ part_num as u64);

        let entry = GptEntry {
            type_guid,
            unique_guid,
            first_lba: start_sector,
            last_lba: end_sector,
            attributes: 0,
            name: name.to_string(),
        };

        let type_name = guid_to_name(&type_guid).to_string();
        let partition = Partition {
            number: part_num,
            first_lba: start_sector,
            last_lba: end_sector,
            size_sectors: end_sector - start_sector + 1,
            name: name.to_string(),
            type_name,
            flags: Vec::new(),
            type_guid: Some(type_guid),
            unique_guid: Some(unique_guid),
            attributes: 0,
            mbr_type: None,
            mbr_role: None,
            bootable: false,
        };

        self.gpt_entries.push(entry);
        self.disk.partitions.push(partition);
        self.modified = true;
        Ok(part_num)
    }

    fn mkpart_mbr(
        &mut self,
        role: MbrPartRole,
        fs_type: &str,
        start_bytes: u64,
        end_bytes: u64,
    ) -> Result<u32, String> {
        let start_sector = align_up(
            bytes_to_sectors_ceil(start_bytes, self.disk.sector_size),
            self.alignment,
        );
        let end_sector = align_down(
            bytes_to_sectors_floor(end_bytes, self.disk.sector_size),
            self.alignment,
        )
        .saturating_sub(1);

        if start_sector >= end_sector {
            return Err("Start must be before end".to_string());
        }

        // Check overlap
        for p in &self.disk.partitions {
            if start_sector <= p.last_lba && end_sector >= p.first_lba {
                return Err(format!("Overlaps with partition {}", p.number));
            }
        }

        let primary_count = self
            .disk
            .partitions
            .iter()
            .filter(|p| {
                p.mbr_role == Some(MbrPartRole::Primary) || p.mbr_role == Some(MbrPartRole::Extended)
            })
            .count();

        match role {
            MbrPartRole::Primary | MbrPartRole::Extended => {
                if primary_count >= 4 {
                    return Err("Maximum 4 primary/extended partitions".to_string());
                }
            }
            MbrPartRole::Logical => {
                let has_extended = self
                    .disk
                    .partitions
                    .iter()
                    .any(|p| p.mbr_role == Some(MbrPartRole::Extended));
                if !has_extended {
                    return Err("No extended partition for logical partition".to_string());
                }
            }
        }

        if start_sector > u32::MAX as u64 || end_sector > u32::MAX as u64 {
            return Err("MBR cannot address sectors beyond 2TB".to_string());
        }

        let type_code = match role {
            MbrPartRole::Extended => 0x05,
            _ => mbr_type_code(fs_type).unwrap_or(0x83),
        };

        let part_num = self.disk.partitions.len() as u32 + 1;
        let size_sectors = end_sector - start_sector + 1;
        let type_name = mbr_type_name(type_code).to_string();

        let partition = Partition {
            number: part_num,
            first_lba: start_sector,
            last_lba: end_sector,
            size_sectors,
            name: String::new(),
            type_name,
            flags: Vec::new(),
            type_guid: None,
            unique_guid: None,
            attributes: 0,
            mbr_type: Some(type_code),
            mbr_role: Some(role),
            bootable: false,
        };

        self.disk.partitions.push(partition);

        // Update MBR if primary/extended
        if matches!(role, MbrPartRole::Primary | MbrPartRole::Extended)
            && let Some(ref mut mbr) = self.disk.mbr {
                for i in 0..4 {
                    if mbr.partitions[i].is_empty() {
                        mbr.partitions[i] = MbrPartitionEntry {
                            status: 0,
                            chs_start: MbrPartitionEntry::lba_to_chs(start_sector as u32),
                            partition_type: type_code,
                            chs_end: MbrPartitionEntry::lba_to_chs(end_sector as u32),
                            lba_start: start_sector as u32,
                            lba_size: size_sectors as u32,
                        };
                        break;
                    }
                }
            }

        self.modified = true;
        Ok(part_num)
    }

    fn rm_partition(&mut self, number: u32) -> Result<(), String> {
        let idx = self
            .disk
            .partitions
            .iter()
            .position(|p| p.number == number)
            .ok_or_else(|| format!("Partition {} not found", number))?;

        self.disk.partitions.remove(idx);

        // Remove from GPT entries if applicable
        if idx < self.gpt_entries.len() {
            self.gpt_entries.remove(idx);
        }

        // Renumber remaining
        for (i, p) in self.disk.partitions.iter_mut().enumerate() {
            p.number = i as u32 + 1;
        }

        self.modified = true;
        Ok(())
    }

    fn name_partition(&mut self, number: u32, name: &str) -> Result<(), String> {
        if self.disk.table_type != Some(TableType::Gpt) {
            return Err("Partition names only supported on GPT".to_string());
        }
        let idx = self
            .disk
            .partitions
            .iter()
            .position(|p| p.number == number)
            .ok_or_else(|| format!("Partition {} not found", number))?;

        self.disk.partitions[idx].name = name.to_string();
        if idx < self.gpt_entries.len() {
            self.gpt_entries[idx].name = name.to_string();
        }
        self.modified = true;
        Ok(())
    }

    fn set_flag(&mut self, number: u32, flag: PartitionFlag, on: bool) -> Result<(), String> {
        let idx = self
            .disk
            .partitions
            .iter()
            .position(|p| p.number == number)
            .ok_or_else(|| format!("Partition {} not found", number))?;

        match self.disk.table_type {
            Some(TableType::Gpt) => {
                if idx < self.gpt_entries.len() {
                    self.gpt_entries[idx].set_flag(flag, on);
                }
                let part = &mut self.disk.partitions[idx];
                if on && !part.flags.contains(&flag) {
                    part.flags.push(flag);
                } else if !on {
                    part.flags.retain(|f| *f != flag);
                }
            }
            Some(TableType::Mbr) => {
                if flag.mbr_boot_flag() {
                    // Only one bootable partition in MBR
                    if on {
                        for p in &mut self.disk.partitions {
                            p.bootable = false;
                            p.flags.retain(|f| *f != PartitionFlag::Boot);
                        }
                    }
                    let part = &mut self.disk.partitions[idx];
                    part.bootable = on;
                    if on {
                        part.flags.push(PartitionFlag::Boot);
                    }
                    if let Some(ref mut mbr) = self.disk.mbr {
                        for entry in &mut mbr.partitions {
                            entry.status = 0;
                        }
                        if idx < 4 {
                            mbr.partitions[idx].status = if on { 0x80 } else { 0 };
                        }
                    }
                } else {
                    return Err(format!("Flag '{}' not supported on MBR", flag.name()));
                }
            }
            None => return Err("No partition table".to_string()),
        }
        self.modified = true;
        Ok(())
    }

    fn toggle_flag(&mut self, number: u32, flag: PartitionFlag) -> Result<(), String> {
        let current = self
            .disk
            .partitions
            .iter()
            .find(|p| p.number == number)
            .ok_or_else(|| format!("Partition {} not found", number))?
            .flags
            .contains(&flag);
        self.set_flag(number, flag, !current)
    }

    fn resize_partition(&mut self, number: u32, end_bytes: u64) -> Result<(), String> {
        let idx = self
            .disk
            .partitions
            .iter()
            .position(|p| p.number == number)
            .ok_or_else(|| format!("Partition {} not found", number))?;

        let new_end = align_down(
            bytes_to_sectors_floor(end_bytes, self.disk.sector_size),
            self.alignment,
        )
        .saturating_sub(1);

        let first = self.disk.partitions[idx].first_lba;
        if new_end <= first {
            return Err("New end must be after start".to_string());
        }

        // Check overlap with other partitions
        for (i, p) in self.disk.partitions.iter().enumerate() {
            if i == idx {
                continue;
            }
            if first <= p.last_lba && new_end >= p.first_lba {
                return Err(format!("Would overlap with partition {}", p.number));
            }
        }

        self.disk.partitions[idx].last_lba = new_end;
        self.disk.partitions[idx].size_sectors = new_end - first + 1;
        if idx < self.gpt_entries.len() {
            self.gpt_entries[idx].last_lba = new_end;
        }
        self.modified = true;
        Ok(())
    }

    fn move_partition(&mut self, number: u32, new_start_bytes: u64) -> Result<(), String> {
        let idx = self
            .disk
            .partitions
            .iter()
            .position(|p| p.number == number)
            .ok_or_else(|| format!("Partition {} not found", number))?;

        let new_start = align_up(
            bytes_to_sectors_ceil(new_start_bytes, self.disk.sector_size),
            self.alignment,
        );
        let size = self.disk.partitions[idx].size_sectors;
        let new_end = new_start + size - 1;

        // Check overlap
        for (i, p) in self.disk.partitions.iter().enumerate() {
            if i == idx {
                continue;
            }
            if new_start <= p.last_lba && new_end >= p.first_lba {
                return Err(format!("Would overlap with partition {}", p.number));
            }
        }

        self.disk.partitions[idx].first_lba = new_start;
        self.disk.partitions[idx].last_lba = new_end;
        if idx < self.gpt_entries.len() {
            self.gpt_entries[idx].first_lba = new_start;
            self.gpt_entries[idx].last_lba = new_end;
        }
        self.modified = true;
        Ok(())
    }

    fn check_alignment(&self, number: u32, alignment_type: &str) -> Result<bool, String> {
        let part = self
            .disk
            .partitions
            .iter()
            .find(|p| p.number == number)
            .ok_or_else(|| format!("Partition {} not found", number))?;

        let check_align = match alignment_type {
            "minimal" => 8, // 4096 bytes / 512 byte sectors
            // "optimal" and any other value use the disk's configured alignment.
            _ => self.alignment,
        };

        Ok(part.first_lba % check_align == 0)
    }

    fn print_table(&self) -> String {
        let mut output = String::new();
        let disk = &self.disk;

        output.push_str(&format!(
            "Model: {}\n",
            if disk.model.is_empty() {
                "(unknown)"
            } else {
                &disk.model
            }
        ));
        output.push_str(&format!(
            "Disk {}: {}\n",
            disk.path,
            format_size(disk.size_bytes, self.unit, disk.size_bytes, disk.sector_size)
        ));
        output.push_str(&format!("Sector size (logical/physical): {}B/{}B\n", disk.sector_size, disk.sector_size));
        output.push_str(&format!(
            "Partition Table: {}\n",
            disk.table_type
                .map(|t| format!("{}", t))
                .unwrap_or_else(|| "unknown".to_string())
        ));

        if let Some(ref hdr) = disk.gpt_header {
            output.push_str("Disk Flags: \n");
            output.push_str(&format!("Disk GUID: {}\n", hdr.disk_guid));
        }

        output.push('\n');

        match self.unit {
            DisplayUnit::Sectors => {
                output.push_str("Number  Start       End         Size        File system  Name                  Flags\n");
            }
            _ => {
                output.push_str("Number  Start   End     Size    File system  Name                  Flags\n");
            }
        }

        for part in &disk.partitions {
            let start = format_size(
                part.first_lba * disk.sector_size,
                self.unit,
                disk.size_bytes,
                disk.sector_size,
            );
            let end = format_size(
                (part.last_lba + 1) * disk.sector_size,
                self.unit,
                disk.size_bytes,
                disk.sector_size,
            );
            let size = format_size(
                part.size_bytes(),
                self.unit,
                disk.size_bytes,
                disk.sector_size,
            );
            let flags: Vec<&str> = part.flags.iter().map(|f| f.name()).collect();
            let flag_str = flags.join(", ");

            let type_or_role = if let Some(role) = part.mbr_role {
                format!("{}", role)
            } else {
                part.type_name.clone()
            };

            output.push_str(&format!(
                " {:<6} {:<11} {:<11} {:<11} {:<12} {:<21} {}\n",
                part.number, start, end, size, type_or_role, part.name, flag_str,
            ));
        }

        output
    }

    fn print_free_space(&self) -> String {
        let mut output = String::new();
        let free = self.find_free_regions();
        if free.is_empty() {
            output.push_str("No free space on disk\n");
        } else {
            output.push_str("Free space regions:\n");
            for (start, end) in &free {
                let size = (end - start + 1) * self.disk.sector_size;
                output.push_str(&format!(
                    "  {} - {} ({})  [{} sectors]\n",
                    format_size(
                        start * self.disk.sector_size,
                        self.unit,
                        self.disk.size_bytes,
                        self.disk.sector_size,
                    ),
                    format_size(
                        (end + 1) * self.disk.sector_size,
                        self.unit,
                        self.disk.size_bytes,
                        self.disk.sector_size,
                    ),
                    format_compact(size),
                    end - start + 1,
                ));
            }
        }
        output
    }

    /// Serialize the entire partition table to bytes
    fn serialize_table(&self) -> Vec<u8> {
        match self.disk.table_type {
            Some(TableType::Gpt) => self.serialize_gpt(),
            Some(TableType::Mbr) => self.serialize_mbr(),
            None => Vec::new(),
        }
    }

    fn serialize_gpt(&self) -> Vec<u8> {
        let total_size = self.disk.size_sectors as usize * SECTOR_SIZE as usize;
        let mut data = vec![0u8; total_size.min(1024 * 1024)]; // Cap at 1MB for serialization

        // Protective MBR
        if let Some(ref mbr) = self.disk.mbr {
            let mbr_data = mbr.serialize();
            if data.len() >= 512 {
                data[..512].copy_from_slice(&mbr_data);
            }
        }

        // Partition entries
        let mut entries_data =
            vec![0u8; GPT_MAX_ENTRIES as usize * GPT_ENTRY_SIZE as usize];
        for (i, entry) in self.gpt_entries.iter().enumerate() {
            let offset = i * GPT_ENTRY_SIZE as usize;
            let serialized = entry.serialize();
            entries_data[offset..offset + GPT_ENTRY_SIZE as usize]
                .copy_from_slice(&serialized);
        }
        let entries_crc = crc32(&entries_data);

        // Write entries at LBA 2
        let entry_offset = 2 * SECTOR_SIZE as usize;
        if data.len() >= entry_offset + entries_data.len() {
            data[entry_offset..entry_offset + entries_data.len()]
                .copy_from_slice(&entries_data);
        }

        // GPT Header
        if let Some(ref hdr) = self.disk.gpt_header {
            let mut header = hdr.clone();
            header.partition_entry_crc32 = entries_crc;
            let header_data = header.serialize_with_crc();
            let header_offset = SECTOR_SIZE as usize;
            if data.len() >= header_offset + header_data.len() {
                data[header_offset..header_offset + header_data.len()]
                    .copy_from_slice(&header_data);
            }
        }

        data
    }

    fn serialize_mbr(&self) -> Vec<u8> {
        if let Some(ref mbr) = self.disk.mbr {
            mbr.serialize().to_vec()
        } else {
            vec![0u8; 512]
        }
    }
}

// ============================================================================
// Command parsing
// ============================================================================

#[derive(Debug)]
enum PartedCommand {
    Print { list_all: bool, free: bool },
    MkLabel { label_type: String },
    MkPart { part_type: String, fs_type: String, start: String, end: String },
    Rm { number: u32 },
    Name { number: u32, name: String },
    Set { number: u32, flag: String, state: String },
    Toggle { number: u32, flag: String },
    ResizePart { number: u32, end: String },
    Move { number: u32, start: String },
    Unit { unit: String },
    AlignCheck { align_type: String, number: u32 },
    Select { device: String },
    Help { command: Option<String> },
    Version,
    Quit,
}

fn parse_parted_command(args: &[String]) -> Option<PartedCommand> {
    if args.is_empty() {
        return None;
    }
    let cmd = args[0].to_lowercase();
    match cmd.as_str() {
        "print" | "p" => {
            let list_all = args.iter().any(|a| a == "-l" || a == "--list");
            let free = args.iter().any(|a| a == "free");
            Some(PartedCommand::Print { list_all, free })
        }
        "mklabel" | "mktable" => {
            let label = args.get(1).cloned().unwrap_or_default();
            Some(PartedCommand::MkLabel { label_type: label })
        }
        "mkpart" => {
            let part_type = args.get(1).cloned().unwrap_or_default();
            let fs_type = args.get(2).cloned().unwrap_or_default();
            let start = args.get(3).cloned().unwrap_or_default();
            let end = args.get(4).cloned().unwrap_or_default();
            Some(PartedCommand::MkPart {
                part_type,
                fs_type,
                start,
                end,
            })
        }
        "rm" | "remove" => {
            let number: u32 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
            Some(PartedCommand::Rm { number })
        }
        "name" => {
            let number: u32 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
            let name = args.get(2).cloned().unwrap_or_default();
            Some(PartedCommand::Name { number, name })
        }
        "set" => {
            let number: u32 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
            let flag = args.get(2).cloned().unwrap_or_default();
            let state = args.get(3).cloned().unwrap_or_else(|| "on".to_string());
            Some(PartedCommand::Set {
                number,
                flag,
                state,
            })
        }
        "toggle" => {
            let number: u32 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
            let flag = args.get(2).cloned().unwrap_or_default();
            Some(PartedCommand::Toggle { number, flag })
        }
        "resizepart" => {
            let number: u32 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
            let end = args.get(2).cloned().unwrap_or_default();
            Some(PartedCommand::ResizePart { number, end })
        }
        "move" => {
            let number: u32 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
            let start = args.get(2).cloned().unwrap_or_default();
            Some(PartedCommand::Move { number, start })
        }
        "unit" => {
            let unit = args.get(1).cloned().unwrap_or_default();
            Some(PartedCommand::Unit { unit })
        }
        "align-check" => {
            let align_type = args.get(1).cloned().unwrap_or_else(|| "optimal".to_string());
            let number: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
            Some(PartedCommand::AlignCheck {
                align_type,
                number,
            })
        }
        "select" => {
            let device = args.get(1).cloned().unwrap_or_default();
            Some(PartedCommand::Select { device })
        }
        "help" | "?" => {
            let command = args.get(1).cloned();
            Some(PartedCommand::Help { command })
        }
        "version" | "v" => Some(PartedCommand::Version),
        "quit" | "q" | "exit" => Some(PartedCommand::Quit),
        _ => None,
    }
}

// ============================================================================
// Help text
// ============================================================================

fn print_help(command: Option<&str>) -> String {
    match command {
        Some("print") | Some("p") => {
            "print [free|-l]  Display the partition table, free space, or all disk labels\n\
             \n\
             Options:\n\
             -l     List partition tables of all devices\n\
             free   Show free space as well\n"
                .to_string()
        }
        Some("mklabel") | Some("mktable") => {
            "mklabel LABEL-TYPE  Create a new partition table\n\
             \n\
             Label types: gpt, msdos (mbr)\n"
                .to_string()
        }
        Some("mkpart") => {
            "mkpart PART-TYPE [FS-TYPE] START END\n\
             \n\
             Create a new partition.\n\
             For MBR: PART-TYPE is primary, extended, or logical\n\
             For GPT: PART-TYPE is the partition name\n\
             FS-TYPE: ext4, fat32, ntfs, linux-swap, etc.\n\
             START/END: size with unit (e.g., 1MiB, 50%, 2048s)\n"
                .to_string()
        }
        Some("rm") | Some("remove") => {
            "rm NUMBER  Remove partition NUMBER\n".to_string()
        }
        Some("name") => {
            "name NUMBER NAME  Set the name of partition NUMBER to NAME (GPT only)\n".to_string()
        }
        Some("set") => {
            "set NUMBER FLAG STATE  Set FLAG on partition NUMBER to STATE (on/off)\n\
             \n\
             Flags: boot, esp, swap, raid, lvm, hidden, msftdata, msftres, bios_grub\n"
                .to_string()
        }
        Some("toggle") => {
            "toggle NUMBER FLAG  Toggle FLAG on partition NUMBER\n".to_string()
        }
        Some("resizepart") => {
            "resizepart NUMBER END  Resize partition NUMBER to end at END\n".to_string()
        }
        Some("move") => {
            "move NUMBER START  Move partition NUMBER to start at START\n".to_string()
        }
        Some("unit") => {
            "unit UNIT  Set the display unit\n\
             \n\
             Units: s (sectors), B (bytes), kB, MB, GB, TB, % (percent), compact\n"
                .to_string()
        }
        Some("align-check") => {
            "align-check TYPE NUMBER  Check partition alignment\n\
             \n\
             Types: minimal, optimal\n"
                .to_string()
        }
        Some("select") => {
            "select DEVICE  Select a different disk device\n".to_string()
        }
        _ => {
            "Commands:\n\
             align-check TYPE N                    check partition N alignment\n\
             help [COMMAND]                        show help\n\
             mklabel,mktable LABEL-TYPE            create partition table\n\
             mkpart PART-TYPE [FS-TYPE] START END  create partition\n\
             move NUMBER START                     move partition\n\
             name NUMBER NAME                      set partition name (GPT)\n\
             print [free|-l]                       display partition table\n\
             quit                                  exit\n\
             resizepart NUMBER END                 resize partition\n\
             rm NUMBER                             delete partition\n\
             select DEVICE                         select disk\n\
             set NUMBER FLAG STATE                 set partition flag\n\
             toggle NUMBER FLAG                    toggle partition flag\n\
             unit UNIT                             set display unit\n\
             version                               show version\n"
                .to_string()
        }
    }
}

// ============================================================================
// Parted personality
// ============================================================================

fn run_parted(args: &[String]) -> i32 {
    let mut device_path: Option<String> = None;
    let mut commands: Vec<String> = Vec::new();
    let mut script_mode = false;
    let mut _list_all = false;

    // Parse global options and device
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-s" | "--script" => script_mode = true,
            "-l" | "--list" => _list_all = true,
            "-h" | "--help" => {
                print!("{}", print_help(None));
                return 0;
            }
            "--version" => {
                println!("parted (OurOS) 1.0.0");
                return 0;
            }
            _ => {
                if device_path.is_none() && !args[i].starts_with('-') {
                    device_path = Some(args[i].clone());
                } else {
                    // Everything after device is commands
                    commands.extend_from_slice(&args[i..]);
                    break;
                }
            }
        }
        i += 1;
    }

    let disk_path = device_path.unwrap_or_else(|| "/dev/sda".to_string());

    // Create a default disk for demonstration
    let disk = DiskInfo::new(&disk_path, 500 * GB);
    let mut editor = DiskEditor::new(disk);

    if commands.is_empty() && !script_mode {
        // Interactive mode would read from stdin; for now just print usage
        println!("GNU Parted (OurOS) 1.0.0");
        println!("Usage: parted [OPTION]... [DEVICE [COMMAND [PARAMETERS]...]...]");
        println!("Try 'parted --help' for more information.");
        return 0;
    }

    // Process commands
    if !commands.is_empty() {
        if let Some(cmd) = parse_parted_command(&commands) {
            return execute_parted_command(&mut editor, &cmd);
        }
        eprintln!("Error: Unknown command '{}'", commands[0]);
        return 1;
    }

    0
}

fn execute_parted_command(editor: &mut DiskEditor, cmd: &PartedCommand) -> i32 {
    match cmd {
        PartedCommand::Print { list_all: _, free } => {
            print!("{}", editor.print_table());
            if *free {
                print!("{}", editor.print_free_space());
            }
            0
        }
        PartedCommand::MkLabel { label_type } => {
            match TableType::from_str(label_type) {
                Some(tt) => {
                    editor.create_label(tt);
                    println!("Created {} partition table on {}", tt, editor.disk.path);
                    0
                }
                None => {
                    eprintln!("Error: Unknown label type '{}'", label_type);
                    1
                }
            }
        }
        PartedCommand::MkPart {
            part_type,
            fs_type,
            start,
            end,
        } => {
            let disk_bytes = editor.disk.size_bytes;
            let sector_size = editor.disk.sector_size;
            let start_bytes = match parse_size(start, sector_size, disk_bytes) {
                Some(v) => v,
                None => {
                    eprintln!("Error: Invalid start position '{}'", start);
                    return 1;
                }
            };
            let end_bytes = match parse_size(end, sector_size, disk_bytes) {
                Some(v) => v,
                None => {
                    eprintln!("Error: Invalid end position '{}'", end);
                    return 1;
                }
            };

            match editor.disk.table_type {
                Some(TableType::Gpt) => {
                    match editor.mkpart_gpt(part_type, fs_type, start_bytes, end_bytes) {
                        Ok(n) => {
                            println!("Created partition {}", n);
                            0
                        }
                        Err(e) => {
                            eprintln!("Error: {}", e);
                            1
                        }
                    }
                }
                Some(TableType::Mbr) => {
                    let role = match part_type.to_lowercase().as_str() {
                        "primary" => MbrPartRole::Primary,
                        "extended" | "ext" => MbrPartRole::Extended,
                        "logical" => MbrPartRole::Logical,
                        _ => {
                            eprintln!(
                                "Error: Invalid partition type '{}' (use primary, extended, or logical)",
                                part_type
                            );
                            return 1;
                        }
                    };
                    match editor.mkpart_mbr(role, fs_type, start_bytes, end_bytes) {
                        Ok(n) => {
                            println!("Created partition {}", n);
                            0
                        }
                        Err(e) => {
                            eprintln!("Error: {}", e);
                            1
                        }
                    }
                }
                None => {
                    eprintln!("Error: No partition table. Use mklabel first.");
                    1
                }
            }
        }
        PartedCommand::Rm { number } => {
            match editor.rm_partition(*number) {
                Ok(()) => {
                    println!("Removed partition {}", number);
                    0
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    1
                }
            }
        }
        PartedCommand::Name { number, name } => {
            match editor.name_partition(*number, name) {
                Ok(()) => {
                    println!("Set name of partition {} to '{}'", number, name);
                    0
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    1
                }
            }
        }
        PartedCommand::Set {
            number,
            flag,
            state,
        } => {
            let pf = match PartitionFlag::from_str(flag) {
                Some(f) => f,
                None => {
                    eprintln!("Error: Unknown flag '{}'", flag);
                    return 1;
                }
            };
            let on = match state.to_lowercase().as_str() {
                "on" | "1" | "true" | "yes" => true,
                "off" | "0" | "false" | "no" => false,
                _ => {
                    eprintln!("Error: Invalid state '{}' (use on/off)", state);
                    return 1;
                }
            };
            match editor.set_flag(*number, pf, on) {
                Ok(()) => 0,
                Err(e) => {
                    eprintln!("Error: {}", e);
                    1
                }
            }
        }
        PartedCommand::Toggle { number, flag } => {
            let pf = match PartitionFlag::from_str(flag) {
                Some(f) => f,
                None => {
                    eprintln!("Error: Unknown flag '{}'", flag);
                    return 1;
                }
            };
            match editor.toggle_flag(*number, pf) {
                Ok(()) => 0,
                Err(e) => {
                    eprintln!("Error: {}", e);
                    1
                }
            }
        }
        PartedCommand::ResizePart { number, end } => {
            let disk_bytes = editor.disk.size_bytes;
            let sector_size = editor.disk.sector_size;
            let end_bytes = match parse_size(end, sector_size, disk_bytes) {
                Some(v) => v,
                None => {
                    eprintln!("Error: Invalid end position '{}'", end);
                    return 1;
                }
            };
            match editor.resize_partition(*number, end_bytes) {
                Ok(()) => {
                    println!("Resized partition {}", number);
                    0
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    1
                }
            }
        }
        PartedCommand::Move { number, start } => {
            let disk_bytes = editor.disk.size_bytes;
            let sector_size = editor.disk.sector_size;
            let start_bytes = match parse_size(start, sector_size, disk_bytes) {
                Some(v) => v,
                None => {
                    eprintln!("Error: Invalid start position '{}'", start);
                    return 1;
                }
            };
            match editor.move_partition(*number, start_bytes) {
                Ok(()) => {
                    println!("Moved partition {}", number);
                    0
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    1
                }
            }
        }
        PartedCommand::Unit { unit } => {
            match DisplayUnit::from_str(unit) {
                Some(u) => {
                    editor.unit = u;
                    0
                }
                None => {
                    eprintln!("Error: Unknown unit '{}'", unit);
                    1
                }
            }
        }
        PartedCommand::AlignCheck {
            align_type,
            number,
        } => {
            match editor.check_alignment(*number, align_type) {
                Ok(aligned) => {
                    if aligned {
                        println!("{} aligned", number);
                    } else {
                        println!("{} not aligned", number);
                    }
                    if aligned { 0 } else { 1 }
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    1
                }
            }
        }
        PartedCommand::Select { device } => {
            println!("Using {}", device);
            0
        }
        PartedCommand::Help { command } => {
            print!("{}", print_help(command.as_deref()));
            0
        }
        PartedCommand::Version => {
            println!("parted (OurOS) 1.0.0");
            0
        }
        PartedCommand::Quit => 0,
    }
}

// ============================================================================
// partprobe personality
// ============================================================================

fn run_partprobe(args: &[String]) -> i32 {
    let mut devices: Vec<String> = Vec::new();
    let mut summary = false;

    for arg in args {
        match arg.as_str() {
            "-s" | "--summary" => summary = true,
            "-h" | "--help" => {
                println!("Usage: partprobe [-s] [DEVICE...]");
                println!("Inform the OS of partition table changes.");
                println!();
                println!("Options:");
                println!("  -s, --summary   Show a summary of devices and their partitions");
                println!("  -h, --help      Display this help");
                return 0;
            }
            "--version" => {
                println!("partprobe (OurOS) 1.0.0");
                return 0;
            }
            _ => {
                if !arg.starts_with('-') {
                    devices.push(arg.clone());
                }
            }
        }
    }

    if devices.is_empty() {
        // Scan all block devices
        devices.push("/dev/sda".to_string());
        devices.push("/dev/sdb".to_string());
    }

    for device in &devices {
        if summary {
            // In a real implementation, read the device partition table.
            // For now, show the format.
            println!("{}: unknown partition table", device);
        } else {
            // Silently inform kernel - on OurOS this would be an IPC message
            // to the block device service
            println!("Informing kernel about changes to {}", device);
        }
    }

    0
}

// ============================================================================
// partx personality
// ============================================================================

#[derive(Debug)]
enum PartxAction {
    Show,
    Add,
    Delete,
    Update,
}

fn run_partx(args: &[String]) -> i32 {
    let mut action = PartxAction::Show;
    let mut device: Option<String> = None;
    let mut nr_range: Option<String> = None;
    let mut output_cols: Option<String> = None;
    let mut verbose = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-s" | "--show" | "-l" => action = PartxAction::Show,
            "-a" | "--add" => action = PartxAction::Add,
            "-d" | "--delete" => action = PartxAction::Delete,
            "-u" | "--update" => action = PartxAction::Update,
            "-v" | "--verbose" => verbose = true,
            "--nr" | "-n" => {
                i += 1;
                if i < args.len() {
                    nr_range = Some(args[i].clone());
                }
            }
            "-o" | "--output" => {
                i += 1;
                if i < args.len() {
                    output_cols = Some(args[i].clone());
                }
            }
            "-h" | "--help" => {
                println!("Usage: partx [OPTION]... [DEVICE]");
                println!("Tell the kernel about the presence and numbering of partitions.");
                println!();
                println!("Options:");
                println!("  -s, --show              Show partitions");
                println!("  -a, --add               Add partitions to kernel");
                println!("  -d, --delete            Delete partitions from kernel");
                println!("  -u, --update            Update kernel partition table");
                println!("  -n, --nr M:N            Specify partition range");
                println!("  -o, --output COLUMNS    Select output columns");
                println!("  -v, --verbose           Verbose output");
                println!("  -h, --help              Display this help");
                println!();
                println!("Available output columns:");
                println!("  NR, START, END, SECTORS, SIZE, NAME, UUID, TYPE, FLAGS, SCHEME");
                return 0;
            }
            "--version" => {
                println!("partx (OurOS) 1.0.0");
                return 0;
            }
            _ => {
                if !args[i].starts_with('-') {
                    device = Some(args[i].clone());
                }
            }
        }
        i += 1;
    }

    let dev = device.unwrap_or_else(|| "/dev/sda".to_string());

    // Parse nr range
    let (nr_start, nr_end) = if let Some(ref range) = nr_range {
        parse_nr_range(range)
    } else {
        (None, None)
    };

    // Determine columns to display
    let columns = if let Some(ref cols) = output_cols {
        cols.split(',')
            .map(|s| s.trim().to_uppercase())
            .collect::<Vec<_>>()
    } else {
        vec![
            "NR".to_string(),
            "START".to_string(),
            "END".to_string(),
            "SECTORS".to_string(),
            "SIZE".to_string(),
            "NAME".to_string(),
            "UUID".to_string(),
            "TYPE".to_string(),
            "FLAGS".to_string(),
            "SCHEME".to_string(),
        ]
    };

    match action {
        PartxAction::Show => {
            if verbose {
                println!("Showing partitions of {}", dev);
            }
            // Print header
            let header: Vec<&str> = columns.iter().map(|s| s.as_str()).collect();
            println!("{}", header.join("  "));

            // In a real implementation, we would read the device.
            // Show format with placeholder data.
            println!(
                "{}",
                format_partx_row(
                    &columns,
                    &PartxRow {
                        nr: 1,
                        start: 2048,
                        end: 1026047,
                        sectors: 1024000,
                        name: "EFI System",
                        uuid: "",
                        type_str: "C12A7328-...",
                        flags: "",
                        scheme: "gpt",
                    },
                )
            );

            // Filter by nr range
            if let (Some(start), Some(end)) = (nr_start, nr_end)
                && verbose {
                    println!("Filtering partitions {} to {}", start, end);
                }
            0
        }
        PartxAction::Add => {
            println!("Adding partitions of {} to kernel", dev);
            if let (Some(start), Some(end)) = (nr_start, nr_end) {
                println!("Adding partitions {} to {}", start, end);
            }
            0
        }
        PartxAction::Delete => {
            println!("Deleting partitions of {} from kernel", dev);
            if let (Some(start), Some(end)) = (nr_start, nr_end) {
                println!("Deleting partitions {} to {}", start, end);
            }
            0
        }
        PartxAction::Update => {
            println!("Updating kernel partition table for {}", dev);
            0
        }
    }
}

fn parse_nr_range(range: &str) -> (Option<u32>, Option<u32>) {
    if let Some(idx) = range.find(':') {
        let start = range[..idx].parse::<u32>().ok();
        let end = range[idx + 1..].parse::<u32>().ok();
        (start, end)
    } else if let Some(idx) = range.find('-') {
        let start = range[..idx].parse::<u32>().ok();
        let end = range[idx + 1..].parse::<u32>().ok();
        (start, end)
    } else {
        let n = range.parse::<u32>().ok();
        (n, n)
    }
}

/// Column values for a single `partx` output row.  Grouping the per-row
/// fields keeps `format_partx_row` to two parameters (columns + row).
struct PartxRow<'a> {
    nr: u32,
    start: u64,
    end: u64,
    sectors: u64,
    name: &'a str,
    uuid: &'a str,
    type_str: &'a str,
    flags: &'a str,
    scheme: &'a str,
}

fn format_partx_row(columns: &[String], row: &PartxRow) -> String {
    let mut parts = Vec::new();
    for col in columns {
        match col.as_str() {
            "NR" => parts.push(format!("{:>3}", row.nr)),
            "START" => parts.push(format!("{:>10}", row.start)),
            "END" => parts.push(format!("{:>10}", row.end)),
            "SECTORS" => parts.push(format!("{:>10}", row.sectors)),
            "SIZE" => parts.push(format!("{:>8}", format_compact(row.sectors * SECTOR_SIZE))),
            "NAME" => parts.push(format!("{:<16}", row.name)),
            "UUID" => parts.push(format!("{:<38}", row.uuid)),
            "TYPE" => parts.push(format!("{:<16}", row.type_str)),
            "FLAGS" => parts.push(format!("{:<10}", row.flags)),
            "SCHEME" => parts.push(format!("{:<6}", row.scheme)),
            _ => parts.push("?".to_string()),
        }
    }
    parts.join("  ")
}

// ============================================================================
// Main entry point
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("parted");
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' {
                last_sep = i + 1;
            }
        }
        let base = &s[last_sep..];
        let base = base.strip_suffix(".exe").unwrap_or(base);
        base.to_string()
    };

    let rest = &args[1..];

    let exit_code = match prog_name.as_str() {
        "partprobe" => run_partprobe(rest),
        "partx" => run_partx(rest),
        _ => run_parted(rest),
    };

    process::exit(exit_code);
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ---- CRC32 Tests ----

    #[test]
    fn test_crc32_empty() {
        assert_eq!(crc32(&[]), 0x0000_0000);
    }

    #[test]
    fn test_crc32_known_value() {
        // CRC32 of "123456789" is 0xCBF43926
        let data = b"123456789";
        assert_eq!(crc32(data), 0xCBF4_3926);
    }

    #[test]
    fn test_crc32_single_byte() {
        assert_ne!(crc32(&[0x00]), 0);
        assert_ne!(crc32(&[0xFF]), 0);
    }

    #[test]
    fn test_crc32_all_zeros() {
        let data = [0u8; 16];
        let c = crc32(&data);
        assert_ne!(c, 0);
    }

    #[test]
    fn test_crc32_different_data_different_hash() {
        let a = crc32(b"hello");
        let b = crc32(b"world");
        assert_ne!(a, b);
    }

    #[test]
    fn test_crc32_table_size() {
        assert_eq!(CRC32_TABLE.len(), 256);
    }

    #[test]
    fn test_crc32_deterministic() {
        let data = b"test data for crc";
        assert_eq!(crc32(data), crc32(data));
    }

    // ---- GUID Tests ----

    #[test]
    fn test_guid_zero() {
        let g = Guid::ZERO;
        assert!(g.is_zero());
    }

    #[test]
    fn test_guid_from_bytes_roundtrip() {
        let bytes = [1u8, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
        let g = Guid::from_bytes_le(&bytes).unwrap();
        let out = g.to_bytes_le();
        assert_eq!(out, bytes);
    }

    #[test]
    fn test_guid_from_bytes_too_short() {
        assert!(Guid::from_bytes_le(&[1, 2, 3]).is_none());
    }

    #[test]
    fn test_guid_display() {
        let g = Guid {
            data1: 0xC12A7328,
            data2: 0xF81F,
            data3: 0x11D2,
            data4: [0xBA, 0x4B, 0x00, 0xA0, 0xC9, 0x3E, 0xC9, 0x3B],
        };
        let s = format!("{}", g);
        assert!(s.contains("C12A7328"));
        assert!(s.contains("F81F"));
    }

    #[test]
    fn test_guid_from_str_hex() {
        let g = Guid::from_str_hex("C12A7328-F81F-11D2-BA4B-00A0C93EC93B");
        assert!(g.is_some());
        let g = g.unwrap();
        assert!(!g.is_zero());
    }

    #[test]
    fn test_guid_from_str_hex_invalid() {
        assert!(Guid::from_str_hex("not-a-guid").is_none());
        assert!(Guid::from_str_hex("").is_none());
        assert!(Guid::from_str_hex("ZZZZZZZZ-ZZZZ-ZZZZ-ZZZZ-ZZZZZZZZZZZZ").is_none());
    }

    #[test]
    fn test_guid_equality() {
        let a = Guid::from_str_hex("C12A7328-F81F-11D2-BA4B-00A0C93EC93B").unwrap();
        let b = Guid::from_str_hex("C12A7328-F81F-11D2-BA4B-00A0C93EC93B").unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn test_guid_inequality() {
        let a = Guid::from_str_hex("C12A7328-F81F-11D2-BA4B-00A0C93EC93B").unwrap();
        let b = Guid::from_str_hex("0FC63DAF-8483-4772-8E79-3D69D8477DE4").unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn test_guid_generate() {
        let a = Guid::generate(42);
        let b = Guid::generate(43);
        assert_ne!(a, b);
        assert!(!a.is_zero());
    }

    #[test]
    fn test_guid_generate_deterministic() {
        let a = Guid::generate(100);
        let b = Guid::generate(100);
        assert_eq!(a, b);
    }

    #[test]
    fn test_guid_debug() {
        let g = Guid::ZERO;
        let s = format!("{:?}", g);
        assert!(s.contains("Guid"));
    }

    #[test]
    fn test_guid_clone() {
        let a = Guid::generate(55);
        let b = a;
        assert_eq!(a, b);
    }

    // ---- Well-known GUID Tests ----

    #[test]
    fn test_guid_to_name_efi() {
        let efi = Guid::from_str_hex("C12A7328-F81F-11D2-BA4B-00A0C93EC93B").unwrap();
        assert_eq!(guid_to_name(&efi), "EFI System");
    }

    #[test]
    fn test_guid_to_name_linux() {
        let linux = Guid::from_str_hex("0FC63DAF-8483-4772-8E79-3D69D8477DE4").unwrap();
        assert_eq!(guid_to_name(&linux), "Linux filesystem");
    }

    #[test]
    fn test_guid_to_name_unknown() {
        let unknown = Guid::generate(999999);
        assert_eq!(guid_to_name(&unknown), "unknown");
    }

    #[test]
    fn test_name_to_guid_linux() {
        let g = name_to_guid("linux");
        assert!(g.is_some());
    }

    #[test]
    fn test_name_to_guid_swap() {
        let g = name_to_guid("swap");
        assert!(g.is_some());
    }

    #[test]
    fn test_name_to_guid_efi() {
        let g = name_to_guid("efi");
        assert!(g.is_some());
    }

    #[test]
    fn test_name_to_guid_fat32() {
        let g = name_to_guid("fat32");
        assert!(g.is_some());
    }

    #[test]
    fn test_name_to_guid_unknown() {
        assert!(name_to_guid("nonexistent_type_xyz").is_none());
    }

    #[test]
    fn test_name_to_guid_case_insensitive() {
        let a = name_to_guid("Linux");
        let b = name_to_guid("linux");
        assert_eq!(a, b);
    }

    #[test]
    fn test_name_to_guid_lvm() {
        assert!(name_to_guid("lvm").is_some());
    }

    #[test]
    fn test_name_to_guid_raid() {
        assert!(name_to_guid("raid").is_some());
    }

    #[test]
    fn test_name_to_guid_bios_grub() {
        assert!(name_to_guid("bios_grub").is_some());
    }

    #[test]
    fn test_name_to_guid_msftres() {
        assert!(name_to_guid("msftres").is_some());
    }

    // ---- MBR Type Tests ----

    #[test]
    fn test_mbr_type_name_linux() {
        assert_eq!(mbr_type_name(0x83), "Linux");
    }

    #[test]
    fn test_mbr_type_name_swap() {
        assert_eq!(mbr_type_name(0x82), "Linux swap");
    }

    #[test]
    fn test_mbr_type_name_ntfs() {
        assert_eq!(mbr_type_name(0x07), "HPFS/NTFS");
    }

    #[test]
    fn test_mbr_type_name_fat32() {
        assert_eq!(mbr_type_name(0x0C), "W95 FAT32 (LBA)");
    }

    #[test]
    fn test_mbr_type_name_gpt() {
        assert_eq!(mbr_type_name(0xEE), "GPT protective");
    }

    #[test]
    fn test_mbr_type_name_efi() {
        assert_eq!(mbr_type_name(0xEF), "EFI System");
    }

    #[test]
    fn test_mbr_type_name_unknown() {
        assert_eq!(mbr_type_name(0xFF), "Unknown");
    }

    #[test]
    fn test_mbr_type_code_linux() {
        assert_eq!(mbr_type_code("linux"), Some(0x83));
    }

    #[test]
    fn test_mbr_type_code_swap() {
        assert_eq!(mbr_type_code("swap"), Some(0x82));
    }

    #[test]
    fn test_mbr_type_code_fat32() {
        assert_eq!(mbr_type_code("fat32"), Some(0x0C));
    }

    #[test]
    fn test_mbr_type_code_hex() {
        assert_eq!(mbr_type_code("0x83"), Some(0x83));
        assert_eq!(mbr_type_code("0xEF"), Some(0xEF));
    }

    #[test]
    fn test_mbr_type_code_ntfs() {
        assert_eq!(mbr_type_code("ntfs"), Some(0x07));
    }

    #[test]
    fn test_mbr_type_code_unknown() {
        assert!(mbr_type_code("nonexistent_xyz").is_none());
    }

    // ---- Size Parsing Tests ----

    #[test]
    fn test_parse_size_bytes() {
        assert_eq!(parse_size("1024B", 512, 1_000_000), Some(1024));
    }

    #[test]
    fn test_parse_size_sectors() {
        assert_eq!(parse_size("100s", 512, 1_000_000), Some(51200));
    }

    #[test]
    fn test_parse_size_kb() {
        assert_eq!(parse_size("1kB", 512, 1_000_000), Some(1000));
    }

    #[test]
    fn test_parse_size_mb() {
        assert_eq!(parse_size("1MB", 512, 1_000_000_000), Some(1_000_000));
    }

    #[test]
    fn test_parse_size_gb() {
        assert_eq!(
            parse_size("1GB", 512, 100_000_000_000),
            Some(1_000_000_000)
        );
    }

    #[test]
    fn test_parse_size_tb() {
        assert_eq!(
            parse_size("1TB", 512, 10_000_000_000_000),
            Some(1_000_000_000_000)
        );
    }

    #[test]
    fn test_parse_size_mib() {
        assert_eq!(parse_size("1MiB", 512, 1_000_000_000), Some(1_048_576));
    }

    #[test]
    fn test_parse_size_gib() {
        assert_eq!(
            parse_size("1GiB", 512, 100_000_000_000),
            Some(1_073_741_824)
        );
    }

    #[test]
    fn test_parse_size_percent() {
        assert_eq!(
            parse_size("50%", 512, 1_000_000_000),
            Some(500_000_000)
        );
    }

    #[test]
    fn test_parse_size_percent_100() {
        assert_eq!(
            parse_size("100%", 512, 500_000_000_000),
            Some(500_000_000_000)
        );
    }

    #[test]
    fn test_parse_size_percent_zero() {
        assert_eq!(parse_size("0%", 512, 1_000_000_000), Some(0));
    }

    #[test]
    fn test_parse_size_empty() {
        assert!(parse_size("", 512, 1_000_000).is_none());
    }

    #[test]
    fn test_parse_size_invalid() {
        assert!(parse_size("abc", 512, 1_000_000).is_none());
    }

    #[test]
    fn test_parse_size_fractional() {
        let result = parse_size("1.5GB", 512, 100_000_000_000);
        assert!(result.is_some());
        assert_eq!(result.unwrap(), 1_500_000_000);
    }

    #[test]
    fn test_parse_size_no_suffix() {
        // Plain number should be treated as bytes
        assert_eq!(parse_size("1024", 512, 1_000_000), Some(1024));
    }

    // ---- Format Size Tests ----

    #[test]
    fn test_format_size_sectors() {
        let s = format_size(1_048_576, DisplayUnit::Sectors, 1_000_000_000, 512);
        assert_eq!(s, "2048s");
    }

    #[test]
    fn test_format_size_bytes() {
        let s = format_size(1024, DisplayUnit::Bytes, 1_000_000_000, 512);
        assert_eq!(s, "1024B");
    }

    #[test]
    fn test_format_size_mb() {
        let s = format_size(1_000_000, DisplayUnit::MB, 1_000_000_000, 512);
        assert_eq!(s, "1.0MB");
    }

    #[test]
    fn test_format_size_gb() {
        let s = format_size(1_000_000_000, DisplayUnit::GB, 10_000_000_000, 512);
        assert_eq!(s, "1.00GB");
    }

    #[test]
    fn test_format_size_percent() {
        let s = format_size(500_000_000, DisplayUnit::Percent, 1_000_000_000, 512);
        assert_eq!(s, "50.0%");
    }

    #[test]
    fn test_format_size_compact_bytes() {
        assert_eq!(format_compact(500), "500B");
    }

    #[test]
    fn test_format_compact_kb() {
        let s = format_compact(5000);
        assert!(s.contains("kB"));
    }

    #[test]
    fn test_format_compact_mb() {
        let s = format_compact(5_000_000);
        assert!(s.contains("MB"));
    }

    #[test]
    fn test_format_compact_gb() {
        let s = format_compact(5_000_000_000);
        assert!(s.contains("GB"));
    }

    #[test]
    fn test_format_compact_tb() {
        let s = format_compact(5_000_000_000_000);
        assert!(s.contains("TB"));
    }

    // ---- Alignment Tests ----

    #[test]
    fn test_align_up_already_aligned() {
        assert_eq!(align_up(2048, 2048), 2048);
    }

    #[test]
    fn test_align_up_not_aligned() {
        assert_eq!(align_up(100, 2048), 2048);
    }

    #[test]
    fn test_align_up_zero_alignment() {
        assert_eq!(align_up(100, 0), 100);
    }

    #[test]
    fn test_align_down_already_aligned() {
        assert_eq!(align_down(2048, 2048), 2048);
    }

    #[test]
    fn test_align_down_not_aligned() {
        assert_eq!(align_down(3000, 2048), 2048);
    }

    #[test]
    fn test_align_down_zero_alignment() {
        assert_eq!(align_down(100, 0), 100);
    }

    #[test]
    fn test_bytes_to_sectors_ceil() {
        assert_eq!(bytes_to_sectors_ceil(512, 512), 1);
        assert_eq!(bytes_to_sectors_ceil(513, 512), 2);
        assert_eq!(bytes_to_sectors_ceil(1024, 512), 2);
    }

    #[test]
    fn test_bytes_to_sectors_floor() {
        assert_eq!(bytes_to_sectors_floor(512, 512), 1);
        assert_eq!(bytes_to_sectors_floor(513, 512), 1);
        assert_eq!(bytes_to_sectors_floor(1023, 512), 1);
    }

    // ---- Display Unit Tests ----

    #[test]
    fn test_display_unit_from_str() {
        assert_eq!(DisplayUnit::from_str("s"), Some(DisplayUnit::Sectors));
        assert_eq!(DisplayUnit::from_str("B"), Some(DisplayUnit::Bytes));
        assert_eq!(DisplayUnit::from_str("MB"), Some(DisplayUnit::MB));
        assert_eq!(DisplayUnit::from_str("GB"), Some(DisplayUnit::GB));
        assert_eq!(DisplayUnit::from_str("TB"), Some(DisplayUnit::TB));
        assert_eq!(DisplayUnit::from_str("%"), Some(DisplayUnit::Percent));
        assert_eq!(DisplayUnit::from_str("compact"), Some(DisplayUnit::Compact));
    }

    #[test]
    fn test_display_unit_from_str_invalid() {
        assert!(DisplayUnit::from_str("xyz").is_none());
    }

    #[test]
    fn test_display_unit_suffix() {
        assert_eq!(DisplayUnit::Sectors.suffix(), "s");
        assert_eq!(DisplayUnit::Bytes.suffix(), "B");
        assert_eq!(DisplayUnit::MB.suffix(), "MB");
    }

    #[test]
    fn test_display_unit_case_insensitive() {
        assert_eq!(DisplayUnit::from_str("mb"), Some(DisplayUnit::MB));
        assert_eq!(DisplayUnit::from_str("gb"), Some(DisplayUnit::GB));
    }

    // ---- Partition Flag Tests ----

    #[test]
    fn test_partition_flag_from_str() {
        assert_eq!(
            PartitionFlag::from_str("boot"),
            Some(PartitionFlag::Boot)
        );
        assert_eq!(PartitionFlag::from_str("esp"), Some(PartitionFlag::Esp));
        assert_eq!(
            PartitionFlag::from_str("swap"),
            Some(PartitionFlag::Swap)
        );
        assert_eq!(
            PartitionFlag::from_str("raid"),
            Some(PartitionFlag::Raid)
        );
        assert_eq!(PartitionFlag::from_str("lvm"), Some(PartitionFlag::Lvm));
    }

    #[test]
    fn test_partition_flag_from_str_invalid() {
        assert!(PartitionFlag::from_str("nonexistent").is_none());
    }

    #[test]
    fn test_partition_flag_name() {
        assert_eq!(PartitionFlag::Boot.name(), "boot");
        assert_eq!(PartitionFlag::Esp.name(), "esp");
        assert_eq!(PartitionFlag::BiosGrub.name(), "bios_grub");
    }

    #[test]
    fn test_partition_flag_all() {
        let all = PartitionFlag::all();
        assert!(all.len() >= 10);
        assert!(all.contains(&PartitionFlag::Boot));
        assert!(all.contains(&PartitionFlag::Esp));
    }

    #[test]
    fn test_partition_flag_hidden() {
        assert_eq!(
            PartitionFlag::from_str("hidden"),
            Some(PartitionFlag::Hidden)
        );
    }

    #[test]
    fn test_partition_flag_msftdata() {
        assert_eq!(
            PartitionFlag::from_str("msftdata"),
            Some(PartitionFlag::MsftData)
        );
    }

    #[test]
    fn test_partition_flag_bios_grub() {
        assert_eq!(
            PartitionFlag::from_str("bios_grub"),
            Some(PartitionFlag::BiosGrub)
        );
    }

    // ---- GPT Header Tests ----

    #[test]
    fn test_gpt_header_new() {
        let h = GptHeader::new(2_000_000);
        assert_eq!(h.revision, GPT_REVISION_1_0);
        assert_eq!(h.my_lba, 1);
        assert!(h.first_usable_lba > 1);
        assert!(h.last_usable_lba < 2_000_000);
        assert!(h.first_usable_lba < h.last_usable_lba);
    }

    #[test]
    fn test_gpt_header_serialize_size() {
        let h = GptHeader::new(1_000_000);
        let data = h.serialize();
        assert_eq!(data.len(), GPT_HEADER_SIZE as usize);
    }

    #[test]
    fn test_gpt_header_serialize_signature() {
        let h = GptHeader::new(1_000_000);
        let data = h.serialize();
        let sig = u64::from_le_bytes([
            data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
        ]);
        assert_eq!(sig, GPT_SIGNATURE);
    }

    #[test]
    fn test_gpt_header_roundtrip() {
        let h = GptHeader::new(2_000_000);
        let data = h.serialize_with_crc();
        let parsed = GptHeader::parse(&data);
        assert!(parsed.is_some());
        let p = parsed.unwrap();
        assert_eq!(p.revision, h.revision);
        assert_eq!(p.my_lba, h.my_lba);
        assert_eq!(p.first_usable_lba, h.first_usable_lba);
        assert_eq!(p.last_usable_lba, h.last_usable_lba);
        assert_eq!(p.disk_guid, h.disk_guid);
    }

    #[test]
    fn test_gpt_header_parse_invalid() {
        let data = vec![0u8; 92];
        assert!(GptHeader::parse(&data).is_none());
    }

    #[test]
    fn test_gpt_header_parse_too_short() {
        let data = vec![0u8; 10];
        assert!(GptHeader::parse(&data).is_none());
    }

    #[test]
    fn test_gpt_header_crc_validates() {
        let h = GptHeader::new(1_000_000);
        let mut data = h.serialize_with_crc();
        // Corrupt a byte
        data[50] ^= 0xFF;
        // Should fail CRC check
        assert!(GptHeader::parse(&data).is_none());
    }

    #[test]
    fn test_gpt_header_alternate_lba() {
        let h = GptHeader::new(2_000_000);
        assert_eq!(h.alternate_lba, 1_999_999);
    }

    #[test]
    fn test_gpt_header_usable_range() {
        let h = GptHeader::new(2_000_000);
        let usable_sectors = h.last_usable_lba - h.first_usable_lba + 1;
        assert!(usable_sectors > 0);
        assert!(usable_sectors < 2_000_000);
    }

    // ---- GPT Entry Tests ----

    #[test]
    fn test_gpt_entry_new_is_empty() {
        let e = GptEntry::new();
        assert!(e.is_empty());
    }

    #[test]
    fn test_gpt_entry_serialize_size() {
        let e = GptEntry::new();
        assert_eq!(e.serialize().len(), GPT_ENTRY_SIZE as usize);
    }

    #[test]
    fn test_gpt_entry_roundtrip() {
        let e = GptEntry {
            type_guid: Guid::from_str_hex("0FC63DAF-8483-4772-8E79-3D69D8477DE4").unwrap(),
            unique_guid: Guid::generate(42),
            first_lba: 2048,
            last_lba: 1026047,
            attributes: 0,
            name: "Linux root".to_string(),
        };
        let data = e.serialize();
        let parsed = GptEntry::parse(&data).unwrap();
        assert_eq!(parsed.type_guid, e.type_guid);
        assert_eq!(parsed.unique_guid, e.unique_guid);
        assert_eq!(parsed.first_lba, e.first_lba);
        assert_eq!(parsed.last_lba, e.last_lba);
        assert_eq!(parsed.name, e.name);
    }

    #[test]
    fn test_gpt_entry_size_sectors() {
        let e = GptEntry {
            type_guid: Guid::generate(1),
            unique_guid: Guid::generate(2),
            first_lba: 100,
            last_lba: 199,
            attributes: 0,
            name: String::new(),
        };
        assert_eq!(e.size_sectors(), 100);
    }

    #[test]
    fn test_gpt_entry_size_empty() {
        let e = GptEntry::new();
        assert_eq!(e.size_sectors(), 0);
    }

    #[test]
    fn test_gpt_entry_parse_too_short() {
        let data = vec![0u8; 10];
        assert!(GptEntry::parse(&data).is_none());
    }

    #[test]
    fn test_gpt_entry_name_utf16() {
        let e = GptEntry {
            type_guid: Guid::generate(1),
            unique_guid: Guid::generate(2),
            first_lba: 100,
            last_lba: 200,
            attributes: 0,
            name: "EFI System Partition".to_string(),
        };
        let data = e.serialize();
        let parsed = GptEntry::parse(&data).unwrap();
        assert_eq!(parsed.name, "EFI System Partition");
    }

    #[test]
    fn test_gpt_entry_flags_esp() {
        let mut e = GptEntry::new();
        e.type_guid =
            Guid::from_str_hex("C12A7328-F81F-11D2-BA4B-00A0C93EC93B").unwrap();
        assert!(e.has_flag(PartitionFlag::Esp));
        assert!(!e.has_flag(PartitionFlag::Swap));
    }

    #[test]
    fn test_gpt_entry_set_flag_esp() {
        let mut e = GptEntry::new();
        e.set_flag(PartitionFlag::Esp, true);
        assert!(e.has_flag(PartitionFlag::Esp));
    }

    #[test]
    fn test_gpt_entry_set_flag_boot() {
        let mut e = GptEntry::new();
        e.type_guid =
            Guid::from_str_hex("0FC63DAF-8483-4772-8E79-3D69D8477DE4").unwrap();
        e.set_flag(PartitionFlag::Boot, true);
        assert!(e.has_flag(PartitionFlag::Boot));
    }

    #[test]
    fn test_gpt_entry_toggle_boot() {
        let mut e = GptEntry::new();
        e.type_guid =
            Guid::from_str_hex("0FC63DAF-8483-4772-8E79-3D69D8477DE4").unwrap();
        assert!(!e.has_flag(PartitionFlag::Boot));
        e.set_flag(PartitionFlag::Boot, true);
        assert!(e.has_flag(PartitionFlag::Boot));
        e.set_flag(PartitionFlag::Boot, false);
        assert!(!e.has_flag(PartitionFlag::Boot));
    }

    #[test]
    fn test_gpt_entry_flags_list() {
        let mut e = GptEntry::new();
        e.set_flag(PartitionFlag::Esp, true);
        let flags = e.flags_list();
        assert!(flags.contains(&"esp"));
    }

    #[test]
    fn test_gpt_entry_hidden_attribute() {
        let mut e = GptEntry::new();
        e.type_guid = Guid::generate(1);
        e.set_flag(PartitionFlag::Hidden, true);
        assert!(e.has_flag(PartitionFlag::Hidden));
        assert_ne!(e.attributes & (1 << 62), 0);
    }

    // ---- MBR Tests ----

    #[test]
    fn test_mbr_new() {
        let mbr = Mbr::new();
        assert_eq!(mbr.signature, MBR_SIGNATURE);
        for p in &mbr.partitions {
            assert!(p.is_empty());
        }
    }

    #[test]
    fn test_mbr_serialize_size() {
        let mbr = Mbr::new();
        assert_eq!(mbr.serialize().len(), 512);
    }

    #[test]
    fn test_mbr_signature_bytes() {
        let mbr = Mbr::new();
        let data = mbr.serialize();
        assert_eq!(data[510], 0x55);
        assert_eq!(data[511], 0xAA);
    }

    #[test]
    fn test_mbr_roundtrip() {
        let mut mbr = Mbr::new();
        mbr.partitions[0] = MbrPartitionEntry {
            status: 0x80,
            chs_start: [0, 2, 0],
            partition_type: 0x83,
            chs_end: [0xFF, 0xFF, 0xFF],
            lba_start: 2048,
            lba_size: 1024000,
        };
        let data = mbr.serialize();
        let parsed = Mbr::parse(&data).unwrap();
        assert_eq!(parsed.partitions[0].partition_type, 0x83);
        assert_eq!(parsed.partitions[0].lba_start, 2048);
        assert_eq!(parsed.partitions[0].lba_size, 1024000);
        assert!(parsed.partitions[0].is_bootable());
    }

    #[test]
    fn test_mbr_parse_invalid() {
        let data = vec![0u8; 512];
        assert!(Mbr::parse(&data).is_none()); // No signature
    }

    #[test]
    fn test_mbr_parse_too_short() {
        let data = vec![0u8; 100];
        assert!(Mbr::parse(&data).is_none());
    }

    #[test]
    fn test_mbr_protective_gpt() {
        let mbr = Mbr::new_protective(1_000_000);
        assert!(mbr.is_protective_gpt());
        assert_eq!(mbr.partitions[0].partition_type, 0xEE);
    }

    #[test]
    fn test_mbr_protective_large_disk() {
        let mbr = Mbr::new_protective(u64::MAX);
        assert_eq!(mbr.partitions[0].lba_size, u32::MAX);
    }

    #[test]
    fn test_mbr_entry_is_extended() {
        let e = MbrPartitionEntry {
            status: 0,
            chs_start: [0; 3],
            partition_type: 0x05,
            chs_end: [0; 3],
            lba_start: 100,
            lba_size: 200,
        };
        assert!(e.is_extended());
    }

    #[test]
    fn test_mbr_entry_lba_to_chs() {
        let chs = MbrPartitionEntry::lba_to_chs(0);
        assert_eq!(chs, [0, 1, 0]); // head=0, sector=1, cylinder=0
    }

    #[test]
    fn test_mbr_entry_lba_to_chs_overflow() {
        let chs = MbrPartitionEntry::lba_to_chs(u32::MAX);
        assert_eq!(chs, [0xFE, 0xFF, 0xFF]);
    }

    #[test]
    fn test_mbr_entry_serialize_roundtrip() {
        let e = MbrPartitionEntry {
            status: 0x80,
            chs_start: [1, 2, 3],
            partition_type: 0x83,
            chs_end: [4, 5, 6],
            lba_start: 2048,
            lba_size: 500000,
        };
        let data = e.serialize();
        let parsed = MbrPartitionEntry::parse(&data).unwrap();
        assert_eq!(parsed.status, 0x80);
        assert_eq!(parsed.partition_type, 0x83);
        assert_eq!(parsed.lba_start, 2048);
        assert_eq!(parsed.lba_size, 500000);
    }

    // ---- Table Type Detection Tests ----

    #[test]
    fn test_detect_gpt() {
        let mut data = vec![0u8; 1024];
        // MBR signature
        data[510] = 0x55;
        data[511] = 0xAA;
        // GPT signature at LBA 1
        data[512..520].copy_from_slice(&GPT_SIGNATURE.to_le_bytes());
        assert_eq!(DiskInfo::detect_table_type(&data), Some(TableType::Gpt));
    }

    #[test]
    fn test_detect_mbr() {
        let mut data = vec![0u8; 512];
        data[510] = 0x55;
        data[511] = 0xAA;
        assert_eq!(DiskInfo::detect_table_type(&data), Some(TableType::Mbr));
    }

    #[test]
    fn test_detect_no_table() {
        let data = vec![0u8; 512];
        assert_eq!(DiskInfo::detect_table_type(&data), None);
    }

    #[test]
    fn test_table_type_from_str() {
        assert_eq!(TableType::from_str("gpt"), Some(TableType::Gpt));
        assert_eq!(TableType::from_str("msdos"), Some(TableType::Mbr));
        assert_eq!(TableType::from_str("mbr"), Some(TableType::Mbr));
        assert_eq!(TableType::from_str("dos"), Some(TableType::Mbr));
        assert!(TableType::from_str("unknown").is_none());
    }

    #[test]
    fn test_table_type_display() {
        assert_eq!(format!("{}", TableType::Gpt), "gpt");
        assert_eq!(format!("{}", TableType::Mbr), "msdos");
    }

    // ---- Disk Editor Tests ----

    #[test]
    fn test_editor_create_gpt_label() {
        let disk = DiskInfo::new("/dev/sda", 500 * GB);
        let mut editor = DiskEditor::new(disk);
        editor.create_label(TableType::Gpt);
        assert_eq!(editor.disk.table_type, Some(TableType::Gpt));
        assert!(editor.disk.gpt_header.is_some());
        assert!(editor.disk.mbr.is_some());
        assert!(editor.modified);
    }

    #[test]
    fn test_editor_create_mbr_label() {
        let disk = DiskInfo::new("/dev/sda", 500 * GB);
        let mut editor = DiskEditor::new(disk);
        editor.create_label(TableType::Mbr);
        assert_eq!(editor.disk.table_type, Some(TableType::Mbr));
        assert!(editor.disk.mbr.is_some());
        assert!(editor.disk.gpt_header.is_none());
    }

    #[test]
    fn test_editor_mkpart_gpt() {
        let disk = DiskInfo::new("/dev/sda", 500 * GB);
        let mut editor = DiskEditor::new(disk);
        editor.create_label(TableType::Gpt);
        let result = editor.mkpart_gpt("root", "linux", 1 * MIB, 100 * GIB);
        assert!(result.is_ok());
        assert_eq!(editor.disk.partitions.len(), 1);
    }

    #[test]
    fn test_editor_mkpart_gpt_overlap() {
        let disk = DiskInfo::new("/dev/sda", 500 * GB);
        let mut editor = DiskEditor::new(disk);
        editor.create_label(TableType::Gpt);
        let _ = editor.mkpart_gpt("first", "linux", MIB, 100 * GIB);
        let result = editor.mkpart_gpt("second", "linux", 50 * GIB, 200 * GIB);
        assert!(result.is_err());
    }

    #[test]
    fn test_editor_mkpart_mbr() {
        let disk = DiskInfo::new("/dev/sda", 500 * GB);
        let mut editor = DiskEditor::new(disk);
        editor.create_label(TableType::Mbr);
        let result = editor.mkpart_mbr(MbrPartRole::Primary, "linux", MIB, 100 * GIB);
        assert!(result.is_ok());
        assert_eq!(editor.disk.partitions.len(), 1);
    }

    #[test]
    fn test_editor_mkpart_mbr_max_primary() {
        let disk = DiskInfo::new("/dev/sda", 500 * GB);
        let mut editor = DiskEditor::new(disk);
        editor.create_label(TableType::Mbr);
        for i in 0..4u64 {
            let start = MIB + i * 50 * GIB;
            let end = start + 40 * GIB;
            let _ = editor.mkpart_mbr(MbrPartRole::Primary, "linux", start, end);
        }
        // Fifth should fail
        let result = editor.mkpart_mbr(MbrPartRole::Primary, "linux", 201 * GIB, 250 * GIB);
        assert!(result.is_err());
    }

    #[test]
    fn test_editor_rm_partition() {
        let disk = DiskInfo::new("/dev/sda", 500 * GB);
        let mut editor = DiskEditor::new(disk);
        editor.create_label(TableType::Gpt);
        let _ = editor.mkpart_gpt("test", "linux", MIB, 100 * GIB);
        assert_eq!(editor.disk.partitions.len(), 1);
        let result = editor.rm_partition(1);
        assert!(result.is_ok());
        assert_eq!(editor.disk.partitions.len(), 0);
    }

    #[test]
    fn test_editor_rm_nonexistent() {
        let disk = DiskInfo::new("/dev/sda", 500 * GB);
        let mut editor = DiskEditor::new(disk);
        editor.create_label(TableType::Gpt);
        assert!(editor.rm_partition(99).is_err());
    }

    #[test]
    fn test_editor_name_partition() {
        let disk = DiskInfo::new("/dev/sda", 500 * GB);
        let mut editor = DiskEditor::new(disk);
        editor.create_label(TableType::Gpt);
        let _ = editor.mkpart_gpt("old_name", "linux", MIB, 100 * GIB);
        let result = editor.name_partition(1, "new_name");
        assert!(result.is_ok());
        assert_eq!(editor.disk.partitions[0].name, "new_name");
    }

    #[test]
    fn test_editor_name_mbr_fails() {
        let disk = DiskInfo::new("/dev/sda", 500 * GB);
        let mut editor = DiskEditor::new(disk);
        editor.create_label(TableType::Mbr);
        let _ = editor.mkpart_mbr(MbrPartRole::Primary, "linux", MIB, 100 * GIB);
        assert!(editor.name_partition(1, "test").is_err());
    }

    #[test]
    fn test_editor_set_flag() {
        let disk = DiskInfo::new("/dev/sda", 500 * GB);
        let mut editor = DiskEditor::new(disk);
        editor.create_label(TableType::Gpt);
        let _ = editor.mkpart_gpt("root", "linux", MIB, 100 * GIB);
        let result = editor.set_flag(1, PartitionFlag::Boot, true);
        assert!(result.is_ok());
        assert!(editor.disk.partitions[0].flags.contains(&PartitionFlag::Boot));
    }

    #[test]
    fn test_editor_toggle_flag() {
        let disk = DiskInfo::new("/dev/sda", 500 * GB);
        let mut editor = DiskEditor::new(disk);
        editor.create_label(TableType::Gpt);
        let _ = editor.mkpart_gpt("root", "linux", MIB, 100 * GIB);
        let _ = editor.toggle_flag(1, PartitionFlag::Boot);
        assert!(editor.disk.partitions[0].flags.contains(&PartitionFlag::Boot));
        let _ = editor.toggle_flag(1, PartitionFlag::Boot);
        assert!(!editor.disk.partitions[0].flags.contains(&PartitionFlag::Boot));
    }

    #[test]
    fn test_editor_resize_partition() {
        let disk = DiskInfo::new("/dev/sda", 500 * GB);
        let mut editor = DiskEditor::new(disk);
        editor.create_label(TableType::Gpt);
        let _ = editor.mkpart_gpt("root", "linux", MIB, 100 * GIB);
        let old_end = editor.disk.partitions[0].last_lba;
        let result = editor.resize_partition(1, 200 * GIB);
        assert!(result.is_ok());
        assert!(editor.disk.partitions[0].last_lba > old_end);
    }

    #[test]
    fn test_editor_move_partition() {
        let disk = DiskInfo::new("/dev/sda", 500 * GB);
        let mut editor = DiskEditor::new(disk);
        editor.create_label(TableType::Gpt);
        let _ = editor.mkpart_gpt("root", "linux", MIB, 10 * GIB);
        let result = editor.move_partition(1, 200 * GIB);
        assert!(result.is_ok());
        assert!(editor.disk.partitions[0].first_lba > 2048);
    }

    #[test]
    fn test_editor_check_alignment_optimal() {
        let disk = DiskInfo::new("/dev/sda", 500 * GB);
        let mut editor = DiskEditor::new(disk);
        editor.create_label(TableType::Gpt);
        let _ = editor.mkpart_gpt("root", "linux", MIB, 100 * GIB);
        // Should be aligned since mkpart aligns automatically
        let result = editor.check_alignment(1, "optimal");
        assert!(result.is_ok());
        assert!(result.unwrap());
    }

    #[test]
    fn test_editor_find_free_regions_empty() {
        let disk = DiskInfo::new("/dev/sda", 500 * GB);
        let mut editor = DiskEditor::new(disk);
        editor.create_label(TableType::Gpt);
        let free = editor.find_free_regions();
        assert!(!free.is_empty());
    }

    #[test]
    fn test_editor_find_free_regions_with_partition() {
        let disk = DiskInfo::new("/dev/sda", 500 * GB);
        let mut editor = DiskEditor::new(disk);
        editor.create_label(TableType::Gpt);
        let _ = editor.mkpart_gpt("root", "linux", MIB, 100 * GIB);
        let free = editor.find_free_regions();
        // Should have free space before and after the partition
        assert!(!free.is_empty());
    }

    #[test]
    fn test_editor_print_table() {
        let disk = DiskInfo::new("/dev/sda", 500 * GB);
        let mut editor = DiskEditor::new(disk);
        editor.create_label(TableType::Gpt);
        let _ = editor.mkpart_gpt("root", "linux", MIB, 100 * GIB);
        let output = editor.print_table();
        assert!(output.contains("/dev/sda"));
        assert!(output.contains("gpt"));
        assert!(output.contains("root"));
    }

    #[test]
    fn test_editor_print_free_space() {
        let disk = DiskInfo::new("/dev/sda", 500 * GB);
        let mut editor = DiskEditor::new(disk);
        editor.create_label(TableType::Gpt);
        let output = editor.print_free_space();
        assert!(!output.is_empty());
    }

    #[test]
    fn test_editor_serialize_gpt() {
        let disk = DiskInfo::new("/dev/sda", 500 * GB);
        let mut editor = DiskEditor::new(disk);
        editor.create_label(TableType::Gpt);
        let _ = editor.mkpart_gpt("root", "linux", MIB, 100 * GIB);
        let data = editor.serialize_table();
        assert!(!data.is_empty());
        // Should have MBR signature
        assert_eq!(data[510], 0x55);
        assert_eq!(data[511], 0xAA);
        // Should have GPT signature
        let gpt_sig = u64::from_le_bytes([
            data[512], data[513], data[514], data[515], data[516], data[517], data[518],
            data[519],
        ]);
        assert_eq!(gpt_sig, GPT_SIGNATURE);
    }

    #[test]
    fn test_editor_serialize_mbr() {
        let disk = DiskInfo::new("/dev/sda", 500 * GB);
        let mut editor = DiskEditor::new(disk);
        editor.create_label(TableType::Mbr);
        let data = editor.serialize_table();
        assert_eq!(data.len(), 512);
        assert_eq!(data[510], 0x55);
        assert_eq!(data[511], 0xAA);
    }

    #[test]
    fn test_editor_set_unit() {
        let disk = DiskInfo::new("/dev/sda", 500 * GB);
        let mut editor = DiskEditor::new(disk);
        editor.unit = DisplayUnit::Sectors;
        assert_eq!(editor.unit, DisplayUnit::Sectors);
    }

    #[test]
    fn test_editor_multiple_partitions() {
        let disk = DiskInfo::new("/dev/sda", 500 * GB);
        let mut editor = DiskEditor::new(disk);
        editor.create_label(TableType::Gpt);
        let _ = editor.mkpart_gpt("efi", "fat32", MIB, 512 * MB);
        let _ = editor.mkpart_gpt("root", "linux", GIB, 100 * GIB);
        let _ = editor.mkpart_gpt("home", "linux", 100 * GIB + GIB, 400 * GIB);
        assert_eq!(editor.disk.partitions.len(), 3);
    }

    // ---- Command Parsing Tests ----

    #[test]
    fn test_parse_command_print() {
        let args = vec!["print".to_string()];
        let cmd = parse_parted_command(&args);
        assert!(matches!(cmd, Some(PartedCommand::Print { .. })));
    }

    #[test]
    fn test_parse_command_print_free() {
        let args = vec!["print".to_string(), "free".to_string()];
        if let Some(PartedCommand::Print { free, .. }) = parse_parted_command(&args) {
            assert!(free);
        } else {
            panic!("Expected Print command");
        }
    }

    #[test]
    fn test_parse_command_print_list() {
        let args = vec!["print".to_string(), "-l".to_string()];
        if let Some(PartedCommand::Print { list_all, .. }) = parse_parted_command(&args) {
            assert!(list_all);
        } else {
            panic!("Expected Print command");
        }
    }

    #[test]
    fn test_parse_command_mklabel() {
        let args = vec!["mklabel".to_string(), "gpt".to_string()];
        if let Some(PartedCommand::MkLabel { label_type }) = parse_parted_command(&args) {
            assert_eq!(label_type, "gpt");
        } else {
            panic!("Expected MkLabel command");
        }
    }

    #[test]
    fn test_parse_command_mktable() {
        let args = vec!["mktable".to_string(), "msdos".to_string()];
        assert!(matches!(
            parse_parted_command(&args),
            Some(PartedCommand::MkLabel { .. })
        ));
    }

    #[test]
    fn test_parse_command_mkpart() {
        let args = vec![
            "mkpart".to_string(),
            "primary".to_string(),
            "ext4".to_string(),
            "1MiB".to_string(),
            "100%".to_string(),
        ];
        if let Some(PartedCommand::MkPart {
            part_type,
            fs_type,
            start,
            end,
        }) = parse_parted_command(&args)
        {
            assert_eq!(part_type, "primary");
            assert_eq!(fs_type, "ext4");
            assert_eq!(start, "1MiB");
            assert_eq!(end, "100%");
        } else {
            panic!("Expected MkPart command");
        }
    }

    #[test]
    fn test_parse_command_rm() {
        let args = vec!["rm".to_string(), "3".to_string()];
        if let Some(PartedCommand::Rm { number }) = parse_parted_command(&args) {
            assert_eq!(number, 3);
        } else {
            panic!("Expected Rm command");
        }
    }

    #[test]
    fn test_parse_command_name() {
        let args = vec![
            "name".to_string(),
            "1".to_string(),
            "mypart".to_string(),
        ];
        if let Some(PartedCommand::Name { number, name }) = parse_parted_command(&args) {
            assert_eq!(number, 1);
            assert_eq!(name, "mypart");
        } else {
            panic!("Expected Name command");
        }
    }

    #[test]
    fn test_parse_command_set() {
        let args = vec![
            "set".to_string(),
            "1".to_string(),
            "boot".to_string(),
            "on".to_string(),
        ];
        if let Some(PartedCommand::Set {
            number,
            flag,
            state,
        }) = parse_parted_command(&args)
        {
            assert_eq!(number, 1);
            assert_eq!(flag, "boot");
            assert_eq!(state, "on");
        } else {
            panic!("Expected Set command");
        }
    }

    #[test]
    fn test_parse_command_toggle() {
        let args = vec![
            "toggle".to_string(),
            "2".to_string(),
            "lvm".to_string(),
        ];
        if let Some(PartedCommand::Toggle { number, flag }) = parse_parted_command(&args) {
            assert_eq!(number, 2);
            assert_eq!(flag, "lvm");
        } else {
            panic!("Expected Toggle command");
        }
    }

    #[test]
    fn test_parse_command_resizepart() {
        let args = vec![
            "resizepart".to_string(),
            "1".to_string(),
            "200GB".to_string(),
        ];
        if let Some(PartedCommand::ResizePart { number, end }) = parse_parted_command(&args) {
            assert_eq!(number, 1);
            assert_eq!(end, "200GB");
        } else {
            panic!("Expected ResizePart command");
        }
    }

    #[test]
    fn test_parse_command_move() {
        let args = vec![
            "move".to_string(),
            "1".to_string(),
            "50GB".to_string(),
        ];
        if let Some(PartedCommand::Move { number, start }) = parse_parted_command(&args) {
            assert_eq!(number, 1);
            assert_eq!(start, "50GB");
        } else {
            panic!("Expected Move command");
        }
    }

    #[test]
    fn test_parse_command_unit() {
        let args = vec!["unit".to_string(), "MB".to_string()];
        if let Some(PartedCommand::Unit { unit }) = parse_parted_command(&args) {
            assert_eq!(unit, "MB");
        } else {
            panic!("Expected Unit command");
        }
    }

    #[test]
    fn test_parse_command_align_check() {
        let args = vec![
            "align-check".to_string(),
            "optimal".to_string(),
            "1".to_string(),
        ];
        if let Some(PartedCommand::AlignCheck {
            align_type,
            number,
        }) = parse_parted_command(&args)
        {
            assert_eq!(align_type, "optimal");
            assert_eq!(number, 1);
        } else {
            panic!("Expected AlignCheck command");
        }
    }

    #[test]
    fn test_parse_command_select() {
        let args = vec!["select".to_string(), "/dev/sdb".to_string()];
        if let Some(PartedCommand::Select { device }) = parse_parted_command(&args) {
            assert_eq!(device, "/dev/sdb");
        } else {
            panic!("Expected Select command");
        }
    }

    #[test]
    fn test_parse_command_help() {
        let args = vec!["help".to_string()];
        assert!(matches!(
            parse_parted_command(&args),
            Some(PartedCommand::Help { command: None })
        ));
    }

    #[test]
    fn test_parse_command_help_specific() {
        let args = vec!["help".to_string(), "print".to_string()];
        if let Some(PartedCommand::Help { command }) = parse_parted_command(&args) {
            assert_eq!(command.as_deref(), Some("print"));
        } else {
            panic!("Expected Help command");
        }
    }

    #[test]
    fn test_parse_command_quit() {
        let args = vec!["quit".to_string()];
        assert!(matches!(
            parse_parted_command(&args),
            Some(PartedCommand::Quit)
        ));
    }

    #[test]
    fn test_parse_command_exit() {
        let args = vec!["exit".to_string()];
        assert!(matches!(
            parse_parted_command(&args),
            Some(PartedCommand::Quit)
        ));
    }

    #[test]
    fn test_parse_command_version() {
        let args = vec!["version".to_string()];
        assert!(matches!(
            parse_parted_command(&args),
            Some(PartedCommand::Version)
        ));
    }

    #[test]
    fn test_parse_command_unknown() {
        let args = vec!["nonexistent".to_string()];
        assert!(parse_parted_command(&args).is_none());
    }

    #[test]
    fn test_parse_command_empty() {
        let args: Vec<String> = vec![];
        assert!(parse_parted_command(&args).is_none());
    }

    #[test]
    fn test_parse_command_short_aliases() {
        assert!(matches!(
            parse_parted_command(&["p".to_string()]),
            Some(PartedCommand::Print { .. })
        ));
        assert!(matches!(
            parse_parted_command(&["q".to_string()]),
            Some(PartedCommand::Quit)
        ));
        assert!(matches!(
            parse_parted_command(&["v".to_string()]),
            Some(PartedCommand::Version)
        ));
    }

    // ---- Help Text Tests ----

    #[test]
    fn test_help_general() {
        let h = print_help(None);
        assert!(h.contains("print"));
        assert!(h.contains("mkpart"));
        assert!(h.contains("mklabel"));
    }

    #[test]
    fn test_help_print() {
        let h = print_help(Some("print"));
        assert!(h.contains("print"));
        assert!(h.contains("-l"));
    }

    #[test]
    fn test_help_mklabel() {
        let h = print_help(Some("mklabel"));
        assert!(h.contains("gpt"));
    }

    #[test]
    fn test_help_mkpart() {
        let h = print_help(Some("mkpart"));
        assert!(h.contains("START"));
        assert!(h.contains("END"));
    }

    // ---- NR Range Parsing ----

    #[test]
    fn test_parse_nr_range_colon() {
        let (s, e) = parse_nr_range("1:4");
        assert_eq!(s, Some(1));
        assert_eq!(e, Some(4));
    }

    #[test]
    fn test_parse_nr_range_dash() {
        let (s, e) = parse_nr_range("2-6");
        assert_eq!(s, Some(2));
        assert_eq!(e, Some(6));
    }

    #[test]
    fn test_parse_nr_range_single() {
        let (s, e) = parse_nr_range("3");
        assert_eq!(s, Some(3));
        assert_eq!(e, Some(3));
    }

    #[test]
    fn test_parse_nr_range_open_start() {
        let (s, e) = parse_nr_range(":5");
        assert_eq!(s, None);
        assert_eq!(e, Some(5));
    }

    #[test]
    fn test_parse_nr_range_open_end() {
        let (s, e) = parse_nr_range("2:");
        assert_eq!(s, Some(2));
        assert_eq!(e, None);
    }

    // ---- partx Column Formatting ----

    #[test]
    fn test_format_partx_row_all_columns() {
        let cols = vec![
            "NR".to_string(),
            "START".to_string(),
            "END".to_string(),
            "SECTORS".to_string(),
            "SIZE".to_string(),
            "NAME".to_string(),
            "UUID".to_string(),
            "TYPE".to_string(),
            "FLAGS".to_string(),
            "SCHEME".to_string(),
        ];
        let row = format_partx_row(
            &cols,
            &PartxRow {
                nr: 1,
                start: 2048,
                end: 1026047,
                sectors: 1024000,
                name: "test",
                uuid: "uuid1",
                type_str: "83",
                flags: "",
                scheme: "gpt",
            },
        );
        assert!(row.contains("1"));
        assert!(row.contains("2048"));
        assert!(row.contains("test"));
    }

    #[test]
    fn test_format_partx_row_subset() {
        let cols = vec!["NR".to_string(), "SIZE".to_string()];
        let row = format_partx_row(
            &cols,
            &PartxRow {
                nr: 5,
                start: 0,
                end: 0,
                sectors: 2048000,
                name: "",
                uuid: "",
                type_str: "",
                flags: "",
                scheme: "",
            },
        );
        assert!(row.contains("5"));
    }

    // ---- GPT Full Parse Tests ----

    #[test]
    fn test_gpt_full_create_and_parse() {
        let disk = DiskInfo::new("/dev/sda", 500 * GB);
        let mut editor = DiskEditor::new(disk);
        editor.create_label(TableType::Gpt);
        let _ = editor.mkpart_gpt("EFI", "fat32", MIB, 512 * MB);
        let _ = editor.mkpart_gpt("root", "linux", GIB, 100 * GIB);

        let data = editor.serialize_table();

        // Parse it back
        let mut disk2 = DiskInfo::new("/dev/sda", 500 * GB);
        let parsed = DiskInfo::parse_gpt(&data, &mut disk2);
        assert!(parsed);
        assert_eq!(disk2.partitions.len(), 2);
        assert_eq!(disk2.partitions[0].name, "EFI");
        assert_eq!(disk2.partitions[1].name, "root");
    }

    #[test]
    fn test_gpt_parse_empty() {
        let disk = DiskInfo::new("/dev/sda", 500 * GB);
        let mut editor = DiskEditor::new(disk);
        editor.create_label(TableType::Gpt);

        let data = editor.serialize_table();
        let mut disk2 = DiskInfo::new("/dev/sda", 500 * GB);
        let parsed = DiskInfo::parse_gpt(&data, &mut disk2);
        assert!(parsed);
        assert_eq!(disk2.partitions.len(), 0);
    }

    #[test]
    fn test_gpt_parse_invalid_data() {
        let mut disk = DiskInfo::new("/dev/sda", 500 * GB);
        let data = vec![0u8; 512]; // Too small, no GPT
        assert!(!DiskInfo::parse_gpt(&data, &mut disk));
    }

    // ---- MBR Full Parse Tests ----

    #[test]
    fn test_mbr_full_create_and_parse() {
        let disk = DiskInfo::new("/dev/sda", 500 * GB);
        let mut editor = DiskEditor::new(disk);
        editor.create_label(TableType::Mbr);
        let _ = editor.mkpart_mbr(MbrPartRole::Primary, "linux", MIB, 100 * GIB);

        let data = editor.serialize_table();
        let mut disk2 = DiskInfo::new("/dev/sda", 500 * GB);
        let parsed = DiskInfo::parse_mbr(&data, &mut disk2);
        assert!(parsed);
        assert!(disk2.partitions.len() >= 1);
    }

    #[test]
    fn test_mbr_parse_protective_returns_false() {
        let mbr = Mbr::new_protective(1_000_000);
        let data = mbr.serialize();
        let mut disk = DiskInfo::new("/dev/sda", 500 * GB);
        assert!(!DiskInfo::parse_mbr(&data.to_vec(), &mut disk));
    }

    // ---- Partition Size Tests ----

    #[test]
    fn test_partition_size_bytes() {
        let p = Partition {
            number: 1,
            first_lba: 2048,
            last_lba: 4095,
            size_sectors: 2048,
            name: String::new(),
            type_name: String::new(),
            flags: Vec::new(),
            type_guid: None,
            unique_guid: None,
            attributes: 0,
            mbr_type: None,
            mbr_role: None,
            bootable: false,
        };
        assert_eq!(p.size_bytes(), 2048 * 512);
    }

    // ---- MBR Boot Flag Tests ----

    #[test]
    fn test_mbr_boot_flag_set() {
        let disk = DiskInfo::new("/dev/sda", 500 * GB);
        let mut editor = DiskEditor::new(disk);
        editor.create_label(TableType::Mbr);
        let _ = editor.mkpart_mbr(MbrPartRole::Primary, "linux", MIB, 100 * GIB);
        let _ = editor.set_flag(1, PartitionFlag::Boot, true);
        assert!(editor.disk.partitions[0].bootable);
    }

    #[test]
    fn test_mbr_only_one_bootable() {
        let disk = DiskInfo::new("/dev/sda", 500 * GB);
        let mut editor = DiskEditor::new(disk);
        editor.create_label(TableType::Mbr);
        let _ = editor.mkpart_mbr(MbrPartRole::Primary, "linux", MIB, 100 * GIB);
        let _ = editor.mkpart_mbr(MbrPartRole::Primary, "linux", 101 * GIB, 200 * GIB);
        let _ = editor.set_flag(1, PartitionFlag::Boot, true);
        let _ = editor.set_flag(2, PartitionFlag::Boot, true);
        // Only partition 2 should be bootable now
        assert!(!editor.disk.partitions[0].bootable);
        assert!(editor.disk.partitions[1].bootable);
    }

    #[test]
    fn test_mbr_non_boot_flag_error() {
        let disk = DiskInfo::new("/dev/sda", 500 * GB);
        let mut editor = DiskEditor::new(disk);
        editor.create_label(TableType::Mbr);
        let _ = editor.mkpart_mbr(MbrPartRole::Primary, "linux", MIB, 100 * GIB);
        // ESP flag should fail on MBR
        assert!(editor.set_flag(1, PartitionFlag::Esp, true).is_err());
    }

    // ---- Personality Detection Tests ----

    #[test]
    fn test_personality_detection_unix() {
        let test = |path: &str, expected: &str| {
            let s = path;
            let bytes = s.as_bytes();
            let mut last_sep = 0;
            for (i, &b) in bytes.iter().enumerate() {
                if b == b'/' || b == b'\\' {
                    last_sep = i + 1;
                }
            }
            let base = &s[last_sep..];
            let base = base.strip_suffix(".exe").unwrap_or(base);
            assert_eq!(base, expected);
        };

        test("/usr/sbin/parted", "parted");
        test("/usr/sbin/partprobe", "partprobe");
        test("/usr/sbin/partx", "partx");
        test("C:\\Windows\\parted.exe", "parted");
        test("parted", "parted");
        test("./parted", "parted");
    }

    // ---- Edge Case Tests ----

    #[test]
    fn test_resize_to_before_start_fails() {
        let disk = DiskInfo::new("/dev/sda", 500 * GB);
        let mut editor = DiskEditor::new(disk);
        editor.create_label(TableType::Gpt);
        let _ = editor.mkpart_gpt("root", "linux", 100 * GIB, 200 * GIB);
        // Try to resize to before start
        assert!(editor.resize_partition(1, 50 * GIB).is_err());
    }

    #[test]
    fn test_move_overlap_fails() {
        let disk = DiskInfo::new("/dev/sda", 500 * GB);
        let mut editor = DiskEditor::new(disk);
        editor.create_label(TableType::Gpt);
        let _ = editor.mkpart_gpt("a", "linux", MIB, 100 * GIB);
        let _ = editor.mkpart_gpt("b", "linux", 101 * GIB, 200 * GIB);
        // Move second partition to overlap with first
        assert!(editor.move_partition(2, 50 * GIB).is_err());
    }

    #[test]
    fn test_mkpart_start_past_end() {
        let disk = DiskInfo::new("/dev/sda", 500 * GB);
        let mut editor = DiskEditor::new(disk);
        editor.create_label(TableType::Gpt);
        assert!(editor.mkpart_gpt("bad", "linux", 200 * GIB, 100 * GIB).is_err());
    }

    #[test]
    fn test_set_flag_no_table() {
        let disk = DiskInfo::new("/dev/sda", 500 * GB);
        let mut editor = DiskEditor::new(disk);
        assert!(editor.set_flag(1, PartitionFlag::Boot, true).is_err());
    }

    #[test]
    fn test_format_percent_zero_disk() {
        let s = format_size(500, DisplayUnit::Percent, 0, 512);
        assert_eq!(s, "0%");
    }

    #[test]
    fn test_parse_size_invalid_percent() {
        assert!(parse_size("150%", 512, 1_000_000).is_none());
    }

    #[test]
    fn test_disk_info_new() {
        let disk = DiskInfo::new("/dev/sda", 1 * TB);
        assert_eq!(disk.size_bytes, TB);
        assert_eq!(disk.size_sectors, TB / 512);
        assert_eq!(disk.sector_size, 512);
        assert!(disk.table_type.is_none());
    }

    #[test]
    fn test_mbr_logical_without_extended_fails() {
        let disk = DiskInfo::new("/dev/sda", 500 * GB);
        let mut editor = DiskEditor::new(disk);
        editor.create_label(TableType::Mbr);
        let result = editor.mkpart_mbr(MbrPartRole::Logical, "linux", MIB, 100 * GIB);
        assert!(result.is_err());
    }

    #[test]
    fn test_mbr_2tb_limit() {
        let disk = DiskInfo::new("/dev/sda", 4 * TB);
        let mut editor = DiskEditor::new(disk);
        editor.create_label(TableType::Mbr);
        // This should fail as the sector address exceeds u32 range
        let result = editor.mkpart_mbr(MbrPartRole::Primary, "linux", 3 * TB, 4 * TB);
        assert!(result.is_err());
    }
}
