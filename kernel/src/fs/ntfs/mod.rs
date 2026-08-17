//! NTFS filesystem driver (read-only).
//!
//! Implements the VFS [`FileSystem`] trait for NTFS volumes, so a Windows
//! partition can be mounted and read. Nothing here writes: NTFS's on-disk
//! consistency depends on `$LogFile` journalling that we do not implement,
//! and a driver that writes without it can leave a volume that `chkdsk`
//! cannot repair. Read-only is a deliberate boundary, not an unfinished
//! feature — see `design-decisions.md`.
//!
//! ## What is supported
//!
//! - Boot sector geometry, including the signed power-of-two size encodings
//! - The `$MFT` located through its own `$DATA` runlist, so a fragmented MFT
//!   works
//! - Update-sequence fixups on every multi-sector structure (`FILE`, `INDX`)
//! - Resident and non-resident attributes, sparse runs, and the
//!   `initialized_size` tail that reads as zeroes
//! - `$ATTRIBUTE_LIST`, so a file whose attributes overflow its MFT record is
//!   still readable
//! - Directory indexes in both forms: resident `$INDEX_ROOT` only, and
//!   `$INDEX_ROOT` plus `$INDEX_ALLOCATION` `INDX` blocks
//! - `$FILE_NAME` namespace filtering, so 8.3 aliases do not double every
//!   listing
//! - Timestamps from `$STANDARD_INFORMATION`, converted from `FILETIME`
//!
//! ## What is not
//!
//! - **Writing anything.** See above.
//! - **LZNT1-compressed data.** A compressed `$DATA` is reported as
//!   [`KernelError::NotSupported`] rather than returning the raw compressed
//!   bytes, because silently handing back compressed bytes as file contents
//!   is data corruption that looks like success.
//! - **EFS-encrypted data**, for the same reason.
//! - **Reparse points** (symlinks, junctions): reported as regular entries.
//!
//! ## Layering
//!
//! ```text
//!   NtfsFs  ──  path resolution, VFS trait
//!     │
//!     ├── index.rs   directory B+ trees ($I30)
//!     ├── record.rs  MFT records, fixups
//!     ├── attr.rs    attributes, runlists
//!     ├── boot.rs    volume geometry
//!     ├── raw.rs     bounds-checked little-endian reads
//!     └── source.rs  SectorSource: a device, or an in-RAM test image
//! ```
//!
//! Every read goes through [`SectorSource`], which is what lets `tests.rs`
//! drive the whole parser over a synthetic volume on every boot instead of
//! only when a Windows disk happens to be attached. See `source.rs`.
//!
//! ## References
//!
//! - <https://flatcap.github.io/linux-ntfs/ntfs/index.html> (the Linux-NTFS
//!   documentation project — the most complete public NTFS description)
//! - Linux `fs/ntfs3/`
//! - ntfs-3g

// Much of the module is unused until an NTFS device is actually mounted;
// the self-test exercises it, but that is not visible to dead-code analysis
// on a build where the self-test is compiled out.
#![allow(dead_code)]

pub mod attr;
pub mod boot;
pub mod index;
pub mod raw;
pub mod record;
pub mod tests;

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::error::{KernelError, KernelResult};
use crate::fs::path::{Path, PathBuf};
use crate::fs::vfs::{DirEntry, EntryType, FileMeta, FileSystem, FsInfo};
use crate::serial_println;

use attr::{
    Attribute, AttributeBody, AttributeType, DataRun, FileNameAttr, StandardInformation,
    parse_attribute_list,
};
use boot::BootSector;
use index::{DIR_INDEX_NAME, INDEX_ROOT_NODE_OFFSET, INDX_NODE_OFFSET, IndexEntry, IndexRoot};
use record::{FILE_MAGIC, FileRecord, ROOT_RECORD, apply_fixups, mft_ref_record};
use crate::fs::blocksrc::{DeviceSource, SectorSource, read_bytes};

/// Largest file we will read into memory in one call.
///
/// `read_file` returns a `Vec`, so an unbounded read of a multi-gigabyte
/// Windows hibernation file is an out-of-memory kill of whatever process
/// asked. Callers wanting more must use `read_at` and stream.
const MAX_READ_FILE_BYTES: u64 = 64 * 1024 * 1024;

/// Largest number of extension records we will follow from one
/// `$ATTRIBUTE_LIST`.
const MAX_EXTENSION_RECORDS: usize = 64;

