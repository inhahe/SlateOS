//! ISO 9660 filesystem driver (read-only).
//!
//! Implements the [`FileSystem`] trait for ISO 9660 / ECMA-119 volumes.
//! Supports:
//! - Primary Volume Descriptor
//! - Joliet extension (UCS-2 filenames up to 64 chars)
//! - Rock Ridge extensions (POSIX metadata: uid, gid, permissions,
//!   timestamps, symlinks, deep directories via CL/PL/RE)
//! - Multi-extent files (files > 4 GiB spanning multiple extents)
//! - Directory record timestamps → [`FileMeta`]
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
//! - SUSP / RRIP (IEEE P1282 / P1281) for Rock Ridge

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

/// Maximum number of multi-extent records we will combine for a single
/// file.  Guard against corrupt images causing infinite loops.
const MAX_MULTI_EXTENT: usize = 4096;

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

/// Rock Ridge SUSP signatures we care about.
mod rr_sig {
    pub const RR: [u8; 2] = *b"RR";
    /// POSIX file attributes (mode, nlinks, uid, gid, serial).
    pub const PX: [u8; 2] = *b"PX";
    /// POSIX timestamps.
    pub const TF: [u8; 2] = *b"TF";
    /// Alternate name (long filename).
    pub const NM: [u8; 2] = *b"NM";
    /// Symbolic link target.
    pub const SL: [u8; 2] = *b"SL";
    /// Child link (deep directories relocated).
    pub const CL: [u8; 2] = *b"CL";
    /// Parent link (relocated directory).
    pub const PL: [u8; 2] = *b"PL";
    /// Relocated entry marker.
    pub const RE: [u8; 2] = *b"RE";
    /// System Use Sharing Protocol indicator.
    pub const SP: [u8; 2] = *b"SP";
}

// ---------------------------------------------------------------------------
// Parsed metadata from Rock Ridge extensions
// ---------------------------------------------------------------------------

/// POSIX metadata extracted from Rock Ridge PX/TF/NM/SL entries.
#[derive(Debug, Clone, Default)]
struct RockRidgeMeta {
    /// POSIX permissions (mode_t).
    mode: Option<u32>,
    /// Number of hard links.
    nlinks: Option<u32>,
    /// User ID.
    uid: Option<u32>,
    /// Group ID.
    gid: Option<u32>,
    /// Alternate name (long filename from NM entries).
    alt_name: Option<String>,
    /// Symlink target (from SL entry).
    symlink_target: Option<String>,
    /// Creation timestamp (seconds since epoch).
    create_time: Option<u64>,
    /// Modification timestamp (seconds since epoch).
    modify_time: Option<u64>,
    /// Access timestamp (seconds since epoch).
    access_time: Option<u64>,
    /// Child link LBA (CL entry — relocated directory).
    child_link_lba: Option<u32>,
    /// Whether this entry is a relocated placeholder (RE flag).
    is_relocated: bool,
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
    /// Volume creation date (seconds since epoch).
    creation_time: u64,
    /// Volume modification date (seconds since epoch).
    modification_time: u64,
}

/// Parsed directory record with all metadata.
#[derive(Debug, Clone)]
struct IsoDirectoryRecord {
    /// Extent location (LBA).
    lba: u32,
    /// Data length in bytes.
    size: u32,
    /// Directory record flags.
    flags: u8,
    /// Recording date (seconds since epoch).
    recording_time: u64,
    /// File identifier (raw).
    file_id: Vec<u8>,
    /// Rock Ridge metadata (if present).
    rr: RockRidgeMeta,
}

impl IsoDirectoryRecord {
    fn is_directory(&self) -> bool {
        (self.flags & dir_flags::DIRECTORY) != 0
    }

    fn is_multi_extent(&self) -> bool {
        (self.flags & dir_flags::MULTI_EXTENT) != 0
    }

    /// Best-effort name: Rock Ridge NM > Joliet UCS-2 > ISO 9660 8.3.
    fn display_name(&self, joliet: bool) -> String {
        // Prefer Rock Ridge alternate name.
        if let Some(ref nm) = self.rr.alt_name {
            return nm.clone();
        }

        if joliet {
            parse_ucs2_filename(&self.file_id)
        } else {
            parse_iso_filename(&self.file_id)
        }
    }

    /// Entry type considering Rock Ridge symlinks.
    fn entry_type(&self) -> EntryType {
        if self.rr.symlink_target.is_some() {
            EntryType::Symlink
        } else if self.is_directory() || self.rr.child_link_lba.is_some() {
            EntryType::Directory
        } else {
            EntryType::File
        }
    }
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
    /// Whether Rock Ridge extensions were detected.
    has_rock_ridge: bool,
    /// SUSP skip bytes (from SP entry) — bytes to skip in system use
    /// area before SUSP entries begin.
    susp_skip: u8,
    /// Use Joliet tree for path resolution (preferred when available).
    use_joliet: bool,
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

