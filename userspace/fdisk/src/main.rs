//! OurOS Partition Table Manipulator
//!
//! Non-interactive tool for listing and modifying disk partition tables.
//! Supports GPT (GUID Partition Table) and MBR (Master Boot Record).
//!
//! Reads disk geometry from `/sys/block/` and partition data by reading
//! device files directly (first 34 sectors for GPT, first sector for MBR).
//! Write operations use the `SYS_DISK_IOCTL` syscall (number 660).
//!
//! # Usage
//!
//! ```text
//! fdisk -l                          List all disk partition tables
//! fdisk -l /dev/sda                 List partition table for a specific disk
//! fdisk --new /dev/sda -n 2048 1048576          Create partition
//! fdisk --new /dev/sda -n 2048 1048576 -t 0x83  Create with type
//! fdisk --delete /dev/sda -d 2      Delete partition 2
//! fdisk --type /dev/sda -t 3 0xEF   Change partition 3 type to EFI
//! fdisk -l --json                   JSON output
//! fdisk -l -x                       Extended/expert info
//! fdisk -l --bytes                  Sizes in bytes
//! fdisk -l -o Device,Start,End,Size List with selected columns
//! ```

use std::env;
use std::fs;
use std::io::Read;
use std::process;

// ============================================================================
// Syscall interface
// ============================================================================

const SYS_DISK_IOCTL: u64 = 660;

// Sub-commands for SYS_DISK_IOCTL.
#[allow(dead_code)]
const DISK_GET_SIZE: u64 = 1;
#[allow(dead_code)]
const DISK_READ_PT: u64 = 2;
const DISK_WRITE_PT: u64 = 3;
#[allow(dead_code)]
const DISK_GET_INFO: u64 = 4;

/// Invoke a syscall with up to 5 arguments.
///
/// The kernel receives arguments in: rdi, rsi, rdx, r10, r8.
#[cfg(target_arch = "x86_64")]
unsafe fn syscall5(nr: u64, a1: u64, a2: u64, a3: u64, a4: u64, a5: u64) -> i64 {
    let ret: i64;
    // SAFETY: Caller ensures arguments are valid pointers/values for the
    // given syscall number. The kernel validates all inputs and returns
    // a negative errno on failure.
    unsafe {
        core::arch::asm!(
            "syscall",
            inlateout("rax") nr as i64 => ret,
            in("rdi") a1,
            in("rsi") a2,
            in("rdx") a3,
            in("r10") a4,
            in("r8") a5,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack),
        );
    }
    ret
}

/// Invoke a syscall with 3 arguments.
#[cfg(target_arch = "x86_64")]
unsafe fn syscall3(nr: u64, a1: u64, a2: u64, a3: u64) -> i64 {
    // SAFETY: Delegated to syscall5 with zero-filled trailing args.
    unsafe { syscall5(nr, a1, a2, a3, 0, 0) }
}

/// Make a null-terminated C string from a Rust string slice.
fn c_str(s: &str) -> Vec<u8> {
    let mut v = s.as_bytes().to_vec();
    v.push(0);
    v
}

/// Translate a negative syscall return into a human-readable error.
fn syscall_error_msg(ret: i64) -> String {
    match ret {
        -1 => "operation not permitted".to_string(),
        -2 => "no such file or directory".to_string(),
        -5 => "I/O error".to_string(),
        -12 => "out of memory".to_string(),
        -13 => "permission denied".to_string(),
        -16 => "device busy".to_string(),
        -19 => "no such device".to_string(),
        -22 => "invalid argument".to_string(),
        -28 => "no space left on device".to_string(),
        -30 => "read-only filesystem".to_string(),
        -38 => "function not implemented".to_string(),
        other => format!("error {other}"),
    }
}

// ============================================================================
// Sysfs helpers
// ============================================================================

/// Read a file and return its trimmed contents, or None on failure.
fn read_file(path: &str) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

/// Read a file and parse it as u64, or return 0.
fn read_u64(path: &str) -> u64 {
    read_file(path)
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0)
}

// ============================================================================
// Size formatting
// ============================================================================

/// Format a byte count as a human-readable string (e.g. "1.5G", "512M").
fn format_size(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * KIB;
    const GIB: u64 = 1024 * MIB;
    const TIB: u64 = 1024 * GIB;

    if bytes >= TIB {
        let whole = bytes / TIB;
        let frac = ((bytes % TIB) * 10) / TIB;
        if frac > 0 { format!("{whole}.{frac}T") } else { format!("{whole}T") }
    } else if bytes >= GIB {
        let whole = bytes / GIB;
        let frac = ((bytes % GIB) * 10) / GIB;
        if frac > 0 { format!("{whole}.{frac}G") } else { format!("{whole}G") }
    } else if bytes >= MIB {
        let whole = bytes / MIB;
        let frac = ((bytes % MIB) * 10) / MIB;
        if frac > 0 { format!("{whole}.{frac}M") } else { format!("{whole}M") }
    } else if bytes >= KIB {
        let whole = bytes / KIB;
        let frac = ((bytes % KIB) * 10) / KIB;
        if frac > 0 { format!("{whole}.{frac}K") } else { format!("{whole}K") }
    } else {
        format!("{bytes}B")
    }
}

// ============================================================================
// CRC32 (for GPT header/entry validation)
// ============================================================================

/// Compute CRC32 using the standard polynomial (IEEE 802.3).
/// GPT headers include CRC32 checksums over the header and partition entries.
fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xEDB8_8320;
            } else {
                crc >>= 1;
            }
        }
    }
    !crc
}

// ============================================================================
// GPT type GUIDs
// ============================================================================

/// Known GPT partition type GUIDs mapped to human-readable names.
struct GptTypeEntry {
    guid: [u8; 16],
    name: &'static str,
}

/// Parse a GUID string "XXXXXXXX-XXXX-XXXX-XXXX-XXXXXXXXXXXX" into 16 bytes
/// in mixed-endian format (as stored on disk in GPT entries).
///
/// GPT stores the GUID in mixed-endian: the first three groups are
/// little-endian, the last two are big-endian.
const fn parse_guid(s: &[u8; 36]) -> [u8; 16] {
    // Helper: decode a single hex digit at compile time.
    const fn hex(c: u8) -> u8 {
        match c {
            b'0'..=b'9' => c - b'0',
            b'A'..=b'F' => c - b'A' + 10,
            b'a'..=b'f' => c - b'a' + 10,
            _ => 0, // unreachable in valid input
        }
    }
    const fn hex2(hi: u8, lo: u8) -> u8 {
        (hex(hi) << 4) | hex(lo)
    }

    // "XXXXXXXX-XXXX-XXXX-XXXX-XXXXXXXXXXXX"
    //  01234567 9012 4567 9012 456789012345
    //  group1   g2   g3   g4   group5

    let mut guid = [0u8; 16];

    // Group 1 (4 bytes, little-endian):
    guid[3] = hex2(s[0], s[1]);
    guid[2] = hex2(s[2], s[3]);
    guid[1] = hex2(s[4], s[5]);
    guid[0] = hex2(s[6], s[7]);

    // Group 2 (2 bytes, little-endian):
    guid[5] = hex2(s[9], s[10]);
    guid[4] = hex2(s[11], s[12]);

    // Group 3 (2 bytes, little-endian):
    guid[7] = hex2(s[14], s[15]);
    guid[6] = hex2(s[16], s[17]);

    // Group 4 (2 bytes, big-endian):
    guid[8] = hex2(s[19], s[20]);
    guid[9] = hex2(s[21], s[22]);

    // Group 5 (6 bytes, big-endian):
    guid[10] = hex2(s[24], s[25]);
    guid[11] = hex2(s[26], s[27]);
    guid[12] = hex2(s[28], s[29]);
    guid[13] = hex2(s[30], s[31]);
    guid[14] = hex2(s[32], s[33]);
    guid[15] = hex2(s[34], s[35]);

    guid
}