/// Largest number of `INDX` blocks we will visit while enumerating one
/// directory, and the deepest B+ tree we will descend.
const MAX_INDEX_BLOCKS: usize = 4096;
/// Deepest index tree we will descend. NTFS trees are shallow (a directory
/// with a million entries is ~4 levels); a deeper one means a cycle.
const MAX_INDEX_DEPTH: usize = 32;

/// Largest number of path components we will resolve.
const MAX_PATH_COMPONENTS: usize = 256;

// ---------------------------------------------------------------------------
// A file, with its attributes gathered from every record that holds them
// ---------------------------------------------------------------------------

/// An MFT record plus any attributes that spilled into extension records.
///
/// The distinction from [`FileRecord`] matters: `FileRecord` is one physical
/// record, `NtfsFile` is the logical file. Code that reasons about a file's
/// `$DATA` must use this type, because on a fragmented file the `$DATA` may
/// not be in the base record at all.
pub struct NtfsFile {
    /// MFT record number of the base record.
    pub record_number: u64,
    /// Sequence number of the base record.
    pub sequence: u16,
    /// Base record flags.
    pub flags: u16,
    /// Hard link count.
    pub hard_links: u16,
    /// All attributes, base record first then extensions in list order.
    pub attributes: Vec<Attribute>,
}

impl NtfsFile {
    /// Whether the file is a directory.
    pub fn is_directory(&self) -> bool {
        self.flags & record::FLAG_DIRECTORY != 0
    }

    /// Whether the record is in use.
    pub fn is_in_use(&self) -> bool {
        self.flags & record::FLAG_IN_USE != 0
    }

    /// The first attribute of the given type and name.
    pub fn find(&self, ty: AttributeType, name: &str) -> Option<&Attribute> {
        self.attributes
            .iter()
            .find(|a| a.header.attr_type == ty && a.header.name == name)
    }

    /// All attributes of the given type and name.
    pub fn find_all(&self, ty: AttributeType, name: &str) -> Vec<&Attribute> {
        self.attributes
            .iter()
            .filter(|a| a.header.attr_type == ty && a.header.name == name)
            .collect()
    }

    /// The file's `$STANDARD_INFORMATION`, if present.
    pub fn standard_information(&self) -> Option<StandardInformation> {
        let attr = self.find(AttributeType::STANDARD_INFORMATION, "")?;
        StandardInformation::parse(attr.resident_value()?).ok()
    }

    /// The file's preferred `$FILE_NAME` — the long name, never the 8.3
    /// alias, when both exist.
    pub fn file_name(&self) -> Option<FileNameAttr> {
        let mut fallback = None;
        for attr in self
            .attributes
            .iter()
            .filter(|a| a.header.attr_type == AttributeType::FILE_NAME)
        {
            let Some(value) = attr.resident_value() else {
                continue;
            };
            let Ok(name) = FileNameAttr::parse(value) else {
                continue;
            };
            if name.namespace.is_visible() {
                return Some(name);
            }
            fallback = Some(name);
        }
        fallback
    }
}

// ---------------------------------------------------------------------------
// The filesystem
// ---------------------------------------------------------------------------

/// A mounted NTFS volume.
pub struct NtfsFs {
    /// Where the bytes come from.
    source: Box<dyn SectorSource>,
    /// Volume geometry.
    boot: BootSector,
    /// Runlist of the `$MFT`'s own `$DATA`. Every MFT record read goes
    /// through this, which is what makes a fragmented MFT work.
    mft_runs: Vec<DataRun>,
    /// Size of the `$MFT` data stream, in bytes.
    mft_size: u64,
    /// Volume label from `$Volume`'s `$VOLUME_NAME`, if present.
    volume_label: String,
    /// NTFS version (major, minor) from `$VOLUME_INFORMATION`.
    version: (u8, u8),
    /// Number of MFT records read since mount — a cheap health signal for
    /// `debug_stats`, not a cache statistic.
    reads: u64,
}

impl NtfsFs {
    /// Open an NTFS volume from a named block device.
    ///
    /// # Errors
    ///
    /// See [`NtfsFs::open_source`].
    pub fn open(device: &str) -> KernelResult<Self> {
        Self::open_source(Box::new(DeviceSource::new(device)))
    }

