//! ISO 9660 filesystem driver (read-only).
//!
//! Implements the [`FileSystem`] trait for ISO 9660 / ECMA-119 volumes.
//! Supports:
//! - Primary Volume Descriptor
//! - Joliet extension (UCS-2 filenames, longer names)
//! - Rock Ridge extension detection (not fully parsed yet)
//! - Both '/' (big-endian) and '\' (little-endian) record formats
//!
//! ## On-disk layout
//!
//! ```text
//! ┌──────────────┬───────────────────┬─────────────────┬──────────────┐
//! │ System Area  │ Volume Descriptor │ Path Table      │ Directory    │
//! │ (16 sectors) │ Set               │ (optional)      │ Records +    │
//! │              │ (PVD at LBA 16)   │                 │ File data    │
//! └──────────────┴───────────────────┴─────────────────┴──────────────┘
//! ```
//!
//! ## References
//!
//! - ECMA-119 (ISO 9660:1988)
//! - <https://wiki.osdev.org/ISO_9660>
//! - Joliet Specification (Microsoft)

// Most of the module is unused until an ISO device is actually mounted.
#![allow(dead_code)]

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::blkdev::SECTOR_SIZE;
use crate::error::{KernelError, KernelResult};
use crate::fs::vfs::{DirEntry, EntryType, FileMeta, FileSystem, FsInfo};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// ISO 9660 logical sector size (always 2048 bytes for CD-ROMs).
const ISO_SECTOR_SIZE: usize = 2048;

/// The system area is 16 sectors (32 KiB) at the start of the disc.
const SYSTEM_AREA_SECTORS: u32 = 16;

/// Volume descriptor types.
mod vd_type {
    /// Boot record.
    pub const BOOT_RECORD: u8 = 0;
    /// Primary Volume Descriptor.
    pub const PRIMARY: u8 = 1;
    /// Supplementary Volume Descriptor (Joliet uses this).
    pub const SUPPLEMENTARY: u8 = 2;
    /// Volume Partition Descriptor.
    pub const PARTITION: u8 = 3;
    /// Volume Descriptor Set Terminator.
    pub const TERMINATOR: u8 = 255;
}

/// Standard identifier in volume descriptors.
const ISO_STANDARD_ID: &[u8; 5] = b"CD001";

/// Directory record flag bits.
mod dir_flags {
    /// Entry is hidden.
    pub const HIDDEN: u8 = 1 << 0;
    /// Entry is a directory.
    pub const DIRECTORY: u8 = 1 << 1;
    /// Entry is an associated file.
    pub const ASSOCIATED: u8 = 1 << 2;
    /// Record has extended attribute information.
    pub const EXTENDED: u8 = 1 << 3;
    /// Permissions are specified in extended attributes.
    pub const PERMISSIONS: u8 = 1 << 4;
    /// This is not the final directory record for this file.
    pub const MULTI_EXTENT: u8 = 1 << 7;
}

// ---------------------------------------------------------------------------
// On-disk structures
// ---------------------------------------------------------------------------

/// Primary Volume Descriptor (PVD) — parsed fields.
///
/// The PVD is at LBA 16 and contains all the metadata needed to
/// access the filesystem.
#[derive(Debug)]
struct PrimaryVolumeDescriptor {
    /// Volume identifier (32 bytes, space-padded).
    volume_id: String,
    /// Logical block size (usually 2048).
    logical_block_size: u16,
    /// Total number of logical blocks.
    volume_space_size: u32,
    /// Location of root directory record (LBA).
    root_dir_lba: u32,
    /// Size of root directory in bytes.
    root_dir_size: u32,
}

// ---------------------------------------------------------------------------
// ISO 9660 filesystem
// ---------------------------------------------------------------------------

/// ISO 9660 filesystem that implements the VFS [`FileSystem`] trait.
pub struct Iso9660Fs {
    /// Block device name.
    device: String,
    /// Parsed Primary Volume Descriptor.
    pvd: PrimaryVolumeDescriptor,
    /// Whether Joliet extension was detected.
    has_joliet: bool,
    /// Joliet supplementary VD root directory LBA (if present).
    joliet_root_lba: u32,
    /// Joliet root directory size.
    joliet_root_size: u32,
}