/// Format a 16-byte mixed-endian GUID back to the standard string form.
fn format_guid(g: &[u8; 16]) -> String {
    // Reverse the mixed-endian encoding back to display form.
    format!(
        "{:02X}{:02X}{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}",
        g[3], g[2], g[1], g[0],
        g[5], g[4],
        g[7], g[6],
        g[8], g[9],
        g[10], g[11], g[12], g[13], g[14], g[15],
    )
}

/// Built-in table of common GPT partition type GUIDs.
fn gpt_type_table() -> Vec<GptTypeEntry> {
    vec![
        GptTypeEntry {
            guid: parse_guid(b"C12A7328-F81F-11D2-BA4B-00A0C93EC93B"),
            name: "EFI System",
        },
        GptTypeEntry {
            guid: parse_guid(b"0FC63DAF-8483-4772-8E79-3D69D8477DE4"),
            name: "Linux filesystem",
        },
        GptTypeEntry {
            guid: parse_guid(b"0657FD6D-A4AB-43C4-84E5-0933C84B4F4F"),
            name: "Linux swap",
        },
        GptTypeEntry {
            guid: parse_guid(b"EBD0A0A2-B9E5-4433-87C0-68B6B72699C7"),
            name: "Microsoft basic data",
        },
        GptTypeEntry {
            guid: parse_guid(b"E6D6D379-F507-44C2-A23C-238F2A3DF928"),
            name: "Linux LVM",
        },
        GptTypeEntry {
            guid: parse_guid(b"A19D880F-05FC-4D3B-A006-743F0F84911E"),
            name: "Linux RAID",
        },
        GptTypeEntry {
            guid: parse_guid(b"21686148-6449-6E6F-744E-656564454649"),
            name: "BIOS boot",
        },
        GptTypeEntry {
            guid: parse_guid(b"48465300-0000-11AA-AA11-00306543ECAC"),
            name: "Apple HFS+",
        },
        GptTypeEntry {
            guid: parse_guid(b"7C3457EF-0000-11AA-AA11-00306543ECAC"),
            name: "Apple APFS",
        },
        GptTypeEntry {
            guid: parse_guid(b"DE94BBA4-06D1-4D40-A16A-BFD50179D6AC"),
            name: "Windows recovery",
        },
    ]
}

/// Look up a GPT partition type GUID and return its human-readable name.
fn gpt_type_name(guid: &[u8; 16]) -> &'static str {
    use std::sync::OnceLock;
    static TABLE: OnceLock<Vec<GptTypeEntry>> = OnceLock::new();
    let table = TABLE.get_or_init(gpt_type_table);
    for entry in table {
        if entry.guid == *guid {
            return entry.name;
        }
    }
    "unknown"
}

// ============================================================================
// MBR partition types
// ============================================================================

/// Return the name for an MBR partition type byte.
fn mbr_type_name(type_id: u8) -> &'static str {
    match type_id {
        0x00 => "Empty",
        0x01 => "FAT12",
        0x04 => "FAT16 <32M",
        0x05 => "Extended",
        0x06 => "FAT16",
        0x07 => "HPFS/NTFS",
        0x0B => "W95 FAT32",
        0x0C => "W95 FAT32 (LBA)",
        0x0E => "W95 FAT16 (LBA)",
        0x0F => "W95 Ext'd (LBA)",
        0x11 => "Hidden FAT12",
        0x14 => "Hidden FAT16 <32M",
        0x16 => "Hidden FAT16",
        0x17 => "Hidden HPFS/NTFS",
        0x1B => "Hidden W95 FAT32",
        0x1C => "Hidden W95 FAT32 (LBA)",
        0x1E => "Hidden W95 FAT16 (LBA)",
        0x27 => "Hidden NTFS WinRE",
        0x42 => "SFS / LDM",
        0x82 => "Linux swap",
        0x83 => "Linux",
        0x85 => "Linux extended",
        0x8E => "Linux LVM",
        0xA5 => "FreeBSD",
        0xA6 => "OpenBSD",
        0xA8 => "Darwin UFS",
        0xAB => "Darwin boot",
        0xAF => "HFS / HFS+",
        0xBE => "Solaris boot",
        0xBF => "Solaris",
        0xEE => "GPT protective",
        0xEF => "EFI System",
        0xFB => "VMware VMFS",
        0xFC => "VMware swap",
        0xFD => "Linux RAID",
        _ => "unknown",
    }
}

// ============================================================================
// On-disk structures
// ============================================================================

/// Parsed GPT header from LBA 1.
#[allow(dead_code)]
struct GptHeader {
    /// "EFI PART" signature was valid.
    valid: bool,
    /// Revision (usually 0x00010000).
    revision: u32,
    /// Header size in bytes.
    header_size: u32,
    /// CRC32 of the header (with this field zeroed during computation).
    header_crc: u32,
    /// LBA of this header.
    my_lba: u64,
    /// LBA of the alternate header.
    alternate_lba: u64,
    /// First usable LBA for partitions.
    first_usable_lba: u64,
    /// Last usable LBA for partitions.
    last_usable_lba: u64,
    /// Disk GUID (16 bytes, mixed-endian).
    disk_guid: [u8; 16],
    /// LBA of the start of the partition entry array.
    partition_entry_lba: u64,
    /// Number of partition entries.
    num_partition_entries: u32,
    /// Size of each partition entry in bytes.
    partition_entry_size: u32,
    /// CRC32 of the partition entry array.
    partition_entries_crc: u32,
    /// Whether the header CRC checked out.
    crc_valid: bool,
}

/// A single parsed GPT partition entry (128 bytes on disk).
struct GptPartition {
    /// Partition type GUID (16 bytes, mixed-endian).
    type_guid: [u8; 16],
    /// Unique partition GUID (16 bytes, mixed-endian).
    unique_guid: [u8; 16],
    /// First LBA.
    first_lba: u64,
    /// Last LBA (inclusive).
    last_lba: u64,
    /// Attribute flags.
    attributes: u64,
    /// Partition name (UTF-16LE, up to 36 code units).
    name: String,
}

impl GptPartition {
    /// True if this entry is unused (type GUID is all zeros).
    fn is_empty(&self) -> bool {
        self.type_guid == [0u8; 16]
    }

    /// Size in sectors.
    fn sectors(&self) -> u64 {
        if self.last_lba >= self.first_lba {
            self.last_lba - self.first_lba + 1
        } else {
            0
        }
    }
}

/// A single parsed MBR partition entry (16 bytes on disk).
struct MbrPartition {
    /// Boot indicator (0x80 = bootable, 0x00 = not).
    status: u8,
    /// Partition type byte.
    type_id: u8,
    /// Starting LBA.
    lba_start: u32,
    /// Size in sectors.
    lba_size: u32,
}

impl MbrPartition {
    fn is_empty(&self) -> bool {
        self.type_id == 0x00
    }
}

/// Detected partition table type.
enum DiskLabel {
    Gpt {
        header: GptHeader,
        partitions: Vec<GptPartition>,
    },
    Mbr {
        partitions: [MbrPartition; 4],
    },
    Unknown,
}