    /// Open an NTFS volume from any [`SectorSource`].
    ///
    /// # Errors
    ///
    /// [`KernelError::NotSupported`] if the volume is not NTFS,
    /// [`KernelError::CorruptedData`] if its geometry or `$MFT` is
    /// unparseable, or an I/O error from the source.
    pub fn open_source(source: Box<dyn SectorSource>) -> KernelResult<Self> {
        let boot_bytes = read_bytes(source.as_ref(), 0, 512)?;
        let boot = BootSector::parse(&boot_bytes)?;

        // Bootstrap: read MFT record 0 ($MFT itself) directly at the LCN the
        // boot sector names, because we do not yet have the runlist that
        // would let us address it any other way.
        let mft_offset = boot.cluster_offset(boot.mft_lcn)?;
        let record_len =
            usize::try_from(boot.mft_record_size).map_err(|_| KernelError::CorruptedData)?;
        let mut raw = read_bytes(source.as_ref(), mft_offset, record_len)?;
        apply_fixups(&mut raw, FILE_MAGIC, boot.bytes_per_sector)?;
        let mft_record = FileRecord::parse(&raw, 0)?;

        // $MFT's unnamed $DATA is necessarily non-resident.
        let data = mft_record
            .find_attribute(AttributeType::DATA, "")
            .ok_or(KernelError::CorruptedData)?;
        let (mft_runs, mft_size) = match &data.body {
            AttributeBody::NonResident {
                runs, data_size, ..
            } => (runs.clone(), *data_size),
            AttributeBody::Resident { .. } => return Err(KernelError::CorruptedData),
        };
        if mft_runs.is_empty() || mft_size == 0 {
            return Err(KernelError::CorruptedData);
        }

        let mut fs = Self {
            source,
            boot,
            mft_runs,
            mft_size,
            volume_label: String::new(),
            version: (0, 0),
            reads: 1,
        };

        // $Volume (record 3) carries the label and the format version. A
        // volume without them is unusual but not unreadable, so failures
        // here are absorbed rather than propagated.
        if let Ok(volume) = fs.load_file(3) {
            if let Some(attr) = volume.find(AttributeType::VOLUME_NAME, "") {
                if let Some(value) = attr.resident_value() {
                    if let Some(label) = raw::utf16le_at(value, 0, value.len() / 2) {
                        fs.volume_label = label;
                    }
                }
            }
            if let Some(attr) = volume.find(AttributeType::VOLUME_INFORMATION, "") {
                if let Some(value) = attr.resident_value() {
                    let major = raw::u8_at(value, 0x08).unwrap_or(0);
                    let minor = raw::u8_at(value, 0x09).unwrap_or(0);
                    fs.version = (major, minor);
                }
            }
        }

        serial_println!(
            "[ntfs] {}: label='{}', v{}.{}, cluster={}B, mft_record={}B, {} MiB",
            fs.source.source_name(),
            fs.volume_label,
            fs.version.0,
            fs.version.1,
            fs.boot.cluster_size,
            fs.boot.mft_record_size,
            fs.boot.volume_bytes() / (1024 * 1024),
        );

        Ok(fs)
    }

    // -- MFT access ---------------------------------------------------------

    /// How many records the `$MFT` data stream can hold.
    ///
    /// This is the closest NTFS has to an inode count: every file, directory
    /// and metadata stream on the volume occupies at least one record, and
    /// the MFT only ever grows, so its current length bounds them all.
    fn record_count(&self) -> u64 {
        self.mft_size
            .checked_div(u64::from(self.boot.mft_record_size))
            .unwrap_or(0)
    }

    /// Read and parse one MFT record by number, applying fixups.
    fn read_record(&mut self, number: u64) -> KernelResult<FileRecord> {
        let record_size = u64::from(self.boot.mft_record_size);
        let offset = number
            .checked_mul(record_size)
            .ok_or(KernelError::CorruptedData)?;
        if offset >= self.mft_size {
            return Err(KernelError::NotFound);
        }

        let len = usize::try_from(record_size).map_err(|_| KernelError::CorruptedData)?;
        let mut raw = self.read_runs_range(
            &self.mft_runs.clone(),
            self.mft_size,
            self.mft_size,
            offset,
            len,
        )?;
        if raw.len() < len {
            return Err(KernelError::CorruptedData);
        }

        self.reads = self.reads.saturating_add(1);
        apply_fixups(&mut raw, FILE_MAGIC, self.boot.bytes_per_sector)?;
        FileRecord::parse(&raw, number)
    }