        // Detect Rock Ridge by examining the root directory's system use
        // area for an SP (SUSP indicator) entry.
        let mut has_rock_ridge = false;
        let mut susp_skip = 0u8;

        let root_data = read_extent_raw(
            device,
            pvd.root_dir_lba,
            pvd.root_dir_size,
            pvd.logical_block_size,
        )?;

        if let Some((rr, skip)) = detect_rock_ridge(&root_data) {
            has_rock_ridge = rr;
            susp_skip = skip;
        }

        // Prefer Rock Ridge over Joliet when both are present, because
        // Rock Ridge carries full POSIX metadata.  Joliet is still used
        // for filename parsing as a fallback.
        let use_joliet = has_joliet && !has_rock_ridge;

        serial_println!(
            "[iso9660] Volume: '{}', block_size={}, blocks={}, joliet={}, rock_ridge={}",
            pvd.volume_id,
            pvd.logical_block_size,
            pvd.volume_space_size,
            has_joliet,
            has_rock_ridge,
        );

        Ok(Self {
            device: String::from(device),
            pvd,
            has_joliet,
            joliet_root_lba,
            joliet_root_size,
            has_rock_ridge,
            susp_skip,
            use_joliet,
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
        let records = parse_directory_records(
            &dir_data,
            self.has_rock_ridge,
            self.susp_skip,
        );

        let joliet = self.use_joliet || self.has_joliet;
        let mut entries = Vec::new();

        for rec in &records {
            // RE entries are relocated directories — hide from readdir.
            if rec.rr.is_relocated {
                continue;
            }

            let name = rec.display_name(joliet);
            entries.push(DirEntry {
                name,
                entry_type: rec.entry_type(),
                size: u64::from(rec.size),
            });
        }

        Ok(entries)
    }

    fn read_file(&mut self, path: &str) -> KernelResult<Vec<u8>> {
        let resolved = self.resolve_path_full(path)?;
        if resolved.entry_type() == EntryType::Directory {
            return Err(KernelError::IsADirectory);
        }

        // If this is part of a multi-extent file, resolve_path_full
        // returns the first extent.  We need to collect all extents.
        let extents = self.collect_multi_extent(path, &resolved)?;

        let mut data = Vec::new();
        for (ext_lba, ext_size) in &extents {
            let chunk = self.read_extent(*ext_lba, *ext_size)?;
            data.extend_from_slice(&chunk);
        }

        Ok(data)
    }

    fn stat(&mut self, path: &str) -> KernelResult<DirEntry> {
        let resolved = self.resolve_path_full(path)?;

        let name = path.rsplit('/').next().unwrap_or(path);
        let name = if name.is_empty() {
            String::from("/")
        } else {
            resolved.display_name(self.use_joliet || self.has_joliet)
        };

        // For multi-extent files, sum all extent sizes.
        let total_size = if resolved.is_multi_extent() {
            let extents = self.collect_multi_extent(path, &resolved)?;
            extents.iter().map(|(_, s)| u64::from(*s)).sum()
        } else {
            u64::from(resolved.size)
        };

        Ok(DirEntry {
            name,
            entry_type: resolved.entry_type(),
            size: total_size,
        })
    }

    fn metadata(&mut self, path: &str) -> KernelResult<FileMeta> {
        let resolved = self.resolve_path_full(path)?;

        let entry_type = resolved.entry_type();

        // Use Rock Ridge permissions if available, otherwise default
        // read-only (r-xr-xr-x for dirs, r--r--r-- for files).
        let permissions = resolved
            .rr
            .mode
            .map(|m| (m & 0o7777) as u16)
            .unwrap_or(if entry_type == EntryType::Directory {
                0o555
            } else {
                0o444
            });

        let uid = resolved.rr.uid.unwrap_or(0);
        let gid = resolved.rr.gid.unwrap_or(0);
        let nlinks = resolved.rr.nlinks.unwrap_or(1);

        // Timestamps: prefer Rock Ridge, fall back to directory record.
        let modified_ns = resolved
            .rr
            .modify_time
            .unwrap_or(resolved.recording_time)
            .saturating_mul(1_000_000_000);
        let created_ns = resolved
            .rr
            .create_time
            .unwrap_or(resolved.recording_time)
            .saturating_mul(1_000_000_000);
        let accessed_ns = resolved
            .rr
            .access_time
            .unwrap_or(resolved.recording_time)
            .saturating_mul(1_000_000_000);

        let total_size = if resolved.is_multi_extent() {
            let extents = self.collect_multi_extent(path, &resolved)?;
            extents.iter().map(|(_, s)| u64::from(*s)).sum()
        } else {
            u64::from(resolved.size)
        };

        Ok(FileMeta {
            size: total_size,
            entry_type,
            permissions,
            uid,
            gid,
            nlinks,
            blocks: 0,
            created_ns,
            modified_ns,
            accessed_ns,
            ..FileMeta::minimal(entry_type, total_size)
        })
    }

    fn readlink(&mut self, path: &str) -> KernelResult<String> {
        let resolved = self.resolve_path_full(path)?;
        resolved
            .rr
            .symlink_target
            .ok_or(KernelError::InvalidArgument)
    }

    fn lstat(&mut self, path: &str) -> KernelResult<DirEntry> {
        // For ISO 9660, lstat == stat (we don't follow symlinks).
        self.stat(path)
    }

    fn statvfs(&mut self) -> KernelResult<FsInfo> {
        Ok(FsInfo {
            fs_type: String::from("iso9660"),
            volume_label: self.pvd.volume_id.clone(),
            block_size: 2048,
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
            "ISO 9660: volume='{}', blocks={}, joliet={}, rock_ridge={}",
            self.pvd.volume_id,
            self.pvd.volume_space_size,
            self.has_joliet,
            self.has_rock_ridge,
        )
    }
}

impl Iso9660Fs {
    /// Resolve a path to a directory's (LBA, size).
    fn resolve_dir(&self, path: &str) -> KernelResult<(u32, u32)> {
        let rec = self.resolve_path_full(path)?;
        // CL entries point to a relocated directory.
        if let Some(cl_lba) = rec.rr.child_link_lba {
            // Read the relocated directory to get its size.
            let cl_data = self.read_extent(cl_lba, ISO_SECTOR_SIZE as u32)?;
            if cl_data.len() >= 33 {
                let size = read_u32_le(&cl_data, 10);
                return Ok((cl_lba, size));
            }
            return Err(KernelError::IoError);
        }
        if !rec.is_directory() {
            return Err(KernelError::NotADirectory);
        }
        Ok((rec.lba, rec.size))
    }

    /// Resolve a path to a full directory record.
    fn resolve_path_full(&self, path: &str) -> KernelResult<IsoDirectoryRecord> {
        let path = path.strip_prefix('/').unwrap_or(path);

        // Choose root: Joliet tree when available and preferred.
        let (mut current_lba, mut current_size) = if self.use_joliet {
            (self.joliet_root_lba, self.joliet_root_size)
        } else {
            (self.pvd.root_dir_lba, self.pvd.root_dir_size)
        };

        // Return root itself for empty path.
        if path.is_empty() {
            return Ok(IsoDirectoryRecord {
                lba: current_lba,
                size: current_size,
                flags: dir_flags::DIRECTORY,
                recording_time: self.pvd.creation_time,
                file_id: Vec::new(),
                rr: RockRidgeMeta::default(),
            });
        }

        let joliet = self.use_joliet || self.has_joliet;
        let mut last_rec = None;

        for component in path.split('/') {
            if component.is_empty() || component == "." {
                continue;
            }

            let dir_data = self.read_extent(current_lba, current_size)?;
            let records = parse_directory_records(
                &dir_data,
                self.has_rock_ridge,
                self.susp_skip,
            );

            let found = records
                .into_iter()
                .find(|r| {
                    let n = r.display_name(joliet);
                    n.eq_ignore_ascii_case(component) || n == component
                })
                .ok_or(KernelError::NotFound)?;

            // Follow CL (child link) if present.
            if let Some(cl_lba) = found.rr.child_link_lba {
                let cl_data = self.read_extent(cl_lba, ISO_SECTOR_SIZE as u32)?;
                if cl_data.len() >= 33 {
                    current_lba = cl_lba;
                    current_size = read_u32_le(&cl_data, 10);
                    last_rec = Some(IsoDirectoryRecord {
                        lba: cl_lba,
                        size: current_size,
                        flags: dir_flags::DIRECTORY,
                        recording_time: found.recording_time,
                        file_id: found.file_id.clone(),
                        rr: found.rr.clone(),
                    });
                    continue;
                }
                return Err(KernelError::IoError);
            }

            current_lba = found.lba;
            current_size = found.size;
            last_rec = Some(found);
        }

        last_rec.ok_or(KernelError::NotFound)
    }

    /// Collect all extents for a multi-extent file.
    ///
    /// Multi-extent files have the MULTI_EXTENT flag set on all records
    /// except the last.  Consecutive directory records with the same name
    /// form the extent chain.
    fn collect_multi_extent(
        &self,
        path: &str,
        first: &IsoDirectoryRecord,
    ) -> KernelResult<Vec<(u32, u32)>> {
        if !first.is_multi_extent() {
            return Ok(vec![(first.lba, first.size)]);
        }

        // We need the parent directory to find all extent records.
        let parent = parent_path(path);
        let (dir_lba, dir_size) = self.resolve_dir(parent)?;
        let dir_data = self.read_extent(dir_lba, dir_size)?;

        let joliet = self.use_joliet || self.has_joliet;
        let target_name = first.display_name(joliet);

        let all_records = parse_directory_records(
            &dir_data,
            self.has_rock_ridge,
            self.susp_skip,
        );

        let mut extents = Vec::new();
        let mut found_start = false;

        for rec in &all_records {
            let name = rec.display_name(joliet);
            if name.eq_ignore_ascii_case(&target_name) || name == target_name {
                found_start = true;
                extents.push((rec.lba, rec.size));

                if !rec.is_multi_extent() {
                    break; // Last extent.
                }
                if extents.len() >= MAX_MULTI_EXTENT {
                    break; // Safety limit.
                }
            } else if found_start {
                break; // Non-matching record after our extents.
            }
        }

        if extents.is_empty() {
            extents.push((first.lba, first.size));
        }

        Ok(extents)
    }

    /// Read an extent (contiguous block range) from the device.
    fn read_extent(&self, lba: u32, size: u32) -> KernelResult<Vec<u8>> {
        read_extent_raw(
            &self.device,
            lba,
            size,
            self.pvd.logical_block_size,
        )
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
    let volume_id = data
        .get(40..72)
        .map(|b| String::from_utf8_lossy(b).trim_end().into())
        .unwrap_or_default();

    // Logical block size: bytes 128-129 (LE).
    let logical_block_size = read_u16_le(data, 128);

    // Volume space size: bytes 80-83 (LE).
    let volume_space_size = read_u32_le(data, 80);

    // Root directory record: bytes 156-189 (34 bytes).
    let root_rec = data.get(156..190).ok_or(KernelError::IoError)?;
    let root_dir_lba = read_u32_le(root_rec, 2);
    let root_dir_size = read_u32_le(root_rec, 10);

    // Volume creation date: bytes 813-829 (17-byte dec-datetime).
    let creation_time = data
        .get(813..830)
        .map(parse_dec_datetime)
        .unwrap_or(0);

    // Volume modification date: bytes 830-846.
    let modification_time = data
        .get(830..847)
        .map(parse_dec_datetime)
        .unwrap_or(0);

    Ok(PrimaryVolumeDescriptor {
        volume_id,
        logical_block_size,
        volume_space_size,
        root_dir_lba,
        root_dir_size,
        creation_time,
        modification_time,
    })
}

/// Parse directory records from raw directory data.
///
/// Returns all records except the "." and ".." entries.
fn parse_directory_records(
    data: &[u8],
    rock_ridge: bool,
    susp_skip: u8,
) -> Vec<IsoDirectoryRecord> {
    let mut records = Vec::new();
    let mut offset = 0usize;

    while offset < data.len() {
        let rec_len = *data.get(offset).unwrap_or(&0) as usize;

        if rec_len == 0 {
            // Padding to sector boundary.
            let sector_offset = offset % ISO_SECTOR_SIZE;
            if sector_offset > 0 {
                offset = offset
                    .saturating_add(ISO_SECTOR_SIZE)
                    .saturating_sub(sector_offset);
            } else {
                break;
            }
            continue;
        }

        if offset.saturating_add(rec_len) > data.len() {
            break;
        }

        if let Some(rec) = data.get(offset..offset.saturating_add(rec_len)) {
            if let Some(parsed) = parse_single_record(rec, rock_ridge, susp_skip) {
                records.push(parsed);
            }
        }

        offset = offset.saturating_add(rec_len);
    }

    records
}

/// Parse a single directory record.
///
/// Returns `None` for "." and ".." entries.
fn parse_single_record(
    rec: &[u8],
    rock_ridge: bool,
    susp_skip: u8,
) -> Option<IsoDirectoryRecord> {
    if rec.len() < 33 {
        return None;
    }

    let flags = *rec.get(25).unwrap_or(&0);
    let file_id_len = *rec.get(32).unwrap_or(&0) as usize;
    let file_id = rec
        .get(33..33usize.saturating_add(file_id_len))
        .unwrap_or(&[]);

    // Skip "." and ".." (file identifier 0x00 and 0x01).
    if file_id_len == 1
        && (file_id.first() == Some(&0x00) || file_id.first() == Some(&0x01))
    {
        return None;
    }

    if file_id_len == 0 {
        return None;
    }

    let lba = read_u32_le(rec, 2);
    let size = read_u32_le(rec, 10);
    let recording_time = rec.get(18..25).map(parse_dir_datetime).unwrap_or(0);

    // Rock Ridge: parse System Use area after the file identifier.
    let mut rr = RockRidgeMeta::default();
    if rock_ridge {
        // System Use area starts after file id, padded to even.
        let su_start = 33usize
            .saturating_add(file_id_len)
            .saturating_add(if file_id_len % 2 == 0 { 1 } else { 0 })
            .saturating_add(susp_skip as usize);

        if su_start < rec.len() {
            rr = parse_rock_ridge(&rec[su_start..]);
        }
    }

    Some(IsoDirectoryRecord {
        lba,
        size,
        flags,
        recording_time,
        file_id: file_id.to_vec(),
        rr,
    })
}

/// Detect Rock Ridge extensions by looking for an SP entry in the root
/// directory's first record's system use area.
///
/// Returns `(has_rock_ridge, susp_skip_bytes)`.
fn detect_rock_ridge(root_data: &[u8]) -> Option<(bool, u8)> {
    // The first record is "." (self).
    if root_data.len() < 34 {
        return None;
    }

    let rec_len = *root_data.first()? as usize;
    if rec_len < 34 || rec_len > root_data.len() {
        return None;
    }

    let file_id_len = *root_data.get(32)? as usize;
    let su_start = 33usize
        .saturating_add(file_id_len)
        .saturating_add(if file_id_len % 2 == 0 { 1 } else { 0 });

    if su_start >= rec_len {
        return None;
    }

    let sua = root_data.get(su_start..rec_len)?;

    // Look for SP entry (System Use Sharing Protocol).
    let mut off = 0usize;
    while off.saturating_add(4) <= sua.len() {
        let sig = sua.get(off..off.saturating_add(2))?;
        let len = *sua.get(off.saturating_add(2))? as usize;
        if len < 4 || off.saturating_add(len) > sua.len() {
            break;
        }

        if sig == rr_sig::SP.as_slice() && len >= 7 {
            // SP entry: check magic bytes 0xBE 0xEF.
            let check1 = *sua.get(off.saturating_add(4))?;
            let check2 = *sua.get(off.saturating_add(5))?;
            if check1 == 0xBE && check2 == 0xEF {
                let skip = *sua.get(off.saturating_add(6)).unwrap_or(&0);
                return Some((true, skip));
            }
        }

        // Also detect RR entry as evidence of Rock Ridge.
        if sig == rr_sig::RR.as_slice() {
            return Some((true, 0));
        }

        off = off.saturating_add(len);
    }

    None
}

/// Parse Rock Ridge extensions from a System Use area.
fn parse_rock_ridge(sua: &[u8]) -> RockRidgeMeta {
    let mut meta = RockRidgeMeta::default();
    let mut nm_parts: Vec<Vec<u8>> = Vec::new();
    let mut sl_parts: Vec<String> = Vec::new();
    let mut off = 0usize;

    while off.saturating_add(4) <= sua.len() {
        let sig = match sua.get(off..off.saturating_add(2)) {
            Some(s) => s,
            None => break,
        };
        let len = *sua.get(off.saturating_add(2)).unwrap_or(&0) as usize;

        if len < 4 || off.saturating_add(len) > sua.len() {
            break;
        }

        let entry = match sua.get(off..off.saturating_add(len)) {
            Some(e) => e,
            None => break,
        };

        // PX — POSIX file attributes.
        if sig == rr_sig::PX.as_slice() && len >= 36 {
            meta.mode = Some(read_u32_le(entry, 4));
            meta.nlinks = Some(read_u32_le(entry, 12));
            meta.uid = Some(read_u32_le(entry, 20));
            meta.gid = Some(read_u32_le(entry, 28));
        }

        // TF — Timestamps.
        if sig == rr_sig::TF.as_slice() && len >= 5 {
            let tf_flags = *entry.get(4).unwrap_or(&0);
            // Determine if timestamps are 17-byte decimal or 7-byte
            // directory record format.
            let long_form = (tf_flags & 0x80) != 0;
            let ts_len: usize = if long_form { 17 } else { 7 };

            let mut ts_off = 5usize;
            // Bit 0: creation.
            if (tf_flags & 0x01) != 0 {
                if let Some(ts) = entry.get(ts_off..ts_off.saturating_add(ts_len)) {
                    meta.create_time = Some(if long_form {
                        parse_dec_datetime(ts)
                    } else {
                        parse_dir_datetime(ts)
                    });
                }
                ts_off = ts_off.saturating_add(ts_len);
            }
            // Bit 1: modify.
            if (tf_flags & 0x02) != 0 {
                if let Some(ts) = entry.get(ts_off..ts_off.saturating_add(ts_len)) {
                    meta.modify_time = Some(if long_form {
                        parse_dec_datetime(ts)
                    } else {
                        parse_dir_datetime(ts)
                    });
                }
                ts_off = ts_off.saturating_add(ts_len);
            }
            // Bit 2: access.
            if (tf_flags & 0x04) != 0 {
                if let Some(ts) = entry.get(ts_off..ts_off.saturating_add(ts_len)) {
                    meta.access_time = Some(if long_form {
                        parse_dec_datetime(ts)
                    } else {
                        parse_dir_datetime(ts)
                    });
                }
            }
        }

        // NM — Alternate name.
        if sig == rr_sig::NM.as_slice() && len >= 5 {
            let nm_flags = *entry.get(4).unwrap_or(&0);
            // Flag bit 1 = CURRENT ("."), bit 2 = PARENT ("..").
            if nm_flags & 0x06 == 0 {
                if let Some(name_data) = entry.get(5..) {
                    nm_parts.push(name_data.to_vec());
                }
                // Bit 0 = CONTINUE — more NM entries follow.
                if nm_flags & 0x01 == 0 {
                    // Concatenate all NM fragments.
                    let full: Vec<u8> = nm_parts.iter().flat_map(|p| p.iter().copied()).collect();
                    meta.alt_name = Some(String::from_utf8_lossy(&full).into());
                    nm_parts.clear();
                }
            }
        }

        // SL — Symbolic link.
        if sig == rr_sig::SL.as_slice() && len >= 5 {
            let sl_flags = *entry.get(4).unwrap_or(&0);
            // Parse component records.
            let mut comp_off = 5usize;
            while comp_off.saturating_add(2) <= len {
                let c_flags = *entry.get(comp_off).unwrap_or(&0);
                let c_len = *entry.get(comp_off.saturating_add(1)).unwrap_or(&0) as usize;

                match c_flags & 0x0E {
                    0x02 => sl_parts.push(String::from(".")),  // CURRENT
                    0x04 => sl_parts.push(String::from("..")), // PARENT
                    0x08 => sl_parts.push(String::from("/")),  // ROOT
                    _ => {
                        // Normal component name.
                        if let Some(s) = entry.get(
                            comp_off.saturating_add(2)
                                ..comp_off.saturating_add(2).saturating_add(c_len),
                        ) {
                            sl_parts.push(String::from_utf8_lossy(s).into());
                        }
                    }
                }

                comp_off = comp_off.saturating_add(2).saturating_add(c_len);
            }

            if sl_flags & 0x01 == 0 {
                // No CONTINUE — build final path.
                meta.symlink_target = Some(sl_parts.join("/"));
                sl_parts.clear();
            }
        }

        // CL — Child link (relocated directory).
        if sig == rr_sig::CL.as_slice() && len >= 12 {
            meta.child_link_lba = Some(read_u32_le(entry, 4));
        }

        // RE — Relocated entry marker.
        if sig == rr_sig::RE.as_slice() {
            meta.is_relocated = true;
        }

        off = off.saturating_add(len);
    }

    meta
}

// ---------------------------------------------------------------------------
// Directory record list for readdir compat
// ---------------------------------------------------------------------------

/// Convert parsed records to VFS DirEntry list (used by readdir).
fn records_to_dir_entries(
    records: &[IsoDirectoryRecord],
    joliet: bool,
) -> Vec<DirEntry> {
    let mut entries = Vec::new();
    for rec in records {
        if rec.rr.is_relocated {
            continue;
        }
        let name = rec.display_name(joliet);
        entries.push(DirEntry {
            name,
            entry_type: rec.entry_type(),
            size: u64::from(rec.size),
        });
    }
    entries
}

// ---------------------------------------------------------------------------
// Filename parsing
// ---------------------------------------------------------------------------

/// Parse an ISO 9660 filename (8.3 with ";version").
///
/// Strips the version number and trailing dot.
fn parse_iso_filename(file_id: &[u8]) -> String {
    let raw = String::from_utf8_lossy(file_id);

    // Strip the version number (";1" suffix).
    let name = raw.split(';').next().unwrap_or(&raw);

    // Strip trailing dot if the file has no extension.
    let name = name.strip_suffix('.').unwrap_or(name);

    String::from(name)
}

/// Parse a Joliet UCS-2 filename.
///
/// Joliet filenames are encoded as UCS-2 big-endian (2 bytes per char).
/// Strip ";version" suffix just like ISO filenames.
fn parse_ucs2_filename(file_id: &[u8]) -> String {
    // UCS-2 BE: each character is 2 bytes.
    let mut chars = Vec::new();
    let mut i = 0usize;
    while i.saturating_add(1) < file_id.len() {
        let hi = *file_id.get(i).unwrap_or(&0) as u16;
        let lo = *file_id.get(i.saturating_add(1)).unwrap_or(&0) as u16;
        let codepoint = (hi << 8) | lo;

        // Stop at NUL or semicolon (version separator).
        if codepoint == 0 || codepoint == b';' as u16 {
            break;
        }

        // Basic UCS-2 → UTF-8 conversion.  Characters outside the BMP
        // aren't possible in UCS-2 so we just use char::from_u32.
        if let Some(c) = char::from_u32(u32::from(codepoint)) {
            chars.push(c);
        }

        i = i.saturating_add(2);
    }

    let name: String = chars.into_iter().collect();

    // Strip trailing dot (ISO convention leak).
    name.strip_suffix('.').unwrap_or(&name).into()
}

// ---------------------------------------------------------------------------
// Timestamp parsing
// ---------------------------------------------------------------------------

/// Parse a 7-byte directory record datetime to seconds since epoch.
///
/// Format: years-since-1900, month (1-12), day (1-31), hour, minute,
/// second, GMT offset in 15-minute increments (signed i8).
fn parse_dir_datetime(data: &[u8]) -> u64 {
    if data.len() < 7 {
        return 0;
    }

    let year = *data.get(0).unwrap_or(&0) as u32 + 1900;
    let month = *data.get(1).unwrap_or(&1) as u32;
    let day = *data.get(2).unwrap_or(&1) as u32;
    let hour = *data.get(3).unwrap_or(&0) as u32;
    let minute = *data.get(4).unwrap_or(&0) as u32;
    let second = *data.get(5).unwrap_or(&0) as u32;
    let gmt_offset = *data.get(6).unwrap_or(&0) as i8;

    let epoch = datetime_to_epoch(year, month, day, hour, minute, second);

    // Adjust for GMT offset (15-minute units, signed).
    let offset_secs = i64::from(gmt_offset) * 15 * 60;
    let adjusted = (epoch as i64).saturating_sub(offset_secs);
    if adjusted < 0 { 0 } else { adjusted as u64 }
}

/// Parse a 17-byte decimal datetime string to seconds since epoch.
///
/// Format: "YYYYMMDDHHMMSScc" followed by a GMT offset byte.
/// All digits are ASCII '0'-'9'; "cc" is centiseconds (ignored for
/// our purposes).
fn parse_dec_datetime(data: &[u8]) -> u64 {
    if data.len() < 17 {
        return 0;
    }

    let year = parse_ascii_num(data, 0, 4);
    let month = parse_ascii_num(data, 4, 2);
    let day = parse_ascii_num(data, 6, 2);
    let hour = parse_ascii_num(data, 8, 2);
    let minute = parse_ascii_num(data, 10, 2);
    let second = parse_ascii_num(data, 12, 2);
    let gmt_offset = *data.get(16).unwrap_or(&0) as i8;

    let epoch = datetime_to_epoch(year, month, day, hour, minute, second);

    let offset_secs = i64::from(gmt_offset) * 15 * 60;
    let adjusted = (epoch as i64).saturating_sub(offset_secs);
    if adjusted < 0 { 0 } else { adjusted as u64 }
}

/// Parse N ASCII digits at offset into a u32.
fn parse_ascii_num(data: &[u8], offset: usize, len: usize) -> u32 {
    let mut val = 0u32;
    for i in 0..len {
        let ch = *data.get(offset.saturating_add(i)).unwrap_or(&b'0');
        if ch >= b'0' && ch <= b'9' {
            val = val.saturating_mul(10).saturating_add(u32::from(ch - b'0'));
        }
    }
    val
}

/// Convert a date/time to seconds since Unix epoch (1970-01-01 00:00:00 UTC).
///
/// Uses the standard civil-to-days algorithm.
fn datetime_to_epoch(year: u32, month: u32, day: u32, hour: u32, minute: u32, second: u32) -> u64 {
    // Clamp month to valid range.
    let month = if month < 1 { 1 } else if month > 12 { 12 } else { month };

    // Days from epoch to the start of the year.
    // Use the algorithm from Howard Hinnant's date library.
    let y = if month <= 2 {
        year.wrapping_sub(1)
    } else {
        year
    };
    let m = if month <= 2 {
        month.wrapping_add(9)
    } else {
        month.wrapping_sub(3)
    };

    let era = y / 400;
    let yoe = y - era * 400;
    let doy = (153 * m + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let civil_days = era as i64 * 146097 + doe as i64 - 719468;

    if civil_days < 0 {
        return 0;
    }

    let day_seconds = civil_days as u64 * 86400;
    day_seconds
        .saturating_add(u64::from(hour) * 3600)
        .saturating_add(u64::from(minute) * 60)
        .saturating_add(u64::from(second))
}

// ---------------------------------------------------------------------------
// I/O helpers
// ---------------------------------------------------------------------------

/// Read an extent from the device (standalone, no &self needed).
fn read_extent_raw(
    device: &str,
    lba: u32,
    size: u32,
    logical_block_size: u16,
) -> KernelResult<Vec<u8>> {
    let block_size = logical_block_size as usize;
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
        let sector = read_iso_sector(device, block_lba as u32)?;

        let remaining = (size as usize).saturating_sub(data.len());
        let copy_len = remaining.min(sector.len());
        if let Some(chunk) = sector.get(..copy_len) {
            data.extend_from_slice(chunk);
        }
    }

    data.truncate(size as usize);
    Ok(data)
}

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

/// Helper: get the parent path of a given path.
fn parent_path(path: &str) -> &str {
    let trimmed = path.strip_suffix('/').unwrap_or(path);
    match trimmed.rfind('/') {
        Some(idx) if idx > 0 => &trimmed[..idx],
        Some(_) => "/",
        None => "/",
    }
}

/// Read a little-endian u16 at the given offset.
fn read_u16_le(data: &[u8], offset: usize) -> u16 {
    let b0 = *data.get(offset).unwrap_or(&0) as u16;
    let b1 = *data.get(offset.saturating_add(1)).unwrap_or(&0) as u16;
    b0 | (b1 << 8)
}

/// Read a little-endian u32 at the given offset.
fn read_u32_le(data: &[u8], offset: usize) -> u32 {
    let b0 = *data.get(offset).unwrap_or(&0) as u32;
    let b1 = *data.get(offset.saturating_add(1)).unwrap_or(&0) as u32;
    let b2 = *data.get(offset.saturating_add(2)).unwrap_or(&0) as u32;
    let b3 = *data.get(offset.saturating_add(3)).unwrap_or(&0) as u32;
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

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Self-test: verify ISO 9660 parsing.
pub fn self_test() -> KernelResult<()> {
    serial_println!("[iso9660] Running self-test...");

    // ---- Unit tests (no device needed) ----

    // Test ISO filename parsing.
    test_iso_filename_parsing();

    // Test Joliet UCS-2 filename parsing.
    test_ucs2_filename_parsing();

    // Test timestamp parsing.
    test_timestamp_parsing();

    // Test datetime_to_epoch.
    test_datetime_to_epoch();

    // Test parent_path.
    test_parent_path();

    // ---- Integration test if ISO device mounted ----
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
                    EntryType::Symlink => "LINK",
                    _ => "?   ",
                };
                serial_println!(
                    "[iso9660]     {} {:20} {} bytes",
                    type_str,
                    entry.name,
                    entry.size
                );
            }

            // Test metadata() on root.
            let meta = crate::fs::Vfs::metadata(path)?;
            serial_println!(
                "[iso9660]   Root metadata: type={:?}, perms={:o}, uid={}, gid={}",
                meta.entry_type,
                meta.permissions,
                meta.uid,
                meta.gid,
            );

            serial_println!("[iso9660] Integration test passed.");
        }
        None => {
            serial_println!("[iso9660]   No ISO 9660 filesystem mounted — skipping integration test.");
        }
    }