// ============================================================================
// Parsing raw disk data
// ============================================================================

/// Read little-endian u16 from a byte slice at the given offset.
#[allow(dead_code)]
fn le_u16(buf: &[u8], off: usize) -> u16 {
    let b = buf.get(off..off + 2).unwrap_or(&[0, 0]);
    u16::from_le_bytes([b[0], b[1]])
}

/// Read little-endian u32 from a byte slice at the given offset.
fn le_u32(buf: &[u8], off: usize) -> u32 {
    let b = buf.get(off..off + 4).unwrap_or(&[0, 0, 0, 0]);
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

/// Read little-endian u64 from a byte slice at the given offset.
fn le_u64(buf: &[u8], off: usize) -> u64 {
    let b = buf.get(off..off + 8).unwrap_or(&[0; 8]);
    u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]])
}

/// Copy 16 bytes at offset into a fixed array (used for GUIDs).
fn copy_guid(buf: &[u8], off: usize) -> [u8; 16] {
    let mut g = [0u8; 16];
    if let Some(slice) = buf.get(off..off + 16) {
        g.copy_from_slice(slice);
    }
    g
}

/// Write a little-endian u32 into a byte buffer at a given offset.
fn write_le_u32(buf: &mut [u8], off: usize, val: u32) {
    let bytes = val.to_le_bytes();
    if let Some(dst) = buf.get_mut(off..off + 4) {
        dst.copy_from_slice(&bytes);
    }
}

/// Write a little-endian u64 into a byte buffer at a given offset.
fn write_le_u64(buf: &mut [u8], off: usize, val: u64) {
    let bytes = val.to_le_bytes();
    if let Some(dst) = buf.get_mut(off..off + 8) {
        dst.copy_from_slice(&bytes);
    }
}

/// Parse a GPT header from a 512-byte sector buffer (LBA 1).
fn parse_gpt_header(sector: &[u8]) -> GptHeader {
    let sig = sector.get(0..8).unwrap_or(&[0; 8]);
    let valid = sig == b"EFI PART";

    let revision = le_u32(sector, 8);
    let header_size = le_u32(sector, 12);
    let header_crc = le_u32(sector, 16);
    let my_lba = le_u64(sector, 24);
    let alternate_lba = le_u64(sector, 32);
    let first_usable_lba = le_u64(sector, 40);
    let last_usable_lba = le_u64(sector, 48);
    let disk_guid = copy_guid(sector, 56);
    let partition_entry_lba = le_u64(sector, 72);
    let num_partition_entries = le_u32(sector, 80);
    let partition_entry_size = le_u32(sector, 84);
    let partition_entries_crc = le_u32(sector, 88);

    // Verify header CRC: zero out the CRC field, compute, compare.
    let crc_valid = if valid && (header_size as usize) <= sector.len() {
        let mut hdr_copy = Vec::new();
        if let Some(slice) = sector.get(..header_size as usize) {
            hdr_copy.extend_from_slice(slice);
            // Zero out the CRC field at offset 16.
            write_le_u32(&mut hdr_copy, 16, 0);
            crc32(&hdr_copy) == header_crc
        } else {
            false
        }
    } else {
        false
    };

    GptHeader {
        valid,
        revision,
        header_size,
        header_crc,
        my_lba,
        alternate_lba,
        first_usable_lba,
        last_usable_lba,
        disk_guid,
        partition_entry_lba,
        num_partition_entries,
        partition_entry_size,
        partition_entries_crc,
        crc_valid,
    }
}

/// Parse a single GPT partition entry from a 128-byte buffer.
fn parse_gpt_entry(buf: &[u8]) -> GptPartition {
    let type_guid = copy_guid(buf, 0);
    let unique_guid = copy_guid(buf, 16);
    let first_lba = le_u64(buf, 32);
    let last_lba = le_u64(buf, 40);
    let attributes = le_u64(buf, 48);

    // Name is UTF-16LE at offset 56, up to 72 bytes (36 code units).
    let name_bytes = buf.get(56..128).unwrap_or(&[]);
    let code_units: Vec<u16> = name_bytes
        .chunks(2)
        .map(|chunk| {
            if chunk.len() == 2 {
                u16::from_le_bytes([chunk[0], chunk[1]])
            } else {
                0
            }
        })
        .take_while(|&cu| cu != 0)
        .collect();
    let name = String::from_utf16_lossy(&code_units);

    GptPartition { type_guid, unique_guid, first_lba, last_lba, attributes, name }
}

/// Parse all four MBR partition entries from the 512-byte MBR sector.
fn parse_mbr_entries(sector: &[u8]) -> [MbrPartition; 4] {
    let mut entries = [
        MbrPartition { status: 0, type_id: 0, lba_start: 0, lba_size: 0 },
        MbrPartition { status: 0, type_id: 0, lba_start: 0, lba_size: 0 },
        MbrPartition { status: 0, type_id: 0, lba_start: 0, lba_size: 0 },
        MbrPartition { status: 0, type_id: 0, lba_start: 0, lba_size: 0 },
    ];

    for i in 0..4 {
        let base = 446 + i * 16;
        if base + 16 > sector.len() {
            break;
        }
        entries[i].status = sector.get(base).copied().unwrap_or(0);
        entries[i].type_id = sector.get(base + 4).copied().unwrap_or(0);
        entries[i].lba_start = le_u32(sector, base + 8);
        entries[i].lba_size = le_u32(sector, base + 12);
    }

    entries
}

/// Check for the MBR boot signature (0x55AA at offset 510).
fn has_mbr_signature(sector: &[u8]) -> bool {
    sector.len() >= 512
        && sector.get(510).copied() == Some(0x55)
        && sector.get(511).copied() == Some(0xAA)
}

// ============================================================================
// Reading raw disk sectors
// ============================================================================

/// Read the first N bytes from a device file, returning the data or an error.
fn read_device_bytes(device_path: &str, count: usize) -> Result<Vec<u8>, String> {
    let mut file = fs::File::open(device_path)
        .map_err(|e| format!("cannot open {device_path}: {e}"))?;
    let mut buf = vec![0u8; count];
    let n = file.read(&mut buf).map_err(|e| format!("read {device_path}: {e}"))?;
    buf.truncate(n);
    Ok(buf)
}

/// Detect and parse the partition table from a device by reading its raw sectors.
///
/// Reads up to 34 sectors (17408 bytes) which covers the protective MBR (LBA 0),
/// the GPT header (LBA 1), and all 128 default GPT entries (LBAs 2-33).
fn read_partition_table(device_path: &str) -> DiskLabel {
    // Read 34 sectors: MBR + GPT header + 128 entries * 128 bytes = 17408 bytes.
    let raw = match read_device_bytes(device_path, 34 * 512) {
        Ok(data) => data,
        Err(_) => return DiskLabel::Unknown,
    };

    if raw.len() < 512 {
        return DiskLabel::Unknown;
    }

    // Check for GPT: LBA 1 should have "EFI PART" signature.
    if raw.len() >= 1024 {
        let gpt_sector = &raw[512..1024];
        let header = parse_gpt_header(gpt_sector);

        if header.valid {
            let entry_size = if header.partition_entry_size >= 128 {
                header.partition_entry_size as usize
            } else {
                128
            };
            let max_entries = header.num_partition_entries as usize;

            // Partition entries typically start at LBA 2 (byte offset 1024).
            let entries_start = header.partition_entry_lba.saturating_mul(512) as usize;

            let mut partitions = Vec::new();
            for i in 0..max_entries {
                let off = entries_start + i * entry_size;
                if off + 128 > raw.len() {
                    break;
                }
                let entry = parse_gpt_entry(&raw[off..off + 128]);
                if !entry.is_empty() {
                    partitions.push(entry);
                }
            }

            return DiskLabel::Gpt { header, partitions };
        }
    }

    // Fall back to MBR.
    if has_mbr_signature(&raw[..512]) {
        let entries = parse_mbr_entries(&raw[..512]);
        // Verify at least one non-empty entry to distinguish from blank disk.
        let has_partitions = entries.iter().any(|e| !e.is_empty());
        if has_partitions {
            return DiskLabel::Mbr { partitions: entries };
        }
    }

    DiskLabel::Unknown
}