    /// Load a file: its base record plus every attribute that spilled into an
    /// extension record via `$ATTRIBUTE_LIST`.
    pub fn load_file(&mut self, number: u64) -> KernelResult<NtfsFile> {
        let base = self.read_record(number)?;
        if !base.is_in_use() {
            return Err(KernelError::NotFound);
        }

        let mut file = NtfsFile {
            record_number: number,
            sequence: base.sequence,
            flags: base.flags,
            hard_links: base.hard_links,
            attributes: base.attributes,
        };

        // Take the $ATTRIBUTE_LIST value before mutating `attributes`.
        let list_value = match file.find(AttributeType::ATTRIBUTE_LIST, "") {
            Some(attr) => Some(self.attribute_data(attr)?),
            None => None,
        };

        let Some(list_value) = list_value else {
            return Ok(file);
        };

        let entries = parse_attribute_list(&list_value)?;
        let mut loaded: Vec<u64> = Vec::new();

        for entry in &entries {
            let ext = mft_ref_record(entry.reference);
            if ext == number || loaded.contains(&ext) {
                continue;
            }
            if loaded.len() >= MAX_EXTENSION_RECORDS {
                // A file needing more extension records than this is either
                // corrupt or beyond what this read-only driver is for.
                return Err(KernelError::CorruptedData);
            }
            loaded.push(ext);

            let record = self.read_record(ext)?;
            // An extension record must name its base; one that does not is
            // a reference into unrelated data.
            if mft_ref_record(record.base_reference) != number {
                return Err(KernelError::CorruptedData);
            }
            for attr in record.attributes {
                if attr.header.attr_type == AttributeType::ATTRIBUTE_LIST {
                    continue;
                }
                file.attributes.push(attr);
            }
        }

        Ok(file)
    }

    // -- Attribute data -----------------------------------------------------

    /// Read an attribute's entire value, resident or not.
    fn attribute_data(&mut self, attr: &Attribute) -> KernelResult<Vec<u8>> {
        if attr.header.is_compressed() || attr.header.is_encrypted() {
            return Err(KernelError::NotSupported);
        }
        match &attr.body {
            AttributeBody::Resident { value, .. } => Ok(value.clone()),
            AttributeBody::NonResident {
                runs,
                data_size,
                initialized_size,
                ..
            } => {
                let len = usize::try_from(*data_size).map_err(|_| KernelError::FileTooLarge)?;
                self.read_runs_range(runs, *data_size, *initialized_size, 0, len)
            }
        }
    }

    /// Gather every attribute record of one logical stream, ordered by VCN.
    ///
    /// A large attribute can be split across several records, each covering a
    /// VCN range; only the record starting at VCN 0 carries the true sizes.
    /// Returning the pieces unsorted, or trusting a later piece's size
    /// fields, truncates the file to whatever the last fragment claims.
    fn stream_of(
        file: &NtfsFile,
        ty: AttributeType,
        name: &str,
    ) -> Option<(Vec<DataRun>, u64, u64, bool)> {
        let mut pieces: Vec<(u64, &Attribute)> = Vec::new();
        let mut resident: Option<&Attribute> = None;

        for attr in file
            .attributes
            .iter()
            .filter(|a| a.header.attr_type == ty && a.header.name == name)
        {
            match &attr.body {
                AttributeBody::Resident { .. } => resident = Some(attr),
                AttributeBody::NonResident { start_vcn, .. } => pieces.push((*start_vcn, attr)),
            }
        }

        // A resident stream has no runs; the caller reads its value directly.
        if pieces.is_empty() {
            let attr = resident?;
            let compressed = attr.header.is_compressed() || attr.header.is_encrypted();
            return Some((Vec::new(), attr.data_size(), attr.data_size(), compressed));
        }

        pieces.sort_by_key(|(vcn, _)| *vcn);

        let mut runs = Vec::new();
        let mut data_size = 0u64;
        let mut initialized = 0u64;
        let mut compressed = false;

        for (start_vcn, attr) in &pieces {
            if let AttributeBody::NonResident {
                runs: piece_runs,
                data_size: ds,
                initialized_size: is,
                ..
            } = &attr.body
            {
                if *start_vcn == 0 {
                    data_size = *ds;
                    initialized = *is;
                }
                compressed = compressed || attr.header.is_compressed() || attr.header.is_encrypted();
                runs.extend_from_slice(piece_runs);
            }
        }

        Some((runs, data_size, initialized, compressed))
    }