    serial_println!("[iso9660] Self-test passed (6 tests).");
    Ok(())
}

fn test_iso_filename_parsing() {
    // Normal 8.3 with version.
    assert_eq!(parse_iso_filename(b"README.TXT;1"), "README.TXT");
    // Directory without extension.
    assert_eq!(parse_iso_filename(b"MYDIR.;1"), "MYDIR");
    // No version.
    assert_eq!(parse_iso_filename(b"BOOT.BIN"), "BOOT.BIN");
    // Trailing dot stripped.
    assert_eq!(parse_iso_filename(b"NOEXT."), "NOEXT");
    serial_println!("[iso9660]   ISO filename parsing: ok");
}

fn test_ucs2_filename_parsing() {
    // "test.txt" in UCS-2 BE.
    let ucs2: &[u8] = &[
        0x00, b't', 0x00, b'e', 0x00, b's', 0x00, b't',
        0x00, b'.', 0x00, b't', 0x00, b'x', 0x00, b't',
    ];
    assert_eq!(parse_ucs2_filename(ucs2), "test.txt");

    // With version separator.
    let with_ver: &[u8] = &[
        0x00, b'A', 0x00, b'.', 0x00, b'B', 0x00, b';',
        0x00, b'1',
    ];
    assert_eq!(parse_ucs2_filename(with_ver), "A.B");

    // Unicode character: "café" → U+0063 U+0061 U+0066 U+00E9.
    let cafe: &[u8] = &[
        0x00, 0x63, 0x00, 0x61, 0x00, 0x66, 0x00, 0xE9,
    ];
    assert_eq!(parse_ucs2_filename(cafe), "caf\u{e9}");

    serial_println!("[iso9660]   UCS-2 filename parsing: ok");
}