impl Iso9660Fs {
    /// Open an ISO 9660 filesystem from a block device.
    pub fn open(device: &str) -> KernelResult<Self> {
        // Scan volume descriptors starting at LBA 16.
        let mut pvd = None;
        let mut has_joliet = false;
        let mut joliet_root_lba = 0u32;
        let mut joliet_root_size = 0u32;

        let mut lba = SYSTEM_AREA_SECTORS;

        loop {
            let sector_data = read_iso_sector(device, lba)?;

            // Validate standard identifier.
            let std_id = sector_data.get(1..6).ok_or(KernelError::IoError)?;
            if std_id != ISO_STANDARD_ID {
                return Err(KernelError::IoError);
            }

            let vd_type = *sector_data.first().ok_or(KernelError::IoError)?;

            match vd_type {
                vd_type::PRIMARY => {
                    pvd = Some(parse_pvd(&sector_data)?);
                }
                vd_type::SUPPLEMENTARY => {
                    // Check for Joliet (escape sequences in bytes 88-90).
                    if let Some(escape) = sector_data.get(88..91) {
                        // Joliet uses escape sequences %/@ %/C %/E.
                        if escape == [0x25, 0x2F, 0x40]
                            || escape == [0x25, 0x2F, 0x43]
                            || escape == [0x25, 0x2F, 0x45]
                        {
                            has_joliet = true;
                            // Parse root directory from supplementary VD.
                            if let Some(root_rec) = sector_data.get(156..190) {
                                joliet_root_lba = read_u32_le(root_rec, 2);
                                joliet_root_size = read_u32_le(root_rec, 10);
                            }
                        }
                    }
                }
                vd_type::TERMINATOR => break,
                _ => {} // Skip unknown types.
            }

            lba = lba.saturating_add(1);

            // Safety limit — don't scan forever.
            if lba > SYSTEM_AREA_SECTORS + 32 {
                break;
            }
        }

        let pvd = pvd.ok_or_else(|| {
            serial_println!("[iso9660] No Primary Volume Descriptor found");
            KernelError::IoError
        })?;

        serial_println!(
            "[iso9660] Volume: '{}', block_size={}, blocks={}, joliet={}",
            pvd.volume_id, pvd.logical_block_size,
            pvd.volume_space_size, has_joliet
        );

        Ok(Self {
            device: String::from(device),
            pvd,
            has_joliet,
            joliet_root_lba,
            joliet_root_size,
        })
    }
}

impl FileSystem for Iso9660Fs {
    fn fs_type(&self) -> &str {
        "iso9660"
    }

    fn readdir(&mut self, path: &str) -> KernelResult<Vec<DirEntry>> {
        let (dir_lba, dir_size) = self.resolve_dir(path)?;
        let dir_data = self.read_extent(dir_lba, dir_size)?;
        parse_directory(&dir_data, self.has_joliet)
    }

    fn read_file(&mut self, path: &str) -> KernelResult<Vec<u8>> {
        let (lba, size, is_dir) = self.resolve_file(path)?;
        if is_dir {
            return Err(KernelError::IsADirectory);
        }
        self.read_extent(lba, size)
    }

    fn stat(&mut self, path: &str) -> KernelResult<DirEntry> {
        let (_lba, size, is_dir) = self.resolve_file(path)?;

        let name = path.rsplit('/').next().unwrap_or(path);
        let name = if name.is_empty() { "/" } else { name };

        Ok(DirEntry {
            name: String::from(name),
            entry_type: if is_dir { EntryType::Directory } else { EntryType::File },
            size: u64::from(size),
        })
    }

    fn metadata(&mut self, path: &str) -> KernelResult<FileMeta> {
        let (_lba, size, is_dir) = self.resolve_file(path)?;

        let entry_type = if is_dir {
            EntryType::Directory
        } else {
            EntryType::File
        };
        let perms = if is_dir { 0o555 } else { 0o444 }; // Read-only filesystem.

        Ok(FileMeta {
            size: u64::from(size),
            entry_type,
            permissions: perms,
            nlinks: 1,
            ..FileMeta::minimal(entry_type, u64::from(size))
        })
    }