    /// Read a byte range from a runlist-backed stream.
    ///
    /// Handles the three ways NTFS says "these bytes are zero" without
    /// storing them: a sparse run, the region between `initialized_size` and
    /// `data_size`, and a read that runs off the end of the runlist.
    fn read_runs_range(
        &self,
        runs: &[DataRun],
        data_size: u64,
        initialized_size: u64,
        offset: u64,
        len: usize,
    ) -> KernelResult<Vec<u8>> {
        if len == 0 || offset >= data_size {
            return Ok(Vec::new());
        }

        // Clamp the request to the stream.
        let available = data_size.saturating_sub(offset);
        let len = usize::try_from(available.min(len as u64)).map_err(|_| KernelError::Overflow)?;

        let cluster_size = u64::from(self.boot.cluster_size);
        let mut out = vec![0u8; len];
        let mut done = 0usize;

        while done < len {
            let pos = offset
                .checked_add(done as u64)
                .ok_or(KernelError::Overflow)?;

            // Everything at or past initialized_size reads as zeroes, and
            // `out` is already zeroed.
            if pos >= initialized_size {
                break;
            }

            let vcn = pos
                .checked_div(cluster_size)
                .ok_or(KernelError::CorruptedData)?;
            let in_cluster = pos
                .checked_rem(cluster_size)
                .ok_or(KernelError::CorruptedData)?;

            let Some(run) = runs
                .iter()
                .find(|r| vcn >= r.vcn && vcn < r.vcn.saturating_add(r.length))
            else {
                // Past the end of the runlist. For a well-formed volume this
                // means the stream is shorter than data_size claims; the tail
                // is already zero.
                break;
            };

            // How much of this run remains from `pos`.
            let run_end_vcn = run.vcn.saturating_add(run.length);
            let run_remaining_bytes = run_end_vcn
                .saturating_sub(vcn)
                .saturating_mul(cluster_size)
                .saturating_sub(in_cluster);
            let want = u64::try_from(len.saturating_sub(done)).unwrap_or(u64::MAX);
            let init_remaining = initialized_size.saturating_sub(pos);
            let chunk = run_remaining_bytes.min(want).min(init_remaining);
            let chunk = usize::try_from(chunk).map_err(|_| KernelError::Overflow)?;
            if chunk == 0 {
                break;
            }

            match run.lcn {
                None => {
                    // Sparse: the hole is already zero in `out`.
                }
                Some(lcn) => {
                    let run_start = self.boot.cluster_offset(lcn)?;
                    let within = vcn
                        .checked_sub(run.vcn)
                        .and_then(|c| c.checked_mul(cluster_size))
                        .and_then(|b| b.checked_add(in_cluster))
                        .ok_or(KernelError::CorruptedData)?;
                    let disk_offset = run_start
                        .checked_add(within)
                        .ok_or(KernelError::CorruptedData)?;

                    let bytes = read_bytes(self.source.as_ref(), disk_offset, chunk)?;
                    let end = done.checked_add(chunk).ok_or(KernelError::Overflow)?;
                    let dst = out.get_mut(done..end).ok_or(KernelError::InternalError)?;
                    if bytes.len() != dst.len() {
                        return Err(KernelError::IoError);
                    }
                    dst.copy_from_slice(&bytes);
                }
            }

            done = done.checked_add(chunk).ok_or(KernelError::Overflow)?;
        }

        Ok(out)
    }

    /// Read a range of a file's unnamed `$DATA`.
    fn read_data_range(
        &mut self,
        file: &NtfsFile,
        offset: u64,
        len: usize,
    ) -> KernelResult<Vec<u8>> {
        let Some((runs, data_size, initialized, compressed)) =
            Self::stream_of(file, AttributeType::DATA, "")
        else {
            // A file with no $DATA at all is legal and reads as empty.
            return Ok(Vec::new());
        };

        if compressed {
            return Err(KernelError::NotSupported);
        }

        if runs.is_empty() {
            // Resident data lives in the record.
            let attr = file
                .find(AttributeType::DATA, "")
                .ok_or(KernelError::NotFound)?;
            let value = attr.resident_value().ok_or(KernelError::InternalError)?;
            let start = usize::try_from(offset).unwrap_or(usize::MAX).min(value.len());
            let end = start.saturating_add(len).min(value.len());
            return Ok(value.get(start..end).map_or_else(Vec::new, <[u8]>::to_vec));
        }

        self.read_runs_range(&runs, data_size, initialized, offset, len)
    }

    /// The logical size of a file's unnamed `$DATA`.
    fn data_size_of(file: &NtfsFile) -> u64 {
        Self::stream_of(file, AttributeType::DATA, "").map_or(0, |(_, size, _, _)| size)
    }

    // -- Directory enumeration ----------------------------------------------

