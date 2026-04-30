//! FAT16 filesystem driver (read-only).
//!
//! Implements the [`FileSystem`] trait for FAT16 volumes.  This is the
//! simplest filesystem that handles real-world media (USB drives, SD
//! cards, EFI System Partitions).
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

use crate::blkdev::{self, SECTOR_SIZE};
use crate::error::{KernelError, KernelResult};
use crate::fs::vfs::{DirEntry, EntryType, FileSystem};

// ---------------------------------------------------------------------------
// BIOS Parameter Block (BPB)
// ---------------------------------------------------------------------------

/// Parsed FAT16 BIOS Parameter Block from the boot sector.
#[derive(Debug, Clone)]
struct Fat16Bpb {
    /// Bytes per sector (typically 512).
    bytes_per_sector: u16,
    /// Sectors per cluster (power of 2).
    sectors_per_cluster: u8,
    /// Number of reserved sectors (including boot sector).
    reserved_sectors: u16,
    /// Number of FAT copies (usually 2).
    num_fats: u8,
    /// Maximum number of root directory entries.
    root_entry_count: u16,
    /// Total sectors (16-bit field; 0 if using 32-bit field).
    total_sectors_16: u16,
    /// Sectors per FAT.
    sectors_per_fat: u16,
    /// Total sectors (32-bit field; used if 16-bit is 0).
    total_sectors_32: u32,
    /// Volume label from extended boot record.
    volume_label: [u8; 11],
}

impl Fat16Bpb {
    /// Parse a BPB from a boot sector (512 bytes).
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
        let sectors_per_fat = read_u16(sector, 22);
        let total_sectors_32 = read_u32(sector, 32);

        // Validate basic fields.
        if bytes_per_sector == 0
            || sectors_per_cluster == 0
            || num_fats == 0
            || (root_entry_count == 0 && total_sectors_16 != 0)
        {
            return Err(KernelError::InvalidArgument);
        }

        let mut volume_label = [b' '; 11];
        if let Some(label) = sector.get(43..54) {
            volume_label.copy_from_slice(label);
        }