// ============================================================================
// Disk info from sysfs
// ============================================================================

/// Information about a disk gathered from sysfs.
struct DiskInfo {
    /// Kernel device name (e.g. "sda").
    name: String,
    /// Device path (e.g. "/dev/sda").
    dev_path: String,
    /// Size in 512-byte sectors.
    sectors: u64,
    /// Hardware sector size from sysfs (typically 512).
    hw_sector_size: u64,
    /// Logical sector size (the one used for LBA addressing).
    logical_sector_size: u64,
    /// Device model string.
    model: String,
}

impl DiskInfo {
    fn size_bytes(&self) -> u64 {
        self.sectors.saturating_mul(512)
    }
}

/// Enumerate all block devices from /sys/block/.
fn enumerate_disks() -> Vec<DiskInfo> {
    let mut disks = Vec::new();
    let block_dir = "/sys/block";

    let entries = match fs::read_dir(block_dir) {
        Ok(e) => e,
        Err(_) => return disks,
    };

    for entry in entries.flatten() {
        let name = match entry.file_name().into_string() {
            Ok(n) => n,
            Err(_) => continue,
        };

        // Skip loop and ram devices with zero size.
        if name.starts_with("loop") || name.starts_with("ram") {
            let sz = read_u64(&format!("{block_dir}/{name}/size"));
            if sz == 0 {
                continue;
            }
        }

        let dev_path = format!("/dev/{name}");
        let sys_path = format!("{block_dir}/{name}");
        let sectors = read_u64(&format!("{sys_path}/size"));
        let hw_sector_size = read_u64(&format!("{sys_path}/queue/hw_sector_size"));
        let logical_sector_size = read_u64(&format!("{sys_path}/queue/logical_block_size"));
        let model = read_file(&format!("{sys_path}/device/model")).unwrap_or_default();

        disks.push(DiskInfo {
            name,
            dev_path,
            sectors,
            hw_sector_size: if hw_sector_size > 0 { hw_sector_size } else { 512 },
            logical_sector_size: if logical_sector_size > 0 { logical_sector_size } else { 512 },
            model,
        });
    }

    disks.sort_by(|a, b| a.name.cmp(&b.name));
    disks
}

/// Look up disk info for a specific device path or name.
fn find_disk_info(device: &str) -> Option<DiskInfo> {
    let name = device.strip_prefix("/dev/").unwrap_or(device);
    enumerate_disks().into_iter().find(|d| d.name == name)
}

// ============================================================================
// Output column support
// ============================================================================

#[derive(Clone, Copy, PartialEq, Eq)]
enum Column {
    Device,
    Start,
    End,
    Sectors,
    Size,
    Id,
    Type,
    Uuid,
    Name,
}

impl Column {
    fn from_str(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "device" | "dev" => Some(Self::Device),
            "start" => Some(Self::Start),
            "end" => Some(Self::End),
            "sectors" => Some(Self::Sectors),
            "size" => Some(Self::Size),
            "id" => Some(Self::Id),
            "type" => Some(Self::Type),
            "uuid" => Some(Self::Uuid),
            "name" => Some(Self::Name),
            _ => None,
        }
    }

    fn header(self) -> &'static str {
        match self {
            Self::Device => "Device",
            Self::Start => "Start",
            Self::End => "End",
            Self::Sectors => "Sectors",
            Self::Size => "Size",
            Self::Id => "Id",
            Self::Type => "Type",
            Self::Uuid => "UUID",
            Self::Name => "Name",
        }
    }
}

/// Default column set for GPT listing.
const DEFAULT_GPT_COLUMNS: &[Column] = &[
    Column::Device, Column::Start, Column::End, Column::Sectors, Column::Size, Column::Type,
];

/// Default column set for MBR listing.
const DEFAULT_MBR_COLUMNS: &[Column] = &[
    Column::Device, Column::Start, Column::End, Column::Sectors, Column::Size, Column::Id, Column::Type,
];

// ============================================================================
// JSON helper
// ============================================================================

/// Escape a string for safe inclusion in JSON output.
fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c < '\x20' => {
                let code = c as u32;
                out.push_str(&format!("\\u{code:04x}"));
            }
            c => out.push(c),
        }
    }
    out
}

// ============================================================================
// CLI configuration
// ============================================================================

struct Config {
    /// Show listing (-l).
    list: bool,
    /// Extended/expert info (-x).
    extended: bool,
    /// Sizes in bytes instead of human-readable.
    bytes: bool,
    /// JSON output.
    json: bool,
    /// Custom columns from -o.
    columns: Option<Vec<Column>>,
    /// Devices to operate on (default: all).
    devices: Vec<String>,

    // Mutating operations:
    /// --new: create partition. (device set in `devices[0]`).
    new_partition: bool,
    /// Start sector for new partition (-n).
    new_start: Option<u64>,
    /// Size in sectors for new partition (-n).
    new_size: Option<u64>,
    /// Type code for --new -t or --type -t.
    type_code: Option<String>,

    /// --delete: delete a partition.
    delete_partition: bool,
    /// Partition number to delete (-d).
    delete_num: Option<u32>,

    /// --type: change partition type.
    change_type: bool,
    /// Partition number for --type -t.
    change_type_num: Option<u32>,
}

impl Config {
    fn new() -> Self {
        Self {
            list: false,
            extended: false,
            bytes: false,
            json: false,
            columns: None,
            devices: Vec::new(),
            new_partition: false,
            new_start: None,
            new_size: None,
            type_code: None,
            delete_partition: false,
            delete_num: None,
            change_type: false,
            change_type_num: None,
        }
    }
}

// ============================================================================
// Listing output
// ============================================================================