    /// List a directory's children, filtered to visible names.
    ///
    /// Walks both halves of the index: the resident `$INDEX_ROOT` and, when
    /// the tree has outgrown it, the `INDX` blocks of `$INDEX_ALLOCATION`.
    pub fn list_directory(&mut self, file: &NtfsFile) -> KernelResult<Vec<IndexEntry>> {
        if !file.is_directory() {
            return Err(KernelError::NotADirectory);
        }

        let root_attr = file
            .find(AttributeType::INDEX_ROOT, DIR_INDEX_NAME)
            .ok_or(KernelError::CorruptedData)?;
        let root_value = root_attr
            .resident_value()
            .ok_or(KernelError::CorruptedData)?
            .to_vec();
        let root = IndexRoot::parse(&root_value)?;

        // The allocation stream, if the tree has one.
        let alloc = Self::stream_of(file, AttributeType::INDEX_ALLOCATION, DIR_INDEX_NAME);

        let mut out = Vec::new();
        let mut visited: Vec<u64> = Vec::new();

        let root_entries = index::parse_node(&root_value, INDEX_ROOT_NODE_OFFSET)?;
        self.walk_index_node(
            &root_entries,
            alloc.as_ref(),
            root.index_block_size,
            &mut out,
            &mut visited,
            0,
        )?;

        Ok(out)
    }

    /// Recursive half of [`NtfsFs::list_directory`].
    #[allow(clippy::too_many_arguments)] // Each argument is a distinct piece
    // of walk state; bundling them into a struct would only move the
    // argument list somewhere less visible.
    fn walk_index_node(
        &mut self,
        entries: &[IndexEntry],
        alloc: Option<&(Vec<DataRun>, u64, u64, bool)>,
        block_size: u32,
        out: &mut Vec<IndexEntry>,
        visited: &mut Vec<u64>,
        depth: usize,
    ) -> KernelResult<()> {
        if depth > MAX_INDEX_DEPTH {
            return Err(KernelError::CorruptedData);
        }

        for entry in entries {
            // Descend first: B+ tree order puts a node's subtree before the
            // entry that follows it, so descending first keeps the listing
            // in collation order.
            if let Some(vcn) = entry.child_vcn {
                let child = self.read_index_block(alloc, block_size, vcn, visited)?;
                if let Some(child_entries) = child {
                    self.walk_index_node(
                        &child_entries,
                        alloc,
                        block_size,
                        out,
                        visited,
                        depth.saturating_add(1),
                    )?;
                }
            }

            if entry.is_last {
                continue;
            }
            let Some(key) = &entry.key else {
                continue;
            };
            // Hide 8.3 aliases; see NameSpace's doc comment.
            if !key.namespace.is_visible() {
                continue;
            }
            // Hide the volume's metadata files (records 0..=15) from
            // listings the way Windows does: they are not user data, and
            // $MFT appearing in the root as a multi-gigabyte file confuses
            // every tool that walks a tree.
            if mft_ref_record(entry.reference) < 16 {
                continue;
            }
            out.push(entry.clone());
        }

        Ok(())
    }

    /// Read and parse one `INDX` block by VCN.
    fn read_index_block(
        &mut self,
        alloc: Option<&(Vec<DataRun>, u64, u64, bool)>,
        block_size: u32,
        vcn: u64,
        visited: &mut Vec<u64>,
    ) -> KernelResult<Option<Vec<IndexEntry>>> {
        let Some((runs, data_size, initialized, compressed)) = alloc else {
            // A node claims a child but the directory has no allocation
            // stream. That is corruption, but treating it as "no children"
            // keeps the rest of the directory listable.
            return Ok(None);
        };
        if *compressed {
            return Err(KernelError::NotSupported);
        }

        if visited.contains(&vcn) {
            // A cycle. Refuse rather than loop.
            return Err(KernelError::CorruptedData);
        }
        if visited.len() >= MAX_INDEX_BLOCKS {
            return Err(KernelError::CorruptedData);
        }
        visited.push(vcn);

        // The unit of an index VCN is the cluster when an index block is at
        // least one cluster, and 512 bytes otherwise. (Matches ntfs-3g's
        // `vcn_size_bits`; getting it wrong reads the right *stream* at the
        // wrong *offset*, which yields a valid-looking block from elsewhere
        // in the same directory.)
        let unit = if block_size >= self.boot.cluster_size {
            u64::from(self.boot.cluster_size)
        } else {
            512
        };
        let offset = vcn.checked_mul(unit).ok_or(KernelError::CorruptedData)?;
        let len = usize::try_from(block_size).map_err(|_| KernelError::CorruptedData)?;

        let mut raw = self.read_runs_range(runs, *data_size, *initialized, offset, len)?;
        if raw.len() < len {
            return Err(KernelError::CorruptedData);
        }

        apply_fixups(&mut raw, index::INDX_MAGIC, self.boot.bytes_per_sector)?;
        index::verify_indx(&raw, vcn)?;

        Ok(Some(index::parse_node(&raw, INDX_NODE_OFFSET)?))
    }

    // -- Path resolution ----------------------------------------------------