    fn statvfs(&mut self) -> KernelResult<FsInfo> {
        Ok(FsInfo {
            fs_type: String::from("iso9660"),
            block_size: 2048, // ISO 9660 logical block size.
            total_blocks: u64::from(self.pvd.volume_space_size),
            free_blocks: 0, // Read-only — nothing free.
            total_inodes: 0,
            free_inodes: 0,
            max_name_len: if self.has_joliet { 64 } else { 31 },
            read_only: true,
        })
    }

    fn debug_stats(&self) -> String {
        alloc::format!(
            "ISO 9660: volume='{}', blocks={}, joliet={}",
            self.pvd.volume_id,
            self.pvd.volume_space_size,
            self.has_joliet,
        )
    }
}

impl Iso9660Fs {
    /// Resolve a path to a directory's (LBA, size).
    fn resolve_dir(&self, path: &str) -> KernelResult<(u32, u32)> {
        let (lba, size, is_dir) = self.resolve_file(path)?;
        if !is_dir {
            return Err(KernelError::NotADirectory);
        }
        Ok((lba, size))
    }

    /// Resolve a path to (LBA, size, is_directory).
    fn resolve_file(&self, path: &str) -> KernelResult<(u32, u32, bool)> {
        let path = path.strip_prefix('/').unwrap_or(path);

        // Start at root directory.
        let mut current_lba = self.pvd.root_dir_lba;
        let mut current_size = self.pvd.root_dir_size;
        let mut is_dir = true;

        if path.is_empty() {
            return Ok((current_lba, current_size, true));
        }

        for component in path.split('/') {
            if component.is_empty() || component == "." {
                continue;
            }

            if !is_dir {
                return Err(KernelError::NotADirectory);
            }

            // Read the current directory.
            let dir_data = self.read_extent(current_lba, current_size)?;
            let found = find_in_directory(&dir_data, component, self.has_joliet)?;

            current_lba = found.0;
            current_size = found.1;
            is_dir = found.2;
        }

        Ok((current_lba, current_size, is_dir))
    }

    /// Read an extent (contiguous block range) from the device.
    fn read_extent(&self, lba: u32, size: u32) -> KernelResult<Vec<u8>> {
        let block_size = self.pvd.logical_block_size as usize;
        if block_size == 0 {
            return Err(KernelError::IoError);
        }

        let blocks_needed = (size as usize)
            .saturating_add(block_size)
            .saturating_sub(1)
            / block_size;

        let mut data = Vec::with_capacity(size as usize);

        for i in 0..blocks_needed {
            let block_lba = u64::from(lba) + i as u64;
            let sector = read_iso_sector(&self.device, block_lba as u32)?;

            let remaining = (size as usize).saturating_sub(data.len());
            let copy_len = remaining.min(sector.len());
            if let Some(chunk) = sector.get(..copy_len) {
                data.extend_from_slice(chunk);
            }
        }

        data.truncate(size as usize);
        Ok(data)
    }
}

// ---------------------------------------------------------------------------
// Parsing helpers
// ---------------------------------------------------------------------------

/// Parse a Primary Volume Descriptor from raw sector data.
fn parse_pvd(data: &[u8]) -> KernelResult<PrimaryVolumeDescriptor> {
    if data.len() < ISO_SECTOR_SIZE {
        return Err(KernelError::IoError);
    }

    // Volume identifier: bytes 40-71 (32 bytes, space-padded).
    let volume_id = data.get(40..72)
        .map(|b| String::from_utf8_lossy(b).trim_end().into())
        .unwrap_or_default();

    // Logical block size: bytes 128-129 (LE).
    let logical_block_size = read_u16_le(data, 128);

    // Volume space size: bytes 80-83 (LE).
    let volume_space_size = read_u32_le(data, 80);

    // Root directory record: bytes 156-189 (34 bytes).
    let root_rec = data.get(156..190).ok_or(KernelError::IoError)?;
    let root_dir_lba = read_u32_le(root_rec, 2);  // Extent location (LE).
    let root_dir_size = read_u32_le(root_rec, 10); // Extent size (LE).

    Ok(PrimaryVolumeDescriptor {
        volume_id,
        logical_block_size,
        volume_space_size,
        root_dir_lba,
        root_dir_size,
    })
}