/// Print the partition table listing for a single disk in the standard format.
fn print_disk_listing(disk: &DiskInfo, label: &DiskLabel, cfg: &Config) {
    let sector_size = disk.logical_sector_size;
    let total_bytes = disk.size_bytes();
    let total_sectors = disk.sectors;

    // Disk header line.
    if cfg.bytes {
        println!(
            "Disk {}: {} bytes, {} sectors",
            disk.dev_path, total_bytes, total_sectors,
        );
    } else {
        println!(
            "Disk {}: {}, {} bytes, {} sectors",
            disk.dev_path,
            format_size(total_bytes),
            total_bytes,
            total_sectors,
        );
    }

    if !disk.model.is_empty() {
        println!("Disk model: {}", disk.model);
    }
    println!(
        "Units: sectors of 1 * {} = {} bytes",
        sector_size, sector_size,
    );
    println!(
        "Sector size (logical/physical): {} bytes / {} bytes",
        disk.logical_sector_size, disk.hw_sector_size,
    );

    match label {
        DiskLabel::Gpt { header, partitions } => {
            println!("Disklabel type: gpt");
            if cfg.extended {
                println!("Disk identifier: {}", format_guid(&header.disk_guid));
                println!(
                    "First usable LBA: {}, Last usable LBA: {}",
                    header.first_usable_lba, header.last_usable_lba,
                );
                println!(
                    "Alternate LBA: {}, Partition entries LBA: {}",
                    header.alternate_lba, header.partition_entry_lba,
                );
                println!(
                    "Partition entries: {}, Entry size: {}",
                    header.num_partition_entries, header.partition_entry_size,
                );
                if header.crc_valid {
                    println!("Header CRC: {:08X} (valid)", header.header_crc);
                } else {
                    println!("Header CRC: {:08X} (INVALID)", header.header_crc);
                }
                println!("Entries CRC: {:08X}", header.partition_entries_crc);
            }

            if partitions.is_empty() {
                println!();
                return;
            }

            let columns = cfg.columns.as_deref().unwrap_or(DEFAULT_GPT_COLUMNS);
            println!();
            print_gpt_table(disk, partitions, columns, cfg);
        }
        DiskLabel::Mbr { partitions } => {
            println!("Disklabel type: dos");

            let active = partitions.iter().filter(|p| !p.is_empty()).count();
            if active == 0 {
                println!();
                return;
            }

            let columns = cfg.columns.as_deref().unwrap_or(DEFAULT_MBR_COLUMNS);
            println!();
            print_mbr_table(disk, partitions, columns, cfg);
        }
        DiskLabel::Unknown => {
            println!("Disklabel type: unknown");
            println!();
        }
    }
}

/// Print the GPT partition table rows.
fn print_gpt_table(
    disk: &DiskInfo,
    partitions: &[GptPartition],
    columns: &[Column],
    cfg: &Config,
) {
    let sector_size = disk.logical_sector_size;

    // Build header.
    let headers: Vec<&str> = columns.iter().map(|c| c.header()).collect();

    // Build rows.
    let mut rows: Vec<Vec<String>> = Vec::new();
    for (idx, part) in partitions.iter().enumerate() {
        let part_num = idx + 1;
        let dev_name = format!("{}{part_num}", disk.dev_path);
        let sectors = part.sectors();
        let size_bytes = sectors.saturating_mul(sector_size);
        let type_name = gpt_type_name(&part.type_guid);

        let row: Vec<String> = columns.iter().map(|col| match col {
            Column::Device => dev_name.clone(),
            Column::Start => part.first_lba.to_string(),
            Column::End => part.last_lba.to_string(),
            Column::Sectors => sectors.to_string(),
            Column::Size => {
                if cfg.bytes { size_bytes.to_string() } else { format_size(size_bytes) }
            }
            Column::Id => format_guid(&part.type_guid),
            Column::Type => type_name.to_string(),
            Column::Uuid => format_guid(&part.unique_guid),
            Column::Name => part.name.clone(),
        }).collect();
        rows.push(row);
    }

    print_aligned_table(&headers, &rows);

    // Extended info: show per-partition details.
    if cfg.extended {
        println!();
        for (idx, part) in partitions.iter().enumerate() {
            let part_num = idx + 1;
            println!(
                "Partition {}: type GUID={}, unique GUID={}",
                part_num,
                format_guid(&part.type_guid),
                format_guid(&part.unique_guid),
            );
            if part.attributes != 0 {
                println!("  Attributes: 0x{:016X}", part.attributes);
                if part.attributes & 1 != 0 {
                    println!("    - Required partition");
                }
                if part.attributes & (1 << 2) != 0 {
                    println!("    - Legacy BIOS bootable");
                }
                if part.attributes & (1 << 60) != 0 {
                    println!("    - Read-only");
                }
                if part.attributes & (1 << 62) != 0 {
                    println!("    - Hidden");
                }
                if part.attributes & (1 << 63) != 0 {
                    println!("    - No auto-mount");
                }
            }
            if !part.name.is_empty() {
                println!("  Name: \"{}\"", part.name);
            }
        }
    }
}

/// Print the MBR partition table rows.
fn print_mbr_table(
    disk: &DiskInfo,
    partitions: &[MbrPartition; 4],
    columns: &[Column],
    cfg: &Config,
) {
    let sector_size = disk.logical_sector_size;

    let headers: Vec<&str> = columns.iter().map(|c| c.header()).collect();
    let mut rows: Vec<Vec<String>> = Vec::new();

    for (idx, part) in partitions.iter().enumerate() {
        if part.is_empty() {
            continue;
        }

        let part_num = idx + 1;
        let dev_name = format!("{}{part_num}", disk.dev_path);
        let start = u64::from(part.lba_start);
        let end = start.saturating_add(u64::from(part.lba_size)).saturating_sub(1);
        let sectors = u64::from(part.lba_size);
        let size_bytes = sectors.saturating_mul(sector_size);
        let boot_marker = if part.status == 0x80 { "*" } else { "" };

        let row: Vec<String> = columns.iter().map(|col| match col {
            Column::Device => format!("{dev_name}{boot_marker}"),
            Column::Start => start.to_string(),
            Column::End => end.to_string(),
            Column::Sectors => sectors.to_string(),
            Column::Size => {
                if cfg.bytes { size_bytes.to_string() } else { format_size(size_bytes) }
            }
            Column::Id => format!("{:x}", part.type_id),
            Column::Type => mbr_type_name(part.type_id).to_string(),
            Column::Uuid => String::new(),
            Column::Name => String::new(),
        }).collect();
        rows.push(row);
    }

    print_aligned_table(&headers, &rows);
}

/// Print a table with right-aligned numeric columns and left-aligned text.
fn print_aligned_table(headers: &[&str], rows: &[Vec<String>]) {
    if headers.is_empty() {
        return;
    }

    // Compute column widths.
    let mut widths: Vec<usize> = headers.iter().map(|h| h.len()).collect();
    for row in rows {
        for (i, val) in row.iter().enumerate() {
            if i < widths.len() && val.len() > widths[i] {
                widths[i] = val.len();
            }
        }
    }

    // Determine which columns are numeric (right-align).
    let is_numeric: Vec<bool> = headers.iter().map(|h| {
        matches!(*h, "Start" | "End" | "Sectors" | "Size")
    }).collect();

    // Print header.
    let mut line = String::new();
    for (i, hdr) in headers.iter().enumerate() {
        if i > 0 {
            line.push(' ');
        }
        if is_numeric[i] {
            // Right-align header.
            let pad = widths[i].saturating_sub(hdr.len());
            for _ in 0..pad {
                line.push(' ');
            }
            line.push_str(hdr);
        } else {
            line.push_str(hdr);
            // Don't pad the last column.
            if i < headers.len() - 1 {
                let pad = widths[i].saturating_sub(hdr.len());
                for _ in 0..pad {
                    line.push(' ');
                }
            }
        }
    }
    println!("{}", line.trim_end());

    // Print rows.
    for row in rows {
        let mut line = String::new();
        for (i, val) in row.iter().enumerate() {
            if i > 0 {
                line.push(' ');
            }
            if i < is_numeric.len() && is_numeric[i] {
                let pad = widths[i].saturating_sub(val.len());
                for _ in 0..pad {
                    line.push(' ');
                }
                line.push_str(val);
            } else {
                line.push_str(val);
                if i < headers.len() - 1 {
                    let pad = widths.get(i).copied().unwrap_or(0).saturating_sub(val.len());
                    for _ in 0..pad {
                        line.push(' ');
                    }
                }
            }
        }
        println!("{}", line.trim_end());
    }
}

// ============================================================================
// JSON listing output
// ============================================================================