    /// Resolve an absolute path to an MFT record number.
    fn resolve(&mut self, path: &str) -> KernelResult<u64> {
        let mut current = ROOT_RECORD;
        let mut components = 0usize;

        for component in path.split('/') {
            if component.is_empty() || component == "." {
                continue;
            }
            components = components.saturating_add(1);
            if components > MAX_PATH_COMPONENTS {
                return Err(KernelError::TooManyLinks);
            }

            let dir = self.load_file(current)?;
            if !dir.is_directory() {
                return Err(KernelError::NotADirectory);
            }
            let entries = self.list_directory(&dir)?;
            current = lookup(&entries, component).ok_or(KernelError::NotFound)?;
        }

        Ok(current)
    }

    /// Resolve a path and load the file it names.
    fn resolve_file(&mut self, path: &str) -> KernelResult<NtfsFile> {
        let number = self.resolve(path)?;
        self.load_file(number)
    }

    /// The entry type a record presents to the VFS.
    fn entry_type_of(file: &NtfsFile) -> EntryType {
        if file.is_directory() {
            EntryType::Directory
        } else {
            EntryType::File
        }
    }
}

/// Find a child by name among a directory's index entries.
///
/// Matching is **case-sensitive first**, then falls back to a *unique*
/// case-insensitive match. The two-step rule exists because the two systems
/// disagree: this OS is case-sensitive by design (`design.txt`), while NTFS's
/// own `$I30` collation is case-insensitive, which means a Win32-namespace
/// directory cannot contain two names differing only in case — so the
/// fallback is unambiguous in practice and lets a path typed with
/// Windows-style casing resolve. Where a POSIX-namespace collision *does*
/// exist, the exact match wins and an inexact one is refused rather than
/// guessed at, because opening the wrong file is worse than not opening one.
fn lookup(entries: &[IndexEntry], name: &str) -> Option<u64> {
    for entry in entries {
        if let Some(key) = &entry.key {
            if key.name == name {
                return Some(mft_ref_record(entry.reference));
            }
        }
    }

    let mut found = None;
    for entry in entries {
        let Some(key) = &entry.key else { continue };
        if !key.name.eq_ignore_ascii_case(name) {
            continue;
        }
        if found.is_some() {
            // Ambiguous: refuse.
            return None;
        }
        found = Some(mft_ref_record(entry.reference));
    }
    found
}

/// Decode a VFS path for the NTFS driver.
///
/// NTFS names are UTF-16 on disk and are decoded to UTF-8 by the parser, so a
/// path that is not valid UTF-8 cannot name anything on the volume and
/// `NotFound` is the honest answer — the same reasoning as the ISO 9660
/// driver's `as_str`.
fn as_str(path: &Path) -> KernelResult<&str> {
    path.to_str().ok_or(KernelError::NotFound)
}

impl FileSystem for NtfsFs {
    // The `FileSystem` trait fixes the signature as `&self -> &str`;
    // narrowing this impl to `&'static str` would no longer implement it.
    #[allow(clippy::unnecessary_literal_bound)]
    fn fs_type(&self) -> &str {
        "ntfs"
    }

    fn readdir(&mut self, path: &Path) -> KernelResult<Vec<DirEntry>> {
        let path = as_str(path)?;
        let dir = self.resolve_file(path)?;
        let entries = self.list_directory(&dir)?;

        let mut out = Vec::with_capacity(entries.len());
        for entry in &entries {
            let Some(key) = &entry.key else { continue };
            let entry_type = if key.dos_flags & attr::DOS_DIRECTORY != 0 {
                EntryType::Directory
            } else {
                EntryType::File
            };
            out.push(DirEntry {
                name: PathBuf::from(key.name.as_str()),
                entry_type,
                size: key.data_size,
            });
        }
        Ok(out)
    }

    fn read_file(&mut self, path: &Path) -> KernelResult<Vec<u8>> {
        let path = as_str(path)?;
        let file = self.resolve_file(path)?;
        if file.is_directory() {
            return Err(KernelError::IsADirectory);
        }
        let size = Self::data_size_of(&file);
        if size > MAX_READ_FILE_BYTES {
            return Err(KernelError::FileTooLarge);
        }
        let len = usize::try_from(size).map_err(|_| KernelError::FileTooLarge)?;
        self.read_data_range(&file, 0, len)
    }

    fn read_at(&mut self, path: &Path, offset: u64, len: usize) -> KernelResult<Vec<u8>> {
        let path = as_str(path)?;
        let file = self.resolve_file(path)?;
        if file.is_directory() {
            return Err(KernelError::IsADirectory);
        }
        self.read_data_range(&file, offset, len)
    }

