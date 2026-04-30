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

use crate::blkdev::{self, SECTOR_SIZE};
use crate::error::{KernelError, KernelResult};
use crate::fs::vfs::{DirEntry, EntryType, FileSystem};

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
// FAT filesystem (FAT16 / FAT32)
// ---------------------------------------------------------------------------

/// A mounted FAT filesystem (auto-detects FAT16 or FAT32).
pub struct FatFs {
    /// The block device name in the registry.
    device_name: String,
    /// Parsed BIOS Parameter Block.
    bpb: FatBpb,
}

impl FatFs {
    /// Mount a FAT filesystem from a named block device.
    ///
    /// Reads the boot sector, validates the BPB, auto-detects FAT16 or
    /// FAT32, and returns the filesystem instance.
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

        Ok(Self {
            device_name: String::from(device_name),
            bpb,
        })
    }

    /// Check if a cluster number is valid for data access.
    fn is_valid_cluster(&self, cluster: u32) -> bool {
        self.bpb.is_valid_cluster(cluster)
    }

    /// Read the root directory entries.
    ///
    /// FAT16: reads the fixed-size root directory area.
    /// FAT32: reads the cluster chain starting at `bpb.root_cluster`.
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

    /// Read directory entries from a cluster chain (for subdirectories).
    ///
    /// Subdirectories are stored as files: their data is a chain of clusters
    /// containing 32-byte directory entries.
    #[allow(clippy::arithmetic_side_effects)]
    fn read_dir_cluster(&mut self, first_cluster: u32) -> KernelResult<Vec<FatDirEntry>> {
        let mut entries = Vec::new();
        let mut cluster = first_cluster;
        let mut iterations = 0u32;
        let max_iterations = 65536u32;

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
                        if raw.first().copied() == Some(0x00) {
                            return Ok(entries); // End of directory.
                        }
                        if let Some(entry) = FatDirEntry::parse(raw) {
                            // Skip . and .. entries.
                            if entry.name[0] != b'.' {
                                entries.push(entry);
                            }
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

            // Find the component.
            let found = entries.iter().find(|e| {
                !e.is_volume_label() && e.display_name().eq_ignore_ascii_case(&target)
            });

            match found {
                Some(entry) => {
                    if is_last {
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

    // -- Write support --

    /// Helper: write a sector to the block device.
    fn write_sector(&mut self, lba: u64, buf: &[u8; SECTOR_SIZE]) -> KernelResult<()> {
        let result = blkdev::with_device(&self.device_name, |dev| {
            dev.write_sector(lba, buf)
        });
        match result {
            Some(Ok(())) => Ok(()),
            Some(Err(e)) => Err(e),
            None => Err(KernelError::NoSuchDevice),
        }
    }

    /// Helper: read a sector from the block device.
    fn read_sector(&mut self, lba: u64, buf: &mut [u8; SECTOR_SIZE]) -> KernelResult<()> {
        let result = blkdev::with_device(&self.device_name, |dev| {
            dev.read_sector(lba, buf)
        });
        match result {
            Some(Ok(())) => Ok(()),
            Some(Err(e)) => Err(e),
            None => Err(KernelError::NoSuchDevice),
        }
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

        // Write the 32-byte directory entry.
        if let Some(entry) = sector_buf.get_mut(offset..offset + 32) {
            entry[0..11].copy_from_slice(name83);
            entry[11] = attr;
            // Zero out time/date fields (12-19).
            entry[12..20].fill(0);
            // First cluster high word (offset 20-21, FAT32; zero for FAT16).
            entry[20] = (first_cluster >> 16) as u8;
            entry[21] = (first_cluster >> 24) as u8;
            // Zero out remaining time/date fields (22-25).
            entry[22..26].fill(0);
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
}

impl FileSystem for FatFs {
    fn fs_type(&self) -> &str {
        match self.bpb.fat_type {
            FatType::Fat16 => "fat16",
            FatType::Fat32 => "fat32",
        }
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

    #[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
    fn write_file(&mut self, path: &str, data: &[u8]) -> KernelResult<()> {
        let (parent_path, filename) = split_path(path);
        let name83 = Self::to_83_name(filename).ok_or(KernelError::InvalidArgument)?;

        // Check file size limit (FAT16 max: 2 GiB, but u32 field caps at ~4 GiB).
        if data.len() > u32::MAX as usize {
            return Err(KernelError::InvalidArgument);
        }

        // Resolve the parent directory.
        let parent_cluster = self.resolve_dir_cluster(parent_path)?;

        // Find or create the directory entry in the parent.
        let (dir_lba, dir_offset, exists) = self.find_or_create_slot_in(
            parent_cluster, &name83,
        )?;

        // If overwriting, check we're not clobbering a directory and free old data.
        if exists {
            let mut sector_buf = [0u8; SECTOR_SIZE];
            self.read_sector(dir_lba, &mut sector_buf)?;
            let old_attr = sector_buf.get(dir_offset + 11).copied().unwrap_or(0);
            if old_attr & ATTR_DIRECTORY != 0 {
                return Err(KernelError::IsADirectory);
            }
            let cluster_lo = u32::from(read_u16(&sector_buf, dir_offset + 26));
            let cluster_hi = u32::from(read_u16(&sector_buf, dir_offset + 20));
            let old_cluster = (cluster_hi << 16) | cluster_lo;
            if old_cluster >= 2 {
                self.free_chain(old_cluster)?;
            }
        }

        // Write new data to clusters.
        let first_cluster = self.write_file_data(data)?;

        // Update the directory entry (archive attribute for regular files).
        self.write_dir_entry(
            dir_lba,
            dir_offset,
            &name83,
            first_cluster,
            data.len() as u32,
            0x20, // Archive attribute.
        )?;

        crate::serial_println!(
            "[fat] Wrote '{}' ({} bytes, cluster {})",
            path, data.len(), first_cluster
        );

        Ok(())
    }

    fn remove(&mut self, path: &str) -> KernelResult<()> {
        let (parent_path, filename) = split_path(path);
        let name83 = Self::to_83_name(filename).ok_or(KernelError::InvalidArgument)?;

        // Resolve the parent directory.
        let parent_cluster = self.resolve_dir_cluster(parent_path)?;

        let (dir_lba, dir_offset, exists) = self.find_or_create_slot_in(
            parent_cluster, &name83,
        )?;

        if !exists {
            return Err(KernelError::NotFound);
        }

        // Read the directory entry to check type and get the first cluster.
        let mut sector_buf = [0u8; SECTOR_SIZE];
        self.read_sector(dir_lba, &mut sector_buf)?;
        let attr = sector_buf.get(dir_offset + 11).copied().unwrap_or(0);
        if attr & ATTR_DIRECTORY != 0 {
            return Err(KernelError::IsADirectory);
        }
        let cluster_lo = u32::from(read_u16(&sector_buf, dir_offset + 26));
        let cluster_hi = u32::from(read_u16(&sector_buf, dir_offset + 20));
        let first_cluster = (cluster_hi << 16) | cluster_lo;

        // Free the cluster chain.
        if first_cluster >= 2 {
            self.free_chain(first_cluster)?;
        }

        // Mark directory entry as deleted.
        self.delete_dir_entry(dir_lba, dir_offset)?;

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