/// Print a complete JSON listing for one or more disks.
fn print_json_listing(disks: &[(DiskInfo, DiskLabel)], cfg: &Config) {
    println!("{{");
    println!("  \"partitiontable\": [");

    for (di, (disk, label)) in disks.iter().enumerate() {
        println!("    {{");
        println!("      \"device\": \"{}\",", json_escape(&disk.dev_path));
        println!("      \"size\": {},", disk.size_bytes());
        println!("      \"sectors\": {},", disk.sectors);
        println!("      \"sectorsize\": {},", disk.logical_sector_size);

        match label {
            DiskLabel::Gpt { header, partitions } => {
                println!("      \"label\": \"gpt\",");
                println!("      \"id\": \"{}\",", format_guid(&header.disk_guid));
                if cfg.extended {
                    println!("      \"firstlba\": {},", header.first_usable_lba);
                    println!("      \"lastlba\": {},", header.last_usable_lba);
                }
                println!("      \"partitions\": [");

                for (pi, part) in partitions.iter().enumerate() {
                    let part_num = pi + 1;
                    let sectors = part.sectors();
                    let size_bytes = sectors.saturating_mul(disk.logical_sector_size);

                    println!("        {{");
                    println!("          \"number\": {},", part_num);
                    println!("          \"start\": {},", part.first_lba);
                    println!("          \"end\": {},", part.last_lba);
                    println!("          \"sectors\": {},", sectors);
                    println!("          \"size\": {},", size_bytes);
                    println!("          \"type\": \"{}\",", json_escape(gpt_type_name(&part.type_guid)));
                    println!("          \"typeguid\": \"{}\",", format_guid(&part.type_guid));
                    println!("          \"uuid\": \"{}\",", format_guid(&part.unique_guid));
                    println!("          \"name\": \"{}\"", json_escape(&part.name));

                    if pi < partitions.len() - 1 {
                        println!("        }},");
                    } else {
                        println!("        }}");
                    }
                }

                println!("      ]");
            }
            DiskLabel::Mbr { partitions } => {
                println!("      \"label\": \"dos\",");
                println!("      \"partitions\": [");

                let active: Vec<(usize, &MbrPartition)> = partitions
                    .iter()
                    .enumerate()
                    .filter(|(_, p)| !p.is_empty())
                    .collect();

                for (ai, &(idx, part)) in active.iter().enumerate() {
                    let part_num = idx + 1;
                    let start = u64::from(part.lba_start);
                    let end = start + u64::from(part.lba_size) - 1;
                    let sectors = u64::from(part.lba_size);
                    let size_bytes = sectors.saturating_mul(disk.logical_sector_size);

                    println!("        {{");
                    println!("          \"number\": {},", part_num);
                    println!("          \"start\": {},", start);
                    println!("          \"end\": {},", end);
                    println!("          \"sectors\": {},", sectors);
                    println!("          \"size\": {},", size_bytes);
                    println!("          \"type\": \"{}\",", json_escape(mbr_type_name(part.type_id)));
                    println!("          \"id\": \"0x{:02x}\",", part.type_id);
                    println!("          \"bootable\": {}", part.status == 0x80);

                    if ai < active.len() - 1 {
                        println!("        }},");
                    } else {
                        println!("        }}");
                    }
                }

                println!("      ]");
            }
            DiskLabel::Unknown => {
                println!("      \"label\": \"unknown\",");
                println!("      \"partitions\": []");
            }
        }

        if di < disks.len() - 1 {
            println!("    }},");
        } else {
            println!("    }}");
        }
    }

    println!("  ]");
    println!("}}");
}

// ============================================================================
// Write operations: create, delete, change type (via syscall)
// ============================================================================

/// Build a serialized partition table buffer for writing via DISK_WRITE_PT.
///
/// The format sent to the kernel:
///   - Bytes 0-3:   command tag (1 = create, 2 = delete, 3 = change type)
///   - Bytes 4-7:   partition number (1-based)
///   - Bytes 8-15:  start LBA (for create)
///   - Bytes 16-23: size in sectors (for create)
///   - Bytes 24-39: type GUID or type byte (padded to 16 bytes)
///
/// This is a simplified command-packet interface; the kernel applies the
/// operation to the on-disk table.
const CMD_CREATE: u32 = 1;
const CMD_DELETE: u32 = 2;
const CMD_CHANGE_TYPE: u32 = 3;

fn build_cmd_packet(
    cmd: u32,
    part_num: u32,
    start_lba: u64,
    size_sectors: u64,
    type_data: &[u8; 16],
) -> Vec<u8> {
    let mut buf = vec![0u8; 64];
    write_le_u32(&mut buf, 0, cmd);
    write_le_u32(&mut buf, 4, part_num);
    write_le_u64(&mut buf, 8, start_lba);
    write_le_u64(&mut buf, 16, size_sectors);
    if let Some(dst) = buf.get_mut(24..40) {
        dst.copy_from_slice(type_data);
    }
    buf
}

/// Parse a type code string. Supports:
/// - Hex MBR types: "0x83", "0xEF", "83", "EF"
/// - Full GPT GUIDs: "C12A7328-F81F-11D2-BA4B-00A0C93EC93B"
/// - Short names: "linux", "efi", "swap", "ntfs"
///
/// Returns 16 bytes: for MBR, the type byte is in [0] and [1..] are zero.
fn parse_type_code(s: &str) -> Result<[u8; 16], String> {
    let s = s.trim();

    // Check for a full GUID (contains dashes and is 36 chars).
    if s.len() == 36 && s.chars().filter(|&c| c == '-').count() == 4 {
        let bytes: Vec<u8> = s.as_bytes().to_vec();
        if bytes.len() == 36 {
            let mut arr = [0u8; 36];
            arr.copy_from_slice(&bytes);
            return Ok(parse_guid(&arr));
        }
    }

    // Check for hex MBR type: "0x83" or "83".
    let hex_str = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")).unwrap_or(s);
    if hex_str.len() <= 2 && hex_str.chars().all(|c| c.is_ascii_hexdigit()) {
        if let Ok(val) = u8::from_str_radix(hex_str, 16) {
            let mut result = [0u8; 16];
            result[0] = val;
            return Ok(result);
        }
    }

    // Check for short names.
    match s.to_ascii_lowercase().as_str() {
        "linux" | "linux-fs" => Ok(parse_guid(b"0FC63DAF-8483-4772-8E79-3D69D8477DE4")),
        "efi" | "esp" | "efi-system" => Ok(parse_guid(b"C12A7328-F81F-11D2-BA4B-00A0C93EC93B")),
        "swap" | "linux-swap" => Ok(parse_guid(b"0657FD6D-A4AB-43C4-84E5-0933C84B4F4F")),
        "ntfs" | "windows" | "msdata" => Ok(parse_guid(b"EBD0A0A2-B9E5-4433-87C0-68B6B72699C7")),
        "lvm" | "linux-lvm" => Ok(parse_guid(b"E6D6D379-F507-44C2-A23C-238F2A3DF928")),
        "raid" | "linux-raid" => Ok(parse_guid(b"A19D880F-05FC-4D3B-A006-743F0F84911E")),
        "bios" | "bios-boot" => Ok(parse_guid(b"21686148-6449-6E6F-744E-656564454649")),
        _ => Err(format!("unknown partition type: {s}")),
    }
}