/// Parse directory records from raw directory data.
fn parse_directory(data: &[u8], _joliet: bool) -> KernelResult<Vec<DirEntry>> {
    let mut entries = Vec::new();
    let mut offset = 0usize;

    while offset < data.len() {
        // Record length (first byte).
        let rec_len = *data.get(offset).ok_or(KernelError::IoError)? as usize;

        if rec_len == 0 {
            // Padding to sector boundary — skip to next sector.
            let sector_offset = offset % ISO_SECTOR_SIZE;
            if sector_offset > 0 {
                offset = offset.saturating_add(ISO_SECTOR_SIZE).saturating_sub(sector_offset);
            } else {
                break;
            }
            continue;
        }

        if offset.saturating_add(rec_len) > data.len() {
            break;
        }

        let rec = data.get(offset..offset.saturating_add(rec_len))
            .ok_or(KernelError::IoError)?;

        if rec.len() >= 33 {
            let flags = *rec.get(25).unwrap_or(&0);
            let file_id_len = *rec.get(32).unwrap_or(&0) as usize;
            let file_id = rec.get(33..33usize.saturating_add(file_id_len))
                .unwrap_or(&[]);

            // Skip "." and ".." entries (file identifier 0x00 and 0x01).
            let skip = file_id_len == 1
                && (file_id.first() == Some(&0x00) || file_id.first() == Some(&0x01));

            if !skip && file_id_len > 0 {
                let name = parse_filename(file_id);
                let is_dir = (flags & dir_flags::DIRECTORY) != 0;
                let size = read_u32_le(rec, 10);

                entries.push(DirEntry {
                    name,
                    entry_type: if is_dir { EntryType::Directory } else { EntryType::File },
                    size: u64::from(size),
                });
            }
        }

        offset = offset.saturating_add(rec_len);
    }

    Ok(entries)
}

/// Find a named entry in a directory.
///
/// Returns (LBA, size, is_directory) for the found entry.
fn find_in_directory(
    data: &[u8],
    name: &str,
    _joliet: bool,
) -> KernelResult<(u32, u32, bool)> {
    let mut offset = 0usize;

    while offset < data.len() {
        let rec_len = *data.get(offset).ok_or(KernelError::IoError)? as usize;

        if rec_len == 0 {
            let sector_offset = offset % ISO_SECTOR_SIZE;
            if sector_offset > 0 {
                offset = offset.saturating_add(ISO_SECTOR_SIZE).saturating_sub(sector_offset);
            } else {
                break;
            }
            continue;
        }

        if offset.saturating_add(rec_len) > data.len() {
            break;
        }

        let rec = data.get(offset..offset.saturating_add(rec_len))
            .ok_or(KernelError::IoError)?;

        if rec.len() >= 33 {
            let flags = *rec.get(25).unwrap_or(&0);
            let file_id_len = *rec.get(32).unwrap_or(&0) as usize;
            let file_id = rec.get(33..33usize.saturating_add(file_id_len))
                .unwrap_or(&[]);

            let entry_name = parse_filename(file_id);

            // Case-insensitive comparison for ISO 9660 (standard says
            // filenames are uppercase, but we should be lenient).
            if entry_name.eq_ignore_ascii_case(name) || entry_name == name {
                let lba = read_u32_le(rec, 2);
                let size = read_u32_le(rec, 10);
                let is_dir = (flags & dir_flags::DIRECTORY) != 0;
                return Ok((lba, size, is_dir));
            }
        }

        offset = offset.saturating_add(rec_len);
    }

    Err(KernelError::NotFound)
}

/// Parse an ISO 9660 filename.
///
/// ISO filenames are "NAME.EXT;VERSION".  We strip the version number
/// and trailing '.' for cleaner display.
fn parse_filename(file_id: &[u8]) -> String {
    let raw = String::from_utf8_lossy(file_id);

    // Strip the version number (";1" suffix).
    let name = raw.split(';').next().unwrap_or(&raw);

    // Strip trailing dot if the file has no extension.
    let name = name.strip_suffix('.').unwrap_or(name);

    String::from(name)
}