fn test_timestamp_parsing() {
    // 7-byte directory record datetime: 2024-03-15 10:30:45 UTC.
    let dir_dt: [u8; 7] = [
        124,  // year-1900 = 2024-1900 = 124
        3,    // month = March
        15,   // day
        10,   // hour
        30,   // minute
        45,   // second
        0,    // GMT offset (UTC)
    ];
    let ts = parse_dir_datetime(&dir_dt);
    // 2024-03-15 10:30:45 UTC = a specific epoch value.
    assert!(ts > 1700000000, "timestamp should be after 2023");
    assert!(ts < 1800000000, "timestamp should be before 2027");

    // Test with GMT offset: +4 hours = 16 quarter-hours.
    let dir_dt_gmt: [u8; 7] = [124, 3, 15, 10, 30, 45, 16];
    let ts_gmt = parse_dir_datetime(&dir_dt_gmt);
    // Should be 4 hours earlier in UTC.
    assert_eq!(ts.saturating_sub(ts_gmt), 4 * 3600);

    serial_println!("[iso9660]   Timestamp parsing: ok");
}

fn test_datetime_to_epoch() {
    // Unix epoch itself.
    assert_eq!(datetime_to_epoch(1970, 1, 1, 0, 0, 0), 0);
    // 2000-01-01 00:00:00 UTC = 946684800.
    assert_eq!(datetime_to_epoch(2000, 1, 1, 0, 0, 0), 946684800);
    // 2024-01-01 00:00:00 UTC = 1704067200.
    assert_eq!(datetime_to_epoch(2024, 1, 1, 0, 0, 0), 1704067200);
    serial_println!("[iso9660]   datetime_to_epoch: ok");
}

fn test_parent_path() {
    assert_eq!(parent_path("/a/b/c"), "/a/b");
    assert_eq!(parent_path("/a/b"), "/a");
    assert_eq!(parent_path("/a"), "/");
    assert_eq!(parent_path("/"), "/");
    assert_eq!(parent_path("foo"), "/");
    serial_println!("[iso9660]   parent_path: ok");
}