/// Create a new partition on a device.
fn cmd_new_partition(device: &str, start: u64, size: u64, type_code: &[u8; 16]) {
    let dev_cstr = c_str(device);

    // First verify the device exists and get its size.
    let disk = match find_disk_info(device) {
        Some(d) => d,
        None => {
            eprintln!("fdisk: cannot open {device}: no such device");
            process::exit(1);
        }
    };

    // Sanity checks.
    if start + size > disk.sectors {
        eprintln!(
            "fdisk: partition extends beyond disk (start {} + size {} > {} sectors)",
            start, size, disk.sectors,
        );
        process::exit(1);
    }

    // Build command packet and issue syscall.
    let packet = build_cmd_packet(CMD_CREATE, 0, start, size, type_code);

    let ret = unsafe {
        // SAFETY: dev_cstr is a valid null-terminated string, packet is a
        // valid buffer. The kernel validates all parameters.
        syscall3(
            SYS_DISK_IOCTL,
            dev_cstr.as_ptr() as u64,
            DISK_WRITE_PT,
            packet.as_ptr() as u64,
        )
    };

    if ret < 0 {
        eprintln!("fdisk: failed to create partition: {}", syscall_error_msg(ret));
        process::exit(1);
    }

    let type_name = gpt_type_name(type_code);
    let type_desc = if type_name == "unknown" {
        format!("type 0x{:02X}", type_code[0])
    } else {
        type_name.to_string()
    };

    println!(
        "Created partition: start={}, size={} sectors ({}), type={}",
        start, size, format_size(size.saturating_mul(512)), type_desc,
    );
    println!("The partition table has been altered.");
}

/// Delete a partition from a device.
fn cmd_delete_partition(device: &str, part_num: u32) {
    let dev_cstr = c_str(device);

    if find_disk_info(device).is_none() {
        eprintln!("fdisk: cannot open {device}: no such device");
        process::exit(1);
    }

    if part_num == 0 {
        eprintln!("fdisk: invalid partition number 0");
        process::exit(1);
    }

    let packet = build_cmd_packet(CMD_DELETE, part_num, 0, 0, &[0u8; 16]);

    let ret = unsafe {
        // SAFETY: dev_cstr is a valid null-terminated string, packet is a
        // valid buffer. The kernel validates all parameters.
        syscall3(
            SYS_DISK_IOCTL,
            dev_cstr.as_ptr() as u64,
            DISK_WRITE_PT,
            packet.as_ptr() as u64,
        )
    };

    if ret < 0 {
        eprintln!("fdisk: failed to delete partition {part_num}: {}", syscall_error_msg(ret));
        process::exit(1);
    }

    println!("Partition {part_num} has been deleted.");
    println!("The partition table has been altered.");
}

/// Change the type of a partition on a device.
fn cmd_change_type(device: &str, part_num: u32, type_code: &[u8; 16]) {
    let dev_cstr = c_str(device);

    if find_disk_info(device).is_none() {
        eprintln!("fdisk: cannot open {device}: no such device");
        process::exit(1);
    }

    if part_num == 0 {
        eprintln!("fdisk: invalid partition number 0");
        process::exit(1);
    }

    let packet = build_cmd_packet(CMD_CHANGE_TYPE, part_num, 0, 0, type_code);

    let ret = unsafe {
        // SAFETY: dev_cstr is a valid null-terminated string, packet is a
        // valid buffer. The kernel validates all parameters.
        syscall3(
            SYS_DISK_IOCTL,
            dev_cstr.as_ptr() as u64,
            DISK_WRITE_PT,
            packet.as_ptr() as u64,
        )
    };

    if ret < 0 {
        eprintln!("fdisk: failed to change type of partition {part_num}: {}", syscall_error_msg(ret));
        process::exit(1);
    }

    let type_name = gpt_type_name(type_code);
    let type_desc = if type_name == "unknown" {
        format!("0x{:02X}", type_code[0])
    } else {
        type_name.to_string()
    };

    println!("Changed type of partition {part_num} to '{type_desc}'.");
    println!("The partition table has been altered.");
}

// ============================================================================
// CLI argument parsing
// ============================================================================

fn parse_args() -> Config {
    let args: Vec<String> = env::args().collect();
    let mut cfg = Config::new();

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-l" | "--list" => {
                cfg.list = true;
                i += 1;
            }
            "-x" | "--extended" => {
                cfg.extended = true;
                i += 1;
            }
            "--bytes" => {
                cfg.bytes = true;
                i += 1;
            }
            "--json" => {
                cfg.json = true;
                i += 1;
            }
            "-o" | "--output" => {
                if i + 1 >= args.len() {
                    eprintln!("fdisk: -o requires a column list");
                    process::exit(1);
                }
                let spec = &args[i + 1];
                let mut cols = Vec::new();
                for name in spec.split(',') {
                    let name = name.trim();
                    if name.is_empty() {
                        continue;
                    }
                    match Column::from_str(name) {
                        Some(c) => cols.push(c),
                        None => {
                            eprintln!("fdisk: unknown column '{name}'");
                            eprintln!("Available: Device,Start,End,Sectors,Size,Id,Type,UUID,Name");
                            process::exit(1);
                        }
                    }
                }
                if cols.is_empty() {
                    eprintln!("fdisk: no columns specified");
                    process::exit(1);
                }
                cfg.columns = Some(cols);
                i += 2;
            }
            "--new" => {
                cfg.new_partition = true;
                if i + 1 < args.len() && !args[i + 1].starts_with('-') {
                    cfg.devices.push(args[i + 1].clone());
                    i += 2;
                } else {
                    i += 1;
                }
            }
            "--delete" => {
                cfg.delete_partition = true;
                if i + 1 < args.len() && !args[i + 1].starts_with('-') {
                    cfg.devices.push(args[i + 1].clone());
                    i += 2;
                } else {
                    i += 1;
                }
            }
            "--type" => {
                cfg.change_type = true;
                if i + 1 < args.len() && !args[i + 1].starts_with('-') {
                    cfg.devices.push(args[i + 1].clone());
                    i += 2;
                } else {
                    i += 1;
                }
            }
            "-n" => {
                // -n <start> <size>
                if i + 2 >= args.len() {
                    eprintln!("fdisk: -n requires <start> <size>");
                    process::exit(1);
                }
                cfg.new_start = match args[i + 1].parse::<u64>() {
                    Ok(v) => Some(v),
                    Err(_) => {
                        eprintln!("fdisk: invalid start sector '{}'", args[i + 1]);
                        process::exit(1);
                    }
                };
                cfg.new_size = match args[i + 2].parse::<u64>() {
                    Ok(v) => Some(v),
                    Err(_) => {
                        eprintln!("fdisk: invalid size '{}'", args[i + 2]);
                        process::exit(1);
                    }
                };
                i += 3;
            }
            "-d" => {
                if i + 1 >= args.len() {
                    eprintln!("fdisk: -d requires a partition number");
                    process::exit(1);
                }
                cfg.delete_num = match args[i + 1].parse::<u32>() {
                    Ok(v) => Some(v),
                    Err(_) => {
                        eprintln!("fdisk: invalid partition number '{}'", args[i + 1]);
                        process::exit(1);
                    }
                };
                i += 2;
            }
            "-t" => {
                // -t can be:
                //   - After --new: just a type code
                //   - After --type: <num> <type_code>
                if cfg.change_type {
                    if i + 2 >= args.len() {
                        eprintln!("fdisk: --type -t requires <partition_num> <type_code>");
                        process::exit(1);
                    }
                    cfg.change_type_num = match args[i + 1].parse::<u32>() {
                        Ok(v) => Some(v),
                        Err(_) => {
                            eprintln!("fdisk: invalid partition number '{}'", args[i + 1]);
                            process::exit(1);
                        }
                    };
                    cfg.type_code = Some(args[i + 2].clone());
                    i += 3;
                } else {
                    if i + 1 >= args.len() {
                        eprintln!("fdisk: -t requires a type code");
                        process::exit(1);
                    }
                    cfg.type_code = Some(args[i + 1].clone());
                    i += 2;
                }
            }
            "-h" | "--help" | "help" => {
                print_usage();
                process::exit(0);
            }
            "-V" | "--version" => {
                println!("fdisk (OurOS) 0.1.0");
                process::exit(0);
            }
            arg => {
                // Treat as a device path.
                cfg.devices.push(arg.to_string());
                i += 1;
            }
        }
    }

    cfg
}