    fn stat(&mut self, path: &Path) -> KernelResult<DirEntry> {
        let path_str = as_str(path)?;
        let file = self.resolve_file(path_str)?;

        let name = path_str.rsplit('/').find(|c| !c.is_empty()).unwrap_or("/");
        let entry_type = Self::entry_type_of(&file);
        let size = if entry_type == EntryType::Directory {
            0
        } else {
            Self::data_size_of(&file)
        };

        Ok(DirEntry {
            name: PathBuf::from(name),
            entry_type,
            size,
        })
    }

    fn lstat(&mut self, path: &Path) -> KernelResult<DirEntry> {
        // No symlink support yet (reparse points are not followed), so
        // lstat and stat agree.
        self.stat(path)
    }

    fn metadata(&mut self, path: &Path) -> KernelResult<FileMeta> {
        let path = as_str(path)?;
        let file = self.resolve_file(path)?;

        let entry_type = Self::entry_type_of(&file);
        let size = if entry_type == EntryType::Directory {
            0
        } else {
            Self::data_size_of(&file)
        };

        let info = file.standard_information().unwrap_or_default();

        // NTFS has no Unix mode. Report the read-only permissions that match
        // the driver's actual capability rather than inventing 0o644: a
        // writable mode on a filesystem that refuses every write is a lie
        // that userspace will act on.
        //
        // The per-file DOS read-only bit (`attr::DOS_READONLY`) deliberately
        // does not enter into this. While the whole mount refuses writes,
        // every file is read-only regardless of what its attribute says, and
        // reporting a *narrower* mode for the flagged ones would suggest the
        // unflagged ones are writable.
        let permissions = if entry_type == EntryType::Directory {
            0o555
        } else {
            0o444
        };

        let blocks = size
            .checked_add(511)
            .and_then(|v| v.checked_div(512))
            .unwrap_or(0);

        Ok(FileMeta {
            size,
            entry_type,
            ino: file.record_number,
            created_ns: info.created_ns,
            modified_ns: info.modified_ns,
            accessed_ns: info.accessed_ns,
            changed_ns: info.changed_ns,
            permissions,
            nlinks: u32::from(file.hard_links.max(1)),
            blocks,
            ..FileMeta::minimal(entry_type, size)
        })
    }

    fn statvfs(&mut self) -> KernelResult<FsInfo> {
        Ok(FsInfo {
            fs_type: String::from("ntfs"),
            volume_label: self.volume_label.clone(),
            block_size: u64::from(self.boot.cluster_size),
            total_blocks: self
                .boot
                .total_sectors
                .saturating_mul(u64::from(self.boot.bytes_per_sector))
                .checked_div(u64::from(self.boot.cluster_size))
                .unwrap_or(0),
            // Free space would require reading and counting $Bitmap, which
            // for a large volume is a multi-megabyte read on every statvfs.
            // Reporting 0 free is honest for a read-only mount — nothing can
            // be written to it regardless.
            free_blocks: 0,
            total_inodes: self.record_count(),
            free_inodes: 0,
            max_name_len: 255,
            read_only: true,
        })
    }

    fn debug_stats(&self) -> String {
        alloc::format!(
            "NTFS: label='{}', v{}.{}, cluster={}B, mft_record={}B, mft={} records, reads={}",
            self.volume_label,
            self.version.0,
            self.version.1,
            self.boot.cluster_size,
            self.boot.mft_record_size,
            self.record_count(),
            self.reads,
        )
    }
}

// ---------------------------------------------------------------------------
// Mount / probe
// ---------------------------------------------------------------------------

/// Mount an NTFS volume from `device` at `mount_path`.
///
/// # Errors
///
/// Propagates [`NtfsFs::open`] and [`crate::fs::Vfs::mount`] failures.
pub fn mount(device: &str, mount_path: &str) -> KernelResult<()> {
    let fs = NtfsFs::open(device)?;
    crate::fs::Vfs::mount(mount_path, Box::new(fs))?;
    serial_println!("[ntfs] Mounted {} at {} (read-only)", device, mount_path);
    Ok(())
}

/// Whether `device` holds an NTFS volume.
pub fn probe(device: &str) -> bool {
    let source = DeviceSource::new(device);
    match read_bytes(&source, 0, 512) {
        Ok(data) => boot::looks_like_ntfs(&data),
        Err(_) => false,
    }
}

/// Run the NTFS self-tests.
///
/// # Errors
///
/// Propagates the first failing check.
pub fn self_test() -> KernelResult<()> {
    tests::self_test()
}