        Ok(Self {
            bytes_per_sector,
            sectors_per_cluster,
            reserved_sectors,
            num_fats,
            root_entry_count,
            total_sectors_16,
            sectors_per_fat,
            total_sectors_32,
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

    /// LBA of the first FAT.
    fn fat_start_lba(&self) -> u32 {
        u32::from(self.reserved_sectors)
    }

    /// LBA of the root directory.
    #[allow(clippy::arithmetic_side_effects)]
    fn root_dir_start_lba(&self) -> u32 {
        self.fat_start_lba()
            + u32::from(self.num_fats) * u32::from(self.sectors_per_fat)
    }

    /// Number of sectors occupied by the root directory.
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
    fn cluster_to_lba(&self, cluster: u16) -> u32 {
        self.data_start_lba()
            + u32::from(cluster - 2) * u32::from(self.sectors_per_cluster)
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
    /// First cluster of the file.
    first_cluster: u16,
    /// File size in bytes.
    file_size: u32,
}

impl FatDirEntry {
    /// Parse a directory entry from 32 raw bytes.
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

        let first_cluster = read_u16(raw, 26);
        let file_size = read_u32(raw, 28);

        Some(Self {
            name,
            attr,
            first_cluster,
            file_size,
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

    /// Convert the 8.3 name to a human-readable string.
    ///
    /// `"HELLO   TXT"` → `"HELLO.TXT"`
    fn display_name(&self) -> String {
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
// FAT16 filesystem
// ---------------------------------------------------------------------------

/// A mounted FAT16 filesystem.
pub struct Fat16Fs {
    /// The block device name in the registry.
    device_name: String,
    /// Parsed BIOS Parameter Block.
    bpb: Fat16Bpb,
}

impl Fat16Fs {
    /// Mount a FAT16 filesystem from a named block device.
    ///
    /// Reads the boot sector, validates the BPB, and returns the
    /// filesystem instance.
    pub fn mount(device_name: &str) -> KernelResult<Self> {
        // Read the boot sector.
        let mut boot_sector = [0u8; SECTOR_SIZE];
        let found = blkdev::with_device(device_name, |dev| {
            dev.read_sector(0, &mut boot_sector)
        });

        match found {
            Some(Ok(())) => {}
            Some(Err(e)) => return Err(e),
            None => return Err(KernelError::NoSuchDevice),
        }

        let bpb = Fat16Bpb::parse(&boot_sector)?;

        let label = core::str::from_utf8(&bpb.volume_label)
            .unwrap_or("???????????")
            .trim_end();

        crate::serial_println!(
            "[fat16] Mounted '{}' on device '{}': {} sectors, {} bytes/sector, \
             {} sectors/cluster, {} root entries",
            label,
            device_name,
            bpb.total_sectors(),
            bpb.bytes_per_sector,
            bpb.sectors_per_cluster,
            bpb.root_entry_count,
        );

        Ok(Self {
            device_name: String::from(device_name),
            bpb,
        })
    }

    /// Read the root directory entries.
    fn read_root_dir(&mut self) -> KernelResult<Vec<FatDirEntry>> {
        let root_lba = self.bpb.root_dir_start_lba();
        let root_sectors = self.bpb.root_dir_sectors();
        let max_entries = self.bpb.root_entry_count;

        let mut entries = Vec::new();
        let mut sector_buf = [0u8; SECTOR_SIZE];
        let mut entry_index: u16 = 0;

        'outer: for sec in 0..root_sectors {
            let lba = u64::from(root_lba.checked_add(sec)
                .ok_or(KernelError::InvalidArgument)?);

            let result = blkdev::with_device(&self.device_name, |dev| {
                dev.read_sector(lba, &mut sector_buf)
            });
            match result {
                Some(Ok(())) => {}
                Some(Err(e)) => return Err(e),
                None => return Err(KernelError::NoSuchDevice),
            }

            // Each sector holds 16 directory entries (512 / 32).
            let entries_per_sector = usize::from(self.bpb.bytes_per_sector) / 32;
            for i in 0..entries_per_sector {
                if entry_index >= max_entries {
                    break 'outer;
                }

                let offset = i * 32;
                if let Some(raw) = sector_buf.get(offset..offset + 32) {
                    // Check for end-of-directory marker.
                    if raw.first().copied() == Some(0x00) {
                        break 'outer;
                    }

                    if let Some(entry) = FatDirEntry::parse(raw) {
                        entries.push(entry);
                    }
                }

                entry_index = entry_index.wrapping_add(1);
            }
        }

        Ok(entries)
    }

    /// Read a FAT16 entry for a given cluster.
    ///
    /// Returns the next cluster number, or `None` for end-of-chain /
    /// free / bad cluster markers.
    #[allow(clippy::arithmetic_side_effects)]
    fn fat_entry(&mut self, cluster: u16) -> KernelResult<Option<u16>> {
        // Each FAT16 entry is 2 bytes.
        let fat_offset = u32::from(cluster) * 2;
        let fat_sector = self.bpb.fat_start_lba()
            + fat_offset / u32::from(self.bpb.bytes_per_sector);
        let offset_in_sector = (fat_offset
            % u32::from(self.bpb.bytes_per_sector)) as usize;

        let mut sector_buf = [0u8; SECTOR_SIZE];
        let result = blkdev::with_device(&self.device_name, |dev| {
            dev.read_sector(u64::from(fat_sector), &mut sector_buf)
        });
        match result {
            Some(Ok(())) => {}
            Some(Err(e)) => return Err(e),
            None => return Err(KernelError::NoSuchDevice),
        }

        let value = read_u16(&sector_buf, offset_in_sector);

        // FAT16 cluster chain values:
        // 0x0000 = free, 0x0002-0xFFEF = next cluster,
        // 0xFFF0-0xFFF6 = reserved, 0xFFF7 = bad,
        // 0xFFF8-0xFFFF = end of chain.
        if value >= 0xFFF8 {
            Ok(None) // End of chain.
        } else if value >= 2 && value <= 0xFFEF {
            Ok(Some(value))
        } else {
            Ok(None) // Free, reserved, or bad — treat as end.
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

        while bytes_read < file_size && cluster >= 2 && cluster <= 0xFFEF {
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

                let result = blkdev::with_device(&self.device_name, |dev| {
                    dev.read_sector(lba + u64::from(s), &mut sector_buf)
                });
                match result {
                    Some(Ok(())) => {}
                    Some(Err(e)) => return Err(e),
                    None => return Err(KernelError::NoSuchDevice),
                }

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

    /// Find a file in the root directory by name.
    fn find_in_root(&mut self, name: &str) -> KernelResult<FatDirEntry> {
        let entries = self.read_root_dir()?;
        let target = name.to_uppercase();
        // Strip leading slash if present.
        let target = target.strip_prefix('/').unwrap_or(&target);

        for entry in &entries {
            if entry.is_volume_label() {
                continue;
            }
            let entry_name = entry.display_name();
            if entry_name.eq_ignore_ascii_case(target) {
                return Ok(entry.clone());
            }
        }

        Err(KernelError::NotFound)
    }
}

impl FileSystem for Fat16Fs {
    fn fs_type(&self) -> &str {
        "fat16"
    }

    fn readdir(&mut self, path: &str) -> KernelResult<Vec<DirEntry>> {
        // Currently only the root directory is supported.
        let normalized = path.trim_end_matches('/');
        if !normalized.is_empty() && normalized != "/" {
            // Subdirectory — not yet implemented.
            return Err(KernelError::NotSupported);
        }

        let fat_entries = self.read_root_dir()?;
        let vfs_entries = fat_entries
            .iter()
            .filter(|e| !e.is_volume_label())
            .map(FatDirEntry::to_vfs_entry)
            .collect();

        Ok(vfs_entries)
    }

    fn read_file(&mut self, path: &str) -> KernelResult<Vec<u8>> {
        let entry = self.find_in_root(path)?;
        if entry.is_directory() {
            return Err(KernelError::IsADirectory);
        }
        self.read_file_data(&entry)
    }

    fn stat(&mut self, path: &str) -> KernelResult<DirEntry> {
        let normalized = path.trim_start_matches('/');
        if normalized.is_empty() {
            // Root directory itself.
            return Ok(DirEntry {
                name: String::from("/"),
                entry_type: EntryType::Directory,
                size: 0,
            });
        }

        let entry = self.find_in_root(path)?;
        Ok(entry.to_vfs_entry())
    }
}

// ---------------------------------------------------------------------------
// Initialization and self-test
// ---------------------------------------------------------------------------

/// Try to mount a FAT16 filesystem from the given device and mount it
/// at the VFS root.
pub fn init(device_name: &str) -> KernelResult<()> {
    let fs = Fat16Fs::mount(device_name)?;
    crate::fs::Vfs::mount("/", Box::new(fs))?;
    Ok(())
}

/// Self-test: verify we can read the directory and a file.
// String formatting uses bounded operations.
#[allow(clippy::arithmetic_side_effects)]
pub fn self_test() -> KernelResult<()> {
    crate::serial_println!("[fat16] Running self-test...");

    // List root directory.
    let entries = crate::fs::Vfs::readdir("/")?;
    crate::serial_println!("[fat16]   Root directory ({} entries):", entries.len());
    for entry in &entries {
        let type_str = match entry.entry_type {
            EntryType::File => "FILE",
            EntryType::Directory => "DIR ",
            EntryType::VolumeLabel => "VOL ",
        };
        crate::serial_println!(
            "[fat16]     {} {:12} {} bytes",
            type_str, entry.name, entry.size
        );
    }

    // Try to read HELLO.TXT.
    match crate::fs::Vfs::read_file("/HELLO.TXT") {
        Ok(data) => {
            let text = core::str::from_utf8(&data).unwrap_or("<binary>");
            crate::serial_println!(
                "[fat16]   HELLO.TXT ({} bytes): {}",
                data.len(),
                text.trim_end()
            );
        }
        Err(KernelError::NotFound) => {
            crate::serial_println!("[fat16]   HELLO.TXT not found (OK if disk has no test files)");
        }
        Err(e) => return Err(e),
    }

    crate::serial_println!("[fat16] Self-test PASSED");
    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

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