// ============================================================================
// Normalize device path
// ============================================================================

/// Ensure a device string has the /dev/ prefix.
fn normalize_device(dev: &str) -> String {
    if dev.starts_with("/dev/") {
        dev.to_string()
    } else {
        format!("/dev/{dev}")
    }
}

// ============================================================================
// Usage / help
// ============================================================================

fn print_usage() {
    println!("OurOS Partition Table Manipulator v0.1.0");
    println!();
    println!("USAGE:");
    println!("  fdisk [options] [device...]");
    println!();
    println!("LISTING:");
    println!("  fdisk -l                          List all disk partition tables");
    println!("  fdisk -l /dev/sda                 List partition table for sda");
    println!("  fdisk -l --json                   JSON output");
    println!("  fdisk -l -x                       Extended/expert info");
    println!("  fdisk -l --bytes                  Sizes in bytes");
    println!("  fdisk -l -o Device,Start,Size     Select output columns");
    println!();
    println!("OPERATIONS:");
    println!("  fdisk --new <dev> -n <start> <size> [-t <type>]");
    println!("                                    Create a new partition");
    println!("  fdisk --delete <dev> -d <num>     Delete partition number <num>");
    println!("  fdisk --type <dev> -t <num> <type_code>");
    println!("                                    Change partition type");
    println!();
    println!("OPTIONS:");
    println!("  -l, --list          List partition tables");
    println!("  -x, --extended      Show extended/expert info (UUIDs, attributes)");
    println!("  --bytes             Show sizes in bytes");
    println!("  --json              JSON output");
    println!("  -o <columns>        Comma-separated column list:");
    println!("                        Device,Start,End,Sectors,Size,Id,Type,UUID,Name");
    println!("  -n <start> <size>   New partition start/size in sectors");
    println!("  -d <num>            Partition number to delete");
    println!("  -t <type>           Partition type (hex, GUID, or name)");
    println!("  -h, --help          Show this help");
    println!("  -V, --version       Show version");
    println!();
    println!("TYPE CODES:");
    println!("  Hex:   0x83 (Linux), 0xEF (EFI), 0x82 (swap), 0x07 (NTFS)");
    println!("  GUID:  C12A7328-F81F-11D2-BA4B-00A0C93EC93B (EFI System)");
    println!("  Name:  linux, efi, swap, ntfs, lvm, raid, bios");
    println!();
    println!("EXAMPLES:");
    println!("  fdisk -l");
    println!("  fdisk -l /dev/sda");
    println!("  fdisk --new /dev/sda -n 2048 1048576");
    println!("  fdisk --new /dev/sda -n 2048 1048576 -t efi");
    println!("  fdisk --delete /dev/sda -d 2");
    println!("  fdisk --type /dev/sda -t 3 linux");
}

// ============================================================================
// Main dispatch
// ============================================================================

fn main() {
    let cfg = parse_args();

    // If no operation flags at all, show help.
    if !cfg.list && !cfg.new_partition && !cfg.delete_partition && !cfg.change_type
        && cfg.devices.is_empty()
    {
        print_usage();
        process::exit(0);
    }

    // Handle write operations.
    if cfg.new_partition {
        let device = cfg.devices.first().unwrap_or_else(|| {
            eprintln!("fdisk: --new requires a device");
            process::exit(1);
        });
        let device = normalize_device(device);

        let start = cfg.new_start.unwrap_or_else(|| {
            eprintln!("fdisk: --new requires -n <start> <size>");
            process::exit(1);
        });
        let size = cfg.new_size.unwrap_or_else(|| {
            eprintln!("fdisk: --new requires -n <start> <size>");
            process::exit(1);
        });

        // Default type: Linux filesystem.
        let type_code = if let Some(ref tc) = cfg.type_code {
            match parse_type_code(tc) {
                Ok(code) => code,
                Err(e) => {
                    eprintln!("fdisk: {e}");
                    process::exit(1);
                }
            }
        } else {
            parse_guid(b"0FC63DAF-8483-4772-8E79-3D69D8477DE4")
        };

        cmd_new_partition(&device, start, size, &type_code);
        return;
    }

    if cfg.delete_partition {
        let device = cfg.devices.first().unwrap_or_else(|| {
            eprintln!("fdisk: --delete requires a device");
            process::exit(1);
        });
        let device = normalize_device(device);

        let part_num = cfg.delete_num.unwrap_or_else(|| {
            eprintln!("fdisk: --delete requires -d <partition_number>");
            process::exit(1);
        });

        cmd_delete_partition(&device, part_num);
        return;
    }

    if cfg.change_type {
        let device = cfg.devices.first().unwrap_or_else(|| {
            eprintln!("fdisk: --type requires a device");
            process::exit(1);
        });
        let device = normalize_device(device);

        let part_num = cfg.change_type_num.unwrap_or_else(|| {
            eprintln!("fdisk: --type requires -t <partition_number> <type_code>");
            process::exit(1);
        });
        let type_str = cfg.type_code.as_deref().unwrap_or_else(|| {
            eprintln!("fdisk: --type requires -t <partition_number> <type_code>");
            process::exit(1);
        });
        let type_code = match parse_type_code(type_str) {
            Ok(code) => code,
            Err(e) => {
                eprintln!("fdisk: {e}");
                process::exit(1);
            }
        };

        cmd_change_type(&device, part_num, &type_code);
        return;
    }

    // Listing mode (default if -l or just a device is specified).
    let devices_to_list: Vec<String> = if cfg.devices.is_empty() {
        // List all disks.
        enumerate_disks().into_iter().map(|d| d.dev_path).collect()
    } else {
        cfg.devices.iter().map(|d| normalize_device(d)).collect()
    };

    if devices_to_list.is_empty() {
        eprintln!("fdisk: no disks found (is /sys/block available?)");
        process::exit(1);
    }

    if cfg.json {
        // Collect all disk info + labels, then emit JSON.
        let mut disk_labels = Vec::new();
        for dev_path in &devices_to_list {
            let disk = match find_disk_info(dev_path) {
                Some(d) => d,
                None => {
                    eprintln!("fdisk: cannot open {dev_path}: no such device");
                    continue;
                }
            };
            let label = read_partition_table(dev_path);
            disk_labels.push((disk, label));
        }

        if disk_labels.is_empty() {
            process::exit(1);
        }

        print_json_listing(&disk_labels, &cfg);
    } else {
        let mut first = true;
        for dev_path in &devices_to_list {
            let disk = match find_disk_info(dev_path) {
                Some(d) => d,
                None => {
                    eprintln!("fdisk: cannot open {dev_path}: no such device");
                    continue;
                }
            };
            let label = read_partition_table(dev_path);

            if !first {
                println!();
            }
            first = false;

            print_disk_listing(&disk, &label, &cfg);
        }
    }
}