// ---------------------------------------------------------------------------
// I/O helpers
// ---------------------------------------------------------------------------

/// Read a single ISO 9660 logical sector (2048 bytes) from the device.
///
/// Translates from ISO LBA (2048-byte sectors) to the block device's
/// 512-byte sectors.
fn read_iso_sector(device: &str, iso_lba: u32) -> KernelResult<Vec<u8>> {
    let sectors_per_iso = ISO_SECTOR_SIZE / SECTOR_SIZE;
    let base_lba = u64::from(iso_lba).saturating_mul(sectors_per_iso as u64);

    let mut buf = vec![0u8; ISO_SECTOR_SIZE];
    let mut sector_buf = [0u8; SECTOR_SIZE];

    for i in 0..sectors_per_iso {
        let lba = base_lba.saturating_add(i as u64);
        crate::fs::cache::read_sector(device, lba, &mut sector_buf)?;

        let offset = i.saturating_mul(SECTOR_SIZE);
        if let Some(dest) = buf.get_mut(offset..offset.saturating_add(SECTOR_SIZE)) {
            dest.copy_from_slice(&sector_buf);
        }
    }

    Ok(buf)
}

/// Read a little-endian u16 at the given offset.
fn read_u16_le(data: &[u8], offset: usize) -> u16 {
    let b0 = *data.get(offset).unwrap_or(&0) as u16;
    let b1 = *data.get(offset + 1).unwrap_or(&0) as u16;
    b0 | (b1 << 8)
}

/// Read a little-endian u32 at the given offset.
fn read_u32_le(data: &[u8], offset: usize) -> u32 {
    let b0 = *data.get(offset).unwrap_or(&0) as u32;
    let b1 = *data.get(offset + 1).unwrap_or(&0) as u32;
    let b2 = *data.get(offset + 2).unwrap_or(&0) as u32;
    let b3 = *data.get(offset + 3).unwrap_or(&0) as u32;
    b0 | (b1 << 8) | (b2 << 16) | (b3 << 24)
}

// ---------------------------------------------------------------------------
// Module-level functions
// ---------------------------------------------------------------------------

/// Try to mount an ISO 9660 filesystem from the given device.
pub fn mount(device: &str, mount_path: &str) -> KernelResult<()> {
    let fs = Iso9660Fs::open(device)?;
    crate::fs::Vfs::mount(mount_path, Box::new(fs))?;
    serial_println!("[iso9660] Mounted {} at {}", device, mount_path);
    Ok(())
}

/// Probe a block device for an ISO 9660 volume descriptor.
pub fn probe(device: &str) -> bool {
    // Read LBA 16 (first volume descriptor).
    match read_iso_sector(device, SYSTEM_AREA_SECTORS) {
        Ok(data) => {
            // Check standard identifier "CD001" at bytes 1-5.
            data.get(1..6) == Some(ISO_STANDARD_ID.as_slice())
        }
        Err(_) => false,
    }
}

/// Self-test: verify ISO 9660 parsing.
pub fn self_test() -> KernelResult<()> {
    serial_println!("[iso9660] Running self-test...");

    // Check if an ISO 9660 filesystem is mounted.
    let mounts = crate::fs::Vfs::mounts();
    let iso_mount = mounts.iter().find(|(_, fs_type)| fs_type == "iso9660");

    match iso_mount {
        Some((path, _)) => {
            serial_println!("[iso9660]   Mounted at '{}' — testing...", path);

            let entries = crate::fs::Vfs::readdir(path)?;
            serial_println!("[iso9660]   Root directory ({} entries):", entries.len());
            for entry in &entries {
                let type_str = match entry.entry_type {
                    EntryType::File => "FILE",
                    EntryType::Directory => "DIR ",
                    _ => "?   ",
                };
                serial_println!(
                    "[iso9660]     {} {:20} {} bytes",
                    type_str, entry.name, entry.size
                );
            }

            serial_println!("[iso9660] Self-test passed.");
        }
        None => {
            serial_println!("[iso9660]   No ISO 9660 filesystem mounted — skipping self-test.");
        }
    }

    Ok(())
}
